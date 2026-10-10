//! 全项目唯一的「厂商品牌名显示」数据源。
//!
//! 合并自原本散落在以下四处的表，消除「两套表各自维护、漏改其一就出洞」：
//! - `platform/peripheral_db::BRAND_DISPLAY`（键鼠 / 声卡 / 外设品牌）
//! - `report/lib.rs::vendor_zh` 内的 `MAP`（主板 / 显卡 / 网卡 / 内存 / BIOS …）
//! - `report/lib.rs::disk_vendor_from_model` 内的 `FULL`（硬盘型号关键字 → 英文厂商）
//! - `platform/display.rs::K_PNP_VENDOR`（显示器 PnP 三字母码 → 英文品牌 key）
//!
//! 各层（platform / report / gui）一律委托本模块格式化品牌名，本表为唯一真相来源。
//!
//! # 显示格式规范（Z.W. 定案）
//! 所有界面与报告统一输出 **`英文（中文）`**（全角括号），如
//! `Supermicro（超微）`、`Samsung（三星）`。规范英文名与中文名相同（不翻译条目，
//! 如 `AMD`）或 ODM 兜底说明（如 `定制鼠标(ODM)`）时只显示一段，不套括号。
//!
//! # 中文输入反查
//! WMI / SMBIOS 在中文系统上经常**直接返回中文名**（"超微"、"美商安迈"、"三星"），
//! 查表按中文名反查即可命中同一规范条目——两种输入得到完全一致的显示。
//!
//! # 合并约定
//! - 同一品牌多条长尾写法（如 `Gigabyte Technology Co., Ltd.`）共用一个
//!   规范英文名，显示层不暴露采集原文的拼写噪声；
//! - 显示器 PnP 码（如 `HKC` / `CMO`）以码为 key 收敛，中文统一回查本表。

