# -*- coding: utf-8 -*-
# 第四轮：右键窗口按钮/徽章状态与颜色对齐 demo
#  - "停用" 次按钮：调成更弱的描边款（demo 几乎透明 + 极细边）
#  - "启用" 主按钮：蓝 → 紫色（demo 主色 = 紫，与"已启用"徽章统一）
#  - "已启用" 徽章：绿底绿字 → 紫底白字（与"启用"按钮同色）
#  - "未启用" 徽章：保持灰底灰字
import io, sys

PATH = r"D:\project\WorkBuddy\内置工具\WinSuiteX\main.cpp"
with io.open(PATH, "r", encoding="utf-8") as f:
    s = f.read()

reps = []
def add(old, new, note):
    reps.append((old, new, note))

# ===== F1: DrawButton — 主按钮蓝改紫，次按钮降饱和 =====
old_btn = (
    "// 按钮：圆角 8px；主操作（启用）蓝底白字，次操作（停用）白底描边\n"
    "static void DrawButton(HDC hdc, RECT* r, const WCHAR* text,\n"
    "                       BOOL hovered, BOOL primary) {\n"
    "    COLORREF fill, border, textc;\n"
    "    if (primary) {\n"
    "        fill   = hovered ? C_PRIMARY_H : C_PRIMARY;\n"
    "        border = fill;\n"
    "        textc  = RGB(255, 255, 255);\n"
    "    } else {\n"
    "        // 玻璃风：次按钮底改淡薰衣草，边改浅蓝灰，融进玻璃质感（深字保留可读）\n"
    "        fill   = hovered ? RGB(190, 200, 235) : RGB(220, 228, 245);\n"
    "        border = hovered ? RGB(160, 180, 220) : RGB(190, 200, 230);\n"
    "        textc  = C_TEXT;\n"
    "    }\n"
    "    DrawRoundedRect(hdc, r, Scale(6), fill, border);\n"
)
new_btn = (
    "// 按钮：圆角 8px；主操作（启用）紫底白字（demo 启用态=紫色，与已启用徽章统一），\n"
    "// 次操作（停用）极淡描边款（demo 风格，几乎隐入玻璃底，仅靠细边识别）\n"
    "static void DrawButton(HDC hdc, RECT* r, const WCHAR* text,\n"
    "                       BOOL hovered, BOOL primary) {\n"
    "    COLORREF fill, border, textc;\n"
    "    if (primary) {\n"
    "        // 主按钮：紫（demo 启用态=紫）；hover 加深\n"
    "        fill   = hovered ? RGB(79, 70, 229) : RGB(99, 102, 241);\n"
    "        border = fill;\n"
    "        textc  = RGB(255, 255, 255);\n"
    "    } else {\n"
    "        // 次按钮：几乎隐入玻璃底（fill 接近面板色），仅细边区分（demo 风格）\n"
    "        fill   = hovered ? RGB(225, 230, 248) : RGB(245, 247, 252);\n"
    "        border = hovered ? RGB(140, 160, 210) : RGB(180, 195, 225);\n"
    "        textc  = C_TEXT;\n"
    "    }\n"
    "    DrawRoundedRect(hdc, r, Scale(6), fill, border);\n"
)
add(old_btn, new_btn, "button primary purple / secondary outlined")

# ===== F2: DrawStatusBadge — 已启用徽章改紫色 =====
old_badge = (
    "static void DrawStatusBadge(HDC hdc, RECT* r, BOOL installed) {\n"
    "    COLORREF bg = installed ? C_GREEN_BG : C_BADGE_NO_BG;\n"
    "    COLORREF fg = installed ? C_GREEN_DARK : C_BADGE_NO_FG;\n"
    "    DrawRoundedRect(hdc, r, Scale(11), bg, bg);\n"
)
new_badge = (
    "static void DrawStatusBadge(HDC hdc, RECT* r, BOOL installed) {\n"
    "    // 启用态徽章=紫色实心+白字，与启用按钮同色（demo 启用态视觉语言统一）\n"
    "    COLORREF bg = installed ? RGB(99, 102, 241) : C_BADGE_NO_BG;\n"
    "    COLORREF fg = installed ? RGB(255, 255, 255) : C_BADGE_NO_FG;\n"
    "    DrawRoundedRect(hdc, r, Scale(11), bg, bg);\n"
)
add(old_badge, new_badge, "status badge installed = purple")

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