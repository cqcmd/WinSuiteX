//! 页面渲染：顶栏 / 页签条 / 三个内容页 / 底部栏，以及命中测试与报告导出。
//!
//! 布局采用"96-DPI 基准值 + `scale()`"的方式：几何常量写在 [`theme`]，
//! 绘制时统一缩放，因此高 DPI 下不会错位（与 C++ 版同一策略）。

use windows::Win32::Foundation::{RECT, SIZE};
use windows::Win32::Graphics::Gdi::*;

use winsuitex_core::brand_display_or;
use winsuitex_core::model::GpuKind;
use winsuitex_core::{fmt_link_speed, real_link_speed};

use crate::draw::*;
use crate::icons::{self, HwIcon};
use crate::theme::*;
// 共享的应用状态（页签 / 悬停 / 采集快照 / 版权文案）由 main.rs 持有。
use crate::{
    cur_tab, hover_export, hover_shot, hover_switch, hover_tb, snapshot, FOOTER_COPYRIGHT,
};

// ============================================================
// 顶栏（紫渐变 + 窗口按钮）
// ============================================================

/// 内容区几何 —— **全文件唯一真源**。
///
/// 自绘阴影删除后 `body_pad()` 恒为 0（`SHADOW_PAD = 0`），窗口 = 内容矩形。
/// 访问器保留的原因：布局/命中测试/窗口尺寸**全部经由它们取边界**，
/// 若将来要再加任何"窗口外圈留白"（比如贴边任务栏避让），改这一处即可，
/// 不必翻十几个调用点。
///
/// - [`body_pad`]：窗口外圈留白（当前 0）；
/// - [`content_top`]：内容区顶（= 留白 + 顶栏 + 页签）；
/// - [`content_left`] / [`content_right`]：内容区左右边界（给定窗口宽 `w`）。
#[inline]
pub fn body_pad() -> i32 {
    scale(SHADOW_PAD)
}

/// 内容区顶边 y（页签条下沿）。
#[inline]
pub fn content_top() -> i32 {
    scale(SHADOW_PAD + TITLE_H + TAB_H)
}

/// 内容区左边 x。
#[inline]
pub fn content_left() -> i32 {
    scale(SHADOW_PAD)
}

/// 内容区右边 x（**不含**）。
#[inline]
pub fn content_right(w: i32) -> i32 {
    w - scale(SHADOW_PAD)
}

/// 顶栏 + 窗口按钮（最小化/最大化/关闭）。
///
/// 顶栏**不再单独铺底色**——整窗渐变已经铺满（见 [`draw_window_bg`]），
/// 顶栏只是渐变最上面那一截（C++ 版就是这样：顶栏与内容区是同一条渐变，
/// 不存在色块接缝）。
pub fn draw_titlebar(hdc: HDC, w: i32, h: i32, f_title: HFONT, f_sub: HFONT) {
    let th = scale(TITLE_H);
    let pad = scale(SHADOW_PAD);
    let r = RECT {
        left: pad,
        top: pad,
        right: w - pad,
        bottom: pad + th,
    };

    // 应用名 + 图标瓦片
    let tile = RECT {
        left: pad + scale(MARGIN - 8),
        top: pad + scale((TITLE_H - 22) / 2),
        right: pad + scale(MARGIN - 8) + scale(22),
        bottom: pad + scale((TITLE_H + 22) / 2),
    };
    // 瓦片底 = 渐变上叠白 22%（玻璃主题里没有实色块，所有"底"都是叠出来的）。
    fill_glass_round(hdc, &tile, scale(6), C_WHITE, 0.22, 0, h);
    icons::draw_app_icon(hdc, &tile);
    let name = RECT {
        left: tile.right + scale(10),
        top: r.top,
        // 右边界原先写死 scale(MARGIN*4)=96px，标题被截成"WinSu"；给足到半宽。
        right: w / 2,
        bottom: r.bottom,
    };
    text(hdc, &name, "WinSuiteX", f_title, C_ON_GRAD, false);

    // 窗口按钮（右上角两键：最左=最小化，最右=关闭；最大化键已按反馈取消）。
    let bw = scale(46);
    let labels = ["\u{2500}", "\u{2715}"]; // ─ 最小化 / ✕ 关闭
    let closers = [r.right - bw * 2, r.right - bw];
    for (i, lx) in closers.iter().enumerate() {
        let br = RECT {
            left: *lx,
            top: r.top,
            right: *lx + bw,
            bottom: r.bottom,
        };
        let hovered = hover_tb() == i as i32;
        // hover 底同样是"渐变叠白"，不叠实灰（实灰会在紫底上显脏）。
        // 关闭键 hover 用红，沿用系统习惯。
        if hovered {
            let c = if i == 1 {
                C_TB_CLOSE_HOVER
            } else {
                over(grad_at(r.top, h), C_WHITE, 0.22)
            };
            fill(hdc, &br, c);
        }
        // 字形在键内居中（Windows 惯例；原先 DT_RIGHT 会让 ✕ 贴到窗口右缘）。
        text_center(hdc, &br, labels[i], f_sub, C_ON_GRAD);
    }
}

/// 窗口外框：白底 + 圆角描边。
///
/// **曾有"三层实色自绘阴影"，被 Z.W. 真机反馈"多了一层底"后删除。**复盘：
/// 1. GDI 无 alpha 通道，阴影只能用三层灰矩形近似——本来就是妥协；
/// 2. 实现时三层顺序还写反了（先画最小最深的、最后画全窗最浅的，
///    前两层全被盖住），实际渲染出一圈**均匀的灰边**——不是渐变阴影，
///    看起来就像窗口里还套了一层底；
/// 3. 描边画在内缩 `SHADOW_PAD` 的 body 上，与 `SetWindowRgn` 裁出的窗口
///    圆角**不同心**，窗口真实边缘没有任何描边——"圆角是虚的"。
///
/// 现在的定案：**窗口 = 内容矩形本身**。全窗铺 `C_BG`，贴边画一圈
/// `C_WIN_BORDER`（中深灰，任何壁纸下都可见），圆角交给 `SetWindowRgn`。
/// 桌面阴影由 Windows 11 DWM 对圆角窗口自动补（`WS_POPUP` + region 圆角
/// 在 Win11 会走系统圆角阴影路径；Win10 无阴影但浅灰描边足够定义轮廓）。
/// 现在更进一步：**整窗铺紫蓝渐变**（`(135,81,238)` → `(38,98,235)`，
/// C++ 版实测值），贴边一圈叠白描边 `C_WIN_BORDER`。所有卡片都是在这条渐变上
/// 叠白/叠暗"透"出来的（见 `draw::fill_glass*`），因此窗口与内容是一体的，
/// 不存在"白底 + 紫色顶栏"的割裂感。
///
/// 圆角仍由 `SetWindowRgn` 裁——GDI 画不出圆角外的透明像素，region 是唯一解。
pub fn draw_window_frame(hdc: HDC, w: i32, h: i32) {
    let r = scale(WIN_RADIUS);
    let body = RECT {
        left: 0,
        top: 0,
        right: w,
        bottom: h,
    };
    fill_gradient_v(hdc, &body, C_GRAD_TOP, C_GRAD_BOT);
    stroke_round(hdc, &body, r, C_WIN_BORDER, scale(1));
}

// ============================================================
// 页签条
// ============================================================

/// 三个页签（硬件配置 / 右键增强 / 磁盘信息）。
pub fn draw_tabbar(hdc: HDC, f_sub: HFONT) {
    let pad = body_pad();
    let top = pad + scale(TITLE_H);
    let th = scale(TAB_H);
    // 页签条不铺底色，`bar` 只用于取左右边界（见 `tab_at` 的同源约定）。
    // 页签条不铺底色，直接坐在渐变上（C++ 版同）——
    // 铺实色条会在渐变中间横切一刀，非常显眼。
    let names = ["硬件配置", "右键增强", "磁盘信息"];
    for (i, n) in names.iter().enumerate() {
        let x = content_left() + scale(MARGIN) + scale(TAB_W) * i as i32;
        let r = RECT {
            left: x,
            top: top + scale(6),
            right: x + scale(TAB_W),
            bottom: top + th - scale(6),
        };
        let active = cur_tab() == i as i32;
        if active {
            // 选中页签：玻璃高亮底 + 文字提亮到纯白，比仅下划线更醒目。
            // 浅玻璃（C_WHITE 0.12）叠在渐变上、不切断整体色调（页签条不铺实色条的铁律）。
            // 页签条在固定层、不随滚动，故 scroll=0；win_h 用真实窗口高以对齐背景渐变。
            fill_glass_round(
                hdc,
                &r,
                scale(8),
                C_WHITE,
                0.12,
                0,
                WIN_CACHED_H.load(std::sync::atomic::Ordering::Relaxed),
            );
        }
        text(
            hdc,
            &r,
            n,
            f_sub,
            if active { C_WHITE } else { C_ON_GRAD_FAINT },
            true,
        );
        if active {
            // 选中下划线：白色圆角条（C++ 版是白色实心下划线），高亮底之上的强调线。
            fill_round(
                hdc,
                &RECT {
                    left: x + scale(12),
                    top: top + th - scale(10),
                    right: x + scale(TAB_W) - scale(12),
                    bottom: top + th - scale(7),
                },
                scale(2),
                C_WHITE,
            );
        }
    }
}

/// 命中测试：页签区返回页签索引，否则 -1。
pub fn tab_at(x: i32) -> i32 {
    let x0 = content_left() + scale(MARGIN);
    for i in 0..3 {
        let a = x0 + scale(TAB_W) * i;
        let b = a + scale(TAB_W);
        if x >= a && x < b {
            return i;
        }
    }
    -1
}

// ============================================================
// 底部提示条 + 版权
// ============================================================

/// 底部：随页签变化的操作提示 + 版权/版本行。
pub fn draw_footer(hdc: HDC, w: i32, h: i32, f_sub: HFONT, f_tiny: HFONT) {
    let fh = scale(FOOTER_H);
    let pad = body_pad();
    // 底栏也内缩：外圈留白归阴影，否则阴影会被底栏切成直角。
    let top = h - pad - fh;
    let fr = RECT {
        left: content_left(),
        top,
        right: content_right(w),
        bottom: h - pad,
    };
    // 底栏是**实色深藏青**（C++ 版实测 `(27,54,122)`）——比渐变底更深，
    // 把整窗重心压住，也让"提示 + 版权"这一行有明确的归属。
    fill(hdc, &fr, C_FOOTER_BG);

    // 右键页签的提示随 StartAllBack 状态自适应：接管态下经典菜单开关被禁用，
    // "切换经典菜单会自动重启资源管理器"不会发生，照抄反而误导。
    let menu_hint = if startallback_present() {
        "右键项即时生效；经典菜单由 StartAllBack 接管，无需重启资源管理器"
    } else {
        "右键项即时生效，切换经典菜单会自动重启资源管理器"
    };
    let hints = [
        "页面仅展示常规信息，完整明细请点击右上角「导出详细报告」",
        menu_hint,
        "选中行查看详情，插入新盘后可点刷新",
    ];
    let hint = hints[cur_tab() as usize % hints.len()];
    let hr = RECT {
        left: fr.left + scale(MARGIN),
        top: top + scale(10),
        right: fr.right - scale(MARGIN),
        bottom: top + scale(30),
    };
    text_center(hdc, &hr, hint, f_sub, C_ON_GRAD_DIM);

    let ver = format!("{} · v{}", FOOTER_COPYRIGHT, env!("WSX_GUI_VERSION"));
    let cr = RECT {
        left: fr.left + scale(MARGIN),
        top: top + scale(30),
        right: fr.right - scale(MARGIN),
        bottom: fr.bottom - scale(4),
    };
    text_center(hdc, &cr, &ver, f_tiny, C_ON_GRAD_FAINT);
}

// ============================================================
// 命中测试（导出按钮 + 窗口按钮）
// ============================================================

/// 返回 (命中的头部按钮：0=导出报告 / 1=截图 / -1=无, 命中的窗口按钮索引)。
pub fn hit_test(x: i32, y: i32) -> (i32, i32) {
    // 头部两颗按钮在硬件页右上角（截图在导出左边）。
    if cur_tab() == 0 {
        if hit_rect(x, y, export_btn_rect()) {
            return (0, -1);
        }
        if hit_rect(x, y, shot_btn_rect()) {
            return (1, -1);
        }
    }
    // 窗口按钮（顶栏右侧两键）：布局与 draw_titlebar 完全一致。
    // 从右往左数 (right - x)/bw：=1 是最左键(最小化,0)，=0 是最右键(关闭,1)。
    if y < body_pad() + scale(TITLE_H) {
        let bw = scale(46);
        let right = content_right(WIN_CACHED_W.load(std::sync::atomic::Ordering::Relaxed));
        let from_right = (right - x) / bw;
        let btn = if from_right == 1 {
            0 // 最左 = 最小化
        } else if from_right == 0 {
            1 // 最右 = 关闭
        } else {
            -1
        };
        if btn >= 0 {
            return (-1, btn);
        }
    }
    (-1, -1)
}

/// 头部两颗按钮命中（固定层，**用屏幕坐标 `screen_y`**）。
///
/// 与 `hit_test` 的默认参数不同：内容区命中要扣掉滚动偏移（内容坐标系），
/// 但头部按钮钉在固定层、不随滚动移动，必须用屏幕坐标判。否则滚动越多命中区
/// 偏得越多——拉到底时 `y = screen_y - scroll` 变负，点 (389,121) 这类坐标
/// 再也落不到按钮上，截图/导出按钮彻底失效（用户反馈"竖条拉到最下面按钮失效"）。
///
/// 调用方（`on_click` / `on_mouse_move`）应传**未减 scroll 的屏幕 y**，不要传
/// 内容坐标 `y = screen_y - scroll_y()`。
pub fn hit_header(x: i32, screen_y: i32) -> i32 {
    if cur_tab() != 0 {
        return -1; // 头部按钮仅硬件页有
    }
    if hit_rect(x, screen_y, export_btn_rect()) {
        return 0;
    }
    if hit_rect(x, screen_y, shot_btn_rect()) {
        return 1;
    }
    -1
}

