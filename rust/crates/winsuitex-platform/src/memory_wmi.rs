//! 内存采集的 WMI 兜底：`Win32_PhysicalMemory`（命名空间 `ROOT\CIMV2`）。
//!
//! # 为什么需要这一层
//! SMBIOS Type 17 在部分固件 / 无盘启动环境下**不报 Configured Speed**
//! （于是报告里「没有内存频率」），或把实插槽误报成空槽（Size=0）。
//! WMI 走的是与 SMBIOS 完全独立的固件/OS 数据通路，能补回这些字段。
//!
//! # 纪律（与项目「诚实 None」原则一致）
//! - 只**填** SMBIOS 漏采的字段，**绝不覆盖** SMBIOS 已给的值；
//! - WMI 取不到 / 取到占位串（Unknown / To Be Filled By O.E.M.）就保持原样；
//! - 容量回填（空槽→实插）**必须** (DeviceLocator, BankLabel) 键匹配成功才做，
//!   纯索引对齐只用于补速度/厂商串（这类填充不改变「插槽占用 / 空槽」状态，绝不会
//!   把真·空槽误标成已插）；
//! - 任何一步失败都整体放弃本次兜底，SMBIOS 已采数据原样保留。
//!
//! COM 初始化范式与 `physical_disk.rs` 的 `collect_storage_reliability` 完全一致
//! （MTA + PKT_PRIVACY + CoSetProxyBlanket），此处自包含以避免改动已验证的磁盘路径。

use std::sync::OnceLock;

use windows::core::{IUnknown, Interface, BSTR, PCWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoInitializeSecurity, CoSetProxyBlanket, CoTaskMemFree,
    CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, EOAC_NONE,
    RPC_C_AUTHN_LEVEL_PKT_PRIVACY, RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Variant::{VariantToStringAlloc, VariantToUInt64, VARIANT};
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemContext, IWbemLocator, IWbemServices, WbemLocator,
    WBEM_FLAG_FORWARD_ONLY, WBEM_INFINITE,
};

use crate::to_wide;
use winsuitex_core::model::{MebiBytes, MemStick};
use winsuitex_core::traits::{CollectContext, HardwareSource};

/// RAII 守卫：仅当「本次调用亲自初始化了 COM」时才 `CoUninitialize`，
/// 避免把同线程早先初始化的 COM 引用计数错误归零（与 physical_disk.rs 同款）。
struct ComGuard {
    own: bool,
}
impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.own {
            unsafe { CoUninitialize() };
        }
    }
}

/// WMI 调试日志：仅当进程环境变量 `WSX_WMI_DEBUG` 存在时，把消息追加到 `wmi_debug.log`。
/// 用于在「无法编译验证的真机」上抓取真实失败点（本机 VM 的 WMI 不返回数据，无法自测）。
fn wmi_dbg_enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("WSX_WMI_DEBUG").is_some())
}

fn wmi_dbg(msg: &str) {
    if !wmi_dbg_enabled() {
        return;
    }
    use std::io::Write as _;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("wmi_debug.log")
    {
        let _ = writeln!(f, "[WMI-MEM] {msg}");
    }
}

/// 一条 `Win32_PhysicalMemory` 实例（仅保留本兜底需要的字段）。
#[derive(Debug, Default, Clone)]
pub(crate) struct WmiMem {
    locator: Option<String>,
    bank: Option<String>,
    /// 配置时钟（MT/s），优先于 `speed`。
    configured_clock_speed: Option<u64>,
    /// 标称速率（MT/s）。
    speed: Option<u64>,
    /// 容量（字节）。
    capacity_bytes: Option<u64>,
    manufacturer: Option<String>,
    part_number: Option<String>,
    serial_number: Option<String>,
    /// SMBIOS 原始 Memory Type 字节（`SMBIOSMemoryType`，如 0x1A=DDR4）。
    ///
    /// 用于**回补** SMBIOS Type 17 直采没解析出可展示类型的情况：部分服务器固件
    /// （华为 Insyde 等）在 Type 17 里填的是泛化值（Other/Unknown/DRAM），
    /// 而 WMI 这条独立通路能拿到真实代际。取不到就 None，绝不猜。
    ///
    /// ⚠️ 只用 `SMBIOSMemoryType`（原始字节，与本项目的 `mem_type_name` 同表），
    /// 不用 CIM 版 `MemoryType`（另一套枚举，20=DDR 而非 26=DDR4）。
    smbios_memory_type: Option<u32>,
}

