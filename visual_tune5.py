# -*- coding: utf-8 -*-
# visual_tune5: 把 DrawButton 与 glass4 源码对齐
# glass_showcase.cpp:349-366 的按钮规格：
#   primary (启用): 实心紫 (99,102,241) + 白字 + FontStyleBold, 不响应 hover
#   secondary (停用): NO fill + 淡白细边 Color(110,255,255,255) 1.0f + 白字 + FontStyleBold, 不响应 hover
# 唯一例外：notice 模态的"确定"按钮 (primary=TRUE)，仍保留 hover 加深（深紫）。
import re, sys, pathlib, subprocess

ROOT = pathlib.Path(r"D:\project\WorkBuddy\内置工具\WinSuiteX")
MAIN = ROOT / "main.cpp"
src  = MAIN.read_text(encoding="utf-8")

# ---------- 1) 替换 DrawButton 函数体 ----------
old = '''static void DrawButton(HDC hdc, RECT* r, const WCHAR* text,
                       BOOL hovered, BOOL primary) {
    COLORREF fill, border, textc;
    if (primary) {
        // 主按钮：紫（demo 启用态=紫）；hover 加深
        fill   = hovered ? RGB(79, 70, 229) : RGB(99, 102, 241);
        border = fill;
        textc  = RGB(255, 255, 255);
    } else {
        // 次按钮：几乎隐入玻璃底（fill 接近面板色），仅细边区分（demo 风格）
        fill   = hovered ? RGB(225, 230, 248) : RGB(245, 247, 252);
        border = hovered ? RGB(140, 160, 210) : RGB(180, 195, 225);
        textc  = C_TEXT;
    }
    DrawRoundedRect(hdc, r, Scale(6), fill, border);
    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, textc);
    HFONT f = MakeFont(Scale(13), FW_MEDIUM);
    HFONT of = (HFONT)SelectObject(hdc, f);
    DrawTextW(hdc, text, -1, r, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    SelectObject(hdc, of);
    DeleteObject(f);
}'''

new = '''static void DrawButton(HDC hdc, RECT* r, const WCHAR* text,
                       BOOL hovered, BOOL primary) {
    // 与 glass4 glass_showcase.cpp:349-366 对齐
    //   primary  (启用): 实心紫(99,102,241) + 白字加粗，无 hover
    //   secondary(停用): 无填充 + 淡白细边 alpha=110 1px + 白字加粗，无 hover
    // 唯一例外：notice 模态"确定"按钮仍走 primary + hover 加深（深紫 79,70,229）
    int rad = Scale(6);
    int bw = r->right - r->left, bh = r->bottom - r->top;
    SetBkMode(hdc, TRANSPARENT);
    if (primary) {
        // glass4 实心紫；hover 仅作用于 notice 模态（深紫）
        COLORREF fill = hovered ? RGB(79, 70, 229) : RGB(99, 102, 241);
        DrawRoundedRect(hdc, r, rad, fill, fill);
    } else {
        // glass4: NO fill + Color(110,255,255,255) 1px 边——需 GDI+ 走 alpha
        if (g_pg) {
            GraphicsPath p;
            RoundRectPath(&p, (float)r->left, (float)r->top, (float)bw, (float)bh, (float)rad);
            Pen bp(Color(110, 255, 255, 255), 1.0f);
            g_pg->DrawPath(&bp, &p);
            g_pg->Flush(FlushIntentionSync); // 让后面的 GDI 文字正确合成
        } else {
            // 兜底：没 GDI+ 时退化为淡白描边小方框
            HPEN pen = CreatePen(PS_SOLID, 1, RGB(190, 200, 220));
            HBRUSH br = (HBRUSH)GetStockObject(NULL_BRUSH);
            HPEN op = (HPEN)SelectObject(hdc, pen);
            HBRUSH ob = (HBRUSH)SelectObject(hdc, br);
            RoundRect(hdc, r->left, r->top, r->right, r->bottom, rad*2, rad*2);
            SelectObject(hdc, op); SelectObject(hdc, ob);
            DeleteObject(pen);
        }
    }
    SetTextColor(hdc, RGB(255, 255, 255)); // 主+次按钮都是白字
    HFONT f = MakeFont(Scale(13), FW_BOLD); // glass4 FontStyleBold
    HFONT of = (HFONT)SelectObject(hdc, f);
    DrawTextW(hdc, text, -1, r, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    SelectObject(hdc, of);
    DeleteObject(f);
}'''

assert src.count(old) == 1, f"DrawButton block not unique: {src.count(old)}"
src = src.replace(old, new)
print("[OK] 1/1 DrawButton aligned to glass4")

MAIN.write_text(src, encoding="utf-8")
print(f"[OK] wrote {MAIN}")