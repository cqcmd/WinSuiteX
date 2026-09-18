# -*- coding: utf-8 -*-
# 修复：阴影缓存由"单一尺寸"改为"(宽,高,hover) 多条目"，避免菜单页两类不同尺寸卡片
# 交替调用时每帧反复重算高斯模糊（性能隐患）。
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

# ---- A) 全局缓存变量 ----
anchor_glob = (
    "static Bitmap* g_shadow[2] = { 0 };      // 0=常态, 1=hover\n"
    "static bool g_shadowBuilt = false;\n"
    "static int g_shadowW = 0, g_shadowH = 0;\n"
)
glob_new = (
    "static struct ShadowEnt { int w, h, hov; Bitmap* bmp; } g_shadows[8];\n"
    "static int g_shadowCount = 0;\n"
)
src = replace_once(src, anchor_glob, glob_new, "shadow globals")

# ---- B) BuildShadows -> GetShadow（多条目缓存） ----
anchor_build = (
    "static void BuildShadows(int cw, int ch) {\n"
    "    if (g_shadowBuilt && g_shadowW == cw && g_shadowH == ch) return;\n"
    "    if (g_shadow[0]) { delete g_shadow[0]; g_shadow[0] = NULL; }\n"
    "    if (g_shadow[1]) { delete g_shadow[1]; g_shadow[1] = NULL; }\n"
    "    RectF cr(0.0f, 0.0f, (float)cw, (float)ch);\n"
    "    g_shadow[0] = MakeShadow(cr, (float)Scale(WIN_RADIUS), 16, Color(70, 0, 0, 0), 12);\n"
    "    g_shadow[1] = MakeShadow(cr, (float)Scale(WIN_RADIUS), 18, Color(110, 0, 0, 0), 18);\n"
    "    g_shadowBuilt = true; g_shadowW = cw; g_shadowH = ch;\n"
    "}\n"
)
build_new = (
    "// 阴影按 (宽,高,hover) 缓存：同一尺寸只算一次高斯模糊，避免不同尺寸卡片交替调用时每帧重算。\n"
    "static Bitmap* GetShadow(int cw, int ch, BOOL hovered) {\n"
    "    int hov = hovered ? 1 : 0;\n"
    "    for (int i = 0; i < g_shadowCount; i++)\n"
    "        if (g_shadows[i].w == cw && g_shadows[i].h == ch && g_shadows[i].hov == hov)\n"
    "            return g_shadows[i].bmp;\n"
    "    if (g_shadowCount >= 8) return NULL;   // 缓存满则放弃阴影（不应发生）\n"
    "    int blur = hov ? 18 : 16;\n"
    "    RectF cr(0.0f, 0.0f, (float)cw, (float)ch);\n"
    "    Bitmap* b = MakeShadow(cr, (float)Scale(WIN_RADIUS), blur,\n"
    "                           hov ? Color(110, 0, 0, 0) : Color(70, 0, 0, 0),\n"
    "                           hov ? 18.0f : 12.0f);\n"
    "    g_shadows[g_shadowCount].w = cw;\n"
    "    g_shadows[g_shadowCount].h = ch;\n"
    "    g_shadows[g_shadowCount].hov = hov;\n"
    "    g_shadows[g_shadowCount].bmp = b;\n"
    "    g_shadowCount++;\n"
    "    return b;\n"
    "}\n"
)
src = replace_once(src, anchor_build, build_new, "GetShadow")

# ---- C) DrawCard 调用处 ----
anchor_use = (
    "        // 真实高斯软阴影（缓存，叠在渐变上 → 玻璃悬浮感）\n"
    "        BuildShadows(cw, ch);\n"
    "        int blur = hovered ? 18 : 16;\n"
    "        int pad = blur * 3 + 6;\n"
    "        Bitmap* sh = g_shadow[hovered ? 1 : 0];\n"
    "        if (sh) g_pg->DrawImage(sh, (INT)(r->left - pad), (INT)(r->top - pad));\n"
)
use_new = (
    "        // 真实高斯软阴影（按尺寸缓存，叠在渐变上 → 玻璃悬浮感）\n"
    "        int blur = hovered ? 18 : 16;\n"
    "        int pad = blur * 3 + 6;\n"
    "        Bitmap* sh = GetShadow(cw, ch, hovered);\n"
    "        if (sh) g_pg->DrawImage(sh, (INT)(r->left - pad), (INT)(r->top - pad));\n"
)
src = replace_once(src, anchor_use, use_new, "DrawCard use")

# ---- D) 退出清理 ----
anchor_clean = (
    "    for (int _s = 0; _s < 2; _s++) if (g_shadow[_s]) { delete g_shadow[_s]; g_shadow[_s] = NULL; }\n"
)
clean_new = (
    "    for (int _s = 0; _s < g_shadowCount; _s++) if (g_shadows[_s].bmp) { delete g_shadows[_s].bmp; g_shadows[_s].bmp = NULL; }\n"
    "    g_shadowCount = 0;\n"
)
src = replace_once(src, anchor_clean, clean_new, "shadow cleanup")

with io.open(SRC, "w", encoding="utf-8", errors="surrogateescape") as f:
    f.write(src)
print("shadow cache patch OK")
