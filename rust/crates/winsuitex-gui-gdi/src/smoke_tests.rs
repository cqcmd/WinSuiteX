//! 离屏渲染冒烟测试：把三个页签各画一遍到 1×1 内存 DC。
//!
//! # 为什么必须加这个测试
//!
//! 本项目是**纯 GDI 自绘**，绘制函数没有任何"返回值检查"——一个数组越界、
//! 一个除零都会直接 panic。而 GUI 的失败模式很隐蔽：**只有切到那一页、
//! 那种数据组合才崩**（实测：`col_x[7]` 越界只在磁盘页绘制时爆，
//! 而硬件页正常，程序看起来"大部分是好的"）。
//!
//! 人工点三个页签验一次是不够的——数据组合（分区数 0/1/8、内存全空/满、
//! 功能未装/已装）远比页签多。改成"每次 build都跑一遍三个页签 × N 种数据"，
//! 崩溃会在 CI / 本地门禁阶段就炸出来，而不是等 Z.W. 双击之后。
//!
//! # 数据从哪来
//!
//! **全部是手写的构造数据**，不跑真实采集：
//! - 真实采集（WMI / SMBIOS / COM）在测试进程里跑会带 COM 公寓 / 权限 /
//!   真机差异三重不确定性，测试结果不可复现；
//! - 采集失败时的空快照本来就是必须覆盖的一种组合，`default()` 就能代表。
//!
//! 手写数据能精确构造出"真机上很难复现"的边界组合（8 分区、超长卷标、
//! 内存 4 空槽），这才是测试的价值所在。

use windows::Win32::Graphics::Gdi::*;
use winsuitex_core::model::{
    AudioDevice, AudioKind, DiskSmart, HardwareInfo, HealthStatus, MebiBytes, MemStick, MemoryInfo,
    Nic, PhysicalDisk, Volume,
};
use winsuitex_core::CollectResult;

/// 造一个 1×1 的 32bpp 内存 DC：绘制照样执行（布局照跑），但一个像素都不落。
fn tiny_dc() -> Option<HDC> {
    let screen = unsafe { GetDC(None) };
    let mem = unsafe { CreateCompatibleDC(Some(screen)) };
    unsafe {
        let _ = ReleaseDC(None, screen);
    }
    if mem.is_invalid() {
        return None;
    }
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: 1,
            biHeight: -1,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut core::ffi::c_void = core::ptr::null_mut();
    let bmp = unsafe { CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) };
    let Ok(bmp) = bmp else {
        unsafe {
            let _ = DeleteDC(mem);
        }
        return None;
    };
    // 必须选入位图，否则 SelectObject 位图的那步失败，DC 没有有效绘图面。
    let _ = unsafe { SelectObject(mem, bmp.into()) };
    Some(mem)
}

/// 五个与主程序同口径的字体（微软雅黑 + 负字号逻辑高度）。
///
/// 顺序与 `pages::PageCtx` 字段一一对应：`title / sec / body / sub / tiny`。
type Fonts = (HFONT, HFONT, HFONT, HFONT, HFONT);

fn fonts() -> Fonts {
    let ui: Vec<u16> = "\u{5fae}\u{8f6f}\u{96c5}\u{9ed1}"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    (
        crate::draw::font(&ui, -16, 700),
        crate::draw::font(&ui, -14, 700),
        crate::draw::font(&ui, -13, 400),
        crate::draw::font(&ui, -12, 400),
        crate::draw::font(&ui, -11, 400),
    )
}

/// 关掉测试 DC。`DeleteDC` 返回 `BOOL`，必须显式丢弃。
fn drop_dc(mem: HDC) {
    // SAFETY: mem 是本测试内通过 `tiny_dc` 创建的 1×1 内存 DC，未交给任何窗口。
    unsafe {
        let _ = DeleteDC(mem);
    }
}

