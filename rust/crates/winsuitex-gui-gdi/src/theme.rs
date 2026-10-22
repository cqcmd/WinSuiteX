//! WinSuiteX GDI 面板的**视觉语言**：配色、间距体系。
//!
//! ## GDI 怎么做"半透明"
//! GDI 没有 alpha 通道，所有毛玻璃效果都用**"取该处渐变色 → 与目标色按比例混合"**
//! 实现（见 [`mix`]）。因为底色是随 y 变化的渐变，混合必须**逐行**做，
//! 见 `draw::fill_glass_round` —— 一次性混一个固定值会让长卡片上下端与背景脱节。
//
// ## 双主题运行时切换
// 面板内置两套完整主题：**深空玻璃**（紫蓝渐变 + navy 玻璃，科技/电竞感）与
// **流畅灰紫**（低饱和靛灰渐变 + Smoke 深色玻璃，安静专业）。两套字段完全一致、
// 只是取值不同，因此把颜色从「编译期 `const`」改为「运行时按当前主题取值」：
// `theme()` 返回 `&'static Theme`，调用点写 `theme().C_XXX`。几何常量（窗口尺寸 /
// 圆角 / 行高）两套相同，保持 `const` 不变。切换走全局 `AtomicU8` + `set_theme`，
// 选择持久化到 `%APPDATA%/WinSuiteX/theme.txt`，重启仍生效。

#![allow(dead_code)]
#![allow(non_snake_case)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
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
// 主题调色板
// ============================================================
//
// 两套主题字段完全一致，仅取值不同。颜色名沿用历史 `C_*` 命名（见文件顶部
// 「双主题运行时切换」说明），几何参数不在此结构内（见下方 `const`）。

/// 一套完整主题的全部颜色 + 玻璃观感参数。
#[derive(Clone, Copy)]
pub struct Theme {
    /// 主题名（footer 切换按钮显示）。
    pub NAME: &'static str,
    /// 整窗垂直渐变顶。
    pub C_GRAD_TOP: Color,
    /// 整窗垂直渐变底。
    pub C_GRAD_BOT: Color,
    /// 底部栏实色。
    pub C_FOOTER_BG: Color,
    /// 窗口外描边 / 压圆角弧线对比度的暗边环（窗形由分层 alpha 羽化）。
    pub C_WIN_BORDER: Color,
    /// 主文字（卡片/渐变上的白字）。
    pub C_ON_GRAD: Color,
    /// 次级文字（页签未选中 / 表头）。
    pub C_ON_GRAD_DIM: Color,
    /// 弱文字（字段名 / 说明 / 版权）。
    pub C_ON_GRAD_FAINT: Color,
    /// 渐变上的实色线条/强调（页签下划线、滑块、hover 玻璃层混合目标）= 白。
    pub C_WHITE: Color,
    /// 玻璃卡 tint 混合基准色（压暗让白字清楚）。
    pub C_GLASS_TINT: Color,
    /// 右键页"暗玻璃"混合色。
    pub C_GLASS_DARK: Color,
    /// 分组图标统一描边色。
    pub C_ICON: Color,
    /// 分组图标柔和粉彩强调色（8 组，顺序见 `icons::HwIcon`）。两套主题共用同一组
    /// 柔和粉彩——图标强调是产品统一视觉语言，不随背景气质切换而改变。
    pub C_ACCENT_SOFT: [Color; 8],
    /// 已启用卡片的亮色描边（亮绿）。
    pub C_CARD_EDGE_ON: Color,
    /// 纯色卡兜底底色（死代码原语用）。
    pub C_CARD_BG: Color,
    /// 纯色卡描边。
    pub C_BORDER: Color,
    /// 选中行底（薄荷绿）。
    pub C_SEL_ROW: Color,
    /// 选中行字（深绿）。
    pub C_SEL_ROW_TEXT: Color,
    /// 按钮基底色（品牌紫）。
    pub C_BTN: Color,
    /// 按钮 hover 色。
    pub C_BTN_H: Color,
    /// 白底按钮。
    pub C_BTN_WHITE: Color,
    /// 白底按钮 hover。
    pub C_BTN_WHITE_H: Color,
    /// 白底按钮字（深靛）。
    pub C_BTN_WHITE_TEXT: Color,
    /// 标题栏关闭按钮 hover 红。
    pub C_TB_CLOSE_HOVER: Color,
    /// 状态色：成功 / 预警 / 错误。
    pub C_SUCCESS: Color,
    /// 状态色：预警。
    pub C_WARNING: Color,
    /// 状态色：错误。
    pub C_ERROR: Color,
    /// 徽章（已启用/未启用/警告）文字/背景色。
    pub C_GREEN_DARK: Color,
    pub C_GREEN_BG: Color,
    pub C_GRAY_DARK: Color,
    pub C_GRAY_BG: Color,
    pub C_AMBER_DARK: Color,
    pub C_AMBER_BG: Color,
    /// 卡片圆角（各套略有差异：深空 8 / 其它 10）。
    pub CARD_RADIUS: i32,
    /// 玻璃透明度（tint 叠层不透明度，0.0-1.0）：各套均固定 0.50。
    pub GLASS_ALPHA: f32,
}

