#include "explorercommand.h"
#include "common.h"      // RegReadDword/RegWriteDword（SuperHidden 切换用）
#include <shellapi.h>    // SHChangeNotify（刷新资源管理器）

// ============================================================
// explorercommand.cpp - 通用 IExplorerCommand in-proc COM 服务器
//
// v5.1 混合架构：本文件编译为独立的 WinSuiteXShell.dll（不带提权清单，
//       供 Explorer 作为进程内 COM 服务器正常加载）。该 DLL 由 WinSuiteX.exe
//       以 RCDATA 资源内嵌，用户“启用功能”时释放到 C:\Windows\WinSuiteXShell.dll
//       （或回退 System32）并注册。这样避开了“exe 带 requireAdministrator 清单
//       被 COM 拒绝在 Explorer 内加载”的问题——即 v5.0 单文件命令提示符不显示、
//       显示/隐藏无效的真正根因。
//
// 编译开关：
//   -DBUILD_SHELL_DLL  -> 仅产 DLL（排除文件末尾 _mcentry 的 exe 双形态入口）。
//   不带此宏时（历史单文件实验）保留 _mcentry，可编译进 exe 自身。
//
// 设计：本模块是“通用”服务器。DllGetClassObject 收到任意 CLSID 都创建
// CExplorerCommand(clsid)；该对象运行时从注册表
//   HKCR\CLSID\{clsid}\Title   (菜单标题)
//   HKCR\CLSID\{clsid}\Icon    (图标)
//   HKCR\CLSID\{clsid}\Command (要执行的命令行)
// 读取配置。因此管理器是唯一数据源，DLL 不硬编码任何功能数据。
//
// 设计：本模块是"通用"服务器。DllGetClassObject 收到任意 CLSID 都创建
// CExplorerCommand(clsid)；该对象运行时从注册表
//   HKCR\CLSID\{clsid}\Title   (菜单标题)
//   HKCR\CLSID\{clsid}\Icon    (图标)
//   HKCR\CLSID\{clsid}\Command (要执行的命令行)
// 读取配置。因此管理器是唯一数据源，DLL 不硬编码任何功能数据。
//
// 仅实现 IExplorerCommand（不实现 IInitializeCommand），避开 C++ 多重继承
// 的 IUnknown 菱形问题；Explorer 直接 QueryInterface(IExplorerCommand) 使用。
//
// v4.4：原独立"右键增强切换.exe"已合并进本 DLL。ShellExtension 类功能（如
// SuperHidden）的 Command 写成 "dll:ToggleSuperHidden"，Invoke 时进程内直接
// 调用本 DLL 导出的 ToggleSuperHidden()（运行于 Explorer 非提权上下文，无 UAC）。
// 经典 shdocvw 回退路径则经 rundll32 调用同一导出，同样无需独立 exe。
// ============================================================

static LONG g_cLock = 0;   // 全局锁计数，供 DllCanUnloadNow 使用
static HINSTANCE g_hInst = NULL; // 本 DLL 实例句柄（DllMain 写入，供 GetProcAddress 用）

// 执行命令：支持 "dll:FuncName" 进程内调用本 DLL 导出；否则 CreateProcess
static HRESULT RunCommand(const WCHAR* cmd);

static BOOL SameIID(REFIID a, const IID& b) {
    return a.Data1 == b.Data1 && a.Data2 == b.Data2 && a.Data3 == b.Data3 &&
           a.Data4[0] == b.Data4[0] && a.Data4[1] == b.Data4[1] &&
           a.Data4[2] == b.Data4[2] && a.Data4[3] == b.Data4[3] &&
           a.Data4[4] == b.Data4[4] && a.Data4[5] == b.Data4[5] &&
           a.Data4[6] == b.Data4[6] && a.Data4[7] == b.Data4[7];
}

