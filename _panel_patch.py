# -*- coding: utf-8 -*-
# 第二阶段：硬件/磁盘页大面板改 GDI+ 磨砂半透（让渐变透出），替换掉实心白面板+平涂阴影。
import io

SRC = "main.cpp"

def read(p):
    with io.open(p, "r", encoding="utf-8", errors="surrogateescape") as f:
        return f.read()

def replace_once(s, old, new, label):
    n = s.count(old)
    if n != 1:
        raise SystemExit("FAIL [%s] expected 1, found %d" % (label, n))
    return s.replace(old, new, 1)

src = read(SRC)

# ---- 1) 在 DrawCard 之后加入 DrawGlassPanel ----
anchor_card_end = (
    "    // 左侧 3px 圆角类型彩条（GDI 绘制，位于卡内）\n"
    "    RECT ar = {r->left, r->top + Scale(14),\n"
    "               r->left + Scale(3), r->bottom - Scale(14)};\n"
    "    DrawRoundedRect(hdc, &ar, Scale(2), accent, accent);\n"
    "}\n"
)
panel_helper = anchor_card_end + (
    "\n"
    "// 整页磨砂玻璃面板：GDI+ 半透白（让紫蓝渐变透出）+ AA 圆角亮边\n"
    "static void DrawGlassPanel(HDC hdc, RECT* r, int rad) {\n"
    "    int cw = r->right - r->left, ch = r->bottom - r->top;\n"
    "    if (g_pg) {\n"
    "        LinearGradientBrush cb(RectF((float)r->left, (float)r->top, (float)cw, (float)ch),\n"
    "            Color(205, 255, 255, 255), Color(185, 255, 255, 255), LinearGradientModeVertical);\n"
    "        GraphicsPath p; RoundRectPath(&p, (float)r->left, (float)r->top, (float)cw, (float)ch, (float)rad);\n"
    "        g_pg->FillPath(&cb, &p);\n"
    "        Pen bp(Color(120, 255, 255, 255), 1.0f);\n"
    "        GraphicsPath p2; RoundRectPath(&p2, (float)r->left, (float)r->top, (float)cw, (float)ch, (float)rad);\n"
    "        g_pg->DrawPath(&bp, &p2);\n"
    "        g_pg->Flush(FlushIntentionSync);\n"
    "    } else {\n"
    "        DrawRoundedRect(hdc, r, rad, C_PANEL_GLASS, C_GLASS_BORDER, 1);\n"
    "    }\n"
    "}\n"
)
src = replace_once(src, anchor_card_end, panel_helper, "add DrawGlassPanel")

# ---- 2) 磁盘页面板 ----
anchor_disk = (
    "            RECT dp = {Scale(MARGIN + SHADOW_PAD),\n"
    "                       Scale(TITLE_H + TAB_H + 8 + SHADOW_PAD),\n"
    "                       Scale(WIN_W - MARGIN + SHADOW_PAD),\n"
    "                       g_totalH - Scale(FOOTER_H) + sp};\n"
    "            RECT dsh = {dp.left + Scale(1), dp.top + Scale(3),\n"
    "                        dp.right + Scale(1), dp.bottom + Scale(3)};\n"
    "            DrawRoundedRect(hdc, &dsh, Scale(WIN_RADIUS), C_GLASS_SH, C_GLASS_SH, 1);\n"
    "            DrawRoundedRect(hdc, &dp, Scale(WIN_RADIUS), C_PANEL_GLASS, C_GLASS_BORDER, 1);\n"
    "            DrawDiskPage(hdc, sp);\n"
)
disk_new = (
    "            RECT dp = {Scale(MARGIN + SHADOW_PAD),\n"
    "                       Scale(TITLE_H + TAB_H + 8 + SHADOW_PAD),\n"
    "                       Scale(WIN_W - MARGIN + SHADOW_PAD),\n"
    "                       g_totalH - Scale(FOOTER_H) + sp};\n"
    "            DrawGlassPanel(hdc, &dp, Scale(WIN_RADIUS));\n"
    "            DrawDiskPage(hdc, sp);\n"
)
src = replace_once(src, anchor_disk, disk_new, "disk panel")

# ---- 3) 硬件页面板 ----
anchor_hw = (
    "            RECT hp = {Scale(MARGIN + SHADOW_PAD),\n"
    "                       Scale(TITLE_H + TAB_H + 8 + SHADOW_PAD),\n"
    "                       Scale(WIN_W - MARGIN + SHADOW_PAD),\n"
    "                       g_totalH - Scale(FOOTER_H) + sp};\n"
    "            RECT hsh = {hp.left + Scale(1), hp.top + Scale(3),\n"
    "                        hp.right + Scale(1), hp.bottom + Scale(3)};\n"
    "            DrawRoundedRect(hdc, &hsh, Scale(WIN_RADIUS), C_GLASS_SH, C_GLASS_SH, 1);\n"
    "            DrawRoundedRect(hdc, &hp, Scale(WIN_RADIUS), C_PANEL_GLASS, C_GLASS_BORDER, 1);\n"
    "            DrawHardwarePage(hdc, sp);\n"
)
hw_new = (
    "            RECT hp = {Scale(MARGIN + SHADOW_PAD),\n"
    "                       Scale(TITLE_H + TAB_H + 8 + SHADOW_PAD),\n"
    "                       Scale(WIN_W - MARGIN + SHADOW_PAD),\n"
    "                       g_totalH - Scale(FOOTER_H) + sp};\n"
    "            DrawGlassPanel(hdc, &hp, Scale(WIN_RADIUS));\n"
    "            DrawHardwarePage(hdc, sp);\n"
)
src = replace_once(src, anchor_hw, hw_new, "hardware panel")

with io.open(SRC, "w", encoding="utf-8", errors="surrogateescape") as f:
    f.write(src)
print("panel patch OK")
