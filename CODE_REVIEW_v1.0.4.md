# RightMenuX v1.0.4 发布前代码质量审查报告

> 审查方：win-qa（质守严）｜类型：Workflow B 已有代码审查（纯静态，未实际编译运行）｜日期：2026-07-28
> 主理人导言：本报告为 v1.0.4 发布前质量把关。**结论——无阻断级问题，整体评级 B+，核心功能可发布**；但存在 1 个严重级功能缺陷 S1（per-user 安装路径矛盾），建议在发版前处理（修复或改文案）。

---

## 一、审查范围与方法
- **范围**：RightMenuX 全部 C++ 源码（管理器 exe + 内嵌 COM 服务器 DLL 源码 + 无头自检），覆盖内存安全、Win32 健壮性、安全性、兼容性、可维护性五维度。
- **文件清单**：
  - main.cpp（管理器 GUI / 自绘窗口 / 入口，1072 行）
  - features.cpp（Feature 数组 + 安装/卸载/图标生成/COM 释放，697 行）
  - explorercommand.cpp（通用 IExplorerCommand COM 服务器 + ToggleSuperHidden，445 行）
  - selftest.cpp（无头自检，486 行）
  - common.h / features.h / explorercommand.h / selftest.h
  - 参考：app.rc / shell.rc / resource.h / build.bat / version_stamp.py
- **方法**：静态逐行通读 + 跨文件接口一致性核对（注册表键路径、CLSID 一致性、COM vtable/QI）；对照全局规范（四段式版本方案、UAC 边界、MinGW 宽字符 %ls 约定）。属纯静态审查，未实际编译运行。

## 二、关键结论先行：阻断级问题
**未发现阻断级问题**（无崩溃、无内存泄漏、无接口对不上、无安全漏洞）。内存安全与 COM 引用计数审计结论为「干净」。
但发现 1 个**严重级功能缺陷**（S1，per-user 安装路径在干净机器上对标准用户不可用），以及若干一般/建议项。

---

## 三、发现列表（按严重级别）

### 【严重】S1 —— per-user（HKCU）安装路径在标准用户 + 干净机器上必然失败
- **位置**：features.cpp:252-281（`EnsureShellDll`）；features.cpp:679-682（`Feature_Install` 先调 `EnsureShellDll(FALSE)`）；main.cpp:931-946（失败回退 HKCU）
- **问题**：`Feature_Install` 一进来就调用 `EnsureShellDll(FALSE)`，该函数把 COM 服务器 DLL 释放到 `C:\Windows\RightMenuXShell.dll`（回退 `System32`），二者均要求管理员写权限。对标准用户，`CreateFileW` 两次都失败 → `EnsureShellDll` 返回 FALSE → `Feature_Install` 直接 return FALSE，**在写任何 HKCU 注册表之前就中止**。因此 main.cpp 的「启用失败 → 尝试 HKCU」回退（line 939）对标准用户永远进不去：DLL 落不了地，整个安装就放弃了。
- **风险/影响**：UI 明确承诺「未获管理员权限，仅当前账户生效」（main.cpp:939-946 文案），但在从未由管理员部署过 DLL 的干净机器上，该路径必然失败，用户会看到「启用失败」。本质矛盾：COM 服务器 DLL 必须置于 Explorer 可读取的位置（C:\Windows/System32，需提权），per-user 模式却假设无需提权。
- **建议修复（二选一）**：
  1. HKCU 模式改为把 DLL 释放到**用户可写且 Explorer（同用户上下文）可读取**的位置（如 `%LOCALAPPDATA%\RightMenuX\RightMenuXShell.dll` 或 exe 同目录），并让 `EnsureShellDll` 在 HKCU 场景优先该路径；或
  2. 若坚持只部署到系统目录，则 main.cpp 不要向用户承诺「仅当前账户生效」，直接提示必须以管理员身份运行（与现有弹窗文案一致），避免误导。

