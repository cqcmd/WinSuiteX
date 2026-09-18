// glass_showcase.cpp — D 方案（玻璃拟态）单文件验证
// 目的：验证"紫蓝渐变 + 磨砂半透卡 + 真实高斯柔化软阴影"能否在
// 单 exe / 零额外依赖（gdiplus.dll 为系统自带）的 Win32 环境落地。
// 仅复刻 menu 页布局：标题栏 / 导语 / 4 张功能卡 / 页脚。
// 窗口策略：WS_POPUP（天生无系统标题栏/边框，客户区 ≡ 窗口，无需跟 NC 机制缠斗）。
#include <windows.h>
#include <gdiplus.h>
#include <math.h>
#include <wchar.h>
#include <string.h>
#include <stdlib.h>

using namespace Gdiplus;

// ---------- 布局常量（与 preview.py / main.cpp 同源） ----------
static const int WIN_W = 600;
static const int WIN_H = 718;
static const int TITLE_H = 46;
static const int HEADER_H = 96;
static const int CARD_H = 108;
static const int CARD_GAP = 16;
static const int FOOTER_H = 64;
static const int MARGIN = 24;
static const int RADIUS = 8;
static const int BTN_W = 84, BTN_H = 32;
static const int ICON_SZ = 46;
static const int BADGE_W = 76, BADGE_H = 22;

// ---------- 功能数据 ----------
enum Kind { K_RUN, K_REG, K_SHELL };
struct Feat { const char* name; const char* desc; Kind kind; bool installed; int id; };
static Feat g_feats[4] = {
    {"\u8ba1\u7b97\u673a\u7ba1\u7406", "\u5728\u201c\u6b64\u7535\u8111\u201d\u53f3\u952e\u83dc\u5355\u6dfb\u52a0\u201c\u8ba1\u7b97\u673a\u7ba1\u7406\u201d\uff0c\u4e00\u952e\u6253\u5f00 compmgmt.msc", K_RUN, true, 0},
    {"\u8bbe\u5907\u7ba1\u7406\u5668", "\u6f14\u793a\u6a21\u5757\uff1a\u5728\u201c\u6b64\u7535\u8111\u201d\u53f3\u952e\u83dc\u5355\u6dfb\u52a0\u201c\u8bbe\u5907\u7ba1\u7406\u5668\u201d(devmgmt.msc)", K_RUN, false, 1},
    {"\u663e\u793a/\u9690\u85cf \u7cfb\u7edf\u6587\u4ef6", "\u5728\u6587\u4ef6\u5939\u7a7a\u767d\u5904\u53f3\u952e\u4e00\u952e\u5207\u6362\u201c\u663e\u793a/\u9690\u85cf \u7cfb\u7edf\u6587\u4ef6\u4e0e\u6269\u5c55\u540d\u201d", K_SHELL, true, 2},
    {"\u547d\u4ee4\u63d0\u793a\u7b26", "\u5728\u6587\u4ef6\u5939\u4e0a\u53f3\u952e\u4e00\u952e\u6253\u5f00\u547d\u4ee4\u63d0\u793a\u7b26\uff0c\u5e76\u81ea\u52a8\u5b9a\u4f4d\u5230\u5f53\u524d\u6587\u4ef6\u5939", K_RUN, false, 3},
};
static const int N = 4;
static int g_hover = -1;
static bool g_drag = false;                        // 标题手动拖拽进行中
static int  g_dragStartX = 0, g_dragStartY = 0;   // 按下点屏幕坐标（来自 WM_NCLBUTTONDOWN lParam）
static int  g_dragWinX = 0,   g_dragWinY = 0;     // 按下时窗口左上角（用于把位移换算成新位置）
static int  g_dragMoves = 0;                      // 拖拽移动计数（供自动化测试判别"自绘拖拽分支确实执行"）

static inline RectF CardRect(int i) {
    float y = (float)(TITLE_H + HEADER_H + CARD_GAP + i * (CARD_H + CARD_GAP));
    return RectF((float)MARGIN, y, (float)(WIN_W - 2 * MARGIN), (float)CARD_H);
}

// 标题栏右上角 最小化 / 关闭 按钮（客户区坐标矩形）
// 关键：绘制字形与点击命中测试都调用本函数，保证"画在哪就点在哪"
static void GetTitleButtons(RECT* minR, RECT* closeR) {
    int btnSz = 36, top = 10, gap = 4; // top=10 使按钮整体位于 8px 圆角弧下方，避免热区被 SetWindowRgn 裁掉
    int right = WIN_W - 6;
    closeR->left = right - btnSz; closeR->top = top;
    closeR->right = right;       closeR->bottom = top + btnSz;
    minR->left = closeR->left - gap - btnSz; minR->top = top;
    minR->right = closeR->left - gap;        minR->bottom = top + btnSz;
}

