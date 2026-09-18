# -*- coding: utf-8 -*-
# 把 WinSuiteX 主程序视觉对齐到 WinSuiteX_glass4 demo（glass4 风格玻璃拟态）
# 每项替换都断言恰好出现 1 次，避免误伤。
import io, sys

PATH = r"D:\project\WorkBuddy\内置工具\WinSuiteX\main.cpp"
with io.open(PATH, "r", encoding="utf-8") as f:
    s = f.read()

reps = []

def add(old, new, note):
    reps.append((old, new, note))

# R1: 压暗渐变，对齐 glass4（更深饱和 -> 白字对比度更高，玻璃感更强）
add("RGB(126, 92, 222);  // #7E5CDE 紫",
    "RGB(124, 58, 237);  // #7C3AED 紫（对齐 glass4）", "gradient top")
add("RGB(56, 132, 214);  // #3884D6 蓝",
    "RGB(37, 99, 235);   // #2563EB 蓝（对齐 glass4）", "gradient bottom")

# R2: DrawCard 卡体极低透（glass4 风格）
add("        // 磨砂半透卡体（白低透 + 垂直渐变光泽）\n"
    "        LinearGradientBrush cb(RectF((float)r->left, (float)r->top, (float)cw, (float)ch),\n"
    "            Color(150, 255, 255, 255), Color(130, 255, 255, 255), LinearGradientModeVertical);",
    "        // 磨砂半透卡体（glass4 风格：极低透白 + 垂直渐变光泽，紫蓝渐变透出）\n"
    "        LinearGradientBrush cb(RectF((float)r->left, (float)r->top, (float)cw, (float)ch),\n"
    "            Color(60, 255, 255, 255), Color(40, 255, 255, 255), LinearGradientModeVertical);",
    "card alpha")
add("            : Color(95, 255, 255, 255);\n        Pen bp(bc, 1.0f);",
    "            : Color(130, 255, 255, 255);\n        Pen bp(bc, 1.0f);",
    "card border normal")

# R3: DrawGlassPanel 面板极低透（密集页略增一点体量，仍明显透出渐变）
add("        LinearGradientBrush cb(RectF((float)r->left, (float)r->top, (float)cw, (float)ch),\n"
    "            Color(150, 255, 255, 255), Color(130, 255, 255, 255), LinearGradientModeVertical);\n"
    "        GraphicsPath p; RoundRectPath(&p, (float)r->left, (float)r->top, (float)cw, (float)ch, (float)rad);\n"
    "        g_pg->FillPath(&cb, &p);\n"
    "        Pen bp(Color(120, 255, 255, 255), 1.0f);",
    "        LinearGradientBrush cb(RectF((float)r->left, (float)r->top, (float)cw, (float)ch),\n"
    "            Color(78, 255, 255, 255), Color(54, 255, 255, 255), LinearGradientModeVertical);\n"
    "        GraphicsPath p; RoundRectPath(&p, (float)r->left, (float)r->top, (float)cw, (float)ch, (float)rad);\n"
    "        g_pg->FillPath(&cb, &p);\n"
    "        Pen bp(Color(130, 255, 255, 255), 1.0f);",
    "panel alpha")

# R4: 卡片文字 深 -> 白/浅
add('DrawCardText(hdc, L"\\u7ecf\\u5178\\u83dc\\u5355", &nR, Scale(13), FW_SEMIBOLD, C_TEXT,',
    'DrawCardText(hdc, L"\\u7ecf\\u5178\\u83dc\\u5355", &nR, Scale(13), FW_SEMIBOLD, RGB(255, 255, 255),',
    "classic menu name")
add("DrawCardText(hdc, statusText, &dR, Scale(11), FW_NORMAL, on ? C_PRIMARY : C_DESC,",
    "DrawCardText(hdc, statusText, &dR, Scale(11), FW_NORMAL, on ? RGB(150, 240, 190) : RGB(205, 210, 230),",
    "status text")
add("DrawCardText(hdc, f->displayName, &nR, Scale(16), FW_SEMIBOLD, C_TEXT,",
    "DrawCardText(hdc, f->displayName, &nR, Scale(16), FW_SEMIBOLD, RGB(255, 255, 255),",
    "feature name")
add("DrawCardText(hdc, f->description, &dR, Scale(15), FW_NORMAL, C_DESC,",
    "DrawCardText(hdc, f->description, &dR, Scale(15), FW_NORMAL, RGB(220, 228, 245),",
    "feature desc")

# R5: 硬件页 标签:值 文字 深 -> 浅
add("    HFONT ofl = (HFONT)SelectObject(hdc, fl);\n"
    "    SetTextColor(hdc, C_SUBTITLE);\n"
    "    RECT lr = {xLeft, y, xLeft + Scale(96), y + rowH};",
    "    HFONT ofl = (HFONT)SelectObject(hdc, fl);\n"
    "    SetTextColor(hdc, RGB(200, 208, 230));\n"
    "    RECT lr = {xLeft, y, xLeft + Scale(96), y + rowH};",
    "hw label")