/// 造一份页面上下文（`pages::PageCtx`）。
fn ctx<'a>(
    hdc: HDC,
    w: i32,
    h: i32,
    snap: Option<&'a CollectResult>,
    f: Fonts,
) -> crate::pages::PageCtx<'a> {
    let (f_title, f_sec, f_body, f_sub, f_tiny) = f;
    let _ = f_title; // 顶栏专用，页面内用不到
    crate::pages::PageCtx {
        hdc,
        w,
        h,
        snap,
        scroll: 0, // 冒烟测试不涉滚动；截图长图也走 0
        f_sec,
        f_body,
        f_sub,
        f_tiny,
    }
}

/// 一个分区。`n` 决定盘符与容量量级；`long_label` 造超长字段（测裁剪 / 不越界）。
fn volume(n: usize, long_label: bool) -> Volume {
    const LETTERS: [&str; 8] = ["C:", "D:", "E:", "F:", "G:", "H:", "I:", "J:"];
    let gib = 64u64 + (n as u64 + 1) * 97;
    Volume {
        letter: LETTERS[n % LETTERS.len()].to_string(),
        type_name: Some(if long_label {
            "基本固定磁盘集 数据 NTFS 极长的卷标测试串ABCDEFGHIJKLMNOPQRSTUVWXYZ".into()
        } else {
            "基本固定磁盘集".into()
        }),
        fs_name: Some("NTFS".into()),
        sectors_per_cluster: Some(8),
        bytes_per_sector: Some(512),
        cluster_bytes: Some(4096),
        total_bytes: Some(gib << 30),
        free_bytes: Some((gib / 3) << 30),
        vol_label: Some(if long_label {
            "超长卷标-网吧包间测试-2026-Q4-需-每月-盘点-勿删".into()
        } else {
            "SYSTEM".into()
        }),
        disk_index: Some((n % 3) as u32),
    }
}

/// 一根内存条。`cap_mb = None` → 空槽（`is_populated()` 判空槽的正规口径）。
fn stick(locator: &str, cap_mb: Option<u64>) -> MemStick {
    MemStick {
        locator: Some(locator.to_string()),
        bank: Some("BANK 0".into()),
        serial: cap_mb.map(|_| "12345678".into()),
        part_no: cap_mb.map(|_| "M378B1M70".into()),
        vendor: Some("QEMU".into()),
        mem_type: cap_mb.map(|_| "DDR3".into()),
        capacity_mb: cap_mb.map(MebiBytes::new),
        speed_mhz: cap_mb.map(|_| 1600),
    }
}

/// 待测数据组合。
///
/// 每种都对应一类真机上会遇到、但手工点页面很难稳定复现的布局：
/// - `empty`：无任何数据（采集失败 / 刚启动未完成）
/// - `typical`：4 分区 + 4 插槽全插（网吧机常见形态）
/// - `empty_slots`：4 插槽只插 2 根（真机截图暴露的空槽渲染 bug 就是这条路）
/// - `many_volumes`：8 分区 + 超长字段（列宽分配 / 详情卡换行的极端压力）
/// - `zero_filled`：分区容量 / 内存条字段全为 `None`（`unwrap_or(0)` 分支）
fn fixtures() -> Vec<CollectResult> {
    let empty = CollectResult::default();

    let typical = CollectResult {
        volumes: (0..4).map(|n| volume(n, false)).collect(),
        info: HardwareInfo {
            memory: MemoryInfo {
                total_mb: Some(MebiBytes::new(32768)),
                sticks: ["DIMM_A1", "DIMM_A2", "DIMM_B1", "DIMM_B2"]
                    .iter()
                    .map(|l| stick(l, Some(8192)))
                    .collect(),
            },
            ..Default::default()
        },
        ..Default::default()
    };

    let empty_slots = CollectResult {
        volumes: (0..2).map(|n| volume(n, false)).collect(),
        info: HardwareInfo {
            memory: MemoryInfo {
                total_mb: Some(MebiBytes::new(16384)),
                sticks: [
                    stick("DIMM_A1", Some(8192)),
                    stick("DIMM_A2", None),
                    stick("DIMM_B1", Some(8192)),
                    stick("DIMM_B2", None),
                ]
                .into_iter()
                .collect(),
            },
            ..Default::default()
        },
        ..Default::default()
    };

    let many_volumes = CollectResult {
        volumes: (0..8).map(|n| volume(n, true)).collect(),
        info: HardwareInfo {
            memory: MemoryInfo {
                total_mb: Some(MebiBytes::new(65536)),
                sticks: (0..4)
                    .map(|n| stick(&format!("DIMM_X{n}"), Some(16384)))
                    .collect(),
            },
            ..Default::default()
        },
        ..Default::default()
    };

    // 分区字段全空 + 内存条字段全空（mem_type / speed / vendor 均 None）
    // —— 不能拼出 "DDR? / 0 MHz" 这种看着像故障的伪数据。
    let zero_filled = CollectResult {
        volumes: vec![Volume {
            letter: "C:".into(),
            type_name: None,
            fs_name: None,
            sectors_per_cluster: None,
            bytes_per_sector: None,
            cluster_bytes: None,
            total_bytes: None,
            free_bytes: None,
            vol_label: None,
            disk_index: None,
        }],
        info: HardwareInfo {
            memory: MemoryInfo {
                total_mb: None,
                sticks: vec![MemStick {
                    locator: Some("DIMM_A1".into()),
                    bank: None,
                    serial: None,
                    part_no: None,
                    vendor: None,
                    mem_type: None,
                    capacity_mb: None,
                    speed_mhz: None,
                }],
            },
            ..Default::default()
        },
        ..Default::default()
    };

    vec![empty, typical, empty_slots, many_volumes, zero_filled]
}