// 阴影位图缓存：每张卡 rest/hover 两组预生成，Render 只做引用切换（消除每帧盒式模糊的开销）
static Bitmap* MakeShadow(RectF card, float radius, int blur, Color color, float offY); // 前向声明
static Bitmap* g_shadow[2] = { 0 }; // 索引 0=常态, 1=hover；所有卡片几何相同，只需两套模板
static bool g_shadowBuilt = false;
static void BuildShadows() {
    if (g_shadowBuilt) return;
    // 所有卡片几何相同（仅 Y 不同），阴影模板只需两套：常态 / hover。
    // 用 CardRect(0) 生成一次，Render 时按各卡 Y 偏移 DrawImage，内存由 8 张降为 2 张（约省 3.4MB），外观不变。
    RectF cr = CardRect(0);
    g_shadow[0] = MakeShadow(cr, (float)RADIUS, 16, Color(70, 0, 0, 0), 12);  // 常态
    g_shadow[1] = MakeShadow(cr, (float)RADIUS, 18, Color(110, 0, 0, 0), 18); // hover
    g_shadowBuilt = true;
}

// ---------- GDI+ 工具 ----------
static void RoundRectPath(GraphicsPath* p, float x, float y, float w, float h, float r) {
    if (r > w / 2.0f) r = w / 2.0f;
    if (r > h / 2.0f) r = h / 2.0f;
    p->StartFigure();
    p->AddArc(x, y, 2 * r, 2 * r, 180, 90);
    p->AddArc(x + w - 2 * r, y, 2 * r, 2 * r, 270, 90);
    p->AddArc(x + w - 2 * r, y + h - 2 * r, 2 * r, 2 * r, 0, 90);
    p->AddArc(x, y + h - 2 * r, 2 * r, 2 * r, 90, 90);
    p->CloseFigure();
}

// 可分离盒式模糊（2 遍，近似高斯）—— 对带 alpha 的阴影位图柔化
static void boxBlurH(const float* src, float* tmp, int w, int h, int r) {
    float inv = 1.0f / (2 * r + 1);
    for (int y = 0; y < h; y++) {
        int row = y * w; float sum = 0;
        for (int i = -r; i <= r; i++) { int xx = i; if (xx < 0) xx = 0; if (xx >= w) xx = w - 1; sum += src[row + xx]; }
        for (int x = 0; x < w; x++) {
            tmp[row + x] = sum * inv;
            int add = x + r + 1, sub = x - r;
            if (add >= w) add = w - 1;
            if (sub < 0) sub = 0;
            sum += src[row + add] - src[row + sub];
        }
    }
}
static void boxBlurV(const float* src, float* tmp, int w, int h, int r) {
    float inv = 1.0f / (2 * r + 1);
    for (int x = 0; x < w; x++) {
        float sum = 0;
        for (int i = -r; i <= r; i++) { int yy = i; if (yy < 0) yy = 0; if (yy >= h) yy = h - 1; sum += src[yy * w + x]; }
        for (int y = 0; y < h; y++) {
            tmp[y * w + x] = sum * inv;
            int add = y + r + 1, sub = y - r;
            if (add >= h) add = h - 1;
            if (sub < 0) sub = 0;
            sum += src[add * w + x] - src[sub * w + x];
        }
    }
}
static void BoxBlur(Bitmap* bmp, int radius) {
    if (radius < 1) return;
    BitmapData bd; Rect rc(0, 0, bmp->GetWidth(), bmp->GetHeight());
    if (bmp->LockBits(&rc, ImageLockModeRead | ImageLockModeWrite, PixelFormat32bppARGB, &bd) != Ok) return;
    int w = bd.Width, h = bd.Height;
    UINT* px = (UINT*)bd.Scan0;
    float *c[4]; for (int i = 0; i < 4; i++) c[i] = new float[w * h];
    float *t0 = new float[w * h], *t1 = new float[w * h];
    for (int i = 0; i < w * h; i++) { UINT v = px[i]; c[0][i] = v & 0xff; c[1][i] = (v >> 8) & 0xff; c[2][i] = (v >> 16) & 0xff; c[3][i] = (v >> 24) & 0xff; }
    for (int ch = 0; ch < 4; ch++) {
        boxBlurH(c[ch], t0, w, h, radius); boxBlurV(t0, t1, w, h, radius);
        boxBlurH(t1, t0, w, h, radius); boxBlurV(t0, t1, w, h, radius);
        memcpy(c[ch], t1, w * h * sizeof(float));
    }
    for (int i = 0; i < w * h; i++) {
        UINT a = (UINT)(c[3][i] + 0.5), r = (UINT)(c[0][i] + 0.5), g = (UINT)(c[1][i] + 0.5), b = (UINT)(c[2][i] + 0.5);
        if (a > 255) a = 255;
        if (r > 255) r = 255;
        if (g > 255) g = 255;
        if (b > 255) b = 255;
        px[i] = (a << 24) | (b << 16) | (g << 8) | r;
    }
    bmp->UnlockBits(&bd);
    for (int i = 0; i < 4; i++) delete[] c[i];
    delete[] t0; delete[] t1;
}