/// 归一化槽位键：小写、去前缀（`P0 `/`BANK `/`B`/`CHANNEL` 仅保留通道字母），去空白。
///
/// 目的：让 SMBIOS 的 `P0 CHANNEL A` 与 WMI 的 `CHANNEL A` 归一到同一键，
/// 从而在同一物理插槽上正确匹配（华硕双 "DIMM 0" 同序的怪象靠 bank 区分）。
fn normalize_slot(locator: &Option<String>, bank: &Option<String>) -> String {
    let clean = |s: &str| -> String {
        let mut t = s.to_ascii_lowercase();
        t = t
            .trim()
            .trim_start_matches("p0 ")
            .trim_start_matches("bank ")
            .trim_start_matches('b')
            .trim()
            .to_string();
        // 抽掉所有非字母数字，避免空格/连字符差异干扰
        t.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
    };
    format!(
        "{}{}",
        clean(locator.as_deref().unwrap_or("")),
        clean(bank.as_deref().unwrap_or(""))
    )
}

/// 占位串判定：SMBIOS/WMI 都可能回 "Unknown" / "To Be Filled By O.E.M." 等，
/// 这些值等同缺失，应允许被另一来源覆盖。
fn is_meaningless(s: Option<&str>) -> bool {
    match s {
        None => true,
        Some(v) => {
            let t = v.trim();
            t.is_empty()
                || t.eq_ignore_ascii_case("unknown")
                || t.eq_ignore_ascii_case("to be filled by o.e.m.")
                || t.eq_ignore_ascii_case("default string")
                || t.eq_ignore_ascii_case("none")
                || t.eq_ignore_ascii_case("oem")
                || t.eq_ignore_ascii_case("n/a")
        }
    }
}

/// 读取 WMI 对象的 `uint64` 属性（Capacity / Speed / ConfiguredClockSpeed）。
///
/// 统一用 `VariantToUInt64`：小整数（VT_UI4）也能正确读出，且不会像
/// `VariantToUInt16` 那样把超范围值静默钳位成 65535。
fn wmi_get_u64(obj: &IWbemClassObject, prop: &str) -> Option<u64> {
    let wide = to_wide(prop);
    let pc = PCWSTR(wide.as_ptr());
    let mut var = VARIANT::default();
    if unsafe { obj.Get(pc, 0, &mut var, None, None) }.is_err() {
        return None;
    }
    unsafe { VariantToUInt64(&var) }.ok()
}