// 从 HKCR\CLSID\{clsid}\<value> 读字符串到 *ppsz（CoTaskMem 分配，调用方释放）
static HRESULT ReadClsidString(const GUID& clsid, LPCWSTR value, LPWSTR* ppsz) {
    *ppsz = NULL;
    WCHAR clsidStr[64];
    if (StringFromGUID2(clsid, clsidStr, _countof(clsidStr)) == 0) return E_FAIL;
    WCHAR key[96];
    StringCchPrintfW(key, _countof(key), L"CLSID\\%ls", clsidStr);

    HKEY hKey;
    LONG r = RegOpenKeyExW(HKEY_CLASSES_ROOT, key, 0, KEY_READ, &hKey);
    if (r != ERROR_SUCCESS) return HRESULT_FROM_WIN32(r);

    DWORD type = 0, cb = 0;
    r = RegQueryValueExW(hKey, value, NULL, &type, NULL, &cb);
    if (r == ERROR_SUCCESS && type == REG_SZ && cb > 0) {
        WCHAR* buf = (WCHAR*)CoTaskMemAlloc(cb);
        if (buf) {
            r = RegQueryValueExW(hKey, value, NULL, NULL, (LPBYTE)buf, &cb);
            if (r == ERROR_SUCCESS) *ppsz = buf;
            else CoTaskMemFree(buf);
        }
    }
    RegCloseKey(hKey);
    return (*ppsz != NULL) ? S_OK : E_FAIL;
}

// 解析并执行命令行（exe + 参数），通过 CreateProcess 启动
static HRESULT RunCommandLine(const WCHAR* cmd) {
    if (!cmd || !*cmd) return E_FAIL;
    size_t len = wcslen(cmd) + 1;
    WCHAR* buf = (WCHAR*)malloc(len * sizeof(WCHAR));
    if (!buf) return E_OUTOFMEMORY;
    wcscpy(buf, cmd);

    STARTUPINFOW si = {0};
    si.cb = sizeof(si);
    PROCESS_INFORMATION pi = {0};
    // lpApplicationName=NULL：CreateProcess 自行从命令行解析 exe（含空格引号均可）
    BOOL ok = CreateProcessW(NULL, buf, NULL, NULL, FALSE,
                             0, NULL, NULL, &si, &pi);
    if (ok) {
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
    }
    free(buf);
    return ok ? S_OK : HRESULT_FROM_WIN32(GetLastError());
}

