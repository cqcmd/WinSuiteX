//! WinSuiteX GDI 面板的**视觉语言**：配色、字体、间距、圆角。
//!
//! 设计取自 Ant Design 规范（8px 栅格、`colorBgContainer`/`blue-7` 等语义色），
//! 与 C++ 版同源但更精细一档：
//!   - 圆角 12px（C++ 版 8px），在大卡片上更显现代；
//!   - 卡片是"玻璃奶白"而非纯白，配合紫色顶栏形成层次；
//!   - 顶栏用 #7C3AED → #5E6EDC → #2563EB 三段紫蓝渐变（同 C++ 版 glass4）。
//!
//! 所有几何常量都是 **96-DPI 基准值**，绘制时统一经 `scale()` 换算到实际像素，
//! 布局关系因此一目了然，且高 DPI 下不会错位。

// 本模块是**视觉语言表**：色板 / 间距 / 圆角先按设计语言列全，用到的自然消耗，
// 尚未用到的（如按下态、错误态、深色态）在后续迭代自然会被页面代码取用。
// 一次性列全，好过每加一个色就临时起名、导致同色不同名。
#![allow(dead_code)]

use windows::Win32::Foundation::COLORREF;

// ============================================================
// 配色 —— **紫蓝渐变玻璃主题**（与 C++ 版 1.6.7 同一套视觉语言）
//
// 全部色值来自 C++ 版真机截图的**像素采样**（2026-10-07 用 PIL 量），
// 不是凭印象调的。采样口径与结果见 `.workbuddy/memory/2026-10-07.md` 第七轮。
//
// ## GDI 怎么做"半透明"
// GDI 没有 alpha 通道，所有毛玻璃效果都用**"取该处渐变色 → 与目标色按比例混合"**
// 实现（见 [`mix`]）。因为底色是随 y 变化的渐变，混合必须**逐行**做，
// 见 `draw::fill_glass_round` —— 一次性混一个固定值会让长卡片上下端与背景脱节。
// ============================================================

/// 整窗垂直渐变顶（实测 C++ 顶栏 `(135,81,238)`）。
pub const C_GRAD_TOP: Color = rgb(135, 81, 238);
/// 整窗垂直渐变底（实测 C++ 窗口底 `(38,98,235)`）。
pub const C_GRAD_BOT: Color = rgb(38, 98, 235);
/// 底部栏实色（实测 `(27,54,122)`）——比渐变底更深的藏青，压住整窗重心。
pub const C_FOOTER_BG: Color = rgb(27, 54, 122);
/// 窗口外描边（渐变上叠白 45% 的观感，任何壁纸上都能看清轮廓）。
pub const C_WIN_BORDER: Color = rgb(170, 160, 245);

/// 纯白：既是渐变上的文字色，也是玻璃卡片的混合目标色。
pub const C_WHITE: Color = rgb(255, 255, 255);
/// 渐变上的主文字。
pub const C_ON_GRAD: Color = rgb(255, 255, 255);
/// 渐变上次级文字（白 ~85%，页签未选中 / 表头）。
pub const C_ON_GRAD_DIM: Color = rgb(224, 216, 250);
/// 渐变上更淡的文字（白 ~70%，字段名 / 说明 / 版权）。
pub const C_ON_GRAD_FAINT: Color = rgb(198, 190, 240);

/// 右键页卡片的"暗玻璃"混合色（叠在渐变上 ≈ C++ 的毛玻璃暗卡）。
pub const C_GLASS_DARK: Color = rgb(64, 60, 96);
/// 内容卡片的"深玻璃"混合色：在渐变上叠一层深 navy，把卡片**压暗**而非提亮，
/// 让白字对比度从 ~3.5:1 拉到 ~5.5:1（Z.W. 反馈"字看不清"的根因是之前
/// 用白 18% 把卡片提亮，白字压在浅底上对比度不足）。菜单页同理用 C_GLASS_DARK。
pub const C_GLASS_TINT: Color = rgb(20, 24, 52);
/// 分组图标统一描边色：玻璃主题下不再用 8 色高饱强调色（太花眼），
/// 统一柔和淡紫，与 C++ 版单色线性图标一致。
pub const C_ICON: Color = rgb(214, 208, 248);
/// 卡片描边：已启用=亮绿 / 未启用=叠白（运行时混合）。
pub const C_CARD_EDGE_ON: Color = rgb(52, 199, 89);

/// 磁盘页选中行：薄荷绿实底（实测 `(216,240,228)`）+ 深字。
pub const C_SEL_ROW: Color = rgb(216, 240, 228);
pub const C_SEL_ROW_TEXT: Color = rgb(28, 62, 44);

/// 靛蓝按钮（实测 C++「启用」按钮 `(79,70,229)`）。
pub const C_BTN: Color = rgb(79, 70, 229);
pub const C_BTN_H: Color = rgb(99, 91, 240);
/// 白底按钮（导出报告 / 一键复制），深字。
pub const C_BTN_WHITE: Color = rgb(255, 255, 255);
pub const C_BTN_WHITE_H: Color = rgb(232, 228, 252);
pub const C_BTN_WHITE_TEXT: Color = rgb(40, 36, 70);
/// 关闭按钮 hover 红。
pub const C_TB_CLOSE_HOVER: Color = rgb(232, 17, 35);