/// 读取 WMI 对象的 `string` 属性（DeviceLocator / BankLabel / Manufacturer 等）。
fn wmi_get_bstr(obj: &IWbemClassObject, prop: &str) -> Option<String> {
    let wide = to_wide(prop);
    let pc = PCWSTR(wide.as_ptr());
    let mut var = VARIANT::default();
    if unsafe { obj.Get(pc, 0, &mut var, None, None) }.is_err() {
        return None;
    }
    let pw = unsafe { VariantToStringAlloc(&var) }.ok()?;
    let s = String::from_utf16_lossy(unsafe { pw.as_wide() });
    unsafe { CoTaskMemFree(Some(pw.0 as *mut core::ffi::c_void)) };
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// 连接指定 WMI 命名空间（自包含 COM 初始化）。
///
/// 返回的 `(svc, guard)` 中 `guard` **必须由调用方持有到查询全部完成之后**才允许丢弃——
/// 它的 `Drop` 会 `CoUninitialize()`。若 guard 提前析构，COM 被拆除，后续对 `svc` 代理的
/// `ExecQuery`/`Next` 会触发访问违规（段错误）。这与 `physical_disk.rs` 用 `OnceLock`
/// 让 guard 存活整个进程是同一意图的不同实现（本兜底只跑一次，故让 guard 随查询函数结束）。
fn wmi_connect(namespace: &str) -> Option<(IWbemServices, ComGuard)> {
    // SAFETY: 标准 COM 初始化；RPC_E_CHANGED_MODE(0x80010106) 表示线程已初始化，沿用即可。
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    wmi_dbg(&format!("CoInitializeEx hr={:#x}", hr.0));
    // 进程级 COM 安全初始化：WMI 客户端必做（遗漏 → ConnectServer/ExecQuery 看似成功，
    // 取数 Next 被 WBEM_E_ACCESS_DENIED 拒绝，即使已提权）。重复调用返回
    // RPC_E_ALREADY_INITIALIZED / RPC_E_TOO_LATE 属正常，忽略。
    let _sec_hr = unsafe {
        CoInitializeSecurity(
            None,
            -1,
            None,
            None,
            RPC_C_AUTHN_LEVEL_PKT_PRIVACY,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
            None,
        )
    };
    wmi_dbg(&format!(
        "CoInitializeSecurity hr={:?}",
        _sec_hr.as_ref().map(|_| 0).map_err(|e| e.code().0)
    ));

    // COM 只初始化、不在此处反初始化：反初始化（`CoUninitialize`）会拆除公寓，
    // 而 `rows`/`svc` 等 COM 对象此刻仍在作用域内，Release 会触发访问违规（段错误）。
    // 与 `physical_disk.rs` 一致——把 guard 放进 `OnceLock` 常驻进程，由 OS 在退出时回收。
    let _com = if hr.is_ok() || (hr.0 & 0xFFFF) == 0x0106 {
        ComGuard { own: false }
    } else {
        wmi_dbg("CoInitializeEx 失败，放弃 COM 兜底");
        return None;
    };

    let locator: IWbemLocator =
        match unsafe { CoCreateInstance(&WbemLocator, None::<&IUnknown>, CLSCTX_INPROC_SERVER) } {
            Ok(l) => l,
            Err(e) => {
                wmi_dbg(&format!("CoCreateInstance(WbemLocator) 失败: {e:?}"));
                return None;
            }
        };
    let ns = BSTR::from(namespace);
    let empty = BSTR::from("");
    let svc = match unsafe {
        locator.ConnectServer(
            &ns,
            &empty,
            &empty,
            &empty,
            0,
            &empty,
            None::<&IWbemContext>,
        )
    } {
        Ok(s) => s,
        Err(e) => {
            wmi_dbg(&format!("ConnectServer({ns}) 失败: {e:?}"));
            return None;
        }
    };
    wmi_dbg(&format!("ConnectServer({namespace}) 成功"));
    if let Ok(unk) = svc.cast::<IUnknown>() {
        let r = unsafe {
            CoSetProxyBlanket(
                &unk,
                10u32, // RPC_C_AUTHN_WINNT
                0u32,
                PCWSTR::null(),
                RPC_C_AUTHN_LEVEL_PKT_PRIVACY,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None::<*const core::ffi::c_void>,
                EOAC_NONE,
            )
        };
        wmi_dbg(&format!("CoSetProxyBlanket(svc) = {r:?}"));
    }
    // 把 `ComGuard` 一并交还调用方持有——它的 Drop 才 CoUninitialize。
    // 若在此处丢弃，COM 会先于 ExecQuery/Next 被拆除 → 段错误。
    Some((svc, _com))
}

/// 执行一条 WQL 查询，返回所有 `IWbemClassObject`（失败返回空 Vec）。
fn wmi_query(svc: &IWbemServices, wql: &str) -> Vec<IWbemClassObject> {
    let mut out = Vec::new();
    let lang = BSTR::from("WQL");
    let query = BSTR::from(wql);
    let enum_obj = match unsafe {
        svc.ExecQuery(&lang, &query, WBEM_FLAG_FORWARD_ONLY, None::<&IWbemContext>)
    } {
        Ok(e) => e,
        Err(e) => {
            wmi_dbg(&format!("ExecQuery 失败: {e:?} wql={wql}"));
            return out;
        }
    };
    if let Ok(unk) = enum_obj.cast::<IUnknown>() {
        let r = unsafe {
            CoSetProxyBlanket(
                &unk,
                10u32,
                0u32,
                PCWSTR::null(),
                RPC_C_AUTHN_LEVEL_PKT_PRIVACY,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None::<*const core::ffi::c_void>,
                EOAC_NONE,
            )
        };
        wmi_dbg(&format!("CoSetProxyBlanket(enum) = {r:?}"));
    }
    loop {
        let mut objs: [Option<IWbemClassObject>; 1] = [None];
        let mut returned: u32 = 0;
        let hr = unsafe { enum_obj.Next(WBEM_INFINITE, &mut objs, &mut returned) };
        if hr.is_ok() && returned > 0 {
            if let Some(obj) = objs[0].take() {
                out.push(obj);
            }
            continue;
        }
        if hr.is_err() {
            let code = hr.0 as u32;
            let kind = if code == 0x8004_1003 {
                "WBEM_E_ACCESS_DENIED(未以管理员身份运行/权限不足)"
            } else {
                "其他错误"
            };
            wmi_dbg(&format!("Next 失败 hr={code:#x} ({kind}) wql={wql}"));
        }
        break;
    }
    out
}

/// 查询 `Win32_PhysicalMemory`，失败返回 `None`（上层放弃兜底，不动 SMBIOS 数据）。
fn query_wmi_memory() -> Option<Vec<WmiMem>> {
    // `_guard` 必须存活到下面的 `wmi_query` 全部完成；函数返回前它才析构，
    // 确保 ExecQuery/Next 期间 COM 仍初始化。
    let (svc, _guard) = wmi_connect("ROOT\\CIMV2")?;
    let rows = wmi_query(
        &svc,
        // `SMBIOSMemoryType`：Win8/Server2012+ 才有。老系统 Get 失败返回 None，
        // 只影响「类型回补」这一项，其余字段照常工作（纪律：取不到就留 None，不猜）。
        "SELECT DeviceLocator, BankLabel, Speed, ConfiguredClockSpeed, Capacity, \
         Manufacturer, PartNumber, SerialNumber, SMBIOSMemoryType FROM Win32_PhysicalMemory",
    );
    wmi_dbg(&format!("Win32_PhysicalMemory 命中 {} 条", rows.len()));
    if rows.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let capacity = wmi_get_u64(r, "Capacity");
        let speed = wmi_get_u64(r, "Speed");
        let ccs = wmi_get_u64(r, "ConfiguredClockSpeed");
        let m = WmiMem {
            locator: wmi_get_bstr(r, "DeviceLocator"),
            bank: wmi_get_bstr(r, "BankLabel"),
            configured_clock_speed: ccs,
            speed,
            capacity_bytes: capacity,
            manufacturer: wmi_get_bstr(r, "Manufacturer"),
            part_number: wmi_get_bstr(r, "PartNumber"),
            serial_number: wmi_get_bstr(r, "SerialNumber"),
            smbios_memory_type: wmi_get_u64(r, "SMBIOSMemoryType")
                .and_then(|v| u32::try_from(v).ok()),
        };
        wmi_dbg(&format!(
            "  WMI mem: loc={:?} bank={:?} cap={:?} ccs={:?} speed={:?} mfr={:?} smbios_type={:?}",
            m.locator,
            m.bank,
            m.capacity_bytes,
            m.configured_clock_speed,
            m.speed,
            m.manufacturer,
            m.smbios_memory_type
        ));
        out.push(m);
    }
    Some(out)
}

