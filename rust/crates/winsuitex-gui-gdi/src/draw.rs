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
use windows::Win32::Foundation::{POINT, RECT};
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
///
/// 默认走 3x 超采样抗锯齿路径（见 [`AaSurface`]）；小圆角/小尺寸/离屏失败
/// 时自动降级为直接 `RoundRect`（过渡带 <1px 时 AA 无意义，省开销）。
pub fn fill_round(hdc: HDC, r: &RECT, radius: i32, color: Color) {
    let w = r.right - r.left;
    let h = r.bottom - r.top;
    if w <= 0 || h <= 0 {
        return;
    }
    // RoundRect 的椭圆宽高是"直径"，要传 radius*2。
    if aa_worthwhile(r, radius) {
        if let Some(s) = AaSurface::new(hdc, r) {
            let d = (radius * AA_SS * 2).max(1);
            let b = solid(color);
            if !is_invalid(b) {
                // SAFETY: tmp 离屏 DC 内 SelectObject/RoundRect 成对恢复；b 用后即删。
                unsafe {
                    let old = SelectObject(s.tmp, b.into());
                    let npen = GetStockObject(NULL_PEN);
                    let oldp = SelectObject(s.tmp, npen);
                    let _ = RoundRect(s.tmp, 0, 0, s.tw, s.th, d, d);
                    let _ = SelectObject(s.tmp, oldp);
                    let _ = SelectObject(s.tmp, old);
                    let _ = DeleteObject(b.into());
                }
            }
            s.finish(hdc, r);
            return;
        }
    }
    fill_round_raw(hdc, r, radius, color);
}

/// 圆角填充的**硬边**版本（不走 AA 超采样）：用于小尺寸高对比控件
/// （开关旋钮 / 轨道）。AA 的 1px 灰度过渡在这种控件上反而显得"发虚 / 羽化"，
/// 硬边更利落。窗形圆角（`SetWindowRgn`）与卡片大圆角不在此列。
pub fn fill_round_crisp(hdc: HDC, r: &RECT, radius: i32, color: Color) {
    fill_round_raw(hdc, r, radius, color);
}

/// `fill_round` 的原路径（直接 RoundRect，无 AA）：小尺寸/降级时走这里。
fn fill_round_raw(hdc: HDC, r: &RECT, radius: i32, color: Color) {
    let d = (radius * 2).max(1);
    let b = solid(color);
    if is_invalid(b) {
        return;
    }
    // SAFETY: hdc 有效；b 已创建。RoundRect 用当前 DC 的画笔与填充色，
    // 这里临时把 b 选入 DC，绘制后恢复原画笔。
    unsafe {
        let old = SelectObject(hdc, b.into());
        // ⚠️ 必须选 NULL_PEN：RoundRect 会同时用当前画笔描轮廓，
        // 不选就会把 DC 里残留的画笔（常是深色）画成一圈黑边。
        let npen = GetStockObject(NULL_PEN);
        let oldp = SelectObject(hdc, npen);
        let _ = RoundRect(hdc, r.left, r.top, r.right, r.bottom, d, d);
        let _ = SelectObject(hdc, oldp);
        let _ = SelectObject(hdc, old);
        let _ = DeleteObject(b.into());
    }
}

/// 圆角描边（不填充）。默认走 3x 超采样 AA；小尺寸/失败降级直接 RoundRect。
pub fn stroke_round(hdc: HDC, r: &RECT, radius: i32, color: Color, width: i32) {
    let w = r.right - r.left;
    let h = r.bottom - r.top;
    if w <= 0 || h <= 0 {
        return;
    }
    if aa_worthwhile(r, radius) {
        if let Some(s) = AaSurface::new(hdc, r) {
            let d = (radius * AA_SS * 2).max(1);
            // SAFETY: 离屏 DC 内 CreatePen/RoundRect 成对恢复；pen 用后即删。
            unsafe {
                let pen = CreatePen(PS_SOLID, (width.max(1)) * AA_SS, color);
                if !pen.is_invalid() {
                    let old = SelectObject(s.tmp, pen.into());
                    let null = GetStockObject(NULL_BRUSH);
                    let oldb = SelectObject(s.tmp, null);
                    let _ = RoundRect(s.tmp, 0, 0, s.tw, s.th, d, d);
                    let _ = SelectObject(s.tmp, oldb);
                    let _ = SelectObject(s.tmp, old);
                    let _ = DeleteObject(pen.into());
                }
            }
            s.finish(hdc, r);
            return;
        }
    }
    stroke_round_raw(hdc, r, radius, color, width);
}

