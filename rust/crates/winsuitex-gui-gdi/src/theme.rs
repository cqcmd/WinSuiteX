//! WinSuiteX GDI 面板的**视觉语言**：配色、间距体系。
//!
//! ## GDI 怎么做"半透明"
//! GDI 没有 alpha 通道，所有毛玻璃效果都用**"取该处渐变色 → 与目标色按比例混合"**
//! 实现（见 [`mix`]）。因为底色是随 y 变化的渐变，混合必须**逐行**做，
//! 见 `draw::fill_glass_round` —— 一次性混一个固定值会让长卡片上下端与背景脱节。
// ============================================================

#![allow(dead_code)]

use windows::Win32::Foundation::COLORREF;

// ============================================================
// 混合原语
// ============================================================

/// GDI 颜色（`COLORREF`，0x00BBGGRR）。
pub type Color = COLORREF;

/// 按 r/g/b 组装 `COLORREF`（Windows 用 BGR 存储，常量式写法更直观）。
///
/// 注意 `COLORREF` 在 windows 0.62 是 `pub struct COLORREF(pub u32)` 的 tuple struct，
/// 不是 type alias，所以只能 `Color(v)` 构造，不能用 `as Color` 强转。
pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    // 注意：上面 `Color` 只是 `type Color = COLORREF` 的别名，别名不能当构造器用，
    // 因此这里必须写真名 `COLORREF(..)`。
    COLORREF((b as u32) << 16 | (g as u32) << 8 | r as u32)
}

/// 拆出 COLORREF 的 r/g/b（Windows 内部 0x00BBGGRR）。
fn split(c: Color) -> (u8, u8, u8) {
    let v = c.0;
    (
        (v & 0xFF) as u8,
        ((v >> 8) & 0xFF) as u8,
        ((v >> 16) & 0xFF) as u8,
    )
}

/// 颜色按 `t` 线性混合（`t=0` 得 `a`，`t=1` 得 `b`）。
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let (ar, ag, ab) = split(a);
    let (br, bg, bb) = split(b);
    let f = |x: u8, y: u8| -> u8 {
        (x as f32 + (y as f32 - x as f32) * t.clamp(0.0, 1.0))
            .round()
            .clamp(0.0, 255.0) as u8
    };
    rgb(f(ar, br), f(ag, bg), f(ab, bb))
}

/// 在 `base` 上叠一层 `over`，`alpha` 是 `over` 的不透明度（模拟半透明）。
#[inline]
pub fn over(base: Color, over_c: Color, alpha: f32) -> Color {
    mix(base, over_c, alpha)
}

// ============================================================
// 配色（深空玻璃 · 紫蓝渐变 + 深 navy 玻璃）
//
// 与 `design-demos/ui-schemes-compare.html` 方案 A 同源（零回归护栏：
// `smoke_tests::theme_constants_regression` 钉死这些关键值）。
// ============================================================

/// 整窗垂直渐变顶。
pub const C_GRAD_TOP: Color = rgb(135, 81, 238);
/// 整窗垂直渐变底。
pub const C_GRAD_BOT: Color = rgb(38, 98, 235);
/// 底部栏实色。
pub const C_FOOTER_BG: Color = rgb(27, 54, 122);
/// 窗口外描边（中深灰，任何壁纸下可见）。
pub const C_WIN_BORDER: Color = rgb(170, 160, 245);

/// 主文字（卡片/渐变上的白字）。
pub const C_ON_GRAD: Color = rgb(255, 255, 255);
/// 次级文字（页签未选中 / 表头）。
pub const C_ON_GRAD_DIM: Color = rgb(224, 216, 250);
/// 弱文字（字段名 / 说明 / 版权）。
pub const C_ON_GRAD_FAINT: Color = rgb(198, 190, 240);
/// 渐变上的**实色线条/强调**（页签下划线、滑块、hover 玻璃层混合目标）= 白。
pub const C_WHITE: Color = rgb(255, 255, 255);

/// 玻璃卡 tint 混合基准色（深 navy，压暗让白字清楚）。
pub const C_GLASS_TINT: Color = rgb(20, 24, 52);
/// 右键页"暗玻璃"混合色。
pub const C_GLASS_DARK: Color = rgb(64, 60, 96);

/// 分组图标统一描边色（淡紫）。
pub const C_ICON: Color = rgb(214, 208, 248);
/// 分组图标柔和粉彩强调色（8 组，顺序见 `icons::HwIcon`）。
/// `C_ACCENT 高饱和 × 0.55 + 淡紫(214,208,248) × 0.45`，深 navy 玻璃卡上的"柔彩"
/// 观感（第九轮 Z.W. 定案：禁用 emoji、禁用一刀切淡紫、禁用高饱和彩虹）。
pub const C_ACCENT_SOFT: [Color; 8] = [
    rgb(115, 210, 242), // Os   柔天蓝
    rgb(234, 174, 144), // Board 柔桃
    rgb(188, 170, 249), // Cpu  柔薰衣草紫
    rgb(124, 210, 196), // Mem  柔薄荷
    rgb(234, 155, 185), // Gpu  柔粉
    rgb(233, 206, 123), // Disk 柔金
    rgb(149, 184, 249), // Nic  柔蓝
    rgb(230, 156, 212), // Audio 柔品红
];