// 生成一张柔化阴影位图（黑 + alpha），返回后由调用方 DrawImage 到 (card.X-pad, card.Y-pad)
static Bitmap* MakeShadow(RectF card, float radius, int blur, Color color, float offY) {
    int pad = blur * 3 + 6;
    int w = (int)(card.Width + pad * 2), h = (int)(card.Height + pad * 2);
    Bitmap* b = new Bitmap(w, h, PixelFormat32bppARGB);
    {
        Graphics g(b);
        SolidBrush br(color);
        GraphicsPath p; RoundRectPath(&p, (float)pad, (float)pad + offY, card.Width, card.Height, radius);
        g.FillPath(&br, &p);
    }
    BoxBlur(b, blur);
    return b; // pad 由调用方按 blur*3+6 还原
}

// UTF-8 -> WCHAR 辅助，避免源码宽字符编码问题
// 强制在末尾写入终止符：即便字符串恰好填满 cap（无空位），DrawString(buf,-1) 也不会越界扫描
static int U8ToW(const char* s, WCHAR* out, int cap) {
    int n = MultiByteToWideChar(CP_UTF8, 0, s, -1, out, cap);
    if (cap > 0) out[cap - 1] = 0;
    return n;
}

// 字体：依次尝试 微软雅黑 → 宋体 → Arial，取第一个可用，避免中文渲染成方块/乱码
// 用文件级静态指针，便于 WM_DESTROY 时统一释放（避免长期使用/多窗口场景累积泄漏）
static FontFamily* g_fontFam = NULL;
static FontFamily& UiFontFamily() {
    if (!g_fontFam) {
        static const WCHAR* names[] = { L"Microsoft YaHei", L"SimSun", L"Arial" };
        for (int i = 0; i < 3; i++) {
            FontFamily* f = new FontFamily(names[i]);
            if (f->GetLastStatus() == Ok) { g_fontFam = f; break; }
            delete f;
        }
        if (!g_fontFam) g_fontFam = new FontFamily(L"Arial"); // 最后兜底，GDI+ 会自行替换
    }
    return *g_fontFam;
}

static void Text(Graphics* g, const char* utf8, float x, float y, float size, Color color, bool bold, int align) {
    WCHAR buf[256] = {0}; U8ToW(utf8, buf, 256);
    Font font(&UiFontFamily(), size, bold ? FontStyleBold : FontStyleRegular, UnitPixel);
    SolidBrush br(color);
    StringFormat fmt; fmt.SetAlignment((StringAlignment)align); fmt.SetLineAlignment(StringAlignmentCenter);
    fmt.SetFormatFlags(StringFormatFlagsNoWrap);
    RectF r(align == 1 ? x - 1000.0f : x, y - size * 0.7f, 2000.0f, size * 1.4f);
    g->DrawString(buf, -1, &font, r, &fmt, &br);
}
static void TextWrap(Graphics* g, const char* utf8, float x, float y, float w, float size, Color color) {
    WCHAR buf[512] = {0}; U8ToW(utf8, buf, 512);
    Font font(&UiFontFamily(), size, FontStyleRegular, UnitPixel);
    SolidBrush br(color);
    StringFormat fmt; fmt.SetAlignment(StringAlignmentNear); fmt.SetLineAlignment(StringAlignmentNear);
    RectF r(x, y, w, size * 3.0f);
    g->DrawString(buf, -1, &font, r, &fmt, &br);
}