### 【一般】N1 —— SuperHidden 在 Win11 新菜单无图标
- **位置**：features.cpp:344（`SuperHidden` 的 `menuIcon = NULL`）；features.cpp:450-454（`RegisterWin11Command` 仅在 `GetFileAttributesW(outIco)` 成功时才写 `Icon`）
- **问题**：SuperHidden 未配置 `menuIcon`，故 `InstallRunCommand` 的图标生成逻辑（仅 RunCommand 走）不会为其产出 `.ico`；`RegisterWin11Command` 检测到 `outIco` 不存在便跳过 `Icon` 写入。结果 Win11 新菜单里 SuperHidden 项无图标。
- **风险/影响**：纯观感缺陷，不导致功能失效。
- **建议修复**：为 SuperHidden 配置一个内置图标（如复用 eye 线稿生成一个 `builtin:superhidden` 风格图标），或在 `RegisterWin11Command` 中给 ShellExtension 类也补一个默认图标，保证 Win11 菜单项有图。

### 【一般】N2 —— ResolveSystemFile 的 MAX_PATH 截断风险（潜在的越界读）
- **位置**：features.cpp:107-119（`ExpandEnvironmentStringsW(name, tmp, _countof(tmp))` 后 `wcschr(tmp, L'\\')` / `GetFileAttributesW(tmp)`）
- **问题**：`tmp` 为固定 `MAX_PATH` 缓冲。若环境变量展开后长度 > 260，`ExpandEnvironmentStringsW` 会截断且**不保证以 null 结尾**；随后 `wcschr`/`GetFileAttributesW` 可能读取越界。当前输入（compmgmt.msc / cmd.exe / mmc.exe / %SystemRoot%\...）展开后均 < 260，实际不触发，但属固定缓冲深目录截断隐患。
- **建议修复**：用 `ExpandEnvironmentStringsW` 的返回值判断是否 >= 缓冲大小，超长则改用动态分配（与 `SubstituteSelectionPath` 里用 `MAX_PATH*4` 的处理方式保持一致），或改用 `SearchPathW` 直接带输出缓冲探测。

### 【一般】N3 —— ExpandCmdForStore 缓冲不足时回退为「未展开」命令，可能复现 1ystemRoot 损坏
- **位置**：features.cpp:421-426（缓冲不足时 `StringCchCopyW(dst, n, src)` 原样保留含 `%SystemRoot%` 的命令）
- **问题**：当命令展开后超过 1024 字符（当前功能均远小于此，属极端情况），该函数放弃展开、原样复制含 `%SystemRoot%` 的串。而该工具最初修复的 bug 正是「Explorer 把 command 里的 `%S` 当格式符解析成 1ystemRoot」。若此回退被触发，旧 bug 会重现。
- **建议修复**：将 `expandedCmd` 缓冲放大（如 4096），或在回退时仍尝试用 `ExpandEnvironmentStringsW` 分段/动态展开，而非直接丢弃环境变量展开。

### 【一般】N4 —— 自检的 HKLM 探测与 HKCU 用例依赖「DLL 已落地 C:\Windows」
- **位置**：selftest.cpp:466-476（HKLM 探测 `Feature_Install(&g_features[0])`）；且所有 HKCU 用例经 `Feature_Install` → `EnsureShellDll`
- **问题**：自检的「安装成功」断言依赖于 `EnsureShellDll` 能把 DLL 写到 C:\Windows。在干净标准用户机器 / CI（未预先部署 DLL）上，这些用例会报 FAIL，但这是**环境问题而非逻辑错误**；此外 HKLM 探测若中途失败可能残留部分 HKLM 注册表键。
- **建议修复**：自检开头显式检查 `EnsureShellDll` 结果并在报告中标注「DLL 未部署」的环境结论，避免把环境失败误判为代码失败；HKLM 探测用 try/cleanup 包裹，失败也尽力回滚。

### 【一般】N5 —— selftest 的 freopen("CONOUT$") 未校验返回值
- **位置**：main.cpp:1008-1009
- **问题**：`freopen("CONOUT$", "w", stdout)` 返回值未判断。若当前 stdout 已绑定或重定向失败，控制台输出会静默丢弃（报告文件仍正常）。
- **建议修复**：`if (!freopen(...)) { /* 退化为仅文件日志 */ }`，或忽略其返回值但加注释说明「控制台输出为最佳努力」。

### 【建议】J1 —— SH_RefreshExplorer 使用了未文档化的 Explorer 内部 magic
- **位置**：explorercommand.cpp:324-355（`0x7103` 命令、`WM_KEYDOWN VK_F5` 发给 Progman / Shell_TrayWnd、EnumWindows 遍历 CabinetWClass）
- **问题**：`0x7103` 与向 Progman/Tray 发 F5 是社区流传的 Explorer 刷新 hack，跨 Windows 版本脆弱（未来版本可能失效）。代码已同时发送文档化的 `SHChangeNotify` + `WM_SETTINGCHANGE`。
- **建议修复**：保留文档化刷新为主路径；magic 刷新要么移除，要么按 `IsWindowsServer()`/build 号加版本门槛，并在失败时不依赖它。

