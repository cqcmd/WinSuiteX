# -*- coding: utf-8 -*-
# 四项视觉改进（Z.W. 反馈）：磁盘选中行可读性 / 段标题图标 / 页眉样式 / 按钮配色
import io, sys

PATH = r"D:\project\WorkBuddy\内置工具\WinSuiteX\main.cpp"
with io.open(PATH, "r", encoding="utf-8") as f:
    s = f.read()

reps = []

def add(old, new, note):
    reps.append((old, new, note))

# ===== F1: 磁盘页选中/悬停行用深字（修复白字在浅绿底上看不见的 bug） =====
add("        SetTextColor(hdc, d->ok ? RGB(238, 242, 252) : RGB(200, 208, 230));",
    "        // 选中/悬停行有浅绿/浅灰底，白字看不清 -> 强制用深字\n"
    "        SetTextColor(hdc, (sel || hov) ? C_TEXT : (d->ok ? RGB(238, 242, 252) : RGB(200, 208, 230)));",
    "disk row text on sel/hov")

# ===== F2: 页眉样式（玻璃风 + demo 一致）：白字 + 底线 =====
old_head = (
    "    int yHead = Scale(TITLE_H + TAB_H + 14 + SHADOW_PAD);\n"
    "    HFONT hf = MakeFont(Scale(15), FW_SEMIBOLD);\n"
    "    HFONT of = (HFONT)SelectObject(hdc, hf);\n"
    "    SetTextColor(hdc, C_TITLE);\n"
    "    SetBkMode(hdc, TRANSPARENT);\n"
    "    RECT r = {xLeft, yHead, g_copyBtnRect.left - Scale(12), yHead + Scale(24)};\n"
    r'    DrawTextW(hdc, L"\u8ba1\u7b97\u673a\u5e38\u89c4\u914d\u7f6e",' + "\n"
    "              -1, &r, DT_LEFT | DT_SINGLELINE | DT_VCENTER);\n"
    "    SelectObject(hdc, of);\n"
    "    DeleteObject(hf);\n"
    "    DrawCopyButton(hdc, &g_copyBtnRect, g_copyBtnHover,\n"
    r'                   L"\u5bfc\u51fa\u62a5\u544a", L"\u5df2\u5bfc\u51fa");' + "\n"
)
new_head = (
    "    int yHead = Scale(TITLE_H + TAB_H + 14 + SHADOW_PAD);\n"
    "    HFONT hf = MakeFont(Scale(16), FW_SEMIBOLD);\n"
    "    HFONT of = (HFONT)SelectObject(hdc, hf);\n"
    "    // 玻璃风：页眉改浅色，对齐 demo 视觉\n"
    "    SetTextColor(hdc, RGB(245, 246, 255));\n"
    "    SetBkMode(hdc, TRANSPARENT);\n"
    "    RECT r = {xLeft, yHead, g_copyBtnRect.left - Scale(12), yHead + Scale(26)};\n"
    r'    DrawTextW(hdc, L"\u8ba1\u7b97\u673a\u5e38\u89c4\u914d\u7f6e",' + "\n"
    "              -1, &r, DT_LEFT | DT_SINGLELINE | DT_VCENTER);\n"
    "    SelectObject(hdc, of);\n"
    "    DeleteObject(hf);\n"
    "    // 玻璃风：页眉下加分隔细线，对齐 demo 视觉语言\n"
    "    {\n"
    "        HPEN lp = CreatePen(PS_SOLID, 1, C_TXT_LIGHT_DIM);\n"
    "        HPEN olp = (HPEN)SelectObject(hdc, lp);\n"
    "        int dy = yHead + Scale(28);\n"
    "        MoveToEx(hdc, xLeft, dy, NULL);\n"
    "        LineTo(hdc, g_copyBtnRect.left - Scale(12), dy);\n"
    "        SelectObject(hdc, olp);\n"
    "        DeleteObject(lp);\n"
    "    }\n"
    "    DrawCopyButton(hdc, &g_copyBtnRect, g_copyBtnHover,\n"
    r'                   L"\u5bfc\u51fa\u62a5\u544a", L"\u5df2\u5bfc\u51fa");' + "\n"
)
add(old_head, new_head, "hw page header style")