// 线稿图标（白色），绘制在 accent 色圆角瓦片内
static void DrawFeatureIcon(Graphics* g, RectF tile, int id, Color /*accent*/) {
    Pen pen(Color(255, 255, 255), 2.0f);
    float cx = tile.X + tile.Width / 2, cy = tile.Y + tile.Height / 2;
    if (id == 0) { // 显示器
        float mw = 26, mh = 12, mx = cx - mw / 2, my = cy - 10;
        g->DrawRectangle(&pen, mx, my, mw, mh);
        g->DrawLine(&pen, cx, my + mh, cx, my + mh + 5);
        g->DrawLine(&pen, cx - 7, my + mh + 5, cx + 7, my + mh + 5);
    } else if (id == 1) { // 芯片
        float s = 18, sx = cx - s / 2, sy = cy - s / 2;
        g->DrawRectangle(&pen, sx, sy, s, s);
        { SolidBrush wb(Color(255, 255, 255)); g->FillRectangle(&wb, cx - 3, cy - 3, 6.0f, 6.0f); }
        for (int d = -6; d <= 6; d += 6) {
            g->DrawLine(&pen, cx + d, sy, cx + d, sy - 3);
            g->DrawLine(&pen, cx + d, sy + s, cx + d, sy + s + 3);
            g->DrawLine(&pen, sx, cy + d, sx - 3, cy + d);
            g->DrawLine(&pen, sx + s, cy + d, sx + s + 3, cy + d);
        }
    } else if (id == 2) { // 文件夹 + 眼
        float fw = 28, fh = 16, fx = cx - fw / 2, fy = cy - 6;
        g->DrawRectangle(&pen, fx, fy, fw, fh);
        g->DrawRectangle(&pen, fx, fy - 5, 12.0f, 5.0f);
        float ex = cx, ey = cy + 4, er = 4;
        g->DrawEllipse(&pen, ex - er, ey - er, 2 * er, 2 * er);
        { SolidBrush wb(Color(255, 255, 255)); g->FillEllipse(&wb, ex - 1.5f, ey - 1.5f, 3.0f, 3.0f); }
    } else { // 终端
        float tw = 34, th = 22, tx = cx - tw / 2, ty = cy - th / 2 - 2;
        g->DrawRectangle(&pen, tx, ty, tw, th);
        g->DrawLine(&pen, tx + 8, ty + 14, tx + 13, ty + 19);
        g->DrawLine(&pen, tx + 13, ty + 19, tx + 8, ty + 24);
        { SolidBrush wb(Color(255, 255, 255)); g->FillRectangle(&wb, tx + 17, ty + 18, 6.0f, 2.0f); }
    }
}

