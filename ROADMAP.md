# WinSuiteX 规划文档（ROADMAP）

> 目标：把右键增强做成**可扩展套件**，新增功能 = 在 `g_features[]` 数组里加一条描述，不需要改动管理器 UI 与安装逻辑。当前已实现 `WinSuiteX` 统一管理器，含四个模块：计算机管理、设备管理器、命令提示符（RunCommand），以及 SuperHidden（ShellExtension）。

---

## 1. 现状

| 模块 | 位置 | 说明 |
|------|------|------|
| WinSuiteX | 本目录 | 统一管理器（单 exe，内嵌 COM 服务器 DLL）。当前含四个模块：计算机管理、设备管理器、命令提示符（RunCommand，此电脑/文件夹背景右键），以及 SuperHidden（ShellExtension，文件夹背景右键）。 |

> SuperHidden 已作为模块接入本管理器（见第 3、5 节），不再需要单独部署旧 `SuperHiddenApp`（已于 2026-07-24 弃用删除）；管理器安装时一并写入文件夹背景右键菜单，并由内嵌的 `WinSuiteXShell.dll`（`asInvoker`，不弹 UAC）执行切换。

---

## 2. 架构原则

- **数据驱动**：所有功能集中在 `features.cpp` 的 `g_features[]` 数组，一个 `Feature` 结构体描述一个功能。
- **两种注册类型**：
  - `RunCommand` —— 静态 verb：`shell\<verb>\command = 命令行`，直接启动程序/管理工具，**无需额外 exe**。适合"打开某个管理工具"。
  - `ShellExtension` —— COM 服务器 `Invoke` 中切换注册表并刷新资源管理器，适合"切换状态 + 刷新"的功能（如 SuperHidden）。代码已在 `explorercommand.cpp` 中实现（`ToggleSuperHidden` 导出）。
- **通用层 `common.h`**：注册表读写、OS 版本/服务器检测（`RtlGetVersion`）、Shell 刷新通知。所有模块与 UI 共用。
- **管理员权限**：管理器 `app.manifest` 使用 `requireAdministrator`，确保写入 `HKLM\Software\Classes`（即 `HKEY_CLASSES_ROOT`）在 Win10/11/Server 上都能成功；COM 服务器 DLL 为 `asInvoker`，避免每次右键弹 UAC。

---

## 3. "计算机管理"功能要点（已交付）

- **注册位置**：`HKEY_CLASSES_ROOT\CLSID\{20D04FE0-3AEA-1069-A2D8-08002B30309D}\shell\ComputerManagement`
  - 该 CLSID 是"此电脑/我的电脑/计算机"的 Shell 命名空间标识，**桌面图标与资源管理器导航窗格共用**，一次写入即可同时覆盖 Win10 / Win11 / Win Server。
- **写入的值**：
  - `(默认)` = `计算机管理`（菜单文字）
  - `command\(默认)` = `mmc.exe "%SystemRoot%\system32\compmgmt.msc"`
  - `Icon` = 安装时提取的真实 `.ico` 路径（不再写 `.msc` 以免白板）
  - `Position` 不设置（系统默认顺序：第三方项排在"打开/发送到"等系统内置项之后，不强制置顶）
- **兼容性说明**：
  - Win10 / Win Server：右键"此电脑"直接可见。
  - **Win11 新菜单直显（已交付）**：计算机管理 / 设备管理器 / 命令提示符除经典 `command` 子键外，另注册 `DelegateExecute = {CLSID}` 指向通用 COM 服务器 `WinSuiteXShell.dll`（`HKCR\CLSID\{clsid}\InProcServer32`），并在该 CLSID 下写入 `Title`/`Command`/`Icon`；DLL 运行时按自身 CLSID 读取这些配置，直接把菜单项绘入 Win11 新右键菜单，无需"显示更多选项"。
  - 重启（或重启 Explorer）后生效。
- **命令防损坏**：`command` 在存储前用 `ExpandEnvironmentStringsW` 把 `%SystemRoot%` 展开成具体路径，杜绝 Explorer 把 `%S` 当格式符解析成 "1ystemRoot"。
- **%V 占位符修复（命令提示符）**：Win11 新菜单 `Invoke` 读 `cmd.exe /s /k pushd "%V"` 后 `CreateProcess` 不展开 `%V`；已在 `SubstituteSelectionPath()` 中从 `IShellItemArray` 取 `SIGDN_FILESYSPATH` 替换 `%V/%1/%L`。

---

## 4. 如何新增一个功能（标准流程）

以"添加磁盘管理"为例，只需在 `features.cpp` 的 `g_features[]` 里追加一条：