### 【建议】J2 —— WriteIcoFromHICON 对 32bpp 颜色位图用 BI_RGB，alpha 可能不准
- **位置**：features.cpp:32-39（color `biCompression = BI_RGB`）、51（mask 同）
- **问题**：32bpp 图标 XOR 位图用 BI_RGB 读取/写出，部分图标 alpha 通道可能解析不正确（透明区域渲染异常）。`MakeTerminalIcon` 已强制 alpha=255 规避，但系统图标抽取时可能受影响。
- **建议修复**：32bpp 时改用 `BI_BITFIELDS` 并带上 alpha 掩码，或直接 `GetIconInfo` 后 `DrawIconEx` 到 DIB 以保真。

### 【建议】J3 —— MakeFont 每次绘制临时创建/销毁字体（无缓存）
- **位置**：main.cpp:183-187（`MakeFont`），各 Draw* 调用点
- **问题**：每个 WM_PAINT 为每项、每个文本调用 `CreateFontW`/`DeleteObject` 一次。作者已注释「无全局 HFONT 缓存，无句柄泄漏」——正确性没问题，但高频重绘下有不必要的 GDI 对象分配/释放开销。
- **建议修复**：可按 DPI 缓存一组常用字号字体（注意 WM_DPICHANGED 时重建），降低分配压力；当前可保持不变。

### 【建议】J4 —— GetOSDisplayString 不区分 32 位 Win10（项目声明仅 64 位）
- **位置**：common.h:145-161
- **问题**：仅按 major/build 判定 Win10/11/Server，未校验位数。若误在 32 位运行，与「64 位 Explorer 一致」的设计前提冲突。属边界提示，非 bug。
- **建议修复**：在文档/README 明确「仅支持 64 位」，或检测 `IsWow64Process` 给出提示。

### 【信息】—— 关于 _mcentry 双形态入口（explorercommand.cpp:426-444）
确认其被 `#ifndef BUILD_SHELL_DLL` 包裹；而正式 exe 构建（build.bat）**不编译 explorercommand.cpp**，DLL 构建带 `-DBUILD_SHELL_DLL`。故该历史存档代码不会进入任何正式产物，无副作用。

---

## 四、资源泄漏审计摘要（重点）
逐文件核查 GDI / 注册表 / 模块 / CoTaskMem / 文件句柄，**未发现泄漏**：
- **GDI 对象**：`DrawRoundedRect`/`StrokeRoundedRect`/`StrokeEllipse`/`MakeFont`/`DrawButton`/`DrawCard`/`DrawStatusBadge`/`DrawTitleButton`/`DrawFeatureIcon`/`MakeTerminalIcon` 等均在作用域内 `SelectObject` 还原旧对象后 `DeleteObject`；`GetStockObject(NULL_BRUSH)` 不释放（正确）。`SetWindowRgn` 由系统接管 HRGN 生命周期（注释已说明）。✅
- **注册表**：`common.h` 全部辅助函数均成对 `RegOpenKeyExW`/`RegCloseKey`；`RegDeleteTreeW` 把「键不存在」视为成功。✅
- **模块句柄**：`LoadLibraryW(dwmapi)`（main.cpp）均 `FreeLibrary`；selftest `LoadLibraryW(dllPath)` 对应 `FreeLibrary`。✅
- **CoTaskMem / SHGetKnownFolderPath**：`ReadClsidString`、`SubstituteSelectionPath`、`SHGetKnownFolderPath` 均正确 `CoTaskMemFree`。✅
- **文件句柄**：`WriteIcoFromHICON`/`EnsureShellDll`/selftest 报告的 `CreateFileW` 均有 `CloseHandle`；`WriteIcoFromHICON` 在失败两条路径都释放了缓冲与位图。✅
- **CreateProcess**：`RunCommandLine` 对成功路径 `CloseHandle(pi.hThread/hProcess)`。✅
- **COM 引用计数**：`CExplorerCommand`/`CClassFactory` 用 `InterlockedIncrement/Decrement`，`Release==0` 时 `delete this`；全局 `g_cLock` 在构造+1、析构-1，`DllCanUnloadNow` 判定正确，实例存活期间不会误卸载。✅

