//! 主板芯片组采集（Win32 SetupAPI）。
//!
//! 芯片组型号 Windows 不放在 SMBIOS 表里（Type 2 Baseboard 没有该字段），
//! 而是以 PnP 设备形式暴露在「系统设备」下，例如
//! `Intel(R) 600 Series Chipset Family SMBus Controller` / `AMD Ryzen 600 Series Chipset`。
//! 这里用 SetupAPI 遍历 PRESENT 设备，按 FriendlyName 含 "Chipset" 命中，取芯片组家族名。
//!
//! 任意失败 / 未命中 → 返回 `None`（诚实留白，绝不编造型号）。
//! 仅用 SetupAPI，不依赖 COM，可在任意采集线程调用。

use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
    SetupDiGetDeviceRegistryPropertyW, DIGCF_ALLCLASSES, DIGCF_PRESENT, HDEVINFO,
    SETUP_DI_REGISTRY_PROPERTY, SPDRP_DEVICEDESC, SPDRP_FRIENDLYNAME, SP_DEVINFO_DATA,
};

use crate::wide_to_string;

/// 从设备名抽取芯片组家族标签；不含 "chipset" 则 `None`。
///
/// 例：`Intel(R) 600 Series Chipset Family SMBus Controller`
/// → `Intel(R) 600 Series Chipset`（裁掉尾部的 Family / Controller 噪声）。
pub(crate) fn extract_chipset_label(name: &str) -> Option<String> {
    let n = name.trim();
    if !n.to_ascii_lowercase().contains("chipset") {
        return None;
    }
    // 裁掉 "Chipset" 之后的家族 / 控制器后缀噪声，只留芯片组家族名。
    // 后缀匹配用小写副本定位（大小写不敏感），按原串切片保留原始大小写。
    let lower = n.to_ascii_lowercase();
    let base = if let Some(p) = lower.find("chipset family") {
        &n[..p + "chipset".len()]
    } else if let Some(p) = lower.find("chipset controller") {
        &n[..p + "chipset".len()]
    } else {
        n
    };
    let label = base.trim().to_string();
    if label.is_empty() {
        None
    } else {
        Some(label)
    }
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
}
