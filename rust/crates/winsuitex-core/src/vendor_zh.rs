//! 全项目唯一的「厂商 → 中文名」数据源。
//!
//! 合并自原本散落在以下四处的表，消除「两套表各自维护、漏改其一就出洞」：
//! - `platform/peripheral_db::BRAND_DISPLAY`（键鼠 / 声卡 / 外设品牌）
//! - `report/lib.rs::vendor_zh` 内的 `MAP`（主板 / 显卡 / 网卡 / 内存 / BIOS …）
//! - `report/lib.rs::disk_vendor_from_model` 内的 `FULL`（硬盘型号关键字 → 英文厂商）
//! - `platform/display.rs::K_PNP_VENDOR`（显示器 PnP 三字母码 → 英文品牌 key）
//!
//! 各层（platform / report）一律委托本模块查中文，本表为唯一真相来源。
//!
//! # 合并约定
//! - 同一 key 在多表里中文一致 → 只留一条；
//! - 大小写变体（如 `Gigabyte` / `GIGABYTE`）靠大小写不敏感匹配合并，不重复列；
//! - value 与 key 相同（如 `AMD` / `PNY`）表示「不翻译，原样输出」；
//! - 显示器 PnP 码（如 `HKC` / `CMO`）因无对应英文品牌名，直接以码为 key 收敛中文，
//!   `K_PNP_VENDOR` 也改为存 `(PnP码, 品牌key)`，中文统一回查本表。

/// 全项目唯一的厂商中文名表。
///
/// key 为英文品牌 key / 常见写法（匹配大小写不敏感）；value 为规范中文名。
pub const VENDOR_ZH: &[(&str, &str)] = &[
    // —— 键鼠 / 声卡 / 外设（原 BRAND_DISPLAY）——
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
    // —— 主板 / 整机 / 显卡 AIB（原 vendor_zh::MAP）——
    ("MSI", "微星"),
    ("Gigabyte", "技嘉"),
    ("Gigabyte Technology Co., Ltd.", "技嘉"),
    ("ASRock", "华擎"),
    ("ASRock Incorporation", "华擎"),
    ("Supermicro", "超微"),
    ("Inspur", "浪潮"),
    ("Biostar", "映泰"),
    ("Colorful", "七彩虹"),
    ("Maxsun", "铭瑄"),
    ("ONDA", "昂达"),
    ("Zotac", "索泰"),
    ("Sapphire", "蓝宝石"),
    ("XFX", "讯景"),
    ("PowerColor", "撼讯"),
    ("Palit", "同德"),
    ("Gainward", "耕升"),
    ("Galax", "影驰"),
    ("Inno3D", "映众"),
    ("ASPEED", "信骅"),
    ("PNY", "PNY"),
    ("Club 3D", "Club 3D"),
    ("VisionTek", "VisionTek"),
    ("ASUSTeK", "华硕"),
    ("ASUSTeK COMPUTER INC.", "华硕"),
    ("Dell", "戴尔"),
    ("Lenovo", "联想"),
    ("Hewlett-Packard", "惠普"),
    ("Intel", "英特尔"),
    ("Intel(R) Corporation", "英特尔"),
    ("GenuineIntel", "英特尔"),
    ("Advanced Micro Devices", "AMD"),
    ("AMD", "AMD"),
    ("NVIDIA", "英伟达"),
    ("Qualcomm", "高通"),
    ("Broadcom", "博通"),
    ("AVAGO", "博通"),
    ("Avago", "博通"),
    ("MediaTek", "联发科"),
    ("Crucial", "英睿达"),
    ("Micron", "美光"),
    ("Samsung", "三星"),
    ("SK Hynix", "SK 海力士"),
    ("Hynix", "海力士"),
    ("Western Digital", "西部数据"),
    ("WDC", "西部数据"),
    ("Seagate", "希捷"),
    ("Toshiba", "东芝"),
    ("Lite-On", "建兴"),
    ("LiteOn", "建兴"),
    ("AOC", "冠捷"),
    ("Mellanox", "迈络思"),
    ("American Megatrends", "美商安迈"),
    ("American Megatrends Inc.", "美商安迈"),
    ("AMI", "美商安迈"),
    ("Insyde", "系微"),
    ("Phoenix", "凤凰科技"),
    ("Byosoft", "百敖"),
    ("HPE", "慧与"),
    ("Matrox", "迈创"),
    ("Marvell", "迈威"),
    ("Cisco", "思科"),
    ("TP-LINK", "普联"),
    ("TP-Link", "普联"),
    ("D-Link", "友讯"),
    ("Netgear", "网件"),
    ("Tenda", "腾达"),
    ("Xiaomi", "小米"),
    ("Huawei", "华为"),
    ("ZTE", "中兴"),
    // —— 硬盘型号推断的英文厂商列（原 disk_vendor_from_model::FULL）——
    ("Hitachi", "日立"),
    ("HGST", "昱科"),
    ("SanDisk", "闪迪"),
    ("Solidigm", "Solidigm"),
    // —— 显示器 PnP 品牌 key（原 K_PNP_VENDOR 的英文列）——
    ("Acer", "宏碁"),
    ("Sony", "索尼"),
    ("Philips", "飞利浦"),
    ("BenQ", "明基"),
    ("ViewSonic", "优派"),
    ("Eizo", "艺卓"),
    ("Innolux", "群创"),
    ("AUO", "友达"),
    ("BOE", "京东方"),
    ("HannStar", "瀚宇彩晶"),
    ("Sharp", "夏普"),
    ("Panasonic", "松下"),
    ("Fujitsu", "富士通"),
    ("Mag", "美格"),
    ("CPT", "中华映管"),
    ("Daewoo", "大宇"),
    ("Tatung", "大同"),
    ("Westinghouse", "西屋"),
    ("Compaq", "康柏"),
    ("CMO", "奇美"),
    ("HKC", "惠科"),
];

/// 厂商英文 key / 常见写法 → 规范中文名。
///
/// 匹配顺序：`trim` → 全串精确匹配（忽略大小写）→ 退化为「取首个单词」再匹配
/// （忽略大小写）→ `None`。
///
/// 例：`"Dell Inc."` 先精确失败，再按首词 `"Dell"` 命中 → `Some("戴尔")`。
pub fn brand_zh(raw: &str) -> Option<&'static str> {
    let t = raw.trim();
    // 1) 全串精确匹配（大小写不敏感）
    for (k, v) in VENDOR_ZH {
        if t.eq_ignore_ascii_case(k) {
            return Some(v);
        }
    }
    // 2) 退化为「取首个单词」再匹配（大小写不敏感），覆盖 "Dell Inc." → "Dell" → 戴尔
    if let Some(first) = t.split_whitespace().next() {
        for (k, v) in VENDOR_ZH {
            if first.eq_ignore_ascii_case(k) {
                return Some(v);
            }
        }
    }
    None
}

/// 同 [`brand_zh`]，但未命中时返回原文。零分配，返回值的生命周期绑定输入。
pub fn brand_zh_or(raw: &str) -> &str {
    match brand_zh(raw) {
        Some(zh) => zh,
        None => raw,
    }
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
}