// 把命令里出现的 Explorer verb 占位符 %V / %1 / %L（含小写）替换为选中项的
// 文件系统路径。返回新分配的字符串（调用方 free）；若命令不含占位符则返回 NULL。
//
// 关键背景（命令提示符“无效”的根因）：
//   Win11 新菜单经 DelegateExecute → 本 COM 服务器 Invoke 执行 Command。
//   Command 存的是 `...cmd.exe /s /k pushd "%V"`，其中 %V 是 Explorer 专用占位符，
//   CreateProcess **不会**展开它 → 结果 cmd 里执行 `pushd "%V"` 失败，窗口一闪即退。
//   （Win10 经典菜单走 command 子键，由 Explorer 自己替换 %V，所以那条路径正常。）
//   修复：在 Invoke 里从 IShellItemArray 取到用户右键的文件夹路径，先替换再执行。
static WCHAR* SubstituteSelectionPath(const WCHAR* cmd, IShellItemArray* psia) {
    if (!cmd) return NULL;

    // 是否含占位符
    BOOL hasMarker = FALSE;
    for (const WCHAR* p = cmd; *p; p++) {
        if (p[0] == L'%' &&
            (p[1] == L'V' || p[1] == L'v' || p[1] == L'1' ||
             p[1] == L'L' || p[1] == L'l')) { hasMarker = TRUE; break; }
    }
    if (!hasMarker) return NULL;

    // 取选中项（文件夹）路径。注意：SIGDN_FILESYSPATH 可能超过 MAX_PATH（深目录树），
    // 故直接用 GetDisplayName 返回的堆字符串（CoTaskMemAlloc，长度正确），避免截断导致
    // pushd "%V" 失败——路径 > MAX_PATH 时旧代码会截断，是命令提示符在深层目录失效的隐患。
    WCHAR* path = NULL;        // 有效路径（可能是 CoTaskMem 堆串，也可能是 fallback 栈串）
    LPWSTR pszAlloc = NULL;     // 需要 CoTaskMemFree 释放的指针（NULL=无需释放）
    BOOL   havePath = FALSE;
    if (psia) {
        DWORD count = 0;
        if (SUCCEEDED(psia->GetCount(&count)) && count > 0) {
            IShellItem* item = NULL;
            if (SUCCEEDED(psia->GetItemAt(0, &item)) && item) {
                LPWSTR psz = NULL;
                if (SUCCEEDED(item->GetDisplayName(SIGDN_FILESYSPATH, &psz)) && psz) {
                    pszAlloc = psz;     // 下方统一 CoTaskMemFree 释放
                    path = psz;
                    havePath = TRUE;
                }
                item->Release();
            }
        }
    }
    // 空白处右键 / 未取到选中项：逐级回退，避免回退到 Explorer 进程 CWD（常为系统目录）
    //   1) 桌面已知文件夹（最常见的“空白处”即桌面右键）
    //   2) 进程当前目录（最佳努力）
    WCHAR fallback[MAX_PATH * 4] = {0};
    if (!havePath) {
        PWSTR desk = NULL;
        if (SUCCEEDED(SHGetKnownFolderPath(FOLDERID_Desktop, 0, NULL, &desk)) && desk) {
            StringCchCopyW(fallback, _countof(fallback), desk);
            CoTaskMemFree(desk);
            path = fallback;
            havePath = TRUE;
        }
    }
    if (!havePath) {
        GetCurrentDirectoryW(_countof(fallback), fallback);
        path = fallback;
    }

    size_t plen = path ? wcslen(path) : 0;
    size_t clen = wcslen(cmd);
    // 每个占位符最多展开为 plen 长；预留足够空间（path 本身不含需再展开的占位符）
    size_t cap = clen + plen * 4 + 8;
    WCHAR* out = (WCHAR*)malloc(cap * sizeof(WCHAR));
    if (!out) { if (pszAlloc) CoTaskMemFree(pszAlloc); return NULL; }

    size_t w = 0;
    for (size_t i = 0; i < clen; ) {
        if (cmd[i] == L'%' &&
            (cmd[i + 1] == L'V' || cmd[i + 1] == L'v' || cmd[i + 1] == L'1' ||
             cmd[i + 1] == L'L' || cmd[i + 1] == L'l')) {
            for (size_t k = 0; k < plen && w < cap - 1; k++) out[w++] = path[k];
            i += 2;
        } else {
            if (w < cap - 1) out[w++] = cmd[i];
            i++;
        }
    }
    out[w] = L'\0';
    if (pszAlloc) CoTaskMemFree(pszAlloc);
    return out;
}

// ---------------- CExplorerCommand ----------------
class CExplorerCommand : public IExplorerCommand {
public:
    CExplorerCommand(const GUID& clsid) : m_clsid(clsid), m_cRef(1) {
        InterlockedIncrement(&g_cLock);
    }
    virtual ~CExplorerCommand() { InterlockedDecrement(&g_cLock); }

    // IUnknown
    STDMETHODIMP QueryInterface(REFIID riid, void** ppv) {
        if (!ppv) return E_POINTER;
        *ppv = NULL;
        if (SameIID(riid, IID_IUnknown) ||
            SameIID(riid, IID_IExplorerCommand) ||
            SameIID(riid, IID_IExplorerCommand_legacy)) {
            *ppv = static_cast<IExplorerCommand*>(this);
        } else {
            return E_NOINTERFACE;
        }
        AddRef();
        return S_OK;
    }
    STDMETHODIMP_(ULONG) AddRef()  { return InterlockedIncrement(&m_cRef); }
    STDMETHODIMP_(ULONG) Release() {
        ULONG r = InterlockedDecrement(&m_cRef);
        if (r == 0) delete this;
        return r;
    }

