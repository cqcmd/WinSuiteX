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

/// 一组真实声卡：板载 HDA 音箱（默认播放）+ 蓝牙耳麦（播放+录音）。
///
/// 重点覆盖**音频分组**渲染路径——VM 默认没有声卡，这条真机才有、
/// 但恰好是 `HwIcon::Audio = 7`（下标最大值）的高危路径；历史上
/// `col_x[7]` 越界就只在"特定数据组合"才爆。
fn audio() -> Vec<AudioDevice> {
    vec![
        AudioDevice {
            name: "Realtek High Definition Audio".into(),
            role: 0b01,
            kind: AudioKind::Speaker,
            is_bluetooth: false,
            vid: None,
            pid: None,
            manufacturer: Some("Realtek".into()),
        },
        AudioDevice {
            name: "Jabra EVOLINK 耳麦".into(),
            role: 0b11,
            kind: AudioKind::Headset,
            is_bluetooth: true,
            vid: Some("0B0E".into()),
            pid: Some("1234".into()),
            manufacturer: Some("Jabra".into()),
        },
    ]
}

/// 待测数据组合。
///
/// 每种都对应一类真机上会遇到、但手工点页面很难稳定复现的布局：
/// - `empty`：无任何数据（采集失败 / 刚启动未完成）
/// - `typical`：4 分区 + 4 插槽全插 + 声卡（网吧机常见形态）
/// - `empty_slots`：4 插槽只插 2 根（真机截图暴露的空槽渲染 bug 就是这条路）
/// - `many_volumes`：8 分区 + 超长字段 + 声卡（列宽分配 / 详情卡换行的极端压力）
/// - `zero_filled`：分区容量 / 内存条字段全为 `None`（`unwrap_or(0)` 分支）
/// - `real_machine`：声卡 + 多插槽（含空槽），贴近同事报错那台真机的组合
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
            audio: audio(),
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
            audio: audio(),
            ..Default::default()
        },
        ..Default::default()
    };

    // 声卡 + 多插槽（2 空槽）：贴近同事在 1024×768 真机报"无界面"那台的组合
    // —— 音频分组是 `HwIcon::Audio = 7`（下标最大），VM 无声卡平时测不到。
    let real_machine = CollectResult {
        volumes: (0..3).map(|n| volume(n, false)).collect(),
        info: HardwareInfo {
            memory: MemoryInfo {
                total_mb: Some(MebiBytes::new(32768)),
                sticks: ["DIMM_A1", "DIMM_A2", "DIMM_B1", "DIMM_B2"]
                    .iter()
                    .map(|l| stick(l, if l.ends_with('1') { Some(8192) } else { None }))
                    .collect(),
            },
            audio: audio(),
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

    vec![
        empty,
        typical,
        empty_slots,
        many_volumes,
        zero_filled,
        real_machine,
    ]
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
        for h in [600i32, 738, 948] {
            crate::pages::draw_titlebar(mem, w, h, fonts.0, fonts.3);
            crate::pages::draw_tabbar(mem, fonts.3, fonts.4);
            // 首屏固定头部：第十八轮之前这里曾有越界 bug（gdi_err.txt 崩溃点
            // 就在同区域），必须随窗口尺寸一起遍历，否则"头部在 1024×768 才崩"
            // 这类回归测不出来。
            crate::pages::draw_hw_header(mem, w, h, fonts.1, fonts.2);
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
        // 命中集合随实际渲染走，必须和 `build_menu_page` 的过滤口径逐字一致：
        // - slot 0（经典菜单）仅 Windows 11 渲染；
        // - slot 1、2（计算机管理 / 设备管理器）两种菜单都可见，始终渲染；
        // - slot 3、4、5（显示隐藏文件 / 命令提示符 / PowerShell 在此处打开）
        //   仅"Win11 且经典菜单【关闭】"时不渲染（新版菜单看不到、无意义）；
        //   Win10 及更早没有新版菜单，或 Win11 已开经典菜单，这 3 项都照常渲染。
        let win11 = crate::pages::is_win11();
        let classic = crate::pages::classic_menu_on();
        // 必须与 `draw_menu_page` 的过滤口径逐字一致：
        // - slot 0（经典菜单）仅 `win11` 时出现在命中集合；
        // - 扩展项仅在 `win11 && !classic` 时隐藏。
        let hide_extras = win11 && !classic;
        let mut expected: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
        expected.insert(1);
        expected.insert(2);
        if win11 {
            expected.insert(0);
        }
        if !hide_extras {
            expected.insert(3);
            expected.insert(4);
            expected.insert(5);
        }
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

/// 硬件页「硬盘」行应携带分区格式（文件系统）：按 `disk_index` 把卷挂回物理盘，
/// 去重后以 "+" 拼接；无卷 / 文件系统读不到（unknown 脏数据）的盘不添加（诚实留白）。
/// Z.W. 2026-10-14 要求补齐——HTML 报告分区表本就有文件系统列，面板此前缺这项。
#[test]
fn hw_disk_rows_show_partition_filesystem() {
    let r = CollectResult {
        info: HardwareInfo {
            disks: vec![
                PhysicalDisk {
                    index: 0,
                    model: Some("SSD-A".into()),
                    ..Default::default()
                },
                PhysicalDisk {
                    index: 1,
                    model: Some("SSD-B".into()),
                    ..Default::default()
                },
                PhysicalDisk {
                    index: 2,
                    model: Some("SSD-C".into()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
        volumes: vec![
            Volume {
                letter: "C:".into(),
                disk_index: Some(0),
                fs_name: Some("NTFS".into()),
                ..Default::default()
            },
            Volume {
                letter: "D:".into(),
                disk_index: Some(0),
                fs_name: Some("NTFS".into()), // 同盘重复格式 → 去重
                ..Default::default()
            },
            Volume {
                letter: "E:".into(),
                disk_index: Some(0),
                fs_name: Some("FAT32".into()),
                ..Default::default()
            },
            Volume {
                letter: "F:".into(),
                disk_index: Some(2),
                fs_name: Some("unknown".into()), // 脏数据 → 不显示
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let sections = crate::pages::build_hw_sections(&r);
    let disk_sec = sections
        .iter()
        .find(|s| s.title == "硬盘")
        .expect("硬件页应有「硬盘」分组");
    let (k0, v0) = &disk_sec.rows[0];
    assert_eq!(k0, "硬盘#0");
    assert!(v0.contains("NTFS+FAT32"), "同盘多格式应去重后 + 拼接: {v0}");
    let (_, v1) = &disk_sec.rows[1];
    assert!(
        !v1.contains("NTFS") && !v1.contains("FAT32"),
        "无卷的盘不应显示分区格式: {v1}"
    );
    let (_, v2) = &disk_sec.rows[2];
    assert!(
        !v2.to_lowercase().contains("unknown"),
        "文件系统 unknown 脏数据不应显示: {v2}"
    );
}

/// 分区表类型（GPT/MBR，per-disk）应显示在硬盘行；与文件系统（NTFS，per-volume）是两回事。
/// 读不到分区格式（非提权/IOCTL 被拒）的盘不应显示该 token，诚实留白。
#[test]
fn hw_disk_rows_show_partition_style() {
    let r = CollectResult {
        info: HardwareInfo {
            disks: vec![
                PhysicalDisk {
                    index: 0,
                    model: Some("SSD-A".into()),
                    partition_style: Some("GPT".into()),
                    ..Default::default()
                },
                PhysicalDisk {
                    index: 1,
                    model: Some("SSD-B".into()),
                    partition_style: Some("MBR".into()),
                    ..Default::default()
                },
                PhysicalDisk {
                    index: 2,
                    model: Some("SSD-C".into()),
                    ..Default::default() // 读不到分区格式 → 不加
                },
            ],
            ..Default::default()
        },
        ..Default::default()
    };
    let sections = crate::pages::build_hw_sections(&r);
    let disk_sec = sections
        .iter()
        .find(|s| s.title == "硬盘")
        .expect("硬件页应有「硬盘」分组");
    let (k0, v0) = &disk_sec.rows[0];
    assert_eq!(k0, "硬盘#0");
    assert!(v0.contains("GPT"), "应显示分区表类型 GPT: {v0}");
    let (_, v1) = &disk_sec.rows[1];
    assert!(v1.contains("MBR"), "应显示分区表类型 MBR: {v1}");
    let (_, v2) = &disk_sec.rows[2];
    assert!(
        !v2.contains("GPT") && !v2.contains("MBR"),
        "读不到分区格式不应显示: {v2}"
    );
}

/// 单盘快照：`smart=false` 时 `smart=None`（无徽标形态，值列不收窄）。
fn disk_snapshot(model: &str, smart: bool) -> CollectResult {
    CollectResult {
        info: HardwareInfo {
            disks: vec![PhysicalDisk {
                index: 0,
                model: Some(model.to_string()),
                size_bytes: Some(1_000_000_000_000),
                smart: smart.then(|| DiskSmart {
                    health_status: HealthStatus::Healthy,
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        },
        ..Default::default()
    }
}

/// 第三十一轮回归：量高趟与绘制趟**同宽**——带健康徽标的行值列右界收窄
/// `DISK_BADGE_RESERVE`，量高若仍按全宽折行，长值行「量 1 行、画 2 行」，
/// 总高量矮 → `max_scroll` 偏小 → 滚动到底最后一组被裁，截图长图同步裁尾
/// （Z.W. 2026-10-11 真机：硬盘 4 行全带徽标、3 行折两行，底部「网卡」组看不见）。
///
/// 手法：把盘型号名逐字符加长，直到**真实构建出的硬盘行值**满足
/// 「全宽行数 < 收窄宽行数」（现场用 `wrap_value` 判定，不预设像素宽，
/// 字体/DPI 变了也不脆）；然后比较同一份数据在有无 SMART 徽标两种形态下
/// `draw_hw_page` 的返回总高——带徽标版必须严格更高。
/// 旧 bug（量高一律全宽）下两个返回值相等，本测试即红。
#[test]
fn hw_measure_height_accounts_badge_narrowed_wrapping() {
    let Some(mem) = tiny_dc() else {
        eprintln!("[skip] 无法创建 1x1 内存 DC，跳过量高一致性回归");
        return;
    };
    let fonts = fonts();
    let w = 593i32;
    let h = 948i32;
    let (full_w, narrow_w) = crate::pages::hw_value_widths(w);

    // 找到满足折行差的型号名（上限 64 次防意外死循环）。
    let mut model = String::from("SAMSUNG MZQL1T9HAJR-0007");
    let mut found = false;
    for _ in 0..64 {
        let r = disk_snapshot(&model, true);
        let sec = crate::pages::build_hw_sections(&r)
            .into_iter()
            .find(|s| s.title == "硬盘")
            .expect("硬件页应有「硬盘」分组");
        let v = &sec.rows[0].1;
        let lf = crate::pages::wrap_value(mem, fonts.2, v, full_w).len();
        let ln = crate::pages::wrap_value(mem, fonts.2, v, narrow_w).len();
        if lf < ln {
            found = true;
            break;
        }
        model.push('X');
    }
    assert!(
        found,
        "加长型号名后应能构造出「全宽行数 < 收窄行数」的硬盘值"
    );

    // 快照必须绑定局部变量：`ctx` 借用的是快照引用，临时值会在语句尾释放。
    let r_plain = disk_snapshot(&model, false);
    let r_badged = disk_snapshot(&model, true);
    let plain = ctx(mem, w, h, Some(&r_plain), fonts);
    let badged = ctx(mem, w, h, Some(&r_badged), fonts);
    let t_plain = crate::pages::draw_hw_page(&plain);
    let t_badged = crate::pages::draw_hw_page(&badged);
    drop_dc(mem);
    assert!(
        t_badged > t_plain,
        "带徽标版量高必须严格更高（收窄宽多折一行）：plain={t_plain} badged={t_badged}"
    );
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

/// 第三十一轮回归：内存条类型名**原样显示**，不再被拼成「DDRLPDDR4」。
///
/// `mem_type` 来自 `mem_type_name`，合法值含 `LPDDR4` / `LPDDR5` / `FBD2` 等——
/// 旧代码按 `starts_with("DDR")` 判断是否补前缀，LPDDR 系列不以 DDR 开头，
/// 于是被拼成「DDRLPDDR4」（笔记本机型必踩）。只有纯数字才需要补 DDR 前缀。
#[test]
fn hw_mem_type_is_not_double_prefixed() {
    let r = CollectResult {
        info: HardwareInfo {
            memory: MemoryInfo {
                total_mb: Some(MebiBytes::new(16384)),
                sticks: vec![
                    MemStick {
                        locator: Some("DIMM A".into()),
                        bank: Some("BANK 0".into()),
                        serial: None,
                        part_no: None,
                        vendor: None,
                        mem_type: Some("LPDDR4".into()),
                        capacity_mb: Some(MebiBytes::new(16384)),
                        speed_mhz: Some(3200),
                    },
                    MemStick {
                        locator: Some("DIMM B".into()),
                        bank: Some("BANK 1".into()),
                        serial: None,
                        part_no: None,
                        vendor: None,
                        mem_type: Some("DDR4".into()),
                        capacity_mb: Some(MebiBytes::new(16384)),
                        speed_mhz: Some(3200),
                    },
                ],
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let sections = crate::pages::build_hw_sections(&r);
    let mem = sections
        .iter()
        .find(|s| s.title == "内存")
        .expect("硬件页应有「内存」分组");
    let all: String = mem
        .rows
        .iter()
        .map(|(_, v)| v.clone())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(all.contains("LPDDR4"), "LPDDR4 应原样显示: {all}");
    assert!(
        !all.contains("DDRLPDDR"),
        "LPDDR 前缀不得重复拼接成 DDRLPDDR: {all}"
    );
    assert!(all.contains("DDR4"), "DDR4 应正常显示: {all}");
}

/// 第三十一轮回归：面板内存分组带「通道提示」行（与 HTML 报告同一判定函数）。
///
/// 双路各插 1 条、都在 Channel 0 → 条数是偶数 2，光看「已插 2 / 共 4 插槽」
/// 看不出是单通道。现场维护要的就是面板上这一眼。
#[test]
fn hw_memory_section_shows_channel_warning_for_single_channel_per_node() {
    let r = CollectResult {
        info: HardwareInfo {
            memory: MemoryInfo {
                total_mb: Some(MebiBytes::new(65536)),
                sticks: vec![
                    MemStick {
                        locator: Some("DIMM000".into()),
                        bank: Some("_Node0_Channel0_Dimm0".into()),
                        serial: None,
                        part_no: None,
                        vendor: None,
                        mem_type: None,
                        capacity_mb: Some(MebiBytes::new(32768)),
                        speed_mhz: Some(2400),
                    },
                    MemStick {
                        locator: Some("DIMM100".into()),
                        bank: Some("_Node1_Channel0_Dimm0".into()),
                        serial: None,
                        part_no: None,
                        vendor: None,
                        mem_type: None,
                        capacity_mb: Some(MebiBytes::new(32768)),
                        speed_mhz: Some(2400),
                    },
                ],
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let sections = crate::pages::build_hw_sections(&r);
    let mem = sections
        .iter()
        .find(|s| s.title == "内存")
        .expect("硬件页应有「内存」分组");
    let hit = mem.rows.iter().find(|(k, _)| k == "通道提示");
    assert!(hit.is_some(), "双路各 1 条同通道应出现「通道提示」行");
    assert!(
        hit.expect("上一步已断言存在").1.contains("单通道"),
        "提示文案应含单通道"
    );
}

/// 通道跑满（同一路两个通道各一条）→ 面板**不得**出现通道提示行，避免误报刷屏。
#[test]
fn hw_memory_section_omits_channel_warning_when_channels_are_full() {
    let r = CollectResult {
        info: HardwareInfo {
            memory: MemoryInfo {
                total_mb: Some(MebiBytes::new(32768)),
                sticks: vec![
                    MemStick {
                        locator: Some("DIMM_A1".into()),
                        bank: Some("P0 CHANNEL A".into()),
                        serial: None,
                        part_no: None,
                        vendor: None,
                        mem_type: Some("DDR5".into()),
                        capacity_mb: Some(MebiBytes::new(16384)),
                        speed_mhz: Some(6000),
                    },
                    MemStick {
                        locator: Some("DIMM_B1".into()),
                        bank: Some("P0 CHANNEL B".into()),
                        serial: None,
                        part_no: None,
                        vendor: None,
                        mem_type: Some("DDR5".into()),
                        capacity_mb: Some(MebiBytes::new(16384)),
                        speed_mhz: Some(6000),
                    },
                ],
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let sections = crate::pages::build_hw_sections(&r);
    let mem = sections
        .iter()
        .find(|s| s.title == "内存")
        .expect("硬件页应有「内存」分组");
    assert!(
        mem.rows.iter().all(|(k, _)| k != "通道提示"),
        "通道跑满时不应出现通道提示行"
    );
}

/// 主题色板回归（零回归护栏）：把关键 `C_*` 常量钉在深空玻璃配色上，
/// 防止日后误改把默认观感悄悄换掉（第三十七轮皮肤切换系统已撤销，
/// 回退为单一主题常量；颜色值与原 SKIN_A 一致）。
#[test]
fn theme_constants_regression() {
    use crate::theme::{rgb, DEEP_SPACE, FLUENT};
    // 深空玻璃基线（默认主题）。
    let exp_deep: [(u32, (u8, u8, u8)); 12] = [
        (DEEP_SPACE.C_GRAD_TOP.0, (135, 81, 238)),
        (DEEP_SPACE.C_GRAD_BOT.0, (38, 98, 235)),
        (DEEP_SPACE.C_FOOTER_BG.0, (27, 54, 122)),
        (DEEP_SPACE.C_WIN_BORDER.0, (70, 66, 104)),
        (DEEP_SPACE.C_ON_GRAD_DIM.0, (224, 216, 250)),
        (DEEP_SPACE.C_ON_GRAD_FAINT.0, (198, 190, 240)),
        (DEEP_SPACE.C_GLASS_TINT.0, (20, 24, 52)),
        (DEEP_SPACE.C_GLASS_DARK.0, (64, 60, 96)),
        (DEEP_SPACE.C_ICON.0, (214, 208, 248)),
        (DEEP_SPACE.C_SEL_ROW.0, (216, 240, 228)),
        (DEEP_SPACE.C_BTN.0, (79, 70, 229)),
        (DEEP_SPACE.C_TB_CLOSE_HOVER.0, (232, 17, 35)),
    ];
    for (got, want) in exp_deep {
        assert_eq!(got, rgb(want.0, want.1, want.2).0, "深空玻璃色板漂离基线");
    }
    // 流畅灰紫基线（第二主题）。
    let exp_fluent: [(u32, (u8, u8, u8)); 6] = [
        (FLUENT.C_GRAD_TOP.0, (56, 52, 84)),
        (FLUENT.C_GRAD_BOT.0, (32, 32, 50)),
        (FLUENT.C_FOOTER_BG.0, (25, 25, 40)),
        (FLUENT.C_WIN_BORDER.0, (108, 102, 140)),
        (FLUENT.C_GLASS_TINT.0, (30, 30, 44)),
        (FLUENT.C_ICON.0, (204, 204, 218)),
    ];
    for (got, want) in exp_fluent {
        assert_eq!(got, rgb(want.0, want.1, want.2).0, "流畅灰紫色板漂离基线");
    }
    // 0.4.5 固化：两套玻璃透明度均 = 50%。
    assert!(
        (DEEP_SPACE.GLASS_ALPHA - 0.50).abs() < 1e-6,
        "深空玻璃透明度必须固定 0.50"
    );
    assert!(
        (FLUENT.GLASS_ALPHA - 0.50).abs() < 1e-6,
        "流畅玻璃透明度必须固定 0.50"
    );
    // 默认主题（深空）的访问器必须返回深空基线值。
    assert_eq!(
        crate::theme::tint_color(),
        DEEP_SPACE.C_GLASS_TINT,
        "tint_color() 默认应返回深空 navy"
    );
    assert!(
        (crate::theme::glass_alpha() - 0.50).abs() < 1e-6,
        "glass_alpha() 默认应 0.50"
    );
}

/// 磁盘「已用%」告警色：选中行（浅绿底）必须切**深色**告警变体，深底切亮色变体。
///
/// 钉死 Z.W. 反馈"选中第一行后面红色 93% 显不出来"的修复：选中行的已用% 不能被
/// 统一深字吞掉，且浅底上要用深红/深橙保证对比（亮橙在浅绿底对比仅 ~1.7:1 隐身）。
#[test]
fn used_color_selected_row_keeps_alert_visible() {
    use crate::pages::used_color;
    use crate::theme::rgb;
    // 浅底（选中行）：≥90% 深红、≥75% 深橙、正常=选中行深字
    assert_eq!(used_color(93, true), rgb(176, 32, 38), "选中行 ≥90% 应深红");
    assert_eq!(used_color(80, true), rgb(150, 84, 8), "选中行 ≥75% 应深橙");
    assert_eq!(
        used_color(50, true),
        crate::theme::theme().C_SEL_ROW_TEXT,
        "选中行正常档应回落选中行深字"
    );
    // 深底（非选中）：≥90% 亮红（C_ERROR，白字对比充足）
    assert_eq!(
        used_color(93, false),
        crate::theme::theme().C_ERROR,
        "非选中 ≥90% 应亮红 C_ERROR"
    );
    // 关键回归：选中行 ≥90% 的色**不得等于**选中行深字（否则又"显不出来"）
    assert_ne!(
        used_color(93, true),
        crate::theme::theme().C_SEL_ROW_TEXT,
        "选中行 ≥90% 告警不得回落成普通深字（否则红字被吞）"
    );
}

/// 磁盘页自适应窗高**不震荡**的根基：`desired_win_height` 是纯函数、只吃内容量。
///
/// 钉死 2026-10-22「磁盘页短窗口/长窗口反复绘画」事故的收敛条件：
/// 目标高度绝不能依赖**当前**窗口高度（否则 h→desired→h 往返无固定点）。
/// 当时震荡的直接根因是 `present_layered` 的 ULW `psize` 用旧 bb 高把窗口拽回去，
/// 与本函数产出的 SetWindowPos 打架；顺序修复后本函数的纯度就是最后一道防线——
/// 任何给它加 `cur_h` 参数或让返回值随窗口高度抖动的改动，都会让本测试外的
/// 集成路径重新震荡，所以这里连纯度一起钉死。
#[test]
fn desired_win_height_is_content_only_and_clamped() {
    use crate::theme::{scale, DISK_MIN_H, FOOTER_H};

    let cap = crate::max_win_h();
    let min_h = scale(DISK_MIN_H);
    // 取一个远离 clamp 边界的中间内容量（避开 DISK_MIN_H 与 cap 挤在一起的小屏）
    let mid_total = min_h; // → desired = min_h + footer + 16（仍在 clamp 区间内当 cap 足够大）

    // 1) 磁盘页（tab 2）：中段走公式 total + FOOTER_H + 16
    let expect_mid = (mid_total + scale(FOOTER_H) + scale(16)).clamp(min_h, cap);
    assert_eq!(
        crate::desired_win_height(mid_total, 2, cap),
        expect_mid,
        "磁盘页中段应走 total+footer+16 公式"
    );

    // 2) 内容极少 → 钳到 DISK_MIN_H（防窗口畸形）
    assert_eq!(
        crate::desired_win_height(0, 2, cap),
        min_h,
        "内容极少时应钳到 DISK_MIN_H"
    );

    // 3) 内容极多 → 钳到 cap（多分区靠滚动）
    assert_eq!(
        crate::desired_win_height(1_000_000, 2, cap),
        cap,
        "内容极多时应钳到工作区上限"
    );

    // 4) 非磁盘页恒为 cap（切回硬件/右键页窗口长回默认高）
    for tab in [0, 1, 3] {
        assert_eq!(
            crate::desired_win_height(0, tab, cap),
            cap,
            "tab={tab} 应恒为 cap（与内容量无关）"
        );
        assert_eq!(
            crate::desired_win_height(1_000_000, tab, cap),
            cap,
            "tab={tab} 应恒为 cap（与内容量无关）"
        );
    }

    // 5) 纯度/幂等：同一输入重复计算恒等（无隐藏状态——实现里不得读全局窗高）。
    for total in [0, min_h, mid_total, cap, cap * 2] {
        let a = crate::desired_win_height(total, 2, cap);
        let b = crate::desired_win_height(total, 2, cap);
        assert_eq!(a, b, "total={total} 两次计算不一致（存在隐藏状态）");
    }

    // 6) 对 total 单调不减：内容更多窗口只会更高/持平，绝不会反而变矮（震荡源之一）
    let mut prev = 0;
    for total in (0..=cap * 2).step_by(64) {
        let d = crate::desired_win_height(total, 2, cap);
        assert!(
            d >= prev,
            "total={total}: desired={d} < prev={prev}（非单调）"
        );
        prev = d;
    }
}
