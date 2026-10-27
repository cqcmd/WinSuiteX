//! SMBIOS 表解析 —— 【纯函数，无 unsafe、无平台依赖】
//!
//! # 为什么放 core 而不是 platform
//! 解析逻辑是 `&[u8] -> 结构` 的纯算法。放在 core 意味着：
//! - 可以用真实机器的 SMBIOS dump 做 fixture **单元测试**；
//! - 不需要管理员权限、不需要 Windows 环境即可验证。
//!
//! 对比 C++ 版 `hardware.cpp:105-263`：那里用 `#pragma pack(1)` 结构体 +
//! 柔性数组 + 直接转型读表尾字符串，**越界即 UB**。本实现全程用安全切片与
//! `get()`，越界 → 返回 `Err`，**把 UB 变成可处理的错误**。

use crate::model::{Cpu, MebiBytes, MemStick};

#[derive(Debug, thiserror::Error)]
pub enum SmbiosError {
    #[error("SMBIOS 数据过短（{0} 字节），至少需要 8 字节表头")]
    TooShort(usize),
    #[error("SMBIOS 声明长度 {declared} 超出实际数据 {actual}")]
    LengthMismatch { declared: usize, actual: usize },
}

/// BIOS 信息（Type 0）
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BiosData {
    pub vendor: Option<String>,
    pub version: Option<String>,
    pub date: Option<String>,
}

/// 主板信息（Type 2）
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BoardData {
    pub vendor: Option<String>,
    pub product: Option<String>,
    /// 主板序列号（Type 2 偏移 0x07，字符串索引）。
    pub serial: Option<String>,
}

/// 解析结果
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SmbiosData {
    pub bios: Option<BiosData>,
    pub board: Option<BoardData>,
    pub cpus: Vec<Cpu>,
    pub mem_sticks: Vec<MemStick>,
}

// ---- 安全读取助手（全部返回 Option，越界即 None）----
fn u8_at(d: &[u8], i: usize) -> Option<u8> {
    d.get(i).copied()
}
fn u16_le(d: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes([
        d.get(i).copied()?,
        d.get(i + 1).copied()?,
    ]))
}
fn u32_le(d: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        d.get(i).copied()?,
        d.get(i + 1).copied()?,
        d.get(i + 2).copied()?,
        d.get(i + 3).copied()?,
    ]))
}

/// 把字符串区切成若干字符串，返回 (字符串列表, 消耗字节数)。
///
/// # 规范约定（DSP0134 §6.1.3）
/// 格式化区之后紧跟字符串区：若干 `\0` 结尾的字符串，**整体以双 `\0` 结束**。
///
/// 关键陷阱（本项目踩过的真实 bug）：
/// - 有字符串时，最后一串**自身的 `\0` 就是"双 00"的第一个字节**，所以只再多 1 字节；
/// - **没有字符串时则是完整的 2 字节 `00 00`**。
///
/// 早期实现在无字符串时只消耗 1 字节 → 整表错位 1 字节。本机的 Type 16
/// （物理内存阵列）恰好无字符串，错位后把 `00 11` 当成 `type=0 len=17` 的假结构，
/// 遍历随即报废 —— 表现就是"内存条 0 根"。此函数必须保持双 00 语义。
fn split_strings(area: &[u8]) -> (Vec<&[u8]>, usize) {
    // 1) 定位终止符：第一处连续的 00 00
    let mut end = 0usize;
    while end + 1 < area.len() && !(area[end] == 0 && area[end + 1] == 0) {
        end += 1;
    }
    let consumed = (end + 2).min(area.len());

    // 2) 在 [0, end) 内切字符串（end 即最后一串的 \0；无字符串时 end == 0）
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < end {
        let start = i;
        while i < end && area[i] != 0 {
            i += 1;
        }
        out.push(&area[start..i]);
        i += 1; // 跳过该串的 \0
    }
    (out, consumed)
}

/// 遍历结构表，对每个结构回调 `(type, body, strings)`。
///
/// `parse` 与 `list_structures` 共用这一份遍历逻辑 —— 避免两处 walk 各自漂移。
/// 遇到非法长度/越界即静默停止（不 panic）：SMBIOS 是固件提供的数据，
/// 不可信输入不应让采集流程崩溃。
fn for_each_structure<F>(table: &[u8], mut f: F)
where
    F: FnMut(u8, &[u8], &[&[u8]]),
{
    let mut pos = 0usize;
    while pos + 4 <= table.len() {
        let typ = table[pos];
        let hdr_len = table[pos + 1] as usize;
        if hdr_len < 4 {
            break;
        }
        let body_end = pos + hdr_len;
        if body_end > table.len() {
            break;
        }
        let (strings, consumed) = split_strings(&table[body_end..]);
        f(typ, &table[pos..body_end], &strings);
        let next = body_end + consumed;
        if next <= pos {
            break; // 防死循环：consumed 为 0 时不再前进
        }
        pos = next;
    }
}