// ---------- 主渲染 ----------
static void Render(Graphics* g, int W, int H) {
    g->SetSmoothingMode(SmoothingModeAntiAlias);
    g->SetTextRenderingHint(TextRenderingHintAntiAliasGridFit);
    g->SetInterpolationMode(InterpolationModeHighQualityBicubic);

    // 背景：紫蓝线性渐变
    {
        LinearGradientBrush bg(RectF(0, 0, (float)W, (float)H),
            Color(255, 124, 58, 237), Color(255, 37, 99, 235), LinearGradientModeVertical);
        g->FillRectangle(&bg, 0, 0, W, H);
    }
    // 标题栏（磨砂）
    {
        RectF tb(0, 0, (float)W, (float)TITLE_H);
        LinearGradientBrush br(tb, Color(40, 255, 255, 255), Color(16, 255, 255, 255), LinearGradientModeVertical);
        g->FillRectangle(&br, tb);
        Pen line(Color(70, 255, 255, 255), 1.0f);
        g->DrawLine(&line, 0.0f, (float)TITLE_H, (float)W, (float)TITLE_H);
    }
    // 应用图标（品牌色圆角块 + 窗口标记）
    {
        RectF ir(16, 12, 22, 22);
        SolidBrush ib(Color(255, 99, 102, 241));
        GraphicsPath p; RoundRectPath(&p, ir.X, ir.Y, ir.Width, ir.Height, 5);
        g->FillPath(&ib, &p);
        Pen wpen(Color(255, 255, 255), 1.0f);
        g->DrawRectangle(&wpen, ir.X + 5, ir.Y + 8, 12.0f, 6.0f);
        { SolidBrush wf(Color(255, 255, 255)); g->FillRectangle(&wf, ir.X + 5, ir.Y + 4, 12.0f, 2.0f); }
    }
    Text(g, "WinSuiteX", 48, 23, 14, Color(255, 255, 255), true, 0);
    // 最小化 / 关闭 按钮（自绘字形，坐标来自 GetTitleButtons 与命中测试一致）
    {
        RECT minR, closeR; GetTitleButtons(&minR, &closeR);
        int mcx = (minR.left + minR.right) / 2, mcy = (minR.top + minR.bottom) / 2;
        int ccx = (closeR.left + closeR.right) / 2, ccy = (closeR.top + closeR.bottom) / 2;
        Pen gp(Color(220, 230, 245), 1.6f);
        g->DrawLine(&gp, (float)(mcx - 7), (float)mcy, (float)(mcx + 7), (float)mcy);              // min：横线
        g->DrawLine(&gp, (float)(ccx - 7), (float)(ccy - 7), (float)(ccx + 7), (float)(ccy + 7));  // close：X
        g->DrawLine(&gp, (float)(ccx + 7), (float)(ccy - 7), (float)(ccx - 7), (float)(ccy + 7));
    }

    // 导语区
    Text(g, "\u53f3\u952e\u83dc\u5355\u589e\u5f3a\u5de5\u5177", (float)MARGIN, (float)(TITLE_H + 33), 16, Color(255, 255, 255), true, 0);
    {
        SolidBrush ab(Color(255, 99, 102, 241));
        g->FillRectangle(&ab, (float)MARGIN, (float)(TITLE_H + 46), 28.0f, 3.0f);
    }
    Text(g, "Windows 11 (Build 22631)", (float)MARGIN, (float)(TITLE_H + 61), 13, Color(225, 230, 245), false, 0);
    {
        Pen dv(Color(60, 255, 255, 255), 1.0f);
        g->DrawLine(&dv, (float)MARGIN, (float)(TITLE_H + HEADER_H - 16), (float)(W - MARGIN), (float)(TITLE_H + HEADER_H - 16));
    }

    // 卡片
    for (int i = 0; i < N; i++) {
        RectF cr = CardRect(i);
        bool hov = (i == g_hover);
        Color accent = (g_feats[i].kind == K_RUN) ? Color(255, 52, 211, 153) :
                      (g_feats[i].kind == K_SHELL) ? Color(255, 251, 191, 36) :
                                                     Color(255, 129, 140, 248);
        // 软阴影（使用预生成缓存：hover 只切换不重建，避免每帧盒式模糊卡顿）
        int blur = hov ? 18 : 16;
        int pad = blur * 3 + 6;
        Bitmap* sh = g_shadow[hov ? 1 : 0];
        g->DrawImage(sh, (INT)(cr.X - pad), (INT)(cr.Y - pad));

        // 磨砂卡体（白低透 + 垂直渐变光泽）
        {
            LinearGradientBrush cb(cr, Color(60, 255, 255, 255), Color(34, 255, 255, 255), LinearGradientModeVertical);
            GraphicsPath p; RoundRectPath(&p, cr.X, cr.Y, cr.Width, cr.Height, (float)RADIUS);
            g->FillPath(&cb, &p);
        }
        // 边框：hover 用 accent，常态用淡白
        {
            Pen bp(hov ? accent : Color(90, 255, 255, 255), 1.0f);
            GraphicsPath p; RoundRectPath(&p, cr.X, cr.Y, cr.Width, cr.Height, (float)RADIUS);
            g->DrawPath(&bp, &p);
        }
        // 左侧类型彩条
        {
            RectF ar(cr.X, cr.Y + 14, 3, cr.Height - 28);
            SolidBrush ab(accent);
            GraphicsPath p; RoundRectPath(&p, ar.X, ar.Y, ar.Width, ar.Height, 2);
            g->FillPath(&ab, &p);
        }
        // 图标瓦片
        RectF tile(cr.X + 18, cr.Y + (CARD_H - ICON_SZ) / 2, ICON_SZ, ICON_SZ);
        {
            SolidBrush tb(accent);
            GraphicsPath p; RoundRectPath(&p, tile.X, tile.Y, tile.Width, tile.Height, 11);
            g->FillPath(&tb, &p);
        }
        DrawFeatureIcon(g, tile, g_feats[i].id, accent);

        // 文本
        float tx = tile.X + tile.Width + 16;
        Text(g, g_feats[i].name, tx, cr.Y + 18 + 12, 15, Color(255, 255, 255), true, 0);
        TextWrap(g, g_feats[i].desc, tx, cr.Y + 42, cr.Width - (tx - cr.X) - (BTN_W + 14), 12, Color(225, 230, 245));
        // 状态徽章
        {
            float bx = tx, by = cr.Y + 82, bw = BADGE_W, bh = BADGE_H;
            bool inst = g_feats[i].installed;
            SolidBrush bb(inst ? Color(60, 52, 211, 153) : Color(40, 255, 255, 255));
            GraphicsPath p; RoundRectPath(&p, bx, by, bw, bh, 11);
            g->FillPath(&bb, &p);
            // 指示点
            if (inst) { SolidBrush db(Color(255, 150, 240, 190)); g->FillEllipse(&db, bx + 12, by + bh / 2 - 3, 6.0f, 6.0f); }
            else { Pen ep(Color(210, 216, 230)); g->DrawEllipse(&ep, bx + 12, by + bh / 2 - 3, 6.0f, 6.0f); }
            Text(g, inst ? "\u5df2\u542f\u7528" : "\u672a\u542f\u7528", bx + 24, by + bh / 2, 11,
                 inst ? Color(255, 150, 240, 190) : Color(235, 240, 255), false, 0);
        }
        // 操作按钮（卡片右侧居中）
        {
            RectF br(cr.X + cr.Width - 18 - BTN_W, cr.Y + (CARD_H - BTN_H) / 2, BTN_W, BTN_H);
            bool primary = !g_feats[i].installed;
            if (primary) {
                SolidBrush fb(Color(255, 99, 102, 241));
                GraphicsPath p; RoundRectPath(&p, br.X, br.Y, br.Width, br.Height, 6);
                g->FillPath(&fb, &p);
                Text(g, "\u542f\u7528", br.X + br.Width / 2, br.Y + br.Height / 2, 13, Color(255, 255, 255), true, 1);
            } else {
                Pen bp(Color(110, 255, 255, 255), 1.0f);
                GraphicsPath p; RoundRectPath(&p, br.X, br.Y, br.Width, br.Height, 6);
                g->DrawPath(&bp, &p);
                Text(g, "\u505c\u7528", br.X + br.Width / 2, br.Y + br.Height / 2, 13, Color(255, 255, 255), true, 1);
            }
        }
    }

    // 页脚
    Text(g, "\u53f3\u952e\u83dc\u5355\u9879\u542f\u7528\u540e\u5373\u65f6\u751f\u6548\uff0c\u65e0\u9700\u91cd\u542f",
         (float)(W / 2), (float)(H - FOOTER_H / 2), 11, Color(220, 228, 242), false, 1);

    // 窗口 1px 淡白边框
    {
        Pen wp(Color(120, 255, 255, 255), 1.0f);
        GraphicsPath p; RoundRectPath(&p, 0.5f, 0.5f, (float)(W - 1), (float)(H - 1), (float)RADIUS);
        g->DrawPath(&wp, &p);
    }
}

