//! 外设品牌 / VID 数据库（纯数据，不依赖运行时 API）。
//!
//! # 数据来源
//! `D:\jinglingResource\Drives` 下 50 个网吧常用外设厂商驱动目录
//! （Razer / SteelSeries / logitech / rog / HyperX / Edifier / 狼蛛 / 达摩鲨 …），
//! 全树抽取 `VID_` 硬件 ID 后人工核校得到。
//!
//! # 准确率边界（重要，避免"假准确"）
//! 实测发现大量国产电竞外设**共用 ODM VID**，仅靠 VID 无法唯一定品牌：
//! - `1915` 同时出现在 AngryMiao 与 Darmoshark
//! - `248A` 同时出现在 Darmoshark 与 valkyrie
//! - `258A` / `3554` / `3837` / `41E4` 为多个国产鼠标品牌共用
//!
//! 因此本表**只收录 VID→品牌置信度高、可唯一归属**的条目；上述共享 VID
//! **不**录入，必须结合 PID + 运行时 `HidD_GetProductString` 产品串区分
//! （见后续 `hid.rs` 计划）。把共享 VID 硬编进本表会造成误报品牌，是错误做法。
//!
//! # 用途
//! - `brand_for_vid`：把原始 `VID_xxxx` 还原成可识别品牌（如 `1038`→`SteelSeries`）。
//! - `brand_display`：品牌 key → 中文显示名（报告更友好）。
//! - `is_usb_audio_vid`：识别 USB 声卡类 VID，支撑"USB 声卡耳机"判定。
//! - `model_for_pid`：由 `VID+PID` 查真实型号（离线种子表 `pid_model_table`，
//!   来自 `Drives` 各厂商驱动 INF 的硬件 ID）。注意：共享鼠标 ODM VID
//!   （`258A`/`248A`/`3554`/`1915`）在 INF 中**无**型号数据，只能靠运行时
//!   `HidD_GetProductString`；本表仅覆盖干净品牌与音频方案，不作兜底误报。

/// 干净 VID → 品牌（英文规范名）。仅收录可唯一归属者。
///
/// 键为 4 位十六进制（大写，无 `0x` / `VID_` 前缀）。`brand_for_vid` 会先归一化输入。
const VID_BRAND: &[(&str, &str)] = &[
    ("046A", "Cherry"),      // 樱桃键盘
    ("046D", "Logitech"),    // 罗技
    ("045E", "Microsoft"),   // 微软
    ("09DA", "A4Tech"),      // 双飞燕 / 血手幽灵（网吧键鼠主力 ODM）
    ("05AC", "Apple"),       // 苹果（非游戏品牌，仅作识别）
    ("0951", "Kingston"),    // Legacy HyperX（被 HP 收购前）
    ("03F0", "HP"),          // 惠普；现 HyperX 走此 VID
    ("0B05", "ASUS"),        // 华硕 / ROG
    ("0BDA", "Realtek"),     // 瑞昱 USB 声卡
    ("0D8C", "C-Media"),     // 骅讯 USB 声卡（耳机声卡主力）
    ("0E8F", "Rapoo"),       // 雷柏
    ("1038", "SteelSeries"), // 赛睿
    ("1532", "Razer"),       // 雷蛇
    ("1B1C", "Corsair"),     // 海盗船
    ("258A", "SINO_WEALTH"), // ⚠️ 常见国产鼠标 ODM，非单一品牌；仅作"疑似定制鼠标"标记
    // 以下为 ODM（代工厂）VID，不是零售品牌：标厂商名而非品牌名，避免误报成
    // 某个具体牌子。收录前均已核对 USB ID 数据库。
    ("1A2C", "SEMICO"), // 华润上华 —— 网吧低价键鼠主力（真机实测 1A2C:6004 键盘）
    ("1C4F", "SiGma Micro"), // 国产键鼠 ODM（1C4F:0002 键盘、0032 鼠标等）
    ("062A", "MosArt"), // 2.4G 无线键鼠接收器常见 ODM
    ("04F2", "Chicony"), // 群光 —— 笔电内置摄像头/键鼠
];

/// 品牌 key（来自 `VID_BRAND` 右值或驱动目录名）→ 中文显示名。
/// 未列出的品牌 `brand_display` 会原样返回输入（多为英文品牌名，已是可读名）。
const BRAND_DISPLAY: &[(&str, &str)] = &[
    ("Cherry", "樱桃"),
    ("Logitech", "罗技"),
    ("Microsoft", "微软"),
    ("A4Tech", "双飞燕"),
    ("Apple", "苹果"),
    ("Kingston", "金士顿"),
    ("HP", "惠普"),
    ("ASUS", "华硕"),
    ("Realtek", "瑞昱"),
    ("C-Media", "骅讯"),
    ("Rapoo", "雷柏"),
    ("SteelSeries", "赛睿"),
    ("Razer", "雷蛇"),
    ("Corsair", "海盗船"),
    ("SINO_WEALTH", "定制鼠标(ODM)"),
    ("SEMICO", "华润上华(ODM)"),
    ("SiGma Micro", "Sigma Micro(ODM)"),
    ("MosArt", "MosArt(ODM)"),
    ("Chicony", "群光"),
    ("HyperX", "极度未知"),
    ("Edifier", "漫步者"),
    ("ROG", "玩家国度"),
];

