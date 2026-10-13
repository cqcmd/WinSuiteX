//! 内存「通道分布」判定：**单一真源**，GUI 面板与 HTML 报告共用。
//!
//! # 为什么放在 core
//! 通道提示的判定逻辑原本写在 `winsuitex-report` 里，GUI 面板想要同一条提示时
//! 就会变成"两个 crate 各写一份"——必然漂移（本项目已有教训：量高/绘制两趟
//! 宽度不一致导致滚动裁尾）。这里放到模型层，两端共用同一函数与同一份文案。
//!
//! # 为什么不能只看"插了几条"
//! 双路服务器每路各插 1 条、且都在 Channel 0 时，条数是**偶数 2**，
//! 老的"奇数条=非对称双通道"规则完全识别不出来，实际却是**两颗 CPU 都在单通道**
//! 跑，带宽只有双通道的一半（Z.W. 华为 BC11HGSB0 真机：DIMM000/DIMM100 同在 Channel 0）。

use crate::model::MemStick;

/// 取关键字后的第一段标识符（`"_Node0_Channel0_Dimm0"` 取 "node" → "0"）。
///
/// 只看关键字紧跟的那一段字母数字，跳过 `_ - 空格` 分隔符；取不到返回 `None`。
fn token_after(s: &str, key: &str) -> Option<String> {
    let lower = s.to_ascii_lowercase();
    let pos = lower.find(key)?;
    let rest = &lower[pos + key.len()..];
    let rest = rest.trim_start_matches(['_', '-', ' ']);
    let tok: String = rest
        .chars()
        .take_while(char::is_ascii_alphanumeric)
        .collect();
    if tok.is_empty() {
        None
    } else {
        Some(tok)
    }
}

/// 从槽位名取通道字母（`"DIMM_A1"` → `"a"`，`"DIMM000"` → `None`）。
fn dimm_letter(locator: &str) -> Option<String> {
    let lower = locator.to_ascii_lowercase();
    let pos = lower.find("dimm")?;
    let rest = &lower[pos + 4..];
    let rest = rest.trim_start_matches(['_', '-', ' ']);
    let c = rest.chars().next()?;
    // 只认字母（"DIMM_A1" 的 A）；"DIMM000" 这类纯数字槽号无通道信息 → None。
    c.is_ascii_alphabetic()
        .then(|| c.to_ascii_lowercase().to_string())
}

/// 解析一条内存属于「哪个 CPU 路（Node）的哪个通道（Channel）」。
///
/// 拿不准就返回 `None`（调用方据此放弃提示）——**宁可漏报，不可误报**。
/// - Node：bank 里 `node` 后的数字。`_Node0_Channel0_Dimm0` → "0"；`NODE 1` → "1"。
///   单路主板没有 node 字样 → `""`（整机当作一路看）。
/// - Channel：优先 bank 的 `channel`/`chan` 后那一段，退到 locator 的通道字母。
fn node_channel(m: &MemStick) -> Option<(String, String)> {
    let bank = m.bank.as_deref().unwrap_or("");
    let loc = m.locator.as_deref().unwrap_or("");
    let node = token_after(bank, "node").unwrap_or_default();
    let ch = token_after(bank, "channel")
        .or_else(|| token_after(bank, "chan"))
        .or_else(|| dimm_letter(loc))?;
    Some((node, ch))
}

/// 判定内存是否**没跑满通道**；没问题返回 `None`。
///
/// 判定口径：任一路（Node）已插内存只落在 1 个通道上 → 该路单通道。
/// 保守条件：只要有任意一条解析不出通道信息，就整体返回 `None`（避免固件怪名误报）。
///
/// 返回的文案可直接渲染（面板行 / 报告行共用）。
pub fn memory_channel_warning(sticks: &[MemStick]) -> Option<String> {
    let mut per_node: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    for m in sticks.iter().filter(|m| m.is_populated()) {
        // 任一条拿不准 → 放弃（? 提前返回 None）
        let (node, ch) = node_channel(m)?;
        per_node.entry(node).or_default().insert(ch);
    }
    // 至少 2 个通道条目才谈得上"分布"；1 条走调用方更精确的"仅插 1 条"文案。
    let total: usize = per_node.values().map(|s| s.len()).sum();
    if per_node.values().all(std::collections::BTreeSet::is_empty) || total < 2 {
        return None;
    }
    let single: Vec<String> = per_node
        .iter()
        .filter(|(_, chs)| chs.len() == 1)
        .map(|(n, _)| n.clone())
        .collect();
    if single.is_empty() {
        return None; // 每路都占了 ≥2 个通道 → 通道是满的
    }
    let named: Vec<String> = single
        .iter()
        .map(|n| {
            if n.is_empty() {
                "整机".to_string()
            } else {
                format!("Node {n}")
            }
        })
        .collect();
    Some(format!(
        "内存未按通道均匀分布：{} 仅占用 1 个通道 → 对应 CPU 单通道运行，带宽约为双通道一半；\
         建议把同规格内存条插到同一路的另一通道，组成双通道",
        named.join("、")
    ))
}