// ============================================================
// 共享 token 层（所有皮肤一致的产品视觉语言）
//
// **为什么抽**：2026-10-23 加两套皮肤时实测——按钮色、语义状态色、徽章色、
// 图标粉彩、选中行、玻璃透明度，在已有两套里**一字不差重复**。这些是"产品统一
// 视觉语言"（绿=启用、红=错误、图标粉彩是产品识别色），不该随皮肤气质漂移。
// 抽成 `S_*` 常量后新增皮肤只需填**皮肤特有**的 11 个值，其余引用常量：
// 既省一半设计工作量，也从结构上保证"任何皮肤下绿都代表启用"。
//
// ⚠️ **为什么保留 Theme 的字段不删**：调用点有 136 处 `theme().C_XXX`，
// 若把共享字段移出 Theme 就得全改（风险高、收益低）。这里取"字段保留 +
// 值引用共享常量"——单一数据源的效果达到了，调用点零改动。
// ============================================================

/// 纯白（文字/玻璃混合目标/页签高亮底）。
const S_WHITE: Color = rgb(255, 255, 255);
/// 主文字（各套均为纯白，靠玻璃压暗保证对比）。
const S_ON_GRAD: Color = rgb(255, 255, 255);
/// 图标柔和粉彩（8 组，顺序见 `icons::HwIcon`）——产品统一识别色，不随皮肤变。
const S_ACCENT_SOFT: [Color; 8] = [
    rgb(115, 210, 242), // Os   柔天蓝
    rgb(234, 174, 144), // Board 柔桃
    rgb(188, 170, 249), // Cpu  柔薰衣草紫
    rgb(124, 210, 196), // Mem  柔薄荷
    rgb(234, 155, 185), // Gpu  柔粉
    rgb(233, 206, 123), // Disk 柔金
    rgb(149, 184, 249), // Nic  柔蓝
    rgb(230, 156, 212), // Audio 柔品红
];
/// 已启用卡片亮色描边 = 亮绿（"启用=绿"语义）。
const S_CARD_EDGE_ON: Color = rgb(52, 199, 89);
/// 纯色卡兜底底色 / 描边（死代码原语用）。
const S_CARD_BG: Color = rgb(250, 250, 253);
const S_BORDER: Color = rgb(217, 217, 217);
/// 选中行薄荷绿底 + 深绿字（跨皮肤一致，保证"选中"语义统一）。
const S_SEL_ROW: Color = rgb(216, 240, 228);
const S_SEL_ROW_TEXT: Color = rgb(28, 62, 44);
/// 按钮：品牌紫底 / hover / 白底 / 白底 hover / 白底深字。
const S_BTN: Color = rgb(79, 70, 229);
const S_BTN_H: Color = rgb(99, 91, 240);
const S_BTN_WHITE: Color = rgb(255, 255, 255);
const S_BTN_WHITE_H: Color = rgb(232, 228, 252);
const S_BTN_WHITE_TEXT: Color = rgb(40, 36, 70);
/// 标题栏关闭按钮 hover 红。
const S_TB_CLOSE_HOVER: Color = rgb(232, 17, 35);
/// 语义状态色：成功 / 预警 / 错误。
const S_SUCCESS: Color = rgb(52, 199, 89);
const S_WARNING: Color = rgb(250, 173, 20);
const S_ERROR: Color = rgb(255, 77, 79);
/// 徽章（已启用/未启用/警告）文字 + 背景。
const S_GREEN_DARK: Color = rgb(21, 128, 61);
const S_GREEN_BG: Color = rgb(223, 242, 230);
const S_GRAY_DARK: Color = rgb(89, 89, 89);
const S_GRAY_BG: Color = rgb(242, 242, 242);
const S_AMBER_DARK: Color = rgb(202, 110, 4);
const S_AMBER_BG: Color = rgb(254, 244, 225);
/// 玻璃透明度：各套固定 0.50（深底叠色，白字对比度 6.7~7.5:1）。
const S_GLASS_ALPHA: f32 = 0.50;