add("    HFONT ofv = (HFONT)SelectObject(hdc, fv);\n"
    "    SetTextColor(hdc, C_TEXT);\n"
    "    RECT vr = {xLeft + Scale(96), y, xRight, y + rowH};",
    "    HFONT ofv = (HFONT)SelectObject(hdc, fv);\n"
    "    SetTextColor(hdc, RGB(238, 242, 252));\n"
    "    RECT vr = {xLeft + Scale(96), y, xRight, y + rowH};",
    "hw value")

# R6: 硬件段标题 底色 -> 半透白磨砂带；段标题文字 -> 白
add("    HBRUSH bg = CreateSolidBrush(C_BG);\n"
    "    FillRect(hdc, &tr, bg);\n"
    "    DeleteObject(bg);\n"
    "    // 左侧主色竖条",
    "    if (g_pg) {\n"
    "        GraphicsPath sbp; RoundRectPath(&sbp, (float)xLeft, (float)y, (float)(xRight - xLeft), (float)rowH, (float)Scale(6));\n"
    "        SolidBrush sb(Color(42, 255, 255, 255));\n"
    "        g_pg->FillPath(&sb, &sbp);\n"
    "        g_pg->Flush(FlushIntentionSync);\n"
    "    } else {\n"
    "        HBRUSH bg = CreateSolidBrush(C_BG);\n"
    "        FillRect(hdc, &tr, bg);\n"
    "        DeleteObject(bg);\n"
    "    }\n"
    "    // 左侧主色竖条",
    "hw section bg")
add("    SetTextColor(hdc, C_TITLE);\n    RECT r2 = {xLeft + Scale(10), y, xRight, y + rowH};",
    "    SetTextColor(hdc, RGB(245, 246, 255));\n    RECT r2 = {xLeft + Scale(10), y, xRight, y + rowH};",
    "hw section title text")

# R7: 磁盘页 标题/计数/空态/表头 深 -> 浅
add("        HFONT hf = MakeFont(Scale(15), FW_SEMIBOLD);\n"
    "        HFONT of = (HFONT)SelectObject(hdc, hf);\n"
    "        SetTextColor(hdc, C_TITLE);",
    "        HFONT hf = MakeFont(Scale(15), FW_SEMIBOLD);\n"
    "        HFONT of = (HFONT)SelectObject(hdc, hf);\n"
    "        SetTextColor(hdc, RGB(245, 246, 255));",
    "disk heading")
add(r'        swprintf(cnt, _countof(cnt), L"\u5171 %d \u4e2a\u5206\u533a", g_diskCount);' + "\n"
    "        HFONT cf = MakeFont(Scale(13), FW_NORMAL);\n"
    "        HFONT ocf = (HFONT)SelectObject(hdc, cf);\n"
    "        SetTextColor(hdc, C_SUBTITLE);",
    r'        swprintf(cnt, _countof(cnt), L"\u5171 %d \u4e2a\u5206\u533a", g_diskCount);' + "\n"
    "        HFONT cf = MakeFont(Scale(13), FW_NORMAL);\n"
    "        HFONT ocf = (HFONT)SelectObject(hdc, cf);\n"
    "        SetTextColor(hdc, RGB(205, 210, 230));",
    "disk count")
add(r'        HFONT f = MakeFont(Scale(14), FW_NORMAL);' + "\n"
    "        HFONT of = (HFONT)SelectObject(hdc, f);\n"
    "        SetTextColor(hdc, C_SUBTITLE);\n"
    "        SetBkMode(hdc, TRANSPARENT);\n"
    "        RECT r = {Scale(MARGIN + SHADOW_PAD),\n"
    "                  Scale(TITLE_H + TAB_H + DISK_PAGE_HEAD + SHADOW_PAD),\n"
    "                  Scale(WIN_W - MARGIN + SHADOW_PAD),\n"
    "                  Scale(TITLE_H + TAB_H + DISK_PAGE_HEAD + 40 + SHADOW_PAD)};\n"
    r'        DrawTextW(hdc, L"\u6682\u65e0\u53ef\u8bbf\u95ee\u7684\u78c1\u76d8\u5206\u533a", -1, &r,',
    r'        HFONT f = MakeFont(Scale(14), FW_NORMAL);' + "\n"
    "        HFONT of = (HFONT)SelectObject(hdc, f);\n"
    "        SetTextColor(hdc, RGB(205, 210, 230));\n"
    "        SetBkMode(hdc, TRANSPARENT);\n"
    "        RECT r = {Scale(MARGIN + SHADOW_PAD),\n"
    "                  Scale(TITLE_H + TAB_H + DISK_PAGE_HEAD + SHADOW_PAD),\n"
    "                  Scale(WIN_W - MARGIN + SHADOW_PAD),\n"
    "                  Scale(TITLE_H + TAB_H + DISK_PAGE_HEAD + 40 + SHADOW_PAD)};\n"
    r'        DrawTextW(hdc, L"\u6682\u65e0\u53ef\u8bbf\u95ee\u7684\u78c1\u76d8\u5206\u533a", -1, &r,',
    "disk empty msg")