/// 三页签 × 三种窗口尺寸 × 五种数据组合渲染一遍，任何 panic 都让测试失败。
#[test]
fn renders_all_three_pages_without_panicking() {
    let Some(mem) = tiny_dc() else {
        // 建不出 DC 说明环境异常（无 GDI？），跳过而不是误报失败。
        eprintln!("[skip] 无法创建 1x1 内存 DC，跳过离屏渲染冒烟测试");
        return;
    };
    let fonts = fonts();
    // 三种窗口宽高：覆盖窄窗下的列宽分配与文字裁剪分支。
    //
    // 注意 `pages::draw_*` 都是 safe fn（`unsafe` 封在各自函数内部，
    // 因为它们只碰传进来的 HDC），所以这里不需要 `unsafe` 壳。
    for w in [480i32, 593, 900] {
        for h in [600i32, 948] {
            crate::pages::draw_titlebar(mem, w, h, fonts.0, fonts.3);
            crate::pages::draw_tabbar(mem, fonts.3);
            for snap in fixtures() {
                // 三个页签全画一遍——切页才崩的 bug 就藏在这里。
                let c = ctx(mem, w, h, Some(&snap), fonts);
                let _ = crate::pages::draw_hw_page(&c);
                let _ = crate::pages::draw_disk_page(&c);
                // 右键页读注册表真实状态（只读不改），数据组合对它无差别，
                // 但窗口尺寸仍需遍历（说明文字换行分支）。
                let _ = crate::pages::draw_menu_page(&c);
            }
            // 滚动条：只有溢出时才画，这里给足 max 让它进入绘制分支。
            crate::pages::draw_scrollbar(mem, w, h, 88, h - 152, 0, 100);
            // max_offset = 0（未溢出）必须直接 return，别画半截滑块。
            crate::pages::draw_scrollbar(mem, w, h, 88, h - 152, 40, 0);
        }
    }
    drop_dc(mem);
}

/// `None` 快照路径：采集未完成时页面必须立刻返回 0（不画半截内容、不崩）。
#[test]
fn renders_with_missing_snapshot() {
    let Some(mem) = tiny_dc() else {
        return;
    };
    let c = ctx(mem, 593, 948, None, fonts());
    assert_eq!(crate::pages::draw_hw_page(&c), 0);
    assert_eq!(crate::pages::draw_disk_page(&c), 0);
    drop_dc(mem);
}