## 五、编译产物合规检查
- **版本号（四段式）**：✅ 合规。字符串版（FileVersion/ProductVersion/IDS_VERSION）统一写完整 `YYYYMMDD`；数值版 FILEVERSION/PRODUCTVERSION 第 4 段按设计用 `YYMMDD`（`260724`），因 Windows FILEVERSION 第 4 段是 16 位字（上限 65535），属有意设计，不是不一致。
- **版权资源**：✅ `CompanyName=重庆持玛多网络科技有限公司`、`LegalCopyright=持玛多网络 / Z.W.` 在 app.rc 与 shell.rc 均正确；界面版权行用 `LoadString(IDS_VERSION)` 拼版本，与资源一致。
- **单 exe 零依赖**：✅ build.bat 用 `-static`；COM 服务器编译为独立 `RightMenuXShell.dll` 并以 RCDATA 内嵌 exe。
- **UAC 清单**：✅ app.rc 嵌入 `RT_MANIFEST`（requireAdministrator）；shell.rc 故意无清单（可被 Explorer 进程内加载）。边界正确：DLL 内 `ToggleSuperHidden` 仅写 HKCU，无静默提权。
- **MinGW 宽字符**：✅ 版权行 `swprintf(..., L"...v%ls", verBuf)` 正确使用 `%ls`。

## 六、安全性结论
- **无提权/劫持漏洞**：DLL 落点 C:\Windows/System32 需管理员，标准用户无法预置；COM 服务器运行于 Explorer 用户上下文，仅写 HKCU。
- **命令注入**：CmdHere 的 `%V` 替换发生在模板引号内（`pushd "%V"`），NTFS 路径不含 `"` 与 `%VAR%` 语法，替换后无注入面；Win11 路径经 `SubstituteSelectionPath` 正确替换，深目录用堆串不截断。
- **注册表命令值**：仅由管理器（可信）写入；HKCU 下用户自定义 Command 仅在其自身上下文执行，不构成提权。

## 七、兼容性结论
- **Win10/11**：RunCommand 同时写 `command` 子键（经典菜单）与 `DelegateExecute`+`CLSID`（新菜单），`InstallShellExtension` 路径 A 还补了 `command` 子键作回退，双菜单均覆盖。✅
- **64/32 位**：统一 64 位（MinGW-w64），无 32 位 Explorer，无需 WOW6432Node 镜像。HKCU 安装已补 `Software\Classes` 前缀。✅
- **Server Core（无 Explorer）**：注册表写入成功但 COM 服务器不会被加载，属无害退化；建议文档注明。
- **DPI**：Per-Monitor V2（`WM_DPICHANGED` 重算布局），采用「先求和后 Scale」统一缩放，绘制与命中检测共用同一套坐标，避免错位。✅

---

## 八、总体质量评估
- **评级：B+（良好，可发布，但建议先修 S1 与 N1/N2）**。
- 代码质量整体较高：GDI 资源管控细致、COM 引用计数正确、注册表辅助函数健壮、宽字符与版本合规到位、双菜单兼容处理周全。主要短板在「per-user 安装路径的可用性假设」与少量边界缓冲处理。
- **可发布结论（针对 v1.0.4）**：**核心功能（管理员安装 HKLM）经审计可发布**；但 S1 属承诺功能与实际行为不符，建议发布前至少修复 S1（或在 UI 文案上去掉对标准用户的「仅当前账户生效」承诺），并顺手修 N1（SuperHidden 图标）、N2（MAX_PATH 截断）。N3/N4/N5 与 J 系列可在后续迭代处理。

### 发布前最该优先修的 Top 3
1. **S1**：修复 per-user 安装路径——要么把 DLL 落到用户可写的 `%LOCALAPPDATA%` 或 exe 同目录，要么不在 UI 承诺「仅当前账户生效」。
2. **N1**：给 SuperHidden 补一个菜单图标，避免 Win11 新菜单项无图。
3. **N2**：消除 `ResolveSystemFile` 的 MAX_PATH 截断越界隐患（动态缓冲或显式长度判断）。

（注：以上为静态审查结论，未实际编译运行；建议以 `--selftest` 在管理员 + 标准用户两种环境各跑一遍做动态验证后再发版。）
