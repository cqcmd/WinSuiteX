//! GPU 遥测：NVML(N 卡) / ADL(A 卡) 运行时 FFI。
//!
//! 设计要点：
//! - 运行时 `LoadLibraryW` 加载 `nvml.dll` / `atiadlxx.dll`，**不**在构建期链接，
//!   因此无 NVIDIA/AMD 驱动的机器上这两个库不存在 → 加载失败 → 返回空 `Vec`，
//!   主采集流程完全不受影响（`telemetry` 仍为 `None`，不造假、不崩溃）。
//! - 与 C++ 版「tempC == 0 表示未识别」哨兵值不同，这里严格用 `Option`：
//!   任何一项读不到 / 驱动不支持就留 `None`。
//! - 全部 `unsafe` 收敛在此文件；上层 `gpu.rs` 只拿到
//!   `Vec<(名称, GpuTelemetry)>`（NVML，按名）与 `Vec<GpuTelemetry>`（ADL，按索引）。

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::ptr;

use windows::core::{PCSTR, PCWSTR};
use windows::Win32::Foundation::{FreeLibrary, HMODULE};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

use winsuitex_core::model::{GpuTelemetry, MebiBytes};
use winsuitex_core::traits::wsx_trace_enabled_pub;

// ---------- 运行时加载 helpers ----------

/// 遥测级排障打点：`WSX_TRACE=1` 时逐行打到 **stderr**（不带缓冲，进程被硬杀前已落盘）。
///
/// 为什么 GPU 遥测需要这么细的打点：NVML/ADL 都是**运行时动态加载**的第三方闭源库，
/// 开发机（VM）与服务器（BMC 集显）上它们根本不存在，代码路径从未被真实执行过。
/// 真机一旦"温度读不出来"，必须能一眼区分是下面哪一种，否则只能盲猜：
///   ① 库没加载上（非 N/A 卡）→ 预期行为，不是 bug；
///   ② 库加载了但导出缺失 → 驱动版本太老；
///   ③ 库加载了、Init 也成功，但某项 rc != 0 → 该卡/该驱动不支持这项指标；
///   ④ 全都成功但匹配不上 DXGI 显卡名 → 匹配逻辑要改。
/// 因此每个分支都把 **rc 原始返回值** 打出来，而不是只打成功/失败。
pub(crate) fn gtrace(msg: &str) {
    if wsx_trace_enabled_pub() {
        eprintln!("[gpu.tel] {}", msg);
    }
}