/// 磁盘页行命中测试：渲染已知分区表后，垂直扫描命中测试，
/// 验证**每个分区行都能被点到、且序号集合恰好是 0..n（不多不少不漏）**。
///
/// 这条守住的是"点 E: 行但详情卡显示 C: 内容"这类 bug —— 命中矩形与
/// 渲染矩形若各算一套算法，偏移一个滚动量就整体错位；命中区重叠或留缝
/// 也会在这里暴露（少一个 = 点不到，多一个 = 误命中相邻行）。
#[test]
fn disk_row_hit_testing_covers_every_volume_exactly_once() {
    let Some(mem) = tiny_dc() else {
        return;
    };
    let snap = CollectResult {
        volumes: (0..4).map(|n| volume(n, false)).collect(),
        ..Default::default()
    };
    crate::pages::set_sel_vol(0);
    let c = ctx(mem, 593, 948, Some(&snap), fonts());
    let _ = crate::pages::draw_disk_page(&c);

    // 扫描整个窗口纵向：命中的序号集合必须恰好全覆盖且不重复计。
    let mut seen = std::collections::BTreeSet::new();
    for y in 0..1400 {
        if let Some(idx) = crate::pages::vol_row_at(200, y) {
            seen.insert(idx);
        }
    }
    let expected: std::collections::BTreeSet<usize> = (0..snap.volumes.len()).collect();
    assert_eq!(
        seen, expected,
        "命中序号集合应恰好覆盖每个分区行各一次（缺=点不到，多=重叠误命中）"
    );

    // 选中态写入后能读回（详情卡跟随点击的依据）。
    for i in 0..snap.volumes.len() as i32 {
        crate::pages::set_sel_vol(i);
        assert_eq!(crate::pages::sel_vol(), i);
    }
    drop_dc(mem);
}

/// 右键页开关命中测试：渲染后垂直扫描，验证**开关矩形可命中且序号连续**。
///
/// 坐标系错配（内容区用 `OffsetViewportOrgEx(0, -scroll_y)` 绘制，鼠标坐标
/// 却是窗口坐标系）会让"开关明明画在那里、点了没反应"—— 缓存矩形是内容
/// 坐标系的，命中测试不减 `scroll_y` 就整体偏移。这条锁住索引侧的正确性，
/// 偏移量的正确性由 `on_click` 里的 `screen_y - scroll_y()` 保证。
#[test]
fn menu_switch_hit_testing_covers_every_feature() {
    let Some(mem) = tiny_dc() else {
        return;
    };
    let fonts = fonts();
    for w in [480i32, 593, 900] {
        let c = ctx(mem, w, 948, None, fonts);
        let _ = crate::pages::draw_menu_page(&c);
        // FEATURES(5) 始终绘制；经典右键菜单仅在 Windows 11 绘制（slot 0）。
        // 非 Win11 时不渲染经典菜单，命中集合为 {1..5}，不缺不重。
        let n = winsuitex_core::feature::FEATURES.len() + 1;
        let expected: std::collections::BTreeSet<usize> = if crate::pages::is_win11() {
            (0..n).collect()
        } else {
            (1..n).collect()
        };
        // 开关在卡片右侧（`right - PAD_IN - 44`），x 随 pad / dpi 变，
        // 所以横向也全扫、不写死坐标—— 否则改一次 PAD_IN 测试就假失败。
        let mut seen = std::collections::BTreeSet::new();
        for y in 0..1600 {
            for x in (0..w).step_by(4) {
                if let Some(idx) = crate::pages::switch_at(x, y) {
                    seen.insert(idx);
                }
            }
        }
        assert_eq!(
            seen, expected,
            "w={w}: 开关命中序号应完整覆盖（缺=点不动，多=矩形重叠）"
        );
    }
    drop_dc(mem);
}

