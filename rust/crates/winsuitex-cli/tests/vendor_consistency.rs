//! 跨层一致性：验证 core / platform / report 三层查到的厂商中文名来自同一数据源。
//! 这些函数均委托 `winsuitex_core::brand_zh_or`，本测试锁死三者输出一致。

use winsuitex_core::brand_zh;
use winsuitex_platform::peripheral_db::brand_display;
use winsuitex_report::vendor_zh;

#[test]
fn cross_layer_vendor_zh_consistent() {
    // 覆盖四张原表中的代表品牌：键鼠 / 声卡 / 主板 / 显卡 / 网卡 / 硬盘 / 显示器 PnP。
    let brands = [
        "Logitech",
        "Razer",
        "Realtek",
        "Intel",
        "Dell",
        "Matrox",
        "HKC",
        "Seagate",
        "Kingston",
        "Broadcom",
        "SEMICO",
    ];
    for b in brands {
        let zh = brand_zh(b).unwrap_or_else(|| panic!("core::brand_zh({b:?}) 不应为 None"));
        assert_eq!(
            brand_display(b),
            zh,
            "platform::brand_display({b:?}) 与 core 不一致"
        );
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