// ---------- 窗口过程 ----------
// 复用的双缓冲 back buffer（进程级静态资源，窗口销毁时统一释放）
static HDC     g_memDC  = NULL;
static HBITMAP g_memBmp = NULL;
static HBITMAP g_oldBmp = NULL;

static void ReleaseBackBuffer() {
    if (!g_memDC) return;
    if (g_oldBmp) SelectObject(g_memDC, g_oldBmp); // 先恢复原始对象，再释放位图与 DC
    if (g_memBmp) DeleteObject(g_memBmp);
    DeleteDC(g_memDC);
    g_memDC = NULL; g_memBmp = NULL; g_oldBmp = NULL;
}

static LRESULT CALLBACK WndProc(HWND hWnd, UINT msg, WPARAM wp, LPARAM lp) {
    switch (msg) {
    case WM_CREATE: {
        BuildShadows(); // 预生成阴影位图缓存，避免每帧重算盒式模糊
        return 0;
    }
    case WM_NCCALCSIZE:
        // 标准重叠窗口去边框用：保留 rgrc[0]（系统提议的新窗口矩形）原样并 return 0，
        // 即"整窗即客户区"。本窗口为 WS_POPUP 通常不触发，保留为正确实现以便随时切回标准窗口样式。
        if (wp) { return 0; }
        break;
    case WM_NCPAINT:
        return 0; // 阻止系统绘制非客户区（标题栏/边框），实现真正无边框
    case WM_ERASEBKGND:
        return 1; // 不擦背景，交给双缓冲 WM_PAINT 一次画好，消除闪烁
    case WM_NCLBUTTONDBLCLK:
        return 0; // 无边框窗口禁用"双击标题最大化"，避免破坏外观
    case WM_SETCURSOR:
        // 光标闪烁根因：不处理本消息时，DefWindowProc 会按命中测试码（HTCAPTION/HTCLIENT）
        // 在每次鼠标移动时重新设置光标；指针在标题区与控件区之间穿行时便出现光标抖动。
        // 这里统一锁定标准箭头并返回 TRUE，完全接管光标管理，杜绝抖动。
        SetCursor(LoadCursor(NULL, IDC_ARROW));
        return TRUE;
    case WM_PAINT: {
        PAINTSTRUCT ps; HDC hdc = BeginPaint(hWnd, &ps);
        // 双缓冲：先画到离屏 DIB（32bpp），再一次性 BitBlt 到窗口 —— 消除重绘闪烁。
        // 关键优化：back buffer 只创建一次并全程复用（600x718x4 ≈ 1.7MB），
        // 避免每次 hover 重绘都重新分配位图造成的卡顿。
        if (!g_memDC) {
            g_memDC = CreateCompatibleDC(hdc);
            BITMAPINFO bmi = {0};
            bmi.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
            bmi.bmiHeader.biWidth = WIN_W;
            bmi.bmiHeader.biHeight = -WIN_H; // 负值 = top-down，与 GDI+ 坐标一致
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB;
            void* bits = NULL;
            g_memBmp = CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &bits, NULL, 0);
            g_oldBmp = (HBITMAP)SelectObject(g_memDC, g_memBmp);
        }
        try {
            Graphics g(g_memDC);
            Render(&g, WIN_W, WIN_H);
        } catch (...) {
            // 渲染异常不应中断消息循环；静默降级即可
        }
        BitBlt(hdc, 0, 0, WIN_W, WIN_H, g_memDC, 0, 0, SRCCOPY);
        EndPaint(hWnd, &ps);
        return 0;
    }
    case WM_MOUSEMOVE: {
        // 手动拖拽进行中：用消息 lParam（客户坐标）换算屏幕坐标，按"当前位移 - 起始位移"重定位。
        // 完全不依赖 GetCursorPos，真实鼠标与测试消息走同一逻辑，且在 RDP 下也确定可测。
        if (g_drag) {
            RECT wr; GetWindowRect(hWnd, &wr);
            int cx = (int)(short)LOWORD(lp), cy = (int)(short)HIWORD(lp); // 客户区坐标
            int curX = wr.left + cx, curY = wr.top + cy;                 // 还原为屏幕坐标
            int nx = g_dragWinX + (curX - g_dragStartX);
            int ny = g_dragWinY + (curY - g_dragStartY);
            // 加 SWP_NOACTIVATE：移动时不再抢占焦点，避免拖拽中焦点闪烁
            SetWindowPos(hWnd, NULL, nx, ny, 0, 0, SWP_NOZORDER | SWP_NOSIZE | SWP_NOACTIVATE);
            g_dragMoves++;
            SetProp(hWnd, L"GlassDragMoves", (HANDLE)(UINT_PTR)g_dragMoves); // 暴露计数，供测试判别自绘分支是否执行
            return 0;
        }
        int x = (int)LOWORD(lp), y = (int)HIWORD(lp);
        int found = -1;
        for (int i = 0; i < N; i++) {
            RectF cr = CardRect(i);
            if (x >= cr.X && x <= cr.X + cr.Width && y >= cr.Y && y <= cr.Y + cr.Height) { found = i; break; }
        }
        if (found != g_hover) { g_hover = found; InvalidateRect(hWnd, NULL, FALSE); }
        return 0;
    }
    case WM_LBUTTONDOWN: {
        int x = (int)LOWORD(lp), y = (int)HIWORD(lp);
        // 标题栏按钮优先（坐标与 WM_NCHITTEST 共用 GetTitleButtons）
        if (y < TITLE_H) {
            RECT minR, closeR; GetTitleButtons(&minR, &closeR);
            POINT p = { (LONG)x, (LONG)y };
            if (PtInRect(&closeR, p)) { DestroyWindow(hWnd); return 0; }
            if (PtInRect(&minR,   p)) { ShowWindow(hWnd, SW_MINIMIZE); return 0; }
        }
        for (int i = 0; i < N; i++) {
            RectF cr = CardRect(i);
            if (x >= cr.X && x <= cr.X + cr.Width && y >= cr.Y && y <= cr.Y + cr.Height) {
                RectF br(cr.X + cr.Width - 18 - BTN_W, cr.Y + (CARD_H - BTN_H) / 2, BTN_W, BTN_H);
                if (x >= br.X && x <= br.X + br.Width && y >= br.Y && y <= br.Y + br.Height) {
                    g_feats[i].installed = !g_feats[i].installed;
                    InvalidateRect(hWnd, NULL, FALSE);
                }
                break;
            }
        }
        return 0;
    }
    case WM_NCLBUTTONDOWN:
        // 标题空白处按下：自建拖拽，绕开系统默认 caption 拖拽（其在 RDP / 窗口非前台时失灵）。
        // lParam 为屏幕坐标；记录按下点屏幕坐标与窗口初始左上角，后续 WM_MOUSEMOVE 按位移换算新位置。
        if (wp == HTCAPTION) {
            g_dragStartX = (int)(short)LOWORD(lp);
            g_dragStartY = (int)(short)HIWORD(lp);
            RECT wr; GetWindowRect(hWnd, &wr);
            g_dragWinX = wr.left; g_dragWinY = wr.top;
            // 真实前台鼠标场景下 SetCapture 必成功；即便失败，后续若捕获被系统夺走，
            // WM_CAPTURECHANGED 会清 g_drag（那才是防"窗口黏鼠标"软锁死的正确位置）。
            SetCapture(hWnd);
            g_drag = true;
            return 0;
        }
        break;
    case WM_LBUTTONUP:
        if (g_drag) { g_drag = false; ReleaseCapture(); }
        return 0;
    case WM_NCLBUTTONUP:
        if (g_drag) { g_drag = false; ReleaseCapture(); }
        return 0;
    case WM_CAPTURECHANGED:
        // 捕获被系统/其它窗口夺走（Alt-Tab、Win+M、任务切换）时，必须清拖拽态，
        // 否则 g_drag 永远为 true，后续每次 WM_MOUSEMOVE 都会把窗口黏在鼠标上（软锁死）。
        // 注意：此处不要调用 ReleaseCapture，系统已释放捕获。
        g_drag = false;
        return 0;
    case WM_NCHITTEST: {
        POINT pt = { (int)(short)LOWORD(lp), (int)(short)HIWORD(lp) };
        ScreenToClient(hWnd, &pt);
        if (pt.y < TITLE_H) {
            RECT minR, closeR; GetTitleButtons(&minR, &closeR);
            if (PtInRect(&closeR, pt)) return HTCLIENT; // 交给 WM_LBUTTONDOWN 处理关闭
            if (PtInRect(&minR, pt))   return HTCLIENT; // 交给 WM_LBUTTONDOWN 处理最小化
            return HTCAPTION; // 其余标题区域可拖动
        }
        return HTCLIENT;
    }
    case WM_DESTROY:
        ReleaseBackBuffer(); // 释放复用的 back buffer，避免多窗口场景下累积占用
        for (int j = 0; j < 2; j++)            // 释放两套阴影模板位图缓存（否则进程退出才回收）
            if (g_shadow[j]) { delete g_shadow[j]; g_shadow[j] = NULL; }
        if (g_fontFam) { delete g_fontFam; g_fontFam = NULL; } // 释放字体族
        PostQuitMessage(0); return 0;
    }
    return DefWindowProc(hWnd, msg, wp, lp);
}