add("        SetTextColor(hdc, C_SUBTITLE);\n"
    "        SetBkMode(hdc, TRANSPARENT);\n"
    "        const WCHAR* heads[7] = {",
    "        SetTextColor(hdc, RGB(205, 210, 230));\n"
    "        SetBkMode(hdc, TRANSPARENT);\n"
    "        const WCHAR* heads[7] = {",
    "disk header")

# R8: 磁盘行文字
add("        SetTextColor(hdc, d->ok ? C_TEXT : C_SUBTITLE);",
    "        SetTextColor(hdc, d->ok ? RGB(238, 242, 252) : RGB(200, 208, 230));",
    "disk rows")

# R9: 磁盘选中详情框 -> 半透白磨砂
add("        DrawRoundedRect(hdc, &dr, Scale(8), C_CARD_BG, C_BORDER);",
    "        if (g_pg) {\n"
    "            GraphicsPath ddp; RoundRectPath(&ddp, (float)dr.left, (float)dr.top, (float)(dr.right - dr.left), (float)(dr.bottom - dr.top), (float)Scale(8));\n"
    "            SolidBrush dbr(Color(55, 255, 255, 255));\n"
    "            g_pg->FillPath(&dbr, &ddp);\n"
    "            Pen dpen(Color(120, 255, 255, 255), 1.0f);\n"
    "            GraphicsPath ddp2; RoundRectPath(&ddp2, (float)dr.left, (float)dr.top, (float)(dr.right - dr.left), (float)(dr.bottom - dr.top), (float)Scale(8));\n"
    "            g_pg->DrawPath(&dpen, &ddp2);\n"
    "            g_pg->Flush(FlushIntentionSync);\n"
    "        } else {\n"
    "            DrawRoundedRect(hdc, &dr, Scale(8), C_CARD_BG, C_BORDER);\n"
    "        }",
    "disk detail box")

# R10: 磁盘选中详情文字
add("            SetTextColor(hdc, k == 0 ? C_TITLE : C_DESC);",
    "            SetTextColor(hdc, k == 0 ? RGB(245, 246, 255) : RGB(220, 228, 245));",
    "disk detail text")

# R11: 磨砂标题栏带（glass4 风格）
add("        // ====== 自定义标题栏 ======",
    "        // glass4 风格：顶部磨砂标题栏带（极低透白，紫蓝渐变透出）\n"
    "        if (g_pg) {\n"
    "            int ty = sp, bandH = Scale(TITLE_H + SHADOW_PAD) - sp;\n"
    "            LinearGradientBrush tbr(RectF((REAL)sp, (REAL)ty, (REAL)(W - 2 * sp), (REAL)bandH),\n"
    "                Color(40, 255, 255, 255), Color(16, 255, 255, 255), LinearGradientModeVertical);\n"
    "            g_pg->FillRectangle(&tbr, (REAL)sp, (REAL)ty, (REAL)(W - 2 * sp), (REAL)bandH);\n"
    "            g_pg->Flush(FlushIntentionSync);\n"
    "        }\n"
    "\n"
    "        // ====== 自定义标题栏 ======",
    "title band")

# R12: 窗口 1px 边 -> 半透白（glass4 柔和感）
add("        // 窗口 1px 清晰圆角外边框（最后描边，压在内容之上，不被底色覆盖）\n"
    "        {\n"
    "            RECT wf = {sp, sp, W - sp, H - sp};\n"
    "            StrokeRoundedRect(hdc, &wf, Scale(WIN_RADIUS), C_GLASS_BORDER, 1);\n"
    "        }",
    "        // 窗口 1px 淡白圆角外边框（最后描边；glass4 风格：半透白，柔和融入渐变）\n"
    "        {\n"
    "            if (g_pg) {\n"
    "                Pen wp(Color(130, 255, 255, 255), 1.0f);\n"
    "                GraphicsPath wpp;\n"
    "                RoundRectPath(&wpp, (float)sp + 0.5f, (float)sp + 0.5f,\n"
    "                              (float)(W - 2 * sp - 1), (float)(H - 2 * sp - 1), (float)Scale(WIN_RADIUS));\n"
    "                g_pg->DrawPath(&wp, &wpp);\n"
    "                g_pg->Flush(FlushIntentionSync);\n"
    "            } else {\n"
    "                RECT wf = {sp, sp, W - sp, H - sp};\n"
    "                StrokeRoundedRect(hdc, &wf, Scale(WIN_RADIUS), C_GLASS_BORDER, 1);\n"
    "            }\n"
    "        }",
    "window border")

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