/// 归一化 VID 输入：剥离 `VID_` / `0x` 前缀、统一大写，返回 4 位十六进制 `String`。
///
/// 输入示例：`"VID_1038"`、`"0x1038"`、`"1038"`、`"0B05"` 均返回 `"1038"` / `"0B05"`。
/// 同时兼容 5 位带前导零写法（如 `"01038"` → `"1038"`）。无法解析时返回 `None`。
fn normalize_vid(raw: &str) -> Option<String> {
    let mut s = raw.trim().to_ascii_uppercase();
    if let Some(rest) = s.strip_prefix("VID_") {
        s = rest.to_string();
    }
    if let Some(rest) = s.strip_prefix("0X") {
        s = rest.to_string();
    }
    // 接受 4 位；或 5 位且首字符为 0（如 "01038" → "1038"）。
    let candidate = if s.len() == 4 {
        s
    } else if s.len() == 5 && s.starts_with('0') {
        s[1..].to_string()
    } else {
        return None;
    };
    if candidate.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(candidate)
    } else {
        None
    }
}

/// 由 VID 查品牌（英文规范名）。共享 ODM VID 仅返回标记值，不谎报具体品牌。
///
/// 返回 `None` 表示未知 VID（需靠 PID + 运行时产品串进一步判定）。
pub fn brand_for_vid(vid: &str) -> Option<&'static str> {
    let vid = normalize_vid(vid)?;
    VID_BRAND
        .iter()
        .find(|(v, _)| v == &vid.as_str())
        .map(|(_, b)| *b)
}

/// 品牌 key → 中文显示名；未知则原样返回（多为英文品牌名，已可读）。
pub fn brand_display(brand: &str) -> &str {
    BRAND_DISPLAY
        .iter()
        .find(|(b, _)| b.eq_ignore_ascii_case(brand))
        .map(|(_, d)| *d)
        .unwrap_or(brand)
}

/// 是否为 USB 声卡类 VID（C-Media / Realtek / HP 等）。
///
/// 用于支撑"USB 声卡耳机"判定：音频端点回溯到 USB 父节点后，
/// 若父节点 VID 命中此处，可高置信标记"该音频设备是 USB 声卡（多为耳机/耳麦）"。
pub fn is_usb_audio_vid(vid: &str) -> bool {
    matches!(
        normalize_vid(vid).as_deref(),
        Some("0D8C") | Some("0BDA") | Some("03F0") | Some("0951")
    )
}

/// 由 `VID+PID` 查真实型号（离线种子表 `pid_model_table`）。
///
/// 输入 `vid`/`pid` 为 4 位十六进制（大小写、前缀均兼容）。
/// 命中返回真实型号名（如 `1038`+`0472` → `SteelSeries Rival 150 Gaming Mouse`）；
/// 未命中返回 `None`（调用方应回退到运行时产品串或 `brand_for_vid`）。
///
/// **注意**：共享鼠标 ODM VID（`258A`/`248A`/`3554`/`1915`）在驱动 INF 中无型号数据，
/// 因此本表对它们必然返回 `None`——这是预期行为，避免"VID 定品牌"式误报。
pub fn model_for_pid(vid: &str, pid: &str) -> Option<&'static str> {
    let vid = normalize_vid(vid)?;
    let pid = normalize_vid(pid)?;
    crate::pid_model_table::lookup_pid_model(&vid, &pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_for_vid_handles_prefixes() {
        assert_eq!(brand_for_vid("VID_1038"), Some("SteelSeries"));
        assert_eq!(brand_for_vid("0x1532"), Some("Razer"));
        assert_eq!(brand_for_vid("0B05"), Some("ASUS"));
        assert_eq!(brand_for_vid("1038"), Some("SteelSeries"));
    }

    #[test]
    fn brand_for_vid_unknown_returns_none() {
        // 共享 ODM VID 不谎报品牌
        assert_eq!(brand_for_vid("1915"), None);
        assert_eq!(brand_for_vid("248A"), None);
        assert_eq!(brand_for_vid("ZZZZ"), None);
    }

    #[test]
    fn brand_display_maps_known_and_passthrough_unknown() {
        assert_eq!(brand_display("Razer"), "雷蛇");
        assert_eq!(brand_display("SteelSeries"), "赛睿");
        // 未知品牌原样返回
        assert_eq!(brand_display("Attack Shark"), "Attack Shark");
    }

    #[test]
    fn audio_vid_detection() {
        assert!(is_usb_audio_vid("VID_0D8C"));
        assert!(is_usb_audio_vid("0x0BDA"));
        assert!(!is_usb_audio_vid("1038")); // SteelSeries 非声卡
    }

    #[test]
    fn model_for_pid_known_and_unknown() {
        // 种子命中：SteelSeries Rival 150 (VID_1038 & PID_0472)
        assert_eq!(
            model_for_pid("1038", "0472"),
            Some("SteelSeries Rival 150 Gaming Mouse")
        );
        // 大小写 / 0x 前缀兼容
        assert_eq!(
            model_for_pid("0x1038", "0x0472"),
            Some("SteelSeries Rival 150 Gaming Mouse")
        );
        // 共享鼠标 ODM VID 在 INF 中无型号 → 预期 None（不误报）
        assert_eq!(model_for_pid("258A", "1007"), None);
        // 完全未知
        assert_eq!(model_for_pid("FFFF", "FFFF"), None);
    }
}