/// 已启用卡片的亮色描边（亮绿）。
pub const C_CARD_EDGE_ON: Color = rgb(52, 199, 89);
/// 纯色卡兜底底色（`draw::card` 死代码原语用）。
pub const C_CARD_BG: Color = rgb(250, 250, 253);
/// 纯色卡描边。
pub const C_BORDER: Color = rgb(217, 217, 217);

/// 选中行底（薄荷绿）。
pub const C_SEL_ROW: Color = rgb(216, 240, 228);
/// 选中行字（深绿）。
pub const C_SEL_ROW_TEXT: Color = rgb(28, 62, 44);

/// 按钮基底色（品牌紫）。
pub const C_BTN: Color = rgb(79, 70, 229);
/// 按钮 hover 色。
pub const C_BTN_H: Color = rgb(99, 91, 240);
/// 白底按钮。
pub const C_BTN_WHITE: Color = rgb(255, 255, 255);
/// 白底按钮 hover。
pub const C_BTN_WHITE_H: Color = rgb(232, 228, 252);
/// 白底按钮字（深靛）。
pub const C_BTN_WHITE_TEXT: Color = rgb(40, 36, 70);

/// 标题栏关闭按钮 hover 红。
pub const C_TB_CLOSE_HOVER: Color = rgb(232, 17, 35);

/// 状态色：成功 / 预警 / 错误。
pub const C_SUCCESS: Color = rgb(52, 199, 89);
pub const C_WARNING: Color = rgb(250, 173, 20);
pub const C_ERROR: Color = rgb(255, 77, 79);

/// 徽章（已启用/未启用/警告）文字/背景色。
pub const C_GREEN_DARK: Color = rgb(21, 128, 61);
pub const C_GREEN_BG: Color = rgb(223, 242, 230);
pub const C_GRAY_DARK: Color = rgb(89, 89, 89);
pub const C_GRAY_BG: Color = rgb(242, 242, 242);
pub const C_AMBER_DARK: Color = rgb(202, 110, 4);
pub const C_AMBER_BG: Color = rgb(254, 244, 225);

/// 取窗口渐变在屏幕 y（0..`h`）处的颜色。
///
/// 卡片要"透"出背景，就必须知道**自己所在位置**的背景是什么色——
/// 所以混合不能用一个固定值，必须按 y 现算。
pub fn grad_at(y: i32, h: i32) -> Color {
    let t = (y.clamp(0, h.max(1)) as f32 / h.max(1) as f32).clamp(0.0, 1.0);
    mix(C_GRAD_TOP, C_GRAD_BOT, t)
}

// ============================================================
// 几何（96-DPI 基准）
// ============================================================

/// 顶栏高度（标题 + 窗口按钮）。
pub const TITLE_H: i32 = 44;
/// 页签条高度。
pub const TAB_H: i32 = 44;
/// 单个页签宽度。
pub const TAB_W: i32 = 96;
/// 底部提示条高度（提示 + 版权）。
pub const FOOTER_H: i32 = 64;
/// 窗口内边距（Ant 8px 栅格 ×3）。
pub const MARGIN: i32 = 24;
/// 阴影绘制留白。
///
/// **曾是 16，现在 0（Z.W. 真机反馈"多了一层底"后删除）。** 之前 frameless
/// 窗口四周留 16px 画自绘阴影，但 GDI 无 alpha 通道，三层实色矩形近似出来
/// 是一圈均匀的灰边（且三层绘制顺序写反，只剩最外层），看起来像窗口里还套
/// 了层底。真机反馈后定案：**不做自绘阴影，窗口 = 内容矩形本身，圆角由
/// `SetWindowRgn` 直接裁**。常量保留为 0 是为了让 `body_pad()` 的所有引用点
/// （布局/命中测试/窗口尺寸）自动归位，不必逐处改写。
pub const SHADOW_PAD: i32 = 0;
/// 内容区默认内边距。
pub const PAD_IN: i32 = 16;
/// 卡片内两列之间的间距。
pub const COL_GAP: i32 = 16;
/// 键值行高。
///
/// **曾是 30，现在 22。** 30 那个值是从 Web 表单习惯抄来的（Ant 的
/// `padding: 4px 0` + 20px 字号 ≈ 30px），但桌面端窗口窄、字段多，
/// 实测 625×980 的窗口里：
/// - 文字墨水只占内容区 **2.85%**（健康值 15–25%），行高 30 意味着每行只有
///   13px 是字、17px 是上下留白；
/// - 硬件页 26 行信息要滚动才看得到底。
///
/// 改为 22 后同样内容少占27%，实测（见 2026-10-07 日志）首屏能多放约 4 行。
/// 代价是相邻两行间距变紧——扫读时仍能分辨（字号 13px / 行高 22px ≈ 1.7 倍行距，
/// 远高于可读下限 1.4），但逐项抄录时不如 30 舒适。
///
/// 一并影响滚轮步长（`scroll_by` 用 `scale(ROW_H)`），所以滚动手感同步变细腻。
pub const ROW_H: i32 = 22;
/// 分组小节条带高度（比键值行高一档，让"节"比"行"更像一个层级）。
pub const SEC_H: i32 = 30;
/// 小节标题下方「小字说明」每行高度（幽灵槽位等需要附注口径时使用）。
/// 用 tiny 字号（12px）弱对比色，弱于正文行、强于页脚版权（Z.W. 2026-10-13）。
pub const SEC_NOTE_H: i32 = 18;
/// 图标瓦片边长。
pub const TILE: i32 = 22;