/// 点是否落在矩形内（`hit_test` 里每个按钮都要判一次，抽出来省重复）。
fn hit_rect(x: i32, y: i32, r: (i32, i32, i32, i32)) -> bool {
    let (rx, ry, rw, rh) = r;
    x >= rx && x < rx + rw && y >= ry && y < ry + rh
}

/// 导出按钮矩形（硬件页固定头部卡片右上角）。
///
/// 位置与 [`draw_hw_header`] 画的那一颗**同一份算法**（基于 [`hw_header_rect`]），
/// 否则 hover 高亮会和按钮错位——之前 `WIN_CACHED_W` 从不更新就栽过一次同类问题。
///
/// ⚠️ 这颗按钮是**固定头部**的一部分，用屏幕坐标、不随滚动（第十八轮修复：
/// 以前它画在滚动变换内，滚动后既被裁切、点击命中区也错位到 scroll=0 的位置）。
pub fn export_btn_rect() -> (i32, i32, i32, i32) {
    header_btn_rect(scale(120), 0)
}

/// 截图按钮矩形：在导出按钮**左侧**，间隔 8px（第二十二轮新增）。
pub fn shot_btn_rect() -> (i32, i32, i32, i32) {
    header_btn_rect(scale(72), 1)
}

/// 头部按钮的统一几何：从右往左排，`slot=0` 是最右（导出报告），
/// `slot=1` 往左挪一位（截图）。与 [`draw_hw_header`] 的绘制顺序必须一致。
fn header_btn_rect(bw: i32, slot: i32) -> (i32, i32, i32, i32) {
    let w = WIN_CACHED_W.load(std::sync::atomic::Ordering::Relaxed);
    let bh = scale(28);
    let gap = scale(8);
    // 120px：标签"导出详细报告"（6 字 ≈ 90px），96px 会挤到只剩 2~3px 边距。
    let export_bw = scale(120);
    let right = content_right(w) - scale(MARGIN) - scale(PAD_IN);
    let x = if slot == 0 {
        right - bw
    } else {
        right - export_bw - gap - bw
    };
    let hdr = hw_header_rect(w);
    let y = hdr.top + (hdr.bottom - hdr.top - bh) / 2; // 垂直居中于头部卡片
    (x, y, bw, bh)
}

// ============================================================
// 磁盘页交互状态
// ============================================================

thread_local! {
    /// 每行矩形缓存：绘制时写入，点击命中时读取。
    ///
    /// 为什么不改成"点击时重算一遍布局"：布局算法（列宽权重、行高、缩进）只有绘制
    /// 一处实现，两处各算一遍迟早漂移，表现为"点了 A 行高亮跳到 B 行"。缓存即单一真相。
    static VOL_ROWS: std::cell::RefCell<Vec<RECT>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// 当前选中的分区行索引（0 起）。
static SEL_VOL: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// 读选中分区索引。
pub fn sel_vol() -> i32 {
    SEL_VOL.load(std::sync::atomic::Ordering::Relaxed)
}

/// 设置选中分区索引（切页时归零）。
pub fn set_sel_vol(v: i32) {
    SEL_VOL.store(v.max(0), std::sync::atomic::Ordering::Relaxed);
}

/// 点在第几行分区表里（不在表内返回 None）。
pub fn vol_row_at(x: i32, y: i32) -> Option<usize> {
    VOL_ROWS.with(|rows| {
        rows.borrow()
            .iter()
            .position(|r| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
    })
}

/// 缓存窗口宽度（命中测试用，避免每次传参）。
pub static WIN_CACHED_W: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(WIN_W);
/// 缓存窗口高度（玻璃混合 `grad_at(y, win_h)` 需要真实窗口高，否则页签高亮底的渐变
/// 基准色与背景错位；与 `WIN_CACHED_W` 同源、由 `on_paint` 每帧同步）。
pub static WIN_CACHED_H: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(WIN_H);

// ============================================================
// 硬件概览页
// ============================================================

/// 页面绘制的公共入参：目标 DC + 窗口尺寸 + 数据快照 + 一套字体。
///
/// 为什么打包成结构体：原先三个页面函数是 8 个平铺参数
/// （`hdc, w, h, snap, f_sec, f_body, f_sub, f_tiny`），clippy 直接拦
/// `too_many_arguments`；更重要的是每加一个维度就要改三处签名 + 四个调用点。
/// 打包后新增字段只改一处，`draw_*_page(&ctx)` 调用点也不会因参数顺序而错位
/// （字体那五个 HFONT 类型完全一样，顺序写反编译器不报错，只是画出错字重）。
///
/// 字体只有一份且每次 `on_paint` 临时创建，取 `&HFONT` 借用即可，
/// 不必为了廉价拷贝而 `Copy`。
pub struct PageCtx<'a> {
    pub hdc: HDC,
    pub w: i32,
    /// 窗口高度。**当前三个页面都没用它**（都是从上往下顺序排，靠滚动条处理超出），
    /// 但保留在结构体里：分页式布局（页内小节吸顶/分页）第一个要用的就是它，
    /// 到时不必再改三个签名 + 四个调用点。
    #[allow(dead_code)]
    pub h: i32,
    /// `Option<&CollectResult>` 而非 `&Option<..>`：`OnceLock::get()`
    /// 直接返回这个形状，少一层解包；`None` = 采集未完成。
    pub snap: Option<&'a winsuitex_core::CollectResult>,
    /// 当前滚动偏移（玻璃混合要换算回屏幕坐标）。
    ///
    /// 显式入参而不是页面内部读 `crate::scroll_y()`：截图要画**不含滚动的
    /// 完整长图**，必须能传 0（第二十二轮）。
    pub scroll: i32,
    pub f_sec: HFONT,
    pub f_body: HFONT,
    pub f_sub: HFONT,
    pub f_tiny: HFONT,
}

/// 硬件页固定头部卡片矩形（**屏幕坐标**，不随滚动）。
///
/// 头部承载"计算机常规配置"标题与"导出详细报告"按钮，钉在内容区顶部，
/// 滚动时不被 `IntersectClipRect` 裁切（第十八轮修复：曾经画在滚动变换内，
/// 一滚就被裁到页签条下缘，真机多硬件溢出滚动时暴露）。
fn hw_header_rect(w: i32) -> RECT {
    let (left, right) = hw_card_x(w);
    let top = content_top() + scale(10);
    let bottom = top + scale(HW_HEADER_H);
    RECT {
        left,
        top,
        right,
        bottom,
    }
}

/// 正文玻璃卡的左右边界 x（**单一真源**）。
///
/// 头部卡片、正文卡、以及**截图的水平裁切**都取这一对值：几何只有一处定义，
/// 不会出现"界面画在这、截图裁到那"的漂移（观感铁律第 7 条）。
pub fn hw_card_x(w: i32) -> (i32, i32) {
    (
        content_left() + scale(MARGIN),
        content_right(w) - scale(MARGIN),
    )
}

/// 硬件页正文玻璃卡顶边 y（头部卡片下沿 + 留白）。
///
/// 也是**截图的裁切起点**：截图只要"计算机常规配置"下面的分组内容，
/// 顶栏/页签/头部卡片/底栏都是界面外壳，不进图（见 `capture_hw_page`）。
pub fn hw_body_top(w: i32) -> i32 {
    hw_header_rect(w).bottom + scale(10)
}

/// 硬件页固定头部卡片高度（96-DPI 基准，绘制时 `scale()`）。
const HW_HEADER_H: i32 = 46;

/// 遮住固定头部区域（`content_top() .. down_to`），给头部卡一个纯渐变打底。
///
/// `down_to` 要给到**正文卡顶**（头部卡底 + 窄缝）：真机滚动实测（Win11 截图）
/// 残影有两条路径——① 正文从 header 卡上/下的窄缝里直接穿过；
/// ② 正文透过 0.22 半透明玻璃卡身渗出（玻璃不是不透明底！）。
/// 所以先把整块区域用窗体渐变逐行铺满（不透明），再画玻璃卡——
/// scroll=0 时这块本来就是纯渐变，重铺是视觉无操作；滚动后则彻底盖掉穿过/渗出的内容。
fn mask_header_gap(hdc: HDC, w: i32, h: i32, down_to: i32) {
    let mut y = content_top();
    while y < down_to {
        fill(
            hdc,
            &RECT {
                left: 0,
                top: y,
                right: w,
                bottom: y + 1,
            },
            grad_at(y, h),
        );
        y += 1;
    }
}

/// 一个分组小节：图标 + 标题 + 若干键值行。
///
/// 硬件页先把数据整理成这个结构（**纯计算、不碰 DC**），好处见
/// [`draw_hw_page`] 的说明：玻璃卡要先于内容画，可它的高度取决于行数，
/// 只有先把结构建出来才能量出高度。
pub(crate) struct Section {
    pub(crate) icon: HwIcon,
    pub(crate) title: &'static str,
    pub(crate) rows: Vec<(String, String)>,
    /// 每行右侧可选的健康预警徽标（仅「硬盘」分组填充，其余分组为空）。
    /// 与磁盘页详情卡同款色（`smart_color`/`smart_label`），读不到 SMART 时为 `None`
    /// → 不画徽标（诚实留白，不误报"健康"）。
    pub(crate) health: Vec<Option<winsuitex_core::model::HealthStatus>>,
    /// 每行值列（右侧）可选的状态色：用于给温度等数值上阈值色（≥85°红 / ≥75°橙）。
    /// `None` = 用默认白字；与 HTML 报告 `telemetry_line` 的阈值色一致，保持两端观感统一。
    pub(crate) value_colors: Vec<Option<Color>>,
}

/// 硬件页磁盘行健康徽标：实色圆角小条 + 反色状态词。
/// 尺寸随 DPI `scale` 缩放；`DISK_BADGE_RESERVE` 是给值列右侧预留的空白
/// （徽标宽 + 间距），`kv` 绘制时据此收窄值列，避免文字压到徽标。
const DISK_BADGE_W: i32 = 52;
const DISK_BADGE_H: i32 = 20;
const DISK_BADGE_RESERVE: i32 = 64;

/// 只把非空片段用 " · " 拼起来。
///
/// SMBIOS / WMI 大量字段缺失（VM、部分主板），硬拼会显示成
/// `DDR · 0 MHz` 或 ` · 未知`——看着像故障，实际只是没采集到。
fn join_parts(parts: &[String]) -> String {
    parts
        .iter()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// 把 `CollectResult` 整理成分组结构（数据归一全部在这里做，绘制端只管画）。
pub(crate) fn build_hw_sections(r: &winsuitex_core::CollectResult) -> Vec<Section> {
    let info = &r.info;
    let mut out: Vec<Section> = Vec::new();

    // —— 系统 ——
    let os = info.os.as_ref();
    let os_name = os
        .map(|o| {
            let product = o.product.clone().unwrap_or_else(|| "未知系统".into());
            match o.version.clone() {
                Some(v) if !v.trim().is_empty() => format!("{product} · {v}"),
                _ => product,
            }
        })
        .unwrap_or_else(|| "未知".into());
    let os_build = os.and_then(|o| o.build.clone()).unwrap_or_default();
    let mach = info
        .machine
        .as_ref()
        .and_then(|m| m.computer_name.clone())
        .unwrap_or_else(|| "未知".into());
    let user = info
        .machine
        .as_ref()
        .and_then(|m| m.user_name.clone())
        .unwrap_or_default();
    out.push(Section {
        icon: HwIcon::Os,
        title: "系统",
        rows: vec![
            ("操作系统".into(), join_parts(&[os_name, os_build])),
            ("计算机".into(), join_parts(&[mach, user])),
        ],
        health: Vec::new(),
        value_colors: Vec::new(),
    });

    // —— 主板 / BIOS ——
    if let Some(b) = info.board.as_ref() {
        let model = b
            .product
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("未知")
            .to_string();
        let bios = join_parts(&[
            b.bios_vendor
                .as_deref()
                .map(brand_display_or)
                .unwrap_or_default(),
            b.bios_version.clone().unwrap_or_default(),
        ]);
        // 主板厂商单独成行（`英文（中文）` 规范格式，未命中回原文），None 时不加行避免空值。
        let mut board_rows = vec![("型号".into(), model), ("BIOS".into(), bios)];
        if let Some(v) = b.vendor.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            board_rows.insert(0, ("厂商".into(), brand_display_or(v)));
        }
        // 芯片组：SetupAPI 采集，未命中/失败为 None → 不加行（诚实留白，不编造）。
        if let Some(c) = b
            .chipset
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
        {
            board_rows.insert(2, ("芯片组".into(), c.to_string()));
        }
        out.push(Section {
            icon: HwIcon::Board,
            title: "主板 / BIOS",
            rows: board_rows,
            health: Vec::new(),
            value_colors: Vec::new(),
        });
    }

    // —— 处理器 ——
    // ⚠️ 型号与规格必须**分两行**：型号串本身就有 40+ 字符
    // （`Intel(R) Core(TM) i5-3570 CPU @ 3.40GHz`），再拼上"4 核 / 4 线程 · 3400 MHz"
    // 一定超宽，而 `DrawTextW` 单行是**硬裁剪**（直接截断不留省略号），
    // 真机截图上就是"Intel(R) Core(TM) i5-3570 CPU @ 3.4…"这种残串。
    // C++ 版也是拆成"型号 / 规格"两行。
    if !info.cpus.is_empty() {
        let mut rows = Vec::new();
        for c in info.cpus.iter().take(2) {
            let model = c.model.clone().unwrap_or_else(|| "未知".into());
            let spec = format!(
                "{} 核 / {} 线程 · {} MHz",
                c.cores.unwrap_or(0),
                c.threads.unwrap_or(0),
                c.speed_mhz.unwrap_or(0)
            );
            rows.push(("型号".into(), model));
            rows.push(("规格".into(), spec));
        }
        out.push(Section {
            icon: HwIcon::Cpu,
            title: "处理器",
            rows,
            health: Vec::new(),
            value_colors: Vec::new(),
        });
    }

    // —— 内存 ——
    {
        // MebiBytes 是防单位误用的 newtype，取值必须走 .get()。
        let total_gb = info
            .memory
            .total_mb
            .map_or(0.0, |m| m.get() as f32 / 1024.0);
        // 插槽口径：`is_populated()`（容量缺失即空槽），不能拿 `sticks.len()` 当实插数。
        let populated = info
            .memory
            .sticks
            .iter()
            .filter(|s| s.is_populated())
            .count();
        let empty_slots: Vec<_> = info
            .memory
            .sticks
            .iter()
            .filter(|s| !s.is_populated())
            .collect();
        let mut rows = vec![
            ("总计".into(), format!("{total_gb:.1} GiB")),
            (
                "插槽".into(),
                format!("已插 {populated} / 共 {} 插槽", info.memory.sticks.len()),
            ),
        ];
        for s in info.memory.sticks.iter().filter(|s| s.is_populated()) {
            let cap = s.capacity_mb.map_or(0.0, |m| m.get() as f32 / 1024.0);
            let mut parts = vec![
                s.locator.clone().unwrap_or_default(),
                format!("{cap:.0} GiB"),
            ];
            if let Some(t) = s
                .mem_type
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
            {
                // ⚠️ 只有"纯数字"（部分固件/V 老工具只给代际数字）才补 DDR 前缀。
                // 原写法是 `t.starts_with("DDR") ? t : "DDR"+t`，遇到 `mem_type_name`
                // 合法返回的 **LPDDR4 / LPDDR5 / FBD2** 会拼成「DDRLPDDR4」这类错串
                // （笔记本 LPDDR 机型必踩）。含字母即视为完整名称，原样显示。
                parts.push(if t.chars().all(|c| c.is_ascii_digit()) {
                    format!("DDR{t}")
                } else {
                    t.to_string()
                });
            }
            if let Some(mhz) = s.speed_mhz.filter(|m| *m > 0) {
                parts.push(format!("{mhz} MHz"));
            }
            if let Some(v) = s.vendor.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                parts.push(brand_display_or(v));
            }
            rows.push(("插槽".into(), join_parts(&parts)));
        }
        // 空槽单独一行汇总，绝不逐条展开成 "0 GB · DDR? · 0 MHz · [Empty]"。
        if !empty_slots.is_empty() {
            let locs = join_parts(
                &empty_slots
                    .iter()
                    .filter_map(|s| s.locator.clone())
                    .collect::<Vec<_>>(),
            );
            let n = empty_slots.len();
            rows.push(("空槽".into(), join_parts(&[format!("{n} 条未安装"), locs])));
        }
        // 通道提示：与 HTML 报告**同一个判定函数**（`winsuitex_core::memory_channels`），
        // 避免两端各写一套而漂移。现场维护时面板上看一眼就知道是不是单通道，
        // 不必导出报告——双路各插 1 条（都在 Channel 0）这种"条数是偶数、实际单通道"
        // 的情况，光看"已插 2 / 共 16 插槽"是看不出来的（Z.W. 华为真机）。
        // 判定不出来返回 None → 不加行（诚实留白，不编造）。
        if let Some(msg) =
            winsuitex_core::memory_channels::memory_channel_warning(&info.memory.sticks)
        {
            rows.push(("通道提示".into(), msg));
        }
        out.push(Section {
            icon: HwIcon::Mem,
            title: "内存",
            rows,
            health: Vec::new(),
            value_colors: Vec::new(),
        });
    }

    // —— 显卡 ——
    if !info.gpus.is_empty() {
        let mut gpu_rows: Vec<(String, String)> = Vec::new();
        let mut gpu_vc: Vec<Option<Color>> = Vec::new();
        for (i, g) in info.gpus.iter().enumerate() {
            let vram_gb = g.vram_mb.map_or(0.0, |m| m.get() as f32 / 1024.0);
            let sub = if vram_gb > 0.05 {
                format!("{vram_gb:.0} GiB 显存")
            } else {
                "共享显存".to_string()
            };
            let mut parts = vec![g.name.clone(), sub];
            // 板卡品牌（华硕/微星/七彩虹），仅物理显卡展示；
            // 与报告口径一致：vendor 优先，ASPEED/MATROX 从 name 兜底，绝不错译。
            if g.kind == GpuKind::Physical {
                if let Some(brand) = winsuitex_core::gpu_brand_for_display(&g.vendor, &g.name) {
                    // 名称里已经带品牌时不要再挂一次（"Matrox G200e · Matrox" 很啰嗦）。
                    if !g.name.contains(&brand) {
                        parts.push(brand);
                    }
                }
            } else {
                // 虚拟/软件适配器不是真实显卡，显式标注，避免与独显混排误导硬件盘点。
                parts.push(g.kind.label().to_string());
            }
            gpu_rows.push((format!("显卡 #{}", i + 1), join_parts(&parts)));
            gpu_vc.push(None);
            // 温度：只有真正拿到遥测（NVML/ADL）才显示该行并按阈值上色
            // （≥85°红 / ≥75°橙），与 HTML 报告一致；读不到的适配器不画“温度”行，
            // 避免“未读取”噪音（Z.W. 反馈无意义）。
            if let Some(t) = g.telemetry.as_ref().and_then(|t| t.temp_c) {
                gpu_rows.push(("温度".into(), format!("{t} ℃")));
                gpu_vc.push(Some(temp_color(t)));
            }
        }
        out.push(Section {
            icon: HwIcon::Gpu,
            title: "显卡",
            rows: gpu_rows,
            health: Vec::new(),
            value_colors: gpu_vc,
        });
    }

    // —— 硬盘 ——
    // ⚠️ 副行里**不要塞 index**：`硬盘#0` 这个标签已经带了序号，
    // 副行再来一个 `· 0 ·` 就是重复（真机截图上被 Z.W. 一眼看出）。
    if !info.disks.is_empty() {
        let rows = info
            .disks
            .iter()
            .map(|d| {
                let model = d.model.clone().unwrap_or_else(|| "未知".into());
                let sub = join_parts(&[
                    d.iface.clone().unwrap_or_default(),
                    d.media.clone().unwrap_or_default(),
                    format!("{:.1} GiB", d.size_bytes.unwrap_or(0) as f64 / 1073741824.0),
                ]);
                (format!("硬盘#{}", d.index), join_parts(&[model, sub]))
            })
            .collect();
        out.push(Section {
            icon: HwIcon::Disk,
            title: "硬盘",
            rows,
            health: info
                .disks
                .iter()
                .map(|d| d.smart.as_ref().map(|s| s.health_status))
                .collect(),
            value_colors: Vec::new(),
        });
    }

    // —— 网卡 ——
    // ⚠️ 只显示 IP，不拼 MAC：网卡名 + IP + MAC 三连必然超宽被硬裁剪，
    // 而 C++ 版本来也只显示 IP（MAC 在导出的完整报告里有）。
    // ⚠️ 只显示已连接（up=true）的网卡：未连接的多是 169.254.* APIPA 占位地址，
    // 真机截图里它们毫无信息量还挤占版面（Z.W. 第三十轮定案）。
    // 过滤必须在 enumerate 之前做，否则「主网卡」标签会对应到原始索引首块而非已连接首块。
    let nics_up: Vec<&winsuitex_core::model::Nic> = info.nics.iter().filter(|n| n.up).collect();
    if !nics_up.is_empty() {
        let rows = nics_up
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let name = n.desc.clone().unwrap_or_else(|| "未知".into());
                // 多 IPv4 全部显示（用逗号连接），与报告口径一致；仅「已连接但无真实地址」
                // 时才把 APIPA 自动地址列入（避免把拿不到 DHCP 的兜底地址当业务 IP）。
                let ip = winsuitex_core::nic_ipv4_for_display(n).join(", ");
                // 链路速率（bits/s → "1 Gbps" / "2.5 Gbps" / "100 Mbps"）：与报告同一格式化
                // 函数（winsuitex_core::fmt_link_speed），保证面板与导出报告一致。
                // 仅已连接网卡入此分组，speed_bps 通常真实可得；哨兵/0 用 real_link_speed
                // 拦掉，不显示速率。
                let mut parts = vec![name, ip];
                if let Some(bps) = n.speed_bps.filter(|&b| real_link_speed(b)) {
                    parts.push(format!("速率 {}", fmt_link_speed(bps)));
                }
                let label = if i == 0 {
                    "主网卡".to_string()
                } else {
                    format!("网卡#{}", i + 1)
                };
                (label, join_parts(&parts))
            })
            .collect();
        out.push(Section {
            icon: HwIcon::Nic,
            title: "网卡",
            rows,
            health: Vec::new(),
            value_colors: Vec::new(),
        });
    }

    // —— 音频：逐设备显示「真实声卡」——
    // 旧版只取默认播放/录音端点的 name，role 位未标上时（COM 路径未跑/被降级）会整段
    // 显示「未识别默认设备」，毫无信息量。改为逐设备列出：设备名（真实声卡，如
    // "Realtek High Definition Audio" / USB 声卡的硬件名）+ 厂商（USB 声卡/耳机厂商，
    // 板载 HDA 为 None）+ 类型（音箱/耳机/耳麦）+ 默认播放/录音/蓝牙标记。
    // role 位（0b01 播放 / 0b10 录音）仅作标记用，缺失也不影响列出真实声卡。
    if !info.audio.is_empty() {
        let mut rows: Vec<(String, String)> =
            vec![("总计".into(), format!("{} 设备", info.audio.len()))];
        for (i, a) in info.audio.iter().enumerate() {
            let mut v = a.name.clone();
            // 类型 + 角色 + 蓝牙 标记（role 缺失则只带类型/蓝牙）
            let mut tags: Vec<&str> = Vec::new();
            if a.role & 0b01 != 0 {
                tags.push("默认播放");
            }
            if a.role & 0b10 != 0 {
                tags.push("默认录音");
            }
            if a.is_bluetooth {
                tags.push("蓝牙");
            }
            if !tags.is_empty() {
                v.push_str(&format!(" · {}", tags.join(" · ")));
            }
            // 厂商（真实声卡厂商）；已含在设备名里则不重复。
            if let Some(m) = &a.manufacturer {
                if !a.name.contains(m) {
                    v.push_str(&format!(" · {}", m));
                }
            }
            // key 用设备类型（音箱/耳机/耳麦/未知）；未知时退化用序号，避免空 key。
            let key = if a.kind != winsuitex_core::model::AudioKind::Unknown {
                a.kind.label().to_string()
            } else {
                format!("声卡#{}", i + 1)
            };
            rows.push((key, v));
        }
        out.push(Section {
            icon: HwIcon::Audio,
            title: "音频设备",
            rows,
            health: Vec::new(),
            value_colors: Vec::new(),
        });
    }

    out
}