/// 头部按钮（导出 / 截图）命中测试：复现第二十六轮"竖条拉到最下面时按钮失效"。
///
/// 根因：头部按钮钉在**固定层**（屏幕坐标），但早期 `on_click` / `on_mouse_move`
/// 把点击/ hover 的 y 用**内容坐标** `y = screen_y - scroll_y()` 去判——
/// 滚动越多偏越多，拉到底时 `y` 变负，再也落不到按钮矩形上，导出/截图彻底点不动。
///
/// 修复后统一走 `hit_header(x, screen_y)`（屏幕坐标）。这条锁死两点：
/// 1. 新逻辑（屏幕坐标）对两颗头部按钮都正确命中；
/// 2. 旧逻辑（内容坐标 `screen_y - scroll`）在滚到底时返回 -1，即按钮"消失"——
///    这正是用户反馈的 bug 签名，必须和新逻辑形成对照、不可回退。
#[test]
fn header_buttons_hit_uses_screen_coords_not_content_coords() {
    // 头部按钮矩形依赖 WIN_CACHED_W（绘制时才能拿到真实宽）。测试里手动钉成 593，
    // 与 on_paint 写入的值口径一致；其它页签测试也读这个全局，593 是合法宽不串味。
    crate::pages::WIN_CACHED_W.store(593, std::sync::atomic::Ordering::Relaxed);
    // 头部按钮仅硬件页（cur_tab == 0）有。
    crate::set_cur_tab(0);

    let er = crate::pages::export_btn_rect();
    let sr = crate::pages::shot_btn_rect();
    let ex = er.0 + er.2 / 2; // 导出按钮中心 x
    let ey = er.1 + er.3 / 2; // 导出按钮中心 y（屏幕坐标）
    let sx = sr.0 + sr.2 / 2; // 截图按钮中心 x
    let sy = sr.1 + sr.3 / 2; // 截图按钮中心 y（屏幕坐标）

    // 1. 新逻辑（屏幕坐标）：两颗按钮都正确命中。
    assert_eq!(
        crate::pages::hit_header(ex, ey),
        0,
        "导出按钮在屏幕坐标下必须命中 (0)"
    );
    assert_eq!(
        crate::pages::hit_header(sx, sy),
        1,
        "截图按钮在屏幕坐标下必须命中 (1)"
    );

    // 模拟"滚到底"：一个远超可视区的大滚动量。
    let big_scroll: i32 = 5000;
    let cy_ex = ey - big_scroll; // 旧逻辑传入的内容坐标 y
    let cy_sh = sy - big_scroll;

    // 2. 旧逻辑（内容坐标）在滚到底时按钮"消失"——这是 bug 签名，必须成立，
    //    否则说明"修复"其实没解决偏移问题（回归护栏）。
    assert_eq!(
        crate::pages::hit_test(ex, cy_ex).0,
        -1,
        "回归护栏：旧逻辑(内容坐标)滚到底时导出按钮应失效(-1)，若仍命中说明修复无效"
    );
    assert_eq!(
        crate::pages::hit_test(sx, cy_sh).0,
        -1,
        "回归护栏：旧逻辑(内容坐标)滚到底时截图按钮应失效(-1)，若仍命中说明修复无效"
    );
}

/// 字体回退链实测：`draw::font(FONT_UI, …)` 真正用上的字体必须是雅黑/UI/Segoe 之一，
/// 不允许被 GDI **静默替换**成宋体。
///
/// 根因（第二十七轮实测定案）：`FONT_UI` 曾多打一个 `0x4E`（字母 N），字体名变成
/// 不存在的"微软雅黑N"，GDI 不报错、直接用系统默认宋体顶替——VM 和 Z.W. 真机
/// 全部中招（字母带衬线、中文点阵感）。这条测试在开发机上锁死"字体名写错 =
/// 测试当场红"，而不是等真机截图才发现观感走样。
#[test]
fn font_fallback_resolves_to_intended_face() {
    let f = crate::draw::font(crate::draw::FONT_UI, -16, 400);
    assert!(!f.is_invalid(), "draw::font 应返回有效字体句柄");
    unsafe {
        let dc = GetDC(None);
        assert!(!dc.is_invalid(), "测试进程拿不到屏幕 DC");
        let old = SelectObject(dc, f.into());
        let mut buf = [0u16; 64];
        let n = GetTextFaceW(dc, Some(&mut buf[..]));
        let _ = SelectObject(dc, old);
        let _ = ReleaseDC(None, dc);
        assert!(n > 0, "GetTextFaceW 应返回真名长度");
        // 与 draw::real_face 保持一致：GetTextFaceW 多算结尾 NUL 时要用 trim 去掉，
        // 否则 '微软雅黑\0' != '微软雅黑' 会让比对失败。
        let got = String::from_utf16_lossy(&buf[..(n as usize).min(buf.len())])
            .trim_matches('\u{0}')
            .trim()
            .to_string();
        println!("draw::font(FONT_UI) 实际 face = {got}");
        // 与 draw::font 用同一份可接受名单（英文 + 中文本地化名，GetTextFaceW 两种都可能返回）。
        let ok = crate::draw::FONT_GOOD_FACES
            .iter()
            .any(|w| got.to_lowercase() == w.to_lowercase());
        assert!(
            ok,
            "字体被 GDI 静默替换成 '{got}'（应为雅黑/UI/Segoe）——检查 FONT_UI 码点或回退链"
        );
    }
}

