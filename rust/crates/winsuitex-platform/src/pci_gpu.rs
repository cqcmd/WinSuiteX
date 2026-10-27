//! PCI 显示适配器识别（**无驱动兜底**，QA 审核 + Z.W. 2026-10-27 立项）。
//!
//! ## 问题
//! 显卡识别主路径走 DXGI：没装驱动时只会报 "Microsoft 基本显示适配器 /
//! Standard VGA"，拿不到真实型号——网吧无盘重灌后没装显卡驱动的机器，
//! 资产盘点认不出是 GT 730 还是 GT 610。
//!
//! ## 原理
//! 显卡的 PCI 硬件 ID（`PCI\VEN_10DE&DEV_0633&SUBSYS_…`）**烧在芯片里，与驱动
//! 无关**。SetupAPI 枚举 Display 类（GUID_DEVCLASS_DISPLAY）的 PRESENT 设备读
//! HardwareID，查内置表 [`PCI_GPU_TABLE`]（由 `tools/gen_pci_gpu_table.py` 从
//! pci.ids 生成，N/A/I 三家 2579 条）即得真实型号；SUBSYS 的板卡厂商复用
//! gpu.rs 的 `pci_subsys_vendor_to_brand`。
//!
//! ## 诚实边界
//! - 表里没有的 DEV：显示 `NVIDIA (PCI 10DE:xxxx)`，绝不猜型号；
//! - 非三家芯片（ASPEED/Matrox 等 BMC）：不进本表，名称原本就不是占位串，
//!   走不到本模块；

use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
    SetupDiGetDeviceRegistryPropertyW, DIGCF_PRESENT, GUID_DEVCLASS_DISPLAY, HDEVINFO,
    SPDRP_HARDWAREID, SP_DEVINFO_DATA,
};

include!("pci_gpu_table.rs");

/// 从 PCI Display 类枚举出的一个真实显示控制器。
pub(crate) struct PciGpuDevice {
    pub vendor_id: u16,
    pub device_id: u16,
    /// SUBSYS 低 16 位 = 板卡（AIB）厂商 ID，用于复用 gpu.rs 的板卡品牌映射。
    pub subsys_vendor_id: Option<u16>,
}

/// 解析 HardwareID 首串 → (vendor, device, subsys_vendor)。
///
/// 首串形如 `PCI\VEN_10DE&DEV_0633&SUBSYS_00081043&REV_A1`（SUBSYS 可缺省）。
/// SUBSYS 8 位 hex = 高 4 位子系统设备 + 低 4 位子系统厂商（与 DXGI `SubSysId`
/// 低 16 位同义，正好喂给 `pci_subsys_vendor_to_brand`）。
fn parse_hwid(hwid: &str) -> Option<(u16, u16, Option<u16>)> {
    let up = hwid.to_ascii_uppercase();
    let ven = hex4_after(&up, "VEN_")?;
    let dev = hex4_after(&up, "DEV_")?;
    let subsys = up
        .split('&')
        .find_map(|seg| seg.strip_prefix("SUBSYS_"))
        .and_then(|s| s.get(..8.min(s.len())))
        // 畸形串防御：SUBSYS 不足 4 位 hex（如 "SUBSYS_10"）时切片下溢会 panic，
        // 直接按"无子系统厂商"处理（2026-10-27 自查）。
        .and_then(|s| {
            if s.len() >= 4 {
                u16::from_str_radix(&s[s.len() - 4..], 16).ok()
            } else {
                None
            }
        });
    Some((ven, dev, subsys))
}

/// 在串里找 `prefix` 后的前 4 位 hex（VEN_/DEV_/SUBSYS 后缀各出现一次，
/// VEN_ 嵌在首段 `PCI\VEN_xxxx` 中段，不能按 `&` 分段后 strip 前缀）。
fn hex4_after(s: &str, prefix: &str) -> Option<u16> {
    let rest = s.split(prefix).nth(1)?;
    let hex: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() < 4 {
        return None;
    }
    u16::from_str_radix(&hex[..4], 16).ok()
}

/// 从 pci.ids 设备名取展示名。表里已在生成期做过方括号提取，这里只兜底空串。
fn table_name(vendor_id: u16, device_id: u16) -> Option<&'static str> {
    PCI_GPU_TABLE
        .binary_search_by_key(&(vendor_id, device_id), |&(v, d, _)| (v, d))
        .ok()
        .map(|i| PCI_GPU_TABLE[i].2)
}

/// 芯片厂商显示名（表外 ID 返回 None，调用方自行决定降级文案）。
pub(crate) fn chip_vendor_name(vendor_id: u16) -> Option<&'static str> {
    match vendor_id {
        0x10DE => Some("NVIDIA"),
        0x1002 => Some("AMD"),
        0x8086 => Some("Intel"),
        _ => None,
    }
}

/// 表命中 → 型号；未命中 → `芯片厂商 (PCI ven:dev)` 诚实降级；表外芯片 → None。
pub(crate) fn identify(vendor_id: u16, device_id: u16) -> Option<String> {
    if let Some(name) = table_name(vendor_id, device_id) {
        return Some(name.to_string());
    }
    chip_vendor_name(vendor_id)
        .map(|v| format!("{v} 显示适配器 (PCI {vendor_id:04X}:{device_id:04X})"))
}

