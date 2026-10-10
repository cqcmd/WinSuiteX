//! 简笔图标：GDI 线段/矩形/椭圆组合，不依赖任何图标字体或图片资源。
//!
//! # 为什么不用 emoji
//!
//! 最初分类标题用的是 `🖥🧠💾` 这类 emoji，但 GDI 的 `CLEARTYPE_QUALITY`
//! 常规字体**没有这些符号字形**，实测全部渲染成豆腐块（□）。用符号字体
//! (Segoe UI Symbol)虽然能显示，但会引入字体回退的不可控性。
//!
//! 移植 C++ 版（`main.cpp:1850DrawHwSectionIcon`）的做法：纯 `Rectangle` /
//! `Ellipse` / `MoveToEx`+`LineTo` 画 14px 的简笔图元，永远是单色描边，
//! 配彩色瓦片底 —— 网吧机器上缺字体也不会缺图形。

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::*;

use crate::theme::{scale, Color};

/// 硬件分组图标（与 C++ 版 `HWSEC_*` 一一对应）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HwIcon {
    /// 操作系统（窗口）。
    Os,
    /// 主板 / BIOS（芯片 + 引脚）。
    Board,
    /// 处理器（双层方框）。
    Cpu,
    /// 内存（窄条 + 颗粒线）。
    Mem,
    /// 显卡（长条 + 风扇圆）。
    Gpu,
    /// 硬盘（方块 + 中心轴圆）。
    Disk,
    /// 网卡（RJ45 + 触点）。
    Nic,
    /// 音频（喇叭）。
    Audio,
}

