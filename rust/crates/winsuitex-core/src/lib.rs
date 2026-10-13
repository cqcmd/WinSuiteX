//! `winsuitex-core` —— 领域模型与 trait 定义。
//!
//! # 铁律
//! 本 crate **不依赖任何平台 API**（无 `windows` crate、无 `unsafe`）。
//! 平台实现放在 `winsuitex-platform`，通过 trait 对接。
//! 这条保证本 crate 的测试可在任意环境运行（CI 友好）。

pub mod error;
pub mod feature;
pub mod memory_channels;
pub mod model;
pub mod smbios;
pub mod traits;
pub mod vendor_zh;

pub use error::{CollectResult, Diagnostic, Error, Result};
pub use model::nic_vendor_from_desc;
pub use model::{auto_ip_tag, nic_ipv4_for_display, nic_should_show_auto_ip};
pub use model::{fmt_link_speed, normalize_bios_date, real_link_speed};
pub use model::{
    AudioDevice, BoardInfo, Cpu, Display, Feature, FeatureId, FeatureKind, Gpu, GpuTelemetry,
    HardwareInfo, MachineInfo, MebiBytes, MemStick, MemoryInfo, Nic, OsInfo, PhysicalDisk, RegRoot,
    RegValueKind, UsbDevice, Volume,
};
pub use traits::{collect_all, CollectContext, HardwareSource, Registry, ThreadAffinity};
pub use vendor_zh::{
    brand_display, brand_display_or, brand_parts, brand_zh, brand_zh_or, gpu_brand_for_display,
};