/// 内存「容量口径偏差」判定：**单一真源**，GUI 面板与 HTML 报告共用（同 `memory_channel_warning` 的路线）。
///
/// 口径交叉校验：OS 报告的总内存（`os_total_mib`）与 SMBIOS 各**实插**槽位容量之和
/// 通常应当一致；差异过大说明某一口径不可靠（核显占用 / 预留内存 / **固件虚报槽位**），
/// 这里显式提示差异而非静默采用单一口径，避免误导资产盘点。
///
/// 阈值（与 GUI / 报告一致，避免两端漂移）：
/// - ≤2%：属核显 / 预留的正常口径差，不提示；
/// - 2%~10%：温和提示「可能含核显 / 预留内存」；
/// - 偏差 >10%：必须告警。
///
/// 偏差 >10% 这一档的差一半绝不是核显 / 预留能解释的——真机上曾因 SMBIOS `Size=0x7FFF`
/// 哨兵被当成 32767 MiB，各槽合计只有 OS 总计的一半却被「属正常」洗白过去。
/// 该档文案明确点名「固件虚报 / 幽灵槽位」，让技师一眼看懂为何槽位数对不上，
/// 不误以为是采集 bug（Z.W. 真机：物理 1 条，固件虚报 CHANNEL A 幽灵槽 → 显示 2 条）。
///
/// 返回的文案可直接渲染（面板行 / 报告行共用）。`os_total_mib` 为 0（OS 未报总量）时返回 `None`。
pub fn memory_capacity_deviation_warning(sticks: &[MemStick], os_total_mib: u64) -> Option<String> {
    const DIFF_IGNORE_RATIO: f64 = 0.02;
    const DIFF_ALERT_RATIO: f64 = 0.10;
    let sum_sticks: u64 = sticks
        .iter()
        .filter(|m| m.is_populated())
        .filter_map(|m| m.capacity_mb)
        .map(|c| c.get())
        .sum();
    if sum_sticks == 0 || os_total_mib == 0 {
        return None;
    }
    let diff = os_total_mib.abs_diff(sum_sticks);
    let ratio = diff as f64 / os_total_mib as f64;
    if diff == 0 || ratio <= DIFF_IGNORE_RATIO {
        return None;
    }
    let tb = fmt_mib_iec(os_total_mib);
    let sb = fmt_mib_iec(sum_sticks);
    if ratio > DIFF_ALERT_RATIO {
        Some(format!(
            "OS 总计 {tb} 与 SMBIOS 各槽合计 {sb} 相差 {d} MiB（偏差 {p:.0}%）—— \
             偏差过大，SMBIOS 槽位容量口径不可信；常见原因为固件虚报（幽灵槽位被填成满容量 / \
             Size 哨兵 0x7FFF 误当 32767 MiB），请核对实插条数与单条容量",
            tb = tb,
            sb = sb,
            d = diff,
            p = ratio * 100.0
        ))
    } else {
        Some(format!(
            "OS 总计 {tb} 与 SMBIOS 各槽合计 {sb} 相差 {d} MiB（可能含核显 / 预留内存）",
            tb = tb,
            sb = sb,
            d = diff
        ))
    }
}