/// 皮肤总数（`set_theme` 钳制 / 随机取值 / 切换按钮取模都用它）。
pub const THEME_COUNT: u8 = 4;

/// 深空玻璃：紫蓝渐变 + 深 navy 玻璃（科技/电竞感）。
pub const DEEP_SPACE: Theme = Theme {
    NAME: "深空玻璃",
    C_GRAD_TOP: rgb(135, 81, 238),
    C_GRAD_BOT: rgb(38, 98, 235),
    C_FOOTER_BG: rgb(27, 54, 122),
    // 低饱和暗靛灰：既是窗口轮廓线，也是压 1-bit 圆角锯齿的暗边环。
    // 原亮薰衣草 (170,160,245) 在真机窗口圆角上锯齿炸眼，据 Z.W. 反馈压暗。
    C_WIN_BORDER: rgb(70, 66, 104),
    C_ON_GRAD: S_ON_GRAD,
    C_ON_GRAD_DIM: rgb(224, 216, 250),
    C_ON_GRAD_FAINT: rgb(198, 190, 240),
    C_WHITE: S_WHITE,
    C_GLASS_TINT: rgb(20, 24, 52),
    C_GLASS_DARK: rgb(64, 60, 96),
    C_ICON: rgb(214, 208, 248),
    C_ACCENT_SOFT: S_ACCENT_SOFT,
    C_CARD_EDGE_ON: S_CARD_EDGE_ON,
    C_CARD_BG: S_CARD_BG,
    C_BORDER: S_BORDER,
    C_SEL_ROW: S_SEL_ROW,
    C_SEL_ROW_TEXT: S_SEL_ROW_TEXT,
    C_BTN: S_BTN,
    C_BTN_H: S_BTN_H,
    C_BTN_WHITE: S_BTN_WHITE,
    C_BTN_WHITE_H: S_BTN_WHITE_H,
    C_BTN_WHITE_TEXT: S_BTN_WHITE_TEXT,
    C_TB_CLOSE_HOVER: S_TB_CLOSE_HOVER,
    C_SUCCESS: S_SUCCESS,
    C_WARNING: S_WARNING,
    C_ERROR: S_ERROR,
    C_GREEN_DARK: S_GREEN_DARK,
    C_GREEN_BG: S_GREEN_BG,
    C_GRAY_DARK: S_GRAY_DARK,
    C_GRAY_BG: S_GRAY_BG,
    C_AMBER_DARK: S_AMBER_DARK,
    C_AMBER_BG: S_AMBER_BG,
    CARD_RADIUS: 8,
    GLASS_ALPHA: S_GLASS_ALPHA,
};