/// 按 1-based 索引取字符串。索引 0 或非数字返回 None。
fn str_at(strings: &[&[u8]], idx: u8) -> Option<String> {
    if idx == 0 {
        return None;
    }
    let raw = strings.get((idx as usize) - 1)?;
    // SMBIOS 规范要求字符串为 ASCII，但固件里确实存在乱填的 junk。
    // 真机实测（MSI B75MA-P45）：某根 Kingston 内存的 Part Number 是 18 字节
    // 非法 UTF-8。若用 from_utf8_lossy，报告里会出现 "??????????????????"，
    // 运维会以为工具坏了 —— 比留空更糟。
    //
    // 这里选择整串丢弃而不是"截断取前半段"：含非法字节的字符串几乎总是整串
    // junk，截半只会产出一个"看着像型号、其实是错的"值，做资产盘点时会误导。
    // 统一按"无此字段"处理，报告显示 "-"，诚实可辨。
    let s = std::str::from_utf8(raw).ok()?.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// 调试用：列出所有结构的 `(type, length)`，用于排查遍历错位。
///
/// 与 `parse` 共用同一份遍历逻辑，因此它的输出可以**直接代表**解析时看到的
/// 结构序列 —— 排查"某类型解析不到"时先看这里。
pub fn list_structures(raw: &[u8]) -> Vec<(u8, u8)> {
    let mut out = Vec::new();
    if let Ok(table) = table_of(raw) {
        for_each_structure(table, |typ, body, _| out.push((typ, body.len() as u8)));
    }
    out
}

/// 取出表体：跳过 8 字节头，并按声明长度截断。
fn table_of(raw: &[u8]) -> Result<&[u8], SmbiosError> {
    if raw.len() < 8 {
        return Err(SmbiosError::TooShort(raw.len()));
    }
    let declared = u32_le(raw, 4).unwrap_or(0) as usize;
    raw.get(8..8 + declared).ok_or(SmbiosError::LengthMismatch {
        declared,
        actual: raw.len().saturating_sub(8),
    })
}

/// 解析 `GetSystemFirmwareTable('RSMB', ...)` 返回的原始缓冲区。
///
/// 布局：`[0]=调用方法 [1]=主版本 [2]=次版本 [3]=DMI修订 [4..8]=表长度 [8..]=表数据`
pub fn parse(raw: &[u8]) -> Result<SmbiosData, SmbiosError> {
    let table = table_of(raw)?;
    let mut out = SmbiosData::default();

    for_each_structure(table, |typ, body, strings| match typ {
        0 => out.bios = Some(parse_bios(body, strings)),
        2 => out.board = Some(parse_board(body, strings)),
        4 => {
            if let Some(c) = parse_cpu(body, strings) {
                out.cpus.push(c);
            }
        }
        17 => {
            if let Some(m) = parse_mem(body, strings) {
                out.mem_sticks.push(m);
            }
        }
        _ => {}
    });

    Ok(out)
}

/// Type 0 —— BIOS Information
fn parse_bios(body: &[u8], strings: &[&[u8]]) -> BiosData {
    BiosData {
        vendor: u8_at(body, 0x04).and_then(|i| str_at(strings, i)),
        version: u8_at(body, 0x05).and_then(|i| str_at(strings, i)),
        date: u8_at(body, 0x08).and_then(|i| str_at(strings, i)),
    }
}

/// Type 2 —— Base Board Information
fn parse_board(body: &[u8], strings: &[&[u8]]) -> BoardData {
    BoardData {
        vendor: u8_at(body, 0x04).and_then(|i| str_at(strings, i)),
        product: u8_at(body, 0x05).and_then(|i| str_at(strings, i)),
        serial: u8_at(body, 0x07).and_then(|i| str_at(strings, i)),
    }
}

/// Type 4 —— Processor Information
///
/// 核心/线程数：优先用 SMBIOS 3.0+ 的 WORD 字段（0x2A / 0x2E），
/// 回退老版本的 BYTE 字段（0x23 / 0x25）—— 与 C++ 版策略一致。
fn parse_cpu(body: &[u8], strings: &[&[u8]]) -> Option<Cpu> {
    let socket = u8_at(body, 0x04).and_then(|i| str_at(strings, i));
    let vendor = u8_at(body, 0x07).and_then(|i| str_at(strings, i));
    let model = u8_at(body, 0x10).and_then(|i| str_at(strings, i));

    // SMBIOS 3.0+ 的 Core Count 2(0x2A) / Thread Count 2(0x2E) 是 WORD，
    // 按 DSP0134 §7.5，0xFFFF 表示「未知」。只过滤 0 而不排除 0xFFFF，
    // 会让固件的"我不知道"变成 65535 核 / 65535 线程出现在资产报告里。
    // C++ 参考实现 hardware.cpp 有 `c2 != 0xFFFF` 判断，Rust 移植时漏掉了。
    let word_count = |off: usize| -> Option<u32> {
        u16_le(body, off)
            .filter(|v| *v > 0 && *v != 0xFFFF)
            .map(u32::from)
    };

    let cores = word_count(0x2A).or_else(|| u8_at(body, 0x23).filter(|v| *v > 0).map(u32::from));

    let threads = word_count(0x2E).or_else(|| u8_at(body, 0x25).filter(|v| *v > 0).map(u32::from));

    let speed = u16_le(body, 0x16).filter(|v| *v > 0).map(u32::from);

    // 全空则视为未识别（不是有效 CPU 条目）
    if socket.is_none() && vendor.is_none() && model.is_none() {
        return None;
    }

    Some(Cpu {
        socket,
        model,
        vendor,
        codename: None,
        cores,
        threads,
        speed_mhz: speed,
    })
}

/// 校正每个 CPU 的线程数，并剔除「幽灵 Type4」占位条目。
///
/// 部分老旧主板（如 ASUS H61 系）的 SMBIOS Type 4 会把 `Thread Count` 报成
/// 单核线程数甚至「小于核数」，与 `Core Count` 矛盾。物理上线程数必须 ≥ 核数，
/// 出现这种情况属于固件数据损坏，不能直接信任。
///
/// **幽灵 Type4 剔除（对抗审查发现，2026-10-27）**：部分单路板固件会多吐一个
/// 占位 Type4（`model`/`vendor` 全缺且 `cores` 缺失或 0）。这种条目参与路数
/// 计数会把 OS 逻辑处理器均摊值稀释一半，恰好让瞒报（threads == cores）漏修；
/// 它本身渲染出来也是"0 核"占位行，毫无信息量 —— 直接从列表剔除。
///
/// 修正策略：
/// - 当 SMBIOS 线程数缺失，或 `< 核数`（矛盾）时，用操作系统逻辑处理器总数按
///   CPU 路数均摊（`单路即 = logical_processors`）作为更可靠的来源；
/// - **瞒报同样修正**（真实案例：ASUS P8H61 + i3-3220 为 2C/4T，BIOS 把
///   `Thread Count` 写成 2，`threads == cores` 不触发矛盾检查、原逻辑漏网）——
///   OS 逻辑处理器总数（`GetSystemInfo`）与 SMBIOS 线程数不符时，OS 为准；
/// - 修正**只向上**（`per_socket > 现值` 才改）：多路服务器 / 空槽占位 Type4
///   的脏数据只会让均摊值偏小，向下改会把正确值改坏；
/// - 均摊值仍 `< 核数` 时退化为「至少等于核数」；
/// - 已经合理的线程数原样保留。
///
/// 函数幂等（剔除后二次调用无幽灵可剔），可在采集端与渲染端各调用一次。
pub fn sanitize_thread_counts(cpus: &mut Vec<Cpu>, logical_processors: Option<u32>) {
    // 幽灵 Type4 剔除：无身份（model/vendor 全缺）或无核数（cores 缺失/0）的
    // 占位条目先移除，避免稀释路数均摊、也避免"0 核"占位行渲染进报告。
    cpus.retain(|c| {
        let no_core_count = c.cores.is_none_or(|n| n == 0);
        let no_identity = c.model.is_none() && c.vendor.is_none();
        !(no_core_count || no_identity)
    });
    if cpus.is_empty() {
        return;
    }
    let n = cpus.len() as u32;
    let per_socket = logical_processors.map(|lp| lp / n);
    for cpu in cpus.iter_mut() {
        let cores = cpu.cores.unwrap_or(0);
        let bad = match cpu.threads {
            Some(t) => t < cores,
            None => true,
        };
        if bad {
            if let Some(pt) = per_socket {
                if pt >= cores {
                    cpu.threads = Some(pt);
                    continue;
                }
            }
            if cores > 0 {
                cpu.threads = Some(cores);
            }
        } else if let (Some(pt), Some(t)) = (per_socket, cpu.threads) {
            // OS 总线程数大于 SMBIOS 报告值 → BIOS Type4 瞒报，向上修正。
            if pt > t && pt >= cores {
                cpu.threads = Some(pt);
            }
        }
    }
}

/// 内存类型码 → **用于展示**的名称（SMBIOS Type 17 offset 0x12，DSP0134 §7.18）
///
/// 只返回"有信息量"的代际名称，与 C++ 参考实现 `HwMemTypeToStr` 保持同一集合，
/// 保证报告可逐段 diff。`Other(0x01)/Unknown(0x02)/DRAM(0x03)/RAM(0x07)`
/// 这类泛化值返回 `None`（渲染时不输出），否则每台虚拟机都会多出一行无意义差异。
///
/// 取值严格按规范：`0x12=DDR / 0x13=DDR2 / 0x14=DDR2 FB-DIMM / 0x18=DDR3 /
/// 0x1A=DDR4 / 0x22=DDR5`。早期实现把 `0x14/0x15` 误标成 DDR/DDR2 已修正。
///
/// TODO: C++ 目前缺 `0x20 HBM / 0x21 HBM2 / 0x23 LPDDR5 / 0x24 HBM3`。
/// 新笔记本（LPDDR5）在 Rust 侧本可显示，为保持 diff 干净暂不启用；
/// 若要放开，需同时改 C++ `HwMemTypeToStr`，否则又是一处必然差异。
///
/// `pub` 的原因：它是**内存类型码的唯一真源**——SMBIOS 直采与 WMI
/// `Win32_PhysicalMemory.SMBIOSMemoryType`（回传的同样是 SMBIOS 原始字节）
/// 两条路径共用这一张表，避免两处各维护一份枚举而对不上。
pub fn mem_type_name(t: u8) -> Option<&'static str> {
    let s = match t {
        0x12 => "DDR",
        0x13 => "DDR2",
        0x14 => "DDR2 FB-DIMM",
        0x18 => "DDR3",
        0x19 => "FBD2",
        0x1A => "DDR4",
        0x1B => "LPDDR",
        0x1C => "LPDDR2",
        0x1D => "LPDDR3",
        0x1E => "LPDDR4",
        0x22 => "DDR5",
        _ => return None,
    };
    Some(s)
}