/// 窗口默认宽高。
pub const WIN_W: i32 = 593;
pub const WIN_H: i32 = 948;
/// 磁盘页自适应窗口的**最小高度**（分区极少时窗口也不至于矮到畸形）。
pub const DISK_MIN_H: i32 = 360;

/// 卡片圆角（96-DPI 基准）。
pub const CARD_RADIUS: i32 = 8;
/// 窗口圆角（96-DPI 基准，`SetWindowRgn` 用）。
pub const WIN_RADIUS: i32 = 12;

// ============================================================
// DPI
// ============================================================

/// 当前窗口 DPI；`scale()` 把 96-DPI 基准值换算为实际像素。
///
/// 用 `AtomicI32` 而非 `static mut`：DPI 在 `WM_DPICHANGED` 里更新、绘制时读取，
/// 若用 `static mut` 则每次读取都要 `unsafe`，一旦漏写就是数据竞争（UB）。
/// 原子读写在 UI 线程上一条指令，代价可忽略，却能让 `scale()` 保持安全。
static DPI: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(96);

/// 设置当前 DPI（窗口创建 / `WM_DPICHANGED` 时调用）。
pub fn set_dpi(dpi: u32) {
    DPI.store(dpi.max(96) as i32, std::sync::atomic::Ordering::Relaxed);
}

/// 把 96-DPI 基准值缩放到当前 DPI。
#[inline]
pub fn scale(v: i32) -> i32 {
    let dpi = DPI.load(std::sync::atomic::Ordering::Relaxed);
    ((v * dpi + 48) / 96).max(1)
}

// ============================================================
// 调试用可调主题参数（「调试」页实时调节，Z.W. 试出满意值后固化回 theme.rs）
// ============================================================
//
// 这两个参数是**调试滑杆**的作用目标：全部 tint 玻璃卡（硬件页大卡/头部/
// 表格/详情卡/分组横条）都读取它们，于是拖动滑杆时全窗口玻璃观感实时联动。

/// 控件① 玻璃透明度：tint 叠层的不透明度百分比，0-100。
/// 0 = 卡片完全透出窗口渐变（只剩文字浮在渐变上）；100 = tint 层全实。
static GLASS_ALPHA: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(22);

/// 控件② 叠色深浅：tint 混合色在「渐变顶 → 深 navy tint」之间的位置百分比，0-100。
/// 100 = tint 全实（默认深 navy 最暗）；0 = 渐变顶色。
static TINT_DEPTH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(100);

/// 取玻璃透明度（0.0-1.0，供绘制原语直接消费）。
pub fn glass_alpha() -> f32 {
    GLASS_ALPHA.load(std::sync::atomic::Ordering::Relaxed) as f32 / 100.0
}

/// 设置玻璃透明度（百分比，自动钳制 0-100）。
pub fn set_glass_alpha(pct: u32) {
    GLASS_ALPHA.store(pct.min(100), std::sync::atomic::Ordering::Relaxed);
}

/// 取叠色深浅（百分比 0-100）。
pub fn tint_depth() -> u32 {
    TINT_DEPTH.load(std::sync::atomic::Ordering::Relaxed)
}

/// 设置叠色深浅（百分比，自动钳制 0-100）。
pub fn set_tint_depth(pct: u32) {
    TINT_DEPTH.store(pct.min(100), std::sync::atomic::Ordering::Relaxed);
}

/// 当前 tint 混合色：渐变顶 `C_GRAD_TOP` → 深 navy `C_GLASS_TINT` 按深浅插值。
/// 深浅 100 = tint 全实（默认）；0 = 渐变顶色。
pub fn tint_color() -> Color {
    mix(C_GRAD_TOP, C_GLASS_TINT, tint_depth() as f32 / 100.0)
}
