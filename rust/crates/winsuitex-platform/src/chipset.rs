//! 主板芯片组采集（Win32 SetupAPI）。
//!
//! 芯片组型号 Windows 不放在 SMBIOS 表里（Type 2 Baseboard 没有该字段），
//! 而是以 PnP 设备形式暴露在「系统设备」下，例如
//! `Intel(R) 600 Series Chipset Family SMBus Controller` / `AMD Ryzen 600 Series Chipset`。
//! 这里用 SetupAPI 遍历 PRESENT 设备，按 FriendlyName 含 "Chipset" 命中，取芯片组家族名。
//!
//! 但 AMD AM5/AM4 平台的芯片组设备**常不带 "Chipset" 词**，而是直接叫 `AMD B650` /
//! `AMD X670E` / `AMD Promontory 21` 之类——仅靠 "Chipset" 关键词会在真机上漏掉（见
//! 第三十九轮 Win11 真机复检）。因此 `extract_chipset_label` 在 "Chipset" 关键词路径之外，
//! 还有一条**家族号兜底**路径：按词边界匹配 `B650`/`X670E`/`Z690` 等芯片组家族 token，
//! 配合噪声词（Radeon/Ryzen/GeForce…）保护，避免把显卡/CPU 型号误判成芯片组。
//!
//! 任意失败 / 未命中 → 返回 `None`（诚实留白，绝不编造型号）。
//!
//! **第二路径（更可靠）**：主板型号本身编码了芯片组（如 `EX-B650M-V7 PLUS` →
//! `AMD B650`、`PRIME Z690-A` → `Intel Z690`），且已在 SMBIOS 表里采集到，无需额外
//! 枚举。`smbios.rs` 在 PnP 路径未命中时调用 `chipset_from_board_model` 兜底——这正是
//! 第四十一轮那台 Win11 真机的可靠来源（其芯片组 PnP 设备名是 `AMD FCH` 之类，
//! 不含家族号，PnP 路径抓不到，但 board product 明确写着 B650）。
//!
//! 仅用 SetupAPI + SMBIOS，不依赖 COM，可在任意采集线程调用。

use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
    SetupDiGetDeviceRegistryPropertyW, DIGCF_ALLCLASSES, DIGCF_PRESENT, HDEVINFO,
    SETUP_DI_REGISTRY_PROPERTY, SPDRP_DEVICEDESC, SPDRP_FRIENDLYNAME, SP_DEVINFO_DATA,
};

use crate::wide_to_string;

/// 芯片组家族 token（不含 "Chipset" 词、但设备名本身就带家族号的兜底匹配，
/// 覆盖 AMD/Intel 平台芯片组设备常不写 "Chipset" 的情形，如 `AMD B650` / `Intel Z690`）。
/// 顺序：长在前，避免 `B650E` 被 `B650` 抢先（配合词边界其实已足够，这里再保险）。
const CHIPSET_FAMILY_TOKENS: &[&str] = &[
    // AMD AM5 / AM4
    "X670E",
    "B650E",
    "X670",
    "B650",
    "A620",
    "X570",
    "B550",
    "A520",
    "X470",
    "B450",
    "A320",
    "X370",
    "B350",
    "A300",
    "TRX40",
    "WRX80",
    "X399",
    // Intel
    "Z790",
    "Z690",
    "Z590",
    "Z490",
    "H670",
    "H610",
    "H510",
    "H410",
    "B760",
    "B660",
    "B560",
    "B460",
    "Q670",
    "Q570",
    "W680",
    "W580",
    // AMD 芯片组代号（不带数字家族时，如 `AMD Promontory 21`）
    "Promontory",
];

/// 噪声词：命中则绝不视为芯片组，保护显卡/CPU 型号（如 `AMD Ryzen 7` / `Radeon RX` / `Intel Core i`）。
const CHIPSET_NOISE: &[&str] = &[
    "radeon",
    "ryzen",
    "geforce",
    "rtx",
    "gtx",
    "navi",
    "rdna",
    "xeon",
    "core i",
    "core(tm) i",
    "arc ",
    "celeron",
    "pentium",
];

