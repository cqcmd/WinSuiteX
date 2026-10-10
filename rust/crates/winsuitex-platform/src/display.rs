//! 显示器信息源：GDI `EnumDisplayDevicesW` + `EnumDisplaySettingsW`。
//!
//! 设计要点（对齐 C++ 版 `CollectGpu` 显示器分支）：
//! - 只枚举"挂在桌面"（`DISPLAY_DEVICE_ACTIVE`）的适配器下的活动监视器，避免
//!   RDP/镜像适配器产生十几条同名重复显示器行。
//! - 显示器名取自监视器的 `DeviceString`（= EDID 型号），为空则回退到适配器名。
//! - 分辨率/刷新率取自 `EnumDisplaySettingsW(ENUM_CURRENT_SETTINGS)`。
//! - 厂商与型号：EDID 没给具体型号时 Windows 会把监视器名 fallback 成
//!   "Generic PnP Monitor"，真实型号藏在 PnP 硬件 ID（`DeviceID`）第二段里，
//!   形如 `MONITOR\AOC2769\...` 或 `DISPLAY\GSM32G2WG3\...`。见下方纯函数
//!   `monitor_model_from_device_id` / `display_name_with_model` 与 `pnp_vendor`。
//! - 物理尺寸（英寸）：仍需解析 EDID / SetupAPI，本期未实现 → `None`。
//!   不伪造数据，报告层会自然省略。
//! - 边界：API 失败降级（该显示器跳过 / 分辨率置 `None`），不整体失败；
//!   取值 0 视为"拿不到" → `None`，禁止 0 充数。

use std::mem::size_of;

use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{
    EnumDisplayDevicesW, EnumDisplaySettingsW, DEVMODEW, DISPLAY_DEVICEW, DISPLAY_DEVICE_ACTIVE,
    ENUM_CURRENT_SETTINGS,
};

use crate::edid::{parse_edid, read_edid_bytes};
use crate::wide_to_string;
use winsuitex_core::brand_display_or;
use winsuitex_core::error::Result;
use winsuitex_core::model::Display;
use winsuitex_core::traits::{CollectContext, HardwareSource};

/// PnP / EISA 显示器厂商码 → 品牌 key（对齐 C++ 版 `kPnpMonitorVendor`）。
///
/// 硬件 ID 第二段的前 3 个字母即 PnP 厂商码（如 `GSM` / `SAM` / `DEL`），
/// 映射到**英文品牌 key**；中文名统一回查 `winsuitex_core::brand_zh_or`
/// （全项目唯一数据源），本表不再直接写中文。
/// 少数显示器固件会填错厂商码（如 AOC 屏报 LG 的 `GSM`），这是硬件 EDID 问题，
/// 本表按 PnP 标准码翻译，与 Windows 设备管理器行为一致。
const K_PNP_VENDOR: &[(&str, &str)] = &[
    ("GSM", "LG"),
    ("LGD", "LG Display"),
    ("ACR", "Acer"),
    ("AAC", "Acer"),
    ("API", "Acer"),
    ("AOC", "AOC"),
    ("ACI", "ASUS"),
    ("SAM", "Samsung"),
    ("SEC", "Samsung"),
    ("DEL", "Dell"),
    ("HWP", "HP"),
    ("HPN", "HP"),
    ("SNY", "Sony"),
    ("PHL", "Philips"),
    ("BNQ", "BenQ"),
    ("VSC", "ViewSonic"),
    ("IVM", "iiyama"),
    ("EIZ", "Eizo"),
    ("ENC", "Eizo"),
    ("NEC", "NEC"),
    ("CMN", "Innolux"),
    ("AUO", "AUO"),
    ("BOE", "BOE"),
    ("HSD", "HannStar"),
    ("TCL", "TCL"),
    ("SPT", "Sharp"),
    ("MEI", "Panasonic"),
    ("FUS", "Fujitsu"),
    ("MAG", "Mag"),
    ("CPT", "CPT"),
    ("DGC", "Daewoo"),
    ("TAT", "Tatung"),
    ("ZCM", "Zenith"),
    ("WDE", "Westinghouse"),
    ("ONN", "Onn"),
    ("SHP", "Sharp"),
    // —— 二次核对补全：国内常见与办公/电竞品牌 ——
    ("LEN", "Lenovo"),
    ("CMO", "CMO"), // Chi Mei，后并入群创(Innolux)
    ("HKC", "HKC"),
    ("XMI", "Xiaomi"),
    ("VIZ", "Vizio"),
    ("CPQ", "Compaq"),
    ("TOS", "Toshiba"),
    ("AUI", "AOpen"),
];