/// 硬件概览：一张玻璃大卡 + 分组小节 + 键值行。
///
/// ## 为什么要"先建结构、再量高、最后画"
/// 玻璃卡是**背景**，必须先于内容绘制；但它的高度取决于内容行数。
/// 以前的写法边画边把 y 往下推，画到哪算哪——那样永远画不出"包住全部内容"的底。
/// 所以改为两段式：`build_hw_sections` 纯计算出结构 → 这里量出总高 →
/// 画玻璃卡 → 再逐段落像素。
///
/// 副作用是**布局算法只有一处**（下面两个 for 循环用同一套增量），
/// 不会出现"量出来的高度 ≠ 画出来的高度"这种错位。
pub fn draw_hw_page(ctx: &PageCtx<'_>) -> i32 {
    let PageCtx {
        hdc,
        w,
        h,
        snap,
        f_sec,
        f_body,
        ..
    } = *ctx;
    let Some(r) = snap else {
        return 0;
    };
    let scroll = ctx.scroll;
    let (left, right) = hw_card_x(w);
    let secs = build_hw_sections(r);

    // —— 第一趟：量高度（不落像素） ——
    // 头部是固定的（见 `on_paint` 调 [`draw_hw_header`]），这里只量"卡片 + 分组小节"。
    // ⚠️ 折行行的行数必须与 `kv` 绘制走同一个 [`wrap_value`]——值超宽的行（网卡
    // "设备名 · IP"、内存空槽列表）会占多行，按 `rows.len()` 量就会量矮，
    // 滚动到底露白、末行被裁（第二十二轮）。
    let pad = scale(PAD_IN);
    let body_top = hw_body_top(w);
    let lay = KvLayout {
        x_key: left + pad,
        x_val_end: right - pad,
        font: f_body,
    };
    let mut bottom = body_top + pad;
    for (i, s) in secs.iter().enumerate() {
        if i > 0 {
            bottom += scale(12);
        }
        bottom += scale(SEC_H);
        // ⚠️ 带健康徽标的行：绘制趟把值列右界收窄 `DISK_BADGE_RESERVE`（见第二趟
        // 的 `lay_row`），量高必须用**同一个数**——否则硬盘这类长值行量高按全宽
        // 算 1 行、绘制按收窄宽折 2 行，总高量矮 → `max_scroll` 偏小 → 滚动到底
        // 最后一组被裁、截图长图同步裁尾（Z.W. 2026-10-11 真机：硬盘 4 行全带
        // 徽标、3 行折两行，「网卡」组整组看不见）。
        for (i, (_, v)) in s.rows.iter().enumerate() {
            let max_w = if s.health.get(i).copied().flatten().is_some() {
                lay.max_w() - scale(DISK_BADGE_RESERVE)
            } else {
                lay.max_w()
            };
            let n = wrap_value(hdc, lay.font, v, max_w).len().max(1) as i32;
            bottom += scale(ROW_H) * n;
        }
    }
    bottom += pad;

    // —— 玻璃大卡（仅正文区；头部单独画在 on_paint 的固定层）——
    let card = RECT {
        left,
        top: body_top,
        right,
        bottom,
    };
    fill_glass_round(
        hdc,
        &card,
        scale(CARD_RADIUS),
        C_GLASS_TINT,
        0.22,
        scroll,
        h,
    );

    // —— 第二趟：画正文分组（lay 复用量高趟构造，几何必然一致）——
    let mut y = body_top + pad;
    for (i, s) in secs.iter().enumerate() {
        if i > 0 {
            y += scale(12);
        }
        y = section(
            &SecCtx {
                hdc,
                left,
                right,
                scroll,
                win_h: h,
                font: f_sec,
            },
            y,
            s.icon,
            s.title,
        );
        for (i, (k, v)) in s.rows.iter().enumerate() {
            // 该行若有健康徽标，给值列右侧预留空白，避免文字压到徽标。
            let val_end = if s.health.get(i).copied().flatten().is_some() {
                lay.x_val_end - scale(DISK_BADGE_RESERVE)
            } else {
                lay.x_val_end
            };
            let lay_row = KvLayout {
                x_key: lay.x_key,
                x_val_end: val_end,
                font: lay.font,
            };
            let y0 = y;
            y = kv(
                hdc,
                &lay_row,
                y,
                k,
                v,
                s.value_colors.get(i).copied().flatten(),
            );
            if let Some(st) = s.health.get(i).copied().flatten() {
                let bw = scale(DISK_BADGE_W);
                let bh = scale(DISK_BADGE_H);
                let bx = right - pad - bw;
                let by = (y0 + y) / 2 - bh / 2;
                draw_disk_health_badge(hdc, bx, by, bw, bh, st, f_body);
            }
        }
    }
    bottom
}