/// MiB → 人类可读容量（取整，IEC 单位：KiB/MiB/GiB/TiB）。
///
/// 与 `winsuitex_report::fmt_bytes_int_val(mib << 20)` 输出一致，放在 core 让
/// `memory_capacity_deviation_warning` 在 GUI / 报告两端产出完全相同的文案，不漂移。
fn fmt_mib_iec(mib: u64) -> String {
    let bytes = mib * 1024 * 1024;
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;
    const TIB: u64 = 1024 * 1024 * 1024 * 1024;
    if bytes >= TIB {
        format!("{} TiB", (bytes + TIB / 2) / TIB)
    } else if bytes >= GIB {
        format!("{} GiB", (bytes + GIB / 2) / GIB)
    } else if bytes >= MIB {
        format!("{} MiB", (bytes + MIB / 2) / MIB)
    } else if bytes >= KIB {
        format!("{} KiB", (bytes + KIB / 2) / KIB)
    } else {
        format!("{bytes} B")
    }
}

/// 从 Bank Locator 提取通道标识（"P0 CHANNEL A" → "CHANNEL A"），用于槽位名重复时消歧。
///
/// 部分固件把不同物理槽的 locator 都写成一样（华硕 EX-B650M 两槽均 "DIMM 0"），
/// 报告/面板里两行同名分不清。有 "CHANNEL X" 取它；否则回退整段 bank（"NODE 1" / "BANK 0"）。
///
/// 单一真源：原为 `winsuitex-report` 私有函数，GUI 面板需要同样的消歧时收进 core 共用。
pub fn channel_tag(bank: &Option<String>) -> Option<String> {
    let b = bank.as_ref()?;
    let toks: Vec<&str> = b.split_whitespace().collect();
    if let Some(pos) = toks.iter().position(|t| t.eq_ignore_ascii_case("CHANNEL")) {
        if let Some(ch) = toks.get(pos + 1) {
            return Some(format!("CHANNEL {}", ch));
        }
    }
    let trimmed = b.trim();
    if !trimmed.is_empty() {
        return Some(trimmed.to_string());
    }
    None
}

/// 内存槽位「显示名」消歧（单一真源）：统计**已插**槽位 locator 重复情况，
/// 重复的追加通道标识（"DIMM 1" → "DIMM 1（CHANNEL A）"）。
///
/// 返回与 `sticks` 等长的 `Vec`，索引一一对应；locator 缺失/空的条目为 `None`，
/// 由调用方决定兜底文案（报告用「未知」，GUI 用空串）。空槽也参与（与报告历史行为一致：
/// 空槽表头保留原始占位串，仅当与已插条重名时才追加通道）。
pub fn disambiguated_slot_names(sticks: &[MemStick]) -> Vec<Option<String>> {
    let mut freq: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for m in sticks {
        if m.is_populated() {
            if let Some(l) = m.locator.as_deref().filter(|l| !l.is_empty()) {
                *freq.entry(l).or_insert(0) += 1;
            }
        }
    }
    sticks
        .iter()
        .map(|m| {
            let l = m.locator.as_deref().filter(|l| !l.is_empty())?;
            Some(if freq.get(l).copied().unwrap_or(0) > 1 {
                match channel_tag(&m.bank) {
                    Some(ch) => format!("{l}（{ch}）"),
                    None => l.to_string(),
                }
            } else {
                l.to_string()
            })
        })
        .collect()
}

/// 判断一条实插内存是否带「真实 SPD 信息」（类型 / 速率 / PN / 厂商任一为真实数据）。
///
/// 固件幽灵槽的特征是全套 Unknown / 空。但**真条也可能缺 SPD 厂商**（020 类真机：
/// 厂商未返回、显示「未知」），所以这里只做「有真实信息」的肯定判定，绝不因缺厂商
/// 就判幻影；误判的兜底是 `memory_channel_advice` 里「spd 合计必须与 OS 总量吻合」
/// 的双重校验——推断不吻合就放弃排除，宁可不提示也不冤枉真条。
fn has_real_spd(m: &MemStick) -> bool {
    let known =
        |s: Option<&str>| s.is_some_and(|s| !s.is_empty() && !s.eq_ignore_ascii_case("unknown"));
    known(m.mem_type.as_deref())
        || m.speed_mhz.is_some_and(|s| s > 0)
        || known(m.part_no.as_deref())
        || known(m.vendor.as_deref())
}