int WINAPI WinMain(HINSTANCE hInst, HINSTANCE, LPSTR, int) {
    GdiplusStartupInput gdi; ULONG_PTR tok;
    if (GdiplusStartup(&tok, &gdi, NULL) != Ok) return 1;

    WNDCLASSEX wc = { sizeof(wc) };
    wc.hInstance = hInst;
    wc.lpfnWndProc = WndProc;
    wc.lpszClassName = L"GlassShowcase";
    wc.hCursor = LoadCursor(NULL, IDC_ARROW);
    wc.hbrBackground = NULL; // 双缓冲全窗绘制；NULL 避免系统擦背景导致闪烁
    wc.style = CS_HREDRAW | CS_VREDRAW;
    if (!RegisterClassEx(&wc)) return 1;

    // WS_POPUP：天生无系统标题栏/边框，客户区 ≡ 窗口（不需要 WM_NCCALCSIZE 等 NC 处理）。
    // 配 WS_EX_APPWINDOW 保证任务栏出现。此前 WS_POPUP 在 RDP 下"不显示"的元凶是
    // WS_EX_TOPMOST 而非 popup 本身（圆角 region 已证明无害）。
    HWND hWnd = CreateWindowEx(WS_EX_APPWINDOW, L"GlassShowcase", L"WinSuiteX \u2014 D \u73bb\u7483\u62df\u6001",
        WS_POPUP | WS_VISIBLE, CW_USEDEFAULT, CW_USEDEFAULT, WIN_W, WIN_H,
        NULL, NULL, hInst, NULL);
    if (!hWnd) return 1;

    // 圆角 region（仅裁掉四角，无 NC 含义）
    {
        HRGN hr = CreateRoundRectRgn(0, 0, WIN_W, WIN_H, RADIUS * 2, RADIUS * 2);
        if (hr) SetWindowRgn(hWnd, hr, TRUE);
    }

    // 主显示器居中
    RECT wr; GetWindowRect(hWnd, &wr);
    int sw = GetSystemMetrics(SM_CXSCREEN), sh = GetSystemMetrics(SM_CYSCREEN);
    int nx = (sw - (wr.right - wr.left)) / 2, ny = (sh - (wr.bottom - wr.top)) / 2;
    if (nx < 0) nx = 0;
    if (ny < 0) ny = 0;
    SetWindowPos(hWnd, NULL, nx, ny, 0, 0, SWP_NOZORDER | SWP_NOSIZE);
    ShowWindow(hWnd, SW_SHOW);
    SetForegroundWindow(hWnd);

    MSG m; while (GetMessage(&m, NULL, 0, 0)) { TranslateMessage(&m); DispatchMessage(&m); }
    GdiplusShutdown(tok);
    return 0;
}