    // IExplorerCommand
    STDMETHODIMP GetTitle(IShellItemArray*, LPWSTR* ppszName) {
        HRESULT hr = ReadClsidString(m_clsid, L"Title", ppszName);
        if (FAILED(hr)) hr = SHStrDupW(L"WinSuiteX 命令", ppszName);
        return hr;
    }
    STDMETHODIMP GetIcon(IShellItemArray*, LPWSTR* ppszIcon) {
        return ReadClsidString(m_clsid, L"Icon", ppszIcon);
    }
    STDMETHODIMP GetToolTip(IShellItemArray*, LPWSTR* ppszInfotip) {
        *ppszInfotip = NULL;
        return E_NOTIMPL;
    }
    STDMETHODIMP GetCanonicalName(GUID* pguidCommandName) {
        if (!pguidCommandName) return E_POINTER;
        *pguidCommandName = m_clsid;
        return S_OK;
    }
    STDMETHODIMP GetState(IShellItemArray*, WINBOOL, EXPCMDSTATE* pCmdState) {
        if (!pCmdState) return E_POINTER;
        *pCmdState = ECS_ENABLED;
        return S_OK;
    }
    STDMETHODIMP Invoke(IShellItemArray* psia, IBindCtx*) {
        WCHAR* cmd = NULL;
        if (FAILED(ReadClsidString(m_clsid, L"Command", &cmd))) return E_FAIL;
        // 关键修复：CreateProcess 不展开 %V/%1/%L（Explorer 专用占位符），
        // 这里用右键选中的文件夹路径先行替换，命令提示符才能定位到当前目录。
        WCHAR* subst = SubstituteSelectionPath(cmd, psia);
        HRESULT hr = RunCommand(subst ? subst : cmd);
        if (subst) free(subst);
        CoTaskMemFree(cmd);
        return hr;
    }
    STDMETHODIMP GetFlags(EXPCMDFLAGS* pFlags) {
        if (!pFlags) return E_POINTER;
        *pFlags = ECF_DEFAULT;
        return S_OK;
    }
    STDMETHODIMP EnumSubCommands(IEnumExplorerCommand** ppEnum) {
        *ppEnum = NULL;
        return E_NOTIMPL;
    }

private:
    GUID m_clsid;
    LONG m_cRef;
};

// ---------------- CClassFactory ----------------
class CClassFactory : public IClassFactory {
public:
    CClassFactory(const GUID& clsid) : m_clsid(clsid), m_cRef(1) {
        InterlockedIncrement(&g_cLock);
    }
    virtual ~CClassFactory() { InterlockedDecrement(&g_cLock); }

    STDMETHODIMP QueryInterface(REFIID riid, void** ppv) {
        if (!ppv) return E_POINTER;
        *ppv = NULL;
        if (SameIID(riid, IID_IUnknown) || SameIID(riid, IID_IClassFactory)) {
            *ppv = static_cast<IClassFactory*>(this);
        } else {
            return E_NOINTERFACE;
        }
        AddRef();
        return S_OK;
    }
    STDMETHODIMP_(ULONG) AddRef()  { return InterlockedIncrement(&m_cRef); }
    STDMETHODIMP_(ULONG) Release() {
        ULONG r = InterlockedDecrement(&m_cRef);
        if (r == 0) delete this;
        return r;
    }