/// `stroke_round` 的原路径（直接 RoundRect 描边，无 AA）。
fn stroke_round_raw(hdc: HDC, r: &RECT, radius: i32, color: Color, width: i32) {
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

// ============================================================
// 抗锯齿圆角基础设施（3x 超采样 + HALFTONE 缩回）
//
// 背景：GDI 的 `RoundRect` 是 1-bit 硬边，圆角弧在视觉上是阶梯锯齿
// （Z.W. 真机反馈"圆角模糊/发毛"的根源之一；VM 100% 与高分屏均可见）。
// 治本手段是 GDI+/D2D，但主线坚持零依赖单 exe——退而求其次用**超采样**：
// 在 3x 离屏上画，`StretchBlt(HALFTONE)` 缩回时区域平均自动生成边缘
// 灰度过渡，肉眼即平滑。纯 GDI、无新 DLL、无 GPU 依赖（RDP/VM 照常）。
//
// ⚠️ 窗口形状圆角（`SetWindowRgn`）**不在本机制射程内**——region 是二值
// 形状掩码不是绘制内容，超采样无解；那条路只有 DWM 圆角（Win11）或分层
// 窗口（per-pixel alpha）才是真 AA，已评估记录在案（2026-10-21 日志）。
// 这里治的是**绘制出来的**圆角：卡片、按钮、选中行、滑块轨道等。
// ============================================================

/// 超采样倍率：3x 是质量/开销平衡点（2x 过渡生硬，4x 内存耗时翻倍）。
const AA_SS: i32 = 3;
/// 离屏位图像素上限（约 24MB @32bpp）：超过则降为 2x，仍超则放弃 AA。
/// 防硬件页整页大卡（约 600×800）3x 分配拖垮帧时间。
const AA_MAX_PIXELS: i64 = 24_000_000;
/// 小于该尺寸或圆角小于 3px 时 AA 无意义（过渡带不足 1px），直接原路径。
const AA_MIN_SPAN: i32 = 16;

/// 该次绘制是否值得走 AA 路径（尺寸/圆角太小则过渡带 <1px，肉眼无差）。
fn aa_worthwhile(r: &RECT, radius: i32) -> bool {
    radius >= 3 && (r.right - r.left) >= AA_MIN_SPAN && (r.bottom - r.top) >= AA_MIN_SPAN
}

/// 抗锯齿离屏面：`AA_SS` 倍尺寸的 DIB，创建时先把目标区域**当前背景**
/// 放大抠入——圆角外区域因此与真实背景连续，缩回后看不出拼接边界。
/// 调用方在 `tmp` 上画前景（圆角形），`finish` 用 HALFTONE 缩回目标 DC。
struct AaSurface {
    tmp: HDC,
    bmp: HBITMAP,
    old_bmp: HGDIOBJ,
    tw: i32,
    th: i32,
}

impl AaSurface {
    /// 为 `r`（**内容坐标**，可能处于滚动视口）建离屏并抠入背景。
    /// 任一步失败返回 `None`（调用方降级原路径，保证任何环境不劣化）。
    fn new(hdc: HDC, r: &RECT) -> Option<Self> {
        let w = r.right - r.left;
        let h = r.bottom - r.top;
        if w <= 0 || h <= 0 {
            return None;
        }
        // 视口偏移自适应：on_paint 用 OffsetViewportOrgEx(0,-scroll) 滚动，
        // r 是内容坐标；读目标 DC 当前像素必须换算到设备坐标——
        // GetViewportOrgEx 拿到的原点即补偿量（无需调用方传 scroll）。
        let mut org = POINT { x: 0, y: 0 };
        let (sx, sy) = unsafe {
            if GetViewportOrgEx(hdc, &mut org).as_bool() {
                (r.left + org.x, r.top + org.y)
            } else {
                (r.left, r.top)
            }
        };
        let mut ss = AA_SS;
        if (w as i64 * h as i64) > AA_MAX_PIXELS / (AA_SS as i64 * AA_SS as i64) {
            ss = 2; // 巨型卡降倍率（600×800 → 1200×1600 = 7.7MB）
        }
        if (w as i64 * ss as i64 * h as i64 * ss as i64) > AA_MAX_PIXELS {
            return None; // 2x 仍超限（极端尺寸），放弃 AA
        }
        let tw = w * ss;
        let th = h * ss;
        // SAFETY: 离屏 DC/DIB 在本结构内创建，finish 里成对释放。
        unsafe {
            let screen = GetDC(None);
            let tmp = CreateCompatibleDC(Some(screen));
            let _ = ReleaseDC(None, screen);
            if tmp.is_invalid() {
                return None;
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
                    return None;
                }
            };
            let old_bmp = SelectObject(tmp, bmp.into());
            // 把目标区当前背景放大抠入（HALFTONE 平滑放大；渐变低频无可见损失）。
            // 失败（返回 0，如个别远程会话禁屏幕读取）→ 整体放弃 AA 走原路径。
            let prev = SetStretchBltMode(tmp, HALFTONE);
            let ok = StretchBlt(tmp, 0, 0, tw, th, Some(hdc), sx, sy, w, h, SRCCOPY);
            let _ = SetStretchBltMode(tmp, STRETCH_BLT_MODE(prev));
            if !ok.as_bool() {
                let _ = SelectObject(tmp, old_bmp);
                let _ = DeleteObject(bmp.into());
                let _ = DeleteDC(tmp);
                return None;
            }
            Some(Self {
                tmp,
                bmp,
                old_bmp,
                tw,
                th,
            })
        }
    }

    /// HALFTONE 缩回目标 DC（设备坐标同 `new` 的换算）并释放全部 GDI 资源。
    fn finish(self, hdc: HDC, r: &RECT) {
        let w = r.right - r.left;
        let h = r.bottom - r.top;
        // SAFETY: 成员句柄均由 new 创建；这里成对释放，无跨调用泄漏。
        unsafe {
            let mut org = POINT { x: 0, y: 0 };
            let (dx, dy) = if GetViewportOrgEx(hdc, &mut org).as_bool() {
                (r.left + org.x, r.top + org.y)
            } else {
                (r.left, r.top)
            };
            // 保存/恢复拉伸模式，避免影响同帧其它绘制（HALFTONE 是 DC 级状态）。
            let prev = SetStretchBltMode(hdc, HALFTONE);
            let _ = StretchBlt(
                hdc,
                dx,
                dy,
                w,
                h,
                Some(self.tmp),
                0,
                0,
                self.tw,
                self.th,
                SRCCOPY,
            );
            let _ = SetStretchBltMode(hdc, STRETCH_BLT_MODE(prev));
            let _ = SelectObject(self.tmp, self.old_bmp);
            let _ = DeleteObject(self.bmp.into());
            let _ = DeleteDC(self.tmp);
        }
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

/// 玻璃填充的**圆角**版：默认走 3x 超采样 AA 路径（圆角边缘灰度过渡，
/// 治 `RoundRect` 1-bit 硬边锯齿）；小尺寸/离屏失败自动降级为
/// "圆角 Region 裁剪 + 逐行混合"原路径（行为与旧版完全一致）。
///
/// AA 路径与原路径的关键差异：原路径在**窗口 DC** 上 `ExtSelectClipRgn
/// (RGN_AND)`（铁律——必须相交，否则替换会逃掉 on_paint 的可视区裁剪）；
/// AA 路径在**全新离屏 DC** 上裁剪，无外部裁剪需要保护，用
/// `SelectClipRgn`（RGN_COPY）即可，最后整块 HALFTONE 缩回窗口 DC。
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
    if aa_worthwhile(r, radius) {
        if let Some(s) = AaSurface::new(hdc, r) {
            // SAFETY: 离屏 DC 内 region/SelectClipRgn 成对恢复；逐行 fill 用
            // 已封装的 `fill`（内部自建自删刷子）。
            unsafe {
                // 圆角裁剪（离屏坐标 0,0,tw,th；直径随超采样放大）。
                let hrgn =
                    CreateRoundRectRgn(0, 0, s.tw, s.th, radius * AA_SS * 2, radius * AA_SS * 2);
                if !hrgn.is_invalid() {
                    let _ = SelectClipRgn(s.tmp, Some(hrgn));
                }
                // 逐行玻璃：每 3px 一带（= 目标 1 行）。窗口底色渐变是低频的，
                // 3px 同色经 HALFTONE 缩回后无可见色阶；行数降为 1/3，
                // 抵消离屏放大的额外开销。
                let mut y = 0;
                while y < h {
                    let band = (h - y).min(AA_SS);
                    let base = grad_at(r.top + y - scroll, win_h);
                    let c = over(base, over_c, alpha);
                    fill(
                        s.tmp,
                        &RECT {
                            left: 0,
                            top: y * AA_SS,
                            right: s.tw,
                            bottom: (y + band) * AA_SS,
                        },
                        c,
                    );
                    y += band;
                }
                if !hrgn.is_invalid() {
                    let _ = SelectClipRgn(s.tmp, None);
                    let _ = DeleteObject(hrgn.into());
                }
            }
            s.finish(hdc, r);
            return;
        }
    }
    fill_glass_round_raw(hdc, r, radius, over_c, alpha, scroll, win_h);
}

/// `fill_glass_round` 的原路径（窗口 DC 上 Region 相交裁剪 + 逐行混合）。
fn fill_glass_round_raw(
    hdc: HDC,
    r: &RECT,
    radius: i32,
    over_c: Color,
    alpha: f32,
    scroll: i32,
    win_h: i32,
) {
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
        (
            if hovered {
                theme().C_BTN_H
            } else {
                theme().C_BTN
            },
            theme().C_ON_GRAD,
        )
    } else {
        (
            if hovered {
                theme().C_BTN_WHITE_H
            } else {
                theme().C_BTN_WHITE
            },
            theme().C_BTN_WHITE_TEXT,
        )
    };
    fill_round(hdc, r, 6, bg);
    text_center(hdc, r, label, font, fg);
}

