# -*- coding: utf-8 -*-
import io, sys

PATH = r"D:/project/WorkBuddy/内置工具/WinSuiteX/main.cpp"
with io.open(PATH, "r", encoding="utf-8") as f:
    s = f.read()

START = "static void DrawFeatureIcon(HDC hdc, RECT* r, const WCHAR* fid, COLORREF accent) {"
i = s.find(START)
assert i != -1, "DrawFeatureIcon start not found"

# 从 '{' 之后做括号匹配，定位函数结束的 '}'
depth = 0
j = s.index("{", i)
k = j
while k < len(s):
    c = s[k]
    if c == "{":
        depth += 1
    elif c == "}":
        depth -= 1
        if depth == 0:
            break
    k += 1
assert depth == 0, "brace mismatch"
end = k  # 指向结束的 '}'

NEW = '''// 功能图标：彩色圆角瓦片 + 白色语义线稿（玻璃风，零运行时依赖）
// 每个功能对应一个专属矢量图标：辨识度 + 与整体视觉一致。
static void DrawFeatureIcon(HDC hdc, RECT* r, const WCHAR* fid, COLORREF accent) {
    (void)accent; // 瓦片底色由调用处填充，这里只画线稿
    int cx = (r->left + r->right) / 2, cy = (r->top + r->bottom) / 2;
    int pw = Scale(2);   // 线稿统一线宽
    COLORREF W = RGB(255, 255, 255);

    if (wcscmp(fid, L"ComputerManagement") == 0) {
        // 显示器：屏幕 + 支架 + 底座，屏内两条内容线
        RECT s = {cx - Scale(14), cy - Scale(12), cx + Scale(14), cy + Scale(4)};
        StrokeRoundedRect(hdc, &s, Scale(3), W, pw);
        HPEN wp = CreatePen(PS_SOLID, pw, W);
        HPEN owp = (HPEN)SelectObject(hdc, wp);
        int ly = cy - Scale(8);
        MoveToEx(hdc, cx - Scale(9), ly, NULL);            LineTo(hdc, cx + Scale(9), ly);
        MoveToEx(hdc, cx - Scale(9), ly + Scale(5), NULL); LineTo(hdc, cx + Scale(4), ly + Scale(5));
        MoveToEx(hdc, cx, cy + Scale(4), NULL);            LineTo(hdc, cx, cy + Scale(9));
        MoveToEx(hdc, cx - Scale(7), cy + Scale(9), NULL); LineTo(hdc, cx + Scale(7), cy + Scale(9));
        SelectObject(hdc, owp); DeleteObject(wp);
    } else if (wcscmp(fid, L"DeviceManager") == 0) {
        // 芯片：圆角方体 + 四边引脚 + 中心方块
        RECT sq = {cx - Scale(8), cy - Scale(8), cx + Scale(8), cy + Scale(8)};
        StrokeRoundedRect(hdc, &sq, Scale(2), W, pw);
        RECT dot = {cx - Scale(3), cy - Scale(3), cx + Scale(3), cy + Scale(3)};
        HBRUSH wb = CreateSolidBrush(W);
        HBRUSH owb = (HBRUSH)SelectObject(hdc, wb);
        Rectangle(hdc, dot.left, dot.top, dot.right, dot.bottom);
        SelectObject(hdc, owb); DeleteObject(wb);
        HPEN wp = CreatePen(PS_SOLID, pw, W);
        HPEN owp = (HPEN)SelectObject(hdc, wp);
        for (int o = -8; o <= 8; o += 8) {
            int so = Scale(o);
            MoveToEx(hdc, cx + so, cy - Scale(8), NULL);  LineTo(hdc, cx + so, cy - Scale(12));
            MoveToEx(hdc, cx + so, cy + Scale(8), NULL);  LineTo(hdc, cx + so, cy + Scale(12));
            MoveToEx(hdc, cx - Scale(8), cy + so, NULL);  LineTo(hdc, cx - Scale(12), cy + so);
            MoveToEx(hdc, cx + Scale(8), cy + so, NULL);  LineTo(hdc, cx + Scale(12), cy + so);
        }
        SelectObject(hdc, owp); DeleteObject(wp);
    } else if (wcscmp(fid, L"SuperHidden") == 0) {
        // 文件夹 + 眼睛（显示/隐藏系统文件）
        RECT fb = {cx - Scale(14), cy - Scale(4), cx + Scale(14), cy + Scale(12)};
        StrokeRoundedRect(hdc, &fb, Scale(3), W, pw);
        RECT ft = {cx - Scale(14), cy - Scale(10), cx - Scale(2), cy - Scale(4)};
        HBRUSH wb = CreateSolidBrush(W);
        HBRUSH owb = (HBRUSH)SelectObject(hdc, wb);
        RoundRect(hdc, ft.left, ft.top, ft.right, ft.bottom, 2 * Scale(2), 2 * Scale(2));
        SelectObject(hdc, owb); DeleteObject(wb);
        int ex = cx, ey = cy + Scale(4), er = Scale(4);
        StrokeEllipse(hdc, ex, ey, er, W, pw);
        HBRUSH pb = CreateSolidBrush(W);
        HBRUSH opb = (HBRUSH)SelectObject(hdc, pb);
        Ellipse(hdc, ex - Scale(1), ey - Scale(1), ex + Scale(1), ey + Scale(1));
        SelectObject(hdc, opb); DeleteObject(pb);
    } else if (wcscmp(fid, L"CmdHere") == 0) {
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
    } else {
        // 兜底：文件夹 + 眼睛
        RECT fb = {cx - Scale(14), cy - Scale(4), cx + Scale(14), cy + Scale(12)};
        StrokeRoundedRect(hdc, &fb, Scale(3), W, pw);
        RECT ft = {cx - Scale(14), cy - Scale(10), cx - Scale(2), cy - Scale(4)};
        HBRUSH wb = CreateSolidBrush(W);
        HBRUSH owb = (HBRUSH)SelectObject(hdc, wb);
        RoundRect(hdc, ft.left, ft.top, ft.right, ft.bottom, 2 * Scale(2), 2 * Scale(2));
        SelectObject(hdc, owb); DeleteObject(wb);
        int ex = cx, ey = cy + Scale(4), er = Scale(4);
        StrokeEllipse(hdc, ex, ey, er, W, pw);
        HBRUSH pb = CreateSolidBrush(W);
        HBRUSH opb = (HBRUSH)SelectObject(hdc, pb);
        Ellipse(hdc, ex - Scale(1), ey - Scale(1), ex + Scale(1), ey + Scale(1));
        SelectObject(hdc, opb); DeleteObject(pb);
    }
}
'''

new_s = s[:i] + NEW + s[end + 1:]
assert 'L"PowerShellHere"' in new_s, "PowerShellHere branch missing"
with io.open(PATH, "w", encoding="utf-8") as f:
    f.write(new_s)
print("OK: DrawFeatureIcon replaced, new length=%d (was %d..%d)" % (len(NEW), i, end))