    STDMETHODIMP CreateInstance(IUnknown* pUnkOuter, REFIID riid, void** ppv) {
        if (pUnkOuter) return CLASS_E_NOAGGREGATION;
        if (!ppv) return E_POINTER;
        *ppv = NULL;
        CExplorerCommand* obj = new CExplorerCommand(m_clsid);
        if (!obj) return E_OUTOFMEMORY;
        HRESULT hr = obj->QueryInterface(riid, ppv);
        obj->Release();   // 若 QI 失败，计数归零自动析构
        return hr;
    }
    STDMETHODIMP LockServer(BOOL fLock) {
        if (fLock) InterlockedIncrement(&g_cLock);
        else       InterlockedDecrement(&g_cLock);
        return S_OK;
    }

private:
    GUID m_clsid;
    LONG m_cRef;
};

// ---------------- SuperHidden 切换（内置于本 DLL，替代原 右键增强切换.exe）----------------
// 运行于 Explorer 非提权上下文（COM 服务器被 Explorer 加载），写入 HKCU，无 UAC。
// 经典 shdocvw 回退路径经 rundll32 调用同一导出，行为一致。
#define SH_REG_PATH L"Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced"

static BOOL SH_IsCurrentlyHidden() {
    DWORD v = 0;
    if (!RegReadDword(HKEY_CURRENT_USER, SH_REG_PATH, L"ShowSuperHidden", &v)) return FALSE;
    return (v == 1);
}

static void SH_RefreshExplorer() {
    // 轻量刷新：只刷新桌面与所有已打开的文件夹窗口（逐个发 F5），
    // 不再广播 SHCNE_ASSOCCHANGED。
    //
    // 旧实现（整 shell 广播）会让桌面/任务栏图标缓存整体重建——切个隐藏文件
    // 整屏“闪一下”，Z.W. 反馈“刷新不舒服”（对比 SuperHidden.vbs 只 SendKeys
    // "{F5}" 刷当前窗口，干脆利落）。隐藏文件/扩展名变化本来只需刷文件夹视图，
    // 不必惊动整 shell。
    //
    // 注意：去掉 SHCNE_ASSOCCHANGED 后，若当时一个文件夹窗口都没打开，切换结果
    // 要等下次打开才可见（与 vbs 行为一致，可接受）。
    HWND hProgMan = FindWindowW(L"Progman", NULL);
    if (hProgMan) {
        PostMessageW(hProgMan, WM_COMMAND, 0x7103, 0);
        PostMessageW(hProgMan, WM_KEYDOWN, VK_F5, 0);
        PostMessageW(hProgMan, WM_KEYUP,   VK_F5, 0);
    }
    EnumWindows([](HWND hWnd, LPARAM) -> BOOL {
        WCHAR className[64] = {0};
        GetClassNameW(hWnd, className, 64);
        if (wcscmp(className, L"CabinetWClass") == 0 ||
            wcscmp(className, L"ExploreWClass") == 0) {
            PostMessageW(hWnd, WM_COMMAND, 0x7103, 0);
            PostMessageW(hWnd, WM_KEYDOWN, VK_F5, 0);
            PostMessageW(hWnd, WM_KEYUP,   VK_F5, 0);
        }
        return TRUE;
    }, 0);
}

// rundll32 入口签名：void CALLBACK Entry(HWND, HINSTANCE, LPSTR, int)
extern "C" __attribute__((dllexport)) void CALLBACK
ToggleSuperHidden(HWND, HINSTANCE, LPSTR, int) {
    BOOL hidden = SH_IsCurrentlyHidden();
    if (hidden) {
        RegWriteDword(HKEY_CURRENT_USER, SH_REG_PATH, L"ShowSuperHidden", 0);
        RegWriteDword(HKEY_CURRENT_USER, SH_REG_PATH, L"Hidden", 2);
        RegWriteDword(HKEY_CURRENT_USER, SH_REG_PATH, L"HideFileExt", 1);
    } else {
        RegWriteDword(HKEY_CURRENT_USER, SH_REG_PATH, L"ShowSuperHidden", 1);
        RegWriteDword(HKEY_CURRENT_USER, SH_REG_PATH, L"Hidden", 1);
        RegWriteDword(HKEY_CURRENT_USER, SH_REG_PATH, L"HideFileExt", 0);
    }
    SH_RefreshExplorer();
}