/// 硬件页「硬盘」分组的每一行都应携带与其物理盘 SMART 严格对应的健康徽标，
/// 且非磁盘分组不带任何徽标。这是第二十九轮"硬件页按盘预警"的数据层护栏——
/// 渲染端（`draw_hw_page` 的 `draw_disk_health_badge`）只读 `Section.health`，
/// 若这里对不上，界面就会画错/漏画预警。
#[test]
fn hw_disk_rows_carry_smart_health_badge() {
    let statuses = [
        HealthStatus::Healthy,
        HealthStatus::Warning,
        HealthStatus::Critical,
    ];
    let mut disks: Vec<PhysicalDisk> = statuses
        .iter()
        .enumerate()
        .map(|(i, st)| PhysicalDisk {
            index: i as u32,
            model: Some(format!("SSD-{i}")),
            size_bytes: Some(1_000_000_000_000),
            smart: Some(DiskSmart {
                health_status: *st,
                ..Default::default()
            }),
            ..Default::default()
        })
        .collect();
    // 第 4 块盘：读不到 SMART（网吧机未提权常见）→ `smart = None` → 徽标应为 Unknown 的 None。
    disks.push(PhysicalDisk {
        index: 3,
        model: Some("HDD-old".into()),
        size_bytes: Some(2_000_000_000_000),
        ..Default::default()
    });

    let r = CollectResult {
        info: HardwareInfo {
            disks,
            ..Default::default()
        },
        ..Default::default()
    };

    let sections = crate::pages::build_hw_sections(&r);
    let disk_sec = sections
        .iter()
        .find(|s| s.title == "硬盘")
        .expect("硬件页应有「硬盘」分组");
    // 行数 = 物理盘数
    assert_eq!(disk_sec.rows.len(), 4, "硬盘分组行数应 = 物理盘数");
    // 每行 health 与对应盘 smart.health_status 一致
    for (i, h) in disk_sec.health.iter().enumerate() {
        let expected = r.info.disks[i].smart.as_ref().map(|s| s.health_status);
        assert_eq!(
            *h, expected,
            "硬盘行 {i} 健康徽标应与 PhysicalDisk.smart.health_status 一致"
        );
    }
    // 非磁盘分组不应带任何徽标
    for s in &sections {
        if s.title != "硬盘" {
            assert!(
                s.health.iter().all(|h| h.is_none()),
                "非磁盘分组「{}」不应有健康徽标",
                s.title
            );
        }
    }
}