/// 画一个硬件分组简笔图标（描边 `color`，在 `r` 内居中占 `sz` 见方）。
pub fn draw_hw_icon(hdc: HDC, r: &RECT, icon: HwIcon, color: Color) {
    // 统一图标边长：比瓦片略小，留 4px 内边距。
    let sz = scale(14).min((r.right - r.left).min(r.bottom - r.top) - scale(4));
    if sz <= 2 {
        return;
    }
    let t = r.top + ((r.bottom - r.top) - sz) / 2;
    let l = r.left + ((r.right - r.left) - sz) / 2;
    let rt = l + sz;
    let b = t + sz;
    let cx = (l + rt) / 2;
    let cy = (t + b) / 2;

    // SAFETY: 全部是GDI 绘图调用；pen/brush 在本函数内成对Select/恢复与删除。
    unsafe {
        let pen = CreatePen(PS_SOLID, scale(1).max(1), color);
        if pen.is_invalid() {
            return;
        }
        let old_pen = SelectObject(hdc, pen.into());
        let null = GetStockObject(NULL_BRUSH);
        let old_br = SelectObject(hdc, null);

        match icon {
            HwIcon::Os => {
                // 窗口：外框 + 标题栏分隔线。
                let _ = Rectangle(hdc, l, t + scale(3), rt, b);
                let _ = MoveToEx(hdc, l, t + scale(6), None);
                let _ = LineTo(hdc, rt, t + scale(6));
            }
            HwIcon::Board => {
                // 芯片：内框 + 左右各 3 根引脚。
                let _ = Rectangle(hdc, l + scale(3), t + scale(3), rt - scale(3), b - scale(3));
                for i in 0..3 {
                    let py = t + scale(3) + i * scale(3);
                    let _ = MoveToEx(hdc, l, py, None);
                    let _ = LineTo(hdc, l + scale(3), py);
                    let _ = MoveToEx(hdc, rt - scale(3), py, None);
                    let _ = LineTo(hdc, rt, py);
                }
            }
            HwIcon::Cpu => {
                // CPU：外框 + 内框（经典"芯片"观感）。
                let _ = Rectangle(hdc, l + scale(1), t + scale(1), rt - scale(1), b - scale(1));
                let _ = Rectangle(hdc, l + scale(4), t + scale(4), rt - scale(4), b - scale(4));
            }
            HwIcon::Mem => {
                // 内存条：窄长条 + 3 条颗粒线。
                let _ = Rectangle(hdc, l + scale(4), t, rt - scale(4), b);
                for i in 0..3 {
                    let py = t + scale(3) + i * scale(3);
                    let _ = MoveToEx(hdc, l + scale(4), py, None);
                    let _ = LineTo(hdc, rt - scale(4), py);
                }
            }
            HwIcon::Gpu => {
                // 显卡：长条 + 风扇圆。
                let _ = Rectangle(hdc, l, t + scale(3), rt, b - scale(1));
                let _ = Ellipse(
                    hdc,
                    cx - scale(3),
                    cy - scale(2),
                    cx + scale(3),
                    cy + scale(3),
                );
            }
            HwIcon::Disk => {
                // 硬盘：方块 + 中心轴。
                let _ = Rectangle(hdc, l, t + scale(2), rt, b);
                let _ = Ellipse(
                    hdc,
                    cx - scale(2),
                    cy - scale(1),
                    cx + scale(2),
                    cy + scale(2),
                );
            }
            HwIcon::Nic => {
                // RJ45：主体 + 4 根触点。
                let _ = Rectangle(hdc, l + scale(1), t + scale(2), rt - scale(1), b - scale(3));
                for i in 0..4 {
                    let py = t + scale(4) + i * scale(2);
                    let _ = MoveToEx(hdc, l + scale(3), py, None);
                    let _ = LineTo(hdc, rt - scale(3), py);
                }
            }
            HwIcon::Audio => {
                // 喇叭：左侧梯形 + 右侧两道声波弧（用短线代替弧，GDI Arc 需额外 brush）。
                let _ = Rectangle(
                    hdc,
                    l + scale(1),
                    cy - scale(2),
                    l + scale(5),
                    cy + scale(2),
                );
                let _ = MoveToEx(hdc, l + scale(5), cy - scale(2), None);
                let _ = LineTo(hdc, l + scale(8), t + scale(1));
                let _ = MoveToEx(hdc, l + scale(5), cy + scale(2), None);
                let _ = LineTo(hdc, l + scale(8), b - scale(1));
                let _ = MoveToEx(hdc, l + scale(8), t + scale(1), None);
                let _ = LineTo(hdc, l + scale(8), b - scale(1));
                let _ = MoveToEx(hdc, l + scale(10), cy - scale(3), None);
                let _ = LineTo(hdc, l + scale(12), cy + scale(3));
            }
        }

        let _ = SelectObject(hdc, old_br);
        let _ = SelectObject(hdc, old_pen);
        let _ = DeleteObject(pen.into());
    }
}

/// 应用图标（顶栏瓦片用）：一个"芯片+ 心形点"的组合，比 emoji 稳。
pub fn draw_app_icon(hdc: HDC, r: &RECT) {
    // 白色描边（顶栏是紫渐变底，图标必须亮色才看得见）。
    draw_hw_icon(hdc, r, HwIcon::Cpu, crate::theme::C_ON_GRAD);
}

// ============================================================
// 右键增强页功能图标（数据驱动的 6 项，GDI 简笔）
// ============================================================

/// 右键增强页每一行左侧的功能图标。
///
/// `menu_icon` 字段是 Windows shell 图标引用串（如 `compmgmt.msc`），GDI 没法直接
/// 画，所以这里自建一套简笔字形，和硬件页图标同一套绘制语言（单色描边）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FeatureIcon {
    /// 计算机管理（显示器 + 底座）。
    Monitor,
    /// 设备管理器（芯片方块 + 引脚）。
    Chip,
    /// 显示/隐藏系统文件（眼睛）。
    Eye,
    /// 命令提示符（终端窗 + `>` 提示符）。
    Terminal,
    /// PowerShell（`PS>` 提示符）。
    PowerShell,
    /// 经典右键菜单（三横线 ≡）。
    Menu,
}