/// Type 17 —— Memory Device
fn parse_mem(body: &[u8], strings: &[&[u8]]) -> Option<MemStick> {
    let locator = u8_at(body, 0x10).and_then(|i| str_at(strings, i));
    let bank = u8_at(body, 0x11).and_then(|i| str_at(strings, i));
    let serial = u8_at(body, 0x18).and_then(|i| str_at(strings, i));
    let vendor = u8_at(body, 0x17).and_then(|i| str_at(strings, i));
    let part_no = u8_at(body, 0x1A).and_then(|i| str_at(strings, i));
    let mem_type = u8_at(body, 0x12)
        .and_then(mem_type_name)
        .map(str::to_string);

    // 容量：Size(0x0C) 是 WORD，能表达的最大值是 0x7FFF = 32767 MB，所以规范规定：
    //   - 0x0000 → 空槽（未安装）
    //   - 0xFFFF → 容量未知
    //   - 0x7FFF → 单条超过 32 GB 放不下，真实容量见 Extended Size(0x1C, DWORD, 单位 MB)
    //   - bit15=1 → 单位是 KB，低 15 位为数值
    // 0x7FFF 这一支最容易漏，且漏了不会报错 —— 只是把哨兵 32767 当成真实容量。
    // 真机 8×64GB DDR4 服务器曾全部被解析成 32767 MiB（合计 262136），而 OS 报告
    // total 是 524191 MiB，差整整一倍才暴露出来。Linux 内核
    // drivers/firmware/dmi_scan.c 的 dmi_memdev_size() 也是同样的三分支判定：
    //   size & 0x8000 → KB；size != 0x7FFF → MB；否则取 Extended Size。
    let size_word = u16_le(body, 0x0C).unwrap_or(0);
    let capacity_mb: Option<u32> = if size_word == 0xFFFF || size_word == 0x7FFF {
        // 0xFFFFFFFF 是"未知"哨兵值，不能当成 2 TB 内存条（C++ 侧同样排除）
        u32_le(body, 0x1C)
            .filter(|v| *v != 0 && *v != 0xFFFF_FFFF)
            .map(|v| v & 0x7FFF_FFFF)
    } else if size_word == 0 {
        None // 空槽
    } else if size_word & 0x8000 != 0 {
        let kb = (size_word & 0x7FFF) as u32;
        Some(kb / 1024).filter(|v| *v > 0)
    } else {
        Some(size_word as u32)
    };
    // 单位是 MiB —— 用 newtype 包住，报告层无法再把它当字节格式化
    let capacity_mb = capacity_mb.map(MebiBytes::from);

    // Configured Speed 优先（0x20），回退 Speed（0x15）
    let speed = u16_le(body, 0x20)
        .filter(|v| *v > 0)
        .or_else(|| u16_le(body, 0x15).filter(|v| *v > 0))
        .map(u32::from);

    if locator.is_none() && vendor.is_none() && part_no.is_none() && capacity_mb.is_none() {
        return None;
    }

    Some(MemStick {
        locator,
        bank,
        serial,
        part_no,
        vendor,
        mem_type,
        capacity_mb,
        speed_mhz: speed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_fixes_corrupt_thread_count() {
        // 真实案例：ASUS H61 主板把 Thread Count 报成 2，Core Count 为 4。
        // 系统逻辑处理器 = 8（4C/8T 开超线程），单路 → 每颗应为 8。
        let mut cpus = vec![Cpu {
            socket: Some("CPU1".into()),
            model: Some("Xeon E3-1230 V2".into()),
            vendor: Some("Intel".into()),
            codename: None,
            cores: Some(4),
            threads: Some(2), // 固件给的脏数据
            speed_mhz: Some(3300),
        }];
        sanitize_thread_counts(&mut cpus, Some(8));
        assert_eq!(cpus[0].threads, Some(8));
    }

    #[test]
    fn sanitize_keeps_valid_thread_count() {
        // 已经合理（threads >= cores）的保持不变，幂等。
        let mut cpus = vec![Cpu {
            socket: Some("CPU1".into()),
            model: Some("i7".into()),
            vendor: Some("Intel".into()),
            codename: None,
            cores: Some(8),
            threads: Some(16),
            speed_mhz: Some(3400),
        }];
        sanitize_thread_counts(&mut cpus, Some(16));
        assert_eq!(cpus[0].threads, Some(16));
    }

    #[test]
    fn sanitize_handles_missing_threads() {
        // SMBIOS 完全缺线程数时，用逻辑处理器均摊补全。
        let mut cpus = vec![Cpu {
            socket: Some("CPU1".into()),
            model: Some("Ryzen".into()),
            vendor: Some("AMD".into()),
            codename: None,
            cores: Some(6),
            threads: None,
            speed_mhz: None,
        }];
        sanitize_thread_counts(&mut cpus, Some(12));
        assert_eq!(cpus[0].threads, Some(12));
    }

    #[test]
    fn sanitize_fixes_underscored_thread_count() {
        // 真实案例（PRINT 机，2026-10-27）：ASUS P8H61-M PLUS V3 + i3-3220
        // 为 2C/4T（带超线程），BIOS Type4 把 Thread Count 写成 2——
        // threads == cores 不触发矛盾检查，OS 逻辑处理器 = 4 才是真相。
        let mut cpus = vec![Cpu {
            socket: Some("CPU1".into()),
            model: Some("Intel(R) Core(TM) i3-3220 CPU @ 3.30GHz".into()),
            vendor: Some("Intel".into()),
            codename: Some("Ivy Bridge (Core i3)".into()),
            cores: Some(2),
            threads: Some(2), // BIOS 瞒报：应为 4
            speed_mhz: Some(3300),
        }];
        sanitize_thread_counts(&mut cpus, Some(4));
        assert_eq!(cpus[0].threads, Some(4));
    }

    #[test]
    fn sanitize_drops_ghost_type4_and_fixes_underscored_threads() {
        // 对抗审查攻击复现（QA 建议#1）：单路板固件多吐一个幽灵 Type4 占位
        // （cores=0、无身份）。旧逻辑 n=2 把 per_socket 稀释成 2，i3-3220 的
        // threads=2 瞒报（threads==cores）恰好漏改。幽灵剔除后 n=1 → 修正为 4。
        let mut cpus = vec![
            Cpu {
                socket: Some("CPU1".into()),
                model: Some("Intel(R) Core(TM) i3-3220 CPU @ 3.30GHz".into()),
                vendor: Some("Intel".into()),
                codename: None,
                cores: Some(2),
                threads: Some(2), // BIOS 瞒报：应为 4
                speed_mhz: Some(3300),
            },
            Cpu {
                socket: Some("CPU2".into()),
                model: None, // 幽灵占位：无身份、无核数
                vendor: None,
                codename: None,
                cores: Some(0),
                threads: Some(0),
                speed_mhz: None,
            },
        ];
        sanitize_thread_counts(&mut cpus, Some(4));
        assert_eq!(cpus.len(), 1, "幽灵 Type4 占位应被剔除");
        assert_eq!(cpus[0].threads, Some(4), "剔除后瞒报应被向上修正");
    }

    #[test]
    fn sanitize_ghost_dilution_threads_none_fills_truth() {
        // 同场景的 threads=None 变体：稀释时 bad 分支会把均摊值 2 自信写入
        // （错误值）；剔除幽灵后 per_socket=4 ≥ cores=2 → 写入正确的 4。
        let mut cpus = vec![
            Cpu {
                socket: Some("CPU1".into()),
                model: Some("Xeon E3-1220".into()),
                vendor: Some("Intel".into()),
                codename: None,
                cores: Some(4),
                threads: None,
                speed_mhz: Some(3100),
            },
            Cpu {
                socket: Some("CPU2".into()),
                model: None,
                vendor: None,
                codename: None,
                cores: None,
                threads: None,
                speed_mhz: None,
            },
        ];
        sanitize_thread_counts(&mut cpus, Some(4));
        assert_eq!(cpus.len(), 1);
        assert_eq!(cpus[0].threads, Some(4));
    }

    #[test]
    fn sanitize_never_shrinks_valid_threads() {
        // 双路服务器双真实 CPU（4 线程各一）：幽灵剔除后 n=2、per_socket=2，
        // 真实 CPU 的 4 线程不被改小；lp=4 与 sum(threads)=4 一致。
        let mut cpus = vec![
            Cpu {
                socket: Some("CPU1".into()),
                model: Some("Xeon".into()),
                vendor: Some("Intel".into()),
                codename: None,
                cores: Some(2),
                threads: Some(4),
                speed_mhz: Some(2000),
            },
            Cpu {
                socket: Some("CPU2".into()),
                model: Some("Xeon".into()),
                vendor: Some("Intel".into()),
                codename: None,
                cores: Some(2),
                threads: Some(4),
                speed_mhz: Some(2000),
            },
        ];
        sanitize_thread_counts(&mut cpus, Some(4));
        assert_eq!(cpus.len(), 2);
        assert_eq!(cpus[0].threads, Some(4));
        assert_eq!(cpus[1].threads, Some(4));
    }

    #[test]
    fn sanitize_no_lp_change_keeps_threads() {
        // OS 逻辑处理器数未知（None）时维持旧行为：threads >= cores 原样保留。
        let mut cpus = vec![Cpu {
            socket: Some("CPU1".into()),
            model: Some("i3-3220".into()),
            vendor: Some("Intel".into()),
            codename: None,
            cores: Some(2),
            threads: Some(2),
            speed_mhz: Some(3300),
        }];
        sanitize_thread_counts(&mut cpus, None);
        assert_eq!(cpus[0].threads, Some(2));
    }

    /// 构造一个最小 SMBIOS 缓冲区：8 字节头 + 若干结构。
    fn build(tables: &[Vec<u8>]) -> Vec<u8> {
        let mut body = Vec::new();
        for t in tables {
            body.extend_from_slice(t);
        }
        let mut raw = vec![0u8; 8];
        raw[0..4].copy_from_slice(&[0, 3, 3, 0]); // major/minor/rev
        raw[4..8].copy_from_slice(&(body.len() as u32).to_le_bytes());
        raw.extend_from_slice(&body);
        raw
    }

    /// Type 0 BIOS：字符串区 = "ACME\0" "1.23\0" "2024-01-01\0"
    fn bios_table() -> Vec<u8> {
        let mut t = vec![0u8; 0x12];
        t[0] = 0; // Type 0
        t[1] = 0x12; // Length
        t[2] = 1;
        t[3] = 0; // Handle
        t[0x04] = 1; // Vendor -> str#1
        t[0x05] = 2; // Version -> str#2
        t[0x08] = 3; // Date -> str#3
        t.extend_from_slice(b"ACME\0");
        t.extend_from_slice(b"1.23\0");
        t.extend_from_slice(b"2024-01-01\0");
        t.push(0); // 字符串区结束
        t
    }

    #[test]
    fn parses_bios() {
        let raw = build(&[bios_table()]);
        let d = parse(&raw).unwrap();
        let bios = d.bios.expect("应解析出 BIOS");
        assert_eq!(bios.vendor.as_deref(), Some("ACME"));
        assert_eq!(bios.version.as_deref(), Some("1.23"));
        assert_eq!(bios.date.as_deref(), Some("2024-01-01"));
    }

    #[test]
    fn parses_cpu_with_word_fields() {
        // Type 4：核心/线程用 SMBIOS 3.0+ 的 WORD 字段（0x2A / 0x2E）
        let mut t = vec![0u8; 0x30];
        t[0] = 4;
        t[1] = 0x30;
        t[0x04] = 1; // socket
        t[0x07] = 2; // vendor
        t[0x10] = 3; // model
        t[0x2A] = 8;
        t[0x2B] = 0; // cores = 8
        t[0x2E] = 16;
        t[0x2F] = 0; // threads = 16
        t[0x16] = 0x70;
        t[0x17] = 0x17; // speed 6000 MHz (0x1770)
        t.extend_from_slice(b"CPU1\0");
        t.extend_from_slice(b"GenuineIntel\0");
        t.extend_from_slice(b"Core i7\0");
        t.push(0);
        let d = parse(&build(&[t])).unwrap();
        assert_eq!(d.cpus.len(), 1);
        let c = &d.cpus[0];
        assert_eq!(c.socket.as_deref(), Some("CPU1"));
        assert_eq!(c.model.as_deref(), Some("Core i7"));
        assert_eq!(c.cores, Some(8));
        assert_eq!(c.threads, Some(16));
        assert_eq!(c.speed_mhz, Some(6000));
    }

    #[test]
    fn cpu_falls_back_to_byte_fields() {
        // 老版本 SMBIOS：WORD 字段为 0，应回退 BYTE（0x23 / 0x25）
        let mut t = vec![0u8; 0x30];
        t[0] = 4;
        t[1] = 0x30;
        t[0x10] = 1;
        t[0x23] = 4; // cores
        t[0x25] = 8; // threads
        t.extend_from_slice(b"Old CPU\0");
        t.push(0);
        let d = parse(&build(&[t])).unwrap();
        assert_eq!(d.cpus[0].cores, Some(4));
        assert_eq!(d.cpus[0].threads, Some(8));
    }

    #[test]
    fn parses_memory_with_extended_size() {
        // Size = 0xFFFF → 走 Extended Size（>=32GB 场景）
        let mut t = vec![0u8; 0x28];
        t[0] = 17;
        t[1] = 0x28;
        t[0x0C] = 0xFF;
        t[0x0D] = 0xFF; // Size = 0xFFFF
        t[0x1C] = 0x00;
        t[0x1D] = 0x80;
        t[0x1E] = 0x00;
        t[0x1F] = 0x00; // 32768 MB = 32GB
        t[0x10] = 1; // locator
        t[0x12] = 0x1A; // DDR4
        t[0x20] = 0x80;
        t[0x21] = 0x0C; // 0x0C80 = 3200 MHz（小端）
        t.extend_from_slice(b"DIMM_A1\0");
        t.push(0);
        let d = parse(&build(&[t])).unwrap();
        assert_eq!(d.mem_sticks.len(), 1);
        let m = &d.mem_sticks[0];
        assert_eq!(m.capacity_mb, Some(MebiBytes::new(32768)));
        assert_eq!(m.mem_type.as_deref(), Some("DDR4"));
        assert_eq!(m.speed_mhz, Some(3200));
    }

    #[test]
    fn parses_memory_size_0x7fff_uses_extended_size() {
        // 回归：Size 字段是 16 位，单条超过 32 GB 放不下，固件会置 0x7FFF
        // 并把真实容量放进 Extended Size(0x1C)。漏判 0x7FFF 不会报错，
        // 只是把哨兵 32767 当成真实容量 —— 真机 8×64GB 服务器曾全部显示成
        // "32 GB"，OS 总计 512 GB 与之差一倍才暴露出来。
        let mut t = vec![0u8; 0x28];
        t[0] = 17;
        t[1] = 0x28;
        t[0x0C] = 0xFF;
        t[0x0D] = 0x7F; // Size = 0x7FFF（哨兵，不是 32767 MB）
        t[0x1C] = 0x00;
        t[0x1D] = 0x00;
        t[0x1E] = 0x01;
        t[0x1F] = 0x00; // Extended Size = 65536 MB = 64 GB
        t[0x10] = 1; // locator
        t.extend_from_slice(b"DIMM_A1\0");
        t.push(0);
        let d = parse(&build(&[t])).unwrap();
        assert_eq!(
            d.mem_sticks[0].capacity_mb,
            Some(MebiBytes::new(65536)),
            "Size=0x7FFF 时必须取 Extended Size，不能把哨兵 32767 当成容量"
        );
    }

    #[test]
    fn cpu_word_counts_treat_0xffff_as_unknown() {
        // Core Count 2(0x2A) / Thread Count 2(0x2E) 的 0xFFFF 表示「未知」，
        // 必须回退到老版本 BYTE 字段(0x23/0x25)，而不是输出 65535 核 / 65535 线程。
        let mut t = vec![0u8; 0x30];
        t[0] = 4;
        t[1] = 0x30;
        t[0x2A] = 0xFF;
        t[0x2B] = 0xFF; // Core Count 2 = 0xFFFF
        t[0x2E] = 0xFF;
        t[0x2F] = 0xFF; // Thread Count 2 = 0xFFFF
        t[0x23] = 8; // BYTE 版 Core Count
        t[0x25] = 16; // BYTE 版 Thread Count
        t[0x10] = 1; // model
        t.extend_from_slice(b"Xeon\0");
        t.push(0);
        let d = parse(&build(&[t])).unwrap();
        assert_eq!(
            d.cpus[0].cores,
            Some(8),
            "0xFFFF 是未知哨兵，必须回退 BYTE 字段而不是输出 65535"
        );
        assert_eq!(d.cpus[0].threads, Some(16));
    }

    /// Type 2 Baseboard：字符串区 = "ASUSTeK\0" "PRIME-Z690\0" "MB-SN-123\0"
    fn board_table() -> Vec<u8> {
        let mut t = vec![0u8; 0x0A];
        t[0] = 2; // Type 2
        t[1] = 0x0A; // Length
        t[2] = 1;
        t[3] = 0; // Handle
        t[0x04] = 1; // Manufacturer -> str#1
        t[0x05] = 2; // Product -> str#2
        t[0x07] = 3; // Serial Number -> str#3
        t.extend_from_slice(b"ASUSTeK\0");
        t.extend_from_slice(b"PRIME-Z690\0");
        t.extend_from_slice(b"MB-SN-123\0");
        t.push(0);
        t
    }

    #[test]
    fn parses_board_serial() {
        // Type 2 偏移 0x07 是 Serial Number 字符串索引。
        let raw = build(&[board_table()]);
        let d = parse(&raw).unwrap();
        let board = d.board.expect("应解析出主板");
        assert_eq!(board.vendor.as_deref(), Some("ASUSTeK"));
        assert_eq!(board.product.as_deref(), Some("PRIME-Z690"));
        assert_eq!(board.serial.as_deref(), Some("MB-SN-123"));
    }

    /// Type 17 Memory Device：含 Serial Number（偏移 0x18）。
    fn mem_table_with_serial() -> Vec<u8> {
        let mut t = vec![0u8; 0x20];
        t[0] = 17;
        t[1] = 0x20;
        t[0x0C] = 0x00;
        t[0x0D] = 0x40; // 0x4000 = 16384 MB
        t[0x10] = 1; // locator
        t[0x11] = 2; // bank
        t[0x17] = 3; // vendor
        t[0x18] = 4; // serial
        t[0x1A] = 5; // part_no
        t[0x12] = 0x1A; // DDR4
        t.extend_from_slice(b"DIMM_A1\0");
        t.extend_from_slice(b"BANK0\0");
        t.extend_from_slice(b"Kingston\0");
        t.extend_from_slice(b"KST-SN-7788\0");
        t.extend_from_slice(b"ABC-123\0");
        t.push(0);
        t
    }

    #[test]
    fn parses_memory_serial() {
        // Type 17 偏移 0x18 是 Serial Number 字符串索引。
        let raw = build(&[mem_table_with_serial()]);
        let d = parse(&raw).unwrap();
        assert_eq!(d.mem_sticks.len(), 1);
        let m = &d.mem_sticks[0];
        assert_eq!(m.locator.as_deref(), Some("DIMM_A1"));
        assert_eq!(m.vendor.as_deref(), Some("Kingston"));
        assert_eq!(m.serial.as_deref(), Some("KST-SN-7788"));
    }

    #[test]
    fn extended_size_sentinel_is_not_a_capacity() {
        // ExtendedSize = 0xFFFFFFFF 是"未知"哨兵值，不是 2TB 内存条
        let mut t = vec![0u8; 0x28];
        t[0] = 17;
        t[1] = 0x28;
        t[0x0C] = 0xFF;
        t[0x0D] = 0xFF;
        t[0x1C..0x20].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        t[0x10] = 1;
        t.extend_from_slice(b"DIMM_A1\0");
        t.push(0);
        let d = parse(&build(&[t])).unwrap();
        assert_eq!(d.mem_sticks[0].capacity_mb, None);
        assert!(!d.mem_sticks[0].is_populated());
    }

    #[test]
    fn empty_slot_yields_no_capacity() {
        let mut t = vec![0u8; 0x28];
        t[0] = 17;
        t[1] = 0x28;
        t[0x0C] = 0;
        t[0x0D] = 0; // 空槽
        t[0x10] = 1;
        t.extend_from_slice(b"DIMM_B1\0");
        t.push(0);
        let d = parse(&build(&[t])).unwrap();
        assert_eq!(d.mem_sticks[0].capacity_mb, None);
    }

    #[test]
    fn stringless_structure_must_advance_two_bytes() {
        // 回归测试（真实 bug）：Type 16「物理内存阵列」没有字符串区，
        // 只有双 00 终止符。早期实现在此只前进 1 字节 → 整表错位，
        // 后面的 Type 17 被错读成 `type=0 len=17` 的假结构 → 内存条恒为 0 根。
        let mut t16 = vec![0u8; 23];
        t16[0] = 16;
        t16[1] = 23;
        t16.push(0);
        t16.push(0); // 无字符串 → 双 00

        let mut t17 = vec![0u8; 0x28];
        t17[0] = 17;
        t17[1] = 0x28;
        t17[0x0C] = 0x00;
        t17[0x0D] = 0x40; // 0x4000 = 16384 MB
        t17[0x10] = 1; // locator -> str#1
        t17.extend_from_slice(b"DIMM 0\0");
        t17.push(0);

        let raw = build(&[t16, t17]);
        assert_eq!(
            list_structures(&raw),
            vec![(16, 23), (17, 0x28)],
            "遍历必须完整覆盖无字符串结构之后的所有结构"
        );

        let d = parse(&raw).unwrap();
        assert_eq!(
            d.mem_sticks.len(),
            1,
            "无字符串结构之后必须仍能解析到 Type 17"
        );
        assert_eq!(d.mem_sticks[0].locator.as_deref(), Some("DIMM 0"));
        assert_eq!(d.mem_sticks[0].capacity_mb, Some(MebiBytes::new(16384)));
    }

    #[test]
    fn memory_type_codes_follow_smbios_spec() {
        // 早期实现把 DDR/DDR2 错标在 0x14/0x15；规范值是 0x12/0x13。
        assert_eq!(mem_type_name(0x12), Some("DDR"));
        assert_eq!(mem_type_name(0x13), Some("DDR2"));
        assert_eq!(mem_type_name(0x14), Some("DDR2 FB-DIMM"));
        assert_eq!(mem_type_name(0x18), Some("DDR3"));
        assert_eq!(mem_type_name(0x1A), Some("DDR4"));
        assert_eq!(mem_type_name(0x22), Some("DDR5"));
        assert_eq!(
            mem_type_name(0x15),
            None,
            "Reserved 值应返回 None 而非猜一个名字"
        );
        // 展示策略：泛化类型不输出（与 C++ 一致，避免虚拟机上出现无意义差异）
        assert_eq!(mem_type_name(0x07), None, "RAM 属于泛化值，不展示");
        assert_eq!(mem_type_name(0x02), None, "Unknown 不展示");
    }

    #[test]
    fn truncated_table_is_error_not_ub() {
        // 声明长度超出实际数据 → 返回 Err，而不是越界读（C++ 版此处是 UB）
        let mut raw = vec![0u8; 8];
        raw[4..8].copy_from_slice(&999u32.to_le_bytes());
        assert!(matches!(
            parse(&raw),
            Err(SmbiosError::LengthMismatch { .. })
        ));
    }

    #[test]
    fn too_short_is_error() {
        assert!(matches!(parse(&[1, 2, 3]), Err(SmbiosError::TooShort(3))));
    }

    #[test]
    fn invalid_utf8_string_is_dropped_not_replaced() {
        // 真机案例：某根 Kingston 内存的 Part Number 是 18 字节非法 UTF-8。
        // 旧实现 from_utf8_lossy 会产出 "??????????????????" 写进报告，
        // 运维会误判为工具故障。现在应整串丢弃（报告显示 "-"）。
        let junk = vec![0xFFu8; 18];
        let strings: Vec<&[u8]> = vec![b"ChannelB-DIMM0", &junk];
        assert_eq!(str_at(&strings, 1).as_deref(), Some("ChannelB-DIMM0"));
        assert_eq!(str_at(&strings, 2), None);
    }

    #[test]
    fn str_at_basics() {
        let strings: Vec<&[u8]> = vec![b"  Kingston  ", b"   ", b""];
        // 索引 0 = 无此字段；越界 = None；空白串 = None；有效串要去空格
        assert_eq!(str_at(&strings, 0), None);
        assert_eq!(str_at(&strings, 1).as_deref(), Some("Kingston"));
        assert_eq!(str_at(&strings, 2), None);
        assert_eq!(str_at(&strings, 3), None);
        assert_eq!(str_at(&strings, 9), None);
    }
}