/// 把 3 字母 PnP 厂商码转大写后查表，命中返回规范显示名
/// （`英文（中文）` 格式，经 core 统一解析；未收录中文的已知码回原文，如 `LG`）。
///
/// 大小写不敏感；未知码返回 `None`。
pub(crate) fn pnp_vendor(code3: &str) -> Option<String> {
    let code = code3.to_ascii_uppercase();
    K_PNP_VENDOR
        .iter()
        .find_map(|(c, key)| (*c == code.as_str()).then_some(*key))
        .map(brand_display_or)
}

/// 厂商展示名：已知码→可读名；未知码→原样回退为 3 字母码（避免"无厂商"或把原始码漏进型号）。
fn vendor_display(code3: &str) -> String {
    pnp_vendor(code3)
        .map(|s| s.to_string())
        .unwrap_or_else(|| code3.to_string())
}

/// 通用监视器名清单（大小写不敏感、子串匹配）。
///
/// 这些名字属于 Windows 的占位通用名，EDID 没给具体型号时才会用上，
/// 此时应直接用「厂商 型号」整段替换，而非追加 `(MODEL)`。
const GENERIC_NAMES: &[&str] = &[
    "Generic PnP Monitor",
    "Generic Non-PnP Monitor",
    "通用即插即用监视器",
    "通用非即插即用监视器",
];

fn is_generic_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    GENERIC_NAMES
        .iter()
        .any(|g| lower.contains(&g.to_ascii_lowercase()))
}

/// RDP / 镜像 / 间接（虚拟）显示设备关键字（大小写不敏感子串匹配）。
///
/// 这些不是物理显示器（远程桌面、USB 虚拟屏、向日葵/TeamViewer/ToDesk 虚拟显示器、
/// 微软基础显示驱动占位等），列出会污染"真实显示器"统计。仅作二次核对后的兜底过滤，
/// 与适配器层的 `DISPLAY_DEVICE_ACTIVE` 过滤互补。
const VIRTUAL_DISPLAY_KEYWORDS: &[&str] = &[
    "rdp",
    "remote",
    "indirect",
    "mirror",
    "virtual",
    "basic display",
    "terminal server",
    // 常见间接（虚拟）显示驱动：向日葵 / Splashtop / TeamViewer / ToDesk / DisplayLink 等
    "oray",
    "idd",
    "displaylink",
    "todesk",
    "teamviewer",
    "splashtop",
    "usb display",
];

fn is_virtual_display(name: &str, device_id: &str) -> bool {
    let n = name.to_ascii_lowercase();
    if VIRTUAL_DISPLAY_KEYWORDS.iter().any(|k| n.contains(k)) {
        return true;
    }
    device_id.to_ascii_lowercase().contains("indirectdisplay")
}

/// 从监视器 `DeviceID`（PnP 硬件 ID）解析真实型号与厂商。
///
/// 硬件 ID 形如 `MONITOR\AOC2769\4&...` 或 `DISPLAY\GSM32G2WG3\...`：
/// 1. 取反斜杠后的**第二段**硬件 ID（即 `MONITOR\`/`DISPLAY\` 之后、下一个 `\` 之前）。
/// 2. 含下划线（`Default_Monitor`/`Generic_xxx`）属占位 ID，无真实厂商型号 → `None`。
/// 3. 固定取前 3 字符作 PnP 厂商码，第 4 字符起全部是型号；型号为空（只有厂商码）→ `None`。
///
/// 返回 `(厂商名, 型号)`，两者都借用自 `device_id`。解析不出则返回 `None`。
pub(crate) fn monitor_model_from_device_id(device_id: &str) -> Option<(String, String)> {
    // 1. 跳过 "MONITOR\" / "DISPLAY\"，取第二段硬件 ID。
    let sep = device_id.find('\\')?;
    let rest = &device_id[sep + 1..];
    let hw_id = match rest.find('\\') {
        Some(q) => &rest[..q],
        None => rest,
    };
    if hw_id.is_empty() {
        return None;
    }
    // 2. 含下划线的占位 ID 没有真实厂商/型号。
    if hw_id.contains('_') {
        return None;
    }
    // 3. 前 3 字符 = PnP 厂商码；其后全部为型号。型号可能以字母开头（如 C34J / A0B0），
    //    故固定取前 3 字符，不能"剥掉所有前导字母"。
    // 用 `get` 而非直接切片：畸形固件给出非 ASCII 的硬件 ID 时，字节下标可能落在
    // 多字节字符中间，直接切片会 panic。PnP ID 按规范是 ASCII，但解析外部输入
    // 不该靠这个假设撑着 —— 切不出来就当"没有型号"。
    let code3 = hw_id.get(..3)?;
    let model = hw_id.get(3..)?;
    if model.is_empty() {
        return None;
    }
    let vendor = vendor_display(code3);
    Some((vendor, model.to_string()))
}