/// 由芯片组家族 token 推断厂商。
///
/// token 本身已确定厂商，比依赖设备名 / 板号里是否含 "AMD"/"Intel" 词更可靠——
/// 板号 `EX-B650M-V7`、设备名 `B650` 常不带厂商词，但 `B650` 必是 AMD、`Z690`
/// 必是 Intel。注意 `B` 前缀在 AMD(B650) 与 Intel(B760) 间有歧义，故逐 token
/// 显式映射（必须与 `CHIPSET_FAMILY_TOKENS` 全集一一对应）。
fn vendor_for_token(tok: &str) -> &'static str {
    match tok {
        // Intel
        "Z790" | "Z690" | "Z590" | "Z490" | "H670" | "H610" | "H510" | "H410" | "B760" | "B660"
        | "B560" | "B460" | "Q670" | "Q570" | "W680" | "W580" => "Intel",
        // AMD
        "X670E" | "B650E" | "X670" | "B650" | "A620" | "X570" | "B550" | "A520" | "X470"
        | "B450" | "A320" | "X370" | "B350" | "A300" | "TRX40" | "WRX80" | "X399"
        | "Promontory" => "AMD",
        _ => "",
    }
}

/// `s[pos..end]` 是否处于词边界：前后字符必须非字母数字（或串边界），
/// 防止 `B650` 误中 `EX-B650M-V7` 这类板号。
fn at_word_boundary(s: &str, start: usize, end: usize) -> bool {
    let before_ok = start == 0 || !s.as_bytes()[start - 1].is_ascii_alphanumeric();
    let after_ok = end >= s.len() || !s.as_bytes()[end].is_ascii_alphanumeric();
    before_ok && after_ok
}

/// 从设备名抽取芯片组家族标签。
///
/// 路径 1（原逻辑）：含 "chipset" 词 → 裁掉尾部 Family / Controller 噪声。
/// 例：`Intel(R) 600 Series Chipset Family SMBus Controller` → `Intel(R) 600 Series Chipset`。
///
/// 路径 2（兜底）：不含 "chipset" 词时，按家族 token 词边界匹配
/// （`AMD B650` / `Intel Z690` / `AMD Promontory 21` …），命中即返回家族名。
/// 任意失败 / 未命中 → `None`（诚实留白，绝不编造型号）。
pub(crate) fn extract_chipset_label(name: &str) -> Option<String> {
    let n = name.trim();
    if n.is_empty() {
        return None;
    }
    let lower = n.to_ascii_lowercase();

    // 路径 1：含 "chipset" 词。
    if lower.contains("chipset") {
        // 裁掉 "Chipset" 之后的家族 / 控制器后缀噪声，只留芯片组家族名。
        // 后缀匹配用小写副本定位（大小写不敏感），按原串切片保留原始大小写。
        let base = if let Some(p) = lower.find("chipset family") {
            &n[..p + "chipset".len()]
        } else if let Some(p) = lower.find("chipset controller") {
            &n[..p + "chipset".len()]
        } else {
            n
        };
        let label = base.trim().to_string();
        return if label.is_empty() { None } else { Some(label) };
    }

    // 路径 2：家族 token 兜底。噪声词命中直接放弃本设备（显卡/CPU 型号保护）。
    for kw in CHIPSET_NOISE {
        if lower.contains(kw) {
            return None;
        }
    }
    for tok in CHIPSET_FAMILY_TOKENS {
        let ltok = tok.to_ascii_lowercase();
        if let Some(pos) = lower.find(&ltok) {
            let end = pos + tok.len();
            if at_word_boundary(&lower, pos, end) {
                let mut label = String::new();
                let vendor = vendor_for_token(tok);
                if !vendor.is_empty() {
                    label.push_str(vendor);
                    label.push(' ');
                }
                // 用规范 token 大小写输出（如 `B650` / `Promontory`），
                // 不取原始切片，保证小写设备名也得到规范大写家族号。
                label.push_str(tok);
                return Some(label.trim().to_string());
            }
        }
    }
    None
}

/// 检测主板芯片组型号（最佳努力：失败 / 未命中返回 `None`）。
pub fn detect_chipset() -> Option<String> {
    // SAFETY: 枚举 PRESENT 设备，返回有效 HDEVINFO（失败转 None）。
    let dev_info =
        match unsafe { SetupDiGetClassDevsW(None, None, None, DIGCF_ALLCLASSES | DIGCF_PRESENT) } {
            Ok(h) => h,
            Err(_) => return None,
        };
    // SAFETY: dev_info 有效；Drop 时释放。
    let _guard = DevInfoGuard(dev_info);

    let mut did = SP_DEVINFO_DATA {
        cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
        ..Default::default()
    };
    let mut idx = 0u32;
    let mut fallback: Option<String> = None;
    loop {
        // SAFETY: did.cbSize 已初始化；返回 FALSE 表示枚举结束。
        let ok = unsafe { SetupDiEnumDeviceInfo(dev_info, idx, &mut did) }.is_ok();
        if !ok {
            break;
        }
        idx += 1;

        // 优先 FRIENDLYNAME，回落 DEVICEDESC。
        let name = get_dev_prop(dev_info, &did, SPDRP_FRIENDLYNAME)
            .or_else(|| get_dev_prop(dev_info, &did, SPDRP_DEVICEDESC));
        let name = match name {
            Some(n) => n,
            None => continue,
        };
        let label = match extract_chipset_label(&name) {
            Some(l) => l,
            None => continue,
        };
        // 优先选含 "Series" 的家族名（更规范，如 `Intel(R) 600 Series Chipset`），
        // 否则作为兜底（如 `AMD X670 Chipset`）。
        if label.to_ascii_lowercase().contains("series") {
            return Some(label);
        }
        if fallback.is_none() {
            fallback = Some(label);
        }
    }
    fallback
}

