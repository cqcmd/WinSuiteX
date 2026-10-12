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
}