/// 硬件页固定头部：玻璃小卡 + 标题 + 导出按钮。
///
/// 在 `on_paint` 的滚动变换**之外**绘制（屏幕坐标），滚动时钉在内容区顶部——
/// 这是第十八轮修复的核心：标题与"导出详细报告"按钮原本画在滚动变换内，
/// 真机多硬件导致内容溢出滚动后，它们被 `IntersectClipRect` 裁到页签条下缘。
///
/// 同时解决了**点击命中区错位**：`export_btn_rect()` 现在返回的就是这颗固定矩形，
/// 滚动后按钮画在哪、点哪完全一致（以前命中区停留在 scroll=0 位置，滚完点不到）。
pub fn draw_hw_header(hdc: HDC, w: i32, h: i32, f_sec: HFONT, f_body: HFONT) {
    let hdr = hw_header_rect(w);
    // 打底覆盖到正文卡顶（hdr.bottom + 10 窄缝），盖掉穿缝 + 透玻璃渗出的滚动内容。
    mask_header_gap(hdc, w, h, hdr.bottom + scale(10));
    // 头部玻璃卡：渐变上叠深 navy 0.22，与正文卡同一套玻璃主题。
    fill_glass_round(hdc, &hdr, scale(CARD_RADIUS), C_GLASS_TINT, 0.22, 0, h);

    let pad = scale(PAD_IN);
    let th = scale(26);
    // 标题右界让开两颗按钮（导出 120 + 间隔 8 + 截图 72 + 间隔 8），别压到按钮上。
    let head = RECT {
        left: hdr.left + pad,
        top: hdr.top + (hdr.bottom - hdr.top - th) / 2,
        right: hdr.right - pad - scale(120) - scale(72) - scale(16),
        bottom: hdr.top + (hdr.bottom - hdr.top + th) / 2,
    };
    text(hdc, &head, "计算机常规配置", f_sec, C_ON_GRAD, false);

    let (ex, ey, ew, eh) = export_btn_rect();
    let br = RECT {
        left: ex,
        top: ey,
        right: ex + ew,
        bottom: ey + eh,
    };
    button(hdc, &br, "导出详细报告", false, hover_export(), f_body);

    // 截图：把整个硬件页（含滚动后才看得到的部分）渲成一张长图存 PNG。
    let (sx, sy, sw, sh) = shot_btn_rect();
    let sr = RECT {
        left: sx,
        top: sy,
        right: sx + sw,
        bottom: sy + sh,
    };
    button(hdc, &sr, "截图", false, hover_shot(), f_body);
}

// —— 硬件页内部构件 ——

/// 分组小节：一条"叠白 10%"的浅色带 + 强调色简笔图标 + 白色粗体标题。
///
/// C++ 版硬件页没有彩色方瓦片——它是**线性小图标 + 一条比卡片底略亮的带**，
/// 靠"亮一档"而不是"色块"来分层。这里同样处理：带子用玻璃混合画，
/// 图标直接用分组强调色（[`C_ACCENT`]）描边，不垫底色。
fn section(c: &SecCtx, y: i32, icon: HwIcon, title: &str) -> i32 {
    let SecCtx {
        hdc,
        left,
        right,
        scroll,
        win_h,
        font,
    } = *c;
    let band_h = scale(SEC_H);
    let band = RECT {
        left: left + scale(8),
        top: y,
        right: right - scale(8),
        bottom: y + band_h,
    };
    fill_glass_round(hdc, &band, scale(6), C_WHITE, 0.10, scroll, win_h);

    // 图标：14px 简笔，用分组强调色（不垫彩色瓦片，与 C++ 版一致）。
    let tile = RECT {
        left: band.left + scale(12),
        top: y + (band_h - scale(TILE)) / 2,
        right: band.left + scale(12) + scale(TILE),
        bottom: y + (band_h - scale(TILE)) / 2 + scale(TILE),
    };
    icons::draw_hw_icon(hdc, &tile, icon, accent_of(icon));
    let tr = RECT {
        left: tile.right + scale(8),
        top: y,
        right: band.right - scale(12),
        bottom: y + band_h,
    };
    text(hdc, &tr, title, font, C_ON_GRAD, false);
    y + band_h
}

/// 小节绘制的公共入参（把 `section` 的参数从 9 个压到 4 个，过 clippy 门禁）。
struct SecCtx {
    hdc: HDC,
    left: i32,
    right: i32,
    /// 当前滚动偏移（玻璃混合要换算回屏幕坐标才取得到正确渐变色）。
    scroll: i32,
    /// 窗口高度（渐变的取值区间 0..win_h）。
    win_h: i32,
    font: HFONT,
}

/// 分组图标描边色：第九轮改成**柔和粉彩**（`C_ACCENT_SOFT`）——
/// 第八轮一刀切成淡紫单色（`C_ICON`）治"花眼"，但用户又觉得"没了色彩"；
/// 这版每组保留各自色相、只把饱和度压到不刺眼，是两者的折中。
fn accent_of(icon: HwIcon) -> Color {
    C_ACCENT_SOFT[hw_accent_idx(icon)]
}

/// 把 `HwIcon` 映射到 `C_ACCENT` / `C_ACCENT_SOFT` 的下标（顺序与枚举声明一致）。
fn hw_accent_idx(icon: HwIcon) -> usize {
    match icon {
        HwIcon::Os => 0,
        HwIcon::Board => 1,
        HwIcon::Cpu => 2,
        HwIcon::Mem => 3,
        HwIcon::Gpu => 4,
        HwIcon::Disk => 5,
        HwIcon::Nic => 6,
        HwIcon::Audio => 7,
    }
}

/// 右键增强页每一行左侧的功能图标 + 柔彩描边色。
///
/// `idx` 与 `FEATURES` 下标对齐（0..4）；经典右键菜单行不走本映射，
/// 绘制时直接传 `FeatureIcon::Menu`，所以 `_ =>` 分支只是兜底防越界。
/// 颜色从 `C_ACCENT_SOFT` 取各自的粉彩色相，让菜单卡片也"有色彩"但不花眼。
fn feature_icon_for(idx: usize) -> (icons::FeatureIcon, Color) {
    match idx {
        0 => (icons::FeatureIcon::Monitor, C_ACCENT_SOFT[2]), // 计算机管理
        1 => (icons::FeatureIcon::Chip, C_ACCENT_SOFT[1]),    // 设备管理器
        2 => (icons::FeatureIcon::Eye, C_ACCENT_SOFT[6]),     // 显示/隐藏系统文件
        3 => (icons::FeatureIcon::Terminal, C_ACCENT_SOFT[5]), // 命令提示符
        4 => (icons::FeatureIcon::PowerShell, C_ACCENT_SOFT[4]), // PowerShell
        _ => (icons::FeatureIcon::Menu, C_ACCENT_SOFT[0]),    // 经典右键菜单
    }
}

/// 一行"标签 + 值"，返回下一行 y。
///
/// 值串在 [`build_hw_sections`] 里已经拼好（含次级说明），
/// 这里不再接 `sub` 参数——少一个参数少一处"拼错位置"的风险。
///
/// ## 值自动折行（第二十二轮）
/// `DrawTextW` 单行是**硬裁剪**，真机上网卡行"设备名 · IP"超宽后 IP 被直接切没
/// （Z.W. 截图：`…Connection #2 · 169.254.157.?`）。这里按 ` · ` 分隔符贪心折行，
/// 一行放不下就断到下一行，行数多了 y 自然往下长——量高趟走同一 [`wrap_value`]，
/// 不会出现"量出来的高度 ≠ 画出来的高度"。
fn kv(hdc: HDC, lay: &KvLayout, y: i32, k: &str, v: &str, val_color: Option<Color>) -> i32 {
    let kh = scale(ROW_H);
    let kr = RECT {
        left: lay.x_key,
        top: y,
        right: lay.x_key + scale(96),
        bottom: y + kh,
    };
    text(hdc, &kr, k, lay.font, C_ON_GRAD_DIM, false);
    let vcol = val_color.unwrap_or(C_ON_GRAD);
    let lines = wrap_value(hdc, lay.font, v, lay.max_w());
    for (i, ln) in lines.iter().enumerate() {
        let vr = RECT {
            left: kr.right,
            top: y + i as i32 * kh,
            right: lay.x_val_end,
            bottom: y + (i as i32 + 1) * kh,
        };
        text(hdc, &vr, ln, lay.font, vcol, false);
    }
    y + kh * lines.len().max(1) as i32
}

/// 值列可用宽度（`kv` 绘制与量高趟必须用同一个数，否则行数对不上）。
impl KvLayout {
    fn max_w(&self) -> i32 {
        self.x_val_end - (self.x_key + scale(96))
    }
}

/// 值列宽度口径（回归测试与量高趟共用同一份几何）：
/// 返回 `(全宽, 带健康徽标收窄宽)`。真源是 [`KvLayout::max_w`] 与
/// `DISK_BADGE_RESERVE`——测试从这里取数，不在测试里重抄一份几何。
#[cfg(test)]
pub(crate) fn hw_value_widths(w: i32) -> (i32, i32) {
    let (left, right) = hw_card_x(w);
    let pad = scale(PAD_IN);
    let lay = KvLayout {
        x_key: left + pad,
        x_val_end: right - pad,
        font: HFONT::default(),
    };
    (lay.max_w(), lay.max_w() - scale(DISK_BADGE_RESERVE))
}

/// 值文本自动折行：按 ` · ` 分隔符贪心装箱到 `max_w` 宽内，返回行文本数组。
///
/// 单行放得下就是 `[v]` 原样；断行处丢弃分隔符（下一行开头不带 ` · `）；
/// 单段自身超宽（如 60+ 字符的设备名）按字符硬断，保证不超界。
pub fn wrap_value(hdc: HDC, font: HFONT, v: &str, max_w: i32) -> Vec<String> {
    if max_w <= 0 {
        return vec![v.to_string()];
    }
    // SAFETY: 字体选中/恢复在同一函数内成对完成；测量不产生任何绘制。
    let widths = unsafe {
        let old = SelectObject(hdc, font.into());
        // 先把值切成"原子段"：` · ` 分隔 + 超长段逐字符硬断，每个原子保证 ≤ max_w。
        let mut atoms: Vec<String> = Vec::new();
        for seg in v.split(" · ") {
            let w = text_width(hdc, seg);
            if w > max_w {
                atoms.extend(split_overlong(
                    seg,
                    |c| text_width(hdc, &c.to_string()),
                    max_w,
                ));
            } else {
                atoms.push(seg.to_string());
            }
        }
        let atom_ws: Vec<i32> = atoms.iter().map(|a| text_width(hdc, a)).collect();
        let sep_w = text_width(hdc, " · ");
        let _ = SelectObject(hdc, old);
        (atoms, atom_ws, sep_w)
    };
    let (atoms, atom_ws, sep_w) = widths;
    pack_atoms(&atom_ws, sep_w, max_w)
        .into_iter()
        .map(|(a, b)| atoms[a..b].join(" · "))
        .collect()
}

/// 测量单行文本宽度（px）。调用方负责字体已选中。
unsafe fn text_width(hdc: HDC, s: &str) -> i32 {
    let wide: Vec<u16> = s.encode_utf16().collect();
    let mut size = SIZE::default();
    // windows 0.62：`GetTextExtentPoint32W` 直接收 `&[u16]`，返回 `BOOL`。
    if GetTextExtentPoint32W(hdc, &wide, &mut size).as_bool() {
        size.cx
    } else {
        0
    }
}

/// 贪心装箱（纯逻辑，单测钉住）：把保证 ≤`max_w` 的原子段装行，
/// 返回每行的原子下标区间 `[start, end)`。行间加分隔宽 `sep_w`。
///
/// 超宽原子（理论上不会出现，[`split_overlong`] 已保证 ≤max_w）独占一行。
fn pack_atoms(atom_ws: &[i32], sep_w: i32, max_w: i32) -> Vec<(usize, usize)> {
    let mut rows: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    let mut cur_w = 0i32;
    for (i, &w) in atom_ws.iter().enumerate() {
        let add = if i == start { w } else { sep_w + w };
        if i > start && cur_w + add > max_w {
            rows.push((start, i));
            start = i;
            cur_w = w;
        } else {
            cur_w += add;
        }
    }
    if atom_ws.len() > start {
        rows.push((start, atom_ws.len()));
    }
    rows
}