/// 画一个功能简笔图标（描边 `color`，在 `r` 内居中占 `sz` 见方）。
pub fn draw_feature_icon(hdc: HDC, r: &RECT, icon: FeatureIcon, color: Color) {
    let sz = scale(16).min((r.right - r.left).min(r.bottom - r.top) - scale(2));
    if sz <= 2 {
        return;
    }
    let t = r.top + ((r.bottom - r.top) - sz) / 2;
    let l = r.left + ((r.right - r.left) - sz) / 2;
    let rt = l + sz;
    let b = t + sz;
    let cx = (l + rt) / 2;
    let cy = (t + b) / 2;

    // SAFETY: GDI 绘图调用；pen/brush 成对 Select/恢复与删除。
    unsafe {
        let pen = CreatePen(PS_SOLID, scale(1).max(1), color);
        if pen.is_invalid() {
            return;
        }
        let old_pen = SelectObject(hdc, pen.into());
        let null_br = GetStockObject(NULL_BRUSH);
        let old_br = SelectObject(hdc, null_br);

        match icon {
            FeatureIcon::Monitor => {
                // 屏幕 + 底座。
                let _ = Rectangle(hdc, l, t, rt, b - scale(2));
                let _ = MoveToEx(hdc, cx, b - scale(2), None);
                let _ = LineTo(hdc, cx, b);
                let _ = MoveToEx(hdc, l + scale(3), b, None);
                let _ = LineTo(hdc, rt - scale(3), b);
            }
            FeatureIcon::Chip => {
                // 方块 + 左右各 2 根引脚。
                let _ = Rectangle(hdc, l + scale(3), t + scale(3), rt - scale(3), b - scale(3));
                for i in 0..2 {
                    let py = t + scale(5) + i * scale(4);
                    let _ = MoveToEx(hdc, l, py, None);
                    let _ = LineTo(hdc, l + scale(3), py);
                    let _ = MoveToEx(hdc, rt - scale(3), py, None);
                    let _ = LineTo(hdc, rt, py);
                }
            }
            FeatureIcon::Eye => {
                // 眼形（椭圆）+ 瞳孔（实心圆）。
                let _ = Ellipse(hdc, l + scale(1), t + scale(3), rt - scale(1), b - scale(3));
                // 填充瞳孔：切到 NULL_PEN + 实心笔刷，画完还原。
                let np = GetStockObject(NULL_PEN);
                let onp = SelectObject(hdc, np);
                let fb = CreateSolidBrush(color);
                let ofb = SelectObject(hdc, fb.into());
                let pr = scale(2).max(2);
                let _ = Ellipse(hdc, cx - pr, cy - pr, cx + pr, cy + pr);
                let _ = SelectObject(hdc, ofb);
                let _ = DeleteObject(fb.into());
                let _ = SelectObject(hdc, onp);
            }
            FeatureIcon::Terminal => {
                // 终端窗 + `>` 提示符 + 下划线光标。
                let _ = Rectangle(hdc, l, t, rt, b);
                let _ = MoveToEx(hdc, l + scale(3), t + scale(3), None);
                let _ = LineTo(hdc, l + scale(6), cy);
                let _ = LineTo(hdc, l + scale(3), b - scale(3));
                let _ = MoveToEx(hdc, l + scale(3), b - scale(2), None);
                let _ = LineTo(hdc, l + scale(9), b - scale(2));
            }
            FeatureIcon::PowerShell => {
                // `P` 竖杆 + `>` 提示符。
                let _ = MoveToEx(hdc, l + scale(2), t + scale(2), None);
                let _ = LineTo(hdc, l + scale(2), b - scale(2));
                let _ = MoveToEx(hdc, l + scale(4), t + scale(3), None);
                let _ = LineTo(hdc, l + scale(7), cy);
                let _ = LineTo(hdc, l + scale(4), b - scale(3));
            }
            FeatureIcon::Menu => {
                // 三横线 ≡。
                for i in 0..3 {
                    let ly = t + scale(3) + i * scale(4);
                    let _ = MoveToEx(hdc, l, ly, None);
                    let _ = LineTo(hdc, rt, ly);
                }
            }
        }

        let _ = SelectObject(hdc, old_br);
        let _ = SelectObject(hdc, old_pen);
        let _ = DeleteObject(pen.into());
    }
}

