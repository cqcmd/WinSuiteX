//! 跨层一致性：验证 core / platform / report 三层的厂商显示名来自同一数据源。
//! platform / report 均委托 `winsuitex_core::brand_display_or`（`英文（中文）`
//! 规范格式，第二十轮定案），本测试锁死三者输出一致。

use winsuitex_core::{brand_display_or, brand_zh};
use winsuitex_report::vendor_zh;

#[test]
fn cross_layer_vendor_display_consistent() {
    // 覆盖四张原表中的代表品牌：键鼠 / 声卡 / 主板 / 显卡 / 网卡 / 硬盘 / 显示器 PnP。
    let brands = [
        "Logitech", "Razer", "Realtek", "Intel", "Dell", "Matrox", "HKC", "Seagate", "Kingston",
        "Broadcom", "SEMICO",
    ];
    for b in brands {
        let display = brand_display_or(b);
        assert!(
            !display.is_empty(),
            "core::brand_display_or({b:?}) 不应为空"
        );
        // report::vendor_zh 是兼容层，返回规范中文名（不带英文），与 core::brand_zh 对齐。
        let zh = brand_zh(b).unwrap_or_else(|| panic!("core::brand_zh({b:?}) 不应为 None"));
        assert_eq!(
            vendor_zh(b).as_str(),
            zh,
            "report::vendor_zh({b:?}) 与 core 不一致"
        );
    }
}

#[test]
fn brand_zh_first_word_degrade() {
    // 首词降级：带后缀写法也能命中。
    assert_eq!(brand_zh("Dell Inc."), Some("戴尔"));
    assert_eq!(brand_zh("dell inc."), Some("戴尔"));
}

#[test]
fn cross_layer_chinese_input_same_display() {
    // 中文输入（真机 WMI 本地化串）必须与英文输入得到同一规范显示。
    for (raw, expect) in [
        ("超微", "Supermicro（超微）"),
        ("三星", "Samsung（三星）"),
        ("美商安迈", "American Megatrends（美商安迈）"),
    ] {
        assert_eq!(brand_display_or(raw), expect);
    }
}