/// 超长段按字符硬断（纯逻辑，单测钉住）：`w_of` 给单字符宽，
/// 累计到 `max_w` 就断行，保证每行不超界。
fn split_overlong(s: &str, w_of: impl Fn(char) -> i32, max_w: i32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0i32;
    for ch in s.chars() {
        let cw = w_of(ch);
        if cur_w + cw > max_w && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
            cur_w = 0;
        }
        cur.push(ch);
        cur_w += cw;
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// 键值行的几何参数（把 `kv` 的参数从 8 个压到 4 个）。
struct KvLayout {
    /// 标签列左边界（= 卡片内边距）。
    x_key: i32,
    /// 值列右边界（= 卡片右内边距）。
    x_val_end: i32,
    /// 正文字体。
    font: HFONT,
}

// ============================================================
// 右键增强页（真开关）
// ============================================================

/// 一行增强功能：左侧名称+说明，右侧开关 + 状态文字，返回下一段 y。
///
/// `slot` 是命中测试/开关表的下标（绘制顺序，不再绑定 FEATURES 下标）；
/// 图标 `icon`/`icon_color` 与状态文字 `status`/`status_color` 由调用方显式传入，
/// 这样"经典菜单置顶"时也能保持图标与命中索引各自独立、不错位。
#[allow(clippy::too_many_arguments)]
fn feature_row(
    hdc: HDC,
    left: i32,
    right: i32,
    y: i32,
    slot: usize,
    icon: icons::FeatureIcon,
    icon_color: Color,
    name: &str,
    desc: &str,
    on: bool,
    hovered: bool,
    status: &str,
    status_color: Color,
    f_name: HFONT,
    f_sub: HFONT,
    scroll: i32,
    win_h: i32,
    disabled: bool,
) -> i32 {
    // 卡高 78：名称行(~18px) + 说明两行(13pt 字体 ~2×17px) + 上下边距。
    // 之前 66 装不下两行说明，`text_multiline` 的矩形裁剪把第二行拦腰截断
    // （Z.W. 真机截图“每个卡片底部文字被截断”，第十七轮）。
    let row_h = scale(78);
    let card = RECT {
        left,
        top: y,
        right,
        bottom: y + row_h,
    };
    // 暗玻璃卡：渐变上叠暗色（C++ 版右键页是深色毛玻璃，白字）。
    fill_glass_round(
        hdc,
        &card,
        scale(CARD_RADIUS),
        C_GLASS_DARK,
        0.45,
        scroll,
        win_h,
    );
    // 描边即状态：已启用=亮绿（一眼扫出哪几个是开的），未启用=叠白细边；
    // 禁用态=同样用叠白细边（不画绿，明确表示"此开关当前不可用"）。
    let edge = if disabled {
        over(grad_at(y - scroll, win_h), C_WHITE, 0.28)
    } else if on {
        C_CARD_EDGE_ON
    } else {
        over(grad_at(y - scroll, win_h), C_WHITE, 0.28)
    };
    stroke_round(hdc, &card, scale(CARD_RADIUS), edge, scale(1));

    // 左侧功能图标（柔彩，与硬件页图标同一套粉彩强调色）。
    let icon_sz = scale(22);
    let itile = RECT {
        left: left + scale(PAD_IN),
        top: y + (row_h - icon_sz) / 2,
        right: left + scale(PAD_IN) + icon_sz,
        bottom: y + (row_h - icon_sz) / 2 + icon_sz,
    };
    icons::draw_feature_icon(hdc, &itile, icon, icon_color);

    // 名称 + 说明（两行），统一右移给图标让位。
    let text_left = left + scale(PAD_IN) + icon_sz + scale(8);
    let nr = RECT {
        left: text_left,
        top: y + scale(12),
        right: right - scale(140),
        bottom: y + scale(32),
    };
    text(hdc, &nr, name, f_name, C_ON_GRAD, false);
    let dr = RECT {
        left: text_left,
        top: y + scale(33),
        right: right - scale(140),
        // 两行 13pt 文字 ≈ 34px：33+34=67 ≤ 68（= row_h−10，留出下边距）
        bottom: y + scale(68),
    };
    text_multiline(hdc, &dr, desc, f_sub, C_ON_GRAD_DIM);

    // 右侧：状态文字 + 开关
    let sw_w = scale(44);
    let sw_h = scale(24);
    let sw = RECT {
        left: right - scale(PAD_IN) - sw_w,
        top: y + (row_h - sw_h) / 2,
        right: right - scale(PAD_IN),
        bottom: y + (row_h - sw_h) / 2 + sw_h,
    };
    // 禁用态：开关画成"关"且无悬停高亮，明确表示不可点；同时**不写入**
    // SWITCH_RECS，使命中测试天然点不到（避免无效点击还弹错误框）。
    if disabled {
        draw_switch(hdc, &sw, false, false, scroll, win_h);
    } else {
        draw_switch(hdc, &sw, on, hovered, scroll, win_h);
    }
    let st = RECT {
        left: sw.left - scale(64),
        top: sw.top,
        right: sw.left - scale(8),
        bottom: sw.bottom,
    };
    text(hdc, &st, status, f_sub, status_color, true);

    // 禁用态不注册命中矩形——整行纯展示，点击无任何副作用。
    if !disabled {
        SWITCH_RECS.with(|v| {
            let mut r = v.borrow_mut();
            if r.len() <= slot {
                r.resize(slot + 1, RECT::default());
            }
            r[slot] = sw;
        });
    }
    y + row_h + scale(10)
}

/// 开关本体：圆角轨道 + 白色圆形滑块（开=靛蓝轨道，关=叠白玻璃轨道），
/// 滑块下方带 1px 软阴影（伪厚度），整体用 `aa_round` 抗锯齿。
///
/// 第十四轮重设计（Z.W. 反馈"开关不舒服"）：
/// - 去掉上一版"淡灰描边环 + 白旋钮"的双环——双环在深色卡片上显脏、边界乱；
/// - 改成**单色白旋钮 + 软阴影**，像一颗实体拨钮，观感更安静、更舒适；
/// - 关态轨道用叠白玻璃（`over(grad, C_WHITE, 0.30)`），清晰可读的"关"态，
///   不再用半透明白导致发灰不清。
fn draw_switch(hdc: HDC, r: &RECT, on: bool, hovered: bool, scroll: i32, win_h: i32) {
    let grad = grad_at(r.top - scroll, win_h);
    let card = over(grad, C_GLASS_DARK, 0.45);
    // 轨道：开=品牌靛蓝（hover 提亮），关=叠白玻璃（清晰的"关"态）。
    let track = if on {
        if hovered {
            C_BTN_H
        } else {
            C_BTN
        }
    } else {
        over(grad, C_WHITE, 0.30)
    };
    // 轨道圆角（AA）。
    aa_round(hdc, r, (r.bottom - r.top) / 2, track, card);

    // 滑块：白色圆 + 下方 1px 软阴影。
    let d = r.bottom - r.top - scale(6);
    let cx = if on {
        r.right - d / 2 - scale(3)
    } else {
        r.left + d / 2 + scale(3)
    };
    let cy = (r.top + r.bottom) / 2;
    // 软阴影：比滑块大 1px、整体下移 1~2px 的暗圆（bg=轨道色 → 圆角外无缝融进轨道）。
    let sh = RECT {
        left: cx - d / 2 - scale(1),
        top: cy - d / 2,
        right: cx + d / 2 + scale(1),
        bottom: cy + d / 2 + scale(2),
    };
    aa_round(
        hdc,
        &sh,
        (sh.bottom - sh.top) / 2,
        over(grad, rgb(0, 0, 0), 0.30),
        track,
    );
    // 白色滑块（bg=轨道色 → 圆角外无缝融进轨道）。
    let knob = RECT {
        left: cx - d / 2,
        top: cy - d / 2,
        right: cx + d / 2,
        bottom: cy + d / 2,
    };
    aa_round(hdc, &knob, d / 2, C_ON_GRAD, track);
}

thread_local! {
    /// 各功能开关的矩形（绘制时写、点击命中时读）。
    static SWITCH_RECS: std::cell::RefCell<Vec<RECT>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// 点在哪个功能开关上（不在返回 None）。
pub fn switch_at(x: i32, y: i32) -> Option<usize> {
    SWITCH_RECS.with(|v| {
        v.borrow()
            .iter()
            .position(|r| r.right != 0 && x >= r.left && x < r.right && y >= r.top && y < r.bottom)
    })
}

/// 经典右键菜单 CLSID（Win11 新上下文菜单里"显示更多选项"的开关）。
///
/// **值必须与 C++ 版一致**（`main.cpp:604`）。我曾凭记忆写错过一次
/// （`{86ca1aa0-7422-4432-...}`），症状是状态恒为"未启用"，改回来才对。
pub const CLASSIC_MENU_CLSID: &str = "{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}";

/// 经典菜单开关是否已启用（读 HKCU 下的 InprocServer32 是否存在）。
pub fn classic_menu_on() -> bool {
    use winsuitex_core::traits::Registry;
    let key = format!(r"Software\Classes\CLSID\{CLASSIC_MENU_CLSID}\InprocServer32");
    winsuitex_platform::WinRegistry.exists(winsuitex_core::model::RegRoot::CurrentUser, &key)
}

/// 是否 Windows 11（注册表 `CurrentBuildNumber` >= 22000）。
///
/// 经典右键菜单是 Win11 专属——Win10 系统已自带完整右键菜单，此开关无意义。
/// 用 build 号而非 ProductName 判定（Win11 的 ProductName 常被误报为 "Windows 10"，
/// 见 `winsuitex_platform::system` 的 `normalize_os_product`）。
pub fn is_win11() -> bool {
    use winsuitex_core::model::RegRoot;
    use winsuitex_core::traits::Registry;
    let reg = winsuitex_platform::WinRegistry;
    let build = reg
        .read_string(
            RegRoot::LocalMachine,
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
            Some("CurrentBuildNumber"),
        )
        .unwrap_or(None);
    build
        .as_deref()
        .and_then(|b| b.trim().parse::<u32>().ok())
        .is_some_and(|b| b >= 22000)
}

/// 是否安装了 StartAllBack（第三方开始菜单 / 任务栏美化工具）。
///
/// StartAllBack 会注入 explorer 并**接管 Win11 经典右键菜单**：它自己实现的
/// shell 扩展覆盖了我们 `{86ca1aa0}` 这套 COM 注册——所以本程序的"显示经典
/// 菜单"开关在装了 StartAllBack 的机器上**永远不生效**，点下去只会白白重启
/// 一次 explorer（还可能顺手弹出一个主文件夹窗口，见第十六轮修复）。
///
/// 检测信号：
/// - `HKLM\SOFTWARE\Classes\CLSID\{117E3954-5034-453A-A18B-7B79493646E6}`
///   存在（StartAllBack 的 shell 扩展，InProcServer32 = `StartAllBackLoaderX64.dll`）。
///   这是"已安装"的最强信号——只要装了就注册，与当前是否在跑无关。
pub fn startallback_present() -> bool {
    use winsuitex_core::traits::Registry;
    let reg = winsuitex_platform::WinRegistry;
    let key = r"SOFTWARE\Classes\CLSID\{117E3954-5034-453A-A18B-7B79493646E6}";
    reg.exists(winsuitex_core::model::RegRoot::LocalMachine, key)
}

/// 打开/关闭经典右键菜单（写空值 / 删整棵 CLSID 树）。
///
/// 写的是**空值**而不是数据——Windows 11 只看这个键是否存在，不看值。
///
/// 注意：**不在这里通知外壳**。`{86ca1aa0}` 是 shell 扩展注册覆盖，
/// `SHChangeNotify(SHCNE_ASSOCCHANGED)` 对它无效——唯一生效途径是重启
/// explorer，由调用方（`toggle_feature`）在写完注册表后调 `restart_explorer`。
pub fn set_classic_menu(on: bool) -> winsuitex_core::error::Result<()> {
    use winsuitex_core::model::RegValueKind;
    use winsuitex_core::traits::Registry;
    let mut reg = winsuitex_platform::WinRegistry;
    let root = winsuitex_core::model::RegRoot::CurrentUser;
    if on {
        let key = format!(r"Software\Classes\CLSID\{CLASSIC_MENU_CLSID}\InprocServer32");
        // 写空字符串 + Sz 类型：Windows 只看这个键是否存在，不看值。
        reg.write_string(root, &key, None, "", RegValueKind::Sz)?;
    } else {
        let tree = format!(r"Software\Classes\CLSID\{CLASSIC_MENU_CLSID}");
        reg.delete_tree(root, &tree)?;
    }
    Ok(())
}

/// 右键增强页：经典右键菜单（置顶）+ 5 个功能真开关（其下）。
pub fn draw_menu_page(ctx: &PageCtx<'_>) -> i32 {
    let PageCtx {
        hdc,
        w,
        h,
        f_sec,
        f_body,
        f_sub,
        ..
    } = *ctx;
    use winsuitex_core::feature::{is_installed, FEATURES};
    let left = content_left() + scale(MARGIN);
    let right = content_right(w) - scale(MARGIN);
    let scroll = ctx.scroll;
    let mut y = content_top() + scale(14);

    // 清空开关缓存，避免上一帧的旧矩形被误命中
    SWITCH_RECS.with(|v| v.borrow_mut().clear());

    let head = RECT {
        left,
        top: y,
        right,
        bottom: y + scale(28),
    };
    text(hdc, &head, "右键菜单增强工具", f_sec, C_ON_GRAD, false);
    y += scale(32);
    // StartAllBack 一旦安装就接管经典右键菜单，本开关无效（且点了会白重启
    // explorer、可能弹文件夹）——检测到就把它渲染成禁用态、并让扩展项照常显示。
    // 提示行也随状态换文案：接管态不再提"重启资源管理器"（根本不会发生）。
    let startallback = startallback_present();
    let hint_text = if startallback {
        "扩展项开关即时生效；经典菜单已由 StartAllBack 接管，无需重启资源管理器"
    } else {
        "开关即时生效；切换经典右键菜单会自动重启资源管理器（约 1~2 秒）"
    };
    let hint = RECT {
        left,
        top: y,
        right,
        bottom: y + scale(20),
    };
    text(hdc, &hint, hint_text, f_sub, C_ON_GRAD_DIM, false);
    y += scale(32);

    // 当前系统是否 Windows 11（经典菜单仅 Win11 需要；判断系统是否显示该菜单）。
    let win11 = is_win11();

    // —— 经典右键菜单置顶（仅 Windows 11；非 Win11 整块不显示，见下方 if win11）——
    if win11 {
        y += scale(4);
        let classic_head = RECT {
            left: left + scale(PAD_IN),
            top: y,
            right,
            bottom: y + scale(24),
        };
        text(
            hdc,
            &classic_head,
            "Windows 11 经典右键菜单",
            f_sec,
            C_ON_GRAD,
            false,
        );
        y += scale(30);
        let classic_on = classic_menu_on();
        // 装了 StartAllBack：开关画成"已被接管"的禁用态，不响应点击（见 feature_row
        // 的 disabled 分支）。说明文字改为解释为何本开关无意义，避免"点了没反应"的
        // 困惑；没装时才是正常的启用/未启用开关。
        let (desc_text, status_text, status_color) = if startallback {
            (
                "StartAllBack 已接管经典菜单，本开关无需使用、不会生效",
                "已由接管",
                C_ON_GRAD_FAINT,
            )
        } else {
            (
                "在“此电脑”上右键直接看到旧版完整菜单，不再需要点“显示更多选项”",
                if classic_on { "已启用" } else { "未启用" },
                if classic_on {
                    C_CARD_EDGE_ON
                } else {
                    C_ON_GRAD_FAINT
                },
            )
        };
        y = feature_row(
            hdc,
            left,
            right,
            y,
            0,
            icons::FeatureIcon::Menu,
            C_ACCENT_SOFT[0],
            "显示经典菜单",
            desc_text,
            classic_on,
            hover_switch() == 0,
            status_text,
            status_color,
            f_body,
            f_sub,
            scroll,
            h,
            startallback,
        );
    }

    // —— 功能增强开关（slot 1..=5，绘制顺序在经典菜单之下）——
    // 仅 Windows 11 **且经典右键菜单关闭**时，扩展右键项（显示/隐藏系统文件、
    // 命令提示符在此处打开、PowerShell 在此处打开）注册在旧版"显示更多选项"
    // 菜单下、新版菜单看不到 → 开着也毫无意义，直接隐藏；只保留两种菜单都
    // 可见的计算机管理 / 设备管理器（slot 1、2）。Windows 10 及更早没有新版
    // 菜单，这 3 项本就在原生右键菜单里生效，照常显示。启用经典菜单后 3 项自动出现。
    // 装了 StartAllBack 时它已提供完整经典菜单，这 3 项同样可见 → 不隐藏。
    y += scale(10);
    let feat_head = RECT {
        left: left + scale(PAD_IN),
        top: y,
        right,
        bottom: y + scale(24),
    };
    text(hdc, &feat_head, "功能增强", f_sec, C_ON_GRAD, false);
    y += scale(30);
    let classic_on = classic_menu_on();
    let hide_extras = win11 && !classic_on && !startallback;
    if hide_extras {
        // 解释为什么只显示两项，避免"是不是漏了"的疑惑（纯提示，不改功能）。
        let tip = RECT {
            left,
            top: y,
            right,
            bottom: y + scale(18),
        };
        text(
            hdc,
            &tip,
            "启用上方“经典右键菜单”后，其余扩展项才会出现并生效",
            f_sub,
            C_ON_GRAD_FAINT,
            false,
        );
        y += scale(22);
    } else if startallback {
        // 装了 StartAllBack：说明为何扩展项始终可见（它已接管经典菜单）。
        let tip = RECT {
            left,
            top: y,
            right,
            bottom: y + scale(18),
        };
        text(
            hdc,
            &tip,
            "检测到 StartAllBack：经典菜单由它接管，下方扩展项始终可用",
            f_sub,
            C_ON_GRAD_FAINT,
            false,
        );
        y += scale(22);
    }
    let reg = winsuitex_platform::WinRegistry;
    let hovered = hover_switch();
    for (i, f) in FEATURES.iter().enumerate() {
        // 经典菜单关闭的 Win11 下，扩展右键项没意义，跳过绘制（SWITCH_RECS
        // 也不写对应 slot → 命中测试天然点不到，开关状态保持原样不受干扰）。
        if hide_extras && i >= 2 {
            continue;
        }
        let on = is_installed(&reg, f);
        let (fi, fcol) = feature_icon_for(i);
        let status = if on { "已启用" } else { "未启用" };
        let status_color = if on { C_CARD_EDGE_ON } else { C_ON_GRAD_FAINT };
        y = feature_row(
            hdc,
            left,
            right,
            y,
            i + 1,
            fi,
            fcol,
            f.display_name,
            f.description,
            on,
            hovered == (i + 1) as i32,
            status,
            status_color,
            f_body,
            f_sub,
            scroll,
            h,
            false,
        );
    }
    y
}

/// 玻璃卡（`tint` 在渐变上叠 `alpha` 的混合色）+ 一圈叠白细描边。
///
/// 磁盘页表格容器用 `C_GLASS_TINT` 0.22（与硬件页大卡同档，压暗让白字清楚），
/// 详情卡用 0.30（更暗一档，表示"这是选中项的展开"）。
fn card_bg(hdc: HDC, r: &RECT, tint: Color, alpha: f32, scroll: i32, win_h: i32) {
    fill_glass_round(hdc, r, scale(CARD_RADIUS), tint, alpha, scroll, win_h);
    stroke_round(
        hdc,
        r,
        scale(CARD_RADIUS),
        over(grad_at(r.top - scroll, win_h), C_WHITE, 0.32),
        scale(1),
    );
}

// ============================================================
// 磁盘信息页
// ============================================================

/// 磁盘信息：分区表（选中行详情）。
pub fn draw_disk_page(ctx: &PageCtx<'_>) -> i32 {
    let PageCtx {
        hdc,
        w,
        h,
        snap,
        f_sub,
        f_tiny,
        ..
    } = *ctx;
    let left = content_left() + scale(MARGIN);
    let right = content_right(w) - scale(MARGIN);
    let Some(r) = snap else {
        return 0;
    };
    let scroll = ctx.scroll;
    // 头部说明文字是固定层（见 `on_paint` 调 `draw_disk_header`），不随表格滚动。
    let mut y = content_top() + scale(50);

    // 表头 + 行
    let cols = [
        "盘符",
        "磁盘",
        "文件系统",
        "簇大小",
        "类型",
        "总容量",
        "可用",
        "已用%",
    ];
    // 列宽按可用宽度**按权重分配**，不能写死 78px：
    // 多列时 ×78 会超出窄窗口，末列被右边框裁掉。
    // 权重按内容长度给：盘符/磁盘/簇大小/已用% 是短值，类型/容量是长值。
    // 盘符列权重从 6 提到 8：左端要给"分区类型图标(16) + 间距"让位（第九轮新增）。
    let table_w = (right - left - scale(20)).max(scale(200));
    let weights = [8i32, 7, 10, 8, 12, 13, 12, 6];
    let total_w: i32 = weights.iter().sum();
    let rowh = scale(34);
    // 列边界数组：**长度必须是列数 + 1**（9 而不是 8）——
    // 每列要取 `col_x[i]` 与 `col_x[i + 1]`，末列 i=7 时会访问 `col_x[8]`。
    const NCOL: usize = 8;
    let mut col_x = [0i32; NCOL + 1];
    {
        let mut acc = 0;
        for (i, wgt) in weights.iter().enumerate() {
            col_x[i] = left + scale(10) + table_w * acc / total_w;
            acc += wgt;
        }
        // 末列右边界贴齐表格右边（不被padding 挤出去）。
        col_x[NCOL] = right - scale(10);
    }
    // 表格玻璃卡：高度先算好（表头 + N 行），再一次性画底——
    // 与硬件页同理，底必须先于内容，所以行数要先知道。
    let table_bottom = y + rowh * (r.volumes.len() as i32 + 1) + scale(4);
    card_bg(
        hdc,
        &RECT {
            left,
            top: y,
            right,
            bottom: table_bottom,
        },
        C_GLASS_TINT,
        0.22,
        scroll,
        h,
    );
    for (i, c) in cols.iter().enumerate() {
        // ⚠️ 表头对齐规则（第二十七轮 Z.W. 定案）：
        // - 首列「盘符」**与分区图标左缘齐平**（col_x[0]+2，同内容行图标的 x）——
        //   表头左边不留空档。二十五轮曾把表头对齐到盘符文字(+20)，真机反馈
        //   "表头左边空了一截"（图标列上方没字），资源管理器同款做法是表头
        //   顶到图标列头，故回移。
        // - 数值列（总容量/可用，index≥5）右对齐，与内容行的 `numeric` 一致。
        let c_left = if i == 0 {
            col_x[0] + scale(2)
        } else {
            col_x[i]
        };
        let cr = RECT {
            left: c_left,
            top: y,
            right: col_x[i + 1],
            bottom: y + rowh,
        };
        let numeric = i >= 5;
        text(hdc, &cr, c, f_tiny, C_ON_GRAD_DIM, numeric);
    }
    y += rowh + scale(4);

    for (i, v) in r.volumes.iter().enumerate() {
        let row = RECT {
            left,
            top: y,
            right,
            bottom: y + rowh,
        };
        let selected = i as i32 == sel_vol();
        // 选中行：薄荷绿实底 + 深字（C++ 版实测 `(216,240,228)`）。
        // 深字是必须的——绿底上再放白字对比度不够。
        if selected {
            fill_round(hdc, &row, 6, C_SEL_ROW);
        } else if i as i32 == crate::hover_vol_row() {
            // 非选中行的 hover 浅玻璃底：鼠标悬停时给一点反馈，便于瞄准点击。
            // 用 `C_WHITE` 低透明度（0.08）叠在渐变上，比实色更轻、不抢选中行的绿底。
            fill_glass_round(hdc, &row, 6, C_WHITE, 0.08, scroll, h);
        }

        // 分区类型图标（盘符列左端）：本地盘 / 可移动 / 网络 / 光盘四类简笔。
        // 选中行底变浅绿，图标改深字色（`C_SEL_ROW_TEXT`）保证对比；否则用柔彩。
        let (vi, vcol) = volume_icon(v);
        let icon_color = if selected { C_SEL_ROW_TEXT } else { vcol };
        let ir = RECT {
            left: col_x[0] + scale(2),
            top: y + (rowh - scale(16)) / 2,
            right: col_x[0] + scale(2) + scale(16),
            bottom: y + (rowh - scale(16)) / 2 + scale(16),
        };
        icons::draw_volume_icon(hdc, &ir, vi, icon_color);

        // 已用率：=(1 - 可用/总量)×100，总量为 0（未读到）按 0 处理并显示 "-"。
        // 与 HTML 报告分区使用率口径一致；满盘（≥90%）红、偏满（≥75%）橙。
        // ⚠️ `total_bytes`/`free_bytes` 是 `Option<u64>`，必须先 `unwrap_or(0)` 再运算。
        let total = v.total_bytes.unwrap_or(0);
        let free = v.free_bytes.unwrap_or(0).min(total);
        let used_pct: i32 = if total > 0 {
            ((1.0 - free as f64 / total as f64) * 100.0).round() as i32
        } else {
            0
        };
        let used_cell = if total > 0 {
            format!("{used_pct}%")
        } else {
            "-".to_string()
        };
        let used_col = if total > 0 {
            used_color(used_pct)
        } else {
            C_ON_GRAD_DIM
        };
        let cells = [
            v.letter.clone(),
            // 网络盘 / 未关联到物理盘的分区**没有 PD 号**，不能填 `PD0`
            // （真机截图上 Z: 网络盘显示成 PD0，是错的，C++ 版显示 "-"）。
            v.disk_index
                .map_or_else(|| "-".to_string(), |d| format!("PD{d}")),
            v.fs_name.clone().unwrap_or_else(|| "-".into()),
            // 簇大小 0 表示没读到（网络盘常见），显示 "-" 而不是 "0 KB"。
            fmt_cluster(v.cluster_bytes),
            v.type_name.clone().unwrap_or_else(|| "-".into()),
            fmt_gb(v.total_bytes),
            fmt_gb(v.free_bytes),
            used_cell,
        ];
        for (j, cell) in cells.iter().enumerate() {
            // 盘符列左端被图标占了，文字右移 20px（图标 16 + 间距 4）；其余列不动。
            let cell_left = if j == 0 {
                col_x[0] + scale(20)
            } else {
                col_x[j]
            };
            let cr = RECT {
                left: cell_left,
                top: y,
                right: col_x[j + 1],
                bottom: y + rowh,
            };
            // 容量三列（总容量/可用/已用%）右对齐，扫数字比扫字段名快。
            // 已用% 非选中行按阈值上色（满盘红 / 偏满橙），一眼看出哪块盘快满了。
            let numeric = j >= 5;
            let fg = if selected {
                C_SEL_ROW_TEXT
            } else if j == 7 {
                used_col
            } else {
                C_ON_GRAD
            };
            text(hdc, &cr, cell, f_sub, fg, numeric);
        }
        // 记录每行矩形，供点击命中测试用（命中测试不重算布局，避免两处算法漂移）
        VOL_ROWS.with(|rows| {
            let mut v = rows.borrow_mut();
            if v.len() != r.volumes.len() {
                v.clear();
            }
            if v.is_empty() {
                for _ in 0..r.volumes.len() {
                    v.push(RECT::default());
                }
            }
            v[i] = row;
        });
        y += rowh;
    }

    // 选中分区详情卡（跟随点击选中的行，越界时回落到第一行）
    let sel_idx = (sel_vol().max(0) as usize).min(r.volumes.len().saturating_sub(1));
    if let Some(v) = r.volumes.get(sel_idx) {
        // 通过 `disk_index` 关联物理盘，取 SMART 健康（渲染"硬件预警状态"）。
        // 拿不到（无盘索引 / 盘无 SMART / 受限）就如实提示，不假装健康。
        // 选中分区详情卡 SMART 行：每行独立上色——
        // 健康状态按语义（绿/橙/红/灰），温度按阈值（≥85°红 / ≥75°橙），
        // 与硬件页显卡段、HTML 报告 `telemetry_line` 口径一致。
        // ⚠️ 读不到 SMART 时显示"未读取"，用 `C_ON_GRAD_DIM`（深底弱对比、实测 5.5:1），
        // 绝不能用 `C_GRAY_DARK`(89,89,89)——那是浅底深灰，深 navy 详情卡上 ≈1:1 隐形。
        let smart_lines: Vec<(String, Color)> = match v
            .disk_index
            .and_then(|di| r.info.disks.iter().find(|d| d.index == di))
            .and_then(|d| d.smart.as_ref())
        {
            Some(s) => {
                let mut v: Vec<(String, Color)> = vec![(
                    format!("SMART 健康：{}", smart_label(&s.health_status)),
                    smart_color(&s.health_status),
                )];
                if let Some(t) = s.temperature_c {
                    v.push((format!("温度 {}℃", t), temp_color(t as i32)));
                }
                if let Some(p) = s.health_percent {
                    v.push((format!("剩余寿命 {}%", p), C_ON_GRAD));
                }
                v
            }
            None => vec![(
                "SMART 健康：未读取（可能需以管理员身份运行）".to_string(),
                crate::theme::C_ON_GRAD_DIM,
            )],
        };

        // 「已用率」独立彩色行：与磁盘表格「已用%」列口径一致，
        // 满盘（≥90%）红 / 偏满（≥75%）橙，让详情卡也能一眼看出哪块盘快满了。
        let used_pct: Option<i32> = if v.total_bytes.unwrap_or(0) > 0 {
            let free = v.free_bytes.unwrap_or(0).min(v.total_bytes.unwrap_or(0));
            let pct =
                ((1.0 - free as f64 / v.total_bytes.unwrap_or(0) as f64) * 100.0).round() as i32;
            Some(pct)
        } else {
            None
        };
        let used_line: (String, Color) = match used_pct {
            Some(p) => (format!("已用率 {p}%"), used_color(p)),
            None => ("已用率 -".to_string(), C_ON_GRAD_DIM),
        };

        y += scale(12);
        // 每行可带自己的颜色：SMART 状态行用绿/橙/红/灰，其余用白字。
        // 用 `Vec<(String, Color)>` 持有所有权——不能 `&format!`（`vec!` 内临时量
        // 在表达式结束即 drop，借它等于悬垂引用）。
        let mut lines: Vec<(String, Color)> = vec![
            (
                // ⚠️ `v.letter` 自带冒号（表格里就显示 "A:"），格式串不能再加 ": "，
                // 否则真机详情卡渲染成 "A:: 本地磁盘" 双冒号（第二十七轮修）。
                format!(
                    "{}　{}　{}",
                    v.letter.trim_end_matches(':'),
                    v.type_name.clone().unwrap_or_default(),
                    v.fs_name.clone().unwrap_or_default()
                ),
                C_ON_GRAD,
            ),
            (
                format!(
                    "簇大小 {} 字节 = 每簇 {} 扇区 × {} 字节/扇区",
                    v.cluster_bytes.unwrap_or(0),
                    v.sectors_per_cluster.unwrap_or(0),
                    v.bytes_per_sector.unwrap_or(0)
                ),
                C_ON_GRAD,
            ),
            (
                format!(
                    "总容量 {}　可用 {}",
                    fmt_gb(v.total_bytes),
                    fmt_gb(v.free_bytes)
                ),
                C_ON_GRAD,
            ),
            used_line,
            (
                format!(
                    "NT 设备名 {}　卷标 {}",
                    v.disk_index
                        .map_or_else(|| "-".to_string(), |d| format!("\\\\.\\PhysicalDrive{d}")),
                    v.vol_label.clone().unwrap_or_else(|| "无".into())
                ),
                C_ON_GRAD,
            ),
        ];
        // SMART 健康 / 温度 / 剩余寿命：独立彩色行，跟随选中盘渲染。
        lines.extend(smart_lines);
        // 详情卡比表格卡更暗一档（叠深 navy 30%），表示"这是选中项的展开"。
        // 高度按行数动态算：SMART 健康/温度/剩余寿命各自独立成行（最多 +2 行），
        // 固定 scale(116) 在温度+寿命都读到时会把末行挤出卡片，故改为动态高度。
        let card_bottom = y + scale(8) + lines.len() as i32 * scale(20) + scale(8);
        let dr = RECT {
            left,
            top: y,
            right,
            bottom: card_bottom,
        };
        card_bg(hdc, &dr, C_GLASS_TINT, 0.30, scroll, h);
        let mut ly = y + scale(8);
        for (l, col) in &lines {
            let lr = RECT {
                left: left + scale(PAD_IN),
                top: ly,
                right: right - scale(PAD_IN),
                bottom: ly + scale(20),
            };
            text(hdc, &lr, l, f_sub, *col, false);
            ly += scale(20);
        }
        // 返回**详情卡矩形真实底边**，而不是末行文字底（ly）：自适应窗口高度
        // 拿返回值当内容底边算底栏位置，差这 8px 就会让底栏顶进卡片（第十六轮）。
        dr.bottom
    } else {
        y
    }
}

/// SMART 健康状态中文标签（与硬件页预警语义一致）。
fn smart_label(h: &winsuitex_core::model::HealthStatus) -> &'static str {
    match h {
        winsuitex_core::model::HealthStatus::Unknown => "未知",
        winsuitex_core::model::HealthStatus::Healthy => "健康",
        winsuitex_core::model::HealthStatus::Warning => "预警",
        winsuitex_core::model::HealthStatus::Critical => "危险",
    }
}

/// SMART 健康状态对应主题色：健康绿 / 预警橙 / 危险红 / 未知弱化紫。
///
/// ⚠️ Unknown 不能用 `C_GRAY_DARK`(89,89,89)——那是**浅底**用的深灰，放在详情卡
/// 深 navy 底（亮度 ~78）上对比度 ≈1:1，真机实测整行"隐身"、ClearType 下只剩
/// 字形残影像被裁半截（第二十七轮）。改用 `C_ON_GRAD_DIM`：应用里"字段名/提示行"
/// 的统一弱化色，深底实测 5.5:1，与白正文仍拉开层级。
fn smart_color(h: &winsuitex_core::model::HealthStatus) -> Color {
    match h {
        winsuitex_core::model::HealthStatus::Healthy => C_SUCCESS,
        winsuitex_core::model::HealthStatus::Warning => C_WARNING,
        winsuitex_core::model::HealthStatus::Critical => C_ERROR,
        winsuitex_core::model::HealthStatus::Unknown => C_ON_GRAD_DIM,
    }
}

/// 温度阈值色：与 HTML 报告 `telemetry_line` 完全一致——≥85° 红（过热）、
/// ≥75° 橙（偏热）、其余白字。用于硬件页显卡段、磁盘详情卡的温度行，
/// 让"哪块卡/盘在发烧"一眼可辨，两端观感统一。
fn temp_color(t: i32) -> Color {
    if t >= 85 {
        C_ERROR
    } else if t >= 75 {
        C_WARNING
    } else {
        C_ON_GRAD
    }
}

/// 磁盘已用率阈值色：与 HTML 报告分区使用率口径一致——≥90% 红（将满）、
/// ≥75% 橙（偏满）、其余白字。用于磁盘页表格「已用%」列，满盘一眼可辨。
fn used_color(pct: i32) -> Color {
    if pct >= 90 {
        C_ERROR
    } else if pct >= 75 {
        C_WARNING
    } else {
        C_ON_GRAD
    }
}

/// 硬件页磁盘行右侧的 SMART 健康预警小条：实色圆角 + 反色状态词。
///
/// 与磁盘页详情卡**同款色语义**（`smart_color`/`smart_label`），但形态是"徽标"而非
/// 行内文字，让"哪块盘要挂了"在硬件概览上一眼可辨。危险红底用白字，其余（绿/橙/淡紫）
/// 用深字保证对比度（淡紫底白字会糊）。
fn draw_disk_health_badge(
    hdc: HDC,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    st: winsuitex_core::model::HealthStatus,
    font: HFONT,
) {
    let r = RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    };
    fill_round(hdc, &r, scale(6), smart_color(&st));
    let tcol = match st {
        winsuitex_core::model::HealthStatus::Critical => C_WHITE,
        _ => C_BTN_WHITE_TEXT,
    };
    text_center(hdc, &r, smart_label(&st), font, tcol);
}