/// 流畅灰紫：低饱和靛灰渐变 + Smoke 深色玻璃（安静专业，长时间盯屏不累）。
pub const FLUENT: Theme = Theme {
    NAME: "流畅灰紫",
    C_GRAD_TOP: rgb(56, 52, 84),
    C_GRAD_BOT: rgb(32, 32, 50),
    C_FOOTER_BG: rgb(25, 25, 40),
    // 低饱和中深灰：贴 1-bit 圆角边缘时不勾出锯齿台阶。
    C_WIN_BORDER: rgb(108, 102, 140),
    C_ON_GRAD: S_ON_GRAD,
    C_ON_GRAD_DIM: rgb(212, 212, 222),
    C_ON_GRAD_FAINT: rgb(182, 182, 198),
    C_WHITE: S_WHITE,
    C_GLASS_TINT: rgb(30, 30, 44),
    C_GLASS_DARK: rgb(44, 44, 62),
    C_ICON: rgb(204, 204, 218),
    C_ACCENT_SOFT: [
        rgb(115, 210, 242),
        rgb(234, 174, 144),
        rgb(188, 170, 249),
        rgb(124, 210, 196),
        rgb(234, 155, 185),
        rgb(233, 206, 123),
        rgb(149, 184, 249),
        rgb(230, 156, 212),
    ],
    C_CARD_EDGE_ON: S_CARD_EDGE_ON,
    C_CARD_BG: S_CARD_BG,
    C_BORDER: S_BORDER,
    C_SEL_ROW: S_SEL_ROW,
    C_SEL_ROW_TEXT: S_SEL_ROW_TEXT,
    C_BTN: S_BTN,
    C_BTN_H: S_BTN_H,
    C_BTN_WHITE: S_BTN_WHITE,
    C_BTN_WHITE_H: S_BTN_WHITE_H,
    C_BTN_WHITE_TEXT: S_BTN_WHITE_TEXT,
    C_TB_CLOSE_HOVER: S_TB_CLOSE_HOVER,
    C_SUCCESS: S_SUCCESS,
    C_WARNING: S_WARNING,
    C_ERROR: S_ERROR,
    C_GREEN_DARK: S_GREEN_DARK,
    C_GREEN_BG: S_GREEN_BG,
    C_GRAY_DARK: S_GRAY_DARK,
    C_GRAY_BG: S_GRAY_BG,
    C_AMBER_DARK: S_AMBER_DARK,
    C_AMBER_BG: S_AMBER_BG,
    CARD_RADIUS: 10,
    GLASS_ALPHA: S_GLASS_ALPHA,
};

/// 琥珀暖调：暖褐琥珀渐变 + 深褐玻璃（夜间维护护眼，Z.W. 2026-10-23 选定）。
///
/// ⚠️ `C_WIN_BORDER` 用**暗**褐 (96,68,40) 而非亮琥珀：窗形圆角由分层 alpha
/// 羽化，但羽化边缘外仍靠这圈暗边压对比——亮边在浅色桌面下会把锯齿勾出来
/// （深空玻璃踩过同一个坑，原亮薰衣草被压暗）。
pub const AMBER: Theme = Theme {
    NAME: "琥珀暖调",
    C_GRAD_TOP: rgb(132, 82, 40),
    C_GRAD_BOT: rgb(64, 40, 24),
    C_FOOTER_BG: rgb(44, 28, 16),
    C_WIN_BORDER: rgb(96, 68, 40),
    C_ON_GRAD: S_ON_GRAD,
    C_ON_GRAD_DIM: rgb(240, 226, 206),
    C_ON_GRAD_FAINT: rgb(206, 186, 158),
    C_WHITE: S_WHITE,
    C_GLASS_TINT: rgb(40, 26, 14),
    C_GLASS_DARK: rgb(72, 48, 28),
    C_ICON: rgb(245, 220, 190),
    C_ACCENT_SOFT: S_ACCENT_SOFT,
    C_CARD_EDGE_ON: S_CARD_EDGE_ON,
    C_CARD_BG: S_CARD_BG,
    C_BORDER: S_BORDER,
    C_SEL_ROW: S_SEL_ROW,
    C_SEL_ROW_TEXT: S_SEL_ROW_TEXT,
    C_BTN: S_BTN,
    C_BTN_H: S_BTN_H,
    C_BTN_WHITE: S_BTN_WHITE,
    C_BTN_WHITE_H: S_BTN_WHITE_H,
    C_BTN_WHITE_TEXT: S_BTN_WHITE_TEXT,
    C_TB_CLOSE_HOVER: S_TB_CLOSE_HOVER,
    C_SUCCESS: S_SUCCESS,
    C_WARNING: S_WARNING,
    C_ERROR: S_ERROR,
    C_GREEN_DARK: S_GREEN_DARK,
    C_GREEN_BG: S_GREEN_BG,
    C_GRAY_DARK: S_GRAY_DARK,
    C_GRAY_BG: S_GRAY_BG,
    C_AMBER_DARK: S_AMBER_DARK,
    C_AMBER_BG: S_AMBER_BG,
    CARD_RADIUS: 10,
    GLASS_ALPHA: S_GLASS_ALPHA,
};

