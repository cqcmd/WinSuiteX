//! 显卡信息源：DXGI 枚举适配器，取描述名与独立显存。
//!
//! 设计要点：
//! - 主路径用 `CreateDXGIFactory1` → `EnumAdapters1` → `GetDesc1`，与 C++ 版
//!   `CollectGpuVram` 同源；`Description` 是 UTF-16，需截 NUL 并 trim。
//! - 跳过 `DXGI_ADAPTER_FLAG_SOFTWARE`（"Microsoft Basic Render Driver" 这类
//!   WARP 占位），但**保留** "Microsoft Remote Display Adapter"——它并非 software
//!   标记，因此按 C++ 版行为正常显示。
//! - 降级链：DXGI 不可用 / 过滤后为空 → 退而包含 software 适配器；再空 → 退 GDI
//!   显示适配器名（无显存）。保证"至少拿到 1 个 GPU"且整体采集不失败。
//! - 单块适配器 `GetDesc1` 失败只跳过该块，不影响其它。

use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
};

use crate::wide_to_string;
use winsuitex_core::error::Result;
use winsuitex_core::model::{Gpu, GpuKind, MebiBytes};
use winsuitex_core::traits::{CollectContext, HardwareSource};

/// 显卡采集源（DXGI）。
pub struct GpuSource;

impl HardwareSource for GpuSource {
    fn name(&self) -> &'static str {
        "gpu.dxgi"
    }

    fn collect(&self, ctx: &mut CollectContext) -> Result<()> {
        // 三级兜底，顺序是有讲究的：
        //   1) DXGI 非软件适配器 —— 真实机器上的独显/核显，还能拿到显存；
        //   2) GDI 显示适配器 —— RDP/虚拟机场景（如 "Microsoft Remote Display Adapter"），
        //      比软件渲染器的名字更有信息量，也与 C++ 版口径一致；
        //   3) DXGI 软件适配器 —— 无显示会话时的最后退路（Basic Render Driver）。
        // 任一级失败都不让整个采集失败，只是显卡列表为空。
        //
        // 与 C++ 口径对齐的关键：indirect display driver（如向日葵 OrayIddDriver、
        // RDP 的 Microsoft Remote Display Adapter）在 DXGI「非软件适配器」枚举里
        // 不出现，但 GDI `EnumDisplayDevicesW` 能列出。因此这里把 GDI 发现、DXGI
        // 尚未包含的适配器补进来，而不是只在 DXGI 全空时才回落到 GDI。
        let mut gpus = dxgi_adapters(false);
        gpus = merge_gdi_into(gpus, gdi_adapters());
        if gpus.is_empty() {
            gpus = dxgi_adapters(true);
        }

        ctx.set_gpus(dedup_by_name(gpus));
        Ok(())
    }
}

/// 把 `gdi` 中发现、但 `dxgi` 里还没有的适配器追加进去（按名称不区分大小写去重）。
///
/// 真实显卡（如 "NVIDIA GeForce GTX 650"）在 DXGI 与 GDI 下名字一致，会去重掉；
/// 而 indirect display driver 只在 GDI 出现，会被保留下来，从而与 C++ 报告口径一致。
fn merge_gdi_into(mut dxgi: Vec<Gpu>, gdi: Vec<Gpu>) -> Vec<Gpu> {
    for g in gdi {
        if !dxgi.iter().any(|e| e.name.eq_ignore_ascii_case(&g.name)) {
            dxgi.push(g);
        }
    }
    dxgi
}

/// 按名称去重（对应 C++ 版 `GpuDup`）。
///
/// 同一台机器上 DXGI 可能报出多个**同名**软件适配器（本机就有两条
/// "Microsoft Basic Render Driver"），不去重会让报告里出现重复行。
fn dedup_by_name(gpus: Vec<Gpu>) -> Vec<Gpu> {
    let mut out: Vec<Gpu> = Vec::new();
    for g in gpus {
        if !out.iter().any(|e| e.name == g.name) {
            out.push(g);
        }
    }
    out
}