/// 规范条目：`(匹配 key, 规范英文, 中文名)`。
///
/// - `key`：采集层可能出现的英文写法（匹配大小写不敏感，支持首词降级）；
/// - `en`：显示用的规范英文品牌名（可能与 key 不同，如 `ASUSTeK COMPUTER INC.` → `ASUS`）；
/// - `zh`：规范中文名；与 `en` 相同表示该品牌不翻译。
pub const VENDOR_ZH: &[(&str, &str, &str)] = &[
    // —— 键鼠 / 声卡 / 外设（原 BRAND_DISPLAY）——
    ("Cherry", "Cherry", "樱桃"),
    ("Logitech", "Logitech", "罗技"),
    ("Microsoft", "Microsoft", "微软"),
    ("A4Tech", "A4Tech", "双飞燕"),
    ("Apple", "Apple", "苹果"),
    ("Kingston", "Kingston", "金士顿"),
    ("HP", "HP", "惠普"),
    ("ASUS", "ASUS", "华硕"),
    ("Realtek", "Realtek", "瑞昱"),
    ("C-Media", "C-Media", "骅讯"),
    ("Rapoo", "Rapoo", "雷柏"),
    ("SteelSeries", "SteelSeries", "赛睿"),
    ("Razer", "Razer", "雷蛇"),
    ("Corsair", "Corsair", "海盗船"),
    // ODM 代工厂无零售英文品牌，en=zh 只显示兜底说明。
    ("SINO_WEALTH", "定制鼠标(ODM)", "定制鼠标(ODM)"),
    ("SEMICO", "华润上华(ODM)", "华润上华(ODM)"),
    ("SiGma Micro", "Sigma Micro(ODM)", "Sigma Micro(ODM)"),
    ("MosArt", "MosArt(ODM)", "MosArt(ODM)"),
    ("Chicony", "Chicony", "群光"),
    ("HyperX", "HyperX", "极度未知"),
    ("Edifier", "Edifier", "漫步者"),
    ("ROG", "ROG", "玩家国度"),
    // —— 主板 / 整机 / 显卡 AIB（原 vendor_zh::MAP）——
    ("MSI", "MSI", "微星"),
    ("Gigabyte", "GIGABYTE", "技嘉"),
    ("Gigabyte Technology Co., Ltd.", "GIGABYTE", "技嘉"),
    ("ASRock", "ASRock", "华擎"),
    ("ASRock Incorporation", "ASRock", "华擎"),
    ("Supermicro", "Supermicro", "超微"),
    ("Inspur", "Inspur", "浪潮"),
    ("Biostar", "Biostar", "映泰"),
    ("Colorful", "Colorful", "七彩虹"),
    ("Maxsun", "Maxsun", "铭瑄"),
    ("ONDA", "ONDA", "昂达"),
    ("Zotac", "Zotac", "索泰"),
    ("Sapphire", "Sapphire", "蓝宝石"),
    ("XFX", "XFX", "讯景"),
    ("PowerColor", "PowerColor", "撼讯"),
    ("Palit", "Palit", "同德"),
    ("Gainward", "Gainward", "耕升"),
    ("Galax", "Galax", "影驰"),
    ("Inno3D", "Inno3D", "映众"),
    ("ASPEED", "ASPEED", "信骅"),
    ("PNY", "PNY", "PNY"),
    ("Club 3D", "Club 3D", "Club 3D"),
    ("VisionTek", "VisionTek", "VisionTek"),
    ("ASUSTeK", "ASUS", "华硕"),
    ("ASUSTeK COMPUTER INC.", "ASUS", "华硕"),
    ("Dell", "Dell", "戴尔"),
    ("Lenovo", "Lenovo", "联想"),
    ("Hewlett-Packard", "HP", "惠普"),
    ("Intel", "Intel", "英特尔"),
    ("Intel(R) Corporation", "Intel", "英特尔"),
    ("GenuineIntel", "Intel", "英特尔"),
    ("Advanced Micro Devices", "AMD", "AMD"),
    ("AMD", "AMD", "AMD"),
    ("NVIDIA", "NVIDIA", "英伟达"),
    ("Qualcomm", "Qualcomm", "高通"),
    ("Broadcom", "Broadcom", "博通"),
    ("AVAGO", "Broadcom", "博通"),
    ("Avago", "Broadcom", "博通"),
    ("MediaTek", "MediaTek", "联发科"),
    ("Crucial", "Crucial", "英睿达"),
    ("Micron", "Micron", "美光"),
    ("Samsung", "Samsung", "三星"),
    ("SK Hynix", "SK Hynix", "海力士"),
    ("Hynix", "SK Hynix", "海力士"),
    ("Western Digital", "Western Digital", "西部数据"),
    ("WDC", "Western Digital", "西部数据"),
    ("Seagate", "Seagate", "希捷"),
    ("Toshiba", "Toshiba", "东芝"),
    ("Kioxia", "Kioxia", "铠侠"),
    ("Lite-On", "LiteOn", "建兴"),
    ("LiteOn", "LiteOn", "建兴"),
    ("AOC", "AOC", "冠捷"),
    ("Mellanox", "Mellanox", "迈络思"),
    ("American Megatrends", "American Megatrends", "美商安迈"),
    (
        "American Megatrends Inc.",
        "American Megatrends",
        "美商安迈",
    ),
    ("AMI", "American Megatrends", "美商安迈"),
    ("Insyde", "Insyde", "系微"),
    ("Phoenix", "Phoenix", "凤凰科技"),
    ("Byosoft", "Byosoft", "百敖"),
    ("HPE", "HPE", "慧与"),
    ("Matrox", "Matrox", "迈创"),
    ("Marvell", "Marvell", "迈威"),
    ("Cisco", "Cisco", "思科"),
    ("TP-LINK", "TP-Link", "普联"),
    ("TP-Link", "TP-Link", "普联"),
    ("D-Link", "D-Link", "友讯"),
    ("Netgear", "Netgear", "网件"),
    ("Tenda", "Tenda", "腾达"),
    ("Xiaomi", "Xiaomi", "小米"),
    ("Huawei", "Huawei", "华为"),
    ("ZTE", "ZTE", "中兴"),
    // —— 硬盘型号推断的英文厂商列（原 disk_vendor_from_model::FULL）——
    ("Hitachi", "Hitachi", "日立"),
    ("HGST", "HGST", "昱科"),
    ("SanDisk", "SanDisk", "闪迪"),
    ("Solidigm", "Solidigm", "Solidigm"),
    // —— 显示器 PnP 品牌 key（原 K_PNP_VENDOR 的英文列）——
    ("Acer", "Acer", "宏碁"),
    ("Sony", "Sony", "索尼"),
    ("Philips", "Philips", "飞利浦"),
    ("BenQ", "BenQ", "明基"),
    ("ViewSonic", "ViewSonic", "优派"),
    ("Eizo", "Eizo", "艺卓"),
    ("Innolux", "Innolux", "群创"),
    ("AUO", "AUO", "友达"),
    ("BOE", "BOE", "京东方"),
    ("HannStar", "HannStar", "瀚宇彩晶"),
    ("Sharp", "Sharp", "夏普"),
    ("Panasonic", "Panasonic", "松下"),
    ("Fujitsu", "Fujitsu", "富士通"),
    ("Mag", "Mag", "美格"),
    ("CPT", "CPT", "中华映管"),
    ("Daewoo", "Daewoo", "大宇"),
    ("Tatung", "Tatung", "大同"),
    ("Westinghouse", "Westinghouse", "西屋"),
    ("Compaq", "Compaq", "康柏"),
    ("CMO", "CMO", "奇美"),
    ("HKC", "HKC", "惠科"),
];