/// 给「已实插但缺速度」的插槽补频率。不改变插槽占用/空槽状态，索引对齐也安全。
fn fill_speed(stick: &mut MemStick, w: &WmiMem) {
    if stick.speed_mhz.is_none() {
        // 优先 ConfiguredClockSpeed（实际配置频率），回退 Speed（标称速率）。
        if let Some(sp) = w.configured_clock_speed.or(w.speed).filter(|v| *v > 0) {
            stick.speed_mhz = Some(sp as u32);
        }
    }
}

/// 给「厂商/型号/序列号是占位串」的插槽补真实串（绝不改写已有的真值）。
fn fill_meta(stick: &mut MemStick, w: &WmiMem) {
    if is_meaningless(stick.vendor.as_deref()) {
        if let Some(v) = w
            .manufacturer
            .as_ref()
            .filter(|s| !is_meaningless(Some(s.as_str())))
        {
            stick.vendor = Some(v.clone());
        }
    }
    if is_meaningless(stick.part_no.as_deref()) {
        if let Some(p) = w
            .part_number
            .as_ref()
            .filter(|s| !is_meaningless(Some(s.as_str())))
        {
            stick.part_no = Some(p.clone());
        }
    }
    if is_meaningless(stick.serial.as_deref()) {
        if let Some(sn) = w
            .serial_number
            .as_ref()
            .filter(|s| !is_meaningless(Some(s.as_str())))
        {
            stick.serial = Some(sn.clone());
        }
    }
}

/// 给「SMBIOS 没解析出可展示的内存类型」的插槽补类型（如 DDR4）。
///
/// 场景：服务器固件把 SMBIOS Type 17 的 Memory Type 填成泛化值（Other/Unknown/
/// DRAM），按规范 `mem_type_name` 返回 None → 报告里内存条只剩"容量 · 频率"，
/// 看不出是 DDR4 还是 DDR5，对资产盘点/加内存是直接缺失（Z.W. 华为服务器真机）。
/// WMI `SMBIOSMemoryType` 回传的是同一套 SMBIOS 原始字节，可安全复用同一张表。
fn fill_mem_type(stick: &mut MemStick, w: &WmiMem) {
    if stick.mem_type.is_some() {
        return; // SMBIOS 直采已给出类型，绝不覆盖
    }
    let Some(raw) = w.smbios_memory_type else {
        return;
    };
    let Ok(byte) = u8::try_from(raw) else {
        return; // 越界（>255）不是合法 SMBIOS 字节，丢弃
    };
    if let Some(name) = winsuitex_core::smbios::mem_type_name(byte) {
        stick.mem_type = Some(name.to_string());
    }
}

/// 仅在「(locator,bank) 键匹配」成功时，把 SMBIOS 的空槽补成实插（容量来自 WMI）。
/// 这是唯一会改变「空槽→已插」状态的填充，必须强匹配，避免把真·空槽误标成已插。
fn fill_capacity_if_keyed(stick: &mut MemStick, w: &WmiMem) -> bool {
    if stick.capacity_mb.is_some() {
        return false; // 已实插，无需补容量（速度/型号由别的填充负责）
    }
    let s_key = normalize_slot(&stick.locator, &stick.bank);
    let w_key = normalize_slot(&w.locator, &w.bank);
    if s_key.is_empty() || s_key != w_key {
        return false; // 键不匹配：宁可留空槽，也不冒险误补
    }
    if let Some(bytes) = w.capacity_bytes.filter(|b| *b > 0) {
        stick.capacity_mb = Some(MebiBytes::from_bytes_floor(bytes));
        return true;
    }
    false
}

