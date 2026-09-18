# -*- coding: utf-8 -*-
# 将 WinSuiteX main.cpp 的绘制层迁移到 GDI+（玻璃拟态：平滑渐变 + 半透磨砂卡 + 真高斯软阴影）
# 仅改绘制层，功能逻辑（注册表/硬件/磁盘枚举/标签切换/导出/自检/COM）一律不动。
import io, sys

SRC = "main.cpp"
BUILD = "build_git.sh"

def read(p):
    with io.open(p, "r", encoding="utf-8", errors="surrogateescape") as f:
        return f.read()

def replace_once(s, old, new, label):
    n = s.count(old)
    if n != 1:
        raise SystemExit("FAIL [%s] expected 1 occurrence, found %d" % (label, n))
    return s.replace(old, new, 1)

src = read(SRC)

# ---------- 1) 顶部引入 gdiplus ----------
anchor_inc = (
    "#ifndef GET_X_LPARAM\n"
    "#define GET_X_LPARAM(lp) ((int)(short)LOWORD(lp))\n"
    "#define GET_Y_LPARAM(lp) ((int)(short)HIWORD(lp))\n"
    "#endif\n"
)
inc_new = (
    "#include <gdiplus.h>\n"
    "using namespace Gdiplus;\n"
    "\n"
    "#ifndef GET_X_LPARAM\n"
    "#define GET_X_LPARAM(lp) ((int)(short)LOWORD(lp))\n"
    "#define GET_Y_LPARAM(lp) ((int)(short)HIWORD(lp))\n"
    "#endif\n"
)
src = replace_once(src, anchor_inc, inc_new, "include gdiplus")