/// 匹配顺序：
/// 1. 全串精确匹配（忽略大小写）；
/// 2. 取首个单词再匹配（忽略大小写），覆盖 `"Dell Inc."` → `"Dell"`；
/// 3. **中文反查**（新增）：按中文名先全等、再包含（zh 长度 ≥2 防误伤），
///    覆盖中文系统 WMI/SMBIOS 直接返回「超微」「美商安迈」的情况。
///
/// 返回 `(规范英文, 中文名)`；未命中 `None`。
pub fn brand_parts(raw: &str) -> Option<(&'static str, &'static str)> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    // 1) 全串精确匹配（大小写不敏感）
    for (k, en, zh) in VENDOR_ZH {
        if t.eq_ignore_ascii_case(k) {
            return Some((en, zh));
        }
    }
    // 2) 退化为「取首个单词」再匹配（大小写不敏感）
    if let Some(first) = t.split_whitespace().next() {
        for (k, en, zh) in VENDOR_ZH {
            if first.eq_ignore_ascii_case(k) {
                return Some((en, zh));
            }
        }
    }
    // 3) 中文反查：先全等，再包含（含 CJK 的输入才值得跑，纯 ASCII 直接跳过）
    if !t.is_ascii() {
        for (_, en, zh) in VENDOR_ZH {
            if t == *zh {
                return Some((en, zh));
            }
        }
        for (_, en, zh) in VENDOR_ZH {
            if zh.chars().count() >= 2 && t.contains(zh) {
                return Some((en, zh));
            }
        }
    }
    None
}

/// 规范中文名（兼容旧调用：report 的中文列、cli 一致性测试）。
///
/// 中文输入同样能命中（反查），如 `brand_zh("超微") == Some("超微")`。
pub fn brand_zh(raw: &str) -> Option<&'static str> {
    brand_parts(raw).map(|(_, zh)| zh)
}

/// 同 [`brand_zh`]，但未命中时返回原文。零分配，返回值生命周期绑定输入。
pub fn brand_zh_or(raw: &str) -> &str {
    match brand_zh(raw) {
        Some(zh) => zh,
        None => raw,
    }
}

/// 品牌显示名（**Z.W. 定案格式**）：命中时 `英文（中文）`（全角括号）；
/// 规范英文与中文相同（不翻译 / ODM 兜底）只显示一段；未命中 `None`。
///
/// 英文输入与中文输入结果一致：
/// `"Supermicro"`、`"超微"` → `Some("Supermicro（超微）")`。
pub fn brand_display(raw: &str) -> Option<String> {
    brand_parts(raw).map(|(en, zh)| {
        if en == zh {
            en.to_string()
        } else {
            format!("{en}（{zh}）")
        }
    })
}

/// 同 [`brand_display`]，未命中时返回原文（trim 过）。
pub fn brand_display_or(raw: &str) -> String {
    brand_display(raw).unwrap_or_else(|| raw.trim().to_string())
}

