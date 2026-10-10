//! GDI 绘制原语：双缓冲、圆角矩形、渐变、文字、图标瓦片、按钮、徽章。
//!
//! 全部基于 `windows` crate 的 GDI API（`Win32_Graphics_Gdi`），无外部依赖。
//!
//! ## 为什么用 GDI 而不是 D3D/ANGLE
//!
//! GDI 是 Windows 自带的 2D 渲染器，**零 GPU 依赖**：虚拟机、RDP 远程会话、
//! 基础显示驱动（网吧/电竞酒店常见环境）都能正常渲染，且内存占用与传统 Win32
//! 应用同级（十几 MB），单 exe 无需附带任何 dll。

// 本模块是**GDI 绘制原语库**：`text_multiline`（长描述自动换行）、`badge`（状态徽章）、
// `card`（悬浮阴影卡片）都已实现但当前页面还没用到——它们是第二阶段
//（右键增强页真开关 / 磁盘页详情卡）的取用对象。先把原语备齐，
// 避免到时候为了画一个徽章再去翻 GDI 文档。
#![allow(dead_code)]

use windows::core::PCWSTR;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::*;

use crate::theme::*;

// ============================================================
// 画笔 / 字体
// ============================================================

/// 申请一个纯色画笔（用 `CreateSolidBrush`）。调用方负责 `DeleteObject`。
pub fn solid(color: Color) -> HBRUSH {
    // SAFETY: COLORREF 无前置条件；失败返回空句柄，调用点用 `is_invalid` 防御。
    unsafe { CreateSolidBrush(color) }
}

/// 画笔句柄是否无效（GDI 返回 NULL 表示失败）。
pub fn is_invalid(b: HBRUSH) -> bool {
    b.is_invalid()
}

/// 创建一个 GDI 字体，带**回退链**（见 [`FONT_FALLBACKS`]）。
///
/// `size_h` 以逻辑单位给出（负值 = 字符高度），`weight` 用 `FW_*` 常量。
/// 调用方负责 `DeleteObject`。
///
/// ⚠️ 为什么需要回退（第二十七轮两连坑实测定案）：
/// 1. `FONT_UI` 曾多打一个 `0x4E` → 字体名"微软雅黑N"不存在 → GDI **静默替换**成
///    系统默认宋体，`CreateFontW` 不返回 NULL、程序毫无感知；
/// 2. 核对真名时，中文系统上 `GetTextFaceW` 返回的是**本地化名「微软雅黑」**而非
///    英文 "Microsoft YaHei"——只认英文名会把好字体也误判成"被替换"。
///
/// 因此逐候选试建，真名必须落在 [`FONT_GOOD_FACES`]（英文+本地化名都认）里才用，
/// 全部落空才退到系统默认。
pub fn font(family: &[u16], size_h: i32, weight: i32) -> HFONT {
    // 候选顺序：调用方首选 → 雅黑 → 雅黑 UI 变种 → Segoe UI → 空名(系统默认兜底)。
    // 空名交给 DEFAULT_CHARSET，GDI 选当前 locale 的 UI 默认字体（最后手段，宁宋体不崩）。
    let mut cands: Vec<Vec<u16>> = Vec::with_capacity(1 + FONT_FALLBACKS.len());
    cands.push(family.to_vec());
    for n in FONT_FALLBACKS {
        let mut v: Vec<u16> = n.encode_utf16().collect();
        v.push(0);
        cands.push(v);
    }
    for c in &cands {
        let f = create_font(c, size_h, weight);
        if f.is_invalid() {
            continue;
        }
        // 空名（系统默认兜底）不校验，直接接受；其余真名必须落在可接受集合里。
        if c.len() <= 1 || face_accepted(&f) {
            return f;
        }
        // 被替换了（真名不在集合里）→ 丢弃这个，试下一个。
        unsafe {
            let _ = DeleteObject(f.into());
        }
    }
    // 理论不可达（空名兜底必成功），但保底再给一次系统默认。
    create_font(&[0], size_h, weight)
}

/// 字体回退链的英文候选名（不含调用方传入的首选，首选由 `font` 自行拼接）。
///
/// "Microsoft YaHei UI" 是很多精简系统里唯一残留的雅黑变种，必须作为候选。
const FONT_FALLBACKS: &[&str] = &["Microsoft YaHei", "Microsoft YaHei UI", "Segoe UI", ""];