# ===== F3: DrawHwSectionTitle —— 接受 sectionId，画图标替代色条；保留磨砂底带 =====
old_title = (
    "// 段标题（背景色块，左侧主色竖条）\n"
    "static int DrawHwSectionTitle(HDC hdc, int xLeft, int xRight, int y, const WCHAR* title) {\n"
    "    int rowH = Scale(g_hwRowH);\n"
    "    RECT tr = {xLeft, y, xRight, y + rowH};\n"
    "    if (g_pg) {\n"
    "        GraphicsPath sbp; RoundRectPath(&sbp, (float)xLeft, (float)y, (float)(xRight - xLeft), (float)rowH, (float)Scale(6));\n"
    "        SolidBrush sb(Color(12, 255, 255, 255));\n"
    "        g_pg->FillPath(&sb, &sbp);\n"
    "        g_pg->Flush(FlushIntentionSync);\n"
    "    } else {\n"
    "        HBRUSH bg = CreateSolidBrush(C_BG);\n"
    "        FillRect(hdc, &tr, bg);\n"
    "        DeleteObject(bg);\n"
    "    }\n"
    "    // 左侧主色竖条\n"
    "    RECT bar = {xLeft, y + Scale(4), xLeft + Scale(3), y + rowH - Scale(4)};\n"
    "    HBRUSH bb = CreateSolidBrush(C_PRIMARY);\n"
    "    FillRect(hdc, &bar, bb);\n"
    "    DeleteObject(bb);\n"
    "    SetBkMode(hdc, TRANSPARENT);\n"
    "    HFONT ft = MakeFont(Scale(14), FW_SEMIBOLD);\n"
    "    HFONT of = (HFONT)SelectObject(hdc, ft);\n"
    "    SetTextColor(hdc, RGB(245, 246, 255));\n"
    "    RECT r2 = {xLeft + Scale(10), y, xRight, y + rowH};\n"
    "    DrawTextW(hdc, title, -1, &r2, DT_LEFT | DT_VCENTER | DT_SINGLELINE);\n"
    "    SelectObject(hdc, of);\n"
    "    DeleteObject(ft);\n"
    "    return y + rowH + Scale(g_hwSecGap);\n"
    "}\n"
)
new_title = (
    "// 段标题小图标（替代原来左侧纯色竖条；每个 section 配语义图标，对齐 demo 视觉语言）\n"
    "static void DrawHwSectionIcon(HDC hdc, int xLeft, int y, int rowH, int sectionId) {\n"
    "    int iconSz = Scale(14);\n"
    "    int ix = xLeft + Scale(4);\n"
    "    int iy = y + (rowH - iconSz) / 2;\n"
    "    RECT ir = {ix, iy, ix + iconSz, iy + iconSz};\n"
    "    HPEN pen = CreatePen(PS_SOLID, Scale(1), RGB(180, 200, 255));\n"
    "    HPEN oldPen = (HPEN)SelectObject(hdc, pen);\n"
    "    HBRUSH br = (HBRUSH)GetStockObject(NULL_BRUSH);\n"
    "    HBRUSH oldBr = (HBRUSH)SelectObject(hdc, br);\n"
    "    int l = ir.left, t = ir.top, r = ir.right, b = ir.bottom;\n"
    "    int cx = (l + r) / 2, cy = (t + b) / 2;\n"
    "    switch (sectionId) {\n"
    "        case HWSEC_OS: // 窗口（OS）\n"
    "            Rectangle(hdc, l, t + Scale(3), r, b);\n"
    "            MoveToEx(hdc, l, t + Scale(6), NULL); LineTo(hdc, r, t + Scale(6));\n"
    "            break;\n"
    "        case HWSEC_MACHINE: // 显示器+底座（计算机）\n"
    "            Rectangle(hdc, l, t, r, b - Scale(4));\n"
    "            MoveToEx(hdc, cx, b - Scale(4), NULL); LineTo(hdc, cx, b);\n"
    "            MoveToEx(hdc, cx - Scale(3), b, NULL); LineTo(hdc, cx + Scale(3), b);\n"
    "            break;\n"
    "        case HWSEC_BOARD: // 芯片+四周引脚（主板）\n"
    "            Rectangle(hdc, l + Scale(3), t + Scale(3), r - Scale(3), b - Scale(3));\n"
    "            for (int i = 0; i < 3; i++) {\n"
    "                int py = t + Scale(3) + i * Scale(3);\n"
    "                MoveToEx(hdc, l, py, NULL); LineTo(hdc, l + Scale(3), py);\n"
    "                MoveToEx(hdc, r - Scale(3), py, NULL); LineTo(hdc, r, py);\n"
    "            }\n"
    "            break;\n"
    "        case HWSEC_CPU: // CPU（处理器）\n"
    "            Rectangle(hdc, l + Scale(1), t + Scale(1), r - Scale(1), b - Scale(1));\n"
    "            Rectangle(hdc, l + Scale(4), t + Scale(4), r - Scale(4), b - Scale(4));\n"
    "            break;\n"
    "        case HWSEC_MEM: // 内存条（RAM）\n"
    "            Rectangle(hdc, l + Scale(4), t, r - Scale(4), b);\n"
    "            for (int i = 0; i < 3; i++) {\n"
    "                int py = t + Scale(3) + i * Scale(3);\n"
    "                MoveToEx(hdc, l + Scale(4), py, NULL); LineTo(hdc, r - Scale(4), py);\n"
    "            }\n"
    "            break;\n"
    "        case HWSEC_GPU: // 显卡+风扇\n"
    "            Rectangle(hdc, l, t + Scale(3), r, b - Scale(1));\n"
    "            Ellipse(hdc, cx - Scale(3), cy - Scale(2), cx + Scale(3), cy + Scale(3));\n"
    "            break;\n"
    "        case HWSEC_DISK: // 硬盘\n"
    "            Rectangle(hdc, l, t + Scale(2), r, b);\n"
    "            Ellipse(hdc, cx - Scale(2), cy - Scale(1), cx + Scale(2), cy + Scale(2));\n"
    "            break;\n"
    "        case HWSEC_DISP: // 显示器（display）\n"
    "            Rectangle(hdc, l, t, r, b - Scale(4));\n"
    "            MoveToEx(hdc, cx, b - Scale(4), NULL); LineTo(hdc, cx, b);\n"
    "            MoveToEx(hdc, cx - Scale(3), b, NULL); LineTo(hdc, cx + Scale(3), b);\n"
    "            break;\n"
    "        case HWSEC_NIC: // 网卡（WiFi 三层弧）\n"
    "            Arc(hdc, l, t + Scale(2), r, b + Scale(2), l, cy, r, cy);\n"
    "            Arc(hdc, l + Scale(2), t + Scale(4), r - Scale(2), b, l + Scale(2), cy, r - Scale(2), cy);\n"
    "            Ellipse(hdc, cx - Scale(1), cy + Scale(1), cx + Scale(1), cy + Scale(3));\n"
    "            break;\n"
    "        case HWSEC_AUDIO: // 扬声器\n"
    "            MoveToEx(hdc, l, cy - Scale(2), NULL); LineTo(hdc, l + Scale(3), cy - Scale(2));\n"
    "            LineTo(hdc, l + Scale(6), t);\n"
    "            LineTo(hdc, l + Scale(6), b);\n"
    "            LineTo(hdc, l + Scale(3), cy + Scale(2));\n"
    "            LineTo(hdc, l, cy + Scale(2));\n"
    "            LineTo(hdc, l, cy - Scale(2));\n"
    "            break;\n"
    "        case HWSEC_USB: // USB 三叉\n"
    "            MoveToEx(hdc, cx, t, NULL); LineTo(hdc, cx, cy);\n"
    "            MoveToEx(hdc, cx, cy, NULL); LineTo(hdc, l + Scale(2), b - Scale(2));\n"
    "            Ellipse(hdc, l, b - Scale(3), l + Scale(3), b);\n"
    "            MoveToEx(hdc, cx, cy, NULL); LineTo(hdc, r - Scale(2), b - Scale(2));\n"
    "            Ellipse(hdc, r - Scale(3), b - Scale(3), r, b);\n"
    "            break;\n"
    "    }\n"
    "    SelectObject(hdc, oldPen);\n"
    "    SelectObject(hdc, oldBr);\n"
    "    DeleteObject(pen);\n"
    "}\n"
    "\n"
    "// 段标题（玻璃风磨砂带 + 左侧语义图标，替代原纯色竖条）\n"
    "static int DrawHwSectionTitle(HDC hdc, int xLeft, int xRight, int y, const WCHAR* title, int sectionId) {\n"
    "    int rowH = Scale(g_hwRowH);\n"
    "    RECT tr = {xLeft, y, xRight, y + rowH};\n"
    "    if (g_pg) {\n"
    "        GraphicsPath sbp; RoundRectPath(&sbp, (float)xLeft, (float)y, (float)(xRight - xLeft), (float)rowH, (float)Scale(6));\n"
    "        SolidBrush sb(Color(12, 255, 255, 255));\n"
    "        g_pg->FillPath(&sb, &sbp);\n"
    "        g_pg->Flush(FlushIntentionSync);\n"
    "    } else {\n"
    "        HBRUSH bg = CreateSolidBrush(C_BG);\n"
    "        FillRect(hdc, &tr, bg);\n"
    "        DeleteObject(bg);\n"
    "    }\n"
    "    // 左侧语义图标（替代原色条）\n"
    "    DrawHwSectionIcon(hdc, xLeft, y, rowH, sectionId);\n"
    "    SetBkMode(hdc, TRANSPARENT);\n"
    "    HFONT ft = MakeFont(Scale(14), FW_SEMIBOLD);\n"
    "    HFONT of = (HFONT)SelectObject(hdc, ft);\n"
    "    SetTextColor(hdc, RGB(245, 246, 255));\n"
    "    RECT r2 = {xLeft + Scale(22), y, xRight, y + rowH};\n"
    "    DrawTextW(hdc, title, -1, &r2, DT_LEFT | DT_VCENTER | DT_SINGLELINE);\n"
    "    SelectObject(hdc, of);\n"
    "    DeleteObject(ft);\n"
    "    return y + rowH + Scale(g_hwSecGap);\n"
    "}\n"
)
add(old_title, new_title, "section title with icon")