// ============================================================
// 磁盘分区类型图标（按 type_name 判定）
// ============================================================

/// 磁盘分区类型图标（按 `Volume.type_name` 判定）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VolumeIcon {
    /// 本地磁盘 / RAM 盘（硬盘机身 + 中心轴）。
    Drive,
    /// 可移动磁盘（机身 + USB 连接器）。
    Usb,
    /// 网络磁盘（地球：圆 + 经纬线）。
    Network,
    /// 光盘驱动器（圆盘 + 中心孔）。
    Disc,
}

/// 画一个分区类型简笔图标（描边 `color`，在 `r` 内居中占 `sz` 见方）。
pub fn draw_volume_icon(hdc: HDC, r: &RECT, icon: VolumeIcon, color: Color) {
    let sz = scale(16).min((r.right - r.left).min(r.bottom - r.top) - scale(2));
    if sz <= 2 {
        return;
    }
    let t = r.top + ((r.bottom - r.top) - sz) / 2;
    let l = r.left + ((r.right - r.left) - sz) / 2;
    let rt = l + sz;
    let b = t + sz;
    let cx = (l + rt) / 2;
    let cy = (t + b) / 2;

    // SAFETY: GDI 绘图调用；pen/brush 成对 Select/恢复与删除。
    unsafe {
        let pen = CreatePen(PS_SOLID, scale(1).max(1), color);
        if pen.is_invalid() {
            return;
        }
        let old_pen = SelectObject(hdc, pen.into());
        let null_br = GetStockObject(NULL_BRUSH);
        let old_br = SelectObject(hdc, null_br);

        match icon {
            VolumeIcon::Drive => {
                // 硬盘机身 + 中心轴。
                let _ = Rectangle(hdc, l, t + scale(2), rt, b - scale(2));
                let _ = Ellipse(
                    hdc,
                    cx - scale(2),
                    cy - scale(2),
                    cx + scale(2),
                    cy + scale(2),
                );
            }
            VolumeIcon::Usb => {
                // 机身 + 底部 USB 连接器（实心小块）。
                let _ = Rectangle(hdc, l + scale(2), t, rt - scale(2), b - scale(3));
                let np = GetStockObject(NULL_PEN);
                let onp = SelectObject(hdc, np);
                let fb = CreateSolidBrush(color);
                let ofb = SelectObject(hdc, fb.into());
                let _ = Rectangle(hdc, l + scale(4), b - scale(3), rt - scale(4), b);
                let _ = SelectObject(hdc, ofb);
                let _ = DeleteObject(fb.into());
                let _ = SelectObject(hdc, onp);
            }
            VolumeIcon::Network => {
                // 地球：圆 + 赤道横线 + 经线竖线。
                let _ = Ellipse(hdc, l + scale(1), t + scale(1), rt - scale(1), b - scale(1));
                let _ = MoveToEx(hdc, l + scale(1), cy, None);
                let _ = LineTo(hdc, rt - scale(1), cy);
                let _ = MoveToEx(hdc, cx, t + scale(1), None);
                let _ = LineTo(hdc, cx, b - scale(1));
            }
            VolumeIcon::Disc => {
                // 光盘：外圆 + 中心孔（实心）。
                let _ = Ellipse(hdc, l + scale(1), t + scale(1), rt - scale(1), b - scale(1));
                let np = GetStockObject(NULL_PEN);
                let onp = SelectObject(hdc, np);
                let fb = CreateSolidBrush(color);
                let ofb = SelectObject(hdc, fb.into());
                let pr = scale(2).max(2);
                let _ = Ellipse(hdc, cx - pr, cy - pr, cx + pr, cy + pr);
                let _ = SelectObject(hdc, ofb);
                let _ = DeleteObject(fb.into());
                let _ = SelectObject(hdc, onp);
            }
        }

        let _ = SelectObject(hdc, old_br);
        let _ = SelectObject(hdc, old_pen);
        let _ = DeleteObject(pen.into());
    }
}