/// 薄荷青竹：青绿渐变 + 深青玻璃（清新通透，Z.W. 2026-10-23 选定）。
pub const MINT: Theme = Theme {
    NAME: "薄荷青竹",
    C_GRAD_TOP: rgb(24, 118, 106),
    C_GRAD_BOT: rgb(12, 58, 60),
    C_FOOTER_BG: rgb(10, 44, 44),
    C_WIN_BORDER: rgb(44, 92, 88),
    C_ON_GRAD: S_ON_GRAD,
    C_ON_GRAD_DIM: rgb(210, 240, 234),
    C_ON_GRAD_FAINT: rgb(168, 206, 200),
    C_WHITE: S_WHITE,
    C_GLASS_TINT: rgb(12, 36, 36),
    C_GLASS_DARK: rgb(26, 66, 62),
    C_ICON: rgb(200, 240, 232),
    C_ACCENT_SOFT: S_ACCENT_SOFT,
    C_CARD_EDGE_ON: S_CARD_EDGE_ON,
    C_CARD_BG: S_CARD_BG,
    C_BORDER: S_BORDER,
    C_SEL_ROW: S_SEL_ROW,
    C_SEL_ROW_TEXT: S_SEL_ROW_TEXT,
    C_BTN: S_BTN,
    C_BTN_H: S_BTN_H,
    C_BTN_WHITE: S_BTN_WHITE,
    C_BTN_WHITE_H: S_BTN_WHITE_H,
    C_BTN_WHITE_TEXT: S_BTN_WHITE_TEXT,
    C_TB_CLOSE_HOVER: S_TB_CLOSE_HOVER,
    C_SUCCESS: S_SUCCESS,
    C_WARNING: S_WARNING,
    C_ERROR: S_ERROR,
    C_GREEN_DARK: S_GREEN_DARK,
    C_GREEN_BG: S_GREEN_BG,
    C_GRAY_DARK: S_GRAY_DARK,
    C_GRAY_BG: S_GRAY_BG,
    C_AMBER_DARK: S_AMBER_DARK,
    C_AMBER_BG: S_AMBER_BG,
    CARD_RADIUS: 10,
    GLASS_ALPHA: S_GLASS_ALPHA,
};

/// 当前主题索引：0 = 深空玻璃，1 = 流畅灰紫，2 = 琥珀暖调，3 = 薄荷青竹。
/// 初始值仅兜底——`load_theme()` 会覆盖（有偏好读偏好、无偏好随机）。
static CURRENT_THEME: AtomicU8 = AtomicU8::new(0);

/// 设置当前主题（越界钳到最后一档，防 `theme.txt` 被手改成无效值后取到野主题）。
pub fn set_theme(id: u8) {
    CURRENT_THEME.store(id.min(THEME_COUNT - 1), Ordering::Relaxed);
}