/// 可接受的"真用上"face 名集合：**英文 + 中文系统本地化名都要认**。
///
/// 实测（VM Win10 Pro zh-CN）：`CreateFontW("Microsoft YaHei")` 创建成功后
/// `GetTextFaceW` 返回的是「微软雅黑」——只比对英文名会把正确字体误杀。
/// pub 供字体回退测试复用同一份名单（两处不一致时测试红）。
pub const FONT_GOOD_FACES: &[&str] = &[
    "Microsoft YaHei",
    "微软雅黑",
    "Microsoft YaHei UI",
    "微软雅黑 UI",
    "Segoe UI",
];

/// 真正创建一次字体（不校验）。
fn create_font(family: &[u16], size_h: i32, weight: i32) -> HFONT {
    // SAFETY: CreateFontW 只读取入参，不持有指针；返回句柄由调用方 DeleteObject。
    unsafe {
        CreateFontW(
            size_h,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            PCWSTR(family.as_ptr()),
        )
    }
}

/// 字体真名是否落在 [`FONT_GOOD_FACES`]（未被 GDI 替换成宋体等默认字体）。
fn face_accepted(f: &HFONT) -> bool {
    match real_face(f) {
        Some(got) => FONT_GOOD_FACES
            .iter()
            .any(|g| got.to_lowercase() == g.to_lowercase()),
        None => false,
    }
}

/// 查字体真名（`GetTextFaceW`，返回 None = 查不到）。
///
/// 实测注意：
/// 1. 中文系统返回**本地化名**（如「微软雅黑」），不是英文名——见 [`FONT_GOOD_FACES`]；
/// 2. `GetTextFaceW` 的返回长度在某些 Windows 版本会**多算 1（把结尾 NUL 也算进去）**，
///    且 `buf` 是零初始化的，直接 `from_utf16_lossy(&buf[..n])` 会得到带结尾 NUL/空格的串，
///    破坏 `face_accepted` 的精确比对。这里统一 `trim` 掉首尾空白与 NUL 再返回。
fn real_face(f: &HFONT) -> Option<String> {
    unsafe {
        let dc = GetDC(None);
        if dc.is_invalid() {
            return None;
        }
        let old = SelectObject(dc, (*f).into());
        let mut buf = [0u16; 64];
        let n = GetTextFaceW(dc, Some(&mut buf[..]));
        let _ = SelectObject(dc, old);
        let _ = ReleaseDC(None, dc);
        if n == 0 {
            return None;
        }
        let raw = String::from_utf16_lossy(&buf[..(n as usize).min(buf.len())]);
        let face = raw.trim_matches('\u{0}').trim().to_string();
        if face.is_empty() {
            None
        } else {
            Some(face)
        }
    }
}

/// 字体句柄是否无效。
pub fn font_invalid(f: HFONT) -> bool {
    f.is_invalid()
}

/// 常用字体族（UTF-16，末尾 NUL 由 `w!`/字面量保证）。
///
/// ⚠️ 历史坑：这里曾多打一个 `0x4E`（字母 N）——「微软雅黑」是 4 个字，多出的
/// "N" 让字体名变成不存在的"微软雅黑N"，GDI **静默替换**成系统默认宋体，
/// 全部机器（VM + Z.W. 真机）从第一天起就在用宋体（第二十七轮用 GetTextFaceW
/// 实测定案）。名字必须逐字核对，错一个码点就是整盘宋体。
pub const FONT_UI: &[u16] = &[
    0x5FAE, 0x8F6F, 0x96C5, 0x9ED1, // 微软雅黑
    0x0000,
];

// ============================================================
// 矩形与填充
// ============================================================

/// 用给定画笔填充矩形。
pub fn fill(hdc: HDC, r: &RECT, color: Color) {
    let b = solid(color);
    if !is_invalid(b) {
        // SAFETY: hdc 由调用方保证有效；b 已创建且在本函数内使用。
        unsafe {
            let _ = FillRect(hdc, r, b);
            let _ = DeleteObject(b.into());
        }
    }
}