/// 从主板型号字符串反推芯片组家族（**第二路径**，比猜 PnP 设备名可靠）。
///
/// 主板型号本身编码了芯片组，例如 `EX-B650M-V7 PLUS` → `AMD B650`、
/// `PRIME Z690-A` → `Intel Z690`、`B550M DS3H` → `AMD B550`。数据已在 SMBIOS
/// 表里采集到，无需额外枚举，因此也适用于"PnP 设备名不含 chipset / 家族号"的真机
/// （如第四十一轮那台 Win11 物理机：芯片组设备名是 `AMD FCH` 之类，PnP 路径抓不到，
/// 但 board product 明确写着 B650）。
///
/// 边界规则比 PnP 路径宽松：token 前需词边界，token 后允许接形态字母（M=MicroATX /
/// I / X）或型号尾缀——`EX-B650M-V7` 的 `M` 是形态不是芯片组号——只要后面不是数字
/// （避免 `B650` 误中 `B6500`）即命中。噪声词保护仍生效。
pub(crate) fn chipset_from_board_model(product: &str) -> Option<String> {
    let n = product.trim();
    if n.is_empty() {
        return None;
    }
    let lower = n.to_ascii_lowercase();
    for kw in CHIPSET_NOISE {
        if lower.contains(kw) {
            return None;
        }
    }
    for tok in CHIPSET_FAMILY_TOKENS {
        let ltok = tok.to_ascii_lowercase();
        if let Some(pos) = lower.find(&ltok) {
            let before_ok = pos == 0 || !lower.as_bytes()[pos - 1].is_ascii_alphanumeric();
            let after = lower.as_bytes().get(pos + tok.len());
            // token 后允许非数字尾缀（形态字母 / 连字符 / 空格）；仅排除数字以防误中。
            let after_ok = match after {
                None => true,
                Some(&b) => !b.is_ascii_digit(),
            };
            if before_ok && after_ok {
                let mut label = String::new();
                let vendor = vendor_for_token(tok);
                if !vendor.is_empty() {
                    label.push_str(vendor);
                    label.push(' ');
                }
                // 规范 token 大小写输出（不取原始切片）。
                label.push_str(tok);
                return Some(label.trim().to_string());
            }
        }
    }
    None
}

/// 读取设备字符串属性（REG_SZ）。与 audio.rs / usb.rs / hid.rs 内同名函数一致的两段式实现：
/// 先以 `None` 缓冲区查询所需字节数，再按真实大小分配，避免固定缓冲在长属性时被截断。
fn get_dev_prop(
    dev_info: HDEVINFO,
    did: &SP_DEVINFO_DATA,
    prop: SETUP_DI_REGISTRY_PROPERTY,
) -> Option<String> {
    let mut required = 0u32;
    let _ = unsafe {
        SetupDiGetDeviceRegistryPropertyW(dev_info, did, prop, None, None, Some(&mut required))
    };
    if required == 0 {
        return None;
    }
    let mut buf = vec![0u8; required as usize];
    let ok = unsafe {
        SetupDiGetDeviceRegistryPropertyW(dev_info, did, prop, None, Some(&mut buf), None)
    }
    .is_ok();
    if !ok {
        return None;
    }
    let mut wide: Vec<u16> = Vec::with_capacity(buf.len() / 2);
    let mut i = 0;
    while i + 1 < buf.len() {
        wide.push(u16::from_ne_bytes([buf[i], buf[i + 1]]));
        i += 2;
    }
    wide_to_string(&wide)
}

/// `HDEVINFO` RAII 守卫。
struct DevInfoGuard(HDEVINFO);

