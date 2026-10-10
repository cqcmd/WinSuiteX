//! SMBIOS 数据源：从固件表取原始字节，交给 core 的纯函数解析。
//!
//! 本文件是 platform 层，只负责「取数据」；**解析逻辑全在 core::smbios**（可单测）。

use windows::Win32::System::SystemInformation::{GetSystemFirmwareTable, FIRMWARE_TABLE_PROVIDER};
use winsuitex_core::error::{Error, Result};
use winsuitex_core::model::BoardInfo;
use winsuitex_core::smbios;
use winsuitex_core::traits::{CollectContext, HardwareSource};

/// 'RSMB' —— SMBIOS 固件表提供器签名（多字符常量，与 C++ 版 'RSMB' 一致）。
const RSMB: FIRMWARE_TABLE_PROVIDER = FIRMWARE_TABLE_PROVIDER(0x5253_4D42);

/// 读取原始 SMBIOS 表字节。
///
/// 两段式调用：先传 `None` 取所需大小，再传 buffer 取数据。
pub fn read_raw() -> Result<Vec<u8>> {
    // SAFETY: RSMB 是合法的固件表提供器签名；buffer 由本函数独占分配，
    // 长度来自第一次调用返回的所需大小，写入后按实际返回长度截断。
    unsafe {
        let need = GetSystemFirmwareTable(RSMB, 0, None);
        if need == 0 {
            return Err(Error::collect(
                "smbios",
                "GetSystemFirmwareTable 查询大小失败（可能无 SMBIOS 支持）",
            ));
        }
        let mut buf = vec![0u8; need as usize];
        let got = GetSystemFirmwareTable(RSMB, 0, Some(&mut buf));
        if got == 0 {
            return Err(Error::collect("smbios", "GetSystemFirmwareTable 读取失败"));
        }
        buf.truncate(got as usize);
        Ok(buf)
    }
}

/// BIOS + 主板 + CPU + 内存条，全部来自 SMBIOS（无需任何驱动）。
pub struct SmbiosSource;

impl HardwareSource for SmbiosSource {
    fn name(&self) -> &'static str {
        "smbios"
    }

    fn collect(&self, ctx: &mut CollectContext) -> Result<()> {
        let raw = read_raw()?;
        let data = smbios::parse(&raw).map_err(|e| Error::collect("smbios", e.to_string()))?;

        // 主板 + BIOS 合成一个 BoardInfo
        let board = data.board.unwrap_or_default();
        let bios = data.bios.unwrap_or_default();
        if board.vendor.is_some() || board.product.is_some() || bios.vendor.is_some() {
            ctx.set_board(BoardInfo {
                vendor: board.vendor,
                product: board.product,
                serial: board.serial,
                // 芯片组不在 SMBIOS 表里：用 SetupAPI 枚举 PnP 设备按 "Chipset" 命中。
                // 未命中/失败返回 None（诚实留白），绝不编造。
                chipset: crate::chipset::detect_chipset(),
                bios_vendor: bios.vendor,
                bios_version: bios.version,
                bios_date: bios.date,
            });
        }

        if !data.cpus.is_empty() {
            let mut cpus = data.cpus;
            // 微架构代号来自 CPUID 寄存器，**不在 SMBIOS 表里**，所以在这里合并进 CPU 条目。
            // 同一台机器的物理 CPU 型号一致，取一次复用即可；识别不出则保持 None（不编造）。
            if let Some(code) = crate::cpuid::cpu_codename() {
                for c in &mut cpus {
                    c.codename = Some(code.clone());
                }
            }
            // 部分老旧主板 SMBIOS 的 Thread Count 与 Core Count 矛盾（线程数 < 核数），
            // 用操作系统逻辑处理器总数交叉校验补全（见 `sanitize_thread_counts`）。
            smbios::sanitize_thread_counts(&mut cpus, crate::system::logical_processors());
            ctx.set_cpus(cpus);
        }
        if !data.mem_sticks.is_empty() {
            ctx.set_memory_sticks(data.mem_sticks);
        }
        Ok(())
    }
}
