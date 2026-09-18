#include "common.h"
#include "features.h"
#include "resource.h"   // IDR_SHELL_DLL

#include <cstdlib>
#include <cstring>
#include <shellapi.h>   // SHGetFileInfo / SHFILEINFO

// 把 HICON 写出为合法的 .ico 文件（未压缩 BMP 式，适配 <=48px 的菜单图标）
static BOOL WriteIcoFromHICON(HICON hIcon, LPCWSTR pszPath) {
    ICONINFO ii = {0};
    if (!GetIconInfo(hIcon, &ii)) return FALSE;

    // 防御：GetIconInfo 对部分图标可能返回 NULL 位图（如单色图标缺 hbmColor），
    // 此时无法构造 .ico，安全返回失败，避免对 NULL 句柄调用 GetObject 引发未定义行为。
    if (!ii.hbmColor || !ii.hbmMask) {
        if (ii.hbmColor) DeleteObject(ii.hbmColor);
        if (ii.hbmMask)  DeleteObject(ii.hbmMask);
        return FALSE;
    }
    BITMAP bc = {0}, bm = {0};
    GetObject(ii.hbmColor, sizeof(bc), &bc);
    GetObject(ii.hbmMask,  sizeof(bm),  &bm);

    int w  = (int)bc.bmWidth;
    int h  = (int)bc.bmHeight;
    int cb = (int)bc.bmBitsPixel;       // 颜色位深
    int colorStride = ((w * cb + 31) / 32) * 4;
    DWORD cbColor = (DWORD)(colorStride * h);

    // 颜色位图：自上而下（XOR 在 .ico 中为顶向下）
    BITMAPINFO bi = {0};
    bi.bmiHeader.biSize        = sizeof(BITMAPINFOHEADER);
    bi.bmiHeader.biWidth       = w;
    bi.bmiHeader.biHeight      = -h;
    bi.bmiHeader.biPlanes      = 1;
    bi.bmiHeader.biBitCount    = (WORD)cb;
    bi.bmiHeader.biCompression = BI_RGB;

    HDC hdc = GetDC(NULL);
    BYTE* colorBits = (BYTE*)malloc(cbColor ? cbColor : 1);
    GetDIBits(hdc, ii.hbmColor, 0, h, colorBits, &bi, DIB_RGB_COLORS);

    // 掩码位图：自下而上（AND 在 .ico 中为底向上）
    BITMAPINFO bim = {0};
    bim.bmiHeader.biSize        = sizeof(BITMAPINFOHEADER);
    bim.bmiHeader.biWidth       = w;
    bim.bmiHeader.biHeight      = h;     // 正向 -> 底向上
    bim.bmiHeader.biPlanes      = 1;
    bim.bmiHeader.biBitCount    = 1;
    bim.bmiHeader.biCompression = BI_RGB;
    int maskStride = ((w + 31) / 32) * 4;
    DWORD cbMask = (DWORD)(maskStride * h);
    BYTE* maskBits = (BYTE*)malloc(cbMask ? cbMask : 1);
    GetDIBits(hdc, ii.hbmMask, 0, h, maskBits, &bim, DIB_RGB_COLORS);
    ReleaseDC(NULL, hdc);

    // ICONIMAGE = BITMAPINFOHEADER(高度=2h) + XOR + AND
    BITMAPINFOHEADER ih = {0};
    ih.biSize        = sizeof(BITMAPINFOHEADER);
    ih.biWidth       = w;
    ih.biHeight      = h * 2;
    ih.biPlanes      = 1;
    ih.biBitCount    = (WORD)cb;
    ih.biCompression = BI_RGB;
    ih.biSizeImage   = cbColor + cbMask;

    DWORD cbImage = sizeof(BITMAPINFOHEADER) + cbColor + cbMask;
    BYTE* image = (BYTE*)malloc(cbImage ? cbImage : 1);
    memcpy(image, &ih, sizeof(BITMAPINFOHEADER));
    memcpy(image + sizeof(BITMAPINFOHEADER), colorBits, cbColor);
    memcpy(image + sizeof(BITMAPINFOHEADER) + cbColor, maskBits, cbMask);

    // ICONDIR + ICONDIRENTRY
    BYTE dir[6];
    *(WORD*)(dir + 0) = 0;   // reserved
    *(WORD*)(dir + 2) = 1;   // type = icon
    *(WORD*)(dir + 4) = 1;   // count
    BYTE entry[16];
    entry[0] = (w >= 256) ? 0 : (BYTE)w;
    entry[1] = (h >= 256) ? 0 : (BYTE)h;
    entry[2] = 0;            // color count (0 => >=8bpp)
    entry[3] = 0;
    *(WORD*)(entry + 4) = 1;
    *(WORD*)(entry + 6) = (WORD)cb;
    *(DWORD*)(entry + 8)  = cbImage;
    *(DWORD*)(entry + 12) = 6 + 16;

    HANDLE hf = CreateFileW(pszPath, GENERIC_WRITE, 0, NULL,
                            CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    BOOL ret = FALSE;
    if (hf != INVALID_HANDLE_VALUE) {
        DWORD wr;
        WriteFile(hf, dir, 6, &wr, NULL);
        WriteFile(hf, entry, 16, &wr, NULL);
        WriteFile(hf, image, cbImage, &wr, NULL);
        CloseHandle(hf);
        ret = TRUE;
    }
    free(colorBits); free(maskBits); free(image);
    DeleteObject(ii.hbmColor); DeleteObject(ii.hbmMask);
    return ret;
}

// 将文件名（如 compmgmt.msc）解析为完整路径：先展开环境变量，
// 含路径分隔符则直接校验存在性，否则在 PATH（含 System32）中查找。
BOOL ResolveSystemFile(LPCWSTR name, WCHAR* out, size_t n) {
    // 用较大缓冲展开环境变量，避免固定 MAX_PATH 缓冲在展开超长时被截断、
    // 且不保证以 null 结尾，导致后续 wcschr / GetFileAttributesW 越界读。
    // 若展开结果超过初始缓冲，则改用堆分配，确保始终以 null 结尾、不越界。
    WCHAR stack[MAX_PATH * 4];
    WCHAR* tmp = stack;
    DWORD cap = (DWORD)_countof(stack);
    DWORD need = ExpandEnvironmentStringsW(name, tmp, cap);
    if (need == 0) return FALSE;          // 展开失败
    if (need > cap) {
        // 超长（罕见）：堆分配精确大小再展开一次
        tmp = (WCHAR*)HeapAlloc(GetProcessHeap(), 0, (SIZE_T)need * sizeof(WCHAR));
        if (!tmp) return FALSE;
        ExpandEnvironmentStringsW(name, tmp, need);
        cap = need;
    }
    // 此时 tmp 必以 null 结尾
    BOOL result = FALSE;
    if (wcschr(tmp, L'\\') || wcschr(tmp, L'/')) {
        if (GetFileAttributesW(tmp) != INVALID_FILE_ATTRIBUTES) {
            StringCchCopyW(out, n, tmp);
            result = TRUE;
        }
    } else {
        if (SearchPathW(NULL, tmp, NULL, (DWORD)n, out, NULL) != 0) result = TRUE;
    }
    if (tmp != stack) HeapFree(GetProcessHeap(), 0, tmp);
    return result;
}

// 提取源文件（.msc 等）关联的系统图标，另存为 .ico
BOOL ExtractAndSaveIcon(LPCWSTR sourceFile, LPCWSTR outIco) {
    SHFILEINFO sfi = {0};
    DWORD r = SHGetFileInfoW(sourceFile, 0, &sfi, sizeof(sfi),
                             SHGFI_ICON | SHGFI_LARGEICON);
    if (r == 0 || sfi.hIcon == NULL) return FALSE;
    BOOL ok = WriteIcoFromHICON(sfi.hIcon, outIco);
    DestroyIcon(sfi.hIcon);
    return ok;
}

// 生成内置"终端"图标：深色方块 + 白色 ">" 提示符，强制不透明后写出为 .ico。
// 用内置图标而非抽取 cmd.exe：cmd.exe 的图标是透明底，资源管理器菜单里会渲染成
// 一片空白（"白板"）；内置图标底实色不透明，显示清晰可靠。
static HICON MakeTerminalIcon(int sz) {
    HDC hdc = GetDC(NULL);
    HDC mem = CreateCompatibleDC(hdc);

    // 颜色位图（32bpp，后续强制 alpha=255 保证不透明）
    BITMAPINFO bi = {0};
    bi.bmiHeader.biSize        = sizeof(BITMAPINFOHEADER);
    bi.bmiHeader.biWidth       = sz;
    bi.bmiHeader.biHeight      = -sz;   // 顶向下
    bi.bmiHeader.biPlanes      = 1;
    bi.bmiHeader.biBitCount    = 32;
    bi.bmiHeader.biCompression = BI_RGB;
    RGBQUAD* bits = NULL;
    HBITMAP hbmp = CreateDIBSection(mem, &bi, DIB_RGB_COLORS, (void**)&bits, NULL, 0);
    HBITMAP old = (HBITMAP)SelectObject(mem, hbmp);

    // 背景：终端黑
    HBRUSH bg = CreateSolidBrush(RGB(0x20, 0x22, 0x2C));
    HBRUSH ob = (HBRUSH)SelectObject(mem, bg);
    RECT rr = {0, 0, sz, sz};
    FillRect(mem, &rr, bg);
    SelectObject(mem, ob); DeleteObject(bg);
    // 内描边
    HPEN bp = CreatePen(PS_SOLID, 2, RGB(0x5A, 0x64, 0x78));
    HPEN obp = (HPEN)SelectObject(mem, bp);
    Rectangle(mem, 3, 3, sz - 3, sz - 3);
    SelectObject(mem, obp); DeleteObject(bp);
    // 白色 ">" 提示符 + 光标短横
    HPEN wp = CreatePen(PS_SOLID, 3, RGB(255, 255, 255));
    HPEN owp = (HPEN)SelectObject(mem, wp);
    int cx = sz / 2, cy = sz / 2;
    MoveToEx(mem, cx - 8, cy - 6, NULL); LineTo(mem, cx - 1, cy); LineTo(mem, cx - 8, cy + 6);
    MoveToEx(mem, cx + 1, cy + 6, NULL); LineTo(mem, cx + 7, cy + 6);
    SelectObject(mem, owp); DeleteObject(wp);
    SelectObject(mem, old);

    // 强制不透明（alpha=255）：避免 PNG 式透明导致菜单里"白板"
    for (int i = 0; i < sz * sz; i++) bits[i].rgbReserved = 255;

    // 1bpp 全不透明掩码（0 = 不透明）
    BITMAPINFO bmi = {0};
    bmi.bmiHeader.biSize        = sizeof(BITMAPINFOHEADER);
    bmi.bmiHeader.biWidth       = sz;
    bmi.bmiHeader.biHeight      = sz;
    bmi.bmiHeader.biPlanes      = 1;
    bmi.bmiHeader.biBitCount    = 1;
    bmi.bmiHeader.biCompression = BI_RGB;
    BYTE* mbits = NULL;
    HBITMAP hmask = CreateDIBSection(mem, &bmi, DIB_RGB_COLORS, (void**)&mbits, NULL, 0);
    if (mbits) memset(mbits, 0, ((sz + 31) / 32) * 4 * sz);

    // CreateIconIndirect 会拷贝位图，源位图须由调用方释放。用统一句柄 hMaskBmp 持有，
    // 无论来自 CreateDIBSection 还是兜底 CreateBitmap，都释放一次，避免兜底路径泄漏。
    HBITMAP hMaskBmp = hmask ? hmask : CreateBitmap(sz, sz, 1, 1, NULL);
    ICONINFO ii = {0};
    ii.fIcon    = TRUE;
    ii.hbmColor = hbmp;
    ii.hbmMask  = hMaskBmp;
    HICON hIcon = CreateIconIndirect(&ii);

    DeleteObject(hbmp);
    DeleteObject(hMaskBmp);
    DeleteDC(mem);
    ReleaseDC(NULL, hdc);
    return hIcon;
}

// 生成内置"眼睛"图标：蓝色实底 + 白色眼形轮廓 + 深色瞳孔，强制不透明后写出为 .ico。
// 用于 SuperHidden 等无源图标（menuIcon=NULL）的 ShellExtension 功能，
// 确保 Win11 新菜单项有图；若不写图标 Explorer 会显示空白。
// 结构与 MakeTerminalIcon 保持一致（GDI 失败则安全返回 NULL）。
static HICON MakeEyeIcon(int sz) {
    HDC hdc = GetDC(NULL);
    if (!hdc) return NULL;
    HDC mem = CreateCompatibleDC(hdc);
    if (!mem) { ReleaseDC(NULL, hdc); return NULL; }

    // 颜色位图（32bpp，后续强制 alpha=255 保证不透明）
    BITMAPINFO bi = {0};
    bi.bmiHeader.biSize        = sizeof(BITMAPINFOHEADER);
    bi.bmiHeader.biWidth       = sz;
    bi.bmiHeader.biHeight      = -sz;   // 顶向下
    bi.bmiHeader.biPlanes      = 1;
    bi.bmiHeader.biBitCount    = 32;
    bi.bmiHeader.biCompression = BI_RGB;
    RGBQUAD* bits = NULL;
    HBITMAP hbmp = CreateDIBSection(mem, &bi, DIB_RGB_COLORS, (void**)&bits, NULL, 0);
    HBITMAP old = (HBITMAP)SelectObject(mem, hbmp);

    // 背景：蓝色实底（不透明）
    HBRUSH bg = CreateSolidBrush(RGB(0x1E, 0x40, 0xAF));
    HBRUSH ob = (HBRUSH)SelectObject(mem, bg);
    RECT rr = {0, 0, sz, sz};
    FillRect(mem, &rr, bg);
    SelectObject(mem, ob); DeleteObject(bg);

    // 白色眼形：横向椭圆（眼白）+ 中央深色瞳孔
    int mx = sz / 2, my = sz / 2;
    int ew = (int)(sz * 0.36), eh = (int)(sz * 0.20);
    HPEN wp = CreatePen(PS_SOLID, 3, RGB(255, 255, 255));
    HPEN owp = (HPEN)SelectObject(mem, wp);
    HBRUSH wb = CreateSolidBrush(RGB(255, 255, 255));
    HBRUSH owb = (HBRUSH)SelectObject(mem, wb);
    Ellipse(mem, mx - ew, my - eh, mx + ew, my + eh);
    SelectObject(mem, owb); DeleteObject(wb);
    SelectObject(mem, owp); DeleteObject(wp);
    // 瞳孔：深色实心圆
    HPEN pp = CreatePen(PS_SOLID, 2, RGB(0x10, 0x18, 0x40));
    HPEN opp = (HPEN)SelectObject(mem, pp);
    HBRUSH pb = CreateSolidBrush(RGB(0x10, 0x18, 0x40));
    HBRUSH opb = (HBRUSH)SelectObject(mem, pb);
    int pr = (int)(sz * 0.11);
    Ellipse(mem, mx - pr, my - pr, mx + pr, my + pr);
    SelectObject(mem, opb); DeleteObject(pb);
    SelectObject(mem, opp); DeleteObject(pp);
    SelectObject(mem, old);

    // 强制不透明（alpha=255）：避免 PNG 式透明导致菜单里"白板"
    for (int i = 0; i < sz * sz; i++) bits[i].rgbReserved = 255;

    // 1bpp 全不透明掩码（0 = 不透明）
    BITMAPINFO bmi = {0};
    bmi.bmiHeader.biSize        = sizeof(BITMAPINFOHEADER);
    bmi.bmiHeader.biWidth       = sz;
    bmi.bmiHeader.biHeight      = sz;
    bmi.bmiHeader.biPlanes      = 1;
    bmi.bmiHeader.biBitCount    = 1;
    bmi.bmiHeader.biCompression = BI_RGB;
    BYTE* mbits = NULL;
    HBITMAP hmask = CreateDIBSection(mem, &bmi, DIB_RGB_COLORS, (void**)&mbits, NULL, 0);
    if (mbits) memset(mbits, 0, ((sz + 31) / 32) * 4 * sz);

    // 统一句柄持有（与 MakeTerminalIcon 一致），释放一次避免泄漏
    HBITMAP hMaskBmp = hmask ? hmask : CreateBitmap(sz, sz, 1, 1, NULL);
    ICONINFO ii = {0};
    ii.fIcon    = TRUE;
    ii.hbmColor = hbmp;
    ii.hbmMask  = hMaskBmp;
    HICON hIcon = CreateIconIndirect(&ii);

    DeleteObject(hbmp);
    DeleteObject(hMaskBmp);
    DeleteDC(mem);
    ReleaseDC(NULL, hdc);
    return hIcon;
}

// 计算稳定的图标存放目录：
//  perUser=TRUE ：%LOCALAPPDATA%\WinSuiteX（标准用户可写，Explorer 同用户上下文可读取）；
//  perUser=FALSE：优先 %ProgramData%\WinSuiteX（机器级、稳定），不可写时回退 exe 所在目录。
static BOOL GetIconStoreDir(WCHAR* dir, size_t n, BOOL perUser = FALSE) {
    if (perUser) {
        WCHAR la[MAX_PATH];
        if (SUCCEEDED(SHGetFolderPathW(NULL, CSIDL_LOCAL_APPDATA,
                                       NULL, SHGFP_TYPE_CURRENT, la))) {
            StringCchPrintfW(dir, n, L"%ls\\WinSuiteX", la);
            CreateDirectoryW(dir, NULL);   // 用户可写，尽力创建子目录
            return TRUE;
        }
        // 极端回退：exe 同目录
        WCHAR exe[MAX_PATH];
        GetModuleFileNameW(NULL, exe, _countof(exe));
        WCHAR* sl = wcsrchr(exe, L'\\');
        if (sl) *sl = L'\0';
        StringCchCopyW(dir, n, exe);
        return TRUE;
    }
    WCHAR pd[MAX_PATH];
    if (SUCCEEDED(SHGetFolderPathW(NULL, CSIDL_COMMON_APPDATA,
                                   NULL, SHGFP_TYPE_CURRENT, pd))) {
        StringCchPrintfW(dir, n, L"%ls\\WinSuiteX", pd);
        if (CreateDirectoryW(dir, NULL) || GetLastError() == ERROR_ALREADY_EXISTS) {
            DWORD a = GetFileAttributesW(dir);
            if (a != INVALID_FILE_ATTRIBUTES && (a & FILE_ATTRIBUTE_DIRECTORY)) {
                WCHAR probe[MAX_PATH];
                StringCchPrintfW(probe, _countof(probe), L"%ls\\.writetest", dir);
                HANDLE h = CreateFileW(probe, GENERIC_WRITE, 0, NULL,
                                       CREATE_ALWAYS, 0, NULL);
                if (h != INVALID_HANDLE_VALUE) {
                    CloseHandle(h); DeleteFileW(probe);
                    return TRUE;
                }
            }
        }
    }
    WCHAR exe[MAX_PATH];
    GetModuleFileNameW(NULL, exe, _countof(exe));
    WCHAR* sl = wcsrchr(exe, L'\\');
    if (sl) *sl = L'\0';
    StringCchCopyW(dir, n, exe);
    return TRUE;
}

// Shell DLL 释放目标：优先 C:\Windows\WinSuiteXShell.dll（用户指定目录），
// 若该处已存在则直接用之；回退 System32 仅当 Windows 根写入受限且 DLL 落在那里时。
// 注：本工具与 DLL 均为 64 位（MinGW-w64），与 64 位 Win10/11/Server 的原生 64 位
// Explorer 位数一致；64 位系统不存在 32 位 Explorer，故无需 WOW6432Node 镜像注册。
#define SHELL_DLL_NAME L"WinSuiteXShell.dll"

// perUser=TRUE ：DLL 落到用户可写的 %LOCALAPPDATA%\WinSuiteX\WinSuiteXShell.dll，
//   标准用户无需管理员即可写入、并由同用户上下文的 Explorer 加载（COM 服务器仍无提权清单）。
// perUser=FALSE：机器级，优先 C:\Windows\WinSuiteXShell.dll（回退 System32）。
void GetShellDllPath(WCHAR* out, BOOL perUser) {
    if (perUser) {
        WCHAR la[MAX_PATH];
        if (SUCCEEDED(SHGetFolderPathW(NULL, CSIDL_LOCAL_APPDATA,
                                       NULL, SHGFP_TYPE_CURRENT, la))) {
            StringCchPrintfW(out, MAX_PATH, L"%ls\\WinSuiteX\\%ls", la, SHELL_DLL_NAME);
            return;
        }
        // 极端回退：exe 同目录
        WCHAR exe[MAX_PATH];
        GetModuleFileNameW(NULL, exe, _countof(exe));
        WCHAR* sl = wcsrchr(exe, L'\\');
        if (sl) *sl = L'\0';
        StringCchPrintfW(out, MAX_PATH, L"%ls\\%ls", exe, SHELL_DLL_NAME);
        return;
    }
    WCHAR win[MAX_PATH], sys[MAX_PATH];
    GetWindowsDirectoryW(win, MAX_PATH);
    GetSystemDirectoryW(sys, MAX_PATH);
    StringCchPrintfW(out, MAX_PATH, L"%ls\\%ls", win, SHELL_DLL_NAME);
    if (GetFileAttributesW(out) == INVALID_FILE_ATTRIBUTES) {
        WCHAR alt[MAX_PATH];
        StringCchPrintfW(alt, MAX_PATH, L"%ls\\%ls", sys, SHELL_DLL_NAME);
        if (GetFileAttributesW(alt) != INVALID_FILE_ATTRIBUTES)
            StringCchCopyW(out, MAX_PATH, alt);
    }
}

// 从 exe 内嵌资源（RCDATA IDR_SHELL_DLL）释放 Shell DLL。
// force=TRUE 时覆盖已存在的（用于升级）。
// perUser=TRUE ：释放到 %LOCALAPPDATA%\WinSuiteX（标准用户可写，无需管理员）；
// perUser=FALSE：释放到 C:\Windows（需管理员写权限，由 exe 的 requireAdministrator 清单提供），
//                 受限时回退 System32。返回 DLL 是否就绪（存在且可读）。
BOOL EnsureShellDll(BOOL force, BOOL perUser) {
    WCHAR path[MAX_PATH];
    GetShellDllPath(path, perUser);
    // per-user 场景：确保父目录 %LOCALAPPDATA%\WinSuiteX 存在（标准用户可创建）
    if (perUser) {
        WCHAR dir[MAX_PATH];
        StringCchCopyW(dir, MAX_PATH, path);
        WCHAR* sl = wcsrchr(dir, L'\\');
        if (sl) { *sl = L'\0'; CreateDirectoryW(dir, NULL); }
    }
    if (!force && GetFileAttributesW(path) != INVALID_FILE_ATTRIBUTES) return TRUE;

    HRSRC hrs = FindResourceW(NULL, MAKEINTRESOURCE(IDR_SHELL_DLL), RT_RCDATA);
    if (!hrs) return FALSE;
    HGLOBAL hg = LoadResource(NULL, hrs);
    if (!hg) return FALSE;
    LPVOID p = LockResource(hg);
    DWORD sz = SizeofResource(NULL, hrs);
    if (!p || sz == 0) return FALSE;

    // per-user 写入 %LOCALAPPDATA%\WinSuiteX（标准用户可写）；
    // 机器级写入 C:\Windows（失败回退 System32）。
    HANDLE hf = CreateFileW(path, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS,
                            FILE_ATTRIBUTE_NORMAL, NULL);
    if (hf == INVALID_HANDLE_VALUE) {
        if (!perUser) {
            WCHAR sys[MAX_PATH];
            GetSystemDirectoryW(sys, MAX_PATH);
            StringCchPrintfW(path, MAX_PATH, L"%ls\\%ls", sys, SHELL_DLL_NAME);
            hf = CreateFileW(path, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS,
                             FILE_ATTRIBUTE_NORMAL, NULL);
            if (hf == INVALID_HANDLE_VALUE) return FALSE;
        } else {
            // per-user 写入失败（如磁盘满）：不应回退到系统目录（需管理员），直接失败
            return FALSE;
        }
    }
    DWORD wr;
    BOOL ok = WriteFile(hf, p, sz, &wr, NULL);
    CloseHandle(hf);
    if (!ok || wr != sz) { DeleteFileW(path); return FALSE; }
    return TRUE;
}

// ============================================================
// features.cpp - Feature 模块数组与安装/卸载逻辑
// ============================================================

// 此电脑 / 我的电脑 / 计算机 的 Shell 命名空间 CLSID
// 桌面图标与资源管理器导航窗格中的"此电脑"共用此 CLSID，
// 因此写入此路径可同时覆盖 Win10 / Win11 / Win Server。
#define THIS_PC_CLSID L"CLSID\\{20D04FE0-3AEA-1069-A2D8-08002B30309D}\\shell"

// Win11 新菜单直显用的 COM 服务器 CLSID（DelegateExecute 指向本 exe WinSuiteX.exe）。
// 仅作为注册标识，COM 服务器是通用的，运行时按自身 CLSID 从注册表读取标题/命令。
#define CLSID_WIN11_COMPMGMT L"{245923CB-428A-4AA0-A0C1-3B6A30823500}"
#define CLSID_WIN11_DEVMGMT  L"{25EBD382-8E3E-4D90-9F4E-A7F21A118CE1}"
#define CLSID_WIN11_SUPERHIDDEN L"{9B27F9A8-2EEA-4F7B-99BD-9B6DCB96AB49}"
// 命令行（在此打开命令提示符）的 Win11 新菜单直显 CLSID
// 合并文件夹 / 空白处为单一功能，共用一个 CLSID（COM 服务器按 CLSID 读配置）
#define CLSID_WIN11_CMDHERE L"{A9160049-1FCF-435D-BEBA-EEAFF59FFB8F}"
// PowerShell（在此处打开 PowerShell）的 Win11 新菜单直显 CLSID
#define CLSID_WIN11_PWSH    L"{0F486944-1B88-4059-ADFD-2ACF2469D7F2}"
// SuperHidden 的 shdocvw 委托 CLSID
#define CLSID_SUPERHIDDEN    L"{3D6EAAA5-35CF-4051-95E8-A18A4928CF0D}"

// ------------------------------------------------------------
// 模块数组：新增功能只需在这里追加一条
// ------------------------------------------------------------
Feature g_features[] = {
    {
        L"ComputerManagement",                          // id
        L"计算机管理",                                    // 右键菜单文字
        L"在“此电脑”右键菜单添加“计算机管理”，一键打开 compmgmt.msc", // 说明
        THIS_PC_CLSID,                                  // 注册表父路径
        L"ComputerManagement",                          // verb
        FeatureKind::RunCommand,                        // 类型
        L"mmc.exe \"%SystemRoot%\\system32\\compmgmt.msc\"", // 命令
        NULL,                                           // clsid (RunCommand 不用)
        L"compmgmt.msc",                                // 菜单图标
        NULL,                                           // 菜单位置（NULL=系统默认顺序，不设 Position）
        CLSID_WIN11_COMPMGMT,                           // Win11 新菜单 COM CLSID
        NULL                                            // toggleArg
    },
    {
        L"DeviceManager",                               // id (演示用，证明模块可扩展)
        L"设备管理器",
        L"演示模块：在“此电脑”右键菜单添加“设备管理器”(devmgmt.msc)",
        THIS_PC_CLSID,
        L"DeviceManager",
        FeatureKind::RunCommand,
        L"mmc.exe \"%SystemRoot%\\system32\\devmgmt.msc\"",
        NULL,
        L"devmgmt.msc",
        NULL,
        CLSID_WIN11_DEVMGMT,
        NULL
    },
    {
        L"SuperHidden",                                 // id
        L"显示/隐藏 系统文件",                           // 右键菜单文字
        L"在文件夹空白处右键一键切换“显示/隐藏 系统文件与扩展名”",
        L"Directory\\Background\\shell",                // 文件夹空白处右键
        L"SuperHiddenToggle",                           // verb
        FeatureKind::ShellExtension,                    // 类型（调本 DLL 导出的 ToggleSuperHidden）
        NULL,                                           // command（用 toggleArg 构造）
        CLSID_SUPERHIDDEN,                              // shdocvw 委托 CLSID
        NULL,                                           // menuIcon（可选）
        NULL,                                           // 菜单位置（NULL=系统默认顺序）
        CLSID_WIN11_SUPERHIDDEN,                         // Win11 新菜单 COM CLSID（文件夹背景也直显）
        L"ToggleSuperHidden"                           // 标记：该功能由本 DLL 的 ToggleSuperHidden 导出服务
    },
    {
        L"CmdHere",                                   // id
        L"命令提示符",                                  // 右键菜单文字（文件夹与空白处统一显示）
        L"在文件夹或空白处右键一键打开命令提示符，并自动定位到当前文件夹", // 说明
        L"Directory\\shell",                          // 主父路径（文件夹右键）
        L"CmdHere",                                   // verb
        FeatureKind::RunCommand,                      // 类型
        L"%SystemRoot%\\system32\\cmd.exe /s /k pushd \"%V\"", // 命令（%V=当前文件夹路径）
        NULL,                                         // clsid（RunCommand 不用）
        L"builtin:terminal",                          // 菜单图标：内置终端图标（避免 cmd.exe 抽取成白板）
        NULL,                                         // 菜单位置（NULL=系统默认顺序）
        CLSID_WIN11_CMDHERE,                          // Win11 新菜单 COM CLSID（文件夹）
        NULL,                                         // toggleArg
        L"Directory\\Background\\shell",              // 第二父路径：文件夹空白处右键（在此打开命令提示符）
        L"DesktopBackground\\shell"                   // 第三父路径：桌面空白处右键
    },
    {
        L"PowerShellHere",                             // id
        L"PowerShell",                                // 右键菜单文字（文件夹与空白处统一显示）
        L"在文件夹或空白处右键一键打开 PowerShell，并自动定位到当前文件夹", // 说明
        L"Directory\\shell",                          // 主父路径（文件夹右键）
        L"PowerShellHere",                            // verb
        FeatureKind::RunCommand,                      // 类型
        L"%SystemRoot%\\system32\\WindowsPowerShell\\v1.0\\powershell.exe -NoExit -Command \"Set-Location -LiteralPath '%V'\"", // 命令（%V=当前文件夹路径）
        NULL,                                         // clsid（RunCommand 不用）
        L"powershell.exe",                            // 菜单图标：抽取系统 powershell.exe 真实图标
        NULL,                                         // 菜单位置（NULL=系统默认顺序）
        CLSID_WIN11_PWSH,                             // Win11 新菜单 COM CLSID（文件夹）
        NULL,                                         // toggleArg
        L"Directory\\Background\\shell",              // 第二父路径：文件夹空白处右键
        L"DesktopBackground\\shell"                   // 第三父路径：桌面空白处右键
    },
    // 后续可在此追加，例如：
    //  - 磁盘管理 (diskmgmt.msc) / 服务 (services.msc) / 本地用户和组 (lusrmgr.msc)
    //  - 为 SuperHidden 也加 Win11 文件夹背景新菜单直显（再加一个 win11Clsid 即可）
};

const size_t g_featureCount = sizeof(g_features) / sizeof(g_features[0]);

// ------------------------------------------------------------
// 运行状态查询
// ------------------------------------------------------------
// 判断某 verb（相对父路径）是否已注册：机器级(HKCR) 或 用户级(HKCU\Software\Classes)
static BOOL VerbInstalled(const WCHAR* parent, const WCHAR* verb) {
    WCHAR k[512];
    StringCchPrintfW(k, _countof(k), L"%s\\%s", parent, verb);
    if (RegKeyExists(HKEY_CLASSES_ROOT, k)) return TRUE;
    WCHAR hkcu[512];
    StringCchPrintfW(hkcu, _countof(hkcu), L"Software\\Classes\\%s", k);
    return RegKeyExists(HKEY_CURRENT_USER, hkcu);
}

BOOL Feature_IsInstalled(const Feature* f) {
    // 主父路径
    if (VerbInstalled(f->regParent, f->verb)) return TRUE;
    // 第二父路径（如合并后的文件夹空白处右键）
    if (f->regParent2 && VerbInstalled(f->regParent2, f->verb)) return TRUE;
    // 第三父路径（如桌面空白处右键）
    if (f->regParent3 && VerbInstalled(f->regParent3, f->verb)) return TRUE;
    // Win11 委托键也作为“已安装”的判据（即便经典 command 子键不存在）
    if (f->win11Clsid) {
        WCHAR ck[512];
        StringCchPrintfW(ck, _countof(ck), L"CLSID\\%s", f->win11Clsid);
        if (RegKeyExists(HKEY_CLASSES_ROOT, ck)) return TRUE;
        WCHAR hkcuCk[512];
        StringCchPrintfW(hkcuCk, _countof(hkcuCk), L"Software\\Classes\\%s", ck);
        if (RegKeyExists(HKEY_CURRENT_USER, hkcuCk)) return TRUE;
    }
    return FALSE;
}

// 当前用户级类注册表必须位于 HKCU\Software\Classes 下，
// 而 HKEY_CLASSES_ROOT 已隐含 Software\Classes；故写入 HKCU 时补此前缀。
static void ClassesPrefix(HKEY root, WCHAR* prefix, size_t n) {
    if (root == HKEY_CURRENT_USER)
        StringCchCopyW(prefix, n, L"Software\\Classes\\");
    else
        prefix[0] = L'\0';
}

// ------------------------------------------------------------
// RunCommand：静态 verb 安装 / 卸载
// ------------------------------------------------------------

// 将定义里的 %SystemRoot% / %windir% 等环境变量展开为真实路径再存储。
// 原因：上下文菜单 verb 的 command 值若以 REG_SZ 保存且仍含 "%S" 字样，
//       Explorer 会把它当作格式符解析，导致 "SystemRoot" 被误改成 "1ystemRoot"。
//       展开成具体路径后可彻底消除此风险，且不依赖 Shell 是否自动展开环境变量。
static void ExpandCmdForStore(const wchar_t* src, wchar_t* dst, size_t n) {
    DWORD r = ExpandEnvironmentStringsW(src, dst, (DWORD)n);
    if (r == 0 || r > (DWORD)n) {
        StringCchCopyW(dst, n, src);   // 展开失败则原样保留
    }
}

// 为 Win11 新菜单注册 COM 服务器（通用 DLL 按自身 CLSID 读标题/命令）。
// pOverrideCmd: 非空时用此命令覆盖 f->command（ShellExtension 的 toggle 路径由此传入）
static void RegisterWin11Command(const Feature* f, HKEY root, const WCHAR* dllPath,
                                 const WCHAR* pOverrideCmd = NULL) {
    BOOL perUser = (root == HKEY_CURRENT_USER);
    WCHAR prefix[32]; ClassesPrefix(root, prefix, _countof(prefix));
    WCHAR clsidKey[512];
    StringCchPrintfW(clsidKey, _countof(clsidKey), L"%sCLSID\\%s", prefix, f->win11Clsid);

    WCHAR inproc[512];
    StringCchPrintfW(inproc, _countof(inproc), L"%s\\InProcServer32", clsidKey);
    RegWriteString(root, inproc, NULL, dllPath);
    RegWriteString(root, inproc, L"ThreadingModel", L"Apartment");

    // 标题/命令写在本 CLSID 下，DLL 运行时读取（单一数据源 = 管理器）
    // 命令优先使用覆盖值（ShellExtension toggle 路径），否则用 f->command
    WCHAR expandedCmd[1024] = {0};
    const wchar_t* cmdSrc = pOverrideCmd ? pOverrideCmd : f->command;
    ExpandCmdForStore(cmdSrc, expandedCmd, _countof(expandedCmd));
    RegWriteString(root, clsidKey, L"Command", expandedCmd);
    RegWriteString(root, clsidKey, L"Title", f->displayName);

    // 图标：优先复用 RunCommand 已写出的 .ico；SuperHidden 等无 menuIcon 的 ShellExtension
    // 没有生成 .ico，此时兜底生成内置"眼睛"图标，确保 Win11 新菜单项有图（避免空白）。
    WCHAR idir[MAX_PATH]; GetIconStoreDir(idir, _countof(idir), perUser);
    WCHAR outIco[MAX_PATH];
    StringCchPrintfW(outIco, _countof(outIco), L"%ls\\%ls.ico", idir, f->id);
    if (GetFileAttributesW(outIco) == INVALID_FILE_ATTRIBUTES) {
        HICON h = MakeEyeIcon(32);
        if (h) { WriteIcoFromHICON(h, outIco); DestroyIcon(h); }
    }
    if (GetFileAttributesW(outIco) != INVALID_FILE_ATTRIBUTES)
        RegWriteString(root, clsidKey, L"Icon", outIco);
}

static BOOL InstallRunCommand(const Feature* f, HKEY root) {
    BOOL perUser = (root == HKEY_CURRENT_USER);
    WCHAR prefix[32]; ClassesPrefix(root, prefix, _countof(prefix));

    // 抽取/生成菜单图标（一次），供两处注册复用
    WCHAR outIco[MAX_PATH] = {0};
    BOOL haveIcon = FALSE;
    if (f->menuIcon) {
        WCHAR idir[MAX_PATH];
        GetIconStoreDir(idir, _countof(idir), perUser);
        StringCchPrintfW(outIco, _countof(outIco), L"%ls\\%ls.ico", idir, f->id);
        if (wcscmp(f->menuIcon, L"builtin:terminal") == 0) {
            // 内置终端图标：直接由 GDI 生成，保证实底不透明
            HICON h = MakeTerminalIcon(32);
            if (h) { haveIcon = WriteIcoFromHICON(h, outIco); DestroyIcon(h); }
        } else {
            // 把 menuIcon 源文件（如 compmgmt.msc）关联的系统图标
            // 提取为真正的 .ico 再引用。直接写 ".msc" 作为 Icon 会被 Explorer 当成
            // 非 PE 文件而无法提取，导致空白图标；存成 .ico 可彻底规避。
            WCHAR src[MAX_PATH] = {0};
            if (ResolveSystemFile(f->menuIcon, src, _countof(src)) &&
                ExtractAndSaveIcon(src, outIco)) {
                haveIcon = TRUE;
            } else {
                // 提取失败（极少数环境）：回退到 mmc.exe 真实 PE 图标，
                // 至少保证不是空白图标；若仍不可用则干脆不写（优于白板）。
                WCHAR mmc[MAX_PATH] = {0};
                if (ResolveSystemFile(L"%SystemRoot%\\system32\\mmc.exe",
                                      mmc, _countof(mmc))) {
                    StringCchCopyW(outIco, _countof(outIco), mmc);
                    haveIcon = TRUE;
                }
            }
        }
    }

    // 写到主父路径 + 可选的第二/第三父路径（如文件夹空白处、桌面空白处右键），合并为同一功能
    const WCHAR* parents[3] = { f->regParent, f->regParent2, f->regParent3 };
    for (int k = 0; k < 3; k++) {
        if (!parents[k]) continue;
        WCHAR verbKey[512];
        StringCchPrintfW(verbKey, _countof(verbKey), L"%s%s\\%s", prefix, parents[k], f->verb);
        WCHAR cmdKey[512];
        StringCchPrintfW(cmdKey, _countof(cmdKey), L"%s\\command", verbKey);

        // 菜单显示文字
        if (!RegWriteString(root, verbKey, NULL, f->displayName))
            return FALSE;
        // 实际命令（展开 %SystemRoot% 等环境变量，存储为具体路径）
        WCHAR expandedCmd[1024] = {0};
        ExpandCmdForStore(f->command, expandedCmd, _countof(expandedCmd));
        if (!RegWriteString(root, cmdKey, NULL, expandedCmd))
            return FALSE;
        // 图标
        if (haveIcon) RegWriteString(root, verbKey, L"Icon", outIco);
        // 位置：NULL = 系统默认顺序（不设 Position，第三方项排在系统内置项之后）；
        //       非 NULL 才写 Top/Bottom。升级兼容：position 变 NULL 时主动清除旧版
        //       残留的 Position（如曾经的 Top），否则旧值会残留在注册表继续生效。
        if (f->position)
            RegWriteString(root, verbKey, L"Position", f->position);
        else
            RegDeleteValueSafe(root, verbKey, L"Position");
        // Win11 新菜单直显：DelegateExecute 指向通用 COM 服务器
        if (f->win11Clsid)
            RegWriteString(root, verbKey, L"DelegateExecute", f->win11Clsid);
    }

    // Win11 COM 服务器（CLSID 键：InProcServer32 + 标题/命令/图标），两处位置共用
    if (f->win11Clsid) {
        WCHAR dllPath[MAX_PATH] = {0};
        GetShellDllPath(dllPath, perUser);   // per-user 落 %LOCALAPPDATA，机器级落 Windows 目录
        RegisterWin11Command(f, root, dllPath);
    }

    NotifyShellChanged();
    return TRUE;
}

static BOOL UninstallRunCommand(const Feature* f, HKEY root) {
    WCHAR prefix[32]; ClassesPrefix(root, prefix, _countof(prefix));
    BOOL perUser = (root == HKEY_CURRENT_USER);   // 与 InstallRunCommand 对称：决定图标落盘位置
    const WCHAR* parents[3] = { f->regParent, f->regParent2, f->regParent3 };
    BOOL ok = TRUE;
    for (int k = 0; k < 3; k++) {
        if (!parents[k]) continue;
        WCHAR verbKey[512];
        StringCchPrintfW(verbKey, _countof(verbKey), L"%s%s\\%s", prefix, parents[k], f->verb);
        if (!DeleteRegTreeKey(root, verbKey)) ok = FALSE;
    }
    // 顺手删除本功能提取出的图标文件（最佳努力）
    WCHAR idir[MAX_PATH];
    GetIconStoreDir(idir, _countof(idir), perUser);   // per-user 落在 %LOCALAPPDATA%，与安装时一致
    WCHAR outIco[MAX_PATH];
    StringCchPrintfW(outIco, _countof(outIco), L"%ls\\%ls.ico", idir, f->id);
    DeleteFileW(outIco);
    // Win11 COM CLSID 键（删除一次即可）
    if (f->win11Clsid) {
        WCHAR clsidKey[512];
        StringCchPrintfW(clsidKey, _countof(clsidKey), L"%sCLSID\\%s", prefix, f->win11Clsid);
        if (!DeleteRegTreeKey(root, clsidKey)) ok = FALSE;
    }
    // 历史残留清理：命令提示符早期版本曾注册到文件夹空白处右键
    // （v4.0 用 verb "CmdHereBg"；v4.1 合并期用 verb "CmdHere" 于 Directory\Background\shell）。
    // 当前版本仅注册文件夹，故卸载时最佳努力删除这些背景残留，避免菜单里残留空白项。
    if (wcscmp(f->id, L"CmdHere") == 0) {
        WCHAR bgKey[512];
        StringCchPrintfW(bgKey, _countof(bgKey), L"%sDirectory\\Background\\shell\\CmdHere", prefix);
        DeleteRegTreeKey(root, bgKey);
        StringCchPrintfW(bgKey, _countof(bgKey), L"%sDirectory\\Background\\shell\\CmdHereBg", prefix);
        DeleteRegTreeKey(root, bgKey);
        // v4.0 背景专用 CLSID（仅背景使用，可安全删除）
        WCHAR bgClsid[512];
        StringCchPrintfW(bgClsid, _countof(bgClsid),
                         L"%sCLSID\\{642104CF-B2D3-45B1-B99A-1EBFFD25E027}", prefix);
        DeleteRegTreeKey(root, bgClsid);
    }
    NotifyShellChanged();
    return ok;
}

// ------------------------------------------------------------
// ShellExtension：shdocvw 技巧安装 / 卸载
// （用于切换类功能，由本 DLL 导出的 ToggleSuperHidden 服务，无需独立 exe）
// ------------------------------------------------------------
static BOOL InstallShellExtension(const Feature* f, HKEY root) {
    if (!f->clsid && !f->win11Clsid) return FALSE;
    BOOL perUser = (root == HKEY_CURRENT_USER);
    WCHAR prefix[32]; ClassesPrefix(root, prefix, _countof(prefix));

    WCHAR regPath1[512];
    StringCchPrintfW(regPath1, _countof(regPath1),
                     L"%s%s\\%s", prefix, f->regParent, f->verb);

    // 切换命令：调用释放到 Windows 目录的 WinSuiteXShell.dll 导出的 ToggleSuperHidden。
    //  - Win11 新菜单（COM 服务器）走进程内调用：Command = "dll:ToggleSuperHidden"
    //  - 经典 shdocvw 回退 走 rundll32：Param1 = "rundll32 \"<WinSuiteXShell.dll>\",ToggleSuperHidden"
    WCHAR dllPath[MAX_PATH] = {0};
    GetShellDllPath(dllPath, perUser);   // per-user 落 %LOCALAPPDATA，机器级落 Windows 目录
    WCHAR toggleCom[MAX_PATH * 2] = {0};
    WCHAR toggleRundll[MAX_PATH * 4] = {0};
    if (f->toggleArg) {
        StringCchCopyW(toggleCom, _countof(toggleCom), L"dll:ToggleSuperHidden");
        StringCchPrintfW(toggleRundll, _countof(toggleRundll),
                         L"rundll32 \"%ls\",ToggleSuperHidden", dllPath);
    } else {
        ExpandCmdForStore(f->command, toggleCom, _countof(toggleCom));
        ExpandCmdForStore(f->command, toggleRundll, _countof(toggleRundll));
    }

    // ===== 路径 A：有 Win11 CLSID -> 走 DelegateExecute 通道（Win10/11 通用）=====
    // 关键：verb 默认值必须是“显示名文字”，绝不能是 CLSID 字符串，
    // 否则 Win11 新菜单会把默认值当文字直接显示（即用户看到的原始 GUID）。
    if (f->win11Clsid) {
        RegWriteString(root, regPath1, NULL, f->displayName);   // 默认=显示名
        RegWriteString(root, regPath1, L"DelegateExecute", f->win11Clsid);
        // 位置：同上，NULL 时清除旧版残留 Position，非 NULL 才写 Top/Bottom
        if (f->position)
            RegWriteString(root, regPath1, L"Position", f->position);
        else
            RegDeleteValueSafe(root, regPath1, L"Position");
        RegisterWin11Command(f, root, dllPath, toggleCom);

        // 关键回退：Win11 新菜单对 Directory\Background 位置的 DelegateExecute
        // 处理可能与 This PC 不同；若新菜单未走 COM 而回退，需有 command 子键
        // 提供可执行命令，否则 Explorer 会把 verb 默认值（显示名文字）当文件
        // 去"打开" → 报"该文件没有与之关联的应用"。与 InstallRunCommand 保持一致。
        WCHAR cmdKey[512];
        StringCchPrintfW(cmdKey, _countof(cmdKey), L"%s\\command", regPath1);
        RegWriteString(root, cmdKey, NULL, toggleRundll);
        NotifyShellChanged();
        return TRUE;
    }

    // ===== 路径 B：无 Win11 CLSID -> 经典 shdocvw 技巧（Win10/Server 经典菜单）=====
    // 此路径依赖 verb 默认值=CLSID 以触发 shdocvw property bag；仅用于未启用 Win11 直显时。
    if (!f->clsid) return FALSE;
    WCHAR regPath2[512];
    StringCchPrintfW(regPath2, _countof(regPath2),
                     L"%sCLSID\\%s\\InProcServer32", prefix, f->clsid);
    WCHAR regPath3[512];
    StringCchPrintfW(regPath3, _countof(regPath3),
                     L"%sCLSID\\%s\\Instance", prefix, f->clsid);
    WCHAR regPath4[512];
    StringCchPrintfW(regPath4, _countof(regPath4),
                     L"%sCLSID\\%s\\Instance\\InitPropertyBag", prefix, f->clsid);

    RegWriteString(root, regPath1, NULL, f->clsid);
    RegWriteStringExpand(root, regPath2, NULL,
                         L"%SystemRoot%\\system32\\shdocvw.dll");
    RegWriteString(root, regPath2, L"ThreadingModel", L"Apartment");
    RegWriteString(root, regPath3, L"CLSID",
                   L"{3f454f0e-42ae-4d7c-8ea3-328250d6e272}");
    RegWriteString(root, regPath4, L"method", L"ShellExecute");
    RegWriteString(root, regPath4, L"Param1", toggleRundll);
    RegWriteString(root, regPath4, L"command", f->displayName);
    RegWriteString(root, regPath4, L"CLSID",
                   L"{13709620-C279-11CE-A49E-444553540000}");

    NotifyShellChanged();
    return TRUE;
}

static BOOL UninstallShellExtension(const Feature* f, HKEY root) {
    if (!f->clsid) return FALSE;
    WCHAR prefix[32]; ClassesPrefix(root, prefix, _countof(prefix));
    WCHAR regPath1[512];
    StringCchPrintfW(regPath1, _countof(regPath1),
                     L"%s%s\\%s", prefix, f->regParent, f->verb);
    WCHAR clsidKey[512];
    StringCchPrintfW(clsidKey, _countof(clsidKey), L"%sCLSID\\%s", prefix, f->clsid);

    // 最佳努力清理安装时写出的眼睛图标（与 UninstallRunCommand / G1 对称）。
    // 文件名由 RegisterWin11Command 决定为 <f->id>.ico（SuperHidden -> SuperHidden.ico），
    // perUser 决定落在 %LOCALAPPDATA% 还是 %ProgramData%，与安装路径完全对齐，不误删其他功能 .ico。
    BOOL perUser = (root == HKEY_CURRENT_USER);
    WCHAR idir[MAX_PATH];
    GetIconStoreDir(idir, _countof(idir), perUser);
    WCHAR outIco[MAX_PATH];
    StringCchPrintfW(outIco, _countof(outIco), L"%ls\\%ls.ico", idir, f->id);
    if (!DeleteFileW(outIco)) {
        // 最佳努力：图标可能本就没写（如 GDI 不可用 MakeEyeIcon 失败）、已删或路径不可写，忽略。
    }

    BOOL ok = TRUE;
    if (!DeleteRegTreeKey(root, regPath1)) ok = FALSE;
    // 仅经典 shdocvw 路径（!win11Clsid）才创建过 CLSID\{f->clsid} 键；
    // Win11 DelegateExecute 路径（win11Clsid 存在）根本不会创建该键，
    // 此处若仍尝试删除会触发“键不存在”失败，导致误报“注册表删除被拒绝”。
    if (!f->win11Clsid) {
        if (!DeleteRegTreeKey(root, clsidKey)) ok = FALSE;
    }
    // Win11 COM CLSID 键（与 RunCommand 的清理逻辑一致）
    if (f->win11Clsid) {
        WCHAR win11Ck[512];
        StringCchPrintfW(win11Ck, _countof(win11Ck), L"%sCLSID\\%s", prefix, f->win11Clsid);
        if (!DeleteRegTreeKey(root, win11Ck)) ok = FALSE;
    }
    NotifyShellChanged();
    return ok;
}

// ------------------------------------------------------------
// 对外接口
// ------------------------------------------------------------
BOOL Feature_Install(const Feature* f, HKEY root) {
    BOOL perUser = (root == HKEY_CURRENT_USER);
    // 所有功能都依赖 Shell DLL（COM 服务器进程内加载 / rundll32 回退）。
    // per-user(HKCU) 场景把 DLL 释放到用户可写的 %LOCALAPPDATA%\WinSuiteX，
    // 标准用户无需管理员即可写入并供同用户上下文的 Explorer 加载（DLL 仍无提权清单）；
    // 机器级(HKLM) 仍落 C:\Windows，需管理员写权限（由 exe 的 requireAdministrator 清单提供）。
    // 失败（无写权限）则安装无法继续。
    if (!EnsureShellDll(FALSE, perUser)) return FALSE;
    switch (f->kind) {
    case FeatureKind::RunCommand:    return InstallRunCommand(f, root);
    case FeatureKind::ShellExtension: return InstallShellExtension(f, root);
    }
    return FALSE;
}

BOOL Feature_Uninstall(const Feature* f, HKEY root) {
    switch (f->kind) {
    case FeatureKind::RunCommand:    return UninstallRunCommand(f, root);
    case FeatureKind::ShellExtension: return UninstallShellExtension(f, root);
    }
    return FALSE;
}

// ------------------------------------------------------------
// 旧版（RightMenuX）残留清理 —— v1.0.7 改名迁移
// ------------------------------------------------------------
// 背景：v1.0.7 起项目由 RightMenuX 更名为 WinSuiteX，COM 服务器 CLSID 与 DLL
//       文件名全部更换。已装过旧版的机器会残留：旧 CLSID 注册、旧名 DLL
//       （C:\Windows\RightMenuXShell.dll / System32 同名）、旧目录
//       （%LOCALAPPDATA%\RightMenuX、%ProgramData%\RightMenuX）。
// 原则：仅在程序启动时调用一次；最佳努力、静默、失败不阻塞
//       （标准用户无 HKLM 写权限、DLL 被 Explorer 占用等均属正常）。
static const LPCWSTR kLegacyClsids[] = {
    L"CLSID\\{9F3C4D52-7A21-4B8E-9C11-2D5F8A1B6C3E}",
    L"CLSID\\{7B2E9F41-3C88-4D27-A3B5-1E9C7D4F2A60}",
    L"CLSID\\{A3C9D7B2-5E41-4F89-A6C2-8D1E0F3B7A41}",
    L"CLSID\\{D4A1C9E2-8B3F-4C71-9E55-2F6A8D1B0C7D}",
    L"CLSID\\{6A2B8E91-4C30-4F11-BD22-7E3C9A1B5D40}",
    L"CLSID\\{C2D9F3A2-5A83-4B0C-9F44-7E3B2C1D8E6F}",
    L"CLSID\\{642104CF-B2D3-45B1-B99A-1EBFFD25E027}",  // v4.0 背景专用 CLSID
};

// 旧版功能 verb 键（v4.x 在右键菜单里直接注册的字符串命名空间，v5 改为 CLSID 形态）
// 这些键如果不删，重装后 Explorer 可能同时显示新旧两项
static const LPCWSTR kLegacyVerbKeys[] = {
    L"Software\\Classes\\*\\shell\\RightMenuX_ComputerManagement",
    L"Software\\Classes\\*\\shell\\RightMenuX_DeviceManager",
    L"Software\\Classes\\*\\shell\\RightMenuX_SuperHidden",
    L"Software\\Classes\\Directory\\shell\\RightMenuX_CmdHere",
    L"Software\\Classes\\Directory\\Background\\shell\\RightMenuX_CmdHere",
    L"Software\\Classes\\Directory\\Background\\shell\\CmdHereBg",
    L"Software\\Classes\\CLSID\\{20D04FE0-3AEA-1069-A2D8-08002B30309D}\\shell\\RightMenuX_ComputerManagement",
    L"Software\\Classes\\CLSID\\{20D04FE0-3AEA-1069-A2D8-08002B30309D}\\shell\\RightMenuX_DeviceManager",
};

// 递归删除目录（仅限本工具自建目录，里面只有释放的 DLL 与图标，非用户数据）
static void RemoveLegacyDir(const WCHAR* dir) {
    DWORD a = GetFileAttributesW(dir);
    if (a == INVALID_FILE_ATTRIBUTES || !(a & FILE_ATTRIBUTE_DIRECTORY)) return;
    WCHAR pat[MAX_PATH];
    StringCchPrintfW(pat, _countof(pat), L"%ls\\*", dir);
    WIN32_FIND_DATAW fd;
    HANDLE h = FindFirstFileW(pat, &fd);
    if (h == INVALID_HANDLE_VALUE) { RemoveDirectoryW(dir); return; }
    do {
        if (wcscmp(fd.cFileName, L".") == 0 || wcscmp(fd.cFileName, L"..") == 0) continue;
        WCHAR p[MAX_PATH];
        StringCchPrintfW(p, _countof(p), L"%ls\\%ls", dir, fd.cFileName);
        if (fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) RemoveLegacyDir(p);
        else DeleteFileW(p);
    } while (FindNextFileW(h, &fd));
    FindClose(h);
    RemoveDirectoryW(dir);
}

void MigrateCleanupLegacy(void) {
    // 1) 旧 CLSID 注册：HKCU 与 HKLM 都试（HKLM 在标准用户下失败属正常，静默）
    const HKEY roots[2] = { HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE };
    for (int r = 0; r < 2; r++) {
        for (int i = 0; i < (int)_countof(kLegacyClsids); i++) {
            WCHAR key[160];
            StringCchPrintfW(key, _countof(key), L"Software\\Classes\\%ls", kLegacyClsids[i]);
            DeleteRegTreeKey(roots[r], key);
        }
        // 1.5) 旧 verb 键（v4.x 字符串命名空间）。即便旧 CLSID 已删，
        //      这些键也会让 Explorer 显示「右键增强_计算机管理」之类的孤儿项
        for (int i = 0; i < (int)_countof(kLegacyVerbKeys); i++) {
            DeleteRegTreeKey(roots[r], kLegacyVerbKeys[i]);
        }
    }
    // 2) 旧名 DLL：C:\Windows 与 System32
    WCHAR dir[MAX_PATH], p[MAX_PATH];
    if (SUCCEEDED(SHGetFolderPathW(NULL, CSIDL_WINDOWS, NULL, SHGFP_TYPE_CURRENT, dir))) {
        StringCchPrintfW(p, _countof(p), L"%ls\\RightMenuXShell.dll", dir);
        DeleteFileW(p);
    }
    if (SUCCEEDED(SHGetFolderPathW(NULL, CSIDL_SYSTEM, NULL, SHGFP_TYPE_CURRENT, dir))) {
        StringCchPrintfW(p, _countof(p), L"%ls\\RightMenuXShell.dll", dir);
        DeleteFileW(p);
    }
    // 3) 旧目录：%LOCALAPPDATA% 与 %ProgramData%
    if (SUCCEEDED(SHGetFolderPathW(NULL, CSIDL_LOCAL_APPDATA, NULL, SHGFP_TYPE_CURRENT, dir))) {
        StringCchPrintfW(p, _countof(p), L"%ls\\RightMenuX", dir);
        RemoveLegacyDir(p);
    }
    if (SUCCEEDED(SHGetFolderPathW(NULL, CSIDL_COMMON_APPDATA, NULL, SHGFP_TYPE_CURRENT, dir))) {
        StringCchPrintfW(p, _countof(p), L"%ls\\RightMenuX", dir);
        RemoveLegacyDir(p);
    }
}