/// 状态徽章（已启用 / 未启用 / 警告）。
pub fn badge(hdc: HDC, r: &RECT, label: &str, kind: BadgeKind, font: HFONT) {
    let (fg, bg) = match kind {
        BadgeKind::On => (theme().C_GREEN_DARK, theme().C_GREEN_BG),
        BadgeKind::Off => (theme().C_GRAY_DARK, theme().C_GRAY_BG),
        BadgeKind::Warn => (theme().C_AMBER_DARK, theme().C_AMBER_BG),
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
    text(hdc, r, letter, font, theme().C_ON_GRAD, true);
}

/// 卡片（玻璃底 + 细描边）。
///
/// **阴影已彻底移除**（Z.W. 真机反馈"多了一层底"）。玻璃主题下卡片靠
/// "比背景亮一层"来分层，不靠阴影——GDI 没有 alpha 通道做不出真阴影，
/// 硬画只会变成一圈灰边。
pub fn card(hdc: HDC, r: &RECT) {
    fill_round(hdc, r, WIN_RADIUS, theme().C_CARD_BG);
    stroke_round(hdc, r, WIN_RADIUS, theme().C_BORDER, 1);
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

#[cfg(test)]
mod aa_tests {
    use super::*;
    use windows::Win32::Foundation::COLORREF;

    fn r(l: i32, t: i32, rt: i32, b: i32) -> RECT {
        RECT {
            left: l,
            top: t,
            right: rt,
            bottom: b,
        }
    }

    /// AA 路径准入：小圆角/小尺寸不值得（过渡带不足 1px），正常卡值得。
    #[test]
    fn aa_worthwhile_thresholds() {
        // 圆角 < 3px：过渡带不足，不值得。
        assert!(!aa_worthwhile(&r(0, 0, 200, 100), 2));
        assert!(aa_worthwhile(&r(0, 0, 200, 100), 3));
        // 尺寸 < 16px：AA 后肉眼无差且开销不划算。
        assert!(!aa_worthwhile(&r(0, 0, 15, 100), 8));
        assert!(!aa_worthwhile(&r(0, 0, 200, 15), 8));
        // 常规卡片：值得。
        assert!(aa_worthwhile(&r(0, 0, 545, 300), 12));
        // 退化矩形：宽或高 ≤ 0 一律不进。
        assert!(!aa_worthwhile(&r(10, 10, 10, 50), 8));
    }

    /// fill_round/stroke_round/fill_glass_round 在 1×1 内存 DC 上全链路不崩：
    /// 大矩形会尝试 AA 离屏（背景抠入/缩回在 1×1 源上部分失败也要安全降级），
    /// 小矩形走原路径；任何 GDI 失败都不得 panic。
    #[test]
    fn aa_paths_never_panic_on_tiny_dc() {
        let screen = unsafe { GetDC(None) };
        let mem = unsafe { CreateCompatibleDC(Some(screen)) };
        unsafe {
            let _ = ReleaseDC(None, screen);
        }
        if mem.is_invalid() {
            eprintln!("[skip] 无法创建内存 DC，跳过 AA 路径冒烟");
            return;
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
            eprintln!("[skip] 无法创建 DIB，跳过 AA 路径冒烟");
            return;
        };
        unsafe {
            let old = SelectObject(mem, bmp.into());
            let c = COLORREF(0x00201850);
            // 大矩形（AA 路径会启动，离屏抠背景对 1×1 源拉伸成功与否均须安全）。
            fill_round(mem, &r(0, 0, 300, 200), 12, c);
            stroke_round(mem, &r(0, 0, 300, 200), 12, c, 1);
            fill_glass_round(mem, &r(0, 0, 300, 200), 12, c, 0.5, 0, 948);
            // 小矩形（低于阈值，原路径）。
            fill_round(mem, &r(0, 0, 10, 10), 3, c);
            stroke_round(mem, &r(0, 0, 10, 10), 3, c, 1);
            fill_glass_round(mem, &r(0, 0, 10, 10), 3, c, 0.5, 0, 948);
            // 退化矩形（宽高 ≤ 0，直接返回）。
            fill_round(mem, &r(5, 5, 5, 50), 8, c);
            stroke_round(mem, &r(5, 5, 50, 5), 8, c, 1);
            let _ = SelectObject(mem, old);
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(mem);
        }
    }
}
