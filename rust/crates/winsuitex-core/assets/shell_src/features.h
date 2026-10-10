#ifndef FEATURES_H
#define FEATURES_H

#include <windows.h>

// ============================================================
// features.h - Feature 模块定义
// 新增一个个性化功能 = 在 features.cpp 的数组里加一条描述，
// 无需改动管理器 UI 与安装逻辑。
// ============================================================

// 模块类型（纯右键增强：仅 RunCommand 与 ShellExtension 两类）：
//  RunCommand    - 静态 verb：shell\<verb>\command = 命令行，
//                  直接启动程序/管理工具（无需额外 exe）。
//                  若设置了 win11Clsid，还会额外注册 Win11 新菜单直显
//                  （IExplorerCommand / DelegateExecute，COM 服务器为独立
//                   WinSuiteXShell.dll，由 exe 释放到 Windows 目录）。
//  ShellExtension - 借用 shdocvw.dll 的 property bag 技巧，
//                  调用本 DLL 导出的 ToggleSuperHidden（适合需要“切换状态/
//                  刷新资源管理器”的功能，如 SuperHidden）。若设置了 toggleArg，
//                  则 Command 写成 "dll:ToggleSuperHidden"（COM 服务器进程内调用，
//                  无 UAC；经典回退经 rundll32 调用同一导出），不再有独立切换 exe。
enum class FeatureKind {
    RunCommand,
    ShellExtension
};

struct Feature {
    const wchar_t* id;           // 内部标识，例如 L"ComputerManagement"
    const wchar_t* displayName;  // 右键菜单中显示的文字，例如 L"计算机管理"
    const wchar_t* description;  // 管理器 UI 中的说明
    const wchar_t* regParent;    // 注册表父路径（相对 HKEY_CLASSES_ROOT）
    const wchar_t* verb;         // 注册表子键名（建议 ASCII，避免编码问题）
    FeatureKind    kind;
    const wchar_t* command;      // RunCommand: 命令行; ShellExtension: 外部 exe 路径
    const wchar_t* clsid;        // ShellExtension 专用：本功能注册用的唯一 CLSID
    const wchar_t* menuIcon;     // 可选，右键菜单图标（如 L"compmgmt.msc"），NULL 表示无
    const wchar_t* position;     // 可选，菜单位置。NULL=系统默认顺序（不强制置顶，第三方项排在
                                 //   系统内置项之后）；非 NULL 才写 "Top"/"Bottom"。升级兼容：
                                 //   变 NULL 时安装逻辑会清除旧版残留 Position
    const wchar_t* win11Clsid;   // 可选：Win11 新菜单直显用的 COM CLSID；NULL 表示不适用
    const wchar_t* toggleArg;    // 可选：ShellExtension 标记。若设置，则该功能由本 DLL
                                 //          导出的 ToggleSuperHidden 服务（不再依赖独立 exe）
    const wchar_t* regParent2;   // 可选：第二注册表父路径，如同时注册到文件夹空白处右键（NULL=无）
    const wchar_t* regParent3;   // 可选：第三注册表父路径，如桌面空白处右键（NULL=无）
};

// 所有功能模块（在 features.cpp 中定义）
extern Feature g_features[];
extern const size_t g_featureCount;

// 运行时状态与操作
//  root：写入/删除的目标根键。
//       默认 HKEY_CLASSES_ROOT（在提权进程中等价于 HKLM\Software\Classes，机器级生效）；
//       传入 HKEY_CURRENT_USER 则写入 HKCU\Software\Classes（仅当前用户生效，无需管理员）。
BOOL Feature_IsInstalled(const Feature* f);
BOOL Feature_Install(const Feature* f, HKEY root = HKEY_CLASSES_ROOT);
BOOL Feature_Uninstall(const Feature* f, HKEY root = HKEY_CLASSES_ROOT);

// 旧版（RightMenuX）残留清理：启动时调用一次，最佳努力、静默、失败不阻塞
void MigrateCleanupLegacy(void);

// Shell DLL（无提权清单的进程内 COM 服务器）：由 WinSuiteX.exe 内嵌，
// 启用功能时释放到 C:\Windows（回退 System32，机器级）或 %LOCALAPPDATA%\WinSuiteX（per-user）。
void GetShellDllPath(WCHAR* out, BOOL perUser);   // perUser=TRUE 落 %LOCALAPPDATA（标准用户可写）
BOOL EnsureShellDll(BOOL force, BOOL perUser);   // 从内嵌资源释放 DLL；perUser=TRUE 释放到用户可写目录

// 图标辅助：将文件名解析为完整路径 / 抽取系统关联图标并存为 .ico
BOOL ResolveSystemFile(LPCWSTR name, WCHAR* out, size_t n);
BOOL ExtractAndSaveIcon(LPCWSTR sourceFile, LPCWSTR outIco);

#endif // FEATURES_H