/// 第三十轮回归：硬件页「网卡」分组只显示已连接（up=true）的网卡。
///
/// 复现真机截图场景——4 块网卡里有 2 块是 169.254.* 的 APIPA 未连接占位地址，
/// 它们 `up=false`，应被过滤掉，不应挤占版面。
#[test]
fn hw_nic_section_shows_only_connected() {
    // 构造 4 块网卡：2 块已连接（真实 IP），2 块未连接（APIPA 占位）。
    // 注意 up 的排列故意错开（第 2 块是未连接的），验证过滤后「主网卡」标签
    // 对应到过滤后的首块（原始索引第 0 块），而非原始索引。
    let nics = vec![
        Nic {
            desc: Some("Realtek PCIe GbE".into()),
            ipv4: vec!["192.168.1.10".into()],
            up: true,
            ..Default::default()
        },
        Nic {
            desc: Some("Intel I219-V".into()),
            ipv4: vec!["169.254.1.2".into()], // APIPA 未连接占位地址
            up: false,
            ..Default::default()
        },
        Nic {
            desc: Some("WireGuard Tunnel".into()),
            ipv4: vec!["10.0.0.5".into()],
            up: true,
            ..Default::default()
        },
        Nic {
            desc: Some("Hyper-V Switch".into()),
            ipv4: vec!["169.254.123.45".into()], // APIPA 未连接占位地址
            up: false,
            ..Default::default()
        },
    ];

    let r = CollectResult {
        info: HardwareInfo {
            nics,
            ..Default::default()
        },
        ..Default::default()
    };

    let sections = crate::pages::build_hw_sections(&r);
    let nic_sec = sections
        .iter()
        .find(|s| s.title == "网卡")
        .expect("硬件页应有「网卡」分组（已连接网卡数 > 0 时应显示）");

    // 只显示已连接的 2 块，未连接 APIPA 占位地址被过滤
    assert_eq!(nic_sec.rows.len(), 2, "网卡分组应只含已连接（up=true）网卡");

    // 过滤后「主网卡」标签必须对应过滤后首块（Realtek），而非原始索引首块
    // —— 这里原始索引首块恰好也是 up，但标签语义仍是"已连接首块"。
    assert_eq!(
        nic_sec.rows[0].0, "主网卡",
        "已连接首块网卡标签应为「主网卡」"
    );
    assert_eq!(
        nic_sec.rows[0].1, "Realtek PCIe GbE · 192.168.1.10",
        "已连接首块应为 Realtek（真实 IP）"
    );
    assert_eq!(
        nic_sec.rows[1].0, "网卡#2",
        "第二块已连接网卡标签应为「网卡#2」"
    );
    assert!(
        nic_sec.rows[1].1.contains("10.0.0.5"),
        "第二块应为 WireGuard（真实 IP），不应是 APIPA 占位"
    );

    // 负向校验：APIPA 未连接地址不应出现在任何行
    for (label, val) in &nic_sec.rows {
        assert!(
            !val.contains("169.254"),
            "未连接网卡「{label}」({val}) 不应出现（169.254 APIPA 应被过滤）"
        );
    }
}

/// 第三十轮回归：当全部网卡都未连接时，「网卡」分组不应出现（避免空壳占位）。
#[test]
fn hw_nic_section_absent_when_all_down() {
    let nics = vec![
        Nic {
            desc: Some("Intel I219-V".into()),
            ipv4: vec!["169.254.1.2".into()],
            up: false,
            ..Default::default()
        },
        Nic {
            desc: Some("Hyper-V Switch".into()),
            ipv4: vec!["169.254.123.45".into()],
            up: false,
            ..Default::default()
        },
    ];
    let r = CollectResult {
        info: HardwareInfo {
            nics,
            ..Default::default()
        },
        ..Default::default()
    };
    let sections = crate::pages::build_hw_sections(&r);
    assert!(
        !sections.iter().any(|s| s.title == "网卡"),
        "全部网卡未连接时不应显示「网卡」分组"
    );
}