/// 枚举 DXGI 适配器。`include_software` 控制是否保留 `DXGI_ADAPTER_FLAG_SOFTWARE`。
fn dxgi_adapters(include_software: bool) -> Vec<Gpu> {
    // SAFETY: CreateDXGIFactory1 由 windows crate 处理引用计数，factory 在作用域末尾释放。
    let factory: IDXGIFactory1 = match unsafe { CreateDXGIFactory1() } {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };

    let mut out = Vec::new();
    let mut i = 0u32;
    loop {
        // SAFETY: EnumAdapters1 越界时返回 Err(DXGI_ERROR_NOT_FOUND)，即枚举结束。
        let adapter = match unsafe { factory.EnumAdapters1(i) } {
            Ok(a) => a,
            Err(_) => break,
        };
        i += 1;

        // SAFETY: GetDesc1 写入栈上 desc；adapter 有效。
        let desc = match unsafe { adapter.GetDesc1() } {
            Ok(d) => d,
            Err(_) => continue, // 单块失败不影响其它
        };

        let is_software = (desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32)) != 0;
        if is_software && !include_software {
            continue;
        }

        let name = wide_to_string(&desc.Description)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "未知显卡".to_string());

        // DedicatedVideoMemory 单位为字节（usize）；右移 20 得到 MiB。
        // 0 视为"拿不到" → None，禁止用 0 充数。
        let vram_mb = if desc.DedicatedVideoMemory == 0 {
            None
        } else {
            Some(MebiBytes::new((desc.DedicatedVideoMemory >> 20) as u64))
        };

        // DXGI 的 software 标记是 WARP/Basic Render 的权威判据；非软件适配器再用名称
        // 复校一次，防止漏网的间接显示驱动被误判为物理显卡。
        let kind = if is_software {
            GpuKind::Software
        } else {
            classify_gpu_kind(&name)
        };
        let vendor = gpu_board_vendor(desc.SubSysId, &name);
        out.push(Gpu {
            name,
            vram_mb,
            kind,
            telemetry: None,
            vendor,
        });
    }
    out
}

/// GDI 兜底：枚举显示适配器的 `DeviceString` 作为显卡名（无显存）。
/// 仅在 DXGI 完全不可用时调用，保证降级后仍有数据。
#[allow(clippy::field_reassign_with_default)]
fn gdi_adapters() -> Vec<Gpu> {
    use std::mem::size_of;
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayDevicesW, DISPLAY_DEVICEW, DISPLAY_DEVICE_ACTIVE,
    };

    let mut out = Vec::new();
    let mut i = 0u32;
    loop {
        let mut dd = DISPLAY_DEVICEW::default();
        dd.cb = size_of::<DISPLAY_DEVICEW>() as u32;
        // SAFETY: dd.cb 已初始化；设备名传 NULL 枚举适配器。
        if !unsafe { EnumDisplayDevicesW(PCWSTR::null(), i, &mut dd, 0) }.as_bool() {
            break;
        }
        i += 1;

        // 只取挂在桌面的适配器（对齐 C++ `CollectGpu`，避免收进禁用/幻影卡）。
        if !dd.StateFlags.contains(DISPLAY_DEVICE_ACTIVE) {
            continue;
        }

        if let Some(name) = wide_to_string(&dd.DeviceString)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            if !is_placeholder_gpu_adapter(&name) {
                out.push(Gpu {
                    name: name.clone(),
                    vram_mb: None,
                    kind: GpuKind::Physical,
                    telemetry: None,
                    vendor: gpu_vendor_from_name(&name),
                });
            }
        }
    }
    out
}

/// 占位显卡适配器判定（对应 C++ `IsGpuPlaceholderName`，hardware.cpp:781）。
///
/// 宽松匹配（大小写无关），覆盖中英文驱动描述与未来变体。
fn is_placeholder_gpu_adapter(name: &str) -> bool {
    if name.is_empty() {
        return true;
    }
    const PLACEHOLDERS: &[&str] = &["basic display", "basic render", "standard vga"];
    PLACEHOLDERS.iter().any(|p| name.to_lowercase().contains(p))
}

/// 按名称关键词判断 GPU 类型（非软件适配器路径）。
///
/// - 间接 / 虚拟显示驱动 → `Virtual`
/// - 软件渲染器 → `Software`
/// - 其它 → `Physical`
fn classify_gpu_kind(name: &str) -> GpuKind {
    let lower = name.to_lowercase();
    const VIRTUAL: &[&str] = &["orayidddriver", "remote display", "displaylink", "vnc"];
    const SOFTWARE: &[&str] = &["basic render", "warp"];
    if VIRTUAL.iter().any(|k| lower.contains(k)) {
        GpuKind::Virtual
    } else if SOFTWARE.iter().any(|k| lower.contains(k)) {
        GpuKind::Software
    } else {
        GpuKind::Physical
    }
}

