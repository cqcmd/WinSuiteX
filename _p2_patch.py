# -*- coding: utf-8 -*-
import io, sys

PATH = r"D:/project/WorkBuddy/内置工具/WinSuiteX/main.cpp"
with io.open(PATH, "r", encoding="utf-8") as f:
    s = f.read()

def rep(old, new, n=1):
    global s
    c = s.count(old)
    if c != n:
        sys.stderr.write("FAIL count=%d (want %d) for:\n%s\n\n" % (c, n, old[:120]))
        sys.exit(1)
    s = s.replace(old, new, n)
    return s

# ---------- 1) ClearShadows(): invalidate shadow cache on DPI/resize ----------
old1 = """    g_shadowCount++;
    return b;
}
"""
new1 = """    g_shadowCount++;
    return b;
}

// 缩放/DPI 变更后旧阴影尺寸失效 -> 整缓存清空，下次按需重建
static void ClearShadows() {
    for (int i = 0; i < g_shadowCount; i++)
        if (g_shadows[i].bmp) { delete g_shadows[i].bmp; g_shadows[i].bmp = NULL; }
    g_shadowCount = 0;
}
"""
s = rep(old1, new1, 1)

# ---------- 2) DrawCardText(): GDI+ DrawString for card title/desc (AA consistent with cards) ----------
old2 = """static HFONT MakeFont(int hpx, int weight) {
    return CreateFontW(-hpx, 0, 0, 0, weight, FALSE, FALSE, FALSE,
        DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY, DEFAULT_PITCH | FF_SWISS, L"Microsoft YaHei");
}
"""
new2 = """static HFONT MakeFont(int hpx, int weight) {
    return CreateFontW(-hpx, 0, 0, 0, weight, FALSE, FALSE, FALSE,
        DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY, DEFAULT_PITCH | FF_SWISS, L"Microsoft YaHei");
}

// 卡片标题/说明：用 GDI+ DrawString 绘制，使抗锯齿质感与圆角卡(GDI+)一致。
// 用 Font(hdc, hf) 包裹 MakeFont 产出的 HFONT，完整保留中文字体/字重(含 semibold)。
static void DrawCardText(HDC hdc, const WCHAR* text, RECT* r, int hpx, int weight,
                         COLORREF color, DWORD dt) {
    HFONT hf = MakeFont(hpx, weight);
    HFONT of = (HFONT)SelectObject(hdc, hf);
    if (g_pg) {
        Font f(hdc, hf);
        SolidBrush br(Color(255, GetRValue(color), GetGValue(color), GetBValue(color)));
        StringFormat sf;
        sf.SetAlignment(StringAlignmentNear);
        sf.SetLineAlignment((dt & DT_VCENTER) ? StringAlignmentCenter : StringAlignmentNear);
        if (dt & DT_WORDBREAK) sf.SetTrimming(StringTrimmingEllipsisCharacter);
        else                  sf.SetFormatFlags(StringFormatFlagsNoWrap);
        RectF rf((REAL)r->left, (REAL)r->top, (REAL)(r->right - r->left), (REAL)(r->bottom - r->top));
        g_pg->DrawString(text, -1, &f, rf, &sf, &br);
    } else {
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, color);
        DrawTextW(hdc, text, -1, r, dt);
    }
    SelectObject(hdc, of);
    DeleteObject(hf);
}
"""
s = rep(old2, new2, 1)

# ---------- 3) Hook ClearShadows() into WM_DPICHANGED ----------
old3 = """        g_dpi = (UINT)HIWORD(wParam);
        ComputeLayout();                       // 用新 DPI 重算所有坐标/尺寸
"""
new3 = """        g_dpi = (UINT)HIWORD(wParam);
        ComputeLayout();                       // 用新 DPI 重算所有坐标/尺寸
        ClearShadows();                        // DPI 变更 -> 旧尺寸阴影失效，清空重建
"""
s = rep(old3, new3, 1)