// —— 以下保留为兼容层：部分旧常量仍在 `draw.rs` 的通用控件里被引用 ——
/// 窗口底色（渐变主题下不再单独使用，仅作为 DIB 初始化兜底色）。
pub const C_BG: Color = rgb(135, 81, 238);
/// 顶栏底色（同渐变顶）。
pub const C_TB_BG: Color = rgb(135, 81, 238);
/// 卡片底（玻璃主题下卡片由逐行混合生成，这是兜底值）。
pub const C_CARD_BG: Color = rgb(250, 250, 253);
/// 卡片描边。
pub const C_BORDER: Color = rgb(217, 217, 217);
/// 分隔线。
pub const C_DIVIDER: Color = rgb(240, 240, 240);
/// 主文字（渐变主题下统一用白）。
pub const C_TITLE: Color = rgb(255, 255, 255);
/// 正文。
pub const C_TEXT: Color = rgb(255, 255, 255);
/// 次级文字。
pub const C_SUBTITLE: Color = rgb(224, 216, 250);
/// 描述文字。
pub const C_DESC: Color = rgb(198, 190, 240);
/// 主色（渐变主题下=靛蓝按钮色）。
pub const C_PRIMARY: Color = rgb(79, 70, 229);
pub const C_PRIMARY_H: Color = rgb(99, 91, 240);
pub const C_PRIMARY_A: Color = rgb(60, 52, 200);
/// 成功绿 / 警告琥珀 / 错误红（徽章与状态色仍用）。
pub const C_SUCCESS: Color = rgb(52, 199, 89);
pub const C_WARNING: Color = rgb(250, 173, 20);
pub const C_ERROR: Color = rgb(255, 77, 79);
/// 徽章色（已启用 / 未启用 / 警告）。
pub const C_GREEN_DARK: Color = rgb(21, 128, 61);
pub const C_GREEN_BG: Color = rgb(223, 242, 230);
pub const C_GRAY_DARK: Color = rgb(89, 89, 89);
pub const C_GRAY_BG: Color = rgb(242, 242, 242);
pub const C_AMBER_DARK: Color = rgb(202, 110, 4);
pub const C_AMBER_BG: Color = rgb(254, 244, 225);

/// 硬件页各分组的图标强调色（C++ 版同思路：每组一个可辨识的色）。
///
/// ⚠️ 这是**高饱和原版**，第八轮因"太花眼"被停用，这里仅作兼容保留（见下方
/// `C_ACCENT_SOFT`）。分组图标实际渲染走 `C_ACCENT_SOFT`。
pub const C_ACCENT: [Color; 8] = [
    rgb(34, 211, 238),  // Os   青
    rgb(251, 146, 60),  // Board 橙
    rgb(167, 139, 250), // Cpu  紫
    rgb(52, 211, 153),  // Mem  绿
    rgb(251, 113, 133), // Gpu  玫红
    rgb(250, 204, 21),  // Disk 黄
    rgb(96, 165, 250),  // Nic  蓝
    rgb(244, 114, 182), // Audio 粉
];

/// 分组图标**柔和粉彩**强调色（Z.W. 反馈"小图标没色彩了"后的折中）。
///
/// 第九轮：第八轮把图标一刀切成淡紫单色（`C_ICON`）治"花眼"，但用户觉得"没了色彩"。
/// 这版把 `C_ACCENT` 每个色相**混入约 45% 淡紫底 `C_ICON`**——既保留分组可辨识的
/// 色相，又把饱和度压到不刺眼，在深 navy 玻璃卡上是"柔彩"而非"彩虹"。
/// 数值 = `accent * 0.55 + C_ICON(214,208,248) * 0.45`，手写取整。
pub const C_ACCENT_SOFT: [Color; 8] = [
    rgb(115, 210, 242), // Os   柔天蓝（青降饱和）
    rgb(234, 174, 144), // Board 柔桃（橙降饱和）
    rgb(188, 170, 249), // Cpu  柔薰衣草紫
    rgb(124, 210, 196), // Mem  柔薄荷（绿降饱和）
    rgb(234, 155, 185), // Gpu  柔粉（玫红降饱和）
    rgb(233, 206, 123), // Disk 柔金（黄降饱和）
    rgb(149, 184, 249), // Nic  柔蓝
    rgb(230, 156, 212), // Audio 柔品红
];

/// 取窗口渐变在屏幕 y（0..`h`）处的颜色。
///
/// 卡片要"透"出背景，就必须知道**自己所在位置**的背景是什么色——
/// 所以混合不能用一个固定值，必须按 y 现算。
pub fn grad_at(y: i32, h: i32) -> Color {
    let t = (y.clamp(0, h.max(1)) as f32 / h.max(1) as f32).clamp(0.0, 1.0);
    mix(C_GRAD_TOP, C_GRAD_BOT, t)
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

/// 拆出 COLORREF 的 r/g/b（Windows 内部 0x00BBGGRR）。
fn split(c: Color) -> (u8, u8, u8) {
    let v = c.0;
    (
        (v & 0xFF) as u8,
        ((v >> 8) & 0xFF) as u8,
        ((v >> 16) & 0xFF) as u8,
    )
}

/// 卡片圆角（玻璃卡片比窗口圆角小一档，层次更清楚）。
pub const CARD_RADIUS: i32 = 8;

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
/// 窗口/卡片圆角（比 C++ 的 8 略大，更现代）。
pub const WIN_RADIUS: i32 = 12;
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
/// 图标瓦片边长。
pub const TILE: i32 = 22;

/// 窗口默认宽高。
pub const WIN_W: i32 = 593;
pub const WIN_H: i32 = 948;
/// 磁盘页自适应窗口的**最小高度**（分区极少时窗口也不至于矮到畸形）。
pub const DISK_MIN_H: i32 = 360;

// ============================================================
// 类型
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