/// 磁盘页固定头部：玻璃小卡 + 说明文字（分区数）。
///
/// 在 `on_paint` 的滚动变换**之外**绘制（屏幕坐标），分区很多超出 `WIN_H` 触发滚动时
/// 钉在内容区顶部、不被裁切——与硬件页同源（第十八轮）：原本这段说明文字画在滚动变换内，
/// 长列表滚动后会被 `IntersectClipRect` 截掉。玻璃带充当背景，滚动的表格卡滑到其下方被遮住。
pub fn draw_disk_header(hdc: HDC, w: i32, h: i32, f_body: HFONT, n: usize) {
    let left = content_left() + scale(MARGIN);
    let right = content_right(w) - scale(MARGIN);
    let top = content_top() + scale(10);
    let bottom = top + scale(34);
    // 打底到表格卡顶（content_top+50，与 draw_disk_page 的表格起点同一常量），
    // 盖掉从玻璃带渗出/从 6px 窄缝穿过的滚动表格。
    mask_header_gap(hdc, w, h, content_top() + scale(50));
    let band = RECT {
        left,
        top,
        right,
        bottom,
    };
    fill_glass_round(hdc, &band, scale(CARD_RADIUS), C_GLASS_TINT, 0.22, 0, h);

    let th = scale(26);
    let t = RECT {
        left: left + scale(PAD_IN),
        top: top + (bottom - top - th) / 2,
        right,
        bottom: top + (bottom - top + th) / 2,
    };
    text(
        hdc,
        &t,
        &format!("查看各分区的簇大小与容量信息　共 {} 个分区", n),
        f_body,
        C_ON_GRAD,
        false,
    );
}

