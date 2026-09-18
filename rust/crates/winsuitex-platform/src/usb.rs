//! USB 外设采集（Win32 SetupAPI）。
//!
//! 对齐 C++ `CollectAudioUsb`（hardware.cpp:1185-1233）的设备树遍历与过滤逻辑，
//! 但用 `Vec` 替代定长数组、用 `Option<String>` 替代空串哨兵、用 RAII guard
//! 管理 `HDEVINFO` 生命周期，杜绝手动 `SetupDiDestroyDeviceInfoList` 漏调用。
//!
//! 收录类别（按 C++ 顺序）：
//! - 枚举器 `USBSTOR`        → class = "USBSTOR"（U 盘 / 移动硬盘）
//! - 设备类 `HIDClass`       → class = "HID"（键鼠等）
//! - 设备类 `Image`/`Camera` → class = 原类名（摄像头 / 图像设备）
//!
//! 其它设备不收录。

use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
    SetupDiGetDeviceRegistryPropertyW, DIGCF_ALLCLASSES, DIGCF_PRESENT, HDEVINFO,
    SETUP_DI_REGISTRY_PROPERTY, SPDRP_DEVICEDESC, SPDRP_ENUMERATOR_NAME, SP_DEVINFO_DATA,
};

use winsuitex_core::error::{Error, Result};
use winsuitex_core::model::UsbDevice;
use winsuitex_core::traits::{CollectContext, HardwareSource};

use crate::wide_to_string;

/// USB 外设数据源。
pub struct UsbSource;

impl HardwareSource for UsbSource {
    fn name(&self) -> &'static str {
        "usb"
    }

    fn collect(&self, ctx: &mut CollectContext) -> Result<()> {
        match collect_usb() {
            Ok(devices) => {
                ctx.set_usb(devices);
                Ok(())
            }
            Err(e) => Err(Error::collect("usb", e.to_string())),
        }
    }
}

/// 遍历所有 PRESENT 设备，仅收录可移动存储类 USB 外设（U 盘 / 移动硬盘）。
///
/// 对齐 C++ `CollectAudioUsb`（hardware.cpp:1185-1233）的 USBSTOR 分支：
/// - 枚举器 `USBSTOR` → U 盘 / 移动硬盘
///
/// 刻意**不含** `HIDClass`（键鼠走独立的「键盘 / 鼠标」段，产品串更准）与
/// `Image`/`Camera`（不在 USB 段范围）。保持 USB 段聚焦可移动存储、避免与键鼠段重复。
///
/// 过滤：SW 枚举器、`is_not_real_hw` 黑名单。
fn collect_usb() -> Result<Vec<UsbDevice>> {
    // SAFETY: 枚举 PRESENT 设备，返回有效 HDEVINFO（失败转 Err）。
    let dev_info =
        match unsafe { SetupDiGetClassDevsW(None, None, None, DIGCF_ALLCLASSES | DIGCF_PRESENT) } {
            Ok(h) => h,
            Err(e) => return Err(Error::collect("usb", e.to_string())),
        };
    // SAFETY: dev_info 有效；Drop 时释放。
    let _guard = DevInfoGuard(dev_info);

    let mut did = SP_DEVINFO_DATA {
        cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
        ..Default::default()
    };

    let mut out: Vec<UsbDevice> = Vec::new();
    let mut idx = 0u32;
    loop {
        // SAFETY: did.cbSize 已初始化；返回 false 表示枚举结束。
        let ok = unsafe { SetupDiEnumDeviceInfo(dev_info, idx, &mut did) }.is_ok();
        if !ok {
            break;
        }
        idx += 1;

        let desc = match get_dev_prop(dev_info, &did, SPDRP_DEVICEDESC) {
            Some(d) => d,
            None => continue,
        };
        let enumer = get_dev_prop(dev_info, &did, SPDRP_ENUMERATOR_NAME);

        // 与 C++ 一致的过滤：SW 枚举器 / 黑名单虚拟设备跳过。
        if enumer.as_deref() == Some("SW") {
            continue;
        }
        if is_not_real_hw(&desc) {
            continue;
        }

        // 仅收录 U 盘 / 移动硬盘（枚举器 = USBSTOR），并按名称去重。
        // 键鼠等 HID 外设走独立的「键盘 / 鼠标」段（HID API，产品串更准），
        // 摄像头/图像设备不在本段范围内 —— 保持 USB 段聚焦可移动存储、避免与键鼠段重复。
        if enumer.as_deref() == Some("USBSTOR") && !out.iter().any(|d: &UsbDevice| d.name == desc) {
            out.push(UsbDevice {
                name: desc,
                class: Some("USBSTOR".to_string()),
            });
        }
    }
    Ok(out)
}