/// 当前主题索引（0..`THEME_COUNT`）。
pub fn theme_id() -> u8 {
    CURRENT_THEME.load(Ordering::Relaxed)
}

/// 当前主题的不可变引用（指向 `const` 实例，`'static`）。
///
/// N 路 `match`——LLVM 对密集整数匹配生成跳转表，与原来两分支开销同级
/// （实测 136 处调用点、每帧开销仍在微秒量级，与皮肤数量无关）。
#[inline]
pub fn theme() -> &'static Theme {
    match CURRENT_THEME.load(Ordering::Relaxed) {
        1 => &FLUENT,
        2 => &AMBER,
        3 => &MINT,
        _ => &DEEP_SPACE,
    }
}

// ============================================================
// 主题持久化（%APPDATA%/WinSuiteX/theme.txt）
// ============================================================

/// 启动时调用：
/// - 有 `theme.txt`（用户手动选过）→ 尊重用户选择；
/// - 无文件 / 解析失败 → **随机** 0 或 1（Z.W. 2026-10-22「随机皮肤，
///   让使用者有不同感受」），每次冷启动都可能不同，制造新鲜感。
pub fn load_theme() {
    if let Some(dir) = app_data_dir() {
        let p = dir.join("theme.txt");
        if let Ok(s) = std::fs::read_to_string(&p) {
            if let Ok(id) = s.trim().parse::<u8>() {
                set_theme(id);
                return;
            }
        }
    }
    // 无偏好：用纳秒时间戳做熵源，`% THEME_COUNT` 取档（零依赖，不引 rand crate）。
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    set_theme((nanos % u32::from(THEME_COUNT)) as u8);
}

/// 切换主题后调用：把当前选择写盘，重启仍生效。写盘失败静默忽略（内存切换仍有效）。
pub fn save_theme(id: u8) {
    if let Some(dir) = app_data_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("theme.txt"), id.to_string());
    }
}

/// `%APPDATA%/WinSuiteX`。
fn app_data_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|v| PathBuf::from(v).join("WinSuiteX"))
}

/// 取窗口渐变在屏幕 y（0..`h`）处的颜色。
///
/// 卡片要"透"出背景，就必须知道**自己所在位置**的背景是什么色——
/// 所以混合不能用一个固定值，必须按 y 现算。
pub fn grad_at(y: i32, h: i32) -> Color {
    let t = (y.clamp(0, h.max(1)) as f32 / h.max(1) as f32).clamp(0.0, 1.0);
    mix(theme().C_GRAD_TOP, theme().C_GRAD_BOT, t)
}

// ============================================================
// 几何（96-DPI 基准）—— 两套主题完全一致，保持 `const`
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
/// 分层窗口 per-pixel alpha 羽化**（2026-10-22 起，见 `draw::apply_round_alpha`）。
/// 常量保留为 0 是为了让 `body_pad()` 的所有引用点
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

/// 窗口圆角（96-DPI 基准；分层窗口 SDF 羽化用，见 `draw::apply_round_alpha`）。两套均为 12。
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
// 玻璃观感参数（已固化，不再提供调试滑杆）
// ============================================================
//
// 这两个参数原本是「调试」页实时滑杆的作用目标。Z.W. 试出满意值后，于
// 0.4.5 取消调试页、把值固化回 theme.rs：玻璃透明度 = 50%，叠色深浅 = 100%
// （深 navy tint 全实）。全部 tint 玻璃卡（硬件页大卡/头部/表格/详情卡/
// 分组横条）都读取它们，保持全窗口玻璃观感一致。

/// 取玻璃透明度（0.0-1.0，供绘制原语直接消费）。两套主题均固定 0.50。
pub fn glass_alpha() -> f32 {
    theme().GLASS_ALPHA
}

/// 当前 tint 混合色：深浅固定 100% = tint 全实（默认最暗）。
pub fn tint_color() -> Color {
    theme().C_GLASS_TINT
}
