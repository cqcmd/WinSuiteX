#ifndef COMMON_H
#define COMMON_H

#include <windows.h>
#include <shlobj.h>
#include <strsafe.h>

#if defined(__GNUC__)
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wunused-function"
#endif

// ============================================================
// common.h - 通用层：注册表辅助、OS 版本检测、提示
// 供所有 Feature 模块与管理器 UI 共用。
// ============================================================

// ---- 注册表辅助 ----

// 写入字符串值（支持 REG_SZ 与 REG_EXPAND_SZ）
static BOOL RegWriteString(HKEY hRoot, LPCWSTR subKey, LPCWSTR valueName,
                           LPCWSTR value, DWORD type = REG_SZ) {
    if (!value) return FALSE;   // 防御：value 为 NULL 时 wcslen 会越界崩溃
    HKEY hKey;
    LONG r = RegCreateKeyExW(hRoot, subKey, 0, NULL,
                             REG_OPTION_NON_VOLATILE, KEY_SET_VALUE,
                             NULL, &hKey, NULL);
    if (r != ERROR_SUCCESS) return FALSE;
    r = RegSetValueExW(hKey, valueName, 0, type,
                       (const BYTE*)value,
                       (DWORD)((wcslen(value) + 1) * sizeof(WCHAR)));
    RegCloseKey(hKey);
    return (r == ERROR_SUCCESS);
}

// 写入可展开字符串（REG_EXPAND_SZ，支持 %SystemRoot% 等变量）
static BOOL RegWriteStringExpand(HKEY hRoot, LPCWSTR subKey, LPCWSTR valueName,
                                 LPCWSTR value) {
    return RegWriteString(hRoot, subKey, valueName, value, REG_EXPAND_SZ);
}

// 判断某个注册表键是否存在（用于检测功能是否已安装）
static BOOL RegKeyExists(HKEY hRoot, LPCWSTR subKey) {
    HKEY hKey;
    LONG r = RegOpenKeyExW(hRoot, subKey, 0, KEY_READ, &hKey);
    if (r == ERROR_SUCCESS) {
        RegCloseKey(hKey);
        return TRUE;
    }
    return FALSE;
}

// 读取字符串值到缓冲区，成功返回 TRUE（值缺失/类型不符返回 FALSE）
static BOOL RegReadString(HKEY hRoot, LPCWSTR subKey, LPCWSTR valueName,
                          WCHAR* out, DWORD outChars) {
    out[0] = L'\0';
    HKEY hKey;
    LONG r = RegOpenKeyExW(hRoot, subKey, 0, KEY_READ, &hKey);
    if (r != ERROR_SUCCESS) return FALSE;
    DWORD sz = outChars * sizeof(WCHAR);
    r = RegQueryValueExW(hKey, valueName, NULL, NULL, (LPBYTE)out, &sz);
    RegCloseKey(hKey);
    return (r == ERROR_SUCCESS);
}

// 删除整棵注册表子树（避开与 Win32 API RegDeleteTreeW 重名）
static BOOL DeleteRegTreeKey(HKEY hRoot, LPCWSTR subKey) {
    LONG r = RegDeleteTreeW(hRoot, subKey);
    if (r == ERROR_SUCCESS) return TRUE;
    // 键不存在 = 已经卸载（或本就未创建），视为成功。这是“最佳努力清理”的正常情形，
    // 避免把无害的 ERROR_FILE_NOT_FOUND / ERROR_PATH_NOT_FOUND 误报成“删除被拒绝”。
    // 仅当确实无权限（ERROR_ACCESS_DENIED）时才返回失败。
    if (r == ERROR_FILE_NOT_FOUND || r == ERROR_PATH_NOT_FOUND) return TRUE;
    return FALSE;
}

// 删除单个注册表值（最佳努力）：键/值不存在均视为成功，
// 用于安装时清除旧版残留的值（如曾写死的 Position=Top），避免旧值继续生效。
static BOOL RegDeleteValueSafe(HKEY hRoot, LPCWSTR subKey, LPCWSTR valueName) {
    HKEY hKey;
    LONG r = RegOpenKeyExW(hRoot, subKey, 0, KEY_SET_VALUE, &hKey);
    // 键/路径不存在 = 本就无残留，目标已达成；其他打开失败（如权限拒绝）不掩盖，返回失败
    if (r == ERROR_FILE_NOT_FOUND || r == ERROR_PATH_NOT_FOUND) return TRUE;
    if (r != ERROR_SUCCESS) return FALSE;
    r = RegDeleteValueW(hKey, valueName);
    RegCloseKey(hKey);
    // 值不存在（ERROR_FILE_NOT_FOUND）也视为成功（本就是我们要的目标状态）
    return (r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND) ? TRUE : FALSE;
}