/// 次级容量回填：仅用于「(locator,bank) 主键未命中、且仍缺容量」的插槽。
///
/// 真实根因（49 份真机快照量化）：部分固件把 `BankLabel` 回显成与 `DeviceLocator`
/// 完全相同（如 locator=`DIMM_A2` bank=`DIMM_A2`），导致 `normalize_slot` 的
/// `(locator,bank)` 主键与 WMI（bank 通常为空）失配，于是这些**已实插**内存条的容量
/// 永远补不回来（真机 16 根此类缺口）。
///
/// 兜底：用归一化 `locator`（bank 置空）做**唯一**匹配回填容量。这不改变「空槽占用」
/// 状态——locator 即物理插槽号，与 WMI `DeviceLocator` 是同一物理槽；若 WMI 在该 slot
/// 报了容量，说明此槽确实插了内存（即便 SMBIOS 把 vendor 误写成占位串），回填正确；
/// 若真·空槽（WMI 无此 locator 条目），则无匹配，保持空槽。绝不变更 vendor/part_no，
/// 绝不覆盖 SMBIOS 已给的容量。
fn fill_capacity_by_locator_secondary(sticks: &mut [MemStick], wmi: &[WmiMem]) {
    if wmi.is_empty() {
        return;
    }
    use std::collections::{HashMap, HashSet};
    // locator -> wmi 索引（归一化 locator），并记录重复键。
    let mut loc_to_idx: HashMap<String, usize> = HashMap::new();
    let mut loc_dup: HashSet<String> = HashSet::new();
    for (i, w) in wmi.iter().enumerate() {
        let k = normalize_slot(&w.locator, &None);
        if k.is_empty() {
            continue;
        }
        match loc_to_idx.entry(k) {
            std::collections::hash_map::Entry::Occupied(e) => {
                loc_dup.insert(e.key().clone());
            }
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(i);
            }
        }
    }
    if loc_to_idx.is_empty() {
        return;
    }
    for s in sticks.iter_mut() {
        if s.capacity_mb.is_some() {
            continue; // 已有容量（含 SMBIOS 真值），不覆盖
        }
        let k = normalize_slot(&s.locator, &None);
        if k.is_empty() || loc_dup.contains(&k) {
            continue; // 无 locator 锚点 / 歧义：跳过，保持诚实
        }
        let Some(&i) = loc_to_idx.get(&k) else {
            continue; // WMI 无此物理槽 → 真·空槽，保持空
        };
        if let Some(bytes) = wmi[i].capacity_bytes.filter(|b| *b > 0) {
            s.capacity_mb = Some(MebiBytes::from_bytes_floor(bytes));
        }
    }
}

/// 纯函数：用 WMI 数据回填 SMBIOS 已采的内存条。
///
/// 匹配策略：
/// - 槽数一致 → 索引对齐补速度/型号（安全，不改变占用状态），容量仍要求键匹配；
/// - 槽数不一致 → 按归一化 (locator,bank) 键匹配，且要求**唯一**命中，否则跳过（避免歧义误补）；
/// - 主键（含索引对齐）仍未补上容量的，再走 `fill_capacity_by_locator_secondary`
///   （仅以 locator 唯一匹配，回收固件把 bank 回显成 locator 导致的容量缺口）。
pub(crate) fn enrich_from_wmi(sticks: &mut [MemStick], wmi: &[WmiMem]) {
    if wmi.is_empty() {
        return;
    }
    if sticks.len() == wmi.len() {
        for (s, w) in sticks.iter_mut().zip(wmi.iter()) {
            fill_speed(s, w);
            fill_meta(s, w);
            fill_mem_type(s, w);
            fill_capacity_if_keyed(s, w);
        }
        fill_capacity_by_locator_secondary(sticks, wmi);
        return;
    }
    // 槽数不一致：建立 (key -> wmi 索引) 多映射，仅唯一命中才补。
    use std::collections::HashMap;
    let mut key_to_idx: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, w) in wmi.iter().enumerate() {
        let key = normalize_slot(&w.locator, &w.bank);
        if !key.is_empty() {
            key_to_idx.entry(key).or_default().push(i);
        }
    }
    for s in sticks.iter_mut() {
        let key = normalize_slot(&s.locator, &s.bank);
        if key.is_empty() {
            continue;
        }
        let Some(idxs) = key_to_idx.get(&key) else {
            continue;
        };
        if idxs.len() != 1 {
            continue; // 歧义：跳过，保持诚实
        }
        let w = &wmi[idxs[0]];
        fill_speed(s, w);
        fill_meta(s, w);
        fill_mem_type(s, w);
        fill_capacity_if_keyed(s, w);
    }
    fill_capacity_by_locator_secondary(sticks, wmi);
}

