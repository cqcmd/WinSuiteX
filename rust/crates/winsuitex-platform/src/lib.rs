//! `winsuitex-platform` —— Win32 平台实现。
//!
//! **所有 `unsafe` 都收敛在本 crate**，对外只暴露 safe API。
//! 每个 unsafe block 都写明 SAFETY 契约。

pub mod audio;
pub mod chipset;
pub mod cpuid;
pub mod disk;
pub mod display;
pub mod edid;
pub mod gpu;
mod gpu_telemetry;
pub mod hid;
pub mod memory_wmi;
pub mod nic;
mod pci_gpu;
pub mod peripheral_db;
pub mod physical_disk;
pub mod pid_model_table;
pub mod registry;
pub mod smbios;
/// 仅用于真机排障：`WSX_TRACE=1` 时给每个采集源加堆完整性校验，见模块文档。
pub mod sourcetrace;
pub mod system;
pub mod usb;
/// Win7 / Server 2008 R2 兼容垫片：把 `combase.dll!CoTaskMemFree` 重定向到
/// `ole32.dll`，去掉 Win8+ 才有的 DLL 依赖。仅 `--cfg winsuitex_win7`
/// （即 `x86_64-win7-windows-msvc` 构建）时编译，Win10/11 构建行为不变。
#[cfg(winsuitex_win7)]
pub mod win7_compat;

pub use audio::AudioSource;
pub use disk::DiskSource;
pub use display::DisplaySource;
pub use gpu::GpuSource;
pub use hid::merge_input_devices;
pub use hid::HidSource;
pub use memory_wmi::MemoryWmiSource;
pub use nic::NicSource;
pub use physical_disk::{map_volumes_to_disks, PhysicalDiskSource};
pub use registry::WinRegistry;
pub use smbios::SmbiosSource;
pub use system::SystemSource;
pub use usb::UsbSource;

/// 字符串 → 以 NUL 结尾的 UTF-16（Win32 API 通用）。
pub(crate) fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// UTF-16 缓冲区 → String（遇 NUL 截断）。
pub(crate) fn wide_to_string(buf: &[u16]) -> Option<String> {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let s = String::from_utf16_lossy(&buf[..end]);
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// 组装默认数据源集合（与 C++ 版 `HW_Collect` 的采集范围对齐）。
///
/// 顺序有意义：`DiskSource` 必须先跑出分区，`map_volumes_to_disks` 才能在最后
/// 给每个盘符补上 `PhysicalDriveN`。这个"两阶段"是刻意的 —— 映射需要 IOCTL，
/// 放在采集末尾可避免与前面的枚举相互干扰，也便于单独跳过。
pub fn default_sources() -> Vec<Box<dyn winsuitex_core::traits::HardwareSource>> {
    // 统一套一层排障装饰器（sourcetrace）：只有 WSX_TRACE=1 时才工作，
    // 未开启时 `TracedSource::collect` 直接透传，行为与包装前完全一致。
    fn traced<S: winsuitex_core::traits::HardwareSource + 'static>(
        s: S,
    ) -> Box<dyn winsuitex_core::traits::HardwareSource> {
        Box::new(sourcetrace::TracedSource::new(s))
    }

    vec![
        traced(SmbiosSource),
        traced(MemoryWmiSource),
        traced(SystemSource::new()),
        traced(GpuSource),
        traced(DisplaySource),
        traced(NicSource),
        traced(DiskSource),
        traced(PhysicalDiskSource),
        traced(UsbSource),
        traced(HidSource),
        traced(AudioSource),
    ]
}

/// 采集后处理：把盘符映射到 `PhysicalDriveN`。
///
/// 独立于 `default_sources`，因为它是**对已有结果的增强**而非新数据源，
/// 且失败时保持 `None` 即可，不该产生诊断项。
pub fn link_volumes_to_disks(volumes: &mut [winsuitex_core::model::Volume]) {
    map_volumes_to_disks(volumes);
}