# ---------- 4) CmdHere / PowerShellHere: distinct colored terminals (black vs blue) ----------
old4 = r'''    } else if (wcscmp(fid, L"CmdHere") == 0) {
        // 命令提示符：终端窗口 + ">" 提示符 + 光标
        RECT term = {cx - Scale(15), cy - Scale(12), cx + Scale(15), cy + Scale(12)};
        StrokeRoundedRect(hdc, &term, Scale(3), W, pw);
        HPEN wp = CreatePen(PS_SOLID, pw, W);
        HPEN owp = (HPEN)SelectObject(hdc, wp);
        MoveToEx(hdc, cx - Scale(10), cy - Scale(4), NULL); LineTo(hdc, cx - Scale(4), cy);
        MoveToEx(hdc, cx - Scale(4), cy, NULL);            LineTo(hdc, cx - Scale(10), cy + Scale(4));
        SelectObject(hdc, owp); DeleteObject(wp);
        HPEN cp = CreatePen(PS_SOLID, pw, W);
        HPEN ocp = (HPEN)SelectObject(hdc, cp);
        MoveToEx(hdc, cx - Scale(1), cy + Scale(2), NULL); LineTo(hdc, cx + Scale(8), cy + Scale(2));
        SelectObject(hdc, ocp); DeleteObject(cp);
    } else if (wcscmp(fid, L"PowerShellHere") == 0) {
        // PowerShell：终端窗口 + "PS" 字样（与其它终端明确区分）
        RECT term = {cx - Scale(15), cy - Scale(12), cx + Scale(15), cy + Scale(12)};
        StrokeRoundedRect(hdc, &term, Scale(3), W, pw);
        HFONT pf = MakeFont(Scale(16), FW_BOLD);
        HFONT opf = (HFONT)SelectObject(hdc, pf);
        SetTextColor(hdc, W);
        SetBkMode(hdc, TRANSPARENT);
        RECT tr = {term.left, cy - Scale(9), term.right, cy + Scale(9)};
        DrawTextW(hdc, L"PS", -1, &tr, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
        SelectObject(hdc, opf); DeleteObject(pf);
    }'''
new4 = r'''    } else if (wcscmp(fid, L"CmdHere") == 0) {
        // 命令提示符：黑底终端窗口 + ">" 提示符 + 光标（与 PowerShell 蓝底明确区分）
        RECT term = {cx - Scale(15), cy - Scale(12), cx + Scale(15), cy + Scale(12)};
        DrawRoundedRect(hdc, &term, Scale(3), RGB(36, 36, 44), W); // 黑底终端
        HPEN wp = CreatePen(PS_SOLID, pw, W);
        HPEN owp = (HPEN)SelectObject(hdc, wp);
        MoveToEx(hdc, cx - Scale(10), cy - Scale(4), NULL); LineTo(hdc, cx - Scale(4), cy);
        MoveToEx(hdc, cx - Scale(4), cy, NULL);            LineTo(hdc, cx - Scale(10), cy + Scale(4));
        SelectObject(hdc, owp); DeleteObject(wp);
        HPEN cp = CreatePen(PS_SOLID, pw, W);
        HPEN ocp = (HPEN)SelectObject(hdc, cp);
        MoveToEx(hdc, cx - Scale(1), cy + Scale(2), NULL); LineTo(hdc, cx + Scale(8), cy + Scale(2));
        SelectObject(hdc, ocp); DeleteObject(cp);
    } else if (wcscmp(fid, L"PowerShellHere") == 0) {
        // PowerShell：蓝底终端窗口 + "PS" 字样（蓝底 vs cmd 黑底，缩略也分得清）
        RECT term = {cx - Scale(15), cy - Scale(12), cx + Scale(15), cy + Scale(12)};
        DrawRoundedRect(hdc, &term, Scale(3), RGB(0, 48, 104), W); // PowerShell 蓝底
        HFONT pf = MakeFont(Scale(16), FW_BOLD);
        HFONT opf = (HFONT)SelectObject(hdc, pf);
        SetTextColor(hdc, W);
        SetBkMode(hdc, TRANSPARENT);
        RECT tr = {term.left, cy - Scale(9), term.right, cy + Scale(9)};
        DrawTextW(hdc, L"PS", -1, &tr, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
        SelectObject(hdc, opf); DeleteObject(pf);
    }'''
s = rep(old4, new4, 1)

# ---------- 5) Card text: classic-menu name ----------
old5 = r'''            {
                HFONT nameF = MakeFont(Scale(13), FW_SEMIBOLD);
                HFONT of = (HFONT)SelectObject(hdc, nameF);
                SetTextColor(hdc, C_TEXT);
                SetBkMode(hdc, TRANSPARENT);
                RECT nR = {tx, g_classicMenuRect.top + Scale(7),
                           cbr.left - Scale(8), g_classicMenuRect.top + Scale(7) + Scale(18)};
                DrawTextW(hdc, L"\u7ecf\u5178\u83dc\u5355", -1, &nR,
                          DT_LEFT | DT_SINGLELINE | DT_VCENTER);
                SelectObject(hdc, of);
                DeleteObject(nameF);
            }'''
