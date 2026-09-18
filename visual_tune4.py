# -*- coding: utf-8 -*-
# 第六轮：对齐 glass4 源码（demo）+ 修悬停 bug
#  1) 卡 alpha 42/26 -> 60/34（demo 原值）
#  2) 卡边框常态 130 -> 90（demo 原值）
#  3) 悬停检测 g_btnRect -> g_cardRect：让阴影"浮起"+ 边框"亮起"在整张卡都生效
import io, sys

PATH = r"D:\project\WorkBuddy\内置工具\WinSuiteX\main.cpp"
with io.open(PATH, "r", encoding="utf-8") as f:
    s = f.read()

reps = []
def add(old, new, note):
    reps.append((old, new, note))

# ===== F1: 卡 alpha 对齐 demo (60/34) =====
add("            Color(42, 255, 255, 255), Color(26, 255, 255, 255), LinearGradientModeVertical);",
    "            Color(60, 255, 255, 255), Color(34, 255, 255, 255), LinearGradientModeVertical);",
    "card alpha -> glass4 source (60/34)")

# ===== F2: 卡边框常态对齐 demo (130 -> 90) =====
add("            : Color(70, 255, 255, 255);\n        Pen bp(bc, 1.0f);",
    "            : Color(90, 255, 255, 255);\n        Pen bp(bc, 1.0f);",
    "card border normal -> glass4 source (90)")

# ===== F3: 悬停检测用整张卡的矩形（而不是仅按钮），让"动感"生效 =====
add("        for (int i = 0; i < g_count; i++) {\n"
    "            BOOL h = PtInRect(&g_btnRect[i], pt);\n"
    "            if (h != g_hover[i]) { g_hover[i] = h; changed = TRUE; }\n"
    "        }",
    "        for (int i = 0; i < g_count; i++) {\n"
    "            // 玻璃风：悬停检测用整张卡的矩形（demo 风格），），不是仅按钮——\n"
    "            // 让阴影\"浮起\"+ 边框\"亮起\"在整张卡都生效（Z.W. 反馈缺动感）\n"
    "            BOOL h = PtInRect(&g_cardRect[i], pt);\n"
    "            if (h != g_hover[i]) { g_hover[i] = h; changed = TRUE; }\n"
    "        }",
    "hover detect on card rect not btn rect")

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