/// 本轮回归：硬件页「网卡」分组的每行应带**链路速率**（"X Gbps" / "X Mbps"）。
///
/// 网卡速率是资产盘点的关键信息（网吧常因协商到 100Mbps / 1Gbps 而非 2.5Gbps
/// 而掉速），面板与导出报告必须用同一格式化函数（winsuitex_core::fmt_link_speed）。
#[test]
fn hw_nic_section_shows_link_speed() {
    let nics = vec![
        Nic {
            desc: Some("Realtek PCIe 2.5GbE".into()),
            ipv4: vec!["192.168.1.10".into()],
            up: true,
            speed_bps: Some(2_500_000_000), // 2.5 Gbps
            ..Default::default()
        },
        Nic {
            desc: Some("Intel I219-V".into()),
            ipv4: vec!["10.0.0.5".into()],
            up: true,
            speed_bps: Some(1_000_000_000), // 1 Gbps
            ..Default::default()
        },
    ];
    let r = CollectResult {
        info: HardwareInfo {
            nics,
            ..Default::default()
        },
        ..Default::default()
    };
    let sections = crate::pages::build_hw_sections(&r);
    let nic_sec = sections
        .iter()
        .find(|s| s.title == "网卡")
        .expect("已连接网卡应显示「网卡」分组");
    assert_eq!(nic_sec.rows.len(), 2, "应有 2 行已连接网卡");
    // 2.5 Gbps → "2.5 Gbps"；1 Gbps → "1 Gbps"（fmt_link_speed 去 .0）
    assert!(
        nic_sec.rows[0].1.contains("2.5 Gbps"),
        "首块应显示 2.5 Gbps 速率，实际: {}",
        nic_sec.rows[0].1
    );
    assert!(
        nic_sec.rows[1].1.contains("1 Gbps"),
        "次块应显示 1 Gbps 速率，实际: {}",
        nic_sec.rows[1].1
    );
}

/// 本轮回归：硬件页「音频设备」分组应**逐设备显示真实声卡**，
/// 不再用无用的「未识别默认设备」占位（role 位未标上时旧版整段显示它）。
///
/// 校验：行里出现真实设备名 + 厂商（USB 声卡/耳机厂商）；绝不出现「未识别默认设备」。
#[test]
fn hw_audio_section_lists_real_sound_card() {
    let audio = vec![
        AudioDevice {
            name: "High Definition Audio 设备".into(),
            role: 0,
            kind: AudioKind::Speaker,
            is_bluetooth: false,
            vid: None,
            pid: None,
            manufacturer: None, // 板载 HDA 无 VID，厂商为 None
        },
        AudioDevice {
            name: "USB Audio Device".into(),
            role: 0,
            kind: AudioKind::Headphone,
            is_bluetooth: false,
            vid: Some("0D8C".into()),
            pid: Some("0248".into()),
            manufacturer: Some("C-Media".into()), // USB 声卡真实厂商
        },
    ];
    let r = CollectResult {
        info: HardwareInfo {
            audio,
            ..Default::default()
        },
        ..Default::default()
    };
    let sections = crate::pages::build_hw_sections(&r);
    let aud_sec = sections
        .iter()
        .find(|s| s.title == "音频设备")
        .expect("有音频设备应显示「音频设备」分组");
    // 逐设备列出（总计 + 2 设备 = 3 行）
    assert_eq!(aud_sec.rows.len(), 3, "音频分组应为 总计 + 每设备一行");
    // 设备类型标签在行 key 里（音箱/耳机）；真实声卡厂商在行 value 里。
    let all_keys: String = aud_sec
        .rows
        .iter()
        .map(|(k, _)| k.clone())
        .collect::<Vec<_>>()
        .join(" | ");
    let all_vals: String = aud_sec
        .rows
        .iter()
        .map(|(_, v)| v.clone())
        .collect::<Vec<_>>()
        .join(" | ");
    // 真实声卡厂商应出现（USB 声卡的 C-Media）
    assert!(
        all_vals.contains("C-Media"),
        "音频分组应显示真实声卡厂商 C-Media: {all_vals}"
    );
    // 设备类型标签应出现
    assert!(
        all_keys.contains("音箱") && all_keys.contains("耳机"),
        "音频分组应标注设备类型（音箱/耳机）: {all_keys}"
    );
    // 旧版的「未识别默认设备」占位必须消失
    assert!(
        !all_vals.contains("未识别默认设备"),
        "音频分组不应再出现「未识别默认设备」占位: {all_vals}"
    );
}