/// 圆角填充（GDI 无原生圆角矩形 API，用 `RoundRect` + 椭圆刷）。
pub fn fill_round(hdc: HDC, r: &RECT, radius: i32, color: Color) {
    let w = r.right - r.left;
    let h = r.bottom - r.top;
    if w <= 0 || h <= 0 {
        return;
    }
    // RoundRect 的椭圆宽高是"直径"，要传 radius*2。
    let d = (radius * 2).max(1);
    let b = solid(color);
    if is_invalid(b) {
        return;
    }
    // SAFETY: hdc 有效；b 已创建。RoundRect 用当前 DC 的画笔与填充色，
    // 这里临时把 b 选入 DC，绘制后恢复原画笔。
    unsafe {
        let old = SelectObject(hdc, b.into());
        let _ = RoundRect(hdc, r.left, r.top, r.right, r.bottom, d, d);
        let _ = SelectObject(hdc, old);
        let _ = DeleteObject(b.into());
    }
}

/// 圆角描边（不填充）。
pub fn stroke_round(hdc: HDC, r: &RECT, radius: i32, color: Color, width: i32) {
    let d = (radius * 2).max(1);
    // SAFETY: CreatePen 无前置条件，失败返回空句柄（下面立即判掉）。
    let pen = unsafe { CreatePen(PS_SOLID, width.max(1), color) };
    if pen.is_invalid() {
        return;
    }
    // SAFETY: 同 fill_round；SelectObject 成对恢复，避免 DC 残留错误句柄。
    unsafe {
        let old = SelectObject(hdc, pen.into());
        let null = GetStockObject(NULL_BRUSH);
        let oldb = SelectObject(hdc, null);
        let _ = RoundRect(hdc, r.left, r.top, r.right, r.bottom, d, d);
        let _ = SelectObject(hdc, oldb);
        let _ = SelectObject(hdc, old);
        let _ = DeleteObject(pen.into());
    }
}