impl Drop for DevInfoGuard {
    fn drop(&mut self) {
        // SAFETY: self.0 是有效的 HDEVINFO。
        let _ = unsafe { SetupDiDestroyDeviceInfoList(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_chipset_trims_family_suffix() {
        assert_eq!(
            extract_chipset_label("Intel(R) 600 Series Chipset Family SMBus Controller"),
            Some("Intel(R) 600 Series Chipset".into())
        );
    }

    #[test]
    fn extract_chipset_keeps_bare_chipset_name() {
        assert_eq!(
            extract_chipset_label("AMD X670 Chipset"),
            Some("AMD X670 Chipset".into())
        );
        assert_eq!(
            extract_chipset_label("AMD Ryzen 600 Series Chipset"),
            Some("AMD Ryzen 600 Series Chipset".into())
        );
    }

    #[test]
    fn extract_chipset_rejects_non_chipset() {
        assert_eq!(extract_chipset_label("PCI standard host CPU bridge"), None);
        assert_eq!(extract_chipset_label("High Definition Audio 设备"), None);
        assert_eq!(extract_chipset_label("   "), None);
    }

    #[test]
    fn extract_chipset_case_insensitive() {
        assert_eq!(
            extract_chipset_label("intel z690 chipset family lpc controller"),
            Some("intel z690 chipset".into())
        );
    }

    // —— 路径 2：AMD/Intel 家族 token 兜底（第三十九轮 Win11 真机漏识别的修复） ——

    #[test]
    fn extract_chipset_amd_family_token_with_vendor() {
        // AMD B650 平台芯片组设备常直接叫 "AMD B650"，不含 "Chipset" 词。
        assert_eq!(extract_chipset_label("AMD B650"), Some("AMD B650".into()));
        assert_eq!(extract_chipset_label("amd b650"), Some("AMD B650".into()));
        assert_eq!(extract_chipset_label("AMD X670E"), Some("AMD X670E".into()));
        assert_eq!(extract_chipset_label("AMD A620"), Some("AMD A620".into()));
    }

    #[test]
    fn extract_chipset_amd_promontory() {
        assert_eq!(
            extract_chipset_label("AMD Promontory 21"),
            Some("AMD Promontory".into())
        );
    }

    #[test]
    fn extract_chipset_intel_family_token() {
        assert_eq!(
            extract_chipset_label("Intel Z690"),
            Some("Intel Z690".into())
        );
        // 无厂商词时仍由 token 推断厂商（Z690 必是 Intel）。
        assert_eq!(extract_chipset_label("Z690"), Some("Intel Z690".into()));
    }

    #[test]
    fn extract_chipset_rejects_board_model_as_false_positive() {
        // 词边界保护：板号 EX-B650M-V7 里的 "B650" 紧接 "M"，不是独立词 → 不命中。
        assert_eq!(extract_chipset_label("EX-B650M-V7 PLUS"), None);
    }

    #[test]
    fn extract_chipset_rejects_gpu_cpu_names() {
        // 噪声词保护：显卡/CPU 型号绝不被误判为芯片组。
        assert_eq!(extract_chipset_label("AMD Ryzen 7 7800X3D"), None);
        assert_eq!(extract_chipset_label("Radeon RX 7900 XTX"), None);
        assert_eq!(extract_chipset_label("NVIDIA GeForce RTX 4070"), None);
        assert_eq!(extract_chipset_label("Intel(R) Core i7-13700K"), None);
    }

    // —— 第二路径：主板型号反推（第四十一轮那台 Win11 真机靠它识别） ——

    #[test]
    fn chipset_from_board_model_amd_b650_with_formfactor() {
        // EX-B650M-V7 PLUS：B650 后接形态字母 M，应识别为 AMD B650（M 不计入）。
        assert_eq!(
            chipset_from_board_model("EX-B650M-V7 PLUS"),
            Some("AMD B650".into())
        );
        assert_eq!(
            chipset_from_board_model("ASUS TUF GAMING B650-PLUS WIFI"),
            Some("AMD B650".into())
        );
    }

    #[test]
    fn chipset_from_board_model_intel_and_variants() {
        assert_eq!(
            chipset_from_board_model("PRIME Z690-A"),
            Some("Intel Z690".into())
        );
        assert_eq!(
            chipset_from_board_model("B550M DS3H"),
            Some("AMD B550".into())
        );
        // X670E 是独立家族（长 token 优先），不是 X670 + 尾缀 E。
        assert_eq!(
            chipset_from_board_model("X670E GAMING X"),
            Some("AMD X670E".into())
        );
    }

    #[test]
    fn chipset_from_board_model_rejects_non_chipset_board() {
        assert_eq!(chipset_from_board_model("To be filled by O.E.M."), None);
        assert_eq!(chipset_from_board_model(""), None);
        assert_eq!(chipset_from_board_model("   "), None);
    }

    #[test]
    fn chipset_from_board_model_rejects_digit_false_positive() {
        // 后面是数字时不命中，避免 B650 误中某型号里的 "B6500"。
        assert_eq!(chipset_from_board_model("ABC-B6500-XYZ"), None);
    }
}