// 执行命令：支持 "dll:FuncName" 进程内调用本 DLL 导出；否则 CreateProcess
static HRESULT RunCommand(const WCHAR* cmd) {
    if (!cmd || !*cmd) return E_FAIL;
    // 进程内调用本 DLL 导出的函数（如 dll:ToggleSuperHidden）
    if (wcsncmp(cmd, L"dll:", 4) == 0) {
        CHAR fnA[64];
        int n = WideCharToMultiByte(CP_ACP, 0, cmd + 4, -1, fnA, _countof(fnA), NULL, NULL);
        if (n > 0) {
            FARPROC p = GetProcAddress(g_hInst, fnA);
            if (p) {
                typedef void (CALLBACK* TFn)(HWND, HINSTANCE, LPSTR, int);
                ((TFn)p)(NULL, NULL, NULL, 0);
                return S_OK;
            }
        }
        return E_FAIL;
    }
    return RunCommandLine(cmd);
}

// ---------------- DLL 导出 ----------------
extern "C" {

BOOL WINAPI DllMain(HINSTANCE hInst, DWORD, LPVOID) { g_hInst = hInst; return TRUE; }

STDAPI __attribute__((dllexport)) DllGetClassObject(REFCLSID rclsid, REFIID riid, void** ppv) {
    if (!ppv) return E_POINTER;
    *ppv = NULL;
    // 通用服务器：接受任意 CLSID（注册表里 InProcServer32 指向本 DLL 的都由我们处理）
    CClassFactory* cf = new CClassFactory(rclsid);
    if (!cf) return E_OUTOFMEMORY;
    HRESULT hr = cf->QueryInterface(riid, ppv);
    cf->Release();
    return hr;
}

STDAPI __attribute__((dllexport)) DllCanUnloadNow() {
    return (InterlockedExchangeAdd(&g_cLock, 0) == 0) ? S_OK : S_FALSE;
}

STDAPI __attribute__((dllexport)) DllRegisterServer()   { return S_OK; } // 注册由管理器完成
STDAPI __attribute__((dllexport)) DllUnregisterServer() { return S_OK; }

} // extern "C"

// ============================================================
// EXE 双形态入口（仅编译进 WinSuiteX.exe 时启用；DLL 构建用 -DBUILD_SHELL_DLL 排除）
//
// 历史单文件实验（v5.0）：同一个 PE 同时充当管理器 GUI 与被 Explorer 加载的
// 进程内 COM 服务器。但 exe 带 requireAdministrator 清单会被 COM 拒绝在 Explorer
// 进程内加载（命令提示符不显示 / 显示隐藏无效的根因），故 v5.1 已改回独立 DLL。
// 此 _mcentry 代码仅保留作存档/回退，不参与当前正式构建。
// ============================================================
#ifndef BUILD_SHELL_DLL
extern "C" BOOL WINAPI DllMainCRTStartup(HINSTANCE, DWORD, LPVOID); // 来自 dllcrt2.o
extern "C" int  WINAPI WinMainCRTStartup(void);               // 来自 crt2.o（本机 MinGW 仅此符号）

extern "C" __attribute__((used)) int __stdcall
_mcentry(HINSTANCE hInst, DWORD fdwReason, LPVOID lpvReserved) {
    HMODULE selfMod = NULL;
    GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                       GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                       (LPCWSTR)&_mcentry, &selfMod);
    if (selfMod && selfMod != GetModuleHandleW(NULL)) {
        // 被加载为 DLL（进程内 COM 服务器）：走 DLL 的 CRT 启动流程
        return (int)DllMainCRTStartup(selfMod, fdwReason, lpvReserved);
    }
    // 作为普通 exe 运行：走 GUI 的 CRT 启动流程（内部调用 wWinMain，不返回）
    WinMainCRTStartup();
    return 0;
}
#endif // BUILD_SHELL_DLL