/// 板卡品牌（AIB 厂商，如华硕/微星/七彩虹），与芯片厂（NVIDIA/AMD/Intel）不同。
///
/// 优先用 DXGI 子系统厂商 ID（`SubSysId` 低 16 位）查 PCI 表；查不到再用名称关键词兜底。
/// 仅收录 AIB 板卡厂商，不收录芯片厂（NVIDIA/AMD/Intel）与整机厂（Dell/Lenovo/HP），
/// 这样参考公版卡 / 核显不会误标成"品牌"。
fn gpu_board_vendor(subsys_id: u32, name: &str) -> Option<String> {
    // 子系统厂商 ID = SubSysId 低 16 位；高 16 位是子系统设备 ID。
    let subsys_vendor = (subsys_id & 0xFFFF) as u16;
    if let Some(brand) = pci_subsys_vendor_to_brand(subsys_vendor) {
        return Some(brand.to_string());
    }
    gpu_vendor_from_name(name)
}

/// PCI 子系统厂商 ID → 板卡品牌英文名（与报告层 `vendor_zh` 的中文映射键一致）。
///
/// 来源：AMD 官方 GPU-55 子系统厂商 ID 清单 + OpenRGB `pci_ids.h`，均已核对。
fn pci_subsys_vendor_to_brand(id: u16) -> Option<&'static str> {
    const MAP: &[(u16, &str)] = &[
        (0x1043, "ASUS"),
        (0x1462, "MSI"),
        (0x1458, "GIGABYTE"),
        (0x1849, "ASRock"),
        (0x7377, "Colorful"),
        (0x19DA, "Zotac"),
        (0x1DA2, "Sapphire"),
        (0x174B, "Sapphire"), // 旧款
        (0x1682, "XFX"),
        (0x148C, "PowerColor"),
        (0x3842, "EVGA"),
        (0x1569, "Palit"),
        (0x10B0, "Gainward"),
        (0x196E, "PNY"),
        (0x1B4C, "Galax"),
        (0x196D, "Club 3D"),
        (0x1545, "VisionTek"),
    ];
    MAP.iter().find(|(k, _)| *k == id).map(|(_, v)| *v)
}