/// 找出被固件虚报的「已插」槽位在 `sticks` 中的索引（幽影槽 / 幽灵槽）。
///
/// 判定口径（与 `memory_channel_advice` 完全一致，单一真源，避免两端漂移）：
/// 仅当 SMBIOS 各**实插**槽位容量合计 与 OS 总量偏差 >10%，且「带真实 SPD 的实插条」
/// 容量合计与 OS 总量吻合（≤10%）时，把无真实 SPD 的实插条判为固件幻影并标出。
/// 推断不吻合（如核显吃掉一半容量、或真条也缺 SPD 厂商）则整体返回空——宁漏报不误报。
///
/// 返回索引对 `sticks` 有效；调用方据此把对应槽标「固件虚报」并排除出实插计数，
/// 让面板/报告像 SPD 直读工具（CPU-Z / HWiNFO / AIDA64）一样只统计真正生效的内存条，
/// 而不是被 SMBIOS Type 17 的虚报槽带偏（AM5 平台已知 AGESA 缺陷：空槽被写成已插满容量）。
pub fn phantom_slot_indices(sticks: &[MemStick], os_total_mib: Option<u64>) -> Vec<usize> {
    let populated: Vec<(usize, &MemStick)> = sticks
        .iter()
        .enumerate()
        .filter(|(_, m)| m.is_populated())
        .collect();
    if let Some(total) = os_total_mib.filter(|t| *t > 0) {
        let dev = |s: u64| s.abs_diff(total) as f64 / total as f64;
        let sum_all: u64 = populated
            .iter()
            .map(|(_, m)| m.capacity_mb.map_or(0, |c| c.get()))
            .sum();
        if sum_all > 0 && dev(sum_all) > 0.10 {
            let spd: Vec<&MemStick> = populated
                .iter()
                .filter(|(_, m)| has_real_spd(m))
                .map(|(_, m)| *m)
                .collect();
            let sum_spd: u64 = spd
                .iter()
                .map(|m| m.capacity_mb.map_or(0, |c| c.get()))
                .sum();
            if spd.len() < populated.len() && sum_spd > 0 && dev(sum_spd) <= 0.10 {
                return populated
                    .iter()
                    .filter(|(_, m)| !has_real_spd(m))
                    .map(|(i, _)| *i)
                    .collect();
            }
        }
    }
    Vec::new()
}

