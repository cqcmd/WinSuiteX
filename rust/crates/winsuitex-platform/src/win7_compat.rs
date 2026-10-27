//! Win7 / Server 2008 R2 兼容垫片（仅 `x86_64-win7-windows-msvc` 目标编译）。
//!
//! ## 问题
//! 现代 `windows` crate (0.62.2) 把 `CoTaskMemFree` 的导入库指向 `combase.dll`。
//! `combase.dll` 是 Windows 8 才引入的 COM 基础 DLL，Win7 / Server 2008 R2 根本不存在，
//! 于是 exe 一加载就弹“combase.dll 丢失 / 0xc000007b”，连 `--version` 都跑不起来。
//!
//! ## 事实
//! `CoTaskMemFree` 的语义实现从 Win16 起就在 `ole32.dll`（OLE/COM 任务分配器），
//! `combase.dll` 只是 Win8+ 的转发宿主。`ole32.dll` 在所有 Win7+ 都存在，且
//! 自身不依赖 combase。所以把引用重定向到 `ole32.dll` 后，**行为完全一致**，
//! 无任何副作用——在 Win10/11 上 ole32 还会再转发回 combase，结果相同。
//!
//! ## 做法（运行时解析，不能用静态 link）
//! 用自己的强符号 `#[no_mangle] pub fn CoTaskMemFree` 覆盖 `windows` crate /
//! 本仓库对它的全部引用，链接器因此丢弃 `combase.dll` 整条导入。
//! 但**不能**用 `#[link(name = "ole32")] #[link_name = "CoTaskMemFree"]` 静态声明——
//! 那样导入符号也叫 `CoTaskMemFree`，会和上面的强符号同名，链接器把 import 又解析回
//! 本函数造成无限递归，且 ole32 的真实实现根本没进导入表。
//! 所以这里在运行时用 `GetProcAddress(ole32, "CoTaskMemFree")` 取到 ole32 真实地址再调用，
//! 彻底切断与链接器符号表的同名纠缠。调用点无需任何改动。

use core::ffi::c_void;

use std::sync::OnceLock;

use windows::core::{s, w};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

/// ole32 中 `CoTaskMemFree` 的真实函数指针类型。
type CoTaskMemFreeFn = unsafe extern "system" fn(*mut c_void);

/// 运行时解析 `ole32.dll!CoTaskMemFree`（一次性缓存，之后零开销）。
unsafe fn ole32_co_task_mem_free() -> Option<CoTaskMemFreeFn> {
    static ADDR: OnceLock<Option<CoTaskMemFreeFn>> = OnceLock::new();
    *ADDR.get_or_init(|| unsafe {
        // 进程跑 COM 时 ole32 必然已加载；这里只是拿到句柄。
        if let Ok(ole32) = LoadLibraryW(w!("ole32.dll")) {
            if let Some(p) = GetProcAddress(ole32, s!("CoTaskMemFree")) {
                // `GetProcAddress` 返回 `fn() -> isize`，转成真实签名再缓存。
                let real: CoTaskMemFreeFn = core::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    CoTaskMemFreeFn,
                >(p);
                return Some(real);
            }
        }
        None
    })
}

/// 覆盖 `windows` crate 生成的 `combase.dll!CoTaskMemFree` 引用。
///
/// `#[no_mangle]` 保证符号名与导入库一致；其余代码对 `CoTaskMemFree` 的调用
/// 全部解析到这里，combase 导入库因此被链接器丢弃，真正的工作转发给 ole32。
#[no_mangle]
pub unsafe extern "system" fn CoTaskMemFree(pv: *mut c_void) {
    if let Some(f) = ole32_co_task_mem_free() {
        f(pv);
    }
}

// ============================================================================
// DPI / 精确时间：Win8+ 导入（被 windows crate 按模块整块链接"连坐"引入）
// ----------------------------------------------------------------------------
// `windows` crate 0.62 按模块整块链接：只要用到某模块里的一个函数，整个模块的
// 全部函数（含 Win8+ 才有的）都会被写进 exe 导入表。下面用 `#[no_mangle]` 强符号
// 覆盖这些 Win8+ 导入，链接器据此丢弃对应条目；函数体在运行时用 `GetProcAddress`
// 探测真实实现，存在则走真实路径（Win10+ 行为不变），不存在则回退 Win7 等价实现。
// 调用点零改动。仅 `--cfg winsuitex_win7`（x86_64-win7-windows-msvc 构建）编译。