/// 簇大小文本：0 表示没读到（网络盘 / 未挂载卷常见），显示 "-" 而不是 "0 KB"。
fn fmt_cluster(b: Option<u32>) -> String {
    match b {
        Some(v) if v >= 1024 => format!("{} KB", v / 1024),
        Some(v) if v > 0 => format!("{v} B"),
        _ => "-".into(),
    }
}

/// 分区类型图标 + 柔彩描边色（按 `type_name` 判定）。
///
/// `type_name` 来自 `drive_type_name`：本地磁盘 / 可移动磁盘 / 网络磁盘 / 光盘驱动器 /
/// RAM 磁盘 / 未知（见 `winsuitex-platform/src/disk.rs`）。网络/可移动/光盘给各自色相，
/// 本地盘统一用蓝，让"各分区加图标"既好看又不花眼。
fn volume_icon(v: &winsuitex_core::model::Volume) -> (icons::VolumeIcon, Color) {
    let icon = match v.type_name.as_deref() {
        Some("网络磁盘") => icons::VolumeIcon::Network,
        Some("可移动磁盘") => icons::VolumeIcon::Usb,
        Some("光盘驱动器") => icons::VolumeIcon::Disc,
        _ => icons::VolumeIcon::Drive, // 本地磁盘 / RAM / 未知
    };
    let color = match v.type_name.as_deref() {
        Some("网络磁盘") => C_ACCENT_SOFT[0],
        Some("可移动磁盘") => C_ACCENT_SOFT[5],
        Some("光盘驱动器") => C_ACCENT_SOFT[4],
        _ => C_ACCENT_SOFT[6], // 本地磁盘 蓝
    };
    (icon, color)
}

/// 字节 → GiB 文本（按 1024 进制，与 HTML 报告口径一致）。
fn fmt_gb(b: Option<u64>) -> String {
    match b {
        Some(v) if v > 0 => format!("{:.1} GiB", v as f64 / 1073741824.0),
        _ => "-".into(),
    }
}

// ============================================================
// 报告导出
// ============================================================

/// 生成 HTML 报告到 exe 同目录（写不进去回退桌面）并用默认程序打开。
pub fn export_report() {
    let Some(r) = snapshot() else {
        return;
    };
    let features = winsuitex_core::feature::FEATURES
        .iter()
        .map(|f| {
            (
                f.id,
                winsuitex_core::feature::is_installed(&winsuitex_platform::WinRegistry, f),
            )
        })
        .collect::<Vec<_>>();
    let html = winsuitex_report::build_html_with_features(&r.info, &r.volumes, &features);

    let ts = winsuitex_core::model::local_timestamp_string().replace([':', ' '], "-");
    // 存 exe 同目录（Z.W. 定案），写不进去（只读目录如 Program Files）才回退桌面。
    let path = save_output(&format!("WinSuiteX_硬件报告_{ts}.html"), |p| {
        std::fs::write(p, &html).is_ok()
    });
    match path {
        Some(p) => {
            crate::open_in_default(&p);
        }
        None => show_error("报告保存失败：exe 目录与桌面都不可写"),
    }
}

/// 产出文件目录：**exe 所在目录**（Z.W. 定案）。
///
/// 理由：工具是被拷到客户机上用的，产出物跟 exe 放一起最好找、最好一起拷走；
/// 桌面反而会在不同客户机上散落。取不到 exe 路径时回退桌面。
pub fn output_dir() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or_else(desktop_dir)
}

/// 桌面目录（`USERPROFILE\Desktop`）；取不到退化为当前目录。
pub fn desktop_dir() -> std::path::PathBuf {
    std::env::var("USERPROFILE")
        .map(|p| std::path::PathBuf::from(p).join("Desktop"))
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
}

/// 保存产出文件：先试 exe 同目录，失败再试桌面，都失败返回 `None`。
///
/// `write` 由调用方给（报告是 `fs::write`、截图是 GDI+ 编码），这样"保存到哪"
/// 的回退策略只有这一处，两条产出链路不会各写一套而漂移。
pub fn save_output(
    name: &str,
    write: impl Fn(&std::path::Path) -> bool,
) -> Option<std::path::PathBuf> {
    for dir in [output_dir(), desktop_dir()] {
        let p = dir.join(name);
        if write(&p) {
            return Some(p);
        }
    }
    None
}

// ============================================================
// 垂直滚动条
// ============================================================

/// 画右侧细滚动条（仅内容溢出时调用）。
///
/// GDI 没有现成滚动条控件（`CreateScrollBar` 是系统控件、要主题样式），
/// 所以自绘：4px 轨道 + 圆角滑块，滑块位置按 `offset / max_offset` 比例。
pub fn draw_scrollbar(
    hdc: HDC,
    w: i32,
    h: i32,
    top: i32,
    view_h: i32,
    offset: i32,
    max_offset: i32,
) {
    if max_offset <= 0 {
        return;
    }
    let bar_w = scale(6);
    // 轨道贴在内容区右缘（不是窗口右缘）——外圈那一格是阴影留白，
    // 压上去滚动条会看起来"飘"在窗口边框外。
    let x = content_right(w) - bar_w - scale(4);
    let track = RECT {
        left: x,
        top: top + scale(4),
        right: x + bar_w,
        bottom: top + view_h - scale(4),
    };
    // 轨道/滑块同样用"叠白"表达，不引实灰（玻璃主题下实灰会显脏）。
    fill_round(hdc, &track, bar_w / 2, over(grad_at(top, h), C_WHITE, 0.18));

    // 滑块高度按"可视/总内容"比例，最小 24px 保证可点。
    let content_h = view_h + max_offset;
    let ratio = (view_h as f32 / content_h.max(1) as f32).clamp(0.08, 1.0);
    let thumb_h = ((track.bottom - track.top) as f32 * ratio) as i32;
    let thumb_h = thumb_h.max(scale(24));
    let span = (track.bottom - track.top - thumb_h).max(0);
    let pos = (offset as f32 / max_offset.max(1) as f32 * span as f32) as i32;
    let thumb = RECT {
        left: track.left,
        top: track.top + pos,
        right: track.right,
        bottom: track.top + pos + thumb_h,
    };
    fill_round(hdc, &thumb, bar_w / 2, C_WHITE);
}

