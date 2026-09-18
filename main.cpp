#include "common.h"
#include "features.h"
#include "hardware.h"
#include "resource.h"
#include "selftest.h"
#include <cstdio>
#include <cwchar>
#include <tlhelp32.h>   // Process32* 用于重启资源管理器
#include <shellapi.h>   // ShellExecuteW（导出报告后打开）
#include <winioctl.h>   // IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS（卷→物理盘号）

#include <gdiplus.h>
using namespace Gdiplus;

#ifndef GET_X_LPARAM
#define GET_X_LPARAM(lp) ((int)(short)LOWORD(lp))
#define GET_Y_LPARAM(lp) ((int)(short)HIWORD(lp))
#endif

// ============================================================
// main.cpp - WinSuiteX v5.1
// v5.1：弃用 v5.0 的"单文件 EXE-as-COM-server"——带 requireAdministrator 清单的
//       exe 被 COM 拒绝在 Explorer 进程内加载（命令提示符不显示 / 显示隐藏无效的根因）。
//       改回混合架构：COM 服务器编译为独立 WinSuiteXShell.dll（无提权清单），
//       由 WinSuiteX.exe 以 RCDATA 内嵌，启用功能时释放到 C:\Windows 并注册。
//       管理器 exe 仍保留 requireAdministrator（写 HKLM 弹 UAC）。
// v5.0：项目更名「WinSuiteX」。曾尝试彻底单文件化（_mcentry 双形态入口，见
//       explorercommand.cpp 存档），因上述 COM 加载限制而回退。
//       命令提示符 %V 占位符修复保留在 COM 服务器 Invoke 中。
// v4.4：架构合并（用户问"子功能 exe 能不能写成 dll"）：
//       原 右键增强切换.exe（SuperHidden 切换）合并进 右键增强菜单.dll
//       （新增 ToggleSuperHidden 导出，COM 服务器进程内调用，仍无 UAC）；
//       原 右键增强自检.exe 删除，自检逻辑本就内嵌于管理器（--selftest）。
// v4.3：体积优化——链接加 -s -Wl,--gc-sections，保持 -static 零依赖不变。
// v4.2：命令提示符改为仅注册到文件夹右键（不再注册空白处/背景右键）；
//       卸载时最佳努力清理历史版本残留的背景命令提示符键。
// v4.1：UI 打磨（CS_DROPSHADOW 原生阴影 / 命令提示符单一功能 / 自绘弹窗按钮修复 / 圆角标题栏）。
// v4.0：项目更名「右键增强」（原 WinPersonalize）。新增"在此打开命令提示符"右键功能。
//       现工具纯粹为右键菜单增强（RunCommand + ShellExtension）。
// 功能逻辑（拖拽、安装/卸载回退、ESC、DESTROY、--selftest）一律不动。
// ============================================================

#define MAX_FEATURES 32
#define WIN_W        600
#define TITLE_H      46
#define HEADER_H     96
#define CARD_H       116
#define CARD_GAP     16   // 卡片间距（Ant 8px 栅格 ×2）
#define FOOTER_H     64
#define MARGIN       24   // 窗口内边距（Ant 8px 栅格 ×3）
#define TB_BTN_W     46
#define SHADOW_PAD   1       // 仅用于 1px 边框留白（不再绘制灰色外阴影）
#define WIN_RADIUS   8       // 窗口/卡片圆角（Ant 8px）
// ---- 标签页（硬件配置 / 右键增强 / 磁盘信息 三页） ----
#define TAB_H           44   // 顶部页签条高度
#define TAB_W           96   // 单个页签宽度
#define DISK_PAGE_HEAD  54   // 磁盘页顶部说明区高
#define DISK_TABLE_HEAD 30   // 磁盘表表头高
#define DISK_ROW_H      34   // 磁盘表行高
#define DISK_DETAIL_H   148  // 选中分区详情区高（v1.0.12 加 NT 设备名 +1 行：88→108；v1.3.4 加序列号 +1 行：108→128；v1.3.5 加卷标 +1 行：128→148）
#define DISK_MAX_ROWS   12   // 表格最多显示行数（防分区过多导致窗口过高）
#define HW_HEAD         54   // 硬件页顶部说明区高（与磁盘页对齐）
#define HW_ROW_H        26   // 硬件页单行高（"标签：值"一行）
#define HW_SECTION_GAP  16   // 段间间隔（Ant 8px 栅格 ×2）
#define HW_MAX_DISK_SHOWN 4  // 硬件页"硬盘"段最多列出的物理盘数（其余归入"其他"）

// ---- Per-Monitor V2 DPI 自适应（核心） ----
// 所有布局常量保持 96-DPI 基准值不变（几何关系一目了然），
// 实际像素在绘制/命中检测时统一经 Scale() 换算。g_dpi 在 CreateWindow
// 后按窗口实际监视器 DPI 更新，并在 WM_DPICHANGED 时随之刷新。
static UINT g_dpi = 96;
static inline int Scale(int v) { return MulDiv(v, (int)g_dpi, 96); }

#ifndef WM_DPICHANGED
#define WM_DPICHANGED 0x02E0
#endif

// 前向声明（定义见窗口过程之前的辅助区）：供 NoticeProc 等提前调用
static UINT GetWindowDpi(HWND hWnd);
static void ApplyRoundedRegion(HWND hWnd, int w, int h);
static void ComputeLayout();   // 供 SwitchTab（定义在其之前）调用

// ---- 设计 Token v4.0（Ant Design 语言 · 结构移植版） ----
// 相对 v3.5 的结构升级：圆角收 8px、卡片 hover elevation、主色守 WCAG AA。
// GDI 无 alpha 通道，rgba(0,0,0,a) 织白取实色：c = 255*(1-a)。换算见 DESIGN_SYSTEM.md。
static const COLORREF C_BG              = RGB(245, 245, 245); // #F5F5F5 colorBgLayout
static const COLORREF C_TB_BG           = RGB(250, 250, 250); // #FAFAFA 标题栏
static const COLORREF C_CARD_BG         = RGB(255, 255, 255); // #FFFFFF colorBgContainer
static const COLORREF C_BORDER          = RGB(217, 217, 217); // #D9D9D9 colorBorder
static const COLORREF C_DIVIDER         = RGB(240, 240, 240); // #F0F0F0 colorBorderSecondary
static const COLORREF C_TITLE           = RGB(31, 31, 31);    // #1F1F1F colorText 织白(.88)
static const COLORREF C_TEXT            = RGB(38, 38, 38);    // #262626 功能名（Ant gray-9）
static const COLORREF C_SUBTITLE        = RGB(89, 89, 89);    // #595959 次级文字（守 AA）
static const COLORREF C_DESC            = RGB(89, 89, 89);    // #595959 卡片描述（守 AA）
static const COLORREF C_PRIMARY         = RGB(9, 88, 217);    // #0958D9 Ant blue-7（白字 6.16:1 守 AA，原 #1677FF 仅 4.10:1）
static const COLORREF C_PRIMARY_H       = RGB(22, 119, 255);  // #1677FF colorPrimaryHover（Ant 悬停"变亮"）
static const COLORREF C_PRIMARY_A       = RGB(0, 62, 179);    // #003EB3 激活/按下更深（备用）
static const COLORREF C_SUCCESS         = RGB(82, 196, 26);   // #52C41A Ant 成功绿（语义色·备用）
static const COLORREF C_WARNING         = RGB(250, 173, 20);  // #FAAD14 Ant 警告琥珀（语义色·备用）
static const COLORREF C_ERROR           = RGB(255, 77, 79);   // #FF4D4F Ant 错误红（语义色·备用）
static const COLORREF C_GREEN           = RGB(22, 163, 74);   // #16A34A 类型彩条（瓦片面积大，用较深绿避免过艳）
static const COLORREF C_GREEN_DARK      = RGB(21, 128, 61);   // #15803D 已启用徽章字色（AA）
static const COLORREF C_GREEN_BG        = RGB(223, 242, 230); // #DFF2E6 已启用底色
static const COLORREF C_AMBER           = RGB(202, 110, 4);   // #CA6E04 类型彩条琥珀
static const COLORREF C_AMBER_BG        = RGB(254, 244, 225); // #FEF4E1
static const COLORREF C_BTN_SECONDARY   = RGB(255, 255, 255);
static const COLORREF C_BTN_SECONDARY_H = RGB(250, 250, 250); // #FAFAFA hover
static const COLORREF C_BTN_BORDER      = RGB(217, 217, 217); // #D9D9D9 同 colorBorder
static const COLORREF C_BTN_BORDER_H    = RGB(191, 191, 191); // #BFBFBF hover 时略深
// 卡片常态阴影：极淡单层（Ant 卡片常态靠 1px 边框立起，阴影仅作"轻浮"提示）
static const COLORREF C_SHADOW          = RGB(240, 240, 240); // #F0F0F0 常态单层阴影
// 卡片 hover 阴影：三层实色模拟 elevation 上浮（远层偏移大且浅 → 近层偏移小且深）
static const COLORREF C_SH_HOVER_3      = RGB(232, 232, 232); // #E8E8E8 远层 偏移8
static const COLORREF C_SH_HOVER_2      = RGB(220, 220, 220); // #DCDCDC 中层 偏移4
static const COLORREF C_SH_HOVER_1      = RGB(208, 208, 208); // #D0D0D0 近层 偏移1
static const COLORREF C_TB_HOVER        = RGB(232, 232, 232); // #E8E8E8 最小化按钮 hover
static const COLORREF C_TB_CLOSE_HOVER  = RGB(232, 17, 35);   // #E81123 关闭按钮 hover

// ---- 窗口层阴影（纯自绘，多层实色模拟柔和向下投影，收窄变淡） ----
// Ant 语言：中性灰、整体更浅，去掉旧版的冷蓝调。
static const COLORREF C_WIN_SHADOW_BASE = RGB(240, 240, 240); // #F0F0F0 四周极淡环境阴影
static const COLORREF C_WIN_SHADOW_3    = RGB(232, 232, 232); // #E8E8E8 投影最远层(最浅)
static const COLORREF C_WIN_SHADOW_2    = RGB(222, 222, 222); // #DEDEDE 投影中间层
static const COLORREF C_WIN_SHADOW_1    = RGB(212, 212, 212); // #D4D4D4 投影最近层(最深,贴内容底边)
static const COLORREF C_WIN_BORDER      = RGB(217, 217, 217); // #D9D9D9 窗口 1px 圆角外边框(同 colorBorder)

// ---- 玻璃拟态主题覆盖（在 Ant v4.0 结构之上叠加玻璃视觉语言） ----
// 背景：紫→蓝竖向渐变（玻璃底色）；卡片/面板沿用奶白磨砂、深色文字，浮于渐变之上。
// 压在渐变上的文字（标题栏/标签页/导语/页脚/OS 副标题）改用浅色常量。
static const COLORREF C_GRAD_TOP      = RGB(124, 58, 237);  // #7C3AED 紫（对齐 glass4）
static const COLORREF C_GRAD_MID      = RGB(94, 110, 220);  // #5E6EDC 蓝紫过渡（预留）
static const COLORREF C_GRAD_BOT      = RGB(37, 99, 235);   // #2563EB 蓝（对齐 glass4）
static const COLORREF C_CARD_GLASS    = RGB(250, 250, 253); // #FAFAFD 磨砂奶白（比纯白略冷）
static const COLORREF C_PANEL_GLASS   = RGB(250, 250, 253); // 磁盘/硬件页面板同色
static const COLORREF C_TXT_LIGHT     = RGB(245, 246, 255); // 渐变上主文字（近白带蓝调）
static const COLORREF C_TXT_LIGHT_SUB = RGB(208, 212, 236); // 渐变上次级文字
static const COLORREF C_TXT_LIGHT_DIM = RGB(176, 182, 214); // 更暗浅色（分隔线/说明）
static const COLORREF C_GLASS_BORDER  = RGB(255, 255, 255); // 卡片/面板亮边高光
static const COLORREF C_GLASS_SH      = RGB(40, 44, 80);    // 卡片投影（深冷，落在渐变上更自然）

// ---- GDI+ 玻璃渲染基础设施（移植自 glass_showcase.cpp：平滑渐变 / 半透磨砂卡 / 真高斯软阴影）----
static ULONG_PTR g_gdipToken = 0;
static FontFamily* g_fontFam = NULL;
static struct ShadowEnt { int w, h, hov; Bitmap* bmp; } g_shadows[8];
static int g_shadowCount = 0;
static Graphics* g_pg = NULL;            // 当前帧 GDI+ Graphics（仅 WM_PAINT 内有效）

static FontFamily& UiFontFamily() {
    if (!g_fontFam) {
        static const WCHAR* names[] = { L"Microsoft YaHei", L"SimSun", L"Arial" };
        for (int i = 0; i < 3; i++) {
            FontFamily* f = new FontFamily(names[i]);
            if (f->GetLastStatus() == Ok) { g_fontFam = f; break; }
            delete f;
        }
        if (!g_fontFam) g_fontFam = new FontFamily(L"Arial");
    }
    return *g_fontFam;
}

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
        if (a > 255) a = 255; if (r > 255) r = 255; if (g > 255) g = 255; if (b > 255) b = 255;
        px[i] = (a << 24) | (b << 16) | (g << 8) | r;
    }
    bmp->UnlockBits(&bd);
    for (int i = 0; i < 4; i++) delete[] c[i];
    delete[] t0; delete[] t1;
}
static Bitmap* MakeShadow(RectF card, float radius, int blur, Color color, float offY) {
    int pad = blur * 3 + 6;
    int w = (int)(card.Width + pad * 2), h = (int)(card.Height + pad * 2);
    Bitmap* b = new Bitmap(w, h, PixelFormat32bppARGB);
    { Graphics g(b); SolidBrush br(color); GraphicsPath p;
      RoundRectPath(&p, (float)pad, (float)pad + offY, card.Width, card.Height, radius);
      g.FillPath(&br, &p); }
    BoxBlur(b, blur);
    return b;
}
// 阴影按 (宽,高,hover) 缓存：同一尺寸只算一次高斯模糊，避免不同尺寸卡片交替调用时每帧重算。
static Bitmap* GetShadow(int cw, int ch, BOOL hovered) {
    int hov = hovered ? 1 : 0;
    for (int i = 0; i < g_shadowCount; i++)
        if (g_shadows[i].w == cw && g_shadows[i].h == ch && g_shadows[i].hov == hov)
            return g_shadows[i].bmp;
    if (g_shadowCount >= 8) return NULL;   // 缓存满则放弃阴影（不应发生）
    int blur = hov ? 18 : 16;
    RectF cr(0.0f, 0.0f, (float)cw, (float)ch);
    Bitmap* b = MakeShadow(cr, (float)Scale(WIN_RADIUS), blur,
                           hov ? Color(110, 0, 0, 0) : Color(70, 0, 0, 0),
                           hov ? 18.0f : 12.0f);
    g_shadows[g_shadowCount].w = cw;
    g_shadows[g_shadowCount].h = ch;
    g_shadows[g_shadowCount].hov = hov;
    g_shadows[g_shadowCount].bmp = b;
    g_shadowCount++;
    return b;
}

// 缩放/DPI 变更后旧阴影尺寸失效 -> 整缓存清空，下次按需重建
static void ClearShadows() {
    for (int i = 0; i < g_shadowCount; i++)
        if (g_shadows[i].bmp) { delete g_shadows[i].bmp; g_shadows[i].bmp = NULL; }
    g_shadowCount = 0;
}

// DWM 圆角（Win11 生效，Win10 忽略失败）
#ifndef DWMWA_WINDOW_CORNER_PREFERENCE
#define DWMWA_WINDOW_CORNER_PREFERENCE 33
#endif
#ifndef DWMWCP_ROUND
#define DWMWCP_ROUND 2
#endif

static RECT  g_cardRect[MAX_FEATURES];
static RECT  g_btnRect[MAX_FEATURES];
static BOOL  g_hover[MAX_FEATURES];
// 右键增强页底部「使用经典右键菜单」复选框（独立控件，不属 g_features[]）
static RECT  g_classicMenuRect = {0};
static BOOL  g_classicMenuHover = FALSE;
static RECT  g_minBtnRect;
static RECT  g_closeBtnRect;
static BOOL  g_minHover = FALSE;
static BOOL  g_closeHover = FALSE;
static int   g_count = 0;
static int   g_totalH = 0;       // 缩放后内容区总高（96-DPI 基准求和后 Scale）
static int   g_clientW = 0;      // 缩放后窗口客户区宽
static int   g_clientH = 0;      // 缩放后窗口客户区高
static WCHAR g_osString[128] = {0};
static HICON g_hAppIcon = NULL; // 标题栏真实应用图标（LoadIcon）
static HINSTANCE g_hInst = NULL; // 全局实例句柄（供自绘弹窗注册类使用）

// ---- 标签页状态 ----
enum { TAB_HW = 0, TAB_MENU = 1, TAB_DISK = 2, TAB_COUNT = 3 };
static const WCHAR* g_tabNames[TAB_COUNT] = {
    L"\u786c\u4ef6\u914d\u7f6e",   // 硬件配置
    L"\u53f3\u952e\u589e\u5f3a",   // 右键增强
    L"\u78c1\u76d8\u4fe1\u606f"    // 磁盘信息
};
static int  g_curTab = TAB_HW;        // v1.0.7 改为默认停在硬件配置页
static RECT g_tabRects[TAB_COUNT];
static int  g_tabHover[TAB_COUNT] = {0, 0, 0};

// ---- 硬件信息采集结果（全局缓存，硬件页直接读） ----
static HardwareInfo g_hw;
static BOOL         g_hwCollected = FALSE;

// ---- 通用：剪贴板写入（一键复制）+ 短暂反馈状态 ----
static DWORD g_copyAtTick = 0;          // 上次复制时刻，>0 即"已复制"反馈中
static RECT  g_copyBtnRect = {0};        // 当前页"一键复制"按钮矩形（与 g_curTab 同步）
static BOOL  g_copyBtnHover = FALSE;
static BOOL  g_mouseTracking = FALSE;   // WM_MOUSELEAVE 跟踪是否已注册（避免每次 MOUSEMOVE 重复注册）
static BOOL  CopyToClipboardW(const WCHAR* text);   // 实现在文件末尾

// ---- 磁盘信息（分区 / 簇大小等） ----
#define MAX_DISKS 32
struct DiskInfo {
    WCHAR        letter[8];      // "C:"
    const WCHAR* typeName;       // 指向静态类型名（本地磁盘 / 可移动磁盘 ...）
    WCHAR        fsName[16];     // NTFS / FAT32 ...（取不到则为空串）
    DWORD        sectorsPerCluster;
    DWORD        bytesPerSector;
    DWORD        clusterBytes;   // = sectorsPerCluster * bytesPerSector
    ULONGLONG    totalBytes;
    ULONGLONG    freeBytes;
    BOOL         ok;             // GetDiskFreeSpaceW 是否成功（空光驱/未就绪为 FALSE）
    // v1.0.11：盘符 → 物理磁盘号（NT 设备名 \\.\PhysicalDriveN）。
    // 跨盘卷（条带/镜像）会落在多个 PhysicalDrive 上，记首个 + 余下编号。
    int          diskIndex;      // 主 PhysicalDrive 编号；-1 = 无（RAM 盘/网络盘/失败）
    WCHAR        diskExtents[16];// 余下 extents 编号串，如 "+1"、"+1+2"；单盘/无时为空
    // v1.3.0：物理盘信息（用于展示"未分区/未初始化"磁盘）
    WCHAR        model[64];      // 物理盘型号（如 "Samsung SSD 980"）
    WCHAR        busType[16];    // 总线类型（NVMe / SATA / USB ...）
    WCHAR        serial[64];     // 序列号（v1.3.4 新增；来自 STORAGE_DEVICE_DESCRIPTOR）
    WCHAR        volLabel[64];   // 卷标（v1.3.5 新增；来自 GetVolumeInformationW，未分区盘为空）
    ULONGLONG    physSize;       // 物理盘总容量（字节），未分区盘以此为准
    BOOL         unpartitioned;  // TRUE=无挂载卷（未分区/未初始化）
};
static DiskInfo g_disks[MAX_DISKS];
static int      g_diskCount = 0;
static int      g_selDisk   = -1;        // 选中行（-1 = 无）
static int      g_hoverDiskRow = -1;
static BOOL     g_hoverDiskRefresh = FALSE;
static RECT     g_diskRowRect[MAX_DISKS];
static RECT     g_diskRefreshRect;

// ---- 自绘模态弹窗（替代原生 MessageBox，风格与主界面统一） ----
struct NoticeCtx {
    const WCHAR* caption;
    const WCHAR* body;
    BOOL         error;
    HWND         parent;
    int          dlgW, dlgH;
    RECT         btnRect;
    BOOL         btnHover;
};
static NoticeCtx g_notice = {0};
static const WCHAR* NOTICE_CLASS = L"WinNoticeDialog";

// 自绘模态提示框：圆角白底 + 信息/错误圆形图标 + 标题 + 自动换行正文 + 蓝色“确定”按钮。
// 风格与主界面卡片一致，避免原生 MessageBox 的突兀感。
static void ShowNotice(HWND parent, LPCWSTR cap, LPCWSTR body, BOOL error);

// ============================================================
// 绘图基元
// ============================================================

static void DrawRoundedRect(HDC hdc, RECT* r, int radius,
                            COLORREF fill, COLORREF border, int borderW = 1) {
    HPEN pen = CreatePen(PS_SOLID, borderW, border);
    HBRUSH brush = CreateSolidBrush(fill);
    HPEN op = (HPEN)SelectObject(hdc, pen);
    HBRUSH ob = (HBRUSH)SelectObject(hdc, brush);
    RoundRect(hdc, r->left, r->top, r->right, r->bottom, radius * 2, radius * 2);
    SelectObject(hdc, op);
    SelectObject(hdc, ob);
    DeleteObject(pen);
    DeleteObject(brush);
}

// 仅描边（不填充）的圆角矩形——用于图标线稿（填充由彩色瓦片本身提供）
static void StrokeRoundedRect(HDC hdc, RECT* r, int radius,
                              COLORREF stroke, int w = 2) {
    HPEN pen = CreatePen(PS_SOLID, w, stroke);
    HBRUSH br = (HBRUSH)GetStockObject(NULL_BRUSH);
    HPEN op = (HPEN)SelectObject(hdc, pen);
    HBRUSH ob = (HBRUSH)SelectObject(hdc, br);
    RoundRect(hdc, r->left, r->top, r->right, r->bottom, radius * 2, radius * 2);
    SelectObject(hdc, op);
    SelectObject(hdc, ob);
    DeleteObject(pen);
}

// 仅描边的圆（用于"眼睛"等图标线稿）
static void StrokeEllipse(HDC hdc, int cx, int cy, int r,
                          COLORREF stroke, int w = 2) {
    HPEN pen = CreatePen(PS_SOLID, w, stroke);
    HBRUSH br = (HBRUSH)GetStockObject(NULL_BRUSH);
    HPEN op = (HPEN)SelectObject(hdc, pen);
    HBRUSH ob = (HBRUSH)SelectObject(hdc, br);
    Ellipse(hdc, cx - r, cy - r, cx + r, cy + r);
    SelectObject(hdc, op);
    SelectObject(hdc, ob);
    DeleteObject(pen);
}