/// 抗锯齿圆角填充：以 `SS` 倍超采样画进离屏 DIB（背景填 `bg`、前景画 `fg` 圆角），
/// 再用 `HALFTONE` 平滑缩回目标 DC。
///
/// GDI 原生 `RoundRect` 无抗锯齿，高分屏下开关圆角会发虚（这不是 DPI 虚拟化，
/// 是 GDI 本身硬锯齿）。超采样 + 区域平均缩放让边缘变成自然过渡：
/// 旋钮画在轨道上，所以 `bg` 传轨道色，圆角外的背景像素与真实轨道融为一片，
/// 看不到方块角。这是"开关还是糊、不够细腻"的治本修复，且纯 GDI、无 GDI+ 依赖。
pub fn aa_round(hdc: HDC, r: &RECT, radius: i32, fg: Color, bg: Color) {
    let w = r.right - r.left;
    let h = r.bottom - r.top;
    if w <= 0 || h <= 0 {
        return;
    }
    let ss = 3i32;
    let tw = w * ss;
    let th = h * ss;
    // SAFETY: 离屏 DC / DIB 在本函数内创建、选入、绘制、释放；无跨函数句柄泄漏。
    unsafe {
        let screen = GetDC(None);
        let tmp = CreateCompatibleDC(Some(screen));
        let _ = ReleaseDC(None, screen);
        if tmp.is_invalid() {
            return;
        }
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: tw,
                biHeight: -th,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = core::ptr::null_mut();
        let bmp = match CreateDIBSection(Some(tmp), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(b) => b,
            Err(_) => {
                let _ = DeleteDC(tmp);
                return;
            }
        };
        let old = SelectObject(tmp, bmp.into());
        // 背景（圆角外区域；缩放后与真实背景融为一片，边缘过渡自然）。
        let bbg = CreateSolidBrush(bg);
        let _ = FillRect(
            tmp,
            &RECT {
                left: 0,
                top: 0,
                right: tw,
                bottom: th,
            },
            bbg,
        );
        let _ = DeleteObject(bbg.into());
        // 前景圆角：选 NULL_PEN 避免 RoundRect 描出 1px 黑边。
        let bfg = CreateSolidBrush(fg);
        let oldb = SelectObject(tmp, bfg.into());
        let npen = GetStockObject(NULL_PEN);
        let oldp = SelectObject(tmp, npen);
        let _ = RoundRect(tmp, 0, 0, tw, th, radius * ss * 2, radius * ss * 2);
        let _ = SelectObject(tmp, oldp);
        let _ = SelectObject(tmp, oldb);
        let _ = DeleteObject(bfg.into());
        // 平滑缩放回目标矩形（HALFTONE = 区域平均，得到抗锯齿边缘）。
        // 保存/恢复拉伸模式，避免影响同帧其它绘制。
        let prev = SetStretchBltMode(hdc, HALFTONE);
        let _ = StretchBlt(hdc, r.left, r.top, w, h, Some(tmp), 0, 0, tw, th, SRCCOPY);
        let _ = SetStretchBltMode(hdc, STRETCH_BLT_MODE(prev));
        let _ = SelectObject(tmp, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(tmp);
    }
}

/// 竖直线性渐变填充（顶栏用）。`c_top` → `c_bot`。
///
/// GDI 的渐变需 `GradientFill`（msimg32）或手工逐行刷。后者兼容性最好、
/// 无需额外 DLL，用 `brushtool` + 逐行 `FillRect` 实现。
pub fn fill_gradient_v(hdc: HDC, r: &RECT, c_top: Color, c_bot: Color) {
    let h = (r.bottom - r.top).max(1);
    let w = r.right - r.left;
    for y in 0..h {
        let t = y as f32 / (h.saturating_sub(1).max(1)) as f32;
        let c = lerp(c_top, c_bot, t);
        let b = solid(c);
        if !is_invalid(b) {
            let band = RECT {
                left: r.left,
                top: r.top + y,
                right: r.left + w,
                bottom: r.top + y + 1,
            };
            // SAFETY: hdc 有效；b 已创建并在本次调用内使用。
            unsafe {
                let _ = FillRect(hdc, &band, b);
                let _ = DeleteObject(b.into());
            }
        }
    }
}

/// **玻璃填充**：逐行取该处窗口渐变色，再叠一层 `over_c`（`alpha` 不透明度）。
///
/// 为什么不直接算一个固定色填整块：窗口底色是**随 y 变化的渐变**，
/// 卡片若用单一混合色，长卡片（硬件页那张能占 800px）上下端会和背景对不上，
/// 露出"贴上去的色块"感。GDI 无 alpha 通道，所以只能逐行混——
/// 每行取 `grad_at(屏幕 y)` 再混，观感等效于 CSS 的 `rgba(255,255,255,.18)`。
///
/// `scroll` 是当前滚动偏移：页面绘制在 `OffsetViewportOrgEx(0, -scroll)` 的
/// 视口里，入参矩形是**内容坐标**，要换算回屏幕坐标才能取到正确的渐变色。
pub fn fill_glass(hdc: HDC, r: &RECT, over_c: Color, alpha: f32, scroll: i32, win_h: i32) {
    let h = r.bottom - r.top;
    if h <= 0 || r.right <= r.left {
        return;
    }
    for y in 0..h {
        let base = grad_at(r.top + y - scroll, win_h);
        let c = over(base, over_c, alpha);
        fill(
            hdc,
            &RECT {
                left: r.left,
                top: r.top + y,
                right: r.right,
                bottom: r.top + y + 1,
            },
            c,
        );
    }
}

/// 玻璃填充的**圆角**版：先用圆角 Region 裁剪，再逐行混合。
///
/// 圆角裁剪是必须的——否则四个直角会把卡片的圆角"戳"出来。
/// `SaveDC`/`RestoreDC` 成对包住裁剪区，避免污染后续绘制（底栏、滚动条）。
pub fn fill_glass_round(
    hdc: HDC,
    r: &RECT,
    radius: i32,
    over_c: Color,
    alpha: f32,
    scroll: i32,
    win_h: i32,
) {
    let w = r.right - r.left;
    let h = r.bottom - r.top;
    if w <= 0 || h <= 0 {
        return;
    }
    // SAFETY: Region 在本函数内创建/选中/释放；SaveDC+RestoreDC 保证 DC 状态复原。
    unsafe {
        let saved = SaveDC(hdc);
        // ⚠️ 圆角裁剪区是**设备坐标**，`SelectClipRgn` 不会随 `OffsetViewportOrgEx`
        // 的视口偏移而移动；但 `fill` 的 `FillRect` 会。两者必须落在同一屏幕位置，
        // 否则滚动后（scroll>0）填充整条被错位的裁剪区裁掉——硬件页分组横条只有
        // SEC_H=30 高，scroll≥30 时背景全没（第十九轮：真机"拉到底横框底色消失"）。
        // 卡片很高错位只丢顶/底一截、被固定头部盖住不明显，故只有小横条中招。
        // 视口只沿 Y 偏移，故只减 scroll。
        //
        // ⚠️ 必须用 `ExtSelectClipRgn(RGN_AND)` **相交**、不能用 `SelectClipRgn`
        // （默认 RGN_COPY **整块替换**裁剪区）：替换会把 on_paint 里
        // `IntersectClipRect` 设的可视区限制整个逃掉——滚出可视区的分组横条
        // 照样能画到可视区之外（真机实测：横条残留画在标题栏与页签之间，成了
        // 一条空玻璃带，图标/文字在 clip 恢复后又被可视区裁掉，只剩空条）。
        // 相交后：可视区内的 band 照常画（第十九轮修复保持），滚出去的被裁掉。
        let hrgn = CreateRoundRectRgn(
            r.left,
            r.top - scroll,
            r.right,
            r.bottom - scroll,
            radius * 2,
            radius * 2,
        );
        if !hrgn.is_invalid() {
            let _ = ExtSelectClipRgn(hdc, Some(hrgn), RGN_AND);
        }
        fill_glass(hdc, r, over_c, alpha, scroll, win_h);
        let _ = RestoreDC(hdc, saved);
        if !hrgn.is_invalid() {
            let _ = DeleteObject(hrgn.into());
        }
    }
}

/// 颜色线性插值（BGR 分量各自插）。
fn lerp(a: Color, b: Color, t: f32) -> Color {
    let (ar, ag, ab) = split(a);
    let (br, bg, bb) = split(b);
    let mix = |x: u8, y: u8| -> u8 {
        (x as f32 + (y as f32 - x as f32) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    rgb(mix(ar, br), mix(ag, bg), mix(ab, bb))
}

/// 拆出 COLORREF 的 r/g/b 分量（Windows 内部是 0x00BBGGRR）。
fn split(c: Color) -> (u8, u8, u8) {
    let v = c.0;
    (
        (v & 0xFF) as u8,
        ((v >> 8) & 0xFF) as u8,
        ((v >> 16) & 0xFF) as u8,
    )
}

// ============================================================
// 文字
// ============================================================

/// 绘制单行文字（左对齐 / 垂直居中）。
pub fn text(hdc: HDC, r: &RECT, s: &str, font: HFONT, color: Color, align_right: bool) {
    if s.is_empty() {
        return;
    }
    // SAFETY: hdc 与 font 均由调用方保证有效；SelectObject 成对恢复。
    // windows 0.62：DrawTextW 只有 4 个参数（无 hdc 之外的额外参数），
    // 格式位是 typed enum DRAW_TEXT_FORMAT，需按位或组合。
    unsafe {
        let oldf = SelectObject(hdc, font.into());
        let oldc = SetTextColor(hdc, color);
        let oldb = SetBkMode(hdc, TRANSPARENT);
        let mut wide: Vec<u16> = s.encode_utf16().collect();
        wide.push(0);
        let mut flags = DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX;
        if align_right {
            flags |= DT_RIGHT;
        } else {
            flags |= DT_LEFT;
        }
        let mut rect = *r;
        let _ = DrawTextW(hdc, &mut wide[..], &mut rect, flags);
        let _ = SetBkMode(hdc, BACKGROUND_MODE(oldb as u32));
        let _ = SetTextColor(hdc, oldc);
        let _ = SelectObject(hdc, oldf);
    }
}

/// 水平居中的单行文字（底部版权行等需要居中的场景）。
pub fn text_center(hdc: HDC, r: &RECT, s: &str, font: HFONT, color: Color) {
    if s.is_empty() {
        return;
    }
    // SAFETY: 同 text；DT_CENTER 让文字在给定矩形内水平居中。
    unsafe {
        let oldf = SelectObject(hdc, font.into());
        let oldc = SetTextColor(hdc, color);
        let oldb = SetBkMode(hdc, TRANSPARENT);
        let mut wide: Vec<u16> = s.encode_utf16().collect();
        wide.push(0);
        let flags = DT_VCENTER | DT_SINGLELINE | DT_CENTER | DT_NOPREFIX;
        let mut rect = *r;
        let _ = DrawTextW(hdc, &mut wide[..], &mut rect, flags);
        let _ = SetBkMode(hdc, BACKGROUND_MODE(oldb as u32));
        let _ = SetTextColor(hdc, oldc);
        let _ = SelectObject(hdc, oldf);
    }
}

/// 绘制多行文字（左对齐 / 顶端对齐，自动换行）。
pub fn text_multiline(hdc: HDC, r: &RECT, s: &str, font: HFONT, color: Color) {
    if s.is_empty() {
        return;
    }
    // SAFETY: 同 text；DT_WORDBREAK 让长描述在卡片内自动换行。
    unsafe {
        let oldf = SelectObject(hdc, font.into());
        let oldc = SetTextColor(hdc, color);
        let oldb = SetBkMode(hdc, TRANSPARENT);
        let mut wide: Vec<u16> = s.encode_utf16().collect();
        wide.push(0);
        let mut rect = *r;
        let flags = DT_TOP | DT_LEFT | DT_WORDBREAK | DT_NOPREFIX;
        let _ = DrawTextW(hdc, &mut wide[..], &mut rect, flags);
        let _ = SetBkMode(hdc, BACKGROUND_MODE(oldb as u32));
        let _ = SetTextColor(hdc, oldc);
        let _ = SelectObject(hdc, oldf);
    }
}

// ============================================================
// 复合控件
// ============================================================

/// 主色（靛蓝）/ 次色（白底）按钮。命中检测在调用方。
///
/// 配色取自 C++ 版实测：主色按钮 `(79,70,229)` 白字，次色按钮纯白底 + 深色字
/// （「导出报告」「一键复制」都是后者）。玻璃主题下按钮**不再描边**——
/// 白底本身在紫渐变上已经有足够对比度，再加灰边反而脏。
pub fn button(hdc: HDC, r: &RECT, label: &str, primary: bool, hovered: bool, font: HFONT) {
    let (bg, fg) = if primary {
        (if hovered { C_BTN_H } else { C_BTN }, C_ON_GRAD)
    } else {
        (
            if hovered { C_BTN_WHITE_H } else { C_BTN_WHITE },
            C_BTN_WHITE_TEXT,
        )
    };
    fill_round(hdc, r, 6, bg);
    text_center(hdc, r, label, font, fg);
}

/// 状态徽章（已启用 / 未启用 / 警告）。
pub fn badge(hdc: HDC, r: &RECT, label: &str, kind: BadgeKind, font: HFONT) {
    let (fg, bg) = match kind {
        BadgeKind::On => (C_GREEN_DARK, C_GREEN_BG),
        BadgeKind::Off => (C_GRAY_DARK, C_GRAY_BG),
        BadgeKind::Warn => (C_AMBER_DARK, C_AMBER_BG),
    };
    fill_round(hdc, r, scale(10), bg);
    text(hdc, r, label, font, fg, true);
}

/// 徽章语义。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BadgeKind {
    /// 已启用 / 正常。
    On,
    /// 未启用 / 中性。
    Off,
    /// 警告 / 降级。
    Warn,
}

/// 图标瓦片：主色圆角方块 + 白色字母（C++ 版的彩色瓦片同思路）。
pub fn icon_tile(hdc: HDC, r: &RECT, letter: &str, color: Color, font: HFONT) {
    fill_round(hdc, r, scale(6), color);
    text(hdc, r, letter, font, C_ON_GRAD, true);
}

/// 卡片（玻璃底 + 细描边）。
///
/// **阴影已彻底移除**（Z.W. 真机反馈"多了一层底"）。玻璃主题下卡片靠
/// "比背景亮一层"来分层，不靠阴影——GDI 没有 alpha 通道做不出真阴影，
/// 硬画只会变成一圈灰边。
pub fn card(hdc: HDC, r: &RECT) {
    fill_round(hdc, r, WIN_RADIUS, C_CARD_BG);
    stroke_round(hdc, r, WIN_RADIUS, C_BORDER, 1);
}

// ============================================================
// 双缓冲
// ============================================================

/// 一块离屏位图 + 其内存 DC，用于先画后 blit（消除闪烁）。
pub struct Backbuffer {
    /// 位图句柄。
    pub bmp: HBITMAP,
    /// 内存 DC（绘制目标）。
    pub mem: HDC,
    /// 旧位图句柄（BitBlt 后恢复）。
    ///
    /// 注意类型是 `HGDIOBJ` 而不是 `HANDLE`：`SelectObject` 在 windows 0.62 里
    /// 返回 `HGDIOBJ`，两者不能互换（`DeleteObject` 才收 `HGDIOBJ`）。
    pub old_bmp: HGDIOBJ,
    /// DIB 像素缓冲区首地址（`CreateDIBSection` 返回，由系统分配、随位图释放）。
    ///
    /// 有了它才能直接读像素——"完整长图"截图要把离屏内容编码成 PNG，
    /// 不必再走 `GetDIBits` 倒腾一遍（第二十二轮）。
    pub bits: *mut core::ffi::c_void,
    /// 像素宽 / 高。
    pub w: i32,
    pub h: i32,
}

impl Backbuffer {
    /// 按给定宽高创建离屏位图（DIB 32bpp，随窗口尺寸每帧重建）。
    ///
    /// 每帧重建是有意的权衡：DIB 分配/释放只占每帧几十微秒，换来的是
    /// "resize 后绝不出现拉伸模糊"——比缓存一张旧尺寸位图更符合观感预期。
    pub fn new(w: i32, h: i32) -> Option<Self> {
        // SAFETY: 传 None 表示用屏幕 DC 作兼容源，窗口 DC 的格式与之兼容。
        unsafe {
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            let _ = ReleaseDC(None, screen);
            if mem.is_invalid() {
                return None;
            }
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h, // 负高度 = 自上而下
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut core::ffi::c_void = core::ptr::null_mut();
            // 0.62 起 CreateDIBSection 返回 Result，失败直接返回 None 由调用方跳过该帧。
            let bmp = match CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(b) => b,
                Err(_) => {
                    let _ = DeleteDC(mem);
                    return None;
                }
            };
            let old = SelectObject(mem, bmp.into());
            Some(Self {
                bmp,
                mem,
                old_bmp: old,
                bits,
                w,
                h,
            })
        }
    }

    /// 把离屏内容贴到窗口 DC。
    pub fn present(&self, hdc: HDC) {
        // SAFETY: 前后端尺寸一致时逐像素 blit 正确；两端 DC 均有效。
        unsafe {
            let _ = BitBlt(hdc, 0, 0, self.w, self.h, Some(self.mem), 0, 0, SRCCOPY);
        }
    }

    /// 直接读位图像素（BGRA，自上而下，行字节数 = `w * 4`）。
    ///
    /// 位图是自上而下 DIB（`biHeight` 为负），首行即顶行，与 GDI+ 的
    /// `PixelFormat32bppARGB` 内存序一致，可直接交给编码器。
    ///
    /// # SAFETY
    /// `bits` 由 `CreateDIBSection` 分配且在本对象存活期内有效；
    /// 调用方不得在 `Backbuffer` 释放后使用返回的切片。
    pub unsafe fn pixels(&self) -> Option<&[u8]> {
        if self.bits.is_null() || self.w <= 0 || self.h <= 0 {
            return None;
        }
        Some(std::slice::from_raw_parts(
            self.bits as *const u8,
            (self.w * self.h * 4) as usize,
        ))
    }
}

impl Drop for Backbuffer {
    fn drop(&mut self) {
        // SAFETY: 成对释放——先恢复 DC 原位图，再删位图与 DC。
        unsafe {
            let _ = SelectObject(self.mem, self.old_bmp);
            let _ = DeleteObject(self.bmp.into());
            let _ = DeleteDC(self.mem);
        }
    }
}