# ===== F4: 更新 11 个 DrawHwSectionTitle 调用点，传入 sectionId =====
section_calls = [
    (r'y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u64cd\u4f5c\u7cfb\u7edf");',
     r'y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u64cd\u4f5c\u7cfb\u7edf", HWSEC_OS);',
     "OS call"),
    (r'y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u8ba1\u7b97\u673a");',
     r'y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u8ba1\u7b97\u673a", HWSEC_MACHINE);',
     "MACHINE call"),
    (r'y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u4e3b\u677f / BIOS");',
     r'y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u4e3b\u677f / BIOS", HWSEC_BOARD);',
     "BOARD call"),
    (r'y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u5904\u7406\u5668");',
     r'y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u5904\u7406\u5668", HWSEC_CPU);',
     "CPU call"),
    (r'y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u5185\u5b58");',
     r'y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u5185\u5b58", HWSEC_MEM);',
     "MEM call"),
    (r'y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u663e\u5361");',
     r'y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u663e\u5361", HWSEC_GPU);',
     "GPU call"),
    (r'y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u786c\u76d8");',
     r'y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u786c\u76d8", HWSEC_DISK);',
     "DISK call"),
    (r'y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u663e\u793a\u5668");',
     r'y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u663e\u793a\u5668", HWSEC_DISP);',
     "DISP call"),
    (r'y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u7f51\u5361");',
     r'y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u7f51\u5361", HWSEC_NIC);',
     "NIC call"),
    (r'y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u97f3\u9891\u8bbe\u5907");',
     r'y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u97f3\u9891\u8bbe\u5907", HWSEC_AUDIO);',
     "AUDIO call"),
    (r'y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"USB \u5916\u8bbe");',
     r'y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"USB \u5916\u8bbe", HWSEC_USB);',
     "USB call"),
]
for old, new, note in section_calls:
    add(old, new, note)