use windows::core::PCSTR;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;

/// 运行时从指定系统 DLL 解析函数，返回原始指针（`fn() -> isize`）。
/// 调用方需自行 transmute 到真实签名。Win7 上该符号不存在则返回 None。
unsafe fn load_fn(
    module: windows::core::PCWSTR,
    name: PCSTR,
) -> Option<unsafe extern "system" fn() -> isize> {
    let h = LoadLibraryW(module).ok()?;
    GetProcAddress(h, name)
}

// ---- 精确时间：GetSystemTimePreciseAsFileTime (Win8+) -> GetSystemTimeAsFileTime (Win2000+) ----
#[no_mangle]
pub unsafe extern "system" fn GetSystemTimePreciseAsFileTime(lp: *mut FILETIME) {
    if let Some(p) = load_fn(w!("kernel32.dll"), s!("GetSystemTimePreciseAsFileTime")) {
        let f: unsafe extern "system" fn(*mut FILETIME) = core::mem::transmute(p);
        f(lp);
        return;
    }
    // 本工具对时钟精度无强需求；Win7 退化为 GetSystemTimeAsFileTime 即可。
    let ft = GetSystemTimeAsFileTime();
    if !lp.is_null() {
        *lp = ft;
    }
}

// ---- DPI：Win10 专有函数，Win7 回退 ----
#[no_mangle]
pub unsafe extern "system" fn SetProcessDpiAwarenessContext(value: isize) -> i32 {
    if let Some(p) = load_fn(w!("user32.dll"), s!("SetProcessDpiAwarenessContext")) {
        let f: unsafe extern "system" fn(isize) -> i32 = core::mem::transmute(p);
        return f(value);
    }
    // Win7：退化为系统级 DPI 感知（Win2000+）。
    if let Some(p) = load_fn(w!("user32.dll"), s!("SetProcessDPIAware")) {
        let f: unsafe extern "system" fn() -> i32 = core::mem::transmute(p);
        return f();
    }
    1 // DPI 感知非致命，失败按成功处理，避免上层误判
}

#[no_mangle]
pub unsafe extern "system" fn SetProcessDpiAwareness(value: i32) -> i32 {
    if let Some(p) = load_fn(w!("shcore.dll"), s!("SetProcessDpiAwareness")) {
        let f: unsafe extern "system" fn(i32) -> i32 = core::mem::transmute(p);
        return f(value);
    }
    if let Some(p) = load_fn(w!("user32.dll"), s!("SetProcessDPIAware")) {
        let f: unsafe extern "system" fn() -> i32 = core::mem::transmute(p);
        return f();
    }
    1
}

#[no_mangle]
pub unsafe extern "system" fn GetDpiForSystem() -> u32 {
    if let Some(p) = load_fn(w!("user32.dll"), s!("GetDpiForSystem")) {
        let f: unsafe extern "system" fn() -> u32 = core::mem::transmute(p);
        return f();
    }
    96 // Win7 默认系统 DPI
}

#[no_mangle]
pub unsafe extern "system" fn GetDpiForWindow(hwnd: isize) -> u32 {
    if let Some(p) = load_fn(w!("user32.dll"), s!("GetDpiForWindow")) {
        let f: unsafe extern "system" fn(isize) -> u32 = core::mem::transmute(p);
        return f(hwnd);
    }
    96
}

// ---- 以下 HiDpi 模块的其余 Win8+ 符号被整块链接"连坐"引入，本程序不调用，
//      但必须提供同名强符号覆盖才能丢弃 api-ms-win-shcore-scaling 导入。
//      实现仅返回占位值（不会真正走到）。 ----
#[no_mangle]
pub unsafe extern "system" fn GetDpiAwarenessContextForProcess(_hprocess: isize) -> isize {
    0
}
#[no_mangle]
pub unsafe extern "system" fn GetDpiForMonitor(
    _hmonitor: isize,
    _dpitype: i32,
    _dpix: *mut u32,
    _dpiy: *mut u32,
) -> i32 {
    0
}
#[no_mangle]
pub unsafe extern "system" fn GetDpiFromDpiAwarenessContext(_value: isize) -> u32 {
    96
}
#[no_mangle]
pub unsafe extern "system" fn GetProcessDpiAwareness(_hprocess: isize, _value: *mut i32) -> i32 {
    0
}