# ---------- 2) GDI+ 辅助基础设施（插在 C_GLASS_SH 之后） ----------
anchor_helpers = (
    "static const COLORREF C_GLASS_SH      = RGB(40, 44, 80);    // 卡片投影（深冷，落在渐变上更自然）\n"
)
helpers = anchor_helpers + (
    "\n"
    "// ---- GDI+ 玻璃渲染基础设施（移植自 glass_showcase.cpp：平滑渐变 / 半透磨砂卡 / 真高斯软阴影）----\n"
    "static ULONG_PTR g_gdipToken = 0;\n"
    "static FontFamily* g_fontFam = NULL;\n"
    "static Bitmap* g_shadow[2] = { 0 };      // 0=常态, 1=hover\n"
    "static bool g_shadowBuilt = false;\n"
    "static int g_shadowW = 0, g_shadowH = 0;\n"
    "static Graphics* g_pg = NULL;            // 当前帧 GDI+ Graphics（仅 WM_PAINT 内有效）\n"
    "\n"
    "static FontFamily& UiFontFamily() {\n"
    "    if (!g_fontFam) {\n"
    "        static const WCHAR* names[] = { L\"Microsoft YaHei\", L\"SimSun\", L\"Arial\" };\n"
    "        for (int i = 0; i < 3; i++) {\n"
    "            FontFamily* f = new FontFamily(names[i]);\n"
    "            if (f->GetLastStatus() == Ok) { g_fontFam = f; break; }\n"
    "            delete f;\n"
    "        }\n"
    "        if (!g_fontFam) g_fontFam = new FontFamily(L\"Arial\");\n"
    "    }\n"
    "    return *g_fontFam;\n"
    "}\n"
    "\n"
    "static void RoundRectPath(GraphicsPath* p, float x, float y, float w, float h, float r) {\n"
    "    if (r > w / 2.0f) r = w / 2.0f;\n"
    "    if (r > h / 2.0f) r = h / 2.0f;\n"
    "    p->StartFigure();\n"
    "    p->AddArc(x, y, 2 * r, 2 * r, 180, 90);\n"
    "    p->AddArc(x + w - 2 * r, y, 2 * r, 2 * r, 270, 90);\n"
    "    p->AddArc(x + w - 2 * r, y + h - 2 * r, 2 * r, 2 * r, 0, 90);\n"
    "    p->AddArc(x, y + h - 2 * r, 2 * r, 2 * r, 90, 90);\n"
    "    p->CloseFigure();\n"
    "}\n"
    "\n"
    "static void boxBlurH(const float* src, float* tmp, int w, int h, int r) {\n"
    "    float inv = 1.0f / (2 * r + 1);\n"
    "    for (int y = 0; y < h; y++) {\n"
    "        int row = y * w; float sum = 0;\n"
    "        for (int i = -r; i <= r; i++) { int xx = i; if (xx < 0) xx = 0; if (xx >= w) xx = w - 1; sum += src[row + xx]; }\n"
    "        for (int x = 0; x < w; x++) {\n"
    "            tmp[row + x] = sum * inv;\n"
    "            int add = x + r + 1, sub = x - r;\n"
    "            if (add >= w) add = w - 1;\n"
    "            if (sub < 0) sub = 0;\n"
    "            sum += src[row + add] - src[row + sub];\n"
    "        }\n"
    "    }\n"
    "}\n"
    "static void boxBlurV(const float* src, float* tmp, int w, int h, int r) {\n"
    "    float inv = 1.0f / (2 * r + 1);\n"
    "    for (int x = 0; x < w; x++) {\n"
    "        float sum = 0;\n"
    "        for (int i = -r; i <= r; i++) { int yy = i; if (yy < 0) yy = 0; if (yy >= h) yy = h - 1; sum += src[yy * w + x]; }\n"
    "        for (int y = 0; y < h; y++) {\n"
    "            tmp[y * w + x] = sum * inv;\n"
    "            int add = y + r + 1, sub = y - r;\n"
    "            if (add >= h) add = h - 1;\n"
    "            if (sub < 0) sub = 0;\n"
    "            sum += src[add * w + x] - src[sub * w + x];\n"
    "        }\n"
    "    }\n"
    "}\n"
    "static void BoxBlur(Bitmap* bmp, int radius) {\n"
    "    if (radius < 1) return;\n"
    "    BitmapData bd; Rect rc(0, 0, bmp->GetWidth(), bmp->GetHeight());\n"
    "    if (bmp->LockBits(&rc, ImageLockModeRead | ImageLockModeWrite, PixelFormat32bppARGB, &bd) != Ok) return;\n"
    "    int w = bd.Width, h = bd.Height;\n"
    "    UINT* px = (UINT*)bd.Scan0;\n"
    "    float *c[4]; for (int i = 0; i < 4; i++) c[i] = new float[w * h];\n"
    "    float *t0 = new float[w * h], *t1 = new float[w * h];\n"
    "    for (int i = 0; i < w * h; i++) { UINT v = px[i]; c[0][i] = v & 0xff; c[1][i] = (v >> 8) & 0xff; c[2][i] = (v >> 16) & 0xff; c[3][i] = (v >> 24) & 0xff; }\n"
    "    for (int ch = 0; ch < 4; ch++) {\n"
    "        boxBlurH(c[ch], t0, w, h, radius); boxBlurV(t0, t1, w, h, radius);\n"
    "        boxBlurH(t1, t0, w, h, radius); boxBlurV(t0, t1, w, h, radius);\n"
    "        memcpy(c[ch], t1, w * h * sizeof(float));\n"
    "    }\n"
    "    for (int i = 0; i < w * h; i++) {\n"
    "        UINT a = (UINT)(c[3][i] + 0.5), r = (UINT)(c[0][i] + 0.5), g = (UINT)(c[1][i] + 0.5), b = (UINT)(c[2][i] + 0.5);\n"
    "        if (a > 255) a = 255; if (r > 255) r = 255; if (g > 255) g = 255; if (b > 255) b = 255;\n"
    "        px[i] = (a << 24) | (b << 16) | (g << 8) | r;\n"
    "    }\n"
    "    bmp->UnlockBits(&bd);\n"
    "    for (int i = 0; i < 4; i++) delete[] c[i];\n"
    "    delete[] t0; delete[] t1;\n"
    "}\n"
    "static Bitmap* MakeShadow(RectF card, float radius, int blur, Color color, float offY) {\n"
    "    int pad = blur * 3 + 6;\n"
    "    int w = (int)(card.Width + pad * 2), h = (int)(card.Height + pad * 2);\n"
    "    Bitmap* b = new Bitmap(w, h, PixelFormat32bppARGB);\n"
    "    { Graphics g(b); SolidBrush br(color); GraphicsPath p;\n"
    "      RoundRectPath(&p, (float)pad, (float)pad + offY, card.Width, card.Height, radius);\n"
    "      g.FillPath(&br, &p); }\n"
    "    BoxBlur(b, blur);\n"
    "    return b;\n"
    "}\n"
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
src = replace_once(src, anchor_helpers, helpers, "gdi+ helpers")

# ---------- 3) wWinMain 启动 GdiplusStartup ----------
anchor_start = (
    "    g_hInst = hInstance;\n"
    "\n"
    "    // 注册自绘弹窗类（供 ShowNotice 使用，含自检模式）\n"
)
start_new = (
    "    g_hInst = hInstance;\n"
    "\n"
    "    // GDI+ 初始化（玻璃渲染依赖；gdiplus.dll 为系统自带，仍属零额外依赖）\n"
    "    GdiplusStartupInput gpsi;\n"
    "    if (GdiplusStartup(&g_gdipToken, &gpsi, NULL) != Ok) g_gdipToken = 0;\n"
    "\n"
    "    // 注册自绘弹窗类（供 ShowNotice 使用，含自检模式）\n"
)
src = replace_once(src, anchor_start, start_new, "gdiplus startup")

# ---------- 4) wWinMain 退出 GdiplusShutdown ----------
anchor_end = (
    "    MSG msg;\n"
    "    while (GetMessage(&msg, NULL, 0, 0)) {\n"
    "        TranslateMessage(&msg);\n"
    "        DispatchMessage(&msg);\n"
    "    }\n"
    "    return (int)msg.wParam;\n"
)
end_new = (
    "    MSG msg;\n"
    "    while (GetMessage(&msg, NULL, 0, 0)) {\n"
    "        TranslateMessage(&msg);\n"
    "        DispatchMessage(&msg);\n"
    "    }\n"
    "    if (g_gdipToken) GdiplusShutdown(g_gdipToken);\n"
    "    if (g_fontFam) { delete g_fontFam; g_fontFam = NULL; }\n"
    "    for (int _s = 0; _s < 2; _s++) if (g_shadow[_s]) { delete g_shadow[_s]; g_shadow[_s] = NULL; }\n"
    "    return (int)msg.wParam;\n"
)
src = replace_once(src, anchor_end, end_new, "gdiplus shutdown")

# ---------- 5) WM_PAINT 双缓冲改 32bpp DIB + GDI+ Graphics ----------
anchor_buf = (
    "        HDC     mem   = CreateCompatibleDC(hdcReal);\n"
    "        HBITMAP bmp   = mem ? CreateCompatibleBitmap(hdcReal, W, H) : NULL;\n"
    "        HBITMAP oldB  = (mem && bmp) ? (HBITMAP)SelectObject(mem, bmp) : NULL;\n"
    "        HDC hdc = (mem && bmp) ? mem : hdcReal;\n"
    "\n"
    "        // 窗口层：纯自绘柔和向下投影 + 内容区底色（先于内容绘制）\n"
    "        DrawWindowFrame(hdc, W, H);\n"
)
buf_new = (
    "        HDC     mem   = CreateCompatibleDC(hdcReal);\n"
    "        HBITMAP bmp   = NULL;\n"
    "        HBITMAP oldB  = NULL;\n"
    "        if (mem) {\n"
    "            BITMAPINFO bmi = {0};\n"
    "            bmi.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);\n"
    "            bmi.bmiHeader.biWidth = W;\n"
    "            bmi.bmiHeader.biHeight = -H;   // 负值 = top-down，与 GDI+ 坐标一致\n"
    "            bmi.bmiHeader.biPlanes = 1;\n"
    "            bmi.bmiHeader.biBitCount = 32;\n"
    "            bmi.bmiHeader.biCompression = BI_RGB;\n"
    "            void* bits = NULL;\n"
    "            bmp = CreateDIBSection(hdcReal, &bmi, DIB_RGB_COLORS, &bits, NULL, 0);\n"
    "            oldB = (HBITMAP)SelectObject(mem, bmp);\n"
    "        }\n"
    "        HDC hdc = (mem && bmp) ? mem : hdcReal;\n"
    "\n"
    "        // GDI+ 渲染上下文（玻璃效果需要 alpha 合成 / 抗锯齿 / 软阴影模糊）\n"
    "        Graphics g(hdc);\n"
    "        g.SetSmoothingMode(SmoothingModeAntiAlias);\n"
    "        g.SetTextRenderingHint(TextRenderingHintAntiAliasGridFit);\n"
    "        g.SetInterpolationMode(InterpolationModeHighQualityBicubic);\n"
    "        g_pg = &g;\n"
    "\n"
    "        // 窗口层：紫→蓝平滑渐变（GDI+ LinearGradientBrush）\n"
    "        DrawWindowFrame(hdc, W, H);\n"
)
src = replace_once(src, anchor_buf, buf_new, "wm_paint dib+graphics")

# ---------- 6) WM_PAINT 收尾置空 g_pg ----------
anchor_tail = (
    "        if (mem) DeleteDC(mem);\n"
    "        EndPaint(hWnd, &ps);\n"
)
tail_new = (
    "        if (mem) DeleteDC(mem);\n"
    "        g_pg = NULL;\n"
    "        EndPaint(hWnd, &ps);\n"
)
src = replace_once(src, anchor_tail, tail_new, "wm_paint tail")

# ---------- 7) DrawWindowFrame 改 GDI+ 渐变 ----------
anchor_dwf = (
    "static void DrawWindowFrame(HDC hdc, int W, int H) {\n"
    "    RECT full = {0, 0, W, H};\n"
    "    FillVGradient(hdc, &full, C_GRAD_TOP, C_GRAD_BOT);\n"
    "}\n"
)
dwf_new = (
    "static void DrawWindowFrame(HDC hdc, int W, int H) {\n"
    "    if (g_pg) {\n"
    "        LinearGradientBrush bg(RectF(0, 0, (float)W, (float)H),\n"
    "            Color(255, (BYTE)GetRValue(C_GRAD_TOP), (BYTE)GetGValue(C_GRAD_TOP), (BYTE)GetBValue(C_GRAD_TOP)),\n"
    "            Color(255, (BYTE)GetRValue(C_GRAD_BOT), (BYTE)GetGValue(C_GRAD_BOT), (BYTE)GetBValue(C_GRAD_BOT)),\n"
    "            LinearGradientModeVertical);\n"
    "        g_pg->FillRectangle(&bg, 0, 0, W, H);\n"
    "    } else {\n"
    "        RECT full = {0, 0, W, H};\n"
    "        FillVGradient(hdc, &full, C_GRAD_TOP, C_GRAD_BOT);\n"
    "    }\n"
    "}\n"
)
src = replace_once(src, anchor_dwf, dwf_new, "drawwindowframe")

# ---------- 8) DrawCard 改 GDI+ 磨砂半透 + 软阴影 ----------
anchor_card = (
    "static void DrawCard(HDC hdc, RECT* r, BOOL hovered, COLORREF accent) {\n"
    "    int rad = Scale(WIN_RADIUS);\n"
    "    // 投影：柔和深冷色，落在渐变上自然（玻璃悬浮感）\n"
    "    RECT sh = {r->left + Scale(1), r->top + Scale(3),\n"
    "               r->right + Scale(1), r->bottom + Scale(3)};\n"
    "    DrawRoundedRect(hdc, &sh, rad, C_GLASS_SH, C_GLASS_SH);\n"
    "    // 卡片本体：磨砂奶白 + 亮边高光（玻璃质感）\n"
    "    DrawRoundedRect(hdc, r, rad, C_CARD_GLASS, C_GLASS_BORDER, 1);\n"
    "    // hover：主色变亮边框（视觉锚点）\n"
    "    if (hovered) {\n"
    "        DrawRoundedRect(hdc, r, rad, C_CARD_GLASS, C_PRIMARY_H, 2);\n"
    "    }\n"
    "    // 左侧 3px 圆角类型彩条（绿=RunCommand / 琥珀=ShellExtension）\n"
    "    RECT ar = {r->left, r->top + Scale(14),\n"
    "               r->left + Scale(3), r->bottom - Scale(14)};\n"
    "    DrawRoundedRect(hdc, &ar, Scale(2), accent, accent);\n"
    "}\n"
)
card_new = (
    "static void DrawCard(HDC hdc, RECT* r, BOOL hovered, COLORREF accent) {\n"
    "    int rad = Scale(WIN_RADIUS);\n"
    "    int cw = r->right - r->left, ch = r->bottom - r->top;\n"
    "    if (g_pg) {\n"
    "        // 真实高斯软阴影（缓存，叠在渐变上 → 玻璃悬浮感）\n"
    "        BuildShadows(cw, ch);\n"
    "        int blur = hovered ? 18 : 16;\n"
    "        int pad = blur * 3 + 6;\n"
    "        Bitmap* sh = g_shadow[hovered ? 1 : 0];\n"
    "        if (sh) g_pg->DrawImage(sh, (INT)(r->left - pad), (INT)(r->top - pad));\n"
    "        // 磨砂半透卡体（白低透 + 垂直渐变光泽）\n"
    "        LinearGradientBrush cb(RectF((float)r->left, (float)r->top, (float)cw, (float)ch),\n"
    "            Color(205, 255, 255, 255), Color(185, 255, 255, 255), LinearGradientModeVertical);\n"
    "        GraphicsPath p; RoundRectPath(&p, (float)r->left, (float)r->top, (float)cw, (float)ch, (float)rad);\n"
    "        g_pg->FillPath(&cb, &p);\n"
    "        // 边框：hover 用 accent，常态用淡白\n"
    "        Color bc = hovered\n"
    "            ? Color(255, (BYTE)GetRValue(accent), (BYTE)GetGValue(accent), (BYTE)GetBValue(accent))\n"
    "            : Color(95, 255, 255, 255);\n"
    "        Pen bp(bc, 1.0f);\n"
    "        GraphicsPath p2; RoundRectPath(&p2, (float)r->left, (float)r->top, (float)cw, (float)ch, (float)rad);\n"
    "        g_pg->DrawPath(&bp, &p2);\n"
    "        g_pg->Flush(FlushIntentSync);   // 确保后续 GDI 内容（图标/文字）正确合成\n"
    "    }\n"
    "    // 左侧 3px 圆角类型彩条（GDI 绘制，位于卡内）\n"
    "    RECT ar = {r->left, r->top + Scale(14),\n"
    "               r->left + Scale(3), r->bottom - Scale(14)};\n"
    "    DrawRoundedRect(hdc, &ar, Scale(2), accent, accent);\n"
    "}\n"
)
src = replace_once(src, anchor_card, card_new, "drawcard")

with io.open(SRC, "w", encoding="utf-8", errors="surrogateescape") as f:
    f.write(src)
print("main.cpp patched OK")

# ---------- 9) build_git.sh 链接 -lgdiplus ----------
b = read(BUILD)
anchor_link = "    -lsetupapi -liphlpapi -lkernel32 -ldxgi -ldxguid \\\n"
link_new = "    -lsetupapi -liphlpapi -lkernel32 -ldxgi -ldxguid -lgdiplus \\\n"
if b.count(anchor_link) != 1:
    raise SystemExit("FAIL [build link] expected 1, found %d" % b.count(anchor_link))
b = b.replace(anchor_link, link_new, 1)
with io.open(BUILD, "w", encoding="utf-8", errors="surrogateescape") as f:
    f.write(b)
print("build_git.sh patched OK")