// 写入 DWORD 值（用于 SuperHidden 等切换状态的场景）
static BOOL RegWriteDword(HKEY hRoot, LPCWSTR subKey, LPCWSTR valueName, DWORD value) {
    HKEY hKey;
    LONG r = RegCreateKeyExW(hRoot, subKey, 0, NULL,
                             REG_OPTION_NON_VOLATILE, KEY_SET_VALUE,
                             NULL, &hKey, NULL);
    if (r != ERROR_SUCCESS) return FALSE;
    r = RegSetValueExW(hKey, valueName, 0, REG_DWORD,
                       (const BYTE*)&value, sizeof(DWORD));
    RegCloseKey(hKey);
    return (r == ERROR_SUCCESS);
}

// 读取 DWORD 值（缺失/失败返回 FALSE，outValue 不变）
static BOOL RegReadDword(HKEY hRoot, LPCWSTR subKey, LPCWSTR valueName, DWORD* outValue) {
    HKEY hKey;
    LONG r = RegOpenKeyExW(hRoot, subKey, 0, KEY_READ, &hKey);
    if (r != ERROR_SUCCESS) return FALSE;
    DWORD sz = sizeof(DWORD);
    r = RegQueryValueExW(hKey, valueName, NULL, NULL, (LPBYTE)outValue, &sz);
    RegCloseKey(hKey);
    return (r == ERROR_SUCCESS);
}

// 通知 Shell 关联已变更（让右键菜单立即生效）
static void NotifyShellChanged() {
    SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, NULL, NULL);
}

// ---- OS 版本检测 ----
// 直接使用 RtlGetVersion，避免 manifests 导致的 VerifyVersionInfo "说谎" 问题。

typedef LONG (NTAPI *RtlGetVersionPtr)(PRTL_OSVERSIONINFOW);

static void GetOSVersion(DWORD* major, DWORD* minor, DWORD* build) {
    *major = *minor = *build = 0;
    HMODULE ntdll = GetModuleHandleW(L"ntdll.dll");
    if (!ntdll) return;
    RtlGetVersionPtr fn = (RtlGetVersionPtr)GetProcAddress(ntdll, "RtlGetVersion");
    if (!fn) return;
    RTL_OSVERSIONINFOW info = {0};
    info.dwOSVersionInfoSize = sizeof(info);
    if (fn(&info) == 0) {
        *major = info.dwMajorVersion;
        *minor = info.dwMinorVersion;
        *build = info.dwBuildNumber;
    }
}

// 通过 ProductOptions 判断是否为服务器版本
static BOOL IsWindowsServer() {
    HKEY hKey;
    if (RegOpenKeyExW(HKEY_LOCAL_MACHINE,
                      L"SYSTEM\\CurrentControlSet\\Control\\ProductOptions",
                      0, KEY_READ, &hKey) == ERROR_SUCCESS) {
        WCHAR buf[64] = {0};
        DWORD sz = sizeof(buf);
        LONG r = RegQueryValueExW(hKey, L"ProductType", NULL, NULL,
                                  (LPBYTE)buf, &sz);
        RegCloseKey(hKey);
        if (r == ERROR_SUCCESS &&
            (wcsicmp(buf, L"ServerNT") == 0 || wcsicmp(buf, L"LanmanNT") == 0))
            return TRUE;
    }
    return FALSE;
}

// 生成给人看的系统描述，例如 "Windows 11 (Build 22631)"
static void GetOSDisplayString(WCHAR* out, size_t len) {
    DWORD major, minor, build;
    GetOSVersion(&major, &minor, &build);
    const WCHAR* base = L"Windows";
    if (IsWindowsServer()) {
        base = L"Windows Server";
    } else if (major == 10 && build >= 22000) {
        base = L"Windows 11";
    } else if (major == 10) {
        base = L"Windows 10";
    } else if (major == 6 && minor == 3) {
        base = L"Windows 8.1";
    } else if (major == 6 && minor == 1) {
        base = L"Windows 7";
    }
    StringCchPrintfW(out, len, L"%s (Build %u)", base, build);
}

#if defined(__GNUC__)
#pragma GCC diagnostic pop
#endif

#endif // COMMON_H