/// 内存「通道提示」总入口：**单一真源**，GUI 面板与 HTML 报告共用。
///
/// 涵盖三档判定（文案与历史报告完全一致，避免两端漂移）：
/// 1. 仅 1 条实插 → 纯单通道；
/// 2. 奇数条实插（非对称双通道 / flex）；
/// 3. 通道分布不均（按 (Node, Channel) 分组，见 `memory_channel_warning`）。
///
/// **幻影槽感知**：SMBIOS 常把空槽虚报成满容量幽灵条（华硕 B650 真机：物理 1 条
/// 却报 2 条、A/B 通道"都占满"，把单通道问题完全掩盖）。当各槽合计与 OS 总量偏差
/// 超过 10%，且「带 SPD 信息的实插条」容量合计与 OS 总量吻合（≤10%）时，判定无 SPD
/// 的条为固件幽灵槽并排除，基于真实条做上述判定——推断不吻合就按原口径，宁漏报不误报。
pub fn memory_channel_advice(sticks: &[MemStick], os_total_mib: Option<u64>) -> Option<String> {
    let populated: Vec<MemStick> = sticks
        .iter()
        .filter(|m| m.is_populated())
        .cloned()
        .collect();
    if populated.is_empty() {
        return None;
    }
    let has_empty = sticks.len() > populated.len();

    // —— 幻影槽推断（单一真源见 `phantom_slot_indices`）——
    let phantom_idx = phantom_slot_indices(sticks, os_total_mib);
    let phantom = !phantom_idx.is_empty();
    let real: Vec<MemStick> = if phantom {
        let set: std::collections::HashSet<usize> = phantom_idx.iter().copied().collect();
        sticks
            .iter()
            .enumerate()
            .filter(|(i, m)| m.is_populated() && !set.contains(i))
            .map(|(_, m)| m.clone())
            .collect()
    } else {
        populated
    };

    let n = real.len();
    if n == 1 && (has_empty || phantom) {
        let base = "单通道，带宽约为双通道一半，游戏性能受限；建议补同规格条到另一通道组双通道";
        return Some(if phantom {
            format!("仅 1 条内存实际生效（固件幽灵槽位已按 OS 总容量排除）→ {base}")
        } else {
            format!("仅插 1 条内存 → {base}")
        });
    }
    if n >= 3 && n % 2 == 1 && (has_empty || phantom) {
        return Some("实插内存条为奇数（非对称双通道/flex）→ 部分通道单条运行，带宽未用满；建议补齐到偶数条、均匀分布到各通道".into());
    }
    // 通道分布不均（双路各插 1 条同通道、单路两槽同通道等）——**始终检测**，
    // 不受「有无空槽 / 是否幻影」门控，与 GUI 历史行为一致：双路单通道是真实带宽
    // 减半问题，无论插槽是否插满都应提示（空槽门控是报告侧的历史取舍，统一到
    // 「检测即报」更正确，且 `memory_channel_warning` 自身对跑满/不可判定返回 None）。
    memory_channel_warning(&real)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MebiBytes;

    fn stick(locator: &str, bank: &str, cap: Option<u32>) -> MemStick {
        MemStick {
            locator: Some(locator.into()),
            bank: Some(bank.into()),
            serial: None,
            part_no: None,
            vendor: None,
            mem_type: None,
            capacity_mb: cap.map(|c| MebiBytes::new(c as u64)),
            speed_mhz: None,
        }
    }

    /// 真机回归（Z.W. 华为 BC11HGSB0）：双路各插 1 条、都在 Channel 0。
    #[test]
    fn two_sticks_one_per_node_on_same_channel_warns() {
        let sticks = vec![
            stick("DIMM000", "_Node0_Channel0_Dimm0", Some(32768)),
            stick("DIMM100", "_Node1_Channel0_Dimm0", Some(32768)),
        ];
        let msg = memory_channel_warning(&sticks).expect("两路各 1 条同通道应提示");
        assert!(msg.contains("单通道"), "文案应含单通道: {msg}");
        assert!(
            msg.contains("Node 0") && msg.contains("Node 1"),
            "应列出两路: {msg}"
        );
    }

    /// 正常双通道（同路的两个通道各一条）不得误报。
    #[test]
    fn proper_dual_channel_does_not_warn() {
        let sticks = vec![
            stick("DIMM_A1", "P0 CHANNEL A", Some(32768)),
            stick("DIMM_B1", "P0 CHANNEL B", Some(32768)),
        ];
        assert!(
            memory_channel_warning(&sticks).is_none(),
            "通道跑满时不应提示"
        );
    }

    /// 槽位名没有通道信息（"DIMM0" + "BANK 0"）→ 不提示，宁漏报不误报。
    #[test]
    fn unparsable_channel_never_warns() {
        let sticks = vec![
            stick("DIMM0", "BANK 0", Some(16384)),
            stick("DIMM1", "BANK 1", Some(16384)),
        ];
        assert!(
            memory_channel_warning(&sticks).is_none(),
            "通道不可判定时不应提示"
        );
    }

    /// 单条 / 空列表：交给调用方更精确的文案，这里返回 None。
    #[test]
    fn single_or_empty_is_none() {
        assert!(memory_channel_warning(&[]).is_none());
        assert!(memory_channel_warning(&[stick("DIMM_A1", "P0 CHANNEL A", Some(16384))]).is_none());
    }

    /// 容量偏差 ≤2%：双条 16 GiB 实插、OS 总计 32 GiB → 不提示（属核显/预留正常口径差）。
    #[test]
    fn capacity_deviation_ignored_when_close() {
        let sticks = vec![
            stick("DIMM_A1", "P0 CHANNEL A", Some(16384)),
            stick("DIMM_B1", "P0 CHANNEL B", Some(16384)),
        ];
        assert!(
            memory_capacity_deviation_warning(&sticks, 32768).is_none(),
            "差异极小不应提示"
        );
    }

    /// 容量偏差 >10%：物理 1 条 32 GiB，但固件虚报第 2 槽满容量 → SMBIOS 合计 64 GiB（偏差 100%）。
    /// 必须告警且点名「幽灵槽位」，让技师看懂不是采集 bug。
    #[test]
    fn capacity_deviation_alert_mentions_phantom_slot() {
        let sticks = vec![
            stick("DIMM_A1", "P0 CHANNEL A", Some(32768)),
            stick("DIMM_B1", "P0 CHANNEL B", Some(32768)), // 幽灵槽被填成满容量
        ];
        let msg = memory_capacity_deviation_warning(&sticks, 32768).expect("应告警");
        assert!(msg.contains("偏差过大"), "应报偏差过大: {msg}");
        assert!(msg.contains("幽灵槽位"), "应点名固件虚报/幽灵槽位: {msg}");
    }

    /// 容量偏差 2%~10%：SMBIOS 合计比 OS 多 ~3% → 温和提示，不报"偏差过大"。
    #[test]
    fn capacity_deviation_mild_between_2_and_10_percent() {
        // OS 32768 MiB，SMBIOS 合计 16384+17408=33792 MiB（差 1024 MiB ≈ 3.1%）
        let sticks = vec![
            stick("DIMM_A1", "P0 CHANNEL A", Some(16384)),
            stick("DIMM_B1", "P0 CHANNEL B", Some(17408)),
        ];
        let msg = memory_capacity_deviation_warning(&sticks, 32768).expect("应温和提示");
        assert!(!msg.contains("偏差过大"), "3% 不应报偏差过大: {msg}");
        assert!(msg.contains("可能含核显"), "应温和提示核显/预留: {msg}");
    }

    /// OS 未报总量（0）时不提示——避免拿不到 OS 总计时凭空报偏差。
    #[test]
    fn capacity_deviation_none_when_os_total_zero() {
        let sticks = vec![stick("DIMM_A1", "P0 CHANNEL A", Some(32768))];
        assert!(memory_capacity_deviation_warning(&sticks, 0).is_none());
    }

    /// 带 SPD 信息的实插条（真条）。
    fn spd_stick(locator: &str, bank: &str, cap: u32) -> MemStick {
        MemStick {
            locator: Some(locator.into()),
            bank: Some(bank.into()),
            serial: Some("00201196".into()),
            part_no: Some("AX5U6000C3032G-BB300".into()),
            vendor: Some("A-DATA".into()),
            mem_type: Some("DDR5".into()),
            capacity_mb: Some(MebiBytes::new(cap as u64)),
            speed_mhz: Some(6000),
        }
    }

    /// 007 真机形态：A/B 通道各报一条 32 GiB，但 A 通道那条是固件幽灵（全套 Unknown），
    /// OS 只见 1 条容量 → 幻影槽应被排除并正确警告「单通道」。
    #[test]
    fn phantom_slot_single_real_warns_single_channel() {
        let sticks = vec![
            stick("DIMM 0", "P0 CHANNEL A", None),
            stick("DIMM 1", "P0 CHANNEL A", Some(32768)), // 幽灵：无任何 SPD
            stick("DIMM 0", "P0 CHANNEL B", None),
            spd_stick("DIMM 1", "P0 CHANNEL B", 32768), // 真条
        ];
        let msg = memory_channel_advice(&sticks, Some(32426)).expect("应警告单通道");
        assert!(msg.contains("单通道"), "应含单通道: {msg}");
        assert!(msg.contains("幽灵"), "应点名幽灵槽已排除: {msg}");
        assert!(
            !msg.contains("仅插 1 条"),
            "幻影形态不该用原 1 条文案: {msg}"
        );
    }

    /// 真的单条 + 有空槽（无幻影）：保持原「仅插 1 条」文案，不提幽灵。
    #[test]
    fn single_stick_no_phantom_keeps_original_text() {
        let sticks = vec![
            spd_stick("DIMM_A1", "P0 CHANNEL A", 16384),
            stick("DIMM_B1", "P0 CHANNEL B", None),
        ];
        let msg = memory_channel_advice(&sticks, Some(16384)).expect("应警告单通道");
        assert!(msg.contains("仅插 1 条"), "应保留原文案: {msg}");
        assert!(!msg.contains("幽灵"), "无幻影不应提幽灵: {msg}");
    }

    /// 奇数条（flex）文案不变。
    #[test]
    fn flex_three_sticks_unchanged() {
        let sticks = vec![
            spd_stick("DIMM_A1", "P0 CHANNEL A", 16384),
            spd_stick("DIMM_B1", "P0 CHANNEL B", 16384),
            spd_stick("DIMM_C1", "P0 CHANNEL C", 16384),
            stick("DIMM_D1", "P0 CHANNEL D", None),
        ];
        let msg = memory_channel_advice(&sticks, Some(49152)).expect("应提示 flex");
        assert!(msg.contains("奇数"), "应保留 flex 文案: {msg}");
    }

    /// 幻影推断不吻合（spd 合计与 OS 总量也对不上）→ 放弃排除，按原口径
    /// （两真条各占 A/B 通道 → 通道满 → 不提示），宁漏报不误报。
    #[test]
    fn phantom_inference_fails_when_spd_sum_mismatch() {
        let sticks = vec![
            spd_stick("DIMM_A1", "P0 CHANNEL A", 32768),
            spd_stick("DIMM_B1", "P0 CHANNEL B", 32768),
        ];
        // OS 只见 1 条容量（比如核显吃掉一半）：spd 合计与 OS 偏差 50% → 推断失败
        assert!(memory_channel_advice(&sticks, Some(32768)).is_none());
    }

    /// 双通道跑满 + 有空槽：不提示。
    #[test]
    fn dual_channel_full_no_advice() {
        let sticks = vec![
            spd_stick("DIMM_A1", "P0 CHANNEL A", 16384),
            spd_stick("DIMM_B1", "P0 CHANNEL B", 16384),
            stick("DIMM_A2", "P0 CHANNEL A", None),
        ];
        assert!(memory_channel_advice(&sticks, Some(32768)).is_none());
    }

    /// 007 槽位名消歧：两个 populated 都是 "DIMM 1"，应追加（CHANNEL A/B）；
    /// 空槽 locator 不重名时保留原样。
    #[test]
    fn disambiguated_slot_names_appends_channel() {
        let sticks = vec![
            stick("DIMM 0", "P0 CHANNEL A", None),
            stick("DIMM 1", "P0 CHANNEL A", Some(32768)),
            stick("DIMM 0", "P0 CHANNEL B", None),
            spd_stick("DIMM 1", "P0 CHANNEL B", 32768),
        ];
        let names = disambiguated_slot_names(&sticks);
        assert_eq!(names.len(), 4);
        assert_eq!(names[1].as_deref(), Some("DIMM 1（CHANNEL A）"));
        assert_eq!(names[3].as_deref(), Some("DIMM 1（CHANNEL B）"));
        // 空槽 "DIMM 0" 与已插条不重名（已插的是 DIMM 1），保留原样
        assert_eq!(names[0].as_deref(), Some("DIMM 0"));
        assert_eq!(names[2].as_deref(), Some("DIMM 0"));
    }

    /// 幻影槽索引：A 通道无 SPD 的 32 GiB 条应被标出（索引 1），真条（索引 3）不动。
    #[test]
    fn phantom_slot_indices_finds_no_spd_phantom() {
        let sticks = vec![
            stick("DIMM 0", "P0 CHANNEL A", None),
            stick("DIMM 1", "P0 CHANNEL A", Some(32768)), // 幻影：无 SPD
            stick("DIMM 0", "P0 CHANNEL B", None),
            spd_stick("DIMM 1", "P0 CHANNEL B", 32768), // 真条
        ];
        let idx = phantom_slot_indices(&sticks, Some(32426));
        assert_eq!(idx, vec![1], "应标出第 2 条（索引 1）为固件幻影");
    }

    /// 正常双通道（两条都带 SPD、偏差 0）→ 不标任何幻影，绝不误伤真条。
    #[test]
    fn phantom_slot_indices_empty_when_no_phantom() {
        let sticks = vec![
            spd_stick("DIMM_A1", "P0 CHANNEL A", 16384),
            spd_stick("DIMM_B1", "P0 CHANNEL B", 16384),
        ];
        assert!(
            phantom_slot_indices(&sticks, Some(32768)).is_empty(),
            "正常双通道不应标幻影"
        );
    }

    /// 单条 + 空槽（无幻影）：偏差 0 → 不标，保持原「仅插 1 条」口径。
    #[test]
    fn phantom_slot_indices_empty_for_single_real_stick() {
        let sticks = vec![
            spd_stick("DIMM_A1", "P0 CHANNEL A", 16384),
            stick("DIMM_B1", "P0 CHANNEL B", None),
        ];
        assert!(
            phantom_slot_indices(&sticks, Some(16384)).is_empty(),
            "单条真机不应标幻影"
        );
    }
}