/// 弹出错误提示（`MessageBoxW`）。
///
/// 右键功能写注册表可能失败（权限/组策略锁定）。**静默失败比报错更糟**——
/// 用户会以为开关坏了反复点，所以必须弹框说清楚。
pub fn show_error(msg: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let wmsg: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
    let wcap: Vec<u16> = "操作失败"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: 两个串均 NUL 结尾且在调用期间存活；父窗口传None 用桌面为父。
    unsafe {
        let _ = MessageBoxW(
            None,
            windows::core::PCWSTR(wmsg.as_ptr()),
            windows::core::PCWSTR(wcap.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(test)]
mod kv_wrap_tests {
    //! 第二十二轮：键值行值列按 ` · ` 自动折行（真机截图：网卡行 IP、空槽槽位列表
    //! 超出窗口右边缘被 `DrawTextW` 硬裁）。装箱/断行是纯逻辑，宽度序列可造假，
    //! 不依赖 DC 即可钉住行为。

    use super::{pack_atoms, split_overlong};

    #[test]
    fn pack_single_line_when_fits() {
        // 两段 + 分隔 240 ≤ 250 → 一行
        assert_eq!(pack_atoms(&[100, 100], 40, 250), vec![(0, 2)]);
    }

    #[test]
    fn pack_breaks_at_separator() {
        // 第三段放不下 → 断在分隔符处，[(0,2), (2,3)]
        assert_eq!(pack_atoms(&[100, 100, 100], 40, 250), vec![(0, 2), (2, 3)]);
    }

    #[test]
    fn pack_each_atom_own_row_when_too_wide() {
        // 每段都顶满 → 逐段一行
        assert_eq!(
            pack_atoms(&[250, 250, 250], 40, 250),
            vec![(0, 1), (1, 2), (2, 3)]
        );
    }

    #[test]
    fn pack_empty_input() {
        assert!(pack_atoms(&[], 40, 250).is_empty());
    }

    #[test]
    fn split_overlong_breaks_by_char_width() {
        // 每字符 10px、限宽 30 → 3 字符一行
        assert_eq!(split_overlong("abcdef", |_| 10, 30), vec!["abc", "def"]);
    }

    #[test]
    fn split_overlong_single_char_per_row_when_huge() {
        // 单字符就超宽 → 每字符一行（不丢字）
        assert_eq!(split_overlong("ab", |_| 50, 30), vec!["a", "b"]);
    }

    #[test]
    fn split_overlong_keeps_trailing() {
        // 尾行不足也要保留
        assert_eq!(split_overlong("abcde", |_| 10, 30), vec!["abc", "de"]);
    }
}

#[cfg(test)]
mod hw_vendor_zh_tests {
    //! 第十五轮建、第二十轮改：硬件页厂商字段必须经 `brand_display` 渲染
    //! **`英文（中文）`** 规范格式（Z.W. 定案）。
    //!
    //! VM（QEMU 虚拟硬件）截图中所有厂商都无法映射，正向路径只能靠这里验证；
    //! 回退路径（未命中回原文）两边都要钉住，防止有人把 `brand_display_or` 换回
    //! 裸 `v.to_string()` 而不被发现。中文输入（真机 WMI 本地化串）必须与英文
    //! 输入命中同一规范条目。

    use super::{build_hw_sections, Section};
    use winsuitex_core::model::{BoardInfo, GpuKind, MemStick, MemoryInfo};
    use winsuitex_core::CollectResult;

    fn stick(locator: &str, vendor: Option<&str>) -> MemStick {
        MemStick {
            locator: Some(locator.into()),
            bank: None,
            serial: None,
            part_no: None,
            vendor: vendor.map(Into::into),
            mem_type: Some("DDR4".into()),
            capacity_mb: Some(winsuitex_core::model::MebiBytes::new(8192)),
            speed_mhz: Some(2666),
        }
    }

    fn res(board: BoardInfo, sticks: Vec<MemStick>) -> CollectResult {
        CollectResult {
            volumes: vec![],
            info: winsuitex_core::model::HardwareInfo {
                board: Some(board),
                memory: MemoryInfo {
                    total_mb: None,
                    sticks,
                },
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn find<'a>(sections: &'a [Section], title: &str) -> &'a Section {
        sections
            .iter()
            .find(|s| s.title == title)
            .unwrap_or_else(|| panic!("缺 {title} 分组"))
    }

    #[test]
    fn board_vendor_mapped_to_chinese_and_put_first() {
        let r = res(
            BoardInfo {
                vendor: Some("ASUSTeK COMPUTER INC.".into()),
                product: Some("ROG STRIX B550-A".into()),
                bios_vendor: Some("American Megatrends Inc.".into()),
                bios_version: Some("2.60".into()),
                ..Default::default()
            },
            vec![],
        );
        let sections = build_hw_sections(&r);
        let s = find(&sections, "主板 / BIOS");
        assert_eq!(s.rows[0], ("厂商".into(), "ASUS（华硕）".into()));
        assert_eq!(s.rows[1], ("型号".into(), "ROG STRIX B550-A".into()));
        assert_eq!(
            s.rows[2],
            (
                "BIOS".into(),
                "American Megatrends（美商安迈） · 2.60".into()
            )
        );
    }

    #[test]
    fn board_chipset_renders_when_present() {
        // C1：芯片组经 SetupAPI 采集，有值时在「型号」之后、「BIOS」之前单独成行；
        // None 时不加行（诚实留白）。
        let r = res(
            BoardInfo {
                vendor: Some("ASUSTeK COMPUTER INC.".into()),
                product: Some("ROG STRIX B550-A".into()),
                chipset: Some("AMD B550 Chipset".into()),
                bios_vendor: Some("American Megatrends Inc.".into()),
                bios_version: Some("2.60".into()),
                ..Default::default()
            },
            vec![],
        );
        let sections = build_hw_sections(&r);
        let s = find(&sections, "主板 / BIOS");
        assert_eq!(s.rows[0], ("厂商".into(), "ASUS（华硕）".into()));
        assert_eq!(s.rows[1], ("型号".into(), "ROG STRIX B550-A".into()));
        assert_eq!(s.rows[2], ("芯片组".into(), "AMD B550 Chipset".into()));
        assert_eq!(
            s.rows[3],
            (
                "BIOS".into(),
                "American Megatrends（美商安迈） · 2.60".into()
            )
        );
    }

    // —— 显卡厂商渲染（修复：GUI 此前完全漏渲染 g.vendor） ——

    /// 构造一份只带显卡、其余默认空的采集结果。
    fn res_gpu(gpus: Vec<winsuitex_core::model::Gpu>) -> CollectResult {
        CollectResult {
            volumes: vec![],
            info: winsuitex_core::model::HardwareInfo {
                gpus,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn gpu_vendor_renders_in_section() {
        let r = res_gpu(vec![winsuitex_core::model::Gpu {
            name: "NVIDIA GeForce RTX 4070".into(),
            vendor: Some("MSI".into()),
            kind: GpuKind::Physical,
            vram_mb: Some(winsuitex_core::model::MebiBytes::new(8192)),
            ..Default::default()
        }]);
        let sections = build_hw_sections(&r);
        let s = find(&sections, "显卡");
        // 多显卡时每行以「显卡 #N」为键，避免多块都叫「型号」分不清。
        assert_eq!(s.rows[0].0, "显卡 #1");
        let v = &s.rows[0].1;
        assert!(v.contains("RTX 4070"), "型号应在: {v}");
        assert!(v.contains("MSI（微星）"), "厂商应渲染: {v}");
        assert!(v.contains("8 GiB 显存"), "显存应在: {v}");
    }

    #[test]
    fn gpu_vendor_dedup_when_name_already_carries_brand() {
        // 名称已含品牌（如 "PNY GeForce ..."）且品牌为单段（en==zh）时，
        // 不应再挂一次 " · PNY"，避免冗余；与报告 dedupe 口径一致。
        let r = res_gpu(vec![winsuitex_core::model::Gpu {
            name: "PNY GeForce RTX 4060".into(),
            vendor: Some("PNY".into()),
            kind: GpuKind::Physical,
            vram_mb: Some(winsuitex_core::model::MebiBytes::new(8192)),
            ..Default::default()
        }]);
        let sections = build_hw_sections(&r);
        let s = find(&sections, "显卡");
        assert!(
            !s.rows[0].1.contains("· PNY"),
            "名称已含 PNY，不应重复挂品牌: {}",
            s.rows[0].1
        );
    }

    #[test]
    fn gpu_virtual_adapter_labeled_not_branded() {
        // 虚拟/软件适配器不是真实显卡，显式标注、不渲染板卡品牌。
        let r = res_gpu(vec![winsuitex_core::model::Gpu {
            name: "Microsoft Basic Render Driver".into(),
            vendor: None,
            kind: GpuKind::Software,
            vram_mb: None,
            ..Default::default()
        }]);
        let sections = build_hw_sections(&r);
        let s = find(&sections, "显卡");
        assert!(
            s.rows[0].1.contains("软件渲染器"),
            "应标注类型: {}",
            s.rows[0].1
        );
        assert!(
            !s.rows[0].1.contains("（"),
            "虚拟适配器不应渲染板卡品牌: {}",
            s.rows[0].1
        );
    }

    // —— 网卡多 IPv4 渲染（修复：此前只 .find() 第一个 IP） ——

    /// 构造只带网卡、其余默认空的采集结果。
    fn res_nic(nics: Vec<winsuitex_core::model::Nic>) -> CollectResult {
        CollectResult {
            volumes: vec![],
            info: winsuitex_core::model::HardwareInfo {
                nics,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn nic_shows_all_ipv4_addresses() {
        let r = res_nic(vec![winsuitex_core::model::Nic {
            desc: Some("Realtek PCIe GBE".into()),
            ipv4: vec!["192.168.1.10".into(), "10.0.0.5".into()],
            up: true,
            speed_bps: Some(1_000_000_000),
            ..Default::default()
        }]);
        let sections = build_hw_sections(&r);
        let s = find(&sections, "网卡");
        let v = &s.rows[0].1;
        assert!(v.contains("192.168.1.10"), "应含第一个 IP: {v}");
        assert!(v.contains("10.0.0.5"), "应含第二个 IP: {v}");
    }

    #[test]
    fn nic_apipa_hidden_when_real_ip_present() {
        // 有真实地址时，APIPA 兜底地址不显示（与报告口径一致）。
        let r = res_nic(vec![winsuitex_core::model::Nic {
            desc: Some("Intel Ethernet".into()),
            ipv4: vec!["192.168.1.10".into(), "169.254.1.2".into()],
            up: true,
            ..Default::default()
        }]);
        let sections = build_hw_sections(&r);
        let s = find(&sections, "网卡");
        assert!(
            !s.rows[0].1.contains("169.254"),
            "有真实 IP 时不应显示 APIPA: {}",
            s.rows[0].1
        );
    }

    #[test]
    fn nic_down_filtered_out() {
        // 未连接网卡整体不进分组（第三十轮定案）。
        let r = res_nic(vec![winsuitex_core::model::Nic {
            desc: Some("Intel Ethernet".into()),
            ipv4: vec!["192.168.1.10".into()],
            up: false,
            ..Default::default()
        }]);
        let sections = build_hw_sections(&r);
        assert!(
            sections.iter().all(|s| s.title != "网卡"),
            "未连接网卡不应出现网卡分组"
        );
    }

    #[test]
    fn memory_vendor_mapped_to_chinese() {
        let r = res(
            BoardInfo::default(),
            vec![stick("DIMM_A1", Some("Kingston Technology"))],
        );
        let sections = build_hw_sections(&r);
        let s = find(&sections, "内存");
        let row = &s.rows[2]; // 总计 / 插槽 之后第一根条
        assert!(
            row.1.contains("Kingston（金士顿）"),
            "内存行应含 `英文（中文）` 厂商: {}",
            row.1
        );
    }

    #[test]
    fn chinese_vendor_input_maps_to_same_display() {
        // 真机中文系统 WMI 直接返回中文名（第二十轮 Z.W. 截图报障场景）。
        let r = res(
            BoardInfo {
                vendor: Some("超微".into()),
                product: Some("X10SRL-F".into()),
                bios_vendor: Some("美商安迈".into()),
                bios_version: Some("2.0b".into()),
                ..Default::default()
            },
            vec![stick("DIMM1", Some("三星"))],
        );
        let sections = build_hw_sections(&r);
        let s = find(&sections, "主板 / BIOS");
        assert_eq!(s.rows[0], ("厂商".into(), "Supermicro（超微）".into()));
        assert_eq!(
            s.rows[2],
            (
                "BIOS".into(),
                "American Megatrends（美商安迈） · 2.0b".into()
            )
        );
        let m = find(&sections, "内存");
        assert!(
            m.rows[2].1.contains("Samsung（三星）"),
            "中文输入应得同一规范显示: {}",
            m.rows[2].1
        );
    }

    #[test]
    fn unknown_vendor_falls_back_to_raw() {
        let r = res(
            BoardInfo {
                vendor: Some("QEMU".into()),
                ..Default::default()
            },
            vec![stick("DIMM_A1", Some("QEMU"))],
        );
        let sections = build_hw_sections(&r);
        let s = find(&sections, "主板 / BIOS");
        assert_eq!(s.rows[0], ("厂商".into(), "QEMU".into()));
        let msections = build_hw_sections(&r);
        let m = find(&msections, "内存");
        assert!(m.rows[2].1.contains("QEMU"));
    }

    #[test]
    fn missing_vendor_adds_no_row() {
        let r = res(BoardInfo::default(), vec![]);
        let sections = build_hw_sections(&r);
        let s = find(&sections, "主板 / BIOS");
        assert_eq!(s.rows[0].0, "型号", "无厂商时不得出现空厂商行");
    }
}