/// `HDEVINFO` RAII 守卫。
struct DevInfoGuard(HDEVINFO);

impl Drop for DevInfoGuard {
    fn drop(&mut self) {
        // SAFETY: self.0 是有效的 HDEVINFO。
        let _ = unsafe { SetupDiDestroyDeviceInfoList(self.0) };
    }
}

/// 取设备注册表属性值（UTF-16 字符串），失败返回 None。
fn get_dev_prop(
    dev_info: HDEVINFO,
    did: &SP_DEVINFO_DATA,
    prop: SETUP_DI_REGISTRY_PROPERTY,
) -> Option<String> {
    // 两步模式：先以 None 缓冲区查询所需字节数，再按真实大小分配，
    // 避免固定 512 字节在属性更长时返回 ERROR_INSUFFICIENT_BUFFER 而整设备被丢弃。
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

/// 虚拟 / 软件 / 总线占位设备黑名单（与 C++ `IsNotRealHw` 对齐）。
const NOT_REAL_HW: &[&str] = &[
    "Audio Endpoint",
    "音频端点",
    "软件合成器",
    "Wavetable",
    "Microsoft GS",
    "Root Hub",
    "根集线器",
    "Generic USB Hub",
    "通用集线器",
    "USB Composite Device",
    "USB 复合设备",
    "系统设备",
];

/// 设备名是否命中黑名单（大小写不敏感子串匹配）。
fn is_not_real_hw(desc: &str) -> bool {
    NOT_REAL_HW.iter().any(|bad| contains_ci(desc, bad))
}

/// ASCII 大小写不敏感子串匹配，非 ASCII 原样比对。
fn contains_ci(haystack: &str, needle: &str) -> bool {
    let h: Vec<char> = haystack.chars().map(|c| c.to_ascii_lowercase()).collect();
    let n: Vec<char> = needle.chars().map(|c| c.to_ascii_lowercase()).collect();
    if n.is_empty() {
        return false;
    }
    h.windows(n.len()).any(|w| w == n.as_slice())
}

#[cfg(test)]
mod tests {
    use super::is_not_real_hw;
    use crate::UsbSource;
    use winsuitex_core::traits::{CollectContext, HardwareSource};

    #[test]
    fn collect_returns_ok_and_prints() {
        let src = UsbSource;
        let mut ctx = CollectContext::new();
        let r = src.collect(&mut ctx);
        assert!(r.is_ok(), "usb collect() 返回 Err: {:?}", r.err());

        let (info, _volumes) = ctx.into_parts();
        for d in &info.usb {
            eprintln!("USB name={:?} class={:?}", d.name, d.class);
        }
        // 虚拟机/无外设环境可能为空，不强制非空；本机基准应含 U 盘 / 移动硬盘等 USBSTOR 设备。
        eprintln!("USB 设备总数: {}", info.usb.len());
    }

    #[test]
    fn not_real_hw_blacklist_works() {
        assert!(is_not_real_hw("Audio Endpoint"));
        assert!(is_not_real_hw("USB Root Hub"));
        assert!(is_not_real_hw("Generic USB Hub"));
        assert!(is_not_real_hw("USB Composite Device"));
        assert!(!is_not_real_hw("USB 输入设备"));
        assert!(!is_not_real_hw("HID-compliant mouse"));
    }
}