/// 物理显卡板卡品牌展示名（GUI 与报告共用的**单一数据源**）。
///
/// 渲染口径与 [`brand_display`] 一致：命中时 `英文（中文）` 全角括号，未收录返回原文。
/// 组合逻辑：
/// 1. `vendor` 有有效值（非空、非 U+FFFD 损坏字符）→ 直接走 [`brand_display_or`]；
/// 2. 否则从 `name` 兜底识别服务器 BMC 显示芯片（ASPEED / MATROX），
///    这类设备采集层常把 `vendor` 留空，但型号串已含厂商标识；
/// 3. 都识别不到 → `None`，绝不臆造品牌。
///
/// 说明：`Gpu.vendor` 由采集层计算所得（`gpu_board_vendor` / `gpu_vendor_from_name`），
/// 不会是 SMBIOS 占位串（`Unknown` / `Default string` 等），故此处无需再套 `is_placeholder`。
pub fn gpu_brand_for_display(vendor: &Option<String>, name: &str) -> Option<String> {
    if let Some(v) = vendor {
        let s = v.trim();
        if !s.is_empty() && !s.contains('\u{FFFD}') {
            return Some(brand_display_or(s));
        }
    }
    let up = name.to_ascii_uppercase();
    for (kw, en) in [("ASPEED", "ASPEED"), ("MATROX", "Matrox")] {
        if up.contains(kw) {
            return Some(brand_display_or(en));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_case_insensitive() {
        assert_eq!(brand_zh("MSI"), Some("微星"));
        assert_eq!(brand_zh("intel"), Some("英特尔"));
        assert_eq!(brand_zh("GIGABYTE"), Some("技嘉"));
    }

    #[test]
    fn first_word_degrade() {
        assert_eq!(brand_zh("Dell Inc."), Some("戴尔"));
        assert_eq!(brand_zh("dell inc."), Some("戴尔"));
        assert_eq!(brand_zh("Advanced Micro Devices"), Some("AMD"));
    }

    #[test]
    fn unknown_passthrough() {
        assert_eq!(brand_zh("SomeWeirdBrand"), None);
        assert_eq!(brand_zh_or("SomeWeirdBrand"), "SomeWeirdBrand");
        assert_eq!(brand_zh("LG"), None);
        assert_eq!(brand_zh_or("LG"), "LG");
    }

    #[test]
    fn pnp_code_resolves_via_core() {
        assert_eq!(brand_zh("HKC"), Some("惠科"));
        assert_eq!(brand_zh("CMO"), Some("奇美"));
        assert_eq!(brand_zh("AOC"), Some("冠捷"));
    }

    // —— 第二十轮：`英文（中文）` 显示格式 + 中文输入反查 ——

    #[test]
    fn display_uses_en_zh_paren_format() {
        assert_eq!(brand_display_or("Supermicro"), "Supermicro（超微）");
        assert_eq!(brand_display_or("Samsung"), "Samsung（三星）");
        // 长尾写法收敛到规范英文名，不暴露采集原文噪声。
        assert_eq!(brand_display_or("ASUSTeK COMPUTER INC."), "ASUS（华硕）");
        assert_eq!(
            brand_display_or("American Megatrends Inc."),
            "American Megatrends（美商安迈）"
        );
        // BIOS 缩写同义归一。
        assert_eq!(brand_display_or("AMI"), "American Megatrends（美商安迈）");
    }

    #[test]
    fn display_untranslated_brands_single_segment() {
        assert_eq!(brand_display_or("AMD"), "AMD");
        assert_eq!(brand_display_or("PNY"), "PNY");
        assert_eq!(brand_display_or("Advanced Micro Devices"), "AMD");
        assert_eq!(brand_display_or("SEMICO"), "华润上华(ODM)");
    }

    #[test]
    fn display_chinese_input_hits_same_entry() {
        // 中文系统 WMI/SMBIOS 直接返回中文名，必须与英文输入同格式。
        assert_eq!(brand_display_or("超微"), "Supermicro（超微）");
        assert_eq!(brand_display_or("三星"), "Samsung（三星）");
        assert_eq!(
            brand_display_or("美商安迈"),
            "American Megatrends（美商安迈）"
        );
        assert_eq!(brand_zh("超微"), Some("超微"));
    }

    #[test]
    fn display_chinese_contains_match() {
        // 带公司后缀的中文长尾（如「超微股份有限公司」）按包含命中。
        assert_eq!(brand_display_or("超微股份有限公司"), "Supermicro（超微）");
    }

    #[test]
    fn display_unknown_falls_back_to_raw_trimmed() {
        assert_eq!(brand_display_or("QEMU"), "QEMU");
        assert_eq!(brand_display_or("  LG  "), "LG");
        assert_eq!(brand_display("QEMU"), None);
    }

    #[test]
    fn display_empty_is_none() {
        assert_eq!(brand_display(""), None);
        assert_eq!(brand_display("   "), None);
        assert_eq!(brand_display_or("   "), "");
    }

    // —— GPU 板卡品牌（GUI / 报告共用单一数据源） ——

    #[test]
    fn gpu_brand_from_vendor_maps_to_chinese() {
        assert_eq!(
            gpu_brand_for_display(&Some("MSI".into()), "NVIDIA GeForce RTX 4070"),
            Some("MSI（微星）".into())
        );
        // 中文系统直返中文厂商名，应与英文输入同格式。
        assert_eq!(
            gpu_brand_for_display(&Some("华硕".into()), "NVIDIA GeForce RTX 4070"),
            Some("ASUS（华硕）".into())
        );
    }

    #[test]
    fn gpu_brand_falls_back_to_name_for_bmc_chips() {
        // 服务器 BMC 显示芯片常把 vendor 留空，从 name 兜底识别。
        assert_eq!(
            gpu_brand_for_display(&None, "ASPEED Graphics Family"),
            Some("ASPEED（信骅）".into())
        );
        assert_eq!(
            gpu_brand_for_display(&None, "Matrox G200eR (Renesas) WDDM 1.2"),
            Some("Matrox（迈创）".into())
        );
    }

    #[test]
    fn gpu_brand_none_when_unknown() {
        assert_eq!(
            gpu_brand_for_display(&None, "Microsoft Basic Render Driver"),
            None
        );
        // vendor 为空串 / 损坏字符 → 走 name 兜底也识别不到 → None。
        assert_eq!(gpu_brand_for_display(&Some("".into()), "Generic GPU"), None);
        assert_eq!(
            gpu_brand_for_display(&Some("�".into()), "Generic GPU"),
            None
        );
    }
}