/// 根据监视器名与解析出的厂商/型号，决定如何写回显示器名。
///
/// - 厂商名查得到 **且** 当前名属于"通用名"：整段替换为 `"<厂商> <型号>"`
///   （如 `Generic PnP Monitor` + `AOC`/`2769` → `AOC 2769`）。
/// - 否则若名字里已含型号：不重复追加，原样返回。
/// - 否则追加 `" (MODEL)"`。
pub(crate) fn display_name_with_model(name: &str, vendor: Option<&str>, model: &str) -> String {
    // 名字为空（极少数边界）：直接用"厂商 型号"构造，避免前导空格。
    if name.is_empty() {
        return match vendor {
            Some(v) => format!("{} {}", v, model),
            None => model.to_string(),
        };
    }
    // 厂商名存在且当前是通用名 → 整段替换，避免 "Generic PnP Monitor (2769)"。
    if let Some(v) = vendor {
        if is_generic_name(name) {
            return format!("{} {}", v, model);
        }
    }
    // 名字里已含型号则不重复追加。
    if name
        .to_ascii_lowercase()
        .contains(&model.to_ascii_lowercase())
    {
        return name.to_string();
    }
    // 否则追加 " (MODEL)"。
    format!("{} ({})", name, model)
}

/// 显示器采集源（GDI）。
pub struct DisplaySource;

