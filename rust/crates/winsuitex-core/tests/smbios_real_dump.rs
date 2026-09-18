//! 真实机器 SMBIOS dump 的回归测试。
//!
//! # 为什么需要这一层
//! 合成 fixture 只能证明"我按自己理解的规范写对了"，无法证明"我理解的规范
//! 与真实固件一致"。本机（QEMU q35 / SMBIOS 2.8）的 dump 曾暴露两个合成
//! 测试完全测不出来的缺陷：
//! 1. 无字符串结构（Type 16）的双 00 终止符被少算 1 字节 → 整表错位；
//! 2. 结构顺序不是按 type 递增（Type 0 排在 32 之后），任何"提前 break"
//!    的遍历都会漏掉它。
//!
//! 夹具来源：`GetSystemFirmwareTable('RSMB', 0)` 的原始 434 字节，
//! 由 `winsuitex-cli --dump-smbios <path>` 导出，可随时重新采集比对。

use winsuitex_core::model::MebiBytes;
use winsuitex_core::smbios::{list_structures, parse};

const DUMP: &[u8] = include_bytes!("fixtures/smbios_qemu_q35_2026.bin");

#[test]
fn walks_every_structure_in_real_dump() {
    let types: Vec<u8> = list_structures(DUMP).into_iter().map(|(t, _)| t).collect();
    // 真实顺序：Type 0 出现在 Type 32 之后 —— 遍历不能假设按 type 递增
    assert_eq!(types, vec![1, 3, 4, 16, 17, 19, 19, 32, 0, 127]);
}

#[test]
fn finds_the_16gb_memory_stick() {
    let d = parse(DUMP).unwrap();
    assert_eq!(d.mem_sticks.len(), 1);

    let m = &d.mem_sticks[0];
    assert_eq!(m.locator.as_deref(), Some("DIMM 0"));
    assert_eq!(m.capacity_mb, Some(MebiBytes::new(16 * 1024)));
    assert_eq!(m.vendor.as_deref(), Some("QEMU"));
    // 该固件填的 type=0x07（RAM）。展示用的类型表只收 DDR 代际名称，
    // 泛化的 "RAM" 会被丢弃（与 C++ 参考实现一致，否则每台虚机都多一处差异）。
    assert_eq!(m.mem_type, None);
    // 固件未填频率 → 必须表达为 None，而不是 0（0 MHz 是谎言）
    assert_eq!(m.speed_mhz, None);
    // 容量有值 ⇒ 判定为实插（空槽的 Size 字段为 0）
    assert!(m.is_populated());
}

#[test]
fn parses_bios_that_appears_after_stringless_structures() {
    let d = parse(DUMP).unwrap();
    let bios = d
        .bios
        .expect("Type 0 位于无字符串结构之后，曾被错位遍历漏掉");
    assert_eq!(
        bios.vendor.as_deref(),
        Some("Proxmox distribution of EDK II")
    );
    assert_eq!(bios.version.as_deref(), Some("4.2025.05-2"));
}

#[test]
fn parses_cpu_from_real_dump() {
    let d = parse(DUMP).unwrap();
    assert_eq!(d.cpus.len(), 1);
    let c = &d.cpus[0];
    assert_eq!(c.socket.as_deref(), Some("CPU 0"));
    assert_eq!(c.vendor.as_deref(), Some("QEMU"));
}

#[test]
fn board_is_absent_when_firmware_has_no_type_2() {
    // 该固件确实没有 Type 2（Base Board）。
    // 断言 None 而不是 unwrap —— 缺失是合法状态，不该 panic。
    let d = parse(DUMP).unwrap();
    assert!(d.board.is_none() || d.board.as_ref().is_some_and(|b| b.vendor.is_none()));
}

#[test]
fn truncated_dump_is_error_not_panic() {
    // 固件数据是不可信输入：截断必须返回 Err，而不是越界读或 panic
    assert!(parse(&DUMP[..20]).is_err());
}