new5 = r'''            {
                RECT nR = {tx, g_classicMenuRect.top + Scale(7),
                           cbr.left - Scale(8), g_classicMenuRect.top + Scale(7) + Scale(18)};
                DrawCardText(hdc, L"\u7ecf\u5178\u83dc\u5355", &nR, Scale(13), FW_SEMIBOLD, C_TEXT,
                             DT_LEFT | DT_SINGLELINE | DT_VCENTER);
            }'''
s = rep(old5, new5, 1)

# ---------- 6) Card text: classic-menu status desc ----------
old6 = r'''            {
                HFONT descF = MakeFont(Scale(11), FW_NORMAL);
                HFONT of = (HFONT)SelectObject(hdc, descF);
                SetTextColor(hdc, on ? C_PRIMARY : C_DESC);
                RECT dR = {tx, g_classicMenuRect.top + Scale(24),
                           cbr.left - Scale(8), g_classicMenuRect.top + Scale(24) + Scale(16)};
                DrawTextW(hdc, statusText, -1, &dR, DT_LEFT | DT_SINGLELINE | DT_VCENTER);
                SelectObject(hdc, of);
                DeleteObject(descF);
            }'''
new6 = r'''            {
                RECT dR = {tx, g_classicMenuRect.top + Scale(24),
                           cbr.left - Scale(8), g_classicMenuRect.top + Scale(24) + Scale(16)};
                DrawCardText(hdc, statusText, &dR, Scale(11), FW_NORMAL, on ? C_PRIMARY : C_DESC,
                             DT_LEFT | DT_SINGLELINE | DT_VCENTER);
            }'''
s = rep(old6, new6, 1)

# ---------- 7) Card text: feature name ----------
old7 = r'''            // 功能名称
            {
                HFONT nameF = MakeFont(Scale(16), FW_SEMIBOLD);
                HFONT of = (HFONT)SelectObject(hdc, nameF);
                SetTextColor(hdc, C_TEXT);
                SetBkMode(hdc, TRANSPARENT);
                RECT nR = {tx, g_cardRect[i].top + Scale(18),
                           g_btnRect[i].left - Scale(14),
                           g_cardRect[i].top + Scale(18) + Scale(24)};
                DrawTextW(hdc, f->displayName, -1, &nR,
                          DT_LEFT | DT_SINGLELINE | DT_VCENTER);
                SelectObject(hdc, of);
                DeleteObject(nameF);
            }'''
new7 = r'''            // 功能名称
            {
                RECT nR = {tx, g_cardRect[i].top + Scale(18),
                           g_btnRect[i].left - Scale(14),
                           g_cardRect[i].top + Scale(18) + Scale(24)};
                DrawCardText(hdc, f->displayName, &nR, Scale(16), FW_SEMIBOLD, C_TEXT,
                             DT_LEFT | DT_SINGLELINE | DT_VCENTER);
            }'''
s = rep(old7, new7, 1)

# ---------- 8) Card text: feature description ----------
old8 = r'''            // 说明文字（两行完整显示，超长才省略，不再单行截断）
            {
                HFONT descF = MakeFont(Scale(15), FW_NORMAL);
                HFONT of = (HFONT)SelectObject(hdc, descF);
                SetTextColor(hdc, C_DESC);
                SetBkMode(hdc, TRANSPARENT);
                RECT dR = {tx, g_cardRect[i].top + Scale(42),
                           g_btnRect[i].left - Scale(14),
                           g_cardRect[i].top + Scale(42) + Scale(40)};
                DrawTextW(hdc, f->description, -1, &dR,
                          DT_LEFT | DT_WORDBREAK | DT_END_ELLIPSIS | DT_VCENTER);
                SelectObject(hdc, of);
                DeleteObject(descF);
            }'''
new8 = r'''            // 说明文字（两行完整显示，超长才省略，不再单行截断）
            {
                RECT dR = {tx, g_cardRect[i].top + Scale(42),
                           g_btnRect[i].left - Scale(14),
                           g_cardRect[i].top + Scale(42) + Scale(40)};
                DrawCardText(hdc, f->description, &dR, Scale(15), FW_NORMAL, C_DESC,
                             DT_LEFT | DT_WORDBREAK | DT_END_ELLIPSIS | DT_VCENTER);
            }'''
s = rep(old8, new8, 1)

with io.open(PATH, "w", encoding="utf-8") as f:
    f.write(s)
sys.stderr.write("ALL PATCHES APPLIED OK\n")