```cpp
{
    L"DiskManagement",                              // id
    L"磁盘管理",                                      // 右键菜单文字
    L"在"此电脑"右键菜单添加"磁盘管理"(diskmgmt.msc)", // 说明
    THIS_PC_CLSID,                                   // 注册表父路径
    L"DiskManagement",                               // verb（ASCII）
    FeatureKind::RunCommand,                         // 类型
    L"mmc.exe \"%SystemRoot%\\system32\\diskmgmt.msc\"",
    NULL,
    L"diskmgmt.msc",
    NULL                                            // 菜单位置（NULL=系统默认顺序，不设 Position）
},
```

> 无需改 `main.cpp`、`common.h`、`features.h`。管理器会自动渲染新卡片、自动处理安装/卸载/状态显示。

### 常见注册表父路径速查

| 目标 | `regParent`（相对 `HKEY_CLASSES_ROOT`） |
|------|------------------------------------------|
| 此电脑（桌面/导航窗格） | `CLSID\{20D04FE0-3AEA-1069-A2D8-08002B30309D}\shell` |
| 文件夹空白处 | `Directory\Background\shell` |
| 所有文件 | `*\shell` |
| 驱动器 | `Drive\shell` |
| 回收站 | `CLSID\{645FF040-5081-101B-9F08-00AA002F954E}\shell` |

---

## 5. 后续功能候选清单（待定优先级）

- [x] **SuperHidden 接入**：已作为 `ShellExtension` 模块纳入本管理器（COM 服务器 `Invoke` 中 `ToggleSuperHidden` 切换，无需独立 exe）。
- [x] **Win11 新菜单直显**：已实现通用 `IExplorerCommand` COM 服务器 `WinSuiteXShell.dll`，计算机管理 / 设备管理器 / 命令提示符通过 `DelegateExecute` 直显于 Win11 新右键菜单。
- [ ] **更多管理工具**：磁盘管理、服务、本地用户和组、事件查看器、组策略（gpedit.msc，仅专业版/服务器）等（加一条 `g_features[]` 即可）。
- [ ] **文件夹/桌面背景类**：如"在此处打开 PowerShell/终端"、"复制文件路径"等（目标 `Directory\Background\shell` 或 `*\shell`）。
- [ ] **系统外观个性化**：暗色模式切换、任务栏对齐（左/中）、资源管理器布局等（改 `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize` 等）。
- [ ] **设置持久化与导入/导出**：把已启用的功能列表存为 `.reg`/JSON，便于在多台机器间迁移。
- [ ] **托盘常驻（可选）**：`resource.h` 里已预留托盘相关 ID，未来可做常驻托盘快速开关。

---

## 6. 构建与运行

```bat
build.bat            :: 自动定位 MinGW-w64，生成图标并编译 WinSuiteX.exe（内嵌 COM 服务器 DLL）
```

产物：
- `WinSuiteX.exe` —— 个性化功能管理器（**以管理员身份运行**，写 `HKLM` 才对全体用户生效），内嵌 `WinSuiteXShell.dll`。
- `WinSuiteXShell.dll` —— Win11 新菜单直显 + SuperHidden 切换的通用 COM 服务器（构建中间产物，内嵌进 exe，启用时由 `EnsureShellDll()` 释放到 `C:\Windows`）。
- 自检内嵌进 exe：`WinSuiteX.exe --selftest-nogui` 无头运行，报告写入 `selftest_report.txt`。

- 依赖：MinGW-w64（`g++` + `windres`），Python 3（仅用于生成 `app.ico`，可跳过）。
- 运行 `WinSuiteX.exe`（管理员），在卡片上点"启用/停用"，再去"此电脑"右键查看效果；Win11 新菜单项重启 Explorer 后生效。
- 自检：`--selftest-nogui` 在 `HKCU` 镜像中验证全部注册表结构（含 Win11 COM 服务器运行时 `GetTitle`），并输出 `selftest_report.txt`。最近一次（v1.0.2）：通过 73，失败 0。

---

## 7. 目录结构

```
WinSuiteX/
├── main.cpp                 # 管理器 UI（卡片列表，安装/卸载）
├── features.h / features.cpp# Feature 模块定义与数组、安装/卸载逻辑
├── common.h                 # 通用层：注册表、OS 检测、Shell 刷新
├── explorercommand.h / .cpp # Win11 新菜单直显通用 COM 服务器（WinSuiteXShell.dll，内置 ToggleSuperHidden 导出）
├── selftest.h / selftest.cpp # 自检逻辑（已内嵌进 WinSuiteX.exe，--selftest-nogui 运行）
├── app.rc / shell.rc / resource.h # 主程序与 DLL 资源
├── app.ico / app.manifest   # 图标 + requireAdministrator 兼容性声明
├── gen_icon.py / preview.py # 生成多尺寸 ICO / 预览图
├── build.bat / rename_outputs.py / version_stamp.py # 构建脚本
├── version.ini              # 版本号（四段式）
├── README.md / CHANGELOG.md / DESIGN_SYSTEM.md / ROADMAP.md
├── README_部署.md / Server验收清单.txt
└── WinSuiteX.exe           # 发布产物（内嵌 WinSuiteXShell.dll）
```