fn load_lib(name: &str) -> Option<HMODULE> {
    let wide: Vec<u16> = std::ffi::OsStr::new(name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: wide 以 NUL 结尾，LoadLibraryW 仅读取该缓冲区。
    let h = unsafe { LoadLibraryW(PCWSTR::from_raw(wide.as_ptr())) };
    match h {
        Ok(h) if !h.is_invalid() => Some(h),
        _ => None,
    }
}

/// 从已加载的库里取导出函数；取不到返回 `None`（调用方据此判定该库不可用）。
///
/// 返回类型固定为 `unsafe extern "system" fn() -> isize`——这正是 Windows
/// `GetProcAddress` 的语义：指向代码段的裸指针，不携带具体形参 / 返回值信息。
/// 各调用点再用 `transmute` 重解释成自己的真实签名；因为目标类型在调用点是
/// **具体**的，编译器能在编译期确认尺寸一致（fn 指针恒为 1 机器字），
/// 不会触发泛型参数尺寸未知的编译错误。
unsafe fn get_proc(lib: HMODULE, name: &[u8]) -> Option<unsafe extern "system" fn() -> isize> {
    GetProcAddress(lib, PCSTR::from_raw(name.as_ptr()))
}

// ---------- NVML (NVIDIA) ----------

type NvmlInit = unsafe extern "system" fn() -> i32;
type NvmlShutdown = unsafe extern "system" fn() -> i32;
type NvmlDeviceGetCount = unsafe extern "system" fn(*mut u32) -> i32;
type NvmlDeviceGetHandleByIndex = unsafe extern "system" fn(u32, *mut *mut c_void) -> i32;
type NvmlDeviceGetName = unsafe extern "system" fn(*mut c_void, *mut u8, u32) -> i32;
type NvmlDeviceGetTemperature = unsafe extern "system" fn(*mut c_void, u32, *mut u32) -> i32;
type NvmlDeviceGetFanSpeed = unsafe extern "system" fn(*mut c_void, *mut u32) -> i32;
type NvmlDeviceGetPowerUsage = unsafe extern "system" fn(*mut c_void, *mut u32) -> i32;
type NvmlDeviceGetUtilizationRates =
    unsafe extern "system" fn(*mut c_void, *mut NvmlUtilization) -> i32;
type NvmlDeviceGetMemoryInfo = unsafe extern "system" fn(*mut c_void, *mut NvmlMemory) -> i32;
type NvmlDeviceGetClockInfo = unsafe extern "system" fn(*mut c_void, u32, *mut u32) -> i32;

#[repr(C)]
struct NvmlUtilization {
    gpu: u32,
    memory: u32,
}

#[repr(C)]
struct NvmlMemory {
    total: u64,
    used: u64,
    free: u64,
}

const NVML_SUCCESS: i32 = 0;
const NVML_TEMPERATURE_GPU: u32 = 0;
const NVML_CLOCK_GRAPHICS: u32 = 0;

/// 枚举全部 NVIDIA GPU 的遥测，按 `nvmlDeviceGetName` 返回的名称建索引。
///
/// 无 `nvml.dll`、初始化失败、或任意单项读不到时，对应项为 `None`；
/// 整个过程失败则返回空 `Vec`，绝不 panic。
pub fn nvml_collect() -> Vec<(String, GpuTelemetry)> {
    let Some(lib) = load_lib("nvml.dll") else {
        gtrace("nvml.dll 加载失败（非 N 卡 / 未装 NVIDIA 驱动）→ 预期分支，telemetry 保持 None");
        return Vec::new();
    };
    gtrace("nvml.dll 加载成功");

    // 一次性取齐所有需要的导出；任一缺失即视为 NVML 不可用。
    macro_rules! sym {
        ($name:literal, $t:ty) => {
            match unsafe { get_proc(lib, concat!($name, "\0").as_bytes()) } {
                Some(f) => unsafe {
                    std::mem::transmute::<unsafe extern "system" fn() -> isize, $t>(f)
                },
                None => {
                    gtrace(&format!("nvml 导出缺失: {} → 放弃 NVML", $name));
                    unsafe {
                        let _ = FreeLibrary(lib);
                    }
                    return Vec::new();
                }
            }
        };
    }

    let init: NvmlInit = sym!("nvmlInit_v2", NvmlInit);
    let shutdown: NvmlShutdown = sym!("nvmlShutdown", NvmlShutdown);
    let get_count: NvmlDeviceGetCount = sym!("nvmlDeviceGetCount", NvmlDeviceGetCount);
    let get_handle: NvmlDeviceGetHandleByIndex =
        sym!("nvmlDeviceGetHandleByIndex_v2", NvmlDeviceGetHandleByIndex);
    let get_name: NvmlDeviceGetName = sym!("nvmlDeviceGetName", NvmlDeviceGetName);
    let get_temp: NvmlDeviceGetTemperature =
        sym!("nvmlDeviceGetTemperature", NvmlDeviceGetTemperature);
    let get_fan: NvmlDeviceGetFanSpeed = sym!("nvmlDeviceGetFanSpeed", NvmlDeviceGetFanSpeed);
    let get_power: NvmlDeviceGetPowerUsage =
        sym!("nvmlDeviceGetPowerUsage", NvmlDeviceGetPowerUsage);
    let get_util: NvmlDeviceGetUtilizationRates = sym!(
        "nvmlDeviceGetUtilizationRates",
        NvmlDeviceGetUtilizationRates
    );
    let get_mem: NvmlDeviceGetMemoryInfo = sym!("nvmlDeviceGetMemoryInfo", NvmlDeviceGetMemoryInfo);
    let get_clock: NvmlDeviceGetClockInfo = sym!("nvmlDeviceGetClockInfo", NvmlDeviceGetClockInfo);

    // 初始化 NVML；失败 → 释放并返回空。
    let rc = unsafe { init() };
    gtrace(&format!("nvmlInit_v2 rc={}（0 为成功）", rc));
    if rc != NVML_SUCCESS {
        unsafe {
            let _ = FreeLibrary(lib);
        }
        return Vec::new();
    }

    let mut count: u32 = 0;
    let rc = unsafe { get_count(&mut count) };
    gtrace(&format!("nvmlDeviceGetCount rc={} count={}", rc, count));
    if rc != NVML_SUCCESS {
        unsafe {
            let _ = shutdown();
            let _ = FreeLibrary(lib);
        }
        return Vec::new();
    }

    let mut out: Vec<(String, GpuTelemetry)> = Vec::new();
    for i in 0..count.min(128) {
        let mut handle: *mut c_void = ptr::null_mut();
        if unsafe { get_handle(i, &mut handle) } != NVML_SUCCESS || handle.is_null() {
            continue;
        }

        // 名称（用于与 DXGI 显卡名匹配）。
        let mut buf = [0u8; 96];
        let name =
            if unsafe { get_name(handle, buf.as_mut_ptr(), buf.len() as u32) } == NVML_SUCCESS {
                let nul = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                String::from_utf8_lossy(&buf[..nul]).trim().to_string()
            } else {
                continue;
            };
        if name.is_empty() {
            continue;
        }

        let mut t = GpuTelemetry::default();

        let mut v: u32 = 0;
        if unsafe { get_temp(handle, NVML_TEMPERATURE_GPU, &mut v) } == NVML_SUCCESS && v > 0 {
            t.temp_c = Some(v as i32);
        }
        let mut v: u32 = 0;
        if unsafe { get_fan(handle, &mut v) } == NVML_SUCCESS {
            t.fan_pct = Some(v);
        }
        let mut v: u32 = 0;
        if unsafe { get_power(handle, &mut v) } == NVML_SUCCESS && v > 0 {
            t.power_w = Some(v / 1000); // 毫瓦 → 瓦
        }
        let mut u = NvmlUtilization { gpu: 0, memory: 0 };
        if unsafe { get_util(handle, &mut u) } == NVML_SUCCESS {
            t.util_pct = Some(u.gpu);
        }
        let mut m = NvmlMemory {
            total: 0,
            used: 0,
            free: 0,
        };
        if unsafe { get_mem(handle, &mut m) } == NVML_SUCCESS && m.used > 0 {
            t.mem_used_mb = Some(MebiBytes::new(m.used >> 20));
        }
        let mut v: u32 = 0;
        if unsafe { get_clock(handle, NVML_CLOCK_GRAPHICS, &mut v) } == NVML_SUCCESS && v > 0 {
            t.clock_mhz = Some(v);
        }

        // 每项都可能是 None（该卡不支持）→ 打出来才能区分"不支持"与"代码写错"。
        gtrace(&format!(
            "nvml[{}] name=\"{}\" temp={:?} fan={:?} power_w={:?} util={:?} mem_mb={:?} clock_mhz={:?}",
            i, name, t.temp_c, t.fan_pct, t.power_w, t.util_pct, t.mem_used_mb, t.clock_mhz
        ));

        out.push((name, t));
    }

    unsafe {
        let _ = shutdown();
        let _ = FreeLibrary(lib);
    }
    out
}

// ---------- ADL (AMD) ----------
//
// 注意：ADL 结构布局随 SDK 版本变化，存在 ABI 风险。本实现只用最稳定的简单调用
// （温度 / 利用率 / 风扇），且匹配按「DXGI 中 AMD 显卡的顺序」而非名称，避免触碰
// 可变长的 `ADLAdapterInfo` 结构。任意调用失败 → 该项 `None`。
// AMD 数据需在真实 A 卡机器上验证（本机无 AMD 驱动，仅验证不崩溃 / 返回空）。

type AdlAlloc = unsafe extern "system" fn(i32) -> *mut c_void;
type AdlMainControlCreate = unsafe extern "system" fn(AdlAlloc, i32, *mut *mut c_void) -> i32;
type AdlMainControlDestroy = unsafe extern "system" fn(*mut c_void) -> i32;
type AdlNumberOfAdaptersGet = unsafe extern "system" fn(*mut c_void, *mut i32) -> i32;
type AdlTempGet = unsafe extern "system" fn(*mut c_void, i32, *mut i32) -> i32;
type AdlActivityGet = unsafe extern "system" fn(*mut c_void, i32, *mut AdlPmActivity) -> i32;
type AdlFanGet = unsafe extern "system" fn(*mut c_void, i32, *mut i32, i32) -> i32;

/// `ADLPMActivity`：仅用稳定的前两个字段（iSize / iActivityPercent）。
/// 其余字段随 SDK 版本不同，这里故意 oversize（含 padding）以容纳现代 atiadlxx.dll
/// 的写入，避免栈溢出；我们只读 `i_activity_percent`（偏移固定 = 第 2 个 i32）。
#[allow(dead_code)]
#[repr(C)]
struct AdlPmActivity {
    i_size: i32,
    i_activity_percent: i32,
    _pad: [i32; 16],
}

const ADL_OD6_FAN_SPEED_TYPE_PERCENT: i32 = 1;

unsafe extern "system" fn adl_alloc(size: i32) -> *mut c_void {
    use std::alloc::{alloc, Layout};
    let layout = Layout::from_size_align(size as usize, 8).unwrap_or_else(|_| Layout::new::<u8>());
    // SAFETY: layout 大小 > 0 且对齐 8，alloc 返回可被释放/写入的裸指针。
    unsafe { alloc(layout) as *mut c_void }
}

/// 枚举全部 AMD 适配器（按索引顺序）的遥测。
/// 返回 `Vec<GpuTelemetry>`，索引与 `ADL2_Adapter_NumberOfAdapters_Get` 的适配器序号对齐，
/// 由上层 `gpu.rs` 按顺序匹配到 DXGI 中的 AMD 显卡。失败返回空 `Vec`，绝不 panic。
pub fn adl_collect() -> Vec<GpuTelemetry> {
    let Some(lib) = load_lib("atiadlxx.dll") else {
        gtrace("atiadlxx.dll 加载失败（非 A 卡 / 未装 AMD 驱动）→ 预期分支，telemetry 保持 None");
        return Vec::new();
    };
    gtrace("atiadlxx.dll 加载成功");

    macro_rules! sym {
        ($name:literal, $t:ty) => {
            match unsafe { get_proc(lib, concat!($name, "\0").as_bytes()) } {
                Some(f) => unsafe {
                    std::mem::transmute::<unsafe extern "system" fn() -> isize, $t>(f)
                },
                None => {
                    gtrace(&format!("adl 导出缺失: {} → 放弃 ADL", $name));
                    unsafe {
                        let _ = FreeLibrary(lib);
                    }
                    return Vec::new();
                }
            }
        };
    }

    let create: AdlMainControlCreate = sym!("ADL2_Main_Control_Create", AdlMainControlCreate);
    let destroy: AdlMainControlDestroy = sym!("ADL2_Main_Control_Destroy", AdlMainControlDestroy);
    let num_adapters: AdlNumberOfAdaptersGet =
        sym!("ADL2_Adapter_NumberOfAdapters_Get", AdlNumberOfAdaptersGet);
    let temp_get: AdlTempGet = sym!("ADL2_Overdrive6_Temperature_Get", AdlTempGet);
    let activity_get: AdlActivityGet = sym!("ADL2_Overdrive_CurrentActivity_Get", AdlActivityGet);
    let fan_get: AdlFanGet = sym!("ADL2_Overdrive6_FanSpeed_Get", AdlFanGet);

    let mut ctx: *mut c_void = ptr::null_mut();
    // iEnumConnectedAdapters = 1。
    // SAFETY: adl_alloc 符合 ADL_MAIN_MEMORY_ALLOC 回调约定；ctx 出参由 ADL 写入。
    let rc = unsafe { create(adl_alloc, 1, &mut ctx) };
    gtrace(&format!("ADL2_Main_Control_Create rc={}（0 为成功）", rc));
    if rc != 0 || ctx.is_null() {
        unsafe {
            let _ = FreeLibrary(lib);
        }
        return Vec::new();
    }

    let mut count: i32 = 0;
    let rc = unsafe { num_adapters(ctx, &mut count) };
    gtrace(&format!(
        "ADL2_Adapter_NumberOfAdapters_Get rc={} count={}",
        rc, count
    ));
    if rc != 0 || count <= 0 {
        unsafe {
            let _ = destroy(ctx);
            let _ = FreeLibrary(lib);
        }
        return Vec::new();
    }

    let mut out: Vec<GpuTelemetry> = Vec::with_capacity(count as usize);
    for i in 0..count.min(64) {
        let mut t = GpuTelemetry::default();

        let mut v: i32 = 0;
        // SAFETY: temp_get 写入 v；调用前 v 已初始化。
        if unsafe { temp_get(ctx, i, &mut v) } == 0 && v > 0 {
            t.temp_c = Some(v);
        }
        let mut act = AdlPmActivity {
            i_size: 0,
            i_activity_percent: 0,
            _pad: [0; 16],
        };
        act.i_size = std::mem::size_of::<AdlPmActivity>() as i32;
        // SAFETY: act 缓冲已 oversize，足以容纳 atiadlxx.dll 的写入；仅读取稳定的
        // i_activity_percent（第 2 个字段，偏移固定）。
        if unsafe { activity_get(ctx, i, &mut act) } == 0
            && (0..=100).contains(&act.i_activity_percent)
        {
            t.util_pct = Some(act.i_activity_percent as u32);
        }
        let mut v: i32 = 0;
        // SAFETY: fan_get 写入 v；第 4 参指定按百分比返回。
        if unsafe { fan_get(ctx, i, &mut v, ADL_OD6_FAN_SPEED_TYPE_PERCENT) } == 0
            && (0..=100).contains(&v)
        {
            t.fan_pct = Some(v as u32);
        }

        gtrace(&format!(
            "adl[{}] temp={:?} util={:?} fan={:?}",
            i, t.temp_c, t.util_pct, t.fan_pct
        ));

        out.push(t);
    }

    unsafe {
        let _ = destroy(ctx);
        let _ = FreeLibrary(lib);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{adl_collect, nvml_collect};

    #[test]
    fn nvml_collect_does_not_panic() {
        // 无驱动时返回空 Vec；有驱动时返回遥测；绝不 panic。
        let _ = nvml_collect();
    }

    #[test]
    fn adl_collect_does_not_panic() {
        let _ = adl_collect();
    }
}