/// 入口：查询 WMI 并就地回填 `sticks`。任何失败都整体放弃，原数据不动。
pub fn enrich_memory_from_wmi(sticks: &mut [MemStick]) {
    if sticks.is_empty() {
        return; // 无 SMBIOS 槽可补；这里只做「增强」，不凭 WMI 凭空造槽
    }
    wmi_dbg(&format!("enrich: sticks.len()={}", sticks.len()));
    let Some(wmi) = query_wmi_memory() else {
        wmi_dbg("Win32_PhysicalMemory 查询失败/为空，放弃内存 WMI 兜底");
        return;
    };
    wmi_dbg(&format!(
        "enrich: wmi.len()={}, 进入 enrich_from_wmi",
        wmi.len()
    ));
    enrich_from_wmi(sticks, &wmi);
    wmi_dbg("enrich: enrich_from_wmi 完成");
}

/// 内存 WMI 兜底采集源：在 `SmbiosSource` 之后运行，读取已采槽位并回填缺失字段。
pub struct MemoryWmiSource;

impl HardwareSource for MemoryWmiSource {
    fn name(&self) -> &'static str {
        "memory.wmi"
    }

    fn collect(&self, ctx: &mut CollectContext) -> winsuitex_core::error::Result<()> {
        let mut sticks: Vec<MemStick> = ctx.memory_sticks().to_vec();
        if sticks.is_empty() {
            return Ok(()); // 没有可增强的对象，直接放行
        }
        wmi_dbg(&format!("collect: 取到 {} 条 sticks", sticks.len()));
        enrich_memory_from_wmi(&mut sticks);
        wmi_dbg("collect: enrich 完成，准备 set_memory_sticks");
        ctx.set_memory_sticks(sticks);
        wmi_dbg("collect: set_memory_sticks 完成");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stick(
        locator: &str,
        bank: &str,
        cap: Option<u32>,
        speed: Option<u32>,
        vendor: Option<&str>,
    ) -> MemStick {
        MemStick {
            locator: Some(locator.into()),
            bank: Some(bank.into()),
            serial: None,
            part_no: None,
            vendor: vendor.map(Into::into),
            mem_type: None,
            capacity_mb: cap.map(|c| MebiBytes::new(c as u64)),
            speed_mhz: speed,
        }
    }

    fn mk_wmi(
        locator: &str,
        bank: &str,
        cap_bytes: Option<u64>,
        ccs: Option<u64>,
        speed: Option<u64>,
    ) -> WmiMem {
        WmiMem {
            locator: Some(locator.into()),
            bank: Some(bank.into()),
            configured_clock_speed: ccs,
            speed,
            capacity_bytes: cap_bytes,
            manufacturer: Some("Kingston".into()),
            part_number: Some("KF560C30-32".into()),
            serial_number: Some("AD170E88".into()),
            smbios_memory_type: None,
        }
    }

    /// 内存类型回补（第三十一轮）：SMBIOS Type 17 的 Memory Type 是泛化值时
    /// `mem_type=None`，用 WMI `SMBIOSMemoryType`（同一套 SMBIOS 原始字节）补回。
    #[test]
    fn fills_missing_mem_type_from_smbios_memory_type() {
        // 华为服务器真机形态：容量/频率/厂商都在，唯独类型解析不出（0x1A 应为 DDR4）。
        let mut sticks = vec![stick(
            "DIMM000",
            "_Node0_Channel0_Dimm0",
            Some(32768),
            Some(2400),
            Some("SK Hynix"),
        )];
        let mut w = mk_wmi(
            "DIMM000",
            "_Node0_Channel0_Dimm0",
            Some(32u64 << 30),
            Some(2400),
            None,
        );
        w.smbios_memory_type = Some(0x1A); // DDR4
        enrich_from_wmi(&mut sticks, &[w]);
        assert_eq!(
            sticks[0].mem_type.as_deref(),
            Some("DDR4"),
            "应补回 DDR4（0x1A）"
        );
    }

    #[test]
    fn never_overwrites_known_mem_type() {
        let mut sticks = vec![stick(
            "DIMM 0",
            "P0 CHANNEL A",
            Some(32768),
            Some(3200),
            None,
        )];
        sticks[0].mem_type = Some("DDR5".into());
        let mut w = mk_wmi("DIMM 0", "CHANNEL A", Some(32u64 << 30), Some(3200), None);
        w.smbios_memory_type = Some(0x1A); // DDR4（错的，用来验证不被覆盖）
        enrich_from_wmi(&mut sticks, &[w]);
        assert_eq!(
            sticks[0].mem_type.as_deref(),
            Some("DDR5"),
            "SMBIOS 已给的类型绝不被 WMI 覆盖"
        );
    }

    /// 泛化值（Unknown=0x02）不该被渲染成类型；越界值也不是合法 SMBIOS 字节。
    #[test]
    fn mem_type_ignores_generic_and_out_of_range_values() {
        let mut sticks = vec![stick(
            "DIMM 0",
            "P0 CHANNEL A",
            Some(16384),
            Some(2666),
            None,
        )];
        let mut w = mk_wmi("DIMM 0", "CHANNEL A", Some(16u64 << 30), Some(2666), None);
        w.smbios_memory_type = Some(0x02); // Unknown（泛化值，按规范不展示）
        enrich_from_wmi(&mut sticks, &[w]);
        assert_eq!(sticks[0].mem_type, None, "泛化值不应显示成类型");

        let mut w2 = mk_wmi("DIMM 0", "CHANNEL A", Some(16u64 << 30), Some(2666), None);
        w2.smbios_memory_type = Some(0x1_0000); // 越界，非 u8
        enrich_from_wmi(&mut sticks, &[w2]);
        assert_eq!(sticks[0].mem_type, None, "越界值应丢弃");
    }

    #[test]
    fn fills_missing_speed_by_index_alignment() {
        // 实插但 SMBIOS 漏报速度：槽数一致 → 索引对齐补速度。
        let mut sticks = vec![stick(
            "DIMM 0",
            "P0 CHANNEL A",
            Some(32768),
            None,
            Some("Kingston"),
        )];
        let wmi = vec![mk_wmi(
            "DIMM 0",
            "CHANNEL A",
            Some(32u64 << 30),
            Some(6000),
            None,
        )];
        enrich_from_wmi(&mut sticks, &wmi);
        assert_eq!(sticks[0].speed_mhz, Some(6000), "应补上 6000 MT/s");
        assert_eq!(sticks[0].capacity_mb, Some(MebiBytes::new(32768)));
    }

    #[test]
    fn never_overwrites_existing_speed() {
        let mut sticks = vec![stick(
            "DIMM 0",
            "P0 CHANNEL A",
            Some(32768),
            Some(4800),
            Some("Kingston"),
        )];
        let wmi = vec![mk_wmi(
            "DIMM 0",
            "CHANNEL A",
            Some(32u64 << 30),
            Some(6000),
            None,
        )];
        enrich_from_wmi(&mut sticks, &wmi);
        assert_eq!(sticks[0].speed_mhz, Some(4800), "已有速度绝不被覆盖");
    }

    #[test]
    fn corrects_false_empty_slot_only_with_key_match() {
        // SMBIOS 把实插槽误报成空槽，WMI 键匹配成功 → 补成实插。
        let mut sticks = vec![stick("DIMM 1", "P0 CHANNEL B", None, None, Some("Unknown"))];
        let wmi = vec![mk_wmi(
            "DIMM 1",
            "CHANNEL B",
            Some(16u64 << 30),
            Some(5600),
            None,
        )];
        enrich_from_wmi(&mut sticks, &wmi);
        assert_eq!(
            sticks[0].capacity_mb,
            Some(MebiBytes::new(16384)),
            "键匹配应补容量"
        );
        assert_eq!(sticks[0].speed_mhz, Some(5600));
        assert_eq!(sticks[0].vendor.as_deref(), Some("Kingston"));
        assert_eq!(sticks[0].part_no.as_deref(), Some("KF560C30-32"));
    }

    #[test]
    fn genuine_empty_slot_stays_empty_when_wmi_also_empty() {
        // 单条机器：SMBIOS 第二槽空，WMI 该槽容量也为 0 → 保持空槽（不误补）。
        let mut sticks = vec![
            stick(
                "DIMM 0",
                "P0 CHANNEL A",
                Some(32768),
                Some(6000),
                Some("Kingston"),
            ),
            stick("DIMM 0", "P0 CHANNEL B", None, None, Some("Unknown")),
        ];
        let wmi = vec![
            mk_wmi("DIMM 0", "CHANNEL A", Some(32u64 << 30), Some(6000), None),
            mk_wmi("DIMM 0", "CHANNEL B", Some(0), None, None),
        ];
        enrich_from_wmi(&mut sticks, &wmi);
        assert!(
            sticks[1].capacity_mb.is_none(),
            "WMI 容量为 0 时不应误标已插"
        );
    }

    #[test]
    fn ambiguous_locator_without_key_match_is_not_filled() {
        // 槽数不一致且键无法唯一命中（双 "DIMM 0" 且 bank 都归一为空）→ 跳过，保持诚实。
        let mut sticks = vec![stick("DIMM 0", "", Some(32768), None, Some("Kingston"))];
        // 两条 WMI 都归一成同一空键 → 歧义，不应补。
        let wmi = vec![
            mk_wmi("DIMM 0", "", Some(32u64 << 30), Some(6000), None),
            mk_wmi("DIMM 0", "", Some(16u64 << 30), Some(5600), None),
        ];
        enrich_from_wmi(&mut sticks, &wmi);
        assert_eq!(sticks[0].speed_mhz, None, "歧义匹配下不应误补速度");
    }

    #[test]
    fn meaningless_vendor_is_replaced_but_real_vendor_kept() {
        let mut sticks = vec![stick(
            "DIMM 0",
            "P0 CHANNEL A",
            Some(32768),
            Some(6000),
            Some("Unknown"),
        )];
        let wmi = vec![mk_wmi(
            "DIMM 0",
            "CHANNEL A",
            Some(32u64 << 30),
            Some(6000),
            None,
        )];
        enrich_from_wmi(&mut sticks, &wmi);
        assert_eq!(sticks[0].vendor.as_deref(), Some("Kingston"));

        let mut sticks2 = vec![stick(
            "DIMM 0",
            "P0 CHANNEL A",
            Some(32768),
            Some(6000),
            Some("Crucial"),
        )];
        let wmi2 = vec![mk_wmi(
            "DIMM 0",
            "CHANNEL A",
            Some(32u64 << 30),
            Some(6000),
            None,
        )];
        enrich_from_wmi(&mut sticks2, &wmi2);
        assert_eq!(
            sticks2[0].vendor.as_deref(),
            Some("Crucial"),
            "真值厂商不被覆盖"
        );
    }

    #[test]
    fn locator_secondary_recovers_capacity_when_bank_echoed() {
        // 真实根因：固件把 bank 回显成与 locator 相同（DIMM_A2 / DIMM_A2），
        // 主键 (locator,bank) 与 WMI（bank 为空）失配；次级 locator 唯一匹配应补回容量。
        let mut sticks = vec![stick(
            "DIMM_A2",
            "DIMM_A2",
            None,
            None,
            Some("DIMM_A2_Manufacturer"),
        )];
        let wmi = vec![mk_wmi("DIMM_A2", "", Some(32u64 << 30), Some(3200), None)];
        enrich_from_wmi(&mut sticks, &wmi);
        assert_eq!(
            sticks[0].capacity_mb,
            Some(MebiBytes::new(32768)),
            "locator 次级匹配应补回容量"
        );
        assert_eq!(sticks[0].speed_mhz, Some(3200), "速度亦应补回");
    }

    #[test]
    fn locator_secondary_keeps_true_empty_slot_empty() {
        // 真·空槽（WMI 根本没有该 locator 条目）→ 无匹配，保持空槽，绝误补。
        let mut sticks = vec![stick("DIMMA1", "NO DIMM", None, None, Some("NO DIMM"))];
        let wmi = vec![mk_wmi("DIMMB1", "", Some(16u64 << 30), Some(3200), None)];
        enrich_from_wmi(&mut sticks, &wmi);
        assert!(
            sticks[0].capacity_mb.is_none(),
            "WMI 无 DIMMA1 → 真·空槽保持空"
        );
    }

    #[test]
    fn locator_secondary_skips_ambiguous_duplicate_locator() {
        // 两条 WMI 同为 "DIMM 0"（bank 回显成空），主键 (locator,bank) 失配，
        // 次级 locator 唯一匹配出现歧义 → 跳过，保持诚实，不误补。
        let mut sticks = vec![stick("DIMM 0", "DIMM 0", None, None, Some("Kingston"))];
        let wmi = vec![
            mk_wmi("DIMM 0", "", Some(32u64 << 30), Some(6000), None),
            mk_wmi("DIMM 0", "", Some(16u64 << 30), Some(5600), None),
        ];
        enrich_from_wmi(&mut sticks, &wmi);
        assert!(
            sticks[0].capacity_mb.is_none(),
            "locator 歧义下不应误补容量"
        );
    }

    #[test]
    fn locator_secondary_does_not_overwrite_existing_capacity() {
        let mut sticks = vec![stick(
            "DIMM_A2",
            "DIMM_A2",
            Some(32768),
            Some(3200),
            Some("Kingston"),
        )];
        let wmi = vec![mk_wmi("DIMM_A2", "", Some(16u64 << 30), Some(5600), None)];
        enrich_from_wmi(&mut sticks, &wmi);
        assert_eq!(
            sticks[0].capacity_mb,
            Some(MebiBytes::new(32768)),
            "已有容量绝不被覆盖"
        );
        assert_eq!(sticks[0].speed_mhz, Some(3200), "已有速度绝不被覆盖");
    }

    #[test]
    fn normalize_slot_bridges_asus_quirk() {
        // SMBIOS "P0 CHANNEL A" 与 WMI "CHANNEL A" 应归一到同一键。
        assert_eq!(
            normalize_slot(&Some("DIMM 0".into()), &Some("P0 CHANNEL A".into())),
            normalize_slot(&Some("DIMM 0".into()), &Some("CHANNEL A".into())),
        );
        assert_ne!(
            normalize_slot(&Some("DIMM 0".into()), &Some("P0 CHANNEL A".into())),
            normalize_slot(&Some("DIMM 0".into()), &Some("P0 CHANNEL B".into())),
        );
    }
}