impl HardwareSource for DisplaySource {
    fn name(&self) -> &'static str {
        "display.gdi"
    }

    #[allow(clippy::field_reassign_with_default)]
    fn collect(&self, ctx: &mut CollectContext) -> Result<()> {
        let mut displays = Vec::new();

        // 第一层：枚举显示适配器。
        let mut i = 0u32;
        loop {
            let mut dd = DISPLAY_DEVICEW::default();
            dd.cb = size_of::<DISPLAY_DEVICEW>() as u32;
            // SAFETY: dd.cb 已初始化；设备名传 NULL 枚举适配器。
            if !unsafe { EnumDisplayDevicesW(PCWSTR::null(), i, &mut dd, 0) }.as_bool() {
                break;
            }
            i += 1;

            // 只处理挂在桌面的适配器，避免 RDP/镜像适配器产生重复监视器行。
            if !dd.StateFlags.contains(DISPLAY_DEVICE_ACTIVE) {
                continue;
            }

            // 跳过 RDP / 镜像 / 间接（虚拟）显示适配器（见 is_virtual_display）。
            let dd_name = wide_to_string(&dd.DeviceString)
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            let dd_id = wide_to_string(&dd.DeviceID).unwrap_or_default();
            if is_virtual_display(&dd_name, &dd_id) {
                continue;
            }

            // 第二层：在该适配器下枚举监视器。
            let mut j = 0u32;
            loop {
                // 与 dd 同因：HSTRING 字段不能经 ..Default::default() 初始化（语义改变）。
                let mut mdd = DISPLAY_DEVICEW::default();
                mdd.cb = size_of::<DISPLAY_DEVICEW>() as u32;
                // SAFETY: mdd.cb 已初始化；dd.DeviceName 在本轮有效且以 NUL 结尾。
                if !unsafe { EnumDisplayDevicesW(PCWSTR(dd.DeviceName.as_ptr()), j, &mut mdd, 0) }
                    .as_bool()
                {
                    break;
                }
                j += 1;

                if !mdd.StateFlags.contains(DISPLAY_DEVICE_ACTIVE) {
                    continue;
                }

                // 跳过 RDP / 镜像 / 间接（虚拟）监视器（见 is_virtual_display）。
                let mdd_name = wide_to_string(&mdd.DeviceString)
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();
                let mdd_id = wide_to_string(&mdd.DeviceID).unwrap_or_default();
                if is_virtual_display(&mdd_name, &mdd_id) {
                    continue;
                }

                // 显示器名：优先监视器 DeviceString，回退适配器 DeviceString。
                let mut name = wide_to_string(&mdd.DeviceString)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .or_else(|| wide_to_string(&dd.DeviceString).map(|s| s.trim().to_string()));

                // 从 PnP 硬件 ID（DeviceID）补全真实型号与厂商，并尝试读 EDID 增强字段。
                let device_id = wide_to_string(&mdd.DeviceID).unwrap_or_default();
                let mut vendor: Option<String> = None;
                if let Some((v, model)) = monitor_model_from_device_id(&device_id) {
                    let base = name.as_deref().unwrap_or("");
                    name = Some(display_name_with_model(base, Some(v.as_str()), &model));
                    vendor = Some(v);
                }
                // EDID 增强：序列号 / 生产日期 / 物理尺寸（缺则 None，不影响主链路）。
                let edid = if !device_id.is_empty() {
                    read_edid_bytes(&device_id).map(|b| parse_edid(&b))
                } else {
                    None
                };
                let mut dm = DEVMODEW::default();
                dm.dmSize = size_of::<DEVMODEW>() as u16;
                // SAFETY: dm.dmSize 已初始化；设备名指针在本轮有效。
                let ok = unsafe {
                    EnumDisplaySettingsW(
                        PCWSTR(mdd.DeviceName.as_ptr()),
                        ENUM_CURRENT_SETTINGS,
                        &mut dm,
                    )
                }
                .as_bool()
                    || unsafe {
                        EnumDisplaySettingsW(
                            PCWSTR(dd.DeviceName.as_ptr()),
                            ENUM_CURRENT_SETTINGS,
                            &mut dm,
                        )
                    }
                    .as_bool();

                // 0 视为无效 → None（禁止用 0 充数）。
                let (width, height, refresh_hz) = if ok {
                    (
                        (dm.dmPelsWidth != 0).then_some(dm.dmPelsWidth),
                        (dm.dmPelsHeight != 0).then_some(dm.dmPelsHeight),
                        (dm.dmDisplayFrequency != 0).then_some(dm.dmDisplayFrequency),
                    )
                } else {
                    (None, None, None)
                };

                displays.push(Display {
                    name,
                    vendor,
                    size_inch: edid.as_ref().and_then(|e| e.size_inch.clone()),
                    serial: edid.as_ref().and_then(|e| e.serial.clone()),
                    manufacture_year: edid.as_ref().and_then(|e| e.manufacture_year),
                    manufacture_week: edid.as_ref().and_then(|e| e.manufacture_week),
                    width,
                    height,
                    refresh_hz,
                });
            }
        }

        ctx.set_displays(displays);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winsuitex_core::traits::{CollectContext, HardwareSource};

    #[test]
    fn pnp_vendor_lookup() {
        // 大小写不敏感
        assert_eq!(pnp_vendor("gsm"), Some("LG".into()));
        assert_eq!(pnp_vendor("GSM"), Some("LG".into()));
        // 注：品牌显示已统一为 core 的 `英文（中文）` 规范格式（第二十轮）。
        assert_eq!(pnp_vendor("AOC"), Some("AOC（冠捷）".into()));
        // 二次核对补全的国内/办公品牌
        assert_eq!(pnp_vendor("LEN"), Some("Lenovo（联想）".into()));
        assert_eq!(pnp_vendor("HKC"), Some("HKC（惠科）".into()));
        assert_eq!(pnp_vendor("XMI"), Some("Xiaomi（小米）".into()));
        assert_eq!(pnp_vendor("CMO"), Some("CMO（奇美）".into()));
        // 未知码
        assert_eq!(pnp_vendor("XYZ"), None);
    }

    #[test]
    fn monitor_model_from_device_id_ok() {
        // 真实用例：AOC 2769（MSI B75MA-P45 + AOC 2769）。
        // 注：厂商名已统一为 core 的 `英文（中文）` 规范格式（第二十轮）。
        assert_eq!(
            monitor_model_from_device_id(r"MONITOR\AOC2769\4&1a2b3c4&0&UID0"),
            Some(("AOC（冠捷）".to_string(), "2769".to_string()))
        );
        // DISPLAY\ 前缀 + 型号以字母开头（C34J）
        assert_eq!(
            monitor_model_from_device_id(r"DISPLAY\SAMC34JWG\...\0000"),
            Some(("Samsung（三星）".to_string(), "C34JWG".to_string()))
        );
        // 未知厂商码应回退为原始 3 字母码，而非 None
        assert_eq!(
            monitor_model_from_device_id(r"MONITOR\XYZ9999\...\0000"),
            Some(("XYZ".to_string(), "9999".to_string()))
        );
    }

    #[test]
    fn is_virtual_display_detects_remote_and_indirect() {
        // RDP / 远程 / 间接显示应被判为虚拟，不当真实显示器
        assert!(is_virtual_display("Microsoft Remote Display Adapter", ""));
        assert!(is_virtual_display("RDP Display", ""));
        assert!(is_virtual_display("OrayIddDriver Device", "SWD\\..."));
        assert!(is_virtual_display("Microsoft Basic Display Adapter", ""));
        // 真实物理显示器不应被误杀
        assert!(!is_virtual_display("AOC 2769", "MONITOR\\AOC2769\\..."));
        assert!(!is_virtual_display("Dell U2723", "MONITOR\\DELA...\\..."));
    }

    #[test]
    fn display_name_with_model_empty_name_no_leading_space() {
        // 名字为空时直接用"厂商 型号"构造，不应出现前导空格
        assert_eq!(
            display_name_with_model("", Some("三星"), "C34JWG"),
            "三星 C34JWG"
        );
        assert_eq!(display_name_with_model("", None, "2769"), "2769");
    }

    #[test]
    fn monitor_model_from_device_id_placeholder() {
        // 含下划线的占位 ID 返回 None
        assert_eq!(
            monitor_model_from_device_id(r"MONITOR\Default_Monitor\...\0000"),
            None
        );
        // 找不到反斜杠返回 None
        assert_eq!(monitor_model_from_device_id("NO_BACKSLASH"), None);
        // 只有厂商码、无型号返回 None
        assert_eq!(monitor_model_from_device_id(r"MONITOR\AOC\...\0000"), None);
    }

    #[test]
    fn display_name_with_model_replace_generic() {
        // 真实用例：Generic PnP Monitor + AOC/2769 → AOC 2769
        assert_eq!(
            display_name_with_model("Generic PnP Monitor", Some("AOC"), "2769"),
            "AOC 2769"
        );
        // 中文系统通用名同样整段替换
        assert_eq!(
            display_name_with_model("通用即插即用监视器", Some("三星"), "C34JWG"),
            "三星 C34JWG"
        );
    }

    #[test]
    fn display_name_with_model_append() {
        // 厂商为 None 时走追加分支（不整段替换）
        let s = display_name_with_model("Generic PnP Monitor", None, "2769");
        assert!(s.ends_with("(2769)"), "应走追加分支，实际: {s}");

        // 厂商存在但名字不是通用名 → 追加
        assert_eq!(
            display_name_with_model("Dell Monitor", Some("戴尔"), "U2723"),
            "Dell Monitor (U2723)"
        );

        // 名字里已含型号（非通用名）则不重复追加
        assert_eq!(
            display_name_with_model("AOC 2769", Some("AOC"), "2769"),
            "AOC 2769"
        );
        assert_eq!(
            display_name_with_model("Dell Monitor U2723", Some("戴尔"), "U2723"),
            "Dell Monitor U2723"
        );
    }

    #[test]
    fn real_machine_collect() {
        let src = DisplaySource;
        let mut ctx = CollectContext::new();
        src.collect(&mut ctx)
            .expect("display collect must return Ok");

        let (info, _vols) = ctx.into_parts();

        // 注意：无头/断开的会话里 `EnumDisplayDevicesW` 会合法地返回 0 个设备
        // （本机 KVM 虚拟机在会话无桌面时正是如此，C++ 版此时同样输出「未知」）。
        // 因此这里断言的是**内部一致性**而非「必须有显示器」——后者是环境假设，
        // 会让测试在无头 CI / RDP 断连时假失败。
        for d in &info.displays {
            assert!(
                d.width.is_some() && d.height.is_some(),
                "枚举到的显示器若挂在桌面就必须能取到分辨率，实际: {d:?}"
            );
            eprintln!(
                "DISPLAY: name={:?} vendor={:?} size={:?} serial={:?} year={:?} week={:?} {}x{} @ {}Hz",
                d.name,
                d.vendor,
                d.size_inch,
                d.serial,
                d.manufacture_year,
                d.manufacture_week,
                d.width.unwrap_or(0),
                d.height.unwrap_or(0),
                d.refresh_hz.unwrap_or(0)
            );
        }
        eprintln!(
            "DISPLAY: 共 {} 个（无头/RDP 断连会话为 0 属正常）",
            info.displays.len()
        );
    }
}