/// 从显卡名里找品牌关键词（兜底，覆盖名称直接带品牌的卡，如 "MSI GeForce RTX 4070"）。
fn gpu_vendor_from_name(name: &str) -> Option<String> {
    const KW: &[(&str, &str)] = &[
        ("colorful", "Colorful"),
        ("七彩虹", "Colorful"),
        ("asus", "ASUS"),
        ("华硕", "ASUS"),
        ("msi", "MSI"),
        ("微星", "MSI"),
        ("gigabyte", "GIGABYTE"),
        ("技嘉", "GIGABYTE"),
        ("asrock", "ASRock"),
        ("华擎", "ASRock"),
        ("zotac", "Zotac"),
        ("索泰", "Zotac"),
        ("sapphire", "Sapphire"),
        ("蓝宝石", "Sapphire"),
        ("xfx", "XFX"),
        ("讯景", "XFX"),
        ("powercolor", "PowerColor"),
        ("撼讯", "PowerColor"),
        ("evga", "EVGA"),
        ("palit", "Palit"),
        ("同德", "Palit"),
        ("gainward", "Gainward"),
        ("耕升", "Gainward"),
        ("galax", "Galax"),
        ("影驰", "Galax"),
        ("inno3d", "Inno3D"),
        ("映众", "Inno3D"),
        ("pny", "PNY"),
    ];
    let lower = name.to_lowercase();
    KW.iter()
        .find(|(k, _)| lower.contains(k))
        .map(|(_, v)| v.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        classify_gpu_kind, gpu_board_vendor, gpu_vendor_from_name, is_placeholder_gpu_adapter,
        merge_gdi_into,
    };
    use crate::GpuSource;
    use winsuitex_core::model::{Gpu, GpuKind};
    use winsuitex_core::traits::{CollectContext, HardwareSource};

    #[test]
    fn merge_gdi_keeps_virtual_display_and_dedups_real_gpu() {
        // DXGI 报出真实显卡；GDI 报出同一真实显卡（名字一致）+ 虚拟 indirect
        // display driver（OrayIddDriver）。合并后应 = [真实显卡, OrayIddDriver]，
        // 真实显卡不重复，且与 C++ 报告口径一致。
        let dxgi = vec![Gpu {
            name: "NVIDIA GeForce GTX 650".into(),
            vram_mb: Some(winsuitex_core::model::MebiBytes::new(1024)),
            kind: GpuKind::Physical,
            telemetry: None,
            vendor: None,
        }];
        let gdi = vec![
            Gpu {
                name: "NVIDIA GeForce GTX 650".into(),
                vram_mb: None,
                kind: GpuKind::Physical,
                telemetry: None,
                vendor: None,
            },
            Gpu {
                name: "OrayIddDriver Device".into(),
                vram_mb: None,
                kind: GpuKind::Virtual,
                telemetry: None,
                vendor: None,
            },
        ];
        let merged = merge_gdi_into(dxgi, gdi);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].name, "NVIDIA GeForce GTX 650");
        assert_eq!(merged[1].name, "OrayIddDriver Device");
    }

    #[test]
    fn merge_gdi_case_insensitive_dedup() {
        // 大小写不一致也应视为同一显卡去重。
        let dxgi = vec![Gpu {
            name: "Intel(R) HD Graphics".into(),
            vram_mb: None,
            kind: GpuKind::Physical,
            telemetry: None,
            vendor: None,
        }];
        let gdi = vec![Gpu {
            name: "intel(R) HD GRAPHICS".into(),
            vram_mb: None,
            kind: GpuKind::Physical,
            telemetry: None,
            vendor: None,
        }];
        let merged = merge_gdi_into(dxgi, gdi);
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn gdi_placeholder_basic_display_adapter_is_skipped() {
        // 微软基础显示驱动占位卡不应被当成物理 GPU 纳入。
        assert!(is_placeholder_gpu_adapter(
            "Microsoft Basic Display Adapter"
        ));
        assert!(is_placeholder_gpu_adapter(
            "Microsoft Basic Display Adapter (WDDM)"
        ));
        // 真实 GPU 名称不受影响。
        assert!(!is_placeholder_gpu_adapter("NVIDIA GeForce GTX 650"));
        assert!(!is_placeholder_gpu_adapter("Intel(R) HD Graphics"));
    }

    #[test]
    fn classify_gpu_kind_distinguishes_physical_virtual_software() {
        // 真实独显 / 核显 → Physical。
        assert_eq!(
            classify_gpu_kind("NVIDIA GeForce RTX 4070"),
            GpuKind::Physical
        );
        assert_eq!(classify_gpu_kind("Intel(R) HD Graphics"), GpuKind::Physical);
        assert_eq!(classify_gpu_kind("AMD Radeon RX 7900"), GpuKind::Physical);
        // 间接 / 虚拟显示驱动 → Virtual。
        assert_eq!(classify_gpu_kind("OrayIddDriver Device"), GpuKind::Virtual);
        assert_eq!(
            classify_gpu_kind("Microsoft Remote Display Adapter"),
            GpuKind::Virtual
        );
        assert_eq!(
            classify_gpu_kind("DisplayLink USB Device"),
            GpuKind::Virtual
        );
        // 软件渲染器 → Software。
        assert_eq!(
            classify_gpu_kind("Microsoft Basic Render Driver"),
            GpuKind::Software
        );
        assert_eq!(classify_gpu_kind("WARP Renderer"), GpuKind::Software);
    }

    #[test]
    fn gpu_brand_from_subsys_id() {
        // 微星 RTX 4070：芯片厂 NVIDIA(0x10DE)，但子系统厂商 ID 低 16 位 = 0x1462(MSI)。
        assert_eq!(
            gpu_board_vendor(0x1462, "NVIDIA GeForce RTX 4070"),
            Some("MSI".into())
        );
        // 七彩虹：SubSysId 低 16 位 = 0x7377。
        assert_eq!(
            gpu_board_vendor(0x7377, "NVIDIA GeForce RTX 4070"),
            Some("Colorful".into())
        );
        // 华硕：0x1043。
        assert_eq!(
            gpu_board_vendor(0x1043, "AMD Radeon RX 7900"),
            Some("ASUS".into())
        );
        // 参考公版卡（子系统厂商 = 芯片厂 0x10DE）不在 AIB 表里 → 名称兜底也命中不了 → None。
        assert_eq!(gpu_board_vendor(0x10DE, "NVIDIA GeForce RTX 4070"), None);
    }

    #[test]
    fn gpu_brand_from_name_fallback() {
        // 名称直接带品牌的卡，即使没有 SubSysId 也能识别。
        assert_eq!(
            gpu_vendor_from_name("MSI GeForce RTX 4070 Gaming X"),
            Some("MSI".into())
        );
        assert_eq!(
            gpu_vendor_from_name("七彩虹 iGame RTX 4070"),
            Some("Colorful".into())
        );
        // 无品牌关键词 → None。
        assert_eq!(gpu_vendor_from_name("NVIDIA GeForce RTX 4070"), None);
    }

    #[test]
    fn real_machine_collect() {
        let src = GpuSource;
        let mut ctx = CollectContext::new();
        src.collect(&mut ctx).expect("gpu collect must return Ok");

        let (info, _vols) = ctx.into_parts();
        assert!(!info.gpus.is_empty(), "expected >=1 GPU on this machine");
        for g in &info.gpus {
            eprintln!("GPU: name={} vram_mb={:?}", g.name, g.vram_mb);
        }
    }
}