// 统一字体工厂（避免重复样板，降低告警风险）
static HFONT MakeFont(int hpx, int weight) {
    return CreateFontW(-hpx, 0, 0, 0, weight, FALSE, FALSE, FALSE,
        // 灰度抗锯齿（与 glass4 的 TextRenderingHintAntiAliasGridFit 一致）：
        // 渐变/半透背景上禁用 ClearType，避免亚像素彩边导致文字"发糊/不清晰"。
        DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
        ANTIALIASED_QUALITY, DEFAULT_PITCH | FF_SWISS, L"Microsoft YaHei");
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

// 卡片：白色圆角 + 阴影层级（常态"轻贴" / hover"浮起"）+ 左侧 3px 类型彩条
static void DrawCard(HDC hdc, RECT* r, BOOL hovered, COLORREF accent) {
    int rad = Scale(WIN_RADIUS);
    int cw = r->right - r->left, ch = r->bottom - r->top;
    if (g_pg) {
        // 真实高斯软阴影（按尺寸缓存，叠在渐变上 → 玻璃悬浮感）
        int blur = hovered ? 18 : 16;
        int pad = blur * 3 + 6;
        Bitmap* sh = GetShadow(cw, ch, hovered);
        if (sh) g_pg->DrawImage(sh, (INT)(r->left - pad), (INT)(r->top - pad));
        // 磨砂半透卡体（glass4 风格：极低透白 + 垂直渐变光泽，紫蓝渐变透出）
        LinearGradientBrush cb(RectF((float)r->left, (float)r->top, (float)cw, (float)ch),
            Color(60, 255, 255, 255), Color(34, 255, 255, 255), LinearGradientModeVertical);
        GraphicsPath p; RoundRectPath(&p, (float)r->left, (float)r->top, (float)cw, (float)ch, (float)rad);
        g_pg->FillPath(&cb, &p);
        // 边框：hover 用 accent，常态用淡白
        Color bc = hovered
            ? Color(255, (BYTE)GetRValue(accent), (BYTE)GetGValue(accent), (BYTE)GetBValue(accent))
            : Color(90, 255, 255, 255);
        Pen bp(bc, 1.0f);
        GraphicsPath p2; RoundRectPath(&p2, (float)r->left, (float)r->top, (float)cw, (float)ch, (float)rad);
        g_pg->DrawPath(&bp, &p2);
        g_pg->Flush(FlushIntentionSync);   // 确保后续 GDI 内容（图标/文字）正确合成
    }
    // 左侧 3px 圆角类型彩条（GDI 绘制，位于卡内）
    RECT ar = {r->left, r->top + Scale(14),
               r->left + Scale(3), r->bottom - Scale(14)};
    DrawRoundedRect(hdc, &ar, Scale(2), accent, accent);
}

// 整页磨砂玻璃面板：GDI+ 半透白（让紫蓝渐变透出）+ AA 圆角亮边
static void DrawGlassPanel(HDC hdc, RECT* r, int rad) {
    int cw = r->right - r->left, ch = r->bottom - r->top;
    if (g_pg) {
        LinearGradientBrush cb(RectF((float)r->left, (float)r->top, (float)cw, (float)ch),
            Color(45, 255, 255, 255), Color(28, 255, 255, 255), LinearGradientModeVertical);
        GraphicsPath p; RoundRectPath(&p, (float)r->left, (float)r->top, (float)cw, (float)ch, (float)rad);
        g_pg->FillPath(&cb, &p);
        Pen bp(Color(80, 255, 255, 255), 1.0f);
        GraphicsPath p2; RoundRectPath(&p2, (float)r->left, (float)r->top, (float)cw, (float)ch, (float)rad);
        g_pg->DrawPath(&bp, &p2);
        g_pg->Flush(FlushIntentionSync);
    } else {
        DrawRoundedRect(hdc, r, rad, C_PANEL_GLASS, C_GLASS_BORDER, 1);
    }
}

// 按钮：圆角 8px；主操作（启用）紫底白字（demo 启用态=紫色，与已启用徽章统一），
// 次操作（停用）极淡描边款（demo 风格，几乎隐入玻璃底，仅靠细边识别）
static void DrawButton(HDC hdc, RECT* r, const WCHAR* text,
                       BOOL hovered, BOOL primary) {
    // 与 glass4 glass_showcase.cpp:349-366 对齐
    //   primary  (启用): 实心紫(99,102,241) + 白字加粗，无 hover
    //   secondary(停用): 无填充 + 淡白细边 alpha=110 1px + 白字加粗，无 hover
    // 唯一例外：notice 模态"确定"按钮仍走 primary + hover 加深（深紫 79,70,229）
    // 关键：次按钮的边与字必须都在 GDI+ 同一帧合成（与 glass4 一致）。
    //   若边走 GDI+ 后在半透区再画 GDI 文字，会出彩边(255,204,210)，与 demo 不符。
    int rad = Scale(6);
    int bw = r->right - r->left, bh = r->bottom - r->top;
    SetBkMode(hdc, TRANSPARENT);

    if (primary) {
        // glass4 实心紫；hover 仅作用于 notice 模态（深紫）。实心填充上 GDI 白字干净。
        COLORREF fill = hovered ? RGB(79, 70, 229) : RGB(99, 102, 241);
        DrawRoundedRect(hdc, r, rad, fill, fill);
        SetTextColor(hdc, RGB(255, 255, 255));
        HFONT f = MakeFont(Scale(13), FW_BOLD);
        HFONT of = (HFONT)SelectObject(hdc, f);
        DrawTextW(hdc, text, -1, r, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        SelectObject(hdc, of);
        DeleteObject(f);
        return;
    }

    // secondary：无填充，淡白细边 + 白字，全部走 GDI+（边与字同一帧合成，杜绝彩边）
    if (g_pg) {
        GraphicsPath p;
        RoundRectPath(&p, (float)r->left, (float)r->top, (float)bw, (float)bh, (float)rad);
        Pen bp(Color(110, 255, 255, 255), 1.0f);
        g_pg->DrawPath(&bp, &p);
        HFONT hf = MakeFont(Scale(13), FW_BOLD);          // glass4 FontStyleBold
        HFONT of2 = (HFONT)SelectObject(hdc, hf);
        Font f(hdc, hf);
        SolidBrush br(Color(255, 255, 255, 255));
        StringFormat sf;
        sf.SetAlignment(StringAlignmentCenter);
        sf.SetLineAlignment(StringAlignmentCenter);
        sf.SetFormatFlags(StringFormatFlagsNoWrap);
        RectF rf((REAL)r->left, (REAL)r->top, (REAL)bw, (REAL)bh);
        g_pg->DrawString(text, -1, &f, rf, &sf, &br);
        SelectObject(hdc, of2);
        DeleteObject(hf);
        g_pg->Flush(FlushIntentionSync);
    } else {
        // 兜底：无 GDI+ 时退化淡白描边 + GDI 白字
        HPEN pen = CreatePen(PS_SOLID, 1, RGB(190, 200, 220));
        HBRUSH br = (HBRUSH)GetStockObject(NULL_BRUSH);
        HPEN op = (HPEN)SelectObject(hdc, pen);
        HBRUSH ob = (HBRUSH)SelectObject(hdc, br);
        RoundRect(hdc, r->left, r->top, r->right, r->bottom, rad*2, rad*2);
        SelectObject(hdc, op); SelectObject(hdc, ob);
        DeleteObject(pen);
        SetTextColor(hdc, RGB(255, 255, 255));
        HFONT f = MakeFont(Scale(13), FW_BOLD);
        HFONT of = (HFONT)SelectObject(hdc, f);
        DrawTextW(hdc, text, -1, r, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        SelectObject(hdc, of);
        DeleteObject(f);
    }
}

// 顶部操作按钮：与功能卡"停用"按钮同款白底描边，仅文字不同
// g_copyAtTick > 0 且距今 < 1200ms 时显示 doneLabel（"已复制" / "已导出"）反馈
static void DrawCopyButton(HDC hdc, RECT* r, BOOL hovered,
                           const WCHAR* baseLabel, const WCHAR* doneLabel) {
    DWORD now = GetTickCount();
    BOOL justCopied = (g_copyAtTick != 0 && (now - g_copyAtTick) < 1200);
    SetBkMode(hdc, TRANSPARENT);
    if (justCopied) {
        // 反馈态：蓝边蓝字
        DrawRoundedRect(hdc, r, Scale(6), C_BTN_SECONDARY, C_PRIMARY);
        SetTextColor(hdc, C_PRIMARY);
    } else {
        DrawRoundedRect(hdc, r, Scale(6),
                        hovered ? C_BTN_SECONDARY_H : C_BTN_SECONDARY,
                        hovered ? C_BTN_BORDER_H : C_BTN_BORDER);
        SetTextColor(hdc, C_TEXT);
    }
    HFONT f = MakeFont(Scale(13), FW_MEDIUM);
    HFONT of = (HFONT)SelectObject(hdc, f);
    DrawTextW(hdc, justCopied ? doneLabel : baseLabel, -1, r,
              DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    SelectObject(hdc, of);
    DeleteObject(f);
}

// 经典右键菜单复选框：圆角方块 + 勾选时填主色并画白色对勾
static void DrawCheckbox(HDC hdc, RECT* box, BOOL checked, BOOL hovered) {
    COLORREF fill   = checked ? (hovered ? C_PRIMARY_H : C_PRIMARY)
                             : (hovered ? C_BTN_SECONDARY_H : C_BTN_SECONDARY);
    COLORREF border = checked ? fill
                             : (hovered ? C_BTN_BORDER_H : C_BTN_BORDER);
    DrawRoundedRect(hdc, box, Scale(5), fill, border);
    if (checked) {
        HPEN wp = CreatePen(PS_SOLID, Scale(2), RGB(255, 255, 255));
        HPEN owp = (HPEN)SelectObject(hdc, wp);
        int bx = box->left, by = box->top, bw = box->right - box->left, bh = box->bottom - box->top;
        int cx = bx + bw / 2, cy = by + bh / 2;
        MoveToEx(hdc, bx + Scale(4), cy, NULL);
        LineTo(hdc, cx - Scale(1), by + bh - Scale(5));
        LineTo(hdc, bx + bw - Scale(4), by + Scale(4));
        SelectObject(hdc, owp);
        DeleteObject(wp);
    }
}

// {86ca1aa0-...} 这个注册表键是 Windows 11 专属 hack：存在 = 强制"经典（旧版）右键菜单"，
// 不存在 = Win11 默认的"新样式菜单"。Win10 及更早版本右键菜单本就是经典样式，该键不存在也不生效。
static BOOL IsWindows11() {
    DWORD major, minor, build;
    GetOSVersion(&major, &minor, &build);   // common.h：经 RtlGetVersion，不受 manifest 兼容性标记影响
    return (major == 10 && build >= 22000);
}

// 经典（旧版）右键菜单开关当前是否生效
static BOOL IsClassicMenuEnabled() {
    if (!IsWindows11()) return TRUE;   // Win10/更早：经典菜单是系统默认，恒为"已启用"
    // Win11：该 CLSID 的 InprocServer32 空值键存在 = 强制经典菜单。
    // 仅读 HKCU（切换操作也只写/删 HKCU）：若读 HKLM 作兜底，HKLM 残留键会让删除 HKCU 后
    // 函数仍返 TRUE，复选框"点完不变"；HKLM 策略不在本工具管理范围，故不纳入判定。
    return RegKeyExists(HKEY_CURRENT_USER, L"Software\\Classes\\CLSID\\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\\InprocServer32");
}

// 写入剪贴板（CF_UNICODETEXT）。返回是否成功。空文本也允许（清空剪贴板）。
static BOOL CopyToClipboardW(const WCHAR* text) {
    if (!OpenClipboard(NULL)) return FALSE;
    EmptyClipboard();
    size_t cb = (wcslen(text) + 1) * sizeof(WCHAR);
    HGLOBAL h = GlobalAlloc(GMEM_MOVEABLE, cb);
    if (!h) { CloseClipboard(); return FALSE; }
    void* p = GlobalLock(h);
    memcpy(p, text, cb);
    GlobalUnlock(h);
    SetClipboardData(CF_UNICODETEXT, h);   // 系统接管 h，调用者不再 GlobalFree
    CloseClipboard();
    return TRUE;
}

// 功能图标：彩色圆角瓦片 + 白色语义线稿（玻璃风，零运行时依赖）
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


// 状态药丸徽章（与 glass4 glass_showcase.cpp:334-346 像素对齐）
// 半透底色 + 指示圆点 + 文本，全部走 GDI+ 同一帧合成（半透区禁用 GDI，否则出彩边）。
//   启用态：绿底 A60 + 绿实心点 + 绿字 regular
//   未启用：白底 A40 + 灰描边空圆 + 浅蓝字 regular
static void DrawStatusBadge(HDC hdc, RECT* r, BOOL installed) {
    if (!g_pg) return;
    SetBkMode(hdc, TRANSPARENT);
    float bx = (float)r->left, by = (float)r->top;
    float bw = (float)(r->right - r->left), bh = (float)(r->bottom - r->top);

    // 半圆角底色（GDI+ 才有 alpha）
    SolidBrush bb(installed ? Color(60, 52, 211, 153) : Color(40, 255, 255, 255));
    GraphicsPath p; RoundRectPath(&p, bx, by, bw, bh, (float)Scale(11));
    g_pg->FillPath(&bb, &p);

    // 指示圆点：启用=绿实心，未启用=灰描边空圆
    if (installed) {
        SolidBrush db(Color(255, 150, 240, 190));
        g_pg->FillEllipse(&db, bx + Scale(12), by + bh / 2.0f - Scale(3),
                          (float)Scale(6), (float)Scale(6));
    } else {
        Pen ep(Color(210, 216, 230));
        g_pg->DrawEllipse(&ep, bx + Scale(12), by + bh / 2.0f - Scale(3),
                          (float)Scale(6), (float)Scale(6));
    }

    // 文本：regular 字重；启用=绿(150,240,190)，未启用=浅蓝(235,240,255)
    HFONT hf = MakeFont(Scale(11), FW_NORMAL);
    HFONT of = (HFONT)SelectObject(hdc, hf);
    Font f(hdc, hf);
    SolidBrush br(installed ? Color(255, 150, 240, 190) : Color(235, 240, 255));
    StringFormat sf;
    sf.SetAlignment(StringAlignmentNear);
    sf.SetLineAlignment(StringAlignmentCenter);
    sf.SetFormatFlags(StringFormatFlagsNoWrap);
    float sz = (float)Scale(11);
    RectF rf(bx + Scale(24), by + bh / 2.0f - sz * 0.7f, 2000.0f, sz * 1.4f);
    g_pg->DrawString(installed ? L"\u5df2\u542f\u7528" : L"\u672a\u542f\u7528", -1, &f, rf, &sf, &br);
    SelectObject(hdc, of);
    DeleteObject(hf);

    g_pg->Flush(FlushIntentionSync);  // 边+点+字同帧合成，杜绝半透区彩边
}

// 标题栏按钮：圆角背景（hover 高亮）+ 细线字形（minimize 横线 / close X）
static void DrawTitleButton(HDC hdc, RECT* r, BOOL hover, BOOL close) {
    if (hover) {
        COLORREF fill = close ? C_TB_CLOSE_HOVER : C_TB_HOVER;
        DrawRoundedRect(hdc, r, Scale(6), fill, fill);
    }
    int cx = (r->left + r->right) / 2, cy = (r->top + r->bottom) / 2;
    COLORREF c = hover ? (close ? RGB(255, 255, 255) : C_SUBTITLE) : C_SUBTITLE;
    HPEN p = CreatePen(PS_SOLID, 1, c);
    HPEN op = (HPEN)SelectObject(hdc, p);
    int g = Scale(5);   // 字形半幅
    if (close) {
        MoveToEx(hdc, cx - g, cy - g, NULL); LineTo(hdc, cx + g, cy + g);
        MoveToEx(hdc, cx - g, cy + g, NULL); LineTo(hdc, cx + g, cy - g);
    } else {
        MoveToEx(hdc, cx - g, cy, NULL); LineTo(hdc, cx + g, cy);
    }
    SelectObject(hdc, op);
    DeleteObject(p);
}

// ============================================================
// 自绘模态弹窗（替代原生 MessageBox，风格与主界面统一）
// ============================================================
static LRESULT CALLBACK NoticeProc(HWND hWnd, UINT uMsg, WPARAM wParam, LPARAM lParam) {
    switch (uMsg) {
    case WM_CREATE: {
        int W = Scale(380), P = Scale(24), iconSz = Scale(36), gap = Scale(14);
        int bodyW = W - 2 * P;
        // 计算正文高度（自动换行）
        HDC hdc = GetDC(hWnd);
        HFONT bf = MakeFont(Scale(13), FW_NORMAL);
        HFONT of = (HFONT)SelectObject(hdc, bf);
        RECT r = {0, 0, bodyW, 0};
        DrawTextW(hdc, g_notice.body, -1, &r, DT_LEFT | DT_WORDBREAK | DT_CALCRECT);
        int bodyH = r.bottom - r.top;
        SelectObject(hdc, of); DeleteObject(bf); ReleaseDC(hWnd, hdc);

        int btnW = Scale(96), btnH = Scale(34), btnGap = Scale(18);
        int H = P + iconSz + gap + bodyH + btnGap + btnH + P;
        g_notice.dlgW = W; g_notice.dlgH = H;
        SetWindowPos(hWnd, NULL, 0, 0, W, H, SWP_NOMOVE | SWP_NOZORDER | SWP_FRAMECHANGED);
        ApplyRoundedRegion(hWnd, W, H);

        // 确定按钮：右下角对齐
        g_notice.btnRect.left   = W - P - btnW;
        g_notice.btnRect.top    = H - P - btnH;
        g_notice.btnRect.right  = W - P;
        g_notice.btnRect.bottom = H - P;

        // 居中于父窗口（若有）
        if (g_notice.parent) {
            RECT pr; GetWindowRect(g_notice.parent, &pr);
            int px = (pr.left + pr.right) / 2 - W / 2;
            int py = (pr.top + pr.bottom) / 2 - H / 2;
            SetWindowPos(hWnd, NULL, px, py, 0, 0, SWP_NOSIZE | SWP_NOZORDER);
        }

        // Win11 DWM 圆角
        HMODULE hDwm = LoadLibraryW(L"dwmapi.dll");
        if (hDwm) {
            typedef HRESULT (WINAPI *DwmSetWindowAttribute_t)(HWND, DWORD, LPCVOID, DWORD);
            DwmSetWindowAttribute_t p = (DwmSetWindowAttribute_t)GetProcAddress(hDwm, "DwmSetWindowAttribute");
            if (p) { DWORD c = DWMWCP_ROUND; p(hWnd, DWMWA_WINDOW_CORNER_PREFERENCE, &c, sizeof(c)); }
            FreeLibrary(hDwm);
        }
        return 0;
    }

    case WM_PAINT: {
        PAINTSTRUCT ps; HDC hdc = BeginPaint(hWnd, &ps);
        int W = g_notice.dlgW, H = g_notice.dlgH, P = Scale(24), iconSz = Scale(36);

        // 内容圆角白底 + 细边框（无外阴影，简洁）
        RECT fullR = {0, 0, W, H};
        DrawRoundedRect(hdc, &fullR, Scale(10), C_CARD_BG, C_WIN_BORDER);

        // 圆形图标（信息蓝 / 错误红）
        int ix = P, iy = P;
        COLORREF ic = g_notice.error ? RGB(220, 38, 38) : C_PRIMARY;
        HBRUSH ib = CreateSolidBrush(ic);
        HPEN ip = CreatePen(PS_SOLID, 1, ic);
        HPEN oip = (HPEN)SelectObject(hdc, ip);
        HBRUSH oib = (HBRUSH)SelectObject(hdc, ib);
        Ellipse(hdc, ix, iy, ix + iconSz, iy + iconSz);
        SelectObject(hdc, oip); SelectObject(hdc, oib);
        DeleteObject(ip); DeleteObject(ib);
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, RGB(255, 255, 255));
        HFONT sf = MakeFont(Scale(20), FW_BOLD);
        HFONT osf = (HFONT)SelectObject(hdc, sf);
        RECT sr = {ix, iy, ix + iconSz, iy + iconSz};
        DrawTextW(hdc, g_notice.error ? L"!" : L"i", -1, &sr, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        SelectObject(hdc, osf); DeleteObject(sf);

        // 标题（图标右侧垂直居中）
        HFONT cf = MakeFont(Scale(15), FW_SEMIBOLD);
        HFONT ocf = (HFONT)SelectObject(hdc, cf);
        SetTextColor(hdc, C_TITLE);
        RECT cr = {ix + iconSz + Scale(12), iy, W - P, iy + iconSz};
        DrawTextW(hdc, g_notice.caption, -1, &cr, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        SelectObject(hdc, ocf); DeleteObject(cf);

        // 正文（图标下自动换行）
        int bodyTop = iy + iconSz + Scale(14);
        HFONT bf = MakeFont(Scale(13), FW_NORMAL);
        HFONT obf = (HFONT)SelectObject(hdc, bf);
        SetTextColor(hdc, C_TEXT);
        RECT br = {P, bodyTop, W - P, H - P - Scale(34) - Scale(18)};
        DrawTextW(hdc, g_notice.body, -1, &br, DT_LEFT | DT_WORDBREAK | DT_VCENTER);
        SelectObject(hdc, obf); DeleteObject(bf);

        // 确定按钮
        DrawButton(hdc, &g_notice.btnRect, L"\u786e\u5b9a", g_notice.btnHover, TRUE);

        EndPaint(hWnd, &ps);
        return 0;
    }

    case WM_MOUSEMOVE: {
        POINT pt = {LOWORD(lParam), HIWORD(lParam)};
        BOOL h = PtInRect(&g_notice.btnRect, pt);
        if (h != g_notice.btnHover) { g_notice.btnHover = h; InvalidateRect(hWnd, NULL, FALSE); }
        TRACKMOUSEEVENT tme = {sizeof(tme), TME_LEAVE, hWnd, 0};
        TrackMouseEvent(&tme);
        return 0;
    }
    case WM_MOUSELEAVE:
        if (g_notice.btnHover) { g_notice.btnHover = FALSE; InvalidateRect(hWnd, NULL, FALSE); }
        return 0;

    case WM_LBUTTONDOWN: {
        POINT pt = {LOWORD(lParam), HIWORD(lParam)};
        if (PtInRect(&g_notice.btnRect, pt)) DestroyWindow(hWnd);
        return 0;
    }
    case WM_KEYDOWN:
        if (wParam == VK_RETURN || wParam == VK_ESCAPE) DestroyWindow(hWnd);
        return 0;
    case WM_NCHITTEST: {
        // 按钮区域返回 HTCLIENT：否则 WM_NCHITTEST 整体返回 HTCAPTION 会被当成
        // 标题栏拖拽（WM_NCLBUTTONDOWN），导致"确定"按钮的 WM_LBUTTONDOWN 永不触发。
        POINT pt = {GET_X_LPARAM(lParam), GET_Y_LPARAM(lParam)};
        ScreenToClient(hWnd, &pt);
        if (PtInRect(&g_notice.btnRect, pt)) return HTCLIENT;
        return HTCAPTION; // 其余区域允许拖动
    }
    case WM_DESTROY:
        // 注意：自绘弹窗销毁时不要 PostQuitMessage，否则会误退出主窗口循环；
        // 模态循环依据 IsWindow(h) 判断是否结束。
        return 0;
    }
    return DefWindowProc(hWnd, uMsg, wParam, lParam);
}

// 模态显示自绘提示框（替代 MessageBox）。owner 窗口由系统自动禁用/启用。
static void ShowNotice(HWND parent, LPCWSTR cap, LPCWSTR body, BOOL error) {
    g_notice.caption  = cap;
    g_notice.body     = body;
    g_notice.error    = error;
    g_notice.parent   = parent;
    g_notice.btnHover = FALSE;
    HWND h = CreateWindowExW(0, NOTICE_CLASS, L"",
        WS_POPUP | WS_VISIBLE, CW_USEDEFAULT, CW_USEDEFAULT, 380, 200,
        parent, NULL, g_hInst, NULL);
    if (!h) return;
    MSG msg;
    while (GetMessage(&msg, NULL, 0, 0)) {
        TranslateMessage(&msg);
        DispatchMessage(&msg);
        if (!IsWindow(h)) break;   // 弹窗已销毁则退出模态循环
    }
}

// 窗口层面：纯自绘圆角白底 + 1px 清晰外边框。
// 不再绘制灰色外阴影（原 SHADOW_PAD 8px 灰边让用户觉得"四周都有阴影很别扭"），
// 柔和投影改由窗口类的 CS_DROPSHADOW（系统原生、轻量）提供。
// 竖向渐变填充（纯 GDI，无外部依赖）：自上而下从 top 平滑过渡到 bot。
// 每 2px 一档，单次重绘开销可忽略；窗口固定尺寸，重绘频率低。
static void FillVGradient(HDC hdc, RECT* r, COLORREF top, COLORREF bot) {
    int h = r->bottom - r->top;
    if (h <= 0) return;
    int tr = GetRValue(top), tg = GetGValue(top), tb = GetBValue(top);
    int br = GetRValue(bot), bg = GetGValue(bot), bb = GetBValue(bot);
    for (int y = r->top; y < r->bottom; y += 2) {
        int t = (y - r->top) * 100 / h;
        int cr = tr + (br - tr) * t / 100;
        int cg = tg + (bg - tg) * t / 100;
        int cb = tb + (bb - tb) * t / 100;
        HBRUSH b = CreateSolidBrush(RGB((BYTE)cr, (BYTE)cg, (BYTE)cb));
        RECT band = {r->left, y, r->right, (y + 2 < r->bottom) ? y + 2 : r->bottom};
        FillRect(hdc, &band, b);
        DeleteObject(b);
    }
}

// 功能图标瓦片：圆角彩块 + 顶部高光，模拟玻璃质感（仍零运行时依赖）
static void DrawAccentTile(HDC hdc, RECT* r, COLORREF accent) {
    DrawRoundedRect(hdc, r, Scale(11), accent, accent);
    // 顶部高光带（预烘焙浅色，纯 GDI 无 alpha 通道）
    int lr = GetRValue(accent) + (255 - GetRValue(accent)) * 22 / 100;
    int lg = GetGValue(accent) + (255 - GetGValue(accent)) * 22 / 100;
    int lb = GetBValue(accent) + (255 - GetBValue(accent)) * 22 / 100;
    RECT hr = {r->left + Scale(3), r->top + Scale(3),
               r->right - Scale(3), r->top + (r->bottom - r->top) * 2 / 5};
    DrawRoundedRect(hdc, &hr, Scale(7),
                    RGB((BYTE)lr, (BYTE)lg, (BYTE)lb),
                    RGB((BYTE)lr, (BYTE)lg, (BYTE)lb));
}

static void DrawWindowFrame(HDC hdc, int W, int H) {
    if (g_pg) {
        LinearGradientBrush bg(RectF(0, 0, (float)W, (float)H),
            Color(255, (BYTE)GetRValue(C_GRAD_TOP), (BYTE)GetGValue(C_GRAD_TOP), (BYTE)GetBValue(C_GRAD_TOP)),
            Color(255, (BYTE)GetRValue(C_GRAD_BOT), (BYTE)GetGValue(C_GRAD_BOT), (BYTE)GetBValue(C_GRAD_BOT)),
            LinearGradientModeVertical);
        g_pg->FillRectangle(&bg, 0, 0, W, H);
    } else {
        RECT full = {0, 0, W, H};
        FillVGradient(hdc, &full, C_GRAD_TOP, C_GRAD_BOT);
    }
}

// ============================================================
// 布局
// ============================================================

static void CenterWindow(HWND hWnd) {
    RECT s; GetWindowRect(GetDesktopWindow(), &s);
    // 用缩放后的客户区尺寸居中（g_clientW/g_clientH 由 ComputeLayout 维护）
    SetWindowPos(hWnd, NULL,
        (s.right - s.left - g_clientW) / 2,
        (s.bottom - s.top - g_clientH) / 2,
        0, 0, SWP_NOSIZE | SWP_NOZORDER);
}

// 动态取窗口 DPI：优先 GetDpiForWindow（user32，Win10 1607+），
// 拿不到（旧系统 / MinGW 旧导入库缺符号）则回退主显示器 LOGPIXELSY。
// 全程动态加载，绝不静态链接，确保 Win7/8 上能正常加载且不崩溃。
typedef UINT (WINAPI *GetDpiForWindow_t)(HWND);
static UINT GetWindowDpi(HWND hWnd) {
    static GetDpiForWindow_t pGetDpi = NULL;
    static BOOL resolved = FALSE;
    if (!resolved) {
        HMODULE u = GetModuleHandleW(L"user32.dll");
        if (u) pGetDpi = (GetDpiForWindow_t)GetProcAddress(u, "GetDpiForWindow");
        resolved = TRUE;
    }
    if (pGetDpi) return pGetDpi(hWnd);
    HDC dc = GetDC(NULL);
    int d = dc ? GetDeviceCaps(dc, LOGPIXELSY) : 96;
    if (dc) ReleaseDC(NULL, dc);
    return (UINT)(d ? d : 96);
}

// 按当前 DPI 重建窗口圆角 Region（外缘与自绘边框同心，保证圆角视觉一致）。
// SetWindowRgn 接收后由系统接管 HRGN 生命周期，重复调用自动释放旧 Region，无句柄泄漏。
static void ApplyRoundedRegion(HWND hWnd, int w, int h) {
    int r = Scale(WIN_RADIUS + SHADOW_PAD);   // 外缘圆角半径（= 边框半径 + 留白）
    HRGN hr = CreateRoundRectRgn(0, 0, w, h, r * 2, r * 2);
    if (hr) SetWindowRgn(hWnd, hr, TRUE);
}

// ============================================================
// 磁盘信息：容量 / 簇大小格式化 + 分区枚举
// ============================================================

// 容量格式化（TB/GB/MB/KB/B，保留 1 位小数）
// 注意：MinGW 宽字符 printf 中宽字符串一律用 %ls，禁用 %s（同页脚版权行的坑）。
static void FormatBytesStr(ULONGLONG bytes, WCHAR* buf, int n) {
    const WCHAR* unit;
    double v;
    if      (bytes >= 1024ULL*1024*1024*1024) { v = (double)bytes / (1024.0*1024*1024*1024); unit = L"TB"; }
    else if (bytes >= 1024ULL*1024*1024)      { v = (double)bytes / (1024.0*1024*1024);      unit = L"GB"; }
    else if (bytes >= 1024ULL*1024)           { v = (double)bytes / (1024.0*1024);           unit = L"MB"; }
    else if (bytes >= 1024ULL)                { v = (double)bytes / 1024.0;                  unit = L"KB"; }
    else { swprintf(buf, n, L"%llu B", bytes); return; }
    swprintf(buf, n, L"%.1f %ls", v, unit);
}

// 整数容量串（无小数，四舍五入）：用于内存容量——内存条都是整 GB，".0" 显得累赘，
// 且 SMBIOS 上报的 memTotalMB 通常略小于真实容量（如 16364 MB），直接整除会掉到 "15 GB"，
// 故这里 +u/2 做四舍五入，保证显示 "16 GB"。磁盘容量仍用 FormatBytesStr（保留一位小数）。
static void FormatBytesInt(ULONGLONG bytes, WCHAR* buf, int n) {
    const WCHAR* unit; ULONGLONG v, u;
    if      (bytes >= 1024ULL*1024*1024*1024) { u = 1024ULL*1024*1024*1024; unit = L"TB"; }
    else if (bytes >= 1024ULL*1024*1024)      { u = 1024ULL*1024*1024;      unit = L"GB"; }
    else if (bytes >= 1024ULL*1024)           { u = 1024ULL*1024;           unit = L"MB"; }
    else if (bytes >= 1024ULL)                { u = 1024ULL;                unit = L"KB"; }
    else { swprintf(buf, n, L"%llu B", bytes); return; }
    v = (bytes + u/2) / u;   // 四舍五入到整数单位
    swprintf(buf, n, L"%llu %ls", v, unit);
}

// 簇大小：>=1KB 显示 KB（4 KB / 64 KB），否则显示字节数
static void FormatClusterStr(DWORD bytes, WCHAR* buf, int n) {
    if (bytes >= 1024) swprintf(buf, n, L"%.0f KB", (double)bytes / 1024.0);
    else               swprintf(buf, n, L"%lu B", bytes);
}

// 驱动器类型（返回静态串，无需释放）
static const WCHAR* GetDriveTypeName(UINT type) {
    switch (type) {
        case DRIVE_REMOVABLE: return L"\u53ef\u79fb\u52a8\u78c1\u76d8";  // 可移动磁盘
        case DRIVE_FIXED:     return L"\u672c\u5730\u78c1\u76d8";        // 本地磁盘
        case DRIVE_REMOTE:    return L"\u7f51\u7edc\u78c1\u76d8";        // 网络磁盘
        case DRIVE_CDROM:     return L"\u5149\u76d8\u9a71\u52a8\u5668";  // 光盘驱动器
        case DRIVE_RAMDISK:   return L"RAM\u78c1\u76d8";                 // RAM磁盘
        default:              return L"\u672a\u77e5\u7c7b\u578b";        // 未知类型
    }
}

// v1.0.11：通过 IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS 拿盘符 → PhysicalDriveN 的映射。
// 跨盘卷（条带 / 镜像）会落到多个 PhysicalDrive，extents 数组都填；
// 返回首个 extents 的 DiskNumber，余下编号拼成 "+1" "+1+2" 形式写到 extentsBuf。
// 失败（权限 / 无卷 / RAM 盘）返回 -1，extentsBuf 留空。
static int GetVolumeDiskIndex(const WCHAR* root, WCHAR* extentsBuf, size_t extentsChars) {
    if (extentsBuf && extentsChars) extentsBuf[0] = 0;
    // 构造 "\\\\.\\C:" 形式的卷路径
    WCHAR volPath[8];
    StringCchCopyW(volPath, _countof(volPath), L"\\\\.\\");
    volPath[4] = root[0];   // 盘符
    volPath[5] = L':';
    volPath[6] = 0;
    HANDLE hv = CreateFileW(volPath, 0, FILE_SHARE_READ | FILE_SHARE_WRITE,
                            NULL, OPEN_EXISTING, 0, NULL);
    if (hv == INVALID_HANDLE_VALUE) return -1;
    struct {
        DWORD         NumberOfDiskExtents;
        DISK_EXTENT   Extents[8];
    } ext;
    DWORD ret = 0;
    BOOL ok = DeviceIoControl(hv, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS,
                              NULL, 0, &ext, sizeof(ext), &ret, NULL);
    CloseHandle(hv);
    if (!ok || ext.NumberOfDiskExtents == 0) return -1;
    if (ext.NumberOfDiskExtents > 8) ext.NumberOfDiskExtents = 8;
    if (extentsBuf && extentsChars) {
        extentsBuf[0] = 0;
        WCHAR* p = extentsBuf;
        size_t remain = extentsChars;
        for (DWORD i = 1; i < ext.NumberOfDiskExtents; i++) {
            StringCchPrintfW(p, remain, L"+%u", ext.Extents[i].DiskNumber);
            size_t n = wcslen(p);
            p += n; remain -= n;
        }
    }
    return (int)ext.Extents[0].DiskNumber;
}

// 总线类型名（本地副本：BusTypeName 定义在 hardware.cpp，本文件不引用）
static const WCHAR* LocalBusTypeName(STORAGE_BUS_TYPE t) {
    switch (t) {
        case BusTypeNvme:    return L"NVMe";
        case BusTypeSata:    return L"SATA";
        case BusTypeSas:     return L"SAS";
        case BusTypeUsb:     return L"USB";
        case BusTypeAta:     return L"ATA";
        case BusTypeScsi:    return L"SCSI";
        case BusTypeRAID:    return L"RAID";
        case BusTypeVirtual: return L"虚拟";
        default:             return L"";
    }
}

// 清理磁盘型号字符串：去首尾空格、合并连续空格、去除末尾孤立 "-"。
// 某些 SSD 固件在 ProductId 尾部留空格和 "-"，直接拼接会显示成 "INTEL SSDS -"。
static void NormalizeDiskModel(WCHAR* s) {
    if (!s || !s[0]) return;
    int len = (int)wcslen(s), start = 0;
    while (start < len && s[start] == L' ') start++;
    while (len > start && s[len - 1] == L' ') len--;
    int out = 0; BOOL prevSpace = FALSE;
    for (int i = start; i < len; i++) {
        if (s[i] == L' ') {
            if (!prevSpace) { s[out++] = L' '; prevSpace = TRUE; }
        } else {
            s[out++] = s[i]; prevSpace = FALSE;
        }
    }
    s[out] = 0;
    // 去掉末尾孤立 " -" 或单独 "-"
    int n = (int)wcslen(s);
    if (n >= 2 && s[n - 1] == L'-' && s[n - 2] == L' ') {
        n -= 2; s[n] = 0;
    } else if (n == 1 && s[0] == L'-') {
        s[0] = 0; return;
    }
    // 再次去尾部空格
    while (n > 0 && s[n - 1] == L' ') s[--n] = 0;
}

// 读取物理盘序列号：打开 \\.\PhysicalDriveN，查询 STORAGE_DEVICE_DESCRIPTOR 的 SerialNumber。
// 多字节字段转宽字符并去首尾空格；返回是否取到非空序列号。
// 用 0 访问权限打开（仅查询，不锁定），兼容已挂载/在用的磁盘。
static BOOL QueryDiskSerial(int pd, WCHAR* out, int outLen) {
    out[0] = 0;
    WCHAR path[32];
    StringCchPrintfW(path, _countof(path), L"\\\\.\\PhysicalDrive%d", pd);
    HANDLE h = CreateFileW(path, 0, FILE_SHARE_READ | FILE_SHARE_WRITE, NULL, OPEN_EXISTING, 0, NULL);
    if (h == INVALID_HANDLE_VALUE) return FALSE;
    BYTE raw[1024] = {0}; DWORD ret = 0;
    STORAGE_PROPERTY_QUERY q; ZeroMemory(&q, sizeof(q));
    q.PropertyId = StorageDeviceProperty; q.QueryType = PropertyStandardQuery;
    BOOL ok = FALSE;
    if (DeviceIoControl(h, IOCTL_STORAGE_QUERY_PROPERTY, &q, sizeof(q), raw, sizeof(raw), &ret, NULL) && ret) {
        STORAGE_DEVICE_DESCRIPTOR* sd = (STORAGE_DEVICE_DESCRIPTOR*)raw;
        if (sd->SerialNumberOffset && sd->SerialNumberOffset < ret) {
            int mb = (int)(ret - sd->SerialNumberOffset);
            MultiByteToWideChar(CP_UTF8, 0, (char*)(raw + sd->SerialNumberOffset), mb, out, outLen - 1);
            out[outLen - 1] = 0;
            int len = (int)wcslen(out), st = 0;          // 去首尾空格
            while (st < len && out[st] == L' ') st++;
            while (len > st && out[len - 1] == L' ') len--;
            if (st > 0 || len < (int)wcslen(out)) {
                int o = 0;
                for (int i = st; i < len; i++) out[o++] = out[i];
                out[o] = 0;
            }
            ok = (out[0] != 0);
        }
    }
    CloseHandle(h);
    return ok;
}

// 枚举所有逻辑分区，填充 g_disks / g_diskCount / g_selDisk
// 注意两点：
//  1) 未就绪介质（空光驱 / 未插卡的读卡器）访问时会阻塞 UI 线程并可能弹系统
//     "请插入磁盘"对话框。故用 SEM_FAILCRITICALERRORS 抑制弹窗，并跳过
//     DRIVE_NO_ROOT_DIR（无效根路径）以避免无意义的阻塞式访问。
//  2) 刷新时尽量保留用户原先选中的盘符（该盘符仍存在则保持选中），避免选择被重置。
// 非 static：selftest 调一次以让 Report_BuildHtml 看到真实分区表。
void EnumDisks() {
    WCHAR prevLetter = 0;
    if (g_selDisk >= 0 && g_selDisk < g_diskCount) prevLetter = g_disks[g_selDisk].letter[0];

    int volDiskIndex[MAX_DISKS];   // 已枚举分区的物理盘号，用于判断某物理盘是否已分区
    int volCount = 0;

    g_diskCount = 0;
    WCHAR drives[1024] = {0};
    GetLogicalDriveStringsW(_countof(drives) - 1, drives);

    UINT oldErrMode = SetErrorMode(SEM_FAILCRITICALERRORS);
    WCHAR* p = drives;
    while (*p && g_diskCount < MAX_DISKS) {
        DiskInfo* d = &g_disks[g_diskCount];
        d->letter[0] = p[0]; d->letter[1] = L':'; d->letter[2] = 0;
        WCHAR root[4] = { p[0], L':', L'\\', 0 };
        UINT dtype = GetDriveTypeW(root);
        d->typeName = GetDriveTypeName(dtype);
        d->fsName[0] = 0;
        d->sectorsPerCluster = 0;
        d->bytesPerSector    = 0;
        d->clusterBytes      = 0;
        d->totalBytes        = 0;
        d->freeBytes         = 0;
        d->ok = FALSE;
        d->diskIndex = -1;
        d->diskExtents[0] = 0;
        d->model[0] = 0;
        d->busType[0] = 0;
        d->serial[0] = 0;
        d->volLabel[0] = 0;   // v1.3.5：卷标（分区盘由 GetVolumeInformationW 填充，这里先清零）
        d->physSize = 0;
        d->unpartitioned = FALSE;
        if (dtype != DRIVE_NO_ROOT_DIR) {
            DWORD spc = 0, bps = 0, freeClusters = 0, totalClusters = 0;
            if (GetDiskFreeSpaceW(root, &spc, &bps, &freeClusters, &totalClusters)) {
                d->ok = TRUE;
                d->sectorsPerCluster = spc;
                d->bytesPerSector    = bps;
                // 统一先提升到 ULONGLONG 再乘，避免 32 位乘法溢出
                d->clusterBytes = (DWORD)((ULONGLONG)spc * bps);
                d->totalBytes   = (ULONGLONG)totalClusters * spc * bps;
                d->freeBytes    = (ULONGLONG)freeClusters  * spc * bps;
            }
            GetVolumeInformationW(root, d->volLabel, _countof(d->volLabel),
                                  NULL, NULL, NULL, d->fsName, _countof(d->fsName));
            // v1.0.11：盘符 → PhysicalDriveN（跨盘卷会拼出 "+1"）
            d->diskIndex = GetVolumeDiskIndex(root, d->diskExtents, _countof(d->diskExtents));
        }
        if (volCount < MAX_DISKS) volDiskIndex[volCount++] = d->diskIndex;
        g_diskCount++;
        p += wcslen(p) + 1;
    }

    // v1.3.0：补"未分区/未初始化"物理磁盘——探测 PhysicalDrive0..MAX_PD-1，
    // 仅当该物理盘没有任何已枚举分区时才补一行（已分区的盘由其分区行代表，避免重复）。
    const int MAX_PD = 32;
    for (int pd = 0; pd < MAX_PD && g_diskCount < MAX_DISKS; pd++) {
        BOOL hasVol = FALSE;
        for (int v = 0; v < volCount; v++) if (volDiskIndex[v] == pd) { hasVol = TRUE; break; }
        if (hasVol) continue;

        WCHAR path[32];
        StringCchPrintfW(path, _countof(path), L"\\\\.\\PhysicalDrive%d", pd);
        HANDLE h = CreateFileW(path, GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE,
                               NULL, OPEN_EXISTING, 0, NULL);
        if (h == INVALID_HANDLE_VALUE)
            h = CreateFileW(path, 0, FILE_SHARE_READ | FILE_SHARE_WRITE, NULL, OPEN_EXISTING, 0, NULL);
        if (h == INVALID_HANDLE_VALUE) continue;

        BYTE raw[1024] = {0}; DWORD ret = 0;
        STORAGE_PROPERTY_QUERY q; ZeroMemory(&q, sizeof(q));
        q.PropertyId = StorageDeviceProperty; q.QueryType = PropertyStandardQuery;
        WCHAR model[128] = {0}; STORAGE_BUS_TYPE bus = BusTypeUnknown; WCHAR ser[64] = {0};
        if (DeviceIoControl(h, IOCTL_STORAGE_QUERY_PROPERTY, &q, sizeof(q), raw, sizeof(raw), &ret, NULL) && ret) {
            STORAGE_DEVICE_DESCRIPTOR* sd = (STORAGE_DEVICE_DESCRIPTOR*)raw;
            bus = (STORAGE_BUS_TYPE)sd->BusType;
            WCHAR vend[64] = {0}, prod[96] = {0};
            if (sd->VendorIdOffset && sd->VendorIdOffset < ret) {
                int mb = (int)(ret - sd->VendorIdOffset);
                MultiByteToWideChar(CP_UTF8, 0, (char*)(raw + sd->VendorIdOffset), mb, vend, (int)_countof(vend) - 1);
                vend[_countof(vend) - 1] = 0;
            }
            if (sd->ProductIdOffset && sd->ProductIdOffset < ret) {
                int mb = (int)(ret - sd->ProductIdOffset);
                MultiByteToWideChar(CP_UTF8, 0, (char*)(raw + sd->ProductIdOffset), mb, prod, (int)_countof(prod) - 1);
                prod[_countof(prod) - 1] = 0;
            }
            for (int k = (int)wcslen(vend) - 1; k >= 0 && vend[k] == L' '; k--) vend[k] = 0;
            for (int k = (int)wcslen(prod) - 1; k >= 0 && prod[k] == L' '; k--) prod[k] = 0;
            if (vend[0] && prod[0]) StringCchPrintfW(model, _countof(model), L"%ls %ls", vend, prod);
            else if (prod[0])       StringCchCopyW(model, _countof(model), prod);
            else if (vend[0])       StringCchCopyW(model, _countof(model), vend);
            NormalizeDiskModel(model);
            if (sd->SerialNumberOffset && sd->SerialNumberOffset < ret) {   // v1.3.4：序列号
                int mb = (int)(ret - sd->SerialNumberOffset);
                MultiByteToWideChar(CP_UTF8, 0, (char*)(raw + sd->SerialNumberOffset), mb, ser, (int)_countof(ser) - 1);
                ser[_countof(ser) - 1] = 0;
                int sl = (int)wcslen(ser), ss = 0;   // 去首尾空格
                while (ss < sl && ser[ss] == L' ') ss++;
                while (sl > ss && ser[sl - 1] == L' ') sl--;
                if (ss > 0 || sl < (int)wcslen(ser)) {
                    int o = 0;
                    for (int i = ss; i < sl; i++) ser[o++] = ser[i];
                    ser[o] = 0;
                }
            }
        }
        ULONGLONG size = 0; GET_LENGTH_INFORMATION gli = {0};
        if (DeviceIoControl(h, IOCTL_DISK_GET_LENGTH_INFO, NULL, 0, &gli, sizeof(gli), &ret, NULL))
            size = (ULONGLONG)gli.Length.QuadPart;
        CloseHandle(h);

        if (!model[0] && !size) continue;   // 什么都没读到（空卡槽等），跳过

        DiskInfo* d = &g_disks[g_diskCount];
        d->letter[0] = L'\u2014'; d->letter[1] = 0;   // "—"
        d->typeName  = L"\u672a\u5206\u533a";          // 未分区
        d->fsName[0] = 0;
        d->sectorsPerCluster = 0; d->bytesPerSector = 0; d->clusterBytes = 0;
        d->totalBytes = size; d->freeBytes = 0; d->ok = TRUE;
        d->diskIndex = pd; d->diskExtents[0] = 0;
        StringCchCopyW(d->model, _countof(d->model), model[0] ? model : L"\u672a\u77e5\u578b\u53f7");
        const WCHAR* bt = LocalBusTypeName(bus);
        StringCchCopyW(d->busType, _countof(d->busType), bt ? bt : L"");
        StringCchCopyW(d->serial, _countof(d->serial), ser);   // v1.3.4：未分区盘序列号
        d->volLabel[0] = 0;   // v1.3.5：未分区盘无卷标（无挂载卷）
        d->physSize = size;
        d->unpartitioned = TRUE;
        g_diskCount++;
    }

    // v1.3.4：分区盘（diskIndex>=0）补序列号——未分区盘已在上面从描述符取过，
    // 此处仅给分区盘查一次；同一物理盘多个卷会重复查询（刷新期、数量小，可接受）。
    for (int i = 0; i < g_diskCount; i++) {
        DiskInfo* d = &g_disks[i];
        if (d->diskIndex >= 0 && d->serial[0] == 0)
            QueryDiskSerial(d->diskIndex, d->serial, _countof(d->serial));
    }

    SetErrorMode(oldErrMode);

    // 选中策略：恢复刷新前选中的盘符 -> 首个可访问分区 -> 第 0 行。
    // 候选限制在可见行内（超过 DISK_MAX_ROWS 的行不显示，选中也无意义）。
    int limit = (g_diskCount < DISK_MAX_ROWS) ? g_diskCount : DISK_MAX_ROWS;
    g_selDisk = -1;
    if (prevLetter) {
        for (int i = 0; i < limit; i++) {
            if (g_disks[i].letter[0] == prevLetter) { g_selDisk = i; break; }
        }
    }
    if (g_selDisk < 0) {
        for (int i = 0; i < limit; i++) {
            if (g_disks[i].ok) { g_selDisk = i; break; }
        }
    }
    if (g_selDisk < 0 && limit > 0) g_selDisk = 0;
    g_hoverDiskRow = -1;   // 行集合可能已变化，清掉悬停高亮避免残留
}

// 磁盘页"一键复制"用纯文本摘要。out 需 ≥2048 字符。
static void Disk_BuildText(const DiskInfo* disks, int count, int selIdx,
                           WCHAR* out, size_t nChars) {
    WCHAR cs[32];
    swprintf(out, nChars, L"[磁盘信息] 共 %d 个分区\r\n\r\n", count);
    size_t used = wcslen(out);
    for (int i = 0; i < count && i < DISK_MAX_ROWS; i++) {
        const DiskInfo* d = &disks[i];
        FormatClusterStr(d->clusterBytes, cs, _countof(cs));
        WCHAR line[256];
        WCHAR total[32], freeb[32];
        FormatBytesStr(d->totalBytes, total, _countof(total));
        FormatBytesStr(d->freeBytes,  freeb, _countof(freeb));
        WCHAR pdStr[24];
        if (d->diskIndex >= 0) swprintf(pdStr, _countof(pdStr),
                                        L"\\\\.\\PhysicalDrive%d%s",
                                        d->diskIndex, d->diskExtents);
        else                   wcscpy(pdStr, L"-");
        swprintf(line, _countof(line),
                 L"%ls  [%ls]  %ls  %ls  %ls  总 %ls / 可用 %ls%s\r\n",
                 d->letter, pdStr, d->typeName, cs,
                 d->fsName[0] ? d->fsName : L"-",
                 total, freeb,
                 d->ok ? L"" : L"  [不可访问]");
        StringCchCatW(out, nChars, line);
        used += wcslen(line);
    }
    if (selIdx >= 0 && selIdx < count && selIdx < DISK_MAX_ROWS) {
        const DiskInfo* d = &disks[selIdx];
        WCHAR detail[320];
        WCHAR pdStr[24];
        if (d->diskIndex >= 0) swprintf(pdStr, _countof(pdStr),
                                        L"\\\\.\\PhysicalDrive%d%s",
                                        d->diskIndex, d->diskExtents);
        else                   wcscpy(pdStr, L"-");
        if (d->unpartitioned) {
            WCHAR phys[32];
            FormatBytesStr(d->physSize, phys, _countof(phys));
            swprintf(detail, _countof(detail),
                     L"\r\n[选中详情] %ls  (NT 设备名：%ls)\r\n"
                     L"  型号 = %ls\r\n"
                     L"  总线 = %ls\r\n"
                     L"  序列号 = %ls\r\n"
                     L"  总容量 = %ls  （未分区/未初始化，无可访问卷）\r\n",
                     d->letter, pdStr,
                     d->model[0] ? d->model : L"-",
                     d->busType[0] ? d->busType : L"-",
                     d->serial[0] ? d->serial : L"-",
                     phys);
        } else if (d->ok) {
            swprintf(detail, _countof(detail),
                     L"\r\n[选中详情] %ls  (NT 设备名：%ls)\r\n"
                     L"  簇 = 每簇 %u 扇区 × %u 字节/扇区 = %u 字节\r\n"
                     L"  簇大小 = %ls\r\n"
                     L"  文件系统 = %ls\r\n"
                     L"  卷标 = %ls\r\n"
                     L"  序列号 = %ls\r\n",
                     d->letter, pdStr,
                     d->sectorsPerCluster, d->bytesPerSector,
                     d->clusterBytes, cs, d->fsName[0] ? d->fsName : L"-",
                     d->volLabel[0] ? d->volLabel : L"-",
                     d->serial[0] ? d->serial : L"-");
        } else {
            swprintf(detail, _countof(detail),
                     L"\r\n[选中详情] %ls  (NT 设备名：%ls)  （不可访问——可能是空光驱/未就绪介质/无权限）\r\n"
                     L"  序列号 = %ls\r\n",
                     d->letter, pdStr,
                     d->serial[0] ? d->serial : L"-");
        }
        StringCchCatW(out, nChars, detail);
    }
    (void)used;
}

// ============================================================
// 标签页绘制
// ============================================================

// 顶部页签条：选中项主色文字 + 底部 3px 主色指示条
static void DrawTabs(HDC hdc) {
    int sp = Scale(SHADOW_PAD);
    // 页签条底部分隔线
    HPEN line = CreatePen(PS_SOLID, 1, C_TXT_LIGHT_DIM);
    HPEN ol = (HPEN)SelectObject(hdc, line);
    MoveToEx(hdc, sp, Scale(TITLE_H + TAB_H + SHADOW_PAD), NULL);
    LineTo(hdc, Scale(WIN_W + SHADOW_PAD), Scale(TITLE_H + TAB_H + SHADOW_PAD));
    SelectObject(hdc, ol);
    DeleteObject(line);

    for (int t = 0; t < TAB_COUNT; t++) {
        BOOL active = (t == g_curTab);
        HFONT f = MakeFont(Scale(14), active ? FW_SEMIBOLD : FW_NORMAL);
        HFONT of = (HFONT)SelectObject(hdc, f);
        SetTextColor(hdc, active ? C_TXT_LIGHT : (g_tabHover[t] ? C_TXT_LIGHT : C_TXT_LIGHT_DIM));
        SetBkMode(hdc, TRANSPARENT);
        DrawTextW(hdc, g_tabNames[t], -1, &g_tabRects[t],
                  DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        SelectObject(hdc, of);
        DeleteObject(f);
        if (active) {
            RECT ind = {g_tabRects[t].left  + Scale(18), g_tabRects[t].bottom - Scale(3),
                        g_tabRects[t].right - Scale(18), g_tabRects[t].bottom};
            HBRUSH b = CreateSolidBrush(C_TXT_LIGHT);
            FillRect(hdc, &ind, b);
            DeleteObject(b);
        }
    }
}

// 磁盘信息页：说明 + 自绘 7 列表格 + 选中行详情 + 刷新按钮
// sp：本帧统一使用的阴影留白像素（由 WM_PAINT 传入，与 ComputeLayout 一致）
static void DrawDiskPage(HDC hdc, int sp) {
    const int colW[7] = {50, 64, 80, 62, 80, 100, 100};   // v1.0.11 加"磁盘"列；合计 536。"磁盘"列 PD0+1≈40px 宽裕 64 够，"文件系统"4 字 80 不再截断

    // ---- 顶部说明 + 分区数 ----
    {
        int y = Scale(TITLE_H + TAB_H + 14 + SHADOW_PAD);
        HFONT hf = MakeFont(Scale(15), FW_SEMIBOLD);
        HFONT of = (HFONT)SelectObject(hdc, hf);
        SetTextColor(hdc, RGB(245, 246, 255));
        SetBkMode(hdc, TRANSPARENT);
        RECT r = {Scale(MARGIN + SHADOW_PAD), y,
                  g_copyBtnRect.left - Scale(12), y + Scale(24)};
        DrawTextW(hdc, L"\u67e5\u770b\u5404\u5206\u533a\u7684\u7c07\u5927\u5c0f\u4e0e\u5bb9\u91cf\u4fe1\u606f",
                  -1, &r, DT_LEFT | DT_SINGLELINE | DT_VCENTER);
        SelectObject(hdc, of);
        DeleteObject(hf);

        WCHAR cnt[32];
        swprintf(cnt, _countof(cnt), L"\u5171 %d \u4e2a\u5206\u533a", g_diskCount);
        HFONT cf = MakeFont(Scale(13), FW_NORMAL);
        HFONT ocf = (HFONT)SelectObject(hdc, cf);
        SetTextColor(hdc, RGB(205, 210, 230));
        SetBkMode(hdc, TRANSPARENT);
        RECT cr = {Scale(MARGIN + SHADOW_PAD), y,
                   g_copyBtnRect.left - Scale(12), y + Scale(24)};
        DrawTextW(hdc, cnt, -1, &cr, DT_RIGHT | DT_SINGLELINE | DT_VCENTER);
        SelectObject(hdc, ocf);
        DeleteObject(cf);
    }

    // 刷新按钮（右上角）
    DrawButton(hdc, &g_diskRefreshRect, L"\u5237\u65b0", g_hoverDiskRefresh, FALSE);
    // 一键复制按钮（紧邻刷新按钮左侧）
    DrawCopyButton(hdc, &g_copyBtnRect, g_copyBtnHover,
                   L"\u4e00\u952e\u590d\u5236", L"\u5df2\u590d\u5236");   // 一键复制 / 已复制

    if (g_diskCount == 0) {
        HFONT f = MakeFont(Scale(14), FW_NORMAL);
        HFONT of = (HFONT)SelectObject(hdc, f);
        SetTextColor(hdc, RGB(205, 210, 230));
        SetBkMode(hdc, TRANSPARENT);
        RECT r = {Scale(MARGIN + SHADOW_PAD),
                  Scale(TITLE_H + TAB_H + DISK_PAGE_HEAD + SHADOW_PAD),
                  Scale(WIN_W - MARGIN + SHADOW_PAD),
                  Scale(TITLE_H + TAB_H + DISK_PAGE_HEAD + 40 + SHADOW_PAD)};
        DrawTextW(hdc, L"\u6682\u65e0\u53ef\u8bbf\u95ee\u7684\u78c1\u76d8\u5206\u533a", -1, &r,
                  DT_CENTER | DT_SINGLELINE | DT_VCENTER);
        SelectObject(hdc, of);
        DeleteObject(f);
        return;
    }

    int rows = (g_diskCount < DISK_MAX_ROWS) ? g_diskCount : DISK_MAX_ROWS;

    // ---- 表头 ----
    {
        int y = Scale(TITLE_H + TAB_H + DISK_PAGE_HEAD) + sp;   // 与首行同源，避免高 DPI 错位
        // 表头直接画在白磨砂面板上，深色文字保持可读性
        HFONT f = MakeFont(Scale(13), FW_SEMIBOLD);
        HFONT of = (HFONT)SelectObject(hdc, f);
        SetTextColor(hdc, RGB(205, 210, 230));
        SetBkMode(hdc, TRANSPARENT);
        const WCHAR* heads[7] = {
            L"\u76d8\u7b26", L"\u78c1\u76d8", L"\u6587\u4ef6\u7cfb\u7edf",
            L"\u7c07\u5927\u5c0f", L"\u7c7b\u578b", L"\u603b\u5bb9\u91cf", L"\u53ef\u7528"
        };
        int bx = MARGIN;                       // 基准 X，逐列累加后统一 Scale（避免舍入漂移）
        for (int c = 0; c < 7; c++) {
            int x0 = Scale(bx + SHADOW_PAD);   // 与行框左缘同源
            RECT cr = {x0 + Scale(8), y, Scale(bx + colW[c] + SHADOW_PAD), y + Scale(DISK_TABLE_HEAD)};
            DrawTextW(hdc, heads[c], -1, &cr, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
            bx += colW[c];
        }
        SelectObject(hdc, of);
        DeleteObject(f);
    }

    // ---- 数据行 ----
    for (int i = 0; i < rows; i++) {
        const DiskInfo* d = &g_disks[i];
        BOOL sel = (i == g_selDisk);
        BOOL hov = (i == g_hoverDiskRow);
        const RECT* rr = &g_diskRowRect[i];

        if (sel || hov) {
            HBRUSH b = CreateSolidBrush(sel ? C_GREEN_BG : C_BTN_SECONDARY_H);
            FillRect(hdc, rr, b);
            DeleteObject(b);
        }
        HPEN lp = CreatePen(PS_SOLID, 1, C_DIVIDER);
        HPEN olp = (HPEN)SelectObject(hdc, lp);
        MoveToEx(hdc, rr->left, rr->bottom, NULL);
        LineTo(hdc, rr->right, rr->bottom);
        SelectObject(hdc, olp);
        DeleteObject(lp);

        HFONT f = MakeFont(Scale(13), sel ? FW_SEMIBOLD : FW_NORMAL);
        HFONT of = (HFONT)SelectObject(hdc, f);
        // 选中/悬停行有浅绿/浅灰底，白字看不清 -> 强制用深字
        SetTextColor(hdc, (sel || hov) ? C_TEXT : (d->ok ? RGB(238, 242, 252) : RGB(200, 208, 230)));
        SetBkMode(hdc, TRANSPARENT);

        WCHAR buf[48];
        int bx = MARGIN;
        for (int c = 0; c < 7; c++) {
            int x0 = Scale(bx + SHADOW_PAD);
            RECT cr = {x0 + Scale(8), rr->top, Scale(bx + colW[c] + SHADOW_PAD), rr->bottom};
            switch (c) {
                case 0: DrawTextW(hdc, d->letter, -1, &cr, DT_LEFT|DT_VCENTER|DT_SINGLELINE); break;
                case 1:
                    // 磁盘号：v1.0.11 加；跨盘卷显示 PhysicalDrive0+1
                    if (d->diskIndex >= 0)
                        swprintf(buf, _countof(buf), L"PD%d%s", d->diskIndex, d->diskExtents);
                    else
                        wcscpy(buf, L"-");
                    DrawTextW(hdc, buf, -1, &cr, DT_LEFT|DT_VCENTER|DT_SINGLELINE); break;
                case 2:
                    if (d->unpartitioned)
                        DrawTextW(hdc, d->model[0] ? d->model : L"-", -1, &cr,
                                  DT_LEFT|DT_VCENTER|DT_SINGLELINE);
                    else
                        DrawTextW(hdc, d->fsName[0] ? d->fsName : L"-", -1, &cr,
                                  DT_LEFT|DT_VCENTER|DT_SINGLELINE);
                    break;
                case 3:
                    if (d->unpartitioned)            swprintf(buf, _countof(buf), L"-");
                    else if (d->ok)                 FormatClusterStr(d->clusterBytes, buf, _countof(buf));
                    else                             swprintf(buf, _countof(buf), L"-");
                    DrawTextW(hdc, buf, -1, &cr, DT_LEFT|DT_VCENTER|DT_SINGLELINE); break;
                case 4: DrawTextW(hdc, d->typeName, -1, &cr, DT_LEFT|DT_VCENTER|DT_SINGLELINE); break;
                case 5:
                    if (d->ok) FormatBytesStr(d->totalBytes, buf, _countof(buf));
                    else       swprintf(buf, _countof(buf), L"-");
                    DrawTextW(hdc, buf, -1, &cr, DT_LEFT|DT_VCENTER|DT_SINGLELINE); break;
                default:
                    if (d->unpartitioned || !d->ok) swprintf(buf, _countof(buf), L"-");
                    else                             FormatBytesStr(d->freeBytes, buf, _countof(buf));
                    DrawTextW(hdc, buf, -1, &cr, DT_LEFT|DT_VCENTER|DT_SINGLELINE); break;
            }
            bx += colW[c];
        }
        SelectObject(hdc, of);
        DeleteObject(f);
    }

    // ---- 选中分区详情 ----
    if (g_selDisk >= 0 && g_selDisk < g_diskCount) {
        const DiskInfo* d = &g_disks[g_selDisk];
        int dy = Scale(TITLE_H + TAB_H + DISK_PAGE_HEAD + DISK_TABLE_HEAD + rows * DISK_ROW_H + 12);
        RECT dr = {Scale(MARGIN + SHADOW_PAD), dy + sp,
                   Scale(WIN_W - MARGIN + SHADOW_PAD), dy + sp + Scale(DISK_DETAIL_H - 16)};
        if (g_pg) {
            GraphicsPath ddp; RoundRectPath(&ddp, (float)dr.left, (float)dr.top, (float)(dr.right - dr.left), (float)(dr.bottom - dr.top), (float)Scale(8));
            SolidBrush dbr(Color(55, 255, 255, 255));
            g_pg->FillPath(&dbr, &ddp);
            Pen dpen(Color(120, 255, 255, 255), 1.0f);
            GraphicsPath ddp2; RoundRectPath(&ddp2, (float)dr.left, (float)dr.top, (float)(dr.right - dr.left), (float)(dr.bottom - dr.top), (float)Scale(8));
            g_pg->DrawPath(&dpen, &ddp2);
            g_pg->Flush(FlushIntentionSync);
        } else {
            DrawRoundedRect(hdc, &dr, Scale(8), C_CARD_BG, C_BORDER);
        }

        WCHAR line1[128], line2[128], line3[128], line4[128], line5[128], line6[128];
        line3[0] = 0; line4[0] = 0; line5[0] = 0; line6[0] = 0;
        if (d->unpartitioned) {
            WCHAR phys[32];
            FormatBytesStr(d->physSize, phys, _countof(phys));
            swprintf(line1, _countof(line1), L"%ls   %ls", d->letter, d->typeName);
            swprintf(line2, _countof(line2), L"\u578b\u53f7 %ls   \u603b\u7ebf %ls",
                     d->model[0] ? d->model : L"-", d->busType[0] ? d->busType : L"-");
            swprintf(line3, _countof(line3),
                     L"\u603b\u5bb9\u91cf %ls  \uff08\u672a\u5206\u533a/\u672a\u521d\u59cb\u5316\uff0c\u65e0\u53ef\u8bbf\u95ee\u5377\uff09", phys);
        } else if (d->ok) {
            WCHAR cs[32];
            FormatClusterStr(d->clusterBytes, cs, _countof(cs));
            swprintf(line1, _countof(line1), L"%ls   %ls   %ls",
                     d->letter, d->typeName, d->fsName[0] ? d->fsName : L"-");
            swprintf(line2, _countof(line2),
                L"\u7c07\u5927\u5c0f %ls   =   \u6bcf\u7c07 %lu \u6247\u533a  \u00d7  %lu \u5b57\u8282/\u6247\u533a",
                cs, d->sectorsPerCluster, d->bytesPerSector);
            WCHAR tb[32], fb[32];
            FormatBytesStr(d->totalBytes, tb, _countof(tb));
            FormatBytesStr(d->freeBytes,  fb, _countof(fb));
            double usedPct = (d->totalBytes > 0)
                ? (double)(d->totalBytes - d->freeBytes) * 100.0 / (double)d->totalBytes : 0.0;
            swprintf(line3, _countof(line3),
                L"\u603b\u5bb9\u91cf %ls   \u00b7   \u53ef\u7528 %ls   \u00b7   \u5df2\u7528 %.0f%%",
                tb, fb, usedPct);
        } else {
            swprintf(line1, _countof(line1), L"%ls   %ls", d->letter, d->typeName);
            swprintf(line2, _countof(line2), L"\u65e0\u6cd5\u8bbf\u95ee");   // 无法访问
        }
        // v1.0.12：选中详情框加 NT 设备名（与 HTML 报告分区表段保持一致）
        if (d->diskIndex >= 0)
            swprintf(line4, _countof(line4), L"NT \u8bbe\u5907\u540d  %ls%d%s",
                     L"\\\\.\\PhysicalDrive", d->diskIndex, d->diskExtents);
        else
            wcscpy(line4, L"NT \u8bbe\u5907\u540d  -");

        // v1.3.4：序列号（有则显示第 5 行，无则留空、不占位）
        if (d->serial[0])
            swprintf(line5, _countof(line5), L"\u5e8f\u5217\u53f7 %ls", d->serial);

        // v1.3.5：卷标（仅可访问分区有，未分区/无法访问不显示）
        if (d->ok && d->volLabel[0])
            swprintf(line6, _countof(line6), L"\u5377\u6807 %ls", d->volLabel);

        SetBkMode(hdc, TRANSPARENT);
        const WCHAR* lines[6] = {line1, line2, line3, line4, line5, line6};
        int nLines = 1;
        for (int k = 5; k >= 0; k--) if (lines[k][0]) { nLines = k + 1; break; }
        for (int k = 0; k < nLines; k++) {
            HFONT lf = MakeFont(k == 0 ? Scale(14) : Scale(13), k == 0 ? FW_SEMIBOLD : FW_NORMAL);
            HFONT of = (HFONT)SelectObject(hdc, lf);
            SetTextColor(hdc, k == 0 ? RGB(245, 246, 255) : RGB(220, 228, 245));
            RECT lr = {dr.left + Scale(14), dr.top + Scale(10) + k * Scale(20),
                       dr.right - Scale(14), dr.top + Scale(30) + k * Scale(20)};
            DrawTextW(hdc, lines[k], -1, &lr, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
            SelectObject(hdc, of);
            DeleteObject(lf);
        }
    }
}

// ============================================================
// 硬件配置页：自绘"标签：值"两列布局，所有行都可读、一键复制带走全文
// ============================================================

// 在 RECT 内绘制左对齐文字（用于标签/值），返回绘制后 y 增量（= rc.top + HW_ROW_H）
// ============================================================
// 硬件页绘制计划（v1.0.9）
//
// 老毛病：窗口高度在 ComputeLayout 里算、内容在 DrawHardwarePage 里画，
// 两边各数一次段数与行数，只要改一处就会脱节（底部留白 / 内容被截）。
// 这里改为"先排计划、再按计划画"：ComputeLayout 生成 g_hwPlan，
// DrawHardwarePage 只遍历计划，二者不可能对不上。
//
// 另一个作用：当窗口在 HiDPI 下放不下屏幕时（1080p + 150% 缩放会把
// 窗口撑到 1400+ px），先压缩行距，再按优先级裁掉次要段
// （USB / 音频 / 显示器 / 网卡）——这些在导出的 HTML 报告里都有。
// ============================================================
enum HwSectionId {
    HWSEC_OS = 0, HWSEC_MACHINE, HWSEC_BOARD, HWSEC_CPU, HWSEC_MEM,
    HWSEC_GPU, HWSEC_DISK, HWSEC_DISP, HWSEC_NIC, HWSEC_AUDIO, HWSEC_USB,
    HWSEC_COUNT
};
struct HwSecPlan { int id; int rows; };
static HwSecPlan g_hwPlan[HWSEC_COUNT];
static int       g_hwPlanCount = 0;
static int       g_hwRowH   = HW_ROW_H;        // 96-DPI 基准行高（放不下时自动压缩）

// v1.2.2：显示器型号行已含厂商前缀时（如 "AOC 2769"），单独厂商行冗余，隐藏之。
// 判定：vendor 非空、name 非空、且 name 以 vendor 开头（大小写无关）。
static BOOL DispVendorRedundant(const HwDisplay* d) {
    if (!d || !d->vendor[0] || !d->name[0]) return FALSE;
    size_t vlen = wcslen(d->vendor);
    if (vlen == 0 || wcslen(d->name) < vlen) return FALSE;
    return (_wcsnicmp(d->name, d->vendor, vlen) == 0) ? TRUE : FALSE;
}
// v1.2.3：某块 GPU 是否拿到 NVML 遥测（任一遥测字段非 0 即视为有数据）
static BOOL GpuHasTelemetry(const HwGpu* g) {
    return (g && (g->tempC > 0 || g->fanPct > 0 || g->powerW > 0 || g->utilPct > 0 || g->clockMHz > 0));
}
static int       g_hwSecGap = HW_SECTION_GAP;  // 96-DPI 基准段间距

// 每段画几行；返回 0 表示该段不显示（不占高度）
static int HwSecRows(int id) {
    switch (id) {
    case HWSEC_OS:
    case HWSEC_MACHINE:
        return 1;
    case HWSEC_GPU: {
        // v1.2.7：每颗显卡"型号 + （有遥测时）状态"行；多显卡时每颗独立成块（标签 显卡#i）
        if (g_hw.gpuCount == 0) return 1;
        int n = (g_hw.gpuCount < HW_MAX_GPU) ? g_hw.gpuCount : HW_MAX_GPU;
        int rows = 0;
        for (int i = 0; i < n; i++)
            rows += 1 + (GpuHasTelemetry(&g_hw.gpus[i]) ? 1 : 0);
        return rows;
    }
    case HWSEC_BOARD:
        return 2 + (g_hw.boardChipset[0] ? 1 : 0);   // 型号 + BIOS + (芯片组行，若识别到)
    case HWSEC_CPU:
        // v1.1.10：每颗 CPU 画"型号 + 规格"两行；多颗时每颗独立成块（标签 CPU#i），行数 = 颗数*2
        // v1.2.5：曾识别到微架构代号（libcpuid）时加 1 行"代号"；
        // v1.2.7：主页不再显示代号（代号仍保留在 HTML 报告与一键复制文本里）；
        //         cpuCount==0 时与绘制端对齐只计 1 行
        if (g_hw.cpuCount == 0) return 1;
        { int n = (g_hw.cpuCount < HW_MAX_CPU) ? g_hw.cpuCount : HW_MAX_CPU; return n * 2; }
    case HWSEC_MEM:
        // v1.0.12：展开已插内存条（v1.0.9 时只有 1 行汇总）—— 1 汇总 + N 已插 + 1 空槽行（若有）
        if (g_hw.memCount == 0) return 1;
        return 1 + g_hw.memPopulated + ((g_hw.memCount - g_hw.memPopulated) > 0 ? 1 : 0);
    case HWSEC_DISK:
        // 全部列出（最多 HW_MAX_DISK=8 块）；空间不足时由 ComputeLayout 整段裁掉，
        // 不做"还有 N 块未列出"的折叠 —— 硬盘型号是维护时最常看的信息。
        // v1.2.8：diskCount 封顶 HW_MAX_DISK（与 CPU/GPU 一致），防采集端异常越界
        if (g_hw.diskCount == 0) return 1;
        { int n = (g_hw.diskCount < HW_MAX_DISK) ? g_hw.diskCount : HW_MAX_DISK; return n; }
    case HWSEC_DISP:
        // v1.0.12.1：每显示器 2 行（分辨率 + 型号），型号为空时退化为 1 行；
        // v1.1.7：厂商识别到时额外加 1 行（厂商）；
        // v1.2.2：型号行已含厂商前缀时（如"AOC 2769"）不再单列厂商行，避免冗余
        if (g_hw.displayCount == 0) return 1;
        {
            int rows = 0;
            for (int i = 0; i < g_hw.displayCount && i < HW_MAX_DISPLAY; i++) {
                rows++;  // 分辨率行
                if (g_hw.displays[i].name[0]) rows++;     // 型号行（仅当 name 非空）
                if (g_hw.displays[i].vendor[0] && !DispVendorRedundant(&g_hw.displays[i]))
                    rows++;   // 厂商行（仅当厂商识别到且型号行未含该厂商前缀）
            }
            return rows;
        }
    case HWSEC_NIC:
        // v1.2.1：展示前 2 块真实网卡；超过 2 块再加一行"总计"
        if (g_hw.nicCount == 0) return 1;
        if (g_hw.nicCount == 1) return 1;
        if (g_hw.nicCount == 2) return 2;
        return 3;                                      // 网卡1 + 网卡2 + 总计
    case HWSEC_AUDIO:
        // v1.1.5：总计 + 默认播放 + 默认录音 共 3 行
        return (!g_hw.isServer && g_hw.audioCount > 0) ? 3 : 0;
    case HWSEC_USB:
        return (!g_hw.isServer && g_hw.usbCount > 0) ? 2 : 0;
    default:
        return 0;
    }
}

// 屏幕可用高度（工作区 - 任务栏，再留 40px 余量给窗口投影与手感）
static int AvailableScreenH() {
    RECT wa = {0};
    if (SystemParametersInfoW(SPI_GETWORKAREA, 0, &wa, 0) && (wa.bottom - wa.top) > 200)
        return (wa.bottom - wa.top) - 40;
    return GetSystemMetrics(SM_CYSCREEN) - 80;
}

static int DrawHwRow(HDC hdc, int xLeft, int xRight, int y, const WCHAR* label,
                     const WCHAR* value, BOOL labelBold) {
    int rowH = Scale(g_hwRowH);
    HFONT fl = MakeFont(Scale(13), labelBold ? FW_SEMIBOLD : FW_NORMAL);
    HFONT fv = MakeFont(Scale(13), FW_NORMAL);
    SetBkMode(hdc, TRANSPARENT);

    HFONT ofl = (HFONT)SelectObject(hdc, fl);
    SetTextColor(hdc, RGB(200, 208, 230));
    RECT lr = {xLeft, y, xLeft + Scale(96), y + rowH};
    DrawTextW(hdc, label, -1, &lr, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    SelectObject(hdc, ofl);

    HFONT ofv = (HFONT)SelectObject(hdc, fv);
    SetTextColor(hdc, RGB(238, 242, 252));
    RECT vr = {xLeft + Scale(96), y, xRight, y + rowH};
    DrawTextW(hdc, value, -1, &vr, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    SelectObject(hdc, ofv);

    DeleteObject(fl);
    DeleteObject(fv);
    return y + rowH;
}

// 段标题小图标（每段一个语义色，比纯色条更表意；Z.W. 反馈加色 + 重画网卡/USB）
// 颜色：按 sectionId 选 Tailwind 系亮色（避开当前薰衣草面板 bg，避免被吃掉）
static COLORREF HwIconColor(int sectionId) {
    switch (sectionId) {
        case HWSEC_OS:      return RGB(96, 165, 250);  // 蓝   OS/系统
        case HWSEC_MACHINE: return RGB(56, 189, 248);  // 天蓝 计算机
        case HWSEC_BOARD:   return RGB(74, 222, 128);  // 绿   主板/PCB
        case HWSEC_CPU:     return RGB(248, 113, 113); // 红   CPU/算力
        case HWSEC_MEM:     return RGB(192, 132, 252); // 紫   内存
        case HWSEC_GPU:     return RGB(251, 146, 60);  // 橙   显卡
        case HWSEC_DISK:    return RGB(148, 163, 184); // 灰   硬盘
        case HWSEC_DISP:    return RGB(129, 140, 248); // 靛   显示器
        case HWSEC_NIC:     return RGB(45, 212, 191);  // 青绿 网卡
        case HWSEC_AUDIO:   return RGB(244, 114, 182); // 粉   音频
        case HWSEC_USB:     return RGB(251, 191, 36);  // 琥珀 USB
        default:            return RGB(180, 200, 255);
    }
}

static void DrawHwSectionIcon(HDC hdc, int xLeft, int y, int rowH, int sectionId) {
    int iconSz = Scale(14);
    int ix = xLeft + Scale(6);   // 间距加宽 4→6，统一图标起点
    int iy = y + (rowH - iconSz) / 2;
    RECT ir = {ix, iy, ix + iconSz, iy + iconSz};
    HPEN pen = CreatePen(PS_SOLID, Scale(1), HwIconColor(sectionId));
    HPEN oldPen = (HPEN)SelectObject(hdc, pen);
    HBRUSH br = (HBRUSH)GetStockObject(NULL_BRUSH);
    HBRUSH oldBr = (HBRUSH)SelectObject(hdc, br);
    int l = ir.left, t = ir.top, r = ir.right, b = ir.bottom;
    int cx = (l + r) / 2, cy = (t + b) / 2;
    switch (sectionId) {
        case HWSEC_OS: // 窗口（OS）
            Rectangle(hdc, l, t + Scale(3), r, b);
            MoveToEx(hdc, l, t + Scale(6), NULL); LineTo(hdc, r, t + Scale(6));
            break;
        case HWSEC_MACHINE: // 显示器+底座（计算机）
            Rectangle(hdc, l, t, r, b - Scale(4));
            MoveToEx(hdc, cx, b - Scale(4), NULL); LineTo(hdc, cx, b);
            MoveToEx(hdc, cx - Scale(3), b, NULL); LineTo(hdc, cx + Scale(3), b);
            break;
        case HWSEC_BOARD: // 芯片+四周引脚（主板）
            Rectangle(hdc, l + Scale(3), t + Scale(3), r - Scale(3), b - Scale(3));
            for (int i = 0; i < 3; i++) {
                int py = t + Scale(3) + i * Scale(3);
                MoveToEx(hdc, l, py, NULL); LineTo(hdc, l + Scale(3), py);
                MoveToEx(hdc, r - Scale(3), py, NULL); LineTo(hdc, r, py);
            }
            break;
        case HWSEC_CPU: // CPU（处理器）
            Rectangle(hdc, l + Scale(1), t + Scale(1), r - Scale(1), b - Scale(1));
            Rectangle(hdc, l + Scale(4), t + Scale(4), r - Scale(4), b - Scale(4));
            break;
        case HWSEC_MEM: // 内存条（RAM）
            Rectangle(hdc, l + Scale(4), t, r - Scale(4), b);
            for (int i = 0; i < 3; i++) {
                int py = t + Scale(3) + i * Scale(3);
                MoveToEx(hdc, l + Scale(4), py, NULL); LineTo(hdc, r - Scale(4), py);
            }
            break;
        case HWSEC_GPU: // 显卡+风扇
            Rectangle(hdc, l, t + Scale(3), r, b - Scale(1));
            Ellipse(hdc, cx - Scale(3), cy - Scale(2), cx + Scale(3), cy + Scale(3));
            break;
        case HWSEC_DISK: // 硬盘
            Rectangle(hdc, l, t + Scale(2), r, b);
            Ellipse(hdc, cx - Scale(2), cy - Scale(1), cx + Scale(2), cy + Scale(2));
            break;
        case HWSEC_DISP: // 显示器（display）
            Rectangle(hdc, l, t, r, b - Scale(4));
            MoveToEx(hdc, cx, b - Scale(4), NULL); LineTo(hdc, cx, b);
            MoveToEx(hdc, cx - Scale(3), b, NULL); LineTo(hdc, cx + Scale(3), b);
            break;
        case HWSEC_NIC: // 网卡（RJ45 接口 + 内部触点，更易识别）
            // RJ45 主体
            Rectangle(hdc, l + Scale(1), t + Scale(2), r - Scale(1), b - Scale(3));
            // 4 根金触点（横线表示 8P8C）
            for (int i = 0; i < 4; i++) {
                int py = t + Scale(4) + i * Scale(2);
                MoveToEx(hdc, l + Scale(3), py, NULL); LineTo(hdc, r - Scale(3), py);
            }
            // 卡边凸起（PCI 板边）
            MoveToEx(hdc, l + Scale(2), b - Scale(2), NULL); LineTo(hdc, r - Scale(2), b - Scale(2));
            break;
        case HWSEC_AUDIO: // 扬声器
            MoveToEx(hdc, l, cy - Scale(2), NULL); LineTo(hdc, l + Scale(3), cy - Scale(2));
            LineTo(hdc, l + Scale(6), t);
            LineTo(hdc, l + Scale(6), b);
            LineTo(hdc, l + Scale(3), cy + Scale(2));
            LineTo(hdc, l, cy + Scale(2));
            LineTo(hdc, l, cy - Scale(2));
            break;
        case HWSEC_USB: // USB 经典三叉（实心点 + 主干 + 两分支 + 方头 + 实心箭尾）
            {
                // 实心填充笔刷（让顶部点和箭尾实心）
                HBRUSH fb = CreateSolidBrush(HwIconColor(sectionId));
                HBRUSH oldFb = (HBRUSH)SelectObject(hdc, fb);
                // 顶圆点（实心）
                Ellipse(hdc, cx - Scale(2), t, cx + Scale(2), t + Scale(4));
                // 主干
                MoveToEx(hdc, cx, t + Scale(3), NULL); LineTo(hdc, cx, cy - Scale(2));
                // 分叉点
                MoveToEx(hdc, cx, cy, NULL); LineTo(hdc, l + Scale(2), cy + Scale(2));
                MoveToEx(hdc, cx, cy, NULL); LineTo(hdc, r - Scale(2), cy + Scale(2));
                // 左方头
                Rectangle(hdc, l + Scale(1), cy + Scale(2), l + Scale(5), b - Scale(2));
                // 右三角箭头（用 LineTo 画三角并填充）
                MoveToEx(hdc, r - Scale(4), b - Scale(4), NULL);
                LineTo(hdc, r - Scale(1), cy + Scale(2));
                LineTo(hdc, r - Scale(7), cy + Scale(2));
                LineTo(hdc, r - Scale(4), b - Scale(4));
                SelectObject(hdc, oldFb);
                DeleteObject(fb);
            }
            break;
    }
    SelectObject(hdc, oldPen);
    SelectObject(hdc, oldBr);
    DeleteObject(pen);
}

// 段标题（玻璃风磨砂带 + 左侧语义图标，替代原纯色竖条）
static int DrawHwSectionTitle(HDC hdc, int xLeft, int xRight, int y, const WCHAR* title, int sectionId) {
    int rowH = Scale(g_hwRowH);
    RECT tr = {xLeft, y, xRight, y + rowH};
    if (g_pg) {
        GraphicsPath sbp; RoundRectPath(&sbp, (float)xLeft, (float)y, (float)(xRight - xLeft), (float)rowH, (float)Scale(6));
        SolidBrush sb(Color(12, 255, 255, 255));
        g_pg->FillPath(&sb, &sbp);
        g_pg->Flush(FlushIntentionSync);
    } else {
        HBRUSH bg = CreateSolidBrush(C_BG);
        FillRect(hdc, &tr, bg);
        DeleteObject(bg);
    }
    // 左侧语义图标（替代原色条）
    DrawHwSectionIcon(hdc, xLeft, y, rowH, sectionId);
    SetBkMode(hdc, TRANSPARENT);
    HFONT ft = MakeFont(Scale(14), FW_SEMIBOLD);
    HFONT of = (HFONT)SelectObject(hdc, ft);
    SetTextColor(hdc, RGB(245, 246, 255));
    RECT r2 = {xLeft + Scale(26), y, xRight, y + rowH};
    DrawTextW(hdc, title, -1, &r2, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    SelectObject(hdc, of);
    DeleteObject(ft);
    return y + rowH + Scale(g_hwSecGap);
}

// ============================================================
// HTML 报告（v1.0.9）
//
// 主页面只放"常规信息"（一眼看完配置），全部明细（每根内存 / 每张网卡 /
// 每个 USB 外设 / 分区表 ...）都落在这份 HTML 里，由"导出报告"按钮写到桌面。
// 生成逻辑放在这里（而非 hardware.cpp）是因为报告还要汇总磁盘分区与
// 右键功能启用状态，这两份数据只有主程序持有。
// ============================================================
static void HtmlEscape(const WCHAR* s, WCHAR* out, size_t n) {
    // & < > " 四个字符必须转义，否则含 '&' 的设备名会破坏 HTML 结构。
    // out 必须先置空：下面用 StringCchCatW 追加，未终止的缓冲会拼接出垃圾（甚至泄漏栈内容）。
    if (!n) return;
    out[0] = 0;
    size_t o = 0;
    for (size_t i = 0; s[i] && o + 8 < n; i++) {
        const WCHAR* rep = NULL;
        switch (s[i]) {
        case L'&':  rep = L"&amp;";  break;
        case L'<':  rep = L"&lt;";   break;
        case L'>':  rep = L"&gt;";   break;
        case L'"':  rep = L"&quot;"; break;
        default: break;
        }
        if (rep) { StringCchCatW(out, n, rep); o += wcslen(rep); }
        else     { out[o++] = s[i]; out[o] = 0; }
    }
    out[n - 1] = 0;
}

// 报告用容量串（与界面一致，走 FormatBytesStr）
static void HtmlSize(ULONGLONG b, WCHAR* out, size_t n) {
    if (!b) { StringCchCopyW(out, n, L"\u672a\u77e5"); return; }   // 未知
    FormatBytesStr(b, out, (int)n);
}

// v1.1.8：供 selftest 注入确定性值，以在任意机器（含无真实硬件的 VM）上
// 验证新字段（芯片组/显存/内存类型/显示器尺寸）的 HTML 渲染分支。正常流程不使用。
HardwareInfo* SelfTest_PeekHw(void) { return &g_hw; }

// 生成完整 HTML 报告。out 需足够大（建议 >= 32768 字符）。
// 非 static：selftest 需要直接调用验证（链接期可见，见 selftest.cpp 的 extern 声明）。
BOOL Report_BuildHtml(WCHAR* out, size_t nChars) {
    if (!out || !nChars) return FALSE;
    out[0] = 0;
    // tmp 必须放得下最长的一次写入（内联 CSS 那段就有 800+ 字符，
    // 512 会被截断 -> </style> 丢失 -> 整页被浏览器当样式表吞掉，页面空白）
    WCHAR tmp[4096], esc[512];

    #define W(fmt, ...) do { \
        StringCchPrintfW(tmp, _countof(tmp), fmt, ##__VA_ARGS__); \
        StringCchCatW(out, nChars, tmp); } while (0)
    // 转义后写入（设备名/型号可能含 & < >）
    #define WE(s) do { HtmlEscape((s), esc, _countof(esc)); StringCchCatW(out, nChars, esc); } while (0)

    // ---- 页头 ----
    W(L"<!DOCTYPE html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"UTF-8\">\n");
    W(L"<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    W(L"<title>WinSuiteX \u786c\u4ef6\u62a5\u544a - %ls</title>\n",   // 硬件报告
      g_hw.computerName[0] ? g_hw.computerName : L"PC");
    W(L"<style>\n"
      L"*{box-sizing:border-box}\n"
      L"body{margin:0;padding:24px;background:#f5f5f5;color:#1f1f1f;\n"
      L"font-family:\"Microsoft YaHei\",\"Segoe UI\",system-ui,sans-serif;line-height:1.6}\n"
      L".wrap{max-width:980px;margin:0 auto}\n"
      L"h1{font-size:22px;margin:0 0 4px}\n"
      L".sub{color:#595959;font-size:13px;margin-bottom:22px}\n"
      L"section{background:#fff;border:1px solid #d9d9d9;border-radius:8px;\n"
      L"padding:16px 18px;margin-bottom:16px}\n"
      L"h2{font-size:15px;margin:0 0 12px;padding-left:10px;border-left:3px solid #0958d9}\n"
      L"table{width:100%%;border-collapse:collapse;font-size:13px}\n"
      L"td,th{padding:6px 8px;border-bottom:1px solid #f0f0f0;text-align:left;vertical-align:top}\n"
      L"th{width:150px;color:#595959;font-weight:400;white-space:nowrap}\n"
      L"tr:last-child td,tr:last-child th{border-bottom:none}\n"
      L".mono{font-family:Consolas,Menlo,monospace}\n"
      L".badge{display:inline-block;padding:1px 8px;border-radius:10px;font-size:12px}\n"
      L".on{background:#dff2e6;color:#15803d}\n"
      L".off{background:#f0f0f0;color:#595959}\n"
      L".empty{color:#8c8c8c;font-size:13px}\n"
      L".foot{color:#9aa1ad;font-size:12px;text-align:center;margin-top:18px}\n"
      L"</style>\n</head>\n<body>\n<div class=\"wrap\">\n");

    W(L"<h1>WinSuiteX \u8ba1\u7b97\u673a\u786c\u4ef6\u4e0e\u7cfb\u7edf\u62a5\u544a</h1>\n");  // 计算机硬件与系统报告
    {
        WCHAR ver[64] = {0};
        LoadStringW(g_hInst, IDS_VERSION, ver, _countof(ver));
        SYSTEMTIME st; GetLocalTime(&st);
        W(L"<div class=\"sub\">%ls &nbsp;\u00b7&nbsp; v%ls &nbsp;\u00b7&nbsp; %04d-%02d-%02d %02d:%02d</div>\n",
          g_hw.computerName[0] ? g_hw.computerName : L"-",
          ver[0] ? ver : L"-", st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute);
    }

    // ---- 操作系统 / 计算机 ----
    W(L"<section><h2>\u64cd\u4f5c\u7cfb\u7edf</h2><table>\n");          // 操作系统
    W(L"<tr><th>\u4ea7\u54c1</th><td>"); WE(g_hw.osProduct); W(L"</td></tr>\n");       // 产品
    W(L"<tr><th>\u7248\u672c</th><td>"); WE(g_hw.osVersion);
    W(L" (Build "); WE(g_hw.osBuild); W(L")</td></tr>\n");                             // 版本
    W(L"<tr><th>\u67b6\u6784</th><td>"); WE(g_hw.osArch); W(L"</td></tr>\n");          // 架构
    W(L"<tr><th>\u903b\u8f91\u5904\u7406\u5668</th><td>%lu</td></tr>\n",               // 逻辑处理器
      g_hw.logicalProcTotal);
    W(L"</table></section>\n");

    W(L"<section><h2>\u8ba1\u7b97\u673a</h2><table>\n");                                // 计算机
    W(L"<tr><th>\u540d\u79f0</th><td>"); WE(g_hw.computerName); W(L"</td></tr>\n");     // 名称
    W(L"<tr><th>\u5f53\u524d\u7528\u6237</th><td>"); WE(g_hw.userName); W(L"</td></tr>\n"); // 当前用户
    W(L"</table></section>\n");

    // ---- 主板 / BIOS ----
    W(L"<section><h2>\u4e3b\u677f / BIOS</h2><table>\n");
    W(L"<tr><th>\u5236\u9020\u5546</th><td>"); WE(g_hw.boardVendor); W(L"</td></tr>\n");   // 制造商
    W(L"<tr><th>\u4ea7\u54c1</th><td>"); WE(g_hw.boardProduct); W(L"</td></tr>\n");        // 产品
    W(L"<tr><th>BIOS</th><td>"); WE(g_hw.biosVendor); W(L" "); WE(g_hw.biosVersion);
    if (g_hw.biosDate[0]) { W(L" &nbsp;\u00b7&nbsp; "); WE(g_hw.biosDate); }
    W(L"</td></tr>\n");                           // 先闭合 BIOS 行（避免芯片组行被嵌进 BIOS 单元格）
    if (g_hw.boardChipset[0]) {
        W(L"<tr><th>\u82af\u7247\u7ec4</th><td>"); WE(g_hw.boardChipset); W(L"</td></tr>\n");   // v1.1.8
    }
    W(L"</table></section>\n");

    // ---- 处理器（每颗明细）----
    W(L"<section><h2>\u5904\u7406\u5668</h2><table>\n");                                 // 处理器
    if (g_hw.cpuCount == 0) {
        W(L"<tr><td class=\"empty\">\u672a\u77e5</td></tr>\n");                          // 未知
    }
    for (int i = 0; i < g_hw.cpuCount; i++) {
        const HwCpu* c = &g_hw.cpus[i];
        W(L"<tr><th>#%d %ls</th><td>", i + 1, c->socket);
        WE(c->name);
        // v1.1.10：规格行直接显示 CPUID 真实硬件能力（物理核/线程），不再并排"启用值"
        W(L"<br><span class=\"mono\">%u \u6838 / %u \u7ebf\u7a0b", c->maxCores, c->maxThreads);
        if (c->speedMHz) W(L" &nbsp;\u00b7&nbsp; %u MHz", c->speedMHz);
        W(L"</span><br><span class=\"mono\">");
        WE(c->vendor);
        W(L"</span></td></tr>\n");
        // v1.2.5：CPU 微架构代号（libcpuid），识别到才输出
        if (c->codename[0]) {
            W(L"<tr><th>\u4ee3\u53f7</th><td>");   // 代号
            WE(c->codename);
            W(L"</td></tr>\n");
        }
    }
    W(L"</table></section>\n");

    // ---- 内存（每插槽明细）----
    W(L"<section><h2>\u5185\u5b58</h2><table>\n");                                       // 内存
    { WCHAR mt[32]; FormatBytesInt((ULONGLONG)g_hw.memTotalMB * 1024ULL * 1024ULL, mt, _countof(mt));
      W(L"<tr><th>\u603b\u8ba1</th><td>%ls</td></tr>\n", mt); }                          // 总计
    if (g_hw.memCount == 0) {
        W(L"<tr><td class=\"empty\">\u672a\u77e5</td></tr>\n");                          // 未知
    }
    for (int i = 0; i < g_hw.memCount; i++) {
        const HwMemStick* m = &g_hw.mem[i];
        W(L"<tr><th>"); WE(m->locator); W(L"</th><td>");
        if (m->populated) {
            WCHAR cap[32];
            FormatBytesInt((ULONGLONG)m->capacityMB * 1024ULL * 1024ULL, cap, _countof(cap));
            W(L"%ls", cap);
            if (m->memType[0]) W(L" &nbsp;\u00b7&nbsp; %ls", m->memType);   // v1.1.8：内存类型
            if (m->speedMHz) W(L" &nbsp;\u00b7&nbsp; %u MHz", m->speedMHz);
            W(L"<br><span class=\"mono\">"); WE(m->vendor); W(L" "); WE(m->partNo); W(L"</span>");
            if (m->bank[0]) { W(L"<br>"); WE(m->bank); }
        } else {
            W(L"<span class=\"empty\">\u7a7a\u69fd</span>");                             // 空槽
        }
        W(L"</td></tr>\n");
    }
    W(L"</table></section>\n");

    // ---- 显卡 / 显示器 ----
    W(L"<section><h2>\u663e\u5361 / \u663e\u793a\u5668</h2><table>\n");                 // 显卡 / 显示器
    if (g_hw.gpuCount == 0) W(L"<tr><th>\u663e\u5361</th><td class=\"empty\">\u672a\u77e5</td></tr>\n"); // 显卡/未知
    for (int i = 0; i < g_hw.gpuCount; i++) {
        const HwGpu* gg = &g_hw.gpus[i];
        W(L"<tr><th>\u663e\u5361 #%d</th><td>", i + 1); WE(gg->name);
        if (gg->vramMB > 0) W(L" &nbsp;\u00b7&nbsp; %u GB", (gg->vramMB + 512) / 1024);  // v1.1.8 显存
        // v1.2.3：NVML 遥测（N 卡才有，非 N 卡 / 无驱动全 0 则不写）
        if (GpuHasTelemetry(gg)) {
            int totalMB = (gg->memUsedMB + gg->memFreeMB > 0) ? (gg->memUsedMB + gg->memFreeMB)
                                                               : (int)gg->vramMB;
            W(L"<br>\u72b6\u6001: \u6e29\u5ea6 %d\u00b0C \u00b7 \u98ce\u6247 %d%% \u00b7 %dW \u00b7 \u5229\u7528 %d%% \u00b7 \u663e\u5b58 %d/%d MB \u00b7 %u MHz",
              gg->tempC, gg->fanPct, gg->powerW, gg->utilPct, gg->memUsedMB, totalMB, gg->clockMHz);
        }
        W(L"</td></tr>\n");
    }
    if (g_hw.displayCount == 0)
        W(L"<tr><th>\u663e\u793a\u5668</th><td class=\"empty\">\u672a\u77e5</td></tr>\n");  // 显示器/未知
    for (int i = 0; i < g_hw.displayCount; i++) {
        const HwDisplay* d = &g_hw.displays[i];
        W(L"<tr><th>\u663e\u793a\u5668 #%d</th><td>%u \u00d7 %u @ %u Hz",
          i + 1, d->width, d->height, d->refreshHz);
        if (d->size[0]) W(L" &nbsp;\u00b7&nbsp; %ls", d->size);   // v1.1.8：尺寸同行追加
        W(L"<br><span class=\"mono\">");
        WE(d->name);
        W(L"</span>");
        if (d->vendor[0] && !DispVendorRedundant(d)) W(L"<br>\u5382\u5546\uff1a%ls", d->vendor);
        W(L"</td></tr>\n");
    }
    W(L"</table></section>\n");

    // ---- 物理硬盘（明细）----
    W(L"<section><h2>\u786c\u76d8</h2><table>\n");                                       // 硬盘
    if (g_hw.diskCount == 0) W(L"<tr><td class=\"empty\">\u672a\u77e5</td></tr>\n");     // 未知
    for (int i = 0; i < g_hw.diskCount; i++) {
        const HwDisk* d = &g_hw.disks[i];
        WCHAR sz[48]; HtmlSize(d->sizeBytes, sz, _countof(sz));
        W(L"<tr><th>PhysicalDrive%d</th><td>", d->index);
        WE(d->name);
        W(L"<br><span class=\"mono\">%ls &nbsp;\u00b7&nbsp; %ls &nbsp;\u00b7&nbsp; %ls</span></td></tr>\n",
          d->iface, d->media, sz);
    }
    if (g_hw.diskTotalBytes) {
        WCHAR sz[48]; HtmlSize(g_hw.diskTotalBytes, sz, _countof(sz));
        W(L"<tr><th>\u603b\u8ba1</th><td><span class=\"mono\">%ls</span></td></tr>\n", sz);  // 总计
    }
    W(L"</table></section>\n");

    // ---- 网卡（全量：MAC / IPv4 / IPv6）----
    W(L"<section><h2>\u7f51\u5361</h2><table>\n");                                       // 网卡
    if (g_hw.nicCount == 0) W(L"<tr><td class=\"empty\">\u672a\u77e5</td></tr>\n");      // 未知
    for (int i = 0; i < g_hw.nicCount; i++) {
        const HwNic* n = &g_hw.nics[i];
        W(L"<tr><th>#%d</th><td>", i + 1);
        WE(n->desc);
        W(L"<br><span class=\"mono\">MAC %ls", n->mac);
        if (n->ipv4[0] && wcscmp(n->ipv4, L"-")) W(L" &nbsp;\u00b7&nbsp; IPv4 %ls", n->ipv4);
        if (n->ipv6[0] && wcscmp(n->ipv6, L"-")) W(L" &nbsp;\u00b7&nbsp; IPv6 %ls", n->ipv6);
        W(L"</span></td></tr>\n");
    }
    W(L"</table></section>\n");

    // ---- 分区 / 簇大小（来自磁盘信息页）----
    W(L"<section><h2>\u5206\u533a\u4e0e\u7c07\u5927\u5c0f</h2>\n");                       // 分区与簇大小
    if (g_diskCount == 0) {
        W(L"<div class=\"empty\">\u672a\u77e5</div>\n");                                  // 未知
    } else {
        W(L"<table><tr><th style=\"width:auto\">\u76d8\u7b26</th><th style=\"width:auto\">NT \u8bbe\u5907\u540d / \u78c1\u76d8</th><th style=\"width:auto\">\u7c7b\u578b</th>"  // 盘符 / NT 设备名·磁盘 / 类型
          L"<th style=\"width:auto\">\u6587\u4ef6\u7cfb\u7edf</th><th style=\"width:auto\">\u7c07\u5927\u5c0f</th>"             // 文件系统 / 簇大小
          L"<th style=\"width:auto\">\u603b\u5bb9\u91cf</th><th style=\"width:auto\">\u53ef\u7528</th>"             // 总容量 / 可用
          L"<th style=\"width:auto\">\u5e8f\u5217\u53f7</th><th style=\"width:auto\">\u5377\u6807</th></tr>\n");  // 序列号 / 卷标
        for (int i = 0; i < g_diskCount; i++) {
            const DiskInfo* d = &g_disks[i];
            WCHAR clu[32], tot[32], fre[32], pdStr[32], modelEsc[128], busEsc[32], serEsc[64], volEsc[128];
            FormatClusterStr(d->clusterBytes, clu, _countof(clu));
            HtmlSize(d->totalBytes, tot, _countof(tot));
            HtmlSize(d->freeBytes, fre, _countof(fre));
            if (d->diskIndex >= 0) StringCchPrintfW(pdStr, _countof(pdStr),
                                          L"\\\\.\\PhysicalDrive%d%s",
                                          d->diskIndex, d->diskExtents);
            else                   StringCchCopyW(pdStr, _countof(pdStr), L"-");
            if (d->unpartitioned) {
                HtmlEscape(d->model[0] ? d->model : L"\u672a\u77e5\u578b\u53f7", modelEsc, _countof(modelEsc));
                HtmlEscape(d->busType[0] ? d->busType : L"-", busEsc, _countof(busEsc));
                HtmlEscape(d->serial[0] ? d->serial : L"-", serEsc, _countof(serEsc));
                W(L"<tr><td class=\"mono\">"); WE(d->letter); W(L"</td>");
                W(L"<td class=\"mono\">"); WE(pdStr); W(L"</td>");
                W(L"<td>"); WE(d->typeName); W(L"</td>");
                W(L"<td class=\"mono\">%ls <span style=\"color:#888\">(%ls)</span></td>", modelEsc, busEsc);
                W(L"<td class=\"mono\">—</td><td class=\"mono\">%ls</td><td class=\"mono\">—</td>", tot);
                W(L"<td class=\"mono\">%ls</td><td class=\"mono\">—</td></tr>\n", serEsc);
            } else {
                HtmlEscape(d->serial[0] ? d->serial : L"-", serEsc, _countof(serEsc));
                HtmlEscape(d->volLabel[0] ? d->volLabel : L"-", volEsc, _countof(volEsc));
                W(L"<tr><td class=\"mono\">"); WE(d->letter); W(L"</td>");
                W(L"<td class=\"mono\">"); WE(pdStr); W(L"</td>");
                W(L"<td>"); WE(d->typeName); W(L"</td>");
                W(L"<td class=\"mono\">"); WE(d->fsName); W(L"</td>");
                W(L"<td class=\"mono\">%ls</td><td class=\"mono\">%ls</td><td class=\"mono\">%ls</td>", clu, tot, fre);
                W(L"<td class=\"mono\">%ls</td><td class=\"mono\">%ls</td></tr>\n", serEsc, volEsc);
            }
        }
        W(L"</table>\n");
    }
    W(L"</section>\n");

    // ---- 音频 / USB（只列真实识别到的；无则不输出整段）----
    if (!g_hw.isServer && g_hw.audioCount) {
        W(L"<section><h2>\u97f3\u9891\u8bbe\u5907</h2><table>\n");                        // 音频设备
        for (int i = 0; i < g_hw.audioCount; i++) {
            W(L"<tr><th>#%d</th><td>", i + 1); WE(g_hw.audios[i].name); W(L"</td></tr>\n");
        }
        W(L"</table></section>\n");
    }
    if (!g_hw.isServer && g_hw.usbCount) {
        W(L"<section><h2>USB \u5916\u8bbe</h2><table>\n");                               // USB 外设
        for (int i = 0; i < g_hw.usbCount; i++) {
            W(L"<tr><th>"); WE(g_hw.usbs[i].cls); W(L"</th><td>");
            WE(g_hw.usbs[i].name); W(L"</td></tr>\n");
        }
        W(L"</table></section>\n");
    }

    // ---- 右键功能启用状态 ----
    W(L"<section><h2>WinSuiteX \u529f\u80fd\u72b6\u6001</h2><table>\n");                 // 功能状态
    for (int i = 0; i < (int)g_featureCount; i++) {
        const Feature* f = &g_features[i];
        BOOL on = Feature_IsInstalled(f);
        W(L"<tr><th>"); WE(f->displayName); W(L"</th><td>");
        W(L"<span class=\"badge %ls\">%ls</span> &nbsp; ",
          on ? L"on" : L"off", on ? L"\u5df2\u542f\u7528" : L"\u672a\u542f\u7528");      // 已启用 / 未启用
        WE(f->description);
        W(L"</td></tr>\n");
    }
    W(L"</table></section>\n");

    W(L"<div class=\"foot\">WinSuiteX &nbsp;\u00b7&nbsp; "
      L"\u91cd\u5e86\u6301\u739b\u591a\u7f51\u7edc\u79d1\u6280\u6709\u9650\u516c\u53f8 &nbsp;\u00b7&nbsp; Z.W.</div>\n");
    W(L"</div>\n</body>\n</html>\n");
    #undef W
    #undef WE
    return out[0] != 0;
}

// 写 UTF-8 文件（带 BOM，保证双击用浏览器打开不乱码）
// 非 static：selftest 需要直接调用做落盘验证
BOOL WriteUtf8File(const WCHAR* path, const WCHAR* text) {
    int cb = WideCharToMultiByte(CP_UTF8, 0, text, -1, NULL, 0, NULL, NULL);
    if (cb <= 1) return FALSE;
    char* buf = (char*)malloc((size_t)cb + 8);
    if (!buf) return FALSE;
    int got = WideCharToMultiByte(CP_UTF8, 0, text, -1, buf, cb, NULL, NULL);
    if (got <= 0) { free(buf); return FALSE; }
    FILE* f = _wfopen(path, L"wb");
    if (!f) { free(buf); return FALSE; }
    const unsigned char bom[3] = {0xEF, 0xBB, 0xBF};
    size_t bodyLen = (size_t)got - 1;   // 去掉 WideCharToMultiByte 补的 NUL
    BOOL ok = (fwrite(bom, 1, 3, f) == 3) && (fwrite(buf, 1, bodyLen, f) == bodyLen);
    fclose(f);
    free(buf);
    return ok;
}

// 导出 HTML 报告到桌面并打开
static void ExportReport(HWND hWnd) {
    WCHAR desk[MAX_PATH] = {0};
    if (!SHGetSpecialFolderPathW(NULL, desk, CSIDL_DESKTOPDIRECTORY, FALSE) || !desk[0]) {
        // 回退：USERPROFILE\Desktop
        WCHAR prof[MAX_PATH] = {0};
        if (!GetEnvironmentVariableW(L"USERPROFILE", prof, MAX_PATH)) {
            ShowNotice(hWnd, L"WinSuiteX", L"\u65e0\u6cd5\u5b9a\u4f4d\u684c\u9762\u76ee\u5f55\u3002", TRUE);
            return;
        }
        StringCchPrintfW(desk, _countof(desk), L"%ls\\Desktop", prof);
    }

    // 报告缓冲走堆：放 static 会被塞进 .data 段，凭空把 exe 撑大近 200KB
    #define REPORT_MAX_CHARS 40960
    WCHAR* html = (WCHAR*)malloc(sizeof(WCHAR) * REPORT_MAX_CHARS);
    if (!html) {
        ShowNotice(hWnd, L"WinSuiteX", L"\u5185\u5b58\u4e0d\u8db3\uff0c\u65e0\u6cd5\u751f\u6210\u62a5\u544a\u3002", TRUE);
        return;
    }
    BOOL built = Report_BuildHtml(html, REPORT_MAX_CHARS);
    if (!built) {
        free(html);
        ShowNotice(hWnd, L"WinSuiteX", L"\u62a5\u544a\u751f\u6210\u5931\u8d25\u3002", TRUE);
        return;
    }

    SYSTEMTIME st; GetLocalTime(&st);
    WCHAR path[1024];                 // 桌面路径本身可能很长，多留余量避免截断
    WCHAR safeName[64];
    StringCchCopyW(safeName, _countof(safeName),
                   g_hw.computerName[0] ? g_hw.computerName : L"PC");
    for (WCHAR* p = safeName; *p; p++)          // 计算机名里可能有非法文件名字符
        if (*p == L'\\' || *p == L'/' || *p == L':' || *p == L'*' || *p == L'?' ||
            *p == L'"' || *p == L'<' || *p == L'>' || *p == L'|') *p = L'_';
    if (FAILED(StringCchPrintfW(path, _countof(path),
                     L"%ls\\WinSuiteX_\u62a5\u544a_%ls_%04d%02d%02d_%02d%02d.html",
                     desk, safeName, st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute))) {
        free(html);
        ShowNotice(hWnd, L"WinSuiteX",
                   L"\u684c\u9762\u8def\u5f84\u8fc7\u957f\uff0c\u65e0\u6cd5\u62fc\u63a5\u5bfc\u51fa\u8def\u5f84\u3002", TRUE);
        return;
    }

    if (!WriteUtf8File(path, html)) {
        free(html);
        ShowNotice(hWnd, L"WinSuiteX",
                   L"\u62a5\u544a\u5199\u5165\u5931\u8d25\uff0c\u8bf7\u786e\u8ba4\u5bf9\u684c\u9762\u6709\u5199\u6743\u9650\u3002", TRUE);
        return;
    }
    free(html);
    #undef REPORT_MAX_CHARS

    g_copyAtTick = GetTickCount();
    InvalidateRect(hWnd, NULL, FALSE);

    // 直接打开，方便立刻查看/另存/发给客户。返回值 <= 32 表示失败（无关联程序等），
    // 此时文件已经落盘，不弹错误，只在提示里说明需要手动打开。
    intptr_t shRet = (intptr_t)ShellExecuteW(NULL, L"open", path, NULL, NULL, SW_SHOWNORMAL);
    BOOL opened = (shRet > 32);

    const WCHAR* head = opened
        ? L"\u62a5\u544a\u5df2\u5bfc\u51fa\u5e76\u6253\u5f00\uff1a"
        : L"\u62a5\u544a\u5df2\u5bfc\u51fa\uff08\u672a\u80fd\u81ea\u52a8\u6253\u5f00\uff0c\u8bf7\u624b\u52a8\u6253\u5f00\uff09\uff1a";
    WCHAR msg[700];
    StringCchPrintfW(msg, _countof(msg),
                     L"%ls\n%ls\n\n"
                     L"\u5185\u5bb9\u5305\u542b\u5168\u90e8\u786c\u4ef6\u660e\u7ec6\uff08\u5185\u5b58\u63d2\u69fd / "
                     L"\u7f51\u5361 / USB \u5916\u8bbe / \u5206\u533a\u4e0e\u7c07\u5927\u5c0f\uff09\u3002\n\n"
                     L"\u6587\u4ef6\u4f1a\u4fdd\u7559\u5728\u684c\u9762\uff0c\u53ef\u81ea\u884c\u5220\u9664\u6216\u5f52\u6863\u3002",
                     head, path);
    ShowNotice(hWnd, L"WinSuiteX", msg, FALSE);
}

static void DrawHardwarePage(HDC hdc, int sp) {
    // 内容内边距：让"标签:值"文字、段标题磨砂带、页眉分隔线都缩进到玻璃面板内，
    // 不再顶到内边框（原来 xLeft/xRight 直接等于面板左右边，零内边距导致文字超出内边框）。
    int padIn  = Scale(16);
    int xLeft  = Scale(MARGIN + SHADOW_PAD) + padIn;
    int xRight = Scale(WIN_W - MARGIN + SHADOW_PAD) - padIn;

    // ---- 顶部说明 + 导出报告按钮（v1.0.9：一键复制改为导出 HTML 报告到桌面）----
    int yHead = Scale(TITLE_H + TAB_H + 14 + SHADOW_PAD);
    HFONT hf = MakeFont(Scale(16), FW_SEMIBOLD);
    HFONT of = (HFONT)SelectObject(hdc, hf);
    // 玻璃风：页眉改浅色，对齐 demo 视觉
    SetTextColor(hdc, RGB(245, 246, 255));
    SetBkMode(hdc, TRANSPARENT);
    RECT r = {xLeft, yHead, g_copyBtnRect.left - Scale(12), yHead + Scale(26)};
    DrawTextW(hdc, L"\u8ba1\u7b97\u673a\u5e38\u89c4\u914d\u7f6e",
              -1, &r, DT_LEFT | DT_SINGLELINE | DT_VCENTER);
    SelectObject(hdc, of);
    DeleteObject(hf);
    // 玻璃风：页眉下加分隔细线，对齐 demo 视觉语言
    {
        HPEN lp = CreatePen(PS_SOLID, 1, C_TXT_LIGHT_DIM);
        HPEN olp = (HPEN)SelectObject(hdc, lp);
        int dy = yHead + Scale(28);
        MoveToEx(hdc, xLeft, dy, NULL);
        LineTo(hdc, g_copyBtnRect.left - Scale(12), dy);
        SelectObject(hdc, olp);
        DeleteObject(lp);
    }
    DrawCopyButton(hdc, &g_copyBtnRect, g_copyBtnHover,
                   L"\u5bfc\u51fa\u62a5\u544a", L"\u5df2\u5bfc\u51fa");

    int y = Scale(TITLE_H + TAB_H + HW_HEAD + SHADOW_PAD);

    if (!g_hwCollected) {
        HFONT fu = MakeFont(Scale(13), FW_NORMAL);
        HFONT ou = (HFONT)SelectObject(hdc, fu);
        SetTextColor(hdc, C_SUBTITLE);
        RECT er = {xLeft, y, xRight, y + Scale(120)};
        DrawTextW(hdc, L"\u6b63\u5728\u91c7\u96c6\u786c\u4ef6\u4fe1\u606f\uff0c\u8bf7\u7a0d\u5019...",
                  -1, &er, DT_LEFT | DT_TOP | DT_SINGLELINE);
        SelectObject(hdc, ou);
        DeleteObject(fu);
        return;
    }

    // 严格按 ComputeLayout 排好的计划绘制（段数/行数不可能与窗口高度脱节）
    for (int si = 0; si < g_hwPlanCount; si++) {
        const HwSecPlan& sec = g_hwPlan[si];

        switch (sec.id) {

        // ---- 操作系统：产品 · 架构 · 版本(Build)（v1.0.9：不再显示安装时间）----
        case HWSEC_OS: {
            y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u64cd\u4f5c\u7cfb\u7edf", HWSEC_OS);
            WCHAR val[256], versionLine[96];
            StringCchCopyW(versionLine, _countof(versionLine),
                           g_hw.osVersion[0] ? g_hw.osVersion : L"\u672a\u77e5");
            if (g_hw.osBuild[0]) {
                StringCchCatW(versionLine, _countof(versionLine), L" (Build ");
                StringCchCatW(versionLine, _countof(versionLine), g_hw.osBuild);
                StringCchCatW(versionLine, _countof(versionLine), L")");
            }
            StringCchPrintfW(val, _countof(val), L"%ls \u00b7 %ls \u00b7 %ls",
                             g_hw.osProduct[0] ? g_hw.osProduct : L"\u672a\u77e5",
                             g_hw.osArch[0] ? g_hw.osArch : L"\u672a\u77e5",
                             versionLine);
            y = DrawHwRow(hdc, xLeft, xRight, y, L"\u7cfb\u7edf", val, FALSE);
            break;
        }

        // ---- 计算机：名称 · 当前用户 ----
        case HWSEC_MACHINE: {
            y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u8ba1\u7b97\u673a", HWSEC_MACHINE);
            WCHAR val[160];
            StringCchPrintfW(val, _countof(val), L"%ls%ls%ls",
                             g_hw.computerName[0] ? g_hw.computerName : L"\u672a\u77e5",
                             g_hw.userName[0] ? L" \u00b7 " : L"",
                             g_hw.userName[0] ? g_hw.userName : L"");
            y = DrawHwRow(hdc, xLeft, xRight, y, L"\u540d\u79f0/\u7528\u6237", val, FALSE);
            break;
        }

        // ---- 主板 + BIOS（v1.1.5 加回 BIOS 明细，原 v1.0.8.1 合并段在后续改动中丢失）----
        case HWSEC_BOARD: {
            y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u4e3b\u677f / BIOS", HWSEC_BOARD);
            WCHAR mb[192];
            if (g_hw.boardVendor[0] || g_hw.boardProduct[0])
                StringCchPrintfW(mb, _countof(mb), L"%ls %ls", g_hw.boardVendor, g_hw.boardProduct);
            else
                StringCchCopyW(mb, _countof(mb), L"\u672a\u77e5");
            y = DrawHwRow(hdc, xLeft, xRight, y, L"\u578b\u53f7", mb, FALSE);
            WCHAR bios[256];
            if (g_hw.biosVendor[0] || g_hw.biosVersion[0] || g_hw.biosDate[0])
                StringCchPrintfW(bios, _countof(bios), L"%ls %ls %ls",
                                 g_hw.biosVendor, g_hw.biosVersion, g_hw.biosDate);
            else
                StringCchCopyW(bios, _countof(bios), L"\u672a\u77e5");
            y = DrawHwRow(hdc, xLeft, xRight, y, L"BIOS", bios, FALSE);
            if (g_hw.boardChipset[0])
                y = DrawHwRow(hdc, xLeft, xRight, y, L"\u82af\u7247\u7ec4", g_hw.boardChipset, FALSE);   // v1.1.8
            break;
        }

        // ---- 处理器 ----
        case HWSEC_CPU: {
            y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u5904\u7406\u5668", HWSEC_CPU);
            if (g_hw.cpuCount == 0) {
                y = DrawHwRow(hdc, xLeft, xRight, y, L"\u578b\u53f7", L"\u672a\u77e5", FALSE);
            } else {
                // v1.2.7：遍历所有 CPU；多颗时每颗独立成块（标签 CPU#i），单颗时标签"型号"
                for (int i = 0; i < g_hw.cpuCount && i < HW_MAX_CPU; i++) {
                    const HwCpu* c = &g_hw.cpus[i];
                    // 型号行：完整型号名（过长自动省略号裁剪）
                    WCHAR modelLbl[16];
                    if (g_hw.cpuCount > 1)
                        StringCchPrintfW(modelLbl, _countof(modelLbl), L"CPU#%d", i + 1);
                    else
                        StringCchCopyW(modelLbl, _countof(modelLbl), L"\u578b\u53f7");
                    y = DrawHwRow(hdc, xLeft, xRight, y, modelLbl, c->name, FALSE);
                    // 规格行：CPUID 真实硬件能力（物理核/线程 + 频率），维护盘点最关心的真实规格；
                    // v1.1.10：不再并排显示 BIOS 启用的"当前核数"，避免与真实规格混淆（如 BIOS
                    // 单核节能时启用 1 核却真实是 4 核），也消除冗长的"(硬件 X)"标注
                    WCHAR spec[128];
                    if (c->speedMHz)
                        StringCchPrintfW(spec, _countof(spec), L"%u \u6838 / %u \u7ebf\u7a0b \u00b7 %u MHz",
                                         c->maxCores, c->maxThreads, c->speedMHz);
                    else
                        StringCchPrintfW(spec, _countof(spec), L"%u \u6838 / %u \u7ebf\u7a0b",
                                         c->maxCores, c->maxThreads);
                    y = DrawHwRow(hdc, xLeft, xRight, y, L"\u89c4\u683c", spec, FALSE);
                }
            }
            break;
        }

        // ---- 内存：v1.0.12 展开已插内存条（汇总 + 每条 + 空槽折叠）----
        case HWSEC_MEM: {
            y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u5185\u5b58", HWSEC_MEM);
            WCHAR sum[96];
            WCHAR memTotal[32];
            FormatBytesInt((ULONGLONG)g_hw.memTotalMB * 1024ULL * 1024ULL, memTotal, _countof(memTotal));
            if (g_hw.memCount > 0)
                StringCchPrintfW(sum, _countof(sum),
                                 L"%ls \u00b7 \u5df2\u63d2 %d / \u5171 %d \u63d2\u69fd",
                                 memTotal, g_hw.memPopulated, g_hw.memCount);
            else
                StringCchCopyW(sum, _countof(sum), memTotal);
            y = DrawHwRow(hdc, xLeft, xRight, y, L"\u603b\u8ba1", sum, FALSE);
            // v1.0.12：每条已插内存一行（"槽位 | 容量 · 频率 · 厂商"），料号进 HTML 报告
            // v1.0.12.1：locator 字段后加 " · " 视觉分隔符，避免 "ChannelA-DIMM" 紧贴 value
            for (int i = 0; i < g_hw.memCount && i < HW_MAX_MEM; i++) {
                const HwMemStick* m = &g_hw.mem[i];
                if (!m->populated) continue;
                WCHAR cap[24];
                if (m->capacityMB >= 1024)
                    swprintf(cap, _countof(cap), L"%lu GB", (m->capacityMB + 512) / 1024);
                else
                    swprintf(cap, _countof(cap), L"%lu MB", m->capacityMB);
                WCHAR val[200];
                // v1.1.5：插槽名放进 value（用 "·" 分隔），label 固定为"插槽"二字，
                // 避免 "ChannelA-DIMM1" 过长溢出 96px 标签列、与容量黏在一起
                // v1.1.8：内存类型仅在识别到时追加（识别不到不写"未知"，与 HTML/文本一致）
                WCHAR memSeg[64] = {0};
                if (m->memType[0])
                    StringCchPrintfW(memSeg, _countof(memSeg), L" \u00b7 %ls", m->memType);
                StringCchPrintfW(val, _countof(val), L"%ls \u00b7 %ls%ls \u00b7 %lu MHz \u00b7 %ls",
                                 m->locator[0] ? m->locator : L"\u672a\u77e5",
                                 cap, memSeg,
                                 m->speedMHz, m->vendor[0] ? m->vendor : L"\u672a\u77e5");
                WCHAR lbl[64];
                StringCchCopyW(lbl, _countof(lbl), L"\u63d2\u69fd");
                y = DrawHwRow(hdc, xLeft, xRight, y, lbl, val, FALSE);
            }
            int empty = g_hw.memCount - g_hw.memPopulated;
            if (empty > 0) {
                WCHAR es[32];
                StringCchPrintfW(es, _countof(es), L"\u7a7a\u69fd %d \u6761", empty);
                y = DrawHwRow(hdc, xLeft, xRight, y, L"\u7a7a\u69fd", es, FALSE);
            }
            break;
        }

        // ---- 显卡（v1.2.7：遍历所有显卡；多卡时每颗独立成块，标签 显卡#i；v1.1.8 同行追加显存）----
        case HWSEC_GPU: {
            y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u663e\u5361", HWSEC_GPU);
            if (g_hw.gpuCount == 0) {
                y = DrawHwRow(hdc, xLeft, xRight, y, L"\u578b\u53f7", L"\u672a\u77e5", FALSE);
            } else {
                for (int i = 0; i < g_hw.gpuCount && i < HW_MAX_GPU; i++) {
                    const HwGpu* g = &g_hw.gpus[i];
                    WCHAR gval[200];
                    if (g->vramMB > 0)
                        StringCchPrintfW(gval, _countof(gval), L"%ls \u00b7 %u GB",
                                         g->name, (g->vramMB + 512) / 1024);   // 512MB 四舍五入
                    else
                        StringCchCopyW(gval, _countof(gval), g->name);
                    WCHAR modelLbl[16];
                    if (g_hw.gpuCount > 1)
                        StringCchPrintfW(modelLbl, _countof(modelLbl), L"\u663e\u5361#%d", i + 1);
                    else
                        StringCchCopyW(modelLbl, _countof(modelLbl), L"\u578b\u53f7");
                    y = DrawHwRow(hdc, xLeft, xRight, y, modelLbl, gval, FALSE);
                    // v1.2.3：NVML 遥测状态行（温度/风扇/功耗/利用率/显存用/时钟）；全 0 则不画
                    if (GpuHasTelemetry(g)) {
                        WCHAR tv[160];
                        int totalMB = (g->memUsedMB + g->memFreeMB > 0) ? (g->memUsedMB + g->memFreeMB)
                                                                         : (int)g->vramMB;   // NVML total 优先，回退 DXGI 显存
                        StringCchPrintfW(tv, _countof(tv),
                            L"\u6e29\u5ea6 %d\u00b0C \u00b7 \u98ce\u6247 %d%% \u00b7 %dW \u00b7 \u5229\u7528 %d%% \u00b7 \u663e\u5b58 %d/%d MB \u00b7 %u MHz",
                            g->tempC, g->fanPct, g->powerW, g->utilPct,
                            g->memUsedMB, totalMB, g->clockMHz);
                        y = DrawHwRow(hdc, xLeft, xRight, y, L"\u72b6\u6001", tv, FALSE);
                    }
                }
            }
            break;
        }

        // ---- 硬盘（v1.0.9 新增：物理盘型号 / 接口 / 介质 / 容量）----
        case HWSEC_DISK: {
            y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u786c\u76d8", HWSEC_DISK);
            if (g_hw.diskCount == 0) {
                y = DrawHwRow(hdc, xLeft, xRight, y, L"-", L"\u672a\u77e5", FALSE);
            } else {
                for (int i = 0; i < g_hw.diskCount && i < HW_MAX_DISK; i++) {
                    const HwDisk* d = &g_hw.disks[i];
                    WCHAR sz[48]; FormatBytesStr(d->sizeBytes, sz, _countof(sz));
                    WCHAR val[256], lbl[24];
                    StringCchPrintfW(lbl, _countof(lbl), L"\u786c\u76d8#%d", i + 1);
                    StringCchPrintfW(val, _countof(val), L"%ls \u00b7 %ls \u00b7 %ls \u00b7 %ls",
                                     d->name, d->iface, d->media, sz);
                    y = DrawHwRow(hdc, xLeft, xRight, y, lbl, val, FALSE);
                }
            }
            break;
        }

        // ---- 显示器（v1.0.12.1：每显示器 2 行"分辨率 + 型号"，多显示器全列）----
        case HWSEC_DISP: {
            y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u663e\u793a\u5668", HWSEC_DISP);
            if (g_hw.displayCount == 0) {
                y = DrawHwRow(hdc, xLeft, xRight, y, L"-", L"\u672a\u77e5", FALSE);
            } else {
                for (int i = 0; i < g_hw.displayCount && i < HW_MAX_DISPLAY; i++) {
                    const HwDisplay* d = &g_hw.displays[i];
                    WCHAR res[64];
                    if (d->size[0])
                        StringCchPrintfW(res, _countof(res), L"%u \u00d7 %u @ %u Hz \u00b7 %ls",
                                         d->width, d->height, d->refreshHz, d->size);   // v1.1.8：尺寸同行追加
                    else
                        StringCchPrintfW(res, _countof(res), L"%u \u00d7 %u @ %u Hz",
                                         d->width, d->height, d->refreshHz);
                    WCHAR lbl[16];
                    StringCchPrintfW(lbl, _countof(lbl), L"\u663e\u793a\u5668#%d", i + 1);
                    y = DrawHwRow(hdc, xLeft, xRight, y, lbl, res, FALSE);
                    if (d->name[0])
                        y = DrawHwRow(hdc, xLeft, xRight, y, L"\u578b\u53f7", d->name, FALSE);
                    if (d->vendor[0] && !DispVendorRedundant(d))
                        y = DrawHwRow(hdc, xLeft, xRight, y, L"\u5382\u5546", d->vendor, FALSE);
                }
            }
            break;
        }

        // ---- 网卡：展示前 2 块真实网卡；超过 2 块时加"总计"行 ----
        case HWSEC_NIC: {
            y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u7f51\u5361", HWSEC_NIC);
            if (g_hw.nicCount == 0) {
                y = DrawHwRow(hdc, xLeft, xRight, y, L"-", L"\u672a\u77e5", FALSE);
            } else {
                // 优先显示已连接（有 IP）的网卡；其余按采集顺序兜底
                int shown = 0;
                for (int pass = 0; pass < 2 && shown < 2; pass++) {
                    for (int i = 0; i < g_hw.nicCount && shown < 2; i++) {
                        const HwNic* n = &g_hw.nics[i];
                        BOOL hasIp = n->ipv4[0] && wcscmp(n->ipv4, L"-") != 0;
                        if (pass == 0 && !hasIp) continue;   // 第一遍只显示有 IP 的
                        if (pass == 1 && hasIp) continue;    // 第二遍补上没有 IP 的
                        WCHAR val[384];
                        StringCchPrintfW(val, _countof(val), L"%ls \u00b7 %ls",
                                         n->desc, hasIp ? n->ipv4 : L"\u65e0 IP");
                        WCHAR lbl[16];
                        if (shown == 0)
                            StringCchCopyW(lbl, _countof(lbl), L"\u4e3b\u7f51\u5361");
                        else
                            StringCchPrintfW(lbl, _countof(lbl), L"\u7f51\u5361#%d", shown + 1);
                        y = DrawHwRow(hdc, xLeft, xRight, y, lbl, val, FALSE);
                        shown++;
                    }
                }
                if (g_hw.nicCount > 2) {
                    WCHAR sum[32];
                    StringCchPrintfW(sum, _countof(sum), L"\u5171 %d \u5757\u7f51\u5361", g_hw.nicCount);
                    y = DrawHwRow(hdc, xLeft, xRight, y, L"\u603b\u8ba1", sum, FALSE);
                }
            }
            break;
        }

        // ---- 音频：v1.1.5 总计 + 当前默认播放 + 当前默认录音 ----
        case HWSEC_AUDIO: {
            y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"\u97f3\u9891\u8bbe\u5907", HWSEC_AUDIO);
            WCHAR sum[32];
            StringCchPrintfW(sum, _countof(sum), L"%d \u8bbe\u5907", g_hw.audioCount);
            y = DrawHwRow(hdc, xLeft, xRight, y, L"\u603b\u8ba1", sum, FALSE);
            // 找 role & 1（默认播放）/ role & 2（默认录音）
            const HwAudio* defPlay = NULL, *defRec = NULL;
            for (int i = 0; i < g_hw.audioCount; i++) {
                if (!defPlay && (g_hw.audios[i].role & 1)) defPlay = &g_hw.audios[i];
                if (!defRec && (g_hw.audios[i].role & 2)) defRec = &g_hw.audios[i];
            }
            WCHAR val[200];
            if (defPlay) {
                StringCchPrintfW(val, _countof(val), L"\u9ed8\u8ba4\u64ad\u653e \u00b7 %ls", defPlay->name);
                y = DrawHwRow(hdc, xLeft, xRight, y, L"\u64ad\u653e", val, FALSE);
            } else {
                y = DrawHwRow(hdc, xLeft, xRight, y, L"\u64ad\u653e", L"\u672a\u8bc6\u522b\u9ed8\u8ba4\u8bbe\u5907", FALSE);
            }
            if (defRec) {
                StringCchPrintfW(val, _countof(val), L"\u9ed8\u8ba4\u5f55\u97f3 \u00b7 %ls", defRec->name);
                y = DrawHwRow(hdc, xLeft, xRight, y, L"\u5f55\u97f3", val, FALSE);
            } else {
                y = DrawHwRow(hdc, xLeft, xRight, y, L"\u5f55\u97f3", L"\u672a\u8bc6\u522b\u9ed8\u8ba4\u8bbe\u5907", FALSE);
            }
            break;
        }

        // ---- USB 外设：按类折叠（明细进 HTML）----
        case HWSEC_USB: {
            y = DrawHwSectionTitle(hdc, xLeft, xRight, y, L"USB \u5916\u8bbe", HWSEC_USB);
            static const WCHAR* K_FLASH  = L"\u4fdd\u5b58";
            static const WCHAR* K_HID    = L"\u952e\u9f20";
            static const WCHAR* K_IMG    = L"\u6444\u50cf\u5934";
            static const WCHAR* K_PRINTER= L"\u6253\u5370\u673a";
            static const WCHAR* K_HUB    = L"\u96c6\u7ebf\u5668";
            static const WCHAR* K_OTHER  = L"\u5176\u4ed6";
            int nFlash=0, nHid=0, nImg=0, nPrinter=0, nHub=0, nOther=0;
            for (int i = 0; i < g_hw.usbCount; i++) {
                const WCHAR* c = g_hw.usbs[i].cls;
                if (!wcscmp(c, K_FLASH) || !wcscmp(c, L"USBSTOR") || !wcscmp(c, L"USBSTOR_CLASS")) nFlash++;
                else if (!wcscmp(c, K_HID) || !wcscmp(c, L"HID")) nHid++;
                else if (!wcscmp(c, K_IMG) || !wcscmp(c, L"Image") || !wcscmp(c, L"Camera")) nImg++;
                else if (!wcscmp(c, K_PRINTER) || !wcscmp(c, L"Printer")) nPrinter++;
                else if (!wcscmp(c, K_HUB) || !wcscmp(c, L"USBHub") || !wcscmp(c, L"Hub")) nHub++;
                else nOther++;
            }
            WCHAR line[256] = {0};
            WCHAR* p = line;
            size_t remain = _countof(line);
            #define APPEND_PART(label, n) do { \
                if ((n) > 0 && remain > 4) { \
                    if (p != line) { StringCchCatW(p, remain, L", "); remain -= 2; p += 2; } \
                    int wrote = swprintf(p, remain, L"%ls \u00d7 %d", label, n); \
                    if (wrote > 0) { p += wrote; remain -= (size_t)wrote; } \
                } \
            } while (0)
            APPEND_PART(K_FLASH,   nFlash);
            APPEND_PART(K_HID,     nHid);
            APPEND_PART(K_IMG,     nImg);
            APPEND_PART(K_PRINTER, nPrinter);
            APPEND_PART(K_HUB,     nHub);
            APPEND_PART(K_OTHER,   nOther);
            #undef APPEND_PART
            WCHAR sum[32];
            StringCchPrintfW(sum, _countof(sum), L"\u5171 %d \u4ef6 USB \u5916\u8bbe", g_hw.usbCount);
            y = DrawHwRow(hdc, xLeft, xRight, y, L"\u603b\u8ba1", sum, FALSE);
            y = DrawHwRow(hdc, xLeft, xRight, y, L"\u5206\u7c7b", line, FALSE);
            break;
        }

        default:
            break;
        }
    }
}

// 重排窗口：重算布局 -> 调整尺寸 -> 重建圆角 -> 全窗重绘。
// 两页内容高度不同（右键页约 784 / 磁盘页随分区数变化），切页时窗口高度会变。
// 以中心为锚点收放比锚定左上角只向下伸缩更平稳，也不会把窗口推出屏幕。
static void RelayoutWindow(HWND hWnd) {
    RECT rc;
    GetWindowRect(hWnd, &rc);
    int cx = (rc.left + rc.right) / 2;
    int cy = (rc.top + rc.bottom) / 2;

    ComputeLayout();

    int x = cx - g_clientW / 2;
    int y = cy - g_clientH / 2;
    // 夹紧到屏幕内（窗口比屏幕还大的极端情况兜底为 0）
    int sw = GetSystemMetrics(SM_CXSCREEN), sh = GetSystemMetrics(SM_CYSCREEN);
    if (x + g_clientW > sw) x = sw - g_clientW;
    if (y + g_clientH > sh) y = sh - g_clientH;
    if (x < 0) x = 0;
    if (y < 0) y = 0;
    SetWindowPos(hWnd, NULL, x, y, g_clientW, g_clientH,
                 SWP_NOZORDER | SWP_FRAMECHANGED);
    ApplyRoundedRegion(hWnd, g_clientW, g_clientH);
    InvalidateRect(hWnd, NULL, TRUE);
}

// 切换标签页
static void SwitchTab(HWND hWnd, int tab) {
    if (tab == g_curTab || tab < 0 || tab >= TAB_COUNT) return;
    g_curTab = tab;
    if (tab == TAB_DISK) EnumDisks();      // 切到磁盘页时重新枚举（插拔 U 盘后可见）
    // 清掉上一页残留的 hover（否则切页后可能残留高亮，直到鼠标移动才刷新）
    for (int i = 0; i < g_count; i++) g_hover[i] = FALSE;
    g_hoverDiskRow = -1;
    g_hoverDiskRefresh = FALSE;
    g_copyBtnHover = FALSE;   // 切页后旧页 hover 不能残留（页签位置不同）
    g_classicMenuHover = FALSE;   // 切页后旧页经典菜单复选框 hover 不能残留
    for (int t = 0; t < TAB_COUNT; t++) g_tabHover[t] = (t == tab) ? 1 : 0;
    RelayoutWindow(hWnd);
}

// 缩放策略：统一采用「先求和后 Scale」。每个坐标/尺寸都先把 96-DPI 基准常量求和，
// 再整体 Scale 一次（避免 Scale(a)+Scale(b) 的逐项舍入差导致绘制与命中检测 1px 错位）。
// 绘制与命中检测共用此处算出的 g_cardRect / g_btnRect / g_minBtnRect / g_closeBtnRect，
// 因此二者必然一致。
// 当前绘制计划对应的内容总高（96-DPI 基准，未 Scale）。
// 段标题各占 1 行高，每段尾部再加一次段间距（与 DrawHwSectionTitle 的返回一致）。
static int HwPlanBaseH() {
    int rows = 0;
    for (int i = 0; i < g_hwPlanCount; i++) rows += g_hwPlan[i].rows;
    return TITLE_H + TAB_H + HW_HEAD
           + g_hwPlanCount * (g_hwRowH + g_hwSecGap)
           + rows * g_hwRowH
           + FOOTER_H;
}

static void ComputeLayout() {
    int sp = Scale(SHADOW_PAD);
    int baseH;

    // 页签条（两页共用；坐标同样遵循"先求和后 Scale"）
    for (int t = 0; t < TAB_COUNT; t++) {
        int x0 = Scale(MARGIN + SHADOW_PAD + t * TAB_W);
        int x1 = Scale(MARGIN + SHADOW_PAD + (t + 1) * TAB_W);
        g_tabRects[t] = {x0, Scale(TITLE_H + SHADOW_PAD),
                         x1, Scale(TITLE_H + TAB_H + SHADOW_PAD)};
    }

    if (g_curTab == TAB_HW) {
        g_count = 0;

        // ---- 1) 排计划：每段几行由 HwSecRows() 单点决定，绘制端遍历同一份计划 ----
        g_hwPlanCount = 0;
        for (int id = 0; id < HWSEC_COUNT; id++) {
            int rows = HwSecRows(id);
            if (rows <= 0) continue;
            g_hwPlan[g_hwPlanCount].id = id;
            g_hwPlan[g_hwPlanCount].rows = rows;
            g_hwPlanCount++;
        }

        // ---- 2) 高度自适应：HiDPI + 1080p 时窗口会被撑到 1400+ px，必须收得下 ----
        g_hwRowH = HW_ROW_H;
        g_hwSecGap = HW_SECTION_GAP;
        int availH = AvailableScreenH();
        // 先压缩行距（不缩字号，字太小反而看不清）
        if (Scale(HwPlanBaseH() + 2 * SHADOW_PAD) > availH) { g_hwRowH = 23; g_hwSecGap = 9; }
        if (Scale(HwPlanBaseH() + 2 * SHADOW_PAD) > availH) { g_hwRowH = 20; g_hwSecGap = 6; }
        // 仍放不下：按优先级从后往前裁次要段（这些在导出的 HTML 报告里都有）
        static const int kDropOrder[] = { HWSEC_USB, HWSEC_AUDIO, HWSEC_DISP, HWSEC_NIC };
        for (int k = 0; k < (int)(sizeof(kDropOrder) / sizeof(kDropOrder[0])); k++) {
            if (Scale(HwPlanBaseH() + 2 * SHADOW_PAD) <= availH) break;
            for (int i = 0; i < g_hwPlanCount; i++) {
                if (g_hwPlan[i].id != kDropOrder[k]) continue;
                for (int j = i; j < g_hwPlanCount - 1; j++) g_hwPlan[j] = g_hwPlan[j + 1];
                g_hwPlanCount--;
                break;
            }
        }

        // 导出报告按钮（硬件页独有；磁盘页仍用 g_diskRefreshRect）
        int bw = Scale(88), bh = Scale(28);
        int rx = Scale(WIN_W - MARGIN - 18 + SHADOW_PAD);
        int ry = Scale(TITLE_H + TAB_H + 12) + sp;
        g_copyBtnRect = {rx - bw, ry, rx, ry + bh};

        baseH = HwPlanBaseH();
    } else if (g_curTab == TAB_MENU) {
        g_count = (int)g_featureCount;
        for (int i = 0; i < g_count; i++) {
            // 卡片顶部 Y：先把基准偏移求和再 Scale（页签条高度计入基准）
            int yTop = Scale(TITLE_H + TAB_H + HEADER_H + CARD_GAP + i * (CARD_H + CARD_GAP));
            int cardH = Scale(CARD_H);
            g_cardRect[i] = {Scale(MARGIN + SHADOW_PAD), yTop + sp,
                             Scale(WIN_W - MARGIN + SHADOW_PAD), yTop + cardH + sp};

            // 按钮在卡片内垂直居中、靠右（尺寸也先求和后 Scale）
            int bw = Scale(84), bh = Scale(32);
            int byTop = yTop + (cardH - bh) / 2 + sp;
            int btnRight = Scale(WIN_W - MARGIN - 18 + SHADOW_PAD);
            g_btnRect[i] = {btnRight - bw, byTop, btnRight, byTop + bh};
            g_hover[i] = FALSE;
        }
        // 经典右键菜单复选框（标题区右侧空白处，紧凑卡片）
        {
            int boxW = 200, boxH = 50;
            int top = TITLE_H + TAB_H + 18 + SHADOW_PAD;
            int right = WIN_W - MARGIN - 18 + SHADOW_PAD;
            g_classicMenuRect = {Scale(right - boxW), Scale(top) + sp,
                                 Scale(right), Scale(top + boxH) + sp};
        }
        baseH = TITLE_H + TAB_H + HEADER_H + CARD_GAP
                + g_count * (CARD_H + CARD_GAP) + FOOTER_H;
    } else {
        g_count = 0;      // 磁盘页不画功能卡片
        int rows = (g_diskCount < DISK_MAX_ROWS) ? g_diskCount : DISK_MAX_ROWS;
        int baseY = TITLE_H + TAB_H + DISK_PAGE_HEAD + DISK_TABLE_HEAD;
        for (int i = 0; i < rows; i++) {
            int y = Scale(baseY + i * DISK_ROW_H) + sp;
            g_diskRowRect[i] = {Scale(MARGIN + SHADOW_PAD), y,
                                Scale(WIN_W - MARGIN + SHADOW_PAD), y + Scale(DISK_ROW_H)};
        }
        // 磁盘页：刷新按钮（最右）+ 一键复制按钮（紧邻其左）
        int bw = Scale(72), bh = Scale(28);
        int rx = Scale(WIN_W - MARGIN - 18 + SHADOW_PAD);
        int ry = Scale(TITLE_H + TAB_H + 12) + sp;
        g_diskRefreshRect = {rx - bw, ry, rx, ry + bh};
        // 复制按钮在刷新按钮左边（间距 10 像素）
        int cw = Scale(88);
        g_copyBtnRect = {rx - bw - Scale(10) - cw, ry, rx - bw - Scale(10), ry + bh};

        baseH = TITLE_H + TAB_H + DISK_PAGE_HEAD + DISK_TABLE_HEAD
                + rows * DISK_ROW_H + FOOTER_H;
        if (rows > 0) baseH += DISK_DETAIL_H;   // 无分区时不预留空的详情区高度
    }

    // 内容总高与窗口客户区尺寸：均先求和基准值再整体 Scale 一次
    g_totalH = Scale(baseH);
    g_clientW = Scale(WIN_W + 2 * SHADOW_PAD);
    g_clientH = Scale(baseH + 2 * SHADOW_PAD);

    g_minBtnRect  = {Scale(WIN_W + SHADOW_PAD - 2 * TB_BTN_W), Scale(SHADOW_PAD),
                     Scale(WIN_W + SHADOW_PAD - TB_BTN_W),      Scale(TITLE_H + SHADOW_PAD)};
    g_closeBtnRect = {Scale(WIN_W + SHADOW_PAD - TB_BTN_W), Scale(SHADOW_PAD),
                      Scale(WIN_W + SHADOW_PAD),               Scale(TITLE_H + SHADOW_PAD)};
}

// ============================================================
// 窗口过程
// ============================================================

static LRESULT CALLBACK DialogProc(HWND hWnd, UINT uMsg,
                                   WPARAM wParam, LPARAM lParam) {
    switch (uMsg) {
    case WM_CREATE: {
        GetOSDisplayString(g_osString, _countof(g_osString));
        // 真实应用图标（LoadIcon 共享句柄，无需 DestroyIcon）
        g_hAppIcon = LoadIconW(((LPCREATESTRUCTW)lParam)->hInstance,
                               MAKEINTRESOURCE(IDI_APP_ICON));
        MigrateCleanupLegacy();  // v1.0.7 改名迁移：清理旧版 RightMenuX 残留（静默、失败不阻塞）
        HW_Collect(&g_hw);     // 硬件页数据（驱动未装也能识别；耗时 <300ms 实测）
        g_hwCollected = TRUE;
        EnumDisks();        // 磁盘信息页数据（默认停在硬件页，切页时会再刷新一次）
        ComputeLayout();

        // 纯 WS_POPUP 无系统边框：客户区即窗口区。窗口尺寸在内容尺寸之外
        // 每边多留 SHADOW_PAD，用于纯自绘柔和投影（不再依赖 DWM 阴影）。
        // 此处先用 96-DPI 基准尺寸占位；真正的 DPI 尺寸在 CreateWindow 返回后
        // 由 wWinMain 取窗口实际 DPI 重算并 SetWindowPos。
        SetWindowPos(hWnd, NULL, 0, 0, g_clientW, g_clientH,
                     SWP_NOMOVE | SWP_NOZORDER | SWP_FRAMECHANGED);
        ApplyRoundedRegion(hWnd, g_clientW, g_clientH);
        CenterWindow(hWnd);

        // Win11 DWM 圆角（Win10 上忽略失败；与上面的 SetWindowRgn 互为冗余但无害）
        HMODULE hDwm = LoadLibraryW(L"dwmapi.dll");
        if (hDwm) {
            typedef HRESULT (WINAPI *DwmSetWindowAttribute_t)(HWND, DWORD, LPCVOID, DWORD);
            DwmSetWindowAttribute_t p = (DwmSetWindowAttribute_t)GetProcAddress(hDwm, "DwmSetWindowAttribute");
            if (p) {
                DWORD corner = DWMWCP_ROUND;
                p(hWnd, DWMWA_WINDOW_CORNER_PREFERENCE, &corner, sizeof(corner));
            }
            FreeLibrary(hDwm);
        }
        return 0;
    }

    // 纯 WS_POPUP 无系统非客户区，非客户区本就为零，无需 WM_NCCALCSIZE。
    // （旧版用 WS_THICKFRAME + 清零非客户区去白边并保 DWM 阴影，
    //   但清零后 DWM 阴影反而不可见；现改为纯自绘投影，故移除该处理。）

    // 禁止系统擦除背景：全窗内容都是自绘的，擦背景只会让每次重绘多闪一次白。
    case WM_ERASEBKGND:
        return 1;

    case WM_PAINT: {
        PAINTSTRUCT ps;
        HDC hdcReal = BeginPaint(hWnd, &ps);
        RECT cr; GetClientRect(hWnd, &cr);
        int W = cr.right - cr.left, H = cr.bottom - cr.top;

        // ====== 双缓冲（v1.0.9：修复 hover 时窗口闪烁）======
        // 界面全部自绘，且 hover 变化就整窗重绘；若直接画到窗口 DC，
        // 背景/卡片/文字的逐笔绘制过程会被逐帧呈现出来 —— 这就是"闪烁"的根因。
        // 改为先画进内存位图，最后一次 BitBlt 整体上屏。
        HDC     mem   = CreateCompatibleDC(hdcReal);
        HBITMAP bmp   = NULL;
        HBITMAP oldB  = NULL;
        if (mem) {
            BITMAPINFO bmi = {0};
            bmi.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
            bmi.bmiHeader.biWidth = W;
            bmi.bmiHeader.biHeight = -H;   // 负值 = top-down，与 GDI+ 坐标一致
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB;
            void* bits = NULL;
            bmp = CreateDIBSection(hdcReal, &bmi, DIB_RGB_COLORS, &bits, NULL, 0);
            oldB = (HBITMAP)SelectObject(mem, bmp);
        }
        HDC hdc = (mem && bmp) ? mem : hdcReal;

        // GDI+ 渲染上下文（玻璃效果需要 alpha 合成 / 抗锯齿 / 软阴影模糊）
        Graphics g(hdc);
        g.SetSmoothingMode(SmoothingModeAntiAlias);
        g.SetTextRenderingHint(TextRenderingHintAntiAliasGridFit);
        g.SetInterpolationMode(InterpolationModeHighQualityBicubic);
        g_pg = &g;

        // 窗口层：紫→蓝平滑渐变（GDI+ LinearGradientBrush）
        DrawWindowFrame(hdc, W, H);

        int sp = Scale(SHADOW_PAD);   // 本帧统一使用的留白像素

        // 玻璃风：不再实色铺底，让紫色→蓝色渐变透出（卡片/面板另垫白底）

        // glass4 风格：顶部磨砂标题栏带（极低透白，紫蓝渐变透出）
        if (g_pg) {
            int ty = sp, bandH = Scale(TITLE_H + SHADOW_PAD) - sp;
            LinearGradientBrush tbr(RectF((REAL)sp, (REAL)ty, (REAL)(W - 2 * sp), (REAL)bandH),
                Color(40, 255, 255, 255), Color(16, 255, 255, 255), LinearGradientModeVertical);
            g_pg->FillRectangle(&tbr, (REAL)sp, (REAL)ty, (REAL)(W - 2 * sp), (REAL)bandH);
            g_pg->Flush(FlushIntentionSync);
        }

        // ====== 自定义标题栏 ======
        {
            // 玻璃风：标题栏并入渐变，不单独铺色

            // 标题栏按钮（圆角，hover 高亮）
            DrawTitleButton(hdc, &g_minBtnRect, g_minHover, FALSE);
            DrawTitleButton(hdc, &g_closeBtnRect, g_closeHover, TRUE);

            // 底部分隔线（极细，渐变上用浅色更协调）
            HPEN spn = CreatePen(PS_SOLID, 1, C_TXT_LIGHT_DIM);
            HPEN osp = (HPEN)SelectObject(hdc, spn);
            MoveToEx(hdc, sp, Scale(TITLE_H + SHADOW_PAD), NULL);
            LineTo(hdc, Scale(WIN_W + SHADOW_PAD), Scale(TITLE_H + SHADOW_PAD));
            SelectObject(hdc, osp);
            DeleteObject(spn);

            // 左侧真实应用图标（失败则回退为品牌色方块）
            int iSz = Scale(22);
            int ix = Scale(16) + sp, iy = sp + (Scale(TITLE_H) - iSz) / 2;
            if (g_hAppIcon) {
                DrawIconEx(hdc, ix, iy, g_hAppIcon, iSz, iSz, 0, NULL, DI_NORMAL);
            } else {
                RECT ir = {ix, iy, ix + iSz, iy + iSz};
                DrawRoundedRect(hdc, &ir, Scale(5), C_PRIMARY, C_PRIMARY);
            }

            // 标题文字（与图标同高，垂直居中）
            HFONT tf = MakeFont(Scale(14), FW_SEMIBOLD);
            HFONT of = (HFONT)SelectObject(hdc, tf);
            SetTextColor(hdc, C_TXT_LIGHT);
            SetBkMode(hdc, TRANSPARENT);
            RECT tR = {ix + iSz + Scale(10), sp,
                       Scale(WIN_W + SHADOW_PAD - 2 * TB_BTN_W), Scale(TITLE_H + SHADOW_PAD)};
            DrawTextW(hdc, L"WinSuiteX", -1, &tR,
                      DT_LEFT | DT_SINGLELINE | DT_VCENTER);
            SelectObject(hdc, of);
            DeleteObject(tf);

            // 窗口控件字形由 DrawTitleButton 绘制（见上方调用）
        }

        // ====== 标签页（两页共用：右键增强 / 磁盘信息） ======
        DrawTabs(hdc);

        // ====== 导语区（替代重复大标题，提供信息层次；仅右键增强页） ======
        if (g_curTab == TAB_MENU) {
            int heroY = Scale(TITLE_H + TAB_H + 20 + SHADOW_PAD);
            HFONT heroF = MakeFont(Scale(16), FW_SEMIBOLD);
            HFONT of = (HFONT)SelectObject(hdc, heroF);
            SetTextColor(hdc, C_TXT_LIGHT);
            SetBkMode(hdc, TRANSPARENT);
            RECT hR = {Scale(MARGIN + SHADOW_PAD), heroY,
                       Scale(WIN_W - MARGIN + SHADOW_PAD), heroY + Scale(26)};
            DrawTextW(hdc, L"\u53f3\u952e\u83dc\u5355\u589e\u5f3a\u5de5\u5177", -1, &hR,
                      DT_LEFT | DT_SINGLELINE | DT_VCENTER);
            SelectObject(hdc, of);
            DeleteObject(heroF);

            // 品牌色短装饰线
            HPEN ap = CreatePen(PS_SOLID, Scale(3), C_PRIMARY);
            HPEN aop = (HPEN)SelectObject(hdc, ap);
            int lineY = Scale(TITLE_H + TAB_H + 20 + 26 + SHADOW_PAD);
            MoveToEx(hdc, Scale(MARGIN + SHADOW_PAD), lineY, NULL);
            LineTo(hdc, Scale(MARGIN + 28 + SHADOW_PAD), lineY);
            SelectObject(hdc, aop);
            DeleteObject(ap);

            // OS 信息副标题
            int subY = Scale(TITLE_H + TAB_H + 50 + SHADOW_PAD);
            HFONT subF = MakeFont(Scale(13), FW_NORMAL);
            HFONT osf = (HFONT)SelectObject(hdc, subF);
            SetTextColor(hdc, C_TXT_LIGHT_SUB);
            SetBkMode(hdc, TRANSPARENT);
            RECT sR = {Scale(MARGIN + SHADOW_PAD), subY,
                       Scale(WIN_W - MARGIN + SHADOW_PAD), subY + Scale(22)};
            DrawTextW(hdc, g_osString, -1, &sR, DT_LEFT | DT_SINGLELINE | DT_VCENTER);
            SelectObject(hdc, osf);
            DeleteObject(subF);

            // 经典右键菜单开关（标题区右侧紧凑卡片）
            DrawCard(hdc, &g_classicMenuRect, g_classicMenuHover, C_PRIMARY);
            int box = Scale(18);
            int rowH = g_classicMenuRect.bottom - g_classicMenuRect.top;
            RECT cbr = {g_classicMenuRect.right - box - Scale(10),
                        g_classicMenuRect.top + (rowH - box) / 2,
                        g_classicMenuRect.right - Scale(10),
                        g_classicMenuRect.top + (rowH - box) / 2 + box};
            BOOL on = IsClassicMenuEnabled();
            DrawCheckbox(hdc, &cbr, on, g_classicMenuHover);
            int tx = g_classicMenuRect.left + Scale(10);
            {
                RECT nR = {tx, g_classicMenuRect.top + Scale(7),
                           cbr.left - Scale(8), g_classicMenuRect.top + Scale(7) + Scale(18)};
                DrawCardText(hdc, L"\u7ecf\u5178\u83dc\u5355", &nR, Scale(13), FW_SEMIBOLD, RGB(255, 255, 255),
                             DT_LEFT | DT_SINGLELINE | DT_VCENTER);
            }
            // 状态文案：Win10/更早右键菜单本就是经典样式（该 CLSID 键不存在也不生效），
            // 显示"系统默认"更准确；Win11 才显示"已启用/未启用"。
            const WCHAR* statusText;
            if (!IsWindows11()) statusText = L"\u7cfb\u7edf\u9ed8\u8ba4";
            else                statusText = on ? L"\u5df2\u542f\u7528" : L"\u672a\u542f\u7528";
            {
                RECT dR = {tx, g_classicMenuRect.top + Scale(24),
                           cbr.left - Scale(8), g_classicMenuRect.top + Scale(24) + Scale(16)};
                DrawCardText(hdc, statusText, &dR, Scale(11), FW_NORMAL, on ? RGB(150, 240, 190) : RGB(205, 210, 230),
                             DT_LEFT | DT_SINGLELINE | DT_VCENTER);
            }
        }

        // 导语区与卡片列表之间的分隔线（仅右键增强页）
        if (g_curTab == TAB_MENU) {
            HPEN line = CreatePen(PS_SOLID, 1, C_TXT_LIGHT_DIM);
            HPEN ol = (HPEN)SelectObject(hdc, line);
            int divY = Scale(TITLE_H + TAB_H + HEADER_H - 16 + SHADOW_PAD);
            MoveToEx(hdc, Scale(MARGIN + SHADOW_PAD), divY, NULL);
            LineTo(hdc, Scale(WIN_W - MARGIN + SHADOW_PAD), divY);
            SelectObject(hdc, ol);
            DeleteObject(line);
        }

        // ====== 卡片列表 ======
        for (int i = 0; i < g_count; i++) {
            const Feature* f = &g_features[i];
            BOOL installed = Feature_IsInstalled(f);
            COLORREF accent = (f->kind == FeatureKind::RunCommand) ? C_GREEN : C_AMBER;

            DrawCard(hdc, &g_cardRect[i], g_hover[i], accent);

            // 图标瓦片（彩色圆角方块 + 白色线稿）
            int iconSz = Scale(46);
            int cardH = g_cardRect[i].bottom - g_cardRect[i].top;
            int iyTop = g_cardRect[i].top + (cardH - iconSz) / 2;
            int ixL  = g_cardRect[i].left + Scale(18);
            RECT ir = {ixL, iyTop, ixL + iconSz, iyTop + iconSz};
            DrawAccentTile(hdc, &ir, accent);
            DrawFeatureIcon(hdc, &ir, f->id, accent);

            // 文字起始 X
            int tx = g_cardRect[i].left + Scale(18) + iconSz + Scale(16);

            // 功能名称
            {
                RECT nR = {tx, g_cardRect[i].top + Scale(18),
                           g_btnRect[i].left - Scale(14),
                           g_cardRect[i].top + Scale(18) + Scale(24)};
                DrawCardText(hdc, f->displayName, &nR, Scale(16), FW_SEMIBOLD, RGB(255, 255, 255),
                             DT_LEFT | DT_SINGLELINE | DT_VCENTER);
            }

            // 说明文字（两行完整显示，超长才省略，不再单行截断）
            {
                RECT dR = {tx, g_cardRect[i].top + Scale(42),
                           g_btnRect[i].left - Scale(14),
                           g_cardRect[i].top + Scale(42) + Scale(40)};
                DrawCardText(hdc, f->description, &dR, Scale(15), FW_NORMAL, RGB(220, 228, 245),
                             DT_LEFT | DT_WORDBREAK | DT_END_ELLIPSIS | DT_VCENTER);
            }

            // 状态徽章（带指示圆点，卡片底部一行）
            {
                int bW = Scale(76), bH = Scale(22);
                RECT stR = {tx, g_cardRect[i].top + Scale(88),
                            tx + bW, g_cardRect[i].top + Scale(88) + bH};
                DrawStatusBadge(hdc, &stR, installed);
            }

            // 操作按钮（卡片内右侧垂直居中）
            DrawButton(hdc, &g_btnRect[i],
                       installed ? L"\u505c\u7528" : L"\u542f\u7528",
                       g_hover[i], !installed);
        }

        // ====== 磁盘信息页（与右键增强页互斥） ======
        if (g_curTab == TAB_DISK) {
            // 玻璃风：磁盘页整页垫白磨砂面板，深色表格文字保持可读（背景为渐变）
            RECT dp = {Scale(MARGIN + SHADOW_PAD),
                       Scale(TITLE_H + TAB_H + 8 + SHADOW_PAD),
                       Scale(WIN_W - MARGIN + SHADOW_PAD),
                       g_totalH - Scale(FOOTER_H) + sp};
            DrawGlassPanel(hdc, &dp, Scale(WIN_RADIUS));
            DrawDiskPage(hdc, sp);
        }

        // ====== 硬件配置页（默认页） ======
        if (g_curTab == TAB_HW) {
            // 玻璃风：硬件页整页垫白磨砂面板，深色"标签：值"文字保持可读（背景为渐变）
            RECT hp = {Scale(MARGIN + SHADOW_PAD),
                       Scale(TITLE_H + TAB_H + 8 + SHADOW_PAD),
                       Scale(WIN_W - MARGIN + SHADOW_PAD),
                       g_totalH - Scale(FOOTER_H) + sp};
            DrawGlassPanel(hdc, &hp, Scale(WIN_RADIUS));
            DrawHardwarePage(hdc, sp);
        }

        // ====== 页脚（两行：提示略大 + 版权信息） ======
        {
            int zoneTop = g_totalH - Scale(FOOTER_H) + sp;
            int zoneH = Scale(FOOTER_H);
            int cyMid = zoneTop + zoneH / 2;
            // P0 修复：页脚浅色字在蓝渐变上对比度不足(WCAG AA 未达 4.5:1)
            //         → 在页脚区加半透暗色磨砂底条，保证浅字可读
            if (g_pg) {
                GraphicsPath fpp;
                RoundRectPath(&fpp, (float)Scale(MARGIN + SHADOW_PAD), (float)zoneTop,
                              (float)(Scale(WIN_W - 2 * MARGIN)), (float)zoneH, (float)Scale(10));
                SolidBrush fbr(Color(150, 18, 22, 42));
                g_pg->FillPath(&fbr, &fpp);
                g_pg->Flush(FlushIntentionSync);
            }
            // 提示行（字号略增：11 -> 13）
            HFONT footF = MakeFont(Scale(14), FW_NORMAL);
            HFONT of = (HFONT)SelectObject(hdc, footF);
            SetTextColor(hdc, C_TXT_LIGHT_SUB);
            SetBkMode(hdc, TRANSPARENT);
            RECT l1 = {Scale(MARGIN + SHADOW_PAD), cyMid - Scale(18),
                       Scale(WIN_W - MARGIN + SHADOW_PAD), cyMid};
            const WCHAR* footTip;
            if (g_curTab == TAB_MENU)
                footTip = L"\u53f3\u952e\u9879\u5373\u65f6\u751f\u6548\uff0c\u7ecf\u5178\u83dc\u5355\u5207\u6362\u9700\u91cd\u542f\u8d44\u6e90\u7ba1\u7406\u5668";
            else if (g_curTab == TAB_HW)
                footTip = L"\u9875\u9762\u4ec5\u5c55\u793a\u5e38\u89c4\u4fe1\u606f\uff0c\u5b8c\u6574\u660e\u7ec6\u8bf7\u70b9\u51fb\u53f3\u4e0a\u89d2\u300c\u5bfc\u51fa\u62a5\u544a\u300d";
            else
                footTip = L"\u9009\u4e2d\u884c\u67e5\u770b\u8be6\u60c5\uff0c\u63d2\u5165\u65b0\u76d8\u540e\u53ef\u70b9\u5237\u65b0";
            DrawTextW(hdc, footTip, -1, &l1, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
            SelectObject(hdc, of);
            DeleteObject(footF);

            // 版权信息行（与 app.rc 的 CompanyName / LegalCopyright 保持一致）
            HFONT copyF = MakeFont(Scale(13), FW_NORMAL);
            HFONT of2 = (HFONT)SelectObject(hdc, copyF);
            SetTextColor(hdc, RGB(188, 194, 220));   // 页脚版权行：在暗色底条上提亮，保 WCAG AA 对比
            SetBkMode(hdc, TRANSPARENT);
            RECT l2 = {Scale(MARGIN + SHADOW_PAD), cyMid + Scale(1),
                       Scale(WIN_W - MARGIN + SHADOW_PAD), cyMid + Scale(19)};
            WCHAR verBuf[64] = {0};
            LoadStringW(g_hInst, IDS_VERSION, verBuf, _countof(verBuf));
            WCHAR copyLine[128];
            swprintf(copyLine, _countof(copyLine),
                // 注意：宽字符 printf 中必须用 %ls。MinGW 按 C99 语义把 %s 当窄字符串，
                // 宽串 L"1.0.2..." 的首字符高位 0x00 被误判为结尾 → 界面只显示 "v1"。
                L"\u00a9 \u91cd\u5e86\u6301\u739b\u591a\u7f51\u7edc\u79d1\u6280\u6709\u9650\u516c\u53f8 \u00b7 Z.W.  v%ls", verBuf);
            DrawTextW(hdc, copyLine, -1, &l2, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
            SelectObject(hdc, of2);
            DeleteObject(copyF);
        }

        // 窗口 1px 淡白圆角外边框（最后描边；glass4 风格：半透白，柔和融入渐变）
        {
            if (g_pg) {
                Pen wp(Color(130, 255, 255, 255), 1.0f);
                GraphicsPath wpp;
                RoundRectPath(&wpp, (float)sp + 0.5f, (float)sp + 0.5f,
                              (float)(W - 2 * sp - 1), (float)(H - 2 * sp - 1), (float)Scale(WIN_RADIUS));
                g_pg->DrawPath(&wp, &wpp);
                g_pg->Flush(FlushIntentionSync);
            } else {
                RECT wf = {sp, sp, W - sp, H - sp};
                StrokeRoundedRect(hdc, &wf, Scale(WIN_RADIUS), C_GLASS_BORDER, 1);
            }
        }

        // 双缓冲收尾：必须先 BitBlt 再 EndPaint —— EndPaint 之后 BeginPaint 返回的
        // DC 已被释放，再画到 hdcReal 上不会出现在屏幕上。v1.0.9 的真机 bug 根因。
        if (mem && bmp) {
            BitBlt(hdcReal, 0, 0, W, H, mem, 0, 0, SRCCOPY);
            SelectObject(mem, oldB);
            DeleteObject(bmp);
        }
        if (mem) DeleteDC(mem);
        g_pg = NULL;
        EndPaint(hWnd, &ps);
        return 0;
    }

    case WM_NCHITTEST: {
        POINT pt = {GET_X_LPARAM(lParam), GET_Y_LPARAM(lParam)};
        ScreenToClient(hWnd, &pt);
        RECT rc; GetClientRect(hWnd, &rc);
        if (pt.x < 0 || pt.x > rc.right || pt.y < 0 || pt.y > rc.bottom)
            return HTNOWHERE;

        // 固定尺寸工具窗口，禁止任何缩放。
        // 仅标题栏非按钮区域返回 HTCAPTION 用于拖拽，其余一律 HTCLIENT。
        // 阈值与 g_minBtnRect/g_closeBtnRect 的底边一致（均为 Scale(TITLE_H + SHADOW_PAD)）。
        if (pt.y < Scale(TITLE_H + SHADOW_PAD)) {
            if (PtInRect(&g_closeBtnRect, pt) || PtInRect(&g_minBtnRect, pt))
                return HTCLIENT;
            return HTCAPTION;
        }
        return HTCLIENT;
    }

    case WM_MOUSEMOVE: {
        POINT pt = {LOWORD(lParam), HIWORD(lParam)};
        BOOL changed = FALSE;
        for (int i = 0; i < g_count; i++) {
            // 玻璃风：悬停检测用整张卡的矩形（demo 风格），），不是仅按钮——
            // 让阴影"浮起"+ 边框"亮起"在整张卡都生效（Z.W. 反馈缺动感）
            BOOL h = PtInRect(&g_cardRect[i], pt);
            if (h != g_hover[i]) { g_hover[i] = h; changed = TRUE; }
        }
        // 页签 hover
        for (int t = 0; t < TAB_COUNT; t++) {
            int h = PtInRect(&g_tabRects[t], pt) ? 1 : 0;
            if (h != g_tabHover[t]) { g_tabHover[t] = h; changed = TRUE; }
        }
        // 右键增强页：经典菜单复选框 hover
        if (g_curTab == TAB_MENU) {
            BOOL ch = PtInRect(&g_classicMenuRect, pt);
            if (ch != g_classicMenuHover) { g_classicMenuHover = ch; changed = TRUE; }
        }
        // 磁盘页：行 hover + 刷新按钮 hover
        if (g_curTab == TAB_DISK) {
            int rows = (g_diskCount < DISK_MAX_ROWS) ? g_diskCount : DISK_MAX_ROWS;
            int hr = -1;
            for (int i = 0; i < rows; i++) {
                if (PtInRect(&g_diskRowRect[i], pt)) { hr = i; break; }
            }
            if (hr != g_hoverDiskRow) { g_hoverDiskRow = hr; changed = TRUE; }
            BOOL rh = PtInRect(&g_diskRefreshRect, pt);
            if (rh != g_hoverDiskRefresh) { g_hoverDiskRefresh = rh; changed = TRUE; }
            // 磁盘页也有「一键复制」按钮，需更新 hover
            BOOL ch = PtInRect(&g_copyBtnRect, pt);
            if (ch != g_copyBtnHover) { g_copyBtnHover = ch; changed = TRUE; }
        }
        // 硬件页：一键复制按钮 hover
        else if (g_curTab == TAB_HW) {
            BOOL ch = PtInRect(&g_copyBtnRect, pt);
            if (ch != g_copyBtnHover) { g_copyBtnHover = ch; changed = TRUE; }
        }
        BOOL minH = PtInRect(&g_minBtnRect, pt);
        if (minH != g_minHover) { g_minHover = minH; changed = TRUE; }
        BOOL closeH = PtInRect(&g_closeBtnRect, pt);
        if (closeH != g_closeHover) { g_closeHover = closeH; changed = TRUE; }
        if (changed) InvalidateRect(hWnd, NULL, FALSE);
        // 只注册一次：每次 WM_MOUSEMOVE 都调 TrackMouseEvent 会反复重置跟踪，
        // 极端情况下漏掉 WM_MOUSELEAVE（鼠标移出窗口后按钮仍保持高亮）。
        if (!g_mouseTracking) {
            TRACKMOUSEEVENT tme = {sizeof(tme), TME_LEAVE, hWnd, 0};
            if (TrackMouseEvent(&tme)) g_mouseTracking = TRUE;
        }
        return 0;
    }

    case WM_MOUSELEAVE: {
        g_mouseTracking = FALSE;         // 跟踪已消费，下次进入窗口时重新注册
        BOOL changed = FALSE;
        for (int i = 0; i < g_count; i++) {
            if (g_hover[i]) { g_hover[i] = FALSE; changed = TRUE; }
        }
        for (int t = 0; t < TAB_COUNT; t++) {
            if (g_tabHover[t]) { g_tabHover[t] = 0; changed = TRUE; }
        }
        if (g_classicMenuHover) { g_classicMenuHover = FALSE; changed = TRUE; }
        if (g_hoverDiskRow != -1) { g_hoverDiskRow = -1; changed = TRUE; }
        if (g_hoverDiskRefresh)   { g_hoverDiskRefresh = FALSE; changed = TRUE; }
        if (g_copyBtnHover)        { g_copyBtnHover = FALSE; changed = TRUE; }
        if (g_minHover) { g_minHover = FALSE; changed = TRUE; }
        if (g_closeHover) { g_closeHover = FALSE; changed = TRUE; }
        if (changed) InvalidateRect(hWnd, NULL, FALSE);
        return 0;
    }

    case WM_LBUTTONDOWN: {
        POINT pt = {LOWORD(lParam), HIWORD(lParam)};
        if (PtInRect(&g_closeBtnRect, pt)) {
            PostMessageW(hWnd, WM_CLOSE, 0, 0);
            return 0;
        }
        if (PtInRect(&g_minBtnRect, pt)) {
            ShowWindow(hWnd, SW_MINIMIZE);
            return 0;
        }
        // 页签切换（页签在页面内容之上，优先命中）
        for (int t = 0; t < TAB_COUNT; t++) {
            if (PtInRect(&g_tabRects[t], pt)) { SwitchTab(hWnd, t); return 0; }
        }
        // 磁盘信息页：刷新按钮 / 行选择 / 一键复制
        if (g_curTab == TAB_DISK) {
            if (PtInRect(&g_diskRefreshRect, pt)) {
                EnumDisks();
                RelayoutWindow(hWnd);   // 分区数可能变化（插拔盘），需重排窗口
                return 0;
            }
            if (PtInRect(&g_copyBtnRect, pt)) {
                WCHAR buf[8192];   // 磁盘页文本较硬件页短（无 BIOS/CPU 长字段），8KB 足矣
                Disk_BuildText(g_disks, g_diskCount, g_selDisk, buf, _countof(buf));
                if (CopyToClipboardW(buf)) {
                    g_copyAtTick = GetTickCount();
                    InvalidateRect(hWnd, NULL, FALSE);
                    SetTimer(hWnd, 1, 1300, NULL);
                }
                return 0;
            }
            int rows = (g_diskCount < DISK_MAX_ROWS) ? g_diskCount : DISK_MAX_ROWS;
            for (int i = 0; i < rows; i++) {
                if (PtInRect(&g_diskRowRect[i], pt)) {
                    if (g_selDisk != i) { g_selDisk = i; InvalidateRect(hWnd, NULL, FALSE); }
                    return 0;
                }
            }
            return 0;
        }
        // 硬件配置页：导出 HTML 报告到桌面（v1.0.9 取代原"一键复制"）
        if (g_curTab == TAB_HW) {
            if (PtInRect(&g_copyBtnRect, pt)) {
                g_copyAtTick = GetTickCount();       // 立刻给"已导出"反馈（卡顿时也可见）
                InvalidateRect(hWnd, NULL, FALSE);
                ExportReport(hWnd);                  // 写文件 -> 打开 -> 提示（内部自行处理失败）
                SetTimer(hWnd, 1, 1300, NULL);       // 1.3 秒后清除"已导出"反馈
                return 0;
            }
            return 0;
        }
        for (int i = 0; i < g_count; i++) {
            if (PtInRect(&g_btnRect[i], pt)) {
                const Feature* f = &g_features[i];
                BOOL installed = Feature_IsInstalled(f);

                // —— 右键菜单类（RunCommand / ShellExtension） ——
                if (installed) {
                    BOOL a = Feature_Uninstall(f, HKEY_CLASSES_ROOT);
                    BOOL b = Feature_Uninstall(f, HKEY_CURRENT_USER);
                    if (a || b) {
                        ShowNotice(hWnd,
                            L"WinSuiteX",
                            L"\u5df2\u505c\u7528\uff0c\u53f3\u952e\u83dc\u5355\u9879\u5df2\u79fb\u9664\u3002",
                            FALSE);
                    } else {
                        ShowNotice(hWnd,
                            L"WinSuiteX",
                            L"\u505c\u7528\u5931\u8d25\uff1a\u6ce8\u518c\u8868\u5220\u9664\u88ab\u62d2\u7edd\u3002\n"
                            L"\u8bf7\u53f3\u952e\u201c\u4ee5\u7ba1\u7406\u5458\u8eab\u4efd\u8fd0\u884c\u201d\u672c\u7a0b\u5e8f\u540e\u91cd\u8bd5\u3002",
                            TRUE);
                    }
                } else {
                    BOOL ok = Feature_Install(f, HKEY_CLASSES_ROOT);
                    if (ok) {
                        ShowNotice(hWnd,
                            L"WinSuiteX",
                            L"\u5df2\u542f\u7528\uff08\u5bf9\u672c\u673a\u6240\u6709\u7528\u6237\u751f\u6548\uff09"
                            L"\uff0c\u5728\u201c\u6b64\u7535\u8111\u201d\u53f3\u952e\u83dc\u5355\u4e2d\u5373\u53ef\u770b\u5230\u3002",
                            FALSE);
                    } else if (Feature_Install(f, HKEY_CURRENT_USER)) {
                        ShowNotice(hWnd,
                            L"WinSuiteX",
                            L"\u5df2\u4e3a\u5f53\u524d\u7528\u6237\u542f\u7528"
                            L"\uff08\u672a\u83b7\u7ba1\u7406\u5458\u6743\u9650\uff0c\u4ec5\u5f53\u524d\u8d26\u6237\u751f\u6548\uff09"
                            L"\u3002\n\u5982\u9700\u5bf9\u6240\u6709\u7528\u6237\u751f\u6548\uff0c"
                            L"\u8bf7\u53f3\u952e\u201c\u4ee5\u7ba1\u7406\u5458\u8eab\u4ef5\u8fd0\u884c\u201d\u672c\u7a0b\u5e8f\u3002",
                            FALSE);
                    } else {
                        ShowNotice(hWnd,
                            L"WinSuiteX",
                            L"\u542f\u7528\u5931\u8d25\uff1a\u6ce8\u518c\u8868\u5199\u5165\u88ab\u62d2\u7edd\u3002\n"
                            L"\u8bf7\u53f3\u952e\u201c\u4ee5\u7ba1\u7406\u5458\u8eab\u4efd\u8fd0\u884c\u201d\u672c\u7a0b\u5e8f\u540e\u91cd\u8bd5\u3002",
                            TRUE);
                    }
                }
                InvalidateRect(hWnd, NULL, FALSE);
                break;
            }
        }

        // 经典右键菜单复选框：勾选=强制旧版菜单，取消=回到 Win11 新菜单（即时刷新）
        if (g_curTab == TAB_MENU && PtInRect(&g_classicMenuRect, pt)) {
            if (!IsWindows11()) {
                // Win10/更早：经典右键菜单本就是系统默认，{86ca1aa0} 键不存在也不生效，无需切换
                ShowNotice(hWnd, L"WinSuiteX",
                    L"\u5f53\u524d\u7cfb\u7edf\uff08Windows 10 \u53ca\u66f4\u65e9\uff09\u53f3\u952e\u83dc\u5355\u672c\u5c31\u662f\u7ecf\u5178\u6837\u5f0f\uff0c\n"
                    L"\u65e0\u9700\u5207\u6362\uff0c\u6b64\u9009\u9879\u4ec5\u9002\u7528\u4e8e Windows 11\u3002", FALSE);
            } else if (IsClassicMenuEnabled()) {
                DeleteRegTreeKey(HKEY_CURRENT_USER,  L"Software\\Classes\\CLSID\\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}");
                DeleteRegTreeKey(HKEY_CLASSES_ROOT, L"CLSID\\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}");
                ShowNotice(hWnd, L"WinSuiteX",
                    L"\u5df2\u5207\u56de Windows 11 \u65b0\u6837\u5f0f\u53f3\u952e\u83dc\u5355\u3002\n\uff08\u9700\u91cd\u542f\u8d44\u6e90\u7ba1\u7406\u5668\u540e\u5b8c\u5168\u751f\u6548\uff09", FALSE);
                NotifyShellChanged();
            } else {
                RegWriteString(HKEY_CURRENT_USER,
                    L"Software\\Classes\\CLSID\\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\\InprocServer32",
                    NULL, L"");
                ShowNotice(hWnd, L"WinSuiteX",
                    L"\u5df2\u542f\u7528\u7ecf\u5178\uff08\u65e7\u7248\uff09\u53f3\u952e\u83dc\u5355\u3002\n\uff08\u9700\u91cd\u542f\u8d44\u6e90\u7ba1\u7406\u5668\u540e\u5b8c\u5168\u751f\u6548\uff09", FALSE);
                NotifyShellChanged();
            }
            InvalidateRect(hWnd, NULL, FALSE);
        }

        return 0;
    }

    case WM_KEYDOWN:
        if (wParam == VK_ESCAPE) DestroyWindow(hWnd);
        return 0;

    // Per-Monitor V2：窗口跨监视器移动 / 系统缩放变更时收到。
    // wParam 高位 = 新 DPI；lParam = 建议窗口矩形（已按新 DPI 排好位置）。
    case WM_DPICHANGED: {
        g_dpi = (UINT)HIWORD(wParam);
        ComputeLayout();                       // 用新 DPI 重算所有坐标/尺寸
        ClearShadows();                        // DPI 变更 -> 旧尺寸阴影失效，清空重建
        RECT* pr = (RECT*)lParam;
        // 用建议矩形定位 + 我们计算的缩放尺寸（二者一致），并重建圆角 Region。
        // 字体为每次绘制临时创建（无全局 HFONT 缓存），无需重建，无句柄泄漏。
        SetWindowPos(hWnd, NULL, pr->left, pr->top, g_clientW, g_clientH,
                     SWP_NOZORDER | SWP_FRAMECHANGED);
        ApplyRoundedRegion(hWnd, g_clientW, g_clientH);
        InvalidateRect(hWnd, NULL, TRUE);     // 全窗重绘
        return 0;
    }

    // 一键复制反馈定时器：到点清状态并重绘（按钮从"已复制"恢复成"复制"）
    case WM_TIMER: {
        if (wParam == 1) {
            KillTimer(hWnd, 1);
            if (g_copyAtTick != 0) {
                g_copyAtTick = 0;
                InvalidateRect(hWnd, NULL, FALSE);
            }
        }
        return 0;
    }

    case WM_DESTROY:
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProc(hWnd, uMsg, wParam, lParam);
}

// ============================================================
// 入口
// ============================================================

int APIENTRY wWinMain(HINSTANCE hInstance, HINSTANCE, LPWSTR lpCmdLine, int nCmdShow) {
    g_hInst = hInstance;

    // GDI+ 初始化（玻璃渲染依赖；gdiplus.dll 为系统自带，仍属零额外依赖）
    GdiplusStartupInput gpsi;
    if (GdiplusStartup(&g_gdipToken, &gpsi, NULL) != Ok) g_gdipToken = 0;

    // 注册自绘弹窗类（供 ShowNotice 使用，含自检模式）
    WNDCLASSEXW nwc = {sizeof(nwc)};
    nwc.style = CS_HREDRAW | CS_VREDRAW | CS_DROPSHADOW;
    nwc.lpfnWndProc = NoticeProc;
    nwc.hInstance = hInstance;
    nwc.hCursor = LoadCursor(NULL, IDC_ARROW);
    nwc.hbrBackground = (HBRUSH)(COLOR_WINDOW + 1);
    nwc.lpszClassName = NOTICE_CLASS;
    RegisterClassExW(&nwc);

    // 自检模式
    if (lpCmdLine && wcsstr(lpCmdLine, L"--selftest")) {
        if (AllocConsole()) {
            freopen("CONOUT$", "w", stdout);
            freopen("CONOUT$", "w", stderr);
        }
        int rc = SelfTestMain();
        // --selftest-nogui：自动化/无头运行，跳过结尾弹窗直接退出（报告已写入文件）
        if (!wcsstr(lpCmdLine, L"--selftest-nogui")) {
            WCHAR msg[320];
            swprintf(msg, _countof(msg),
                     L"\u81ea\u68c0\u5b8c\u6210\n\u901a\u8fc7\uff1a%d    \u5931\u8d25\uff1a%d\n\n"
                     L"\u5b8c\u6574\u65e5\u5fd7\u89c1 WinSuiteX \u76ee\u5f55\u4e0b\u7684 selftest_report.txt",
                     SelfTestPass, SelfTestFail);
            ShowNotice(NULL, L"WinSuiteX \u81ea\u68c0", msg, rc != 0);
        }
        return rc;
    }

    // --report "<路径.html>"：无头导出 HTML 报告（批量巡检 / 自动化验证用）。
    // 不弹任何窗口，采集完成后直接写文件退出，可放进开机脚本或域脚本里跑。
    if (lpCmdLine && wcsstr(lpCmdLine, L"--report")) {
        const WCHAR* p = wcsstr(lpCmdLine, L"--report") + 8;
        while (*p == L' ' || *p == L'\t') p++;
        WCHAR path[1024] = {0};          // 允许长路径（UNC 共享目录等），不用 MAX_PATH
        if (*p == L'"') {
            p++;
            const WCHAR* e = wcschr(p, L'"');
            size_t n = e ? (size_t)(e - p) : wcslen(p);
            if (n >= _countof(path)) n = _countof(path) - 1;
            wcsncpy(path, p, n); path[n] = 0;
        } else {
            StringCchCopyW(path, _countof(path), p);
        }
        if (!path[0]) StringCchCopyW(path, _countof(path), L"WinSuiteX_report.html");

        HW_Collect(&g_hw);
        g_hwCollected = TRUE;
        EnumDisks();
        #define REPORT_MAX 40960
        WCHAR* rep = (WCHAR*)malloc(sizeof(WCHAR) * REPORT_MAX);
        BOOL ok = rep && Report_BuildHtml(rep, REPORT_MAX) && WriteUtf8File(path, rep);
        free(rep);
        #undef REPORT_MAX
        return ok ? 0 : 2;
    }

    WNDCLASSEXW wc = {sizeof(wc)};
    wc.style = CS_HREDRAW | CS_VREDRAW | CS_DROPSHADOW;
    wc.lpfnWndProc = DialogProc;
    wc.hInstance = hInstance;
    wc.hIcon = LoadIcon(hInstance, MAKEINTRESOURCE(IDI_APP_ICON));
    wc.hIconSm = wc.hIcon;
    wc.hCursor = LoadCursor(NULL, IDC_ARROW);
    wc.hbrBackground = (HBRUSH)(COLOR_WINDOW + 1);
    wc.lpszClassName = L"WinSuiteXDialog";

    if (!RegisterClassExW(&wc)) return 1;

    // 纯 WS_POPUP：彻底无系统尺寸边框、无白边、无缩放。
    // 阴影与 1px 边框全部由 WM_PAINT 纯自绘（见 DrawWindowFrame）。
    // 初始尺寸用 96-DPI 基准占位；创建后立即按窗口实际 DPI 重算（见下方）。
    HWND hWnd = CreateWindowExW(0, L"WinSuiteXDialog",
        L"WinSuiteX",
        WS_POPUP | WS_VISIBLE,
        CW_USEDEFAULT, CW_USEDEFAULT, WIN_W + 2 * SHADOW_PAD, 400,
        NULL, NULL, hInstance, NULL);
    if (!hWnd) return 1;

    // Per-Monitor V2：CreateWindow 后窗口已在目标监视器上，此刻取真实 DPI。
    // 若与初始 96 不同（如 125%/150% 主屏），按新 DPI 重算布局并重设尺寸 +
    // 重建圆角 Region + 重新居中；字体为绘制时临时创建，无需重建。
    {
        UINT realDpi = GetWindowDpi(hWnd);
        if (realDpi != g_dpi) {
            g_dpi = realDpi;
            ComputeLayout();
            SetWindowPos(hWnd, NULL, 0, 0, g_clientW, g_clientH,
                         SWP_NOMOVE | SWP_NOZORDER | SWP_FRAMECHANGED);
            ApplyRoundedRegion(hWnd, g_clientW, g_clientH);
            CenterWindow(hWnd);
            InvalidateRect(hWnd, NULL, TRUE);
        }
    }

    ShowWindow(hWnd, nCmdShow);
    UpdateWindow(hWnd);

    MSG msg;
    while (GetMessage(&msg, NULL, 0, 0)) {
        TranslateMessage(&msg);
        DispatchMessage(&msg);
    }
    if (g_gdipToken) GdiplusShutdown(g_gdipToken);
    if (g_fontFam) { delete g_fontFam; g_fontFam = NULL; }
    for (int _s = 0; _s < g_shadowCount; _s++) if (g_shadows[_s].bmp) { delete g_shadows[_s].bmp; g_shadows[_s].bmp = NULL; }
    g_shadowCount = 0;
    return (int)msg.wParam;
}