/// 枚举 PCI Display 类 PRESENT 设备。任何失败返回空 Vec（调用方保持原数据）。
pub(crate) fn display_devices() -> Vec<PciGpuDevice> {
    // SAFETY: 枚举 PRESENT Display 类设备，返回有效 HDEVINFO（失败转空）。
    let dev_info = match unsafe {
        SetupDiGetClassDevsW(Some(&GUID_DEVCLASS_DISPLAY), None, None, DIGCF_PRESENT)
    } {
        Ok(h) => h,
        Err(_) => return Vec::new(),
    };
    // SAFETY: dev_info 有效；Drop 时释放（同 usb.rs 的 DevInfoGuard 纪律）。
    let _guard = HandleGuard(dev_info);

    let mut did = SP_DEVINFO_DATA {
        cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
        ..Default::default()
    };
    let mut out = Vec::new();
    let mut idx = 0u32;
    loop {
        // SAFETY: did.cbSize 已初始化；返回 Err 表示枚举结束。
        if unsafe { SetupDiEnumDeviceInfo(dev_info, idx, &mut did) }.is_err() {
            break;
        }
        idx += 1;

        // HardwareID 是 REG_MULTI_SZ，get_first_hwid 取首串（规范形态即
        // `PCI\VEN_…&DEV_…&SUBSYS_…&REV_…`）。
        let Some(hw) = first_hwid(dev_info, &did) else {
            continue;
        };
        let Some((vendor_id, device_id, subsys_vendor_id)) = parse_hwid(&hw) else {
            continue;
        };
        out.push(PciGpuDevice {
            vendor_id,
            device_id,
            subsys_vendor_id,
        });
    }
    out
}

/// 取 SPDRP_HARDWAREID 的首串（REG_MULTI_SZ 首个 NUL 前）。
///
/// 两步模式先问所需字节数再分配（同 usb.rs `get_dev_prop`，防长属性被截断丢弃）。
fn first_hwid(dev_info: HDEVINFO, did: &SP_DEVINFO_DATA) -> Option<String> {
    let mut required = 0u32;
    // SAFETY: 首步传 None 缓冲区仅查询字节数；错误由 required==0 兜住。
    let _ = unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            dev_info,
            did,
            SPDRP_HARDWAREID,
            None,
            None,
            Some(&mut required),
        )
    };
    if required == 0 {
        return None;
    }
    let mut buf = vec![0u8; required as usize];
    // SAFETY: buf 恰好 required 字节；成功后含 NUL 结尾的 multi-sz。
    unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            dev_info,
            did,
            SPDRP_HARDWAREID,
            None,
            Some(&mut buf),
            None,
        )
    }
    .ok()?;
    let bytes = &buf;
    let end = bytes
        .windows(2)
        .position(|w| w == [0, 0])
        .map(|p| p + 1)
        .unwrap_or(bytes.len());
    let wide: Vec<u16> = bytes[..end]
        .chunks_exact(2)
        .map(|c| u16::from_ne_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    if wide.is_empty() {
        return None;
    }
    Some(String::from_utf16_lossy(&wide))
}

/// HDEVINFO RAII 守卫（同 usb.rs）。
struct HandleGuard(HDEVINFO);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        // SAFETY: self.0 是有效 HDEVINFO，只此一处释放。
        let _ = unsafe { SetupDiDestroyDeviceInfoList(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hwid_extracts_ven_dev_subsys() {
        let (v, d, s) =
            parse_hwid("PCI\\VEN_10DE&DEV_0633&SUBSYS_00081043&REV_A1").expect("应解析成功");
        assert_eq!(v, 0x10DE);
        assert_eq!(d, 0x0633);
        assert_eq!(s, Some(0x1043)); // 低 16 位 = 板卡厂商（ASUS）
                                     // SUBSYS 缺省（无驱动早期枚举可能只有 VEN/DEV）。
        let (v, d, s) = parse_hwid("PCI\\VEN_8086&DEV_0102").expect("应解析成功");
        assert_eq!((v, d, s), (0x8086, 0x0102, None));
        // 畸形 SUBSYS（不足 4 位 hex）→ 不 panic，按无子系统处理。
        assert_eq!(
            parse_hwid("PCI\\VEN_10DE&DEV_0633&SUBSYS_10").map(|(_, _, s)| s),
            Some(None)
        );
        // 非 PCI 串 → None。
        assert!(parse_hwid("ROOT\\BASICDISPLAY").is_none());
        assert!(parse_hwid("PCI\\VEN_ZZZZ").is_none());
    }

    #[test]
    fn table_lookup_hits_real_devices() {
        // 表内命中：GT 220（PRINT 机真实案例）/ GTX 1080 / Intel 核显 / AMD。
        assert_eq!(identify(0x10DE, 0x0633).as_deref(), Some("GeForce GT 220"));
        assert_eq!(
            identify(0x10DE, 0x1B80).as_deref(),
            Some("GeForce GTX 1080")
        );
        // 表外同厂 DEV：诚实降级，绝不猜型号。
        assert_eq!(
            identify(0x10DE, 0xFFFF).as_deref(),
            Some("NVIDIA 显示适配器 (PCI 10DE:FFFF)")
        );
        // 表外厂商：None（ASPEED/Matrox 等 BMC 不属本模块射程）。
        assert!(identify(0x1A03, 0x2000).is_none());
    }
}