# ===== F5: 按钮配色：次按钮（停用）改成淡薰衣草底 + 浅蓝边 =====
add("    if (primary) {\n"
    "        fill   = hovered ? C_PRIMARY_H : C_PRIMARY;\n"
    "        border = fill;\n"
    "        textc  = RGB(255, 255, 255);\n"
    "    } else {\n"
    "        fill   = hovered ? C_BTN_SECONDARY_H : C_BTN_SECONDARY;\n"
    "        border = hovered ? C_BTN_BORDER_H : C_BTN_BORDER;\n"
    "        textc  = C_TEXT;\n"
    "    }",
    "    if (primary) {\n"
    "        fill   = hovered ? C_PRIMARY_H : C_PRIMARY;\n"
    "        border = fill;\n"
    "        textc  = RGB(255, 255, 255);\n"
    "    } else {\n"
    "        // 玻璃风：次按钮底改淡薰衣草，边改浅蓝灰，融进玻璃质感（深字保留可读）\n"
    "        fill   = hovered ? RGB(190, 200, 235) : RGB(220, 228, 245);\n"
    "        border = hovered ? RGB(160, 180, 220) : RGB(190, 200, 230);\n"
    "        textc  = C_TEXT;\n"
    "    }",
    "button secondary color")

# ---- 执行 + 断言 ----
fails = []
for old, new, note in reps:
    cnt = s.count(old)
    if cnt != 1:
        fails.append((note, cnt))
        continue
    s = s.replace(old, new, 1)

if fails:
    print("ABORT - 以下替换未命中恰好 1 次：")
    for note, cnt in fails:
        print("  [%s] count=%d" % (note, cnt))
    sys.exit(1)

with io.open(PATH, "w", encoding="utf-8") as f:
    f.write(s)
print("ALL %d PATCHES APPLIED OK" % len(reps))