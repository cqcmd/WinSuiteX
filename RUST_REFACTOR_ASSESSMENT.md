# WinSuiteX — 功能梳理与 Rust 重构可行性评估

> 评估日期：2026-09-14　|　目标：为 C++ → Rust 迁移做决策依据
> 当前形态：纯 C++ / Win32 GDI 自绘，MinGW-w64 g++ 构建，双产物（exe + COM dll）

---

## 0. 一句话结论

**这是一个「只读硬件信息展示 + 注册表右键菜单增强」工具，不是系统清理/优化工具。**
Rust 迁移的**最优切入点是硬件采集层与注册表层**（纯数据、无 UI 耦合、易测试），
**UI 层（3673 行 GDI 自绘 + 30+ 全局状态）是最大风险，应放到最后或保留 C++ 混合**。

---

## 1. 项目结构与模块职责

### 1.1 顶层文件地图

| 文件 | 行数 | 职责 | 是否参与构建 |
|---|---:|---|---|
| `main.cpp` | 3673 | **主入口 + 全部 GUI 绘制 + 磁盘页 + HTML 报告** | ✅ exe |
| `hardware.cpp` | 1724 | 硬件信息采集（SMBIOS/CPUID/SetupAPI/IOCTL/DXGI/COM/NVML/ADL） | ✅ exe |
| `features.cpp` | 981 | **右键菜单功能注册表增删改 + DLL 释放 + 图标生成** | ✅ exe |
| `selftest.cpp` | 857 | 无头自检（注册表结构 / 图标 / COM 导出验证） | ✅ exe |
| `explorercommand.cpp` | 444 | **COM 服务器**（`IExplorerCommand` + `ToggleSuperHidden` 导出） | ✅ dll（独立） |
| `common.h` | 181 | 注册表读写助手、OS 版本检测、Shell 刷新通知 | 头文件 |
| `hardware.h` | 144 | 硬件数据结构定义（`HardwareInfo` 等，定长数组） | 头文件 |
| `features.h` | 72 | `Feature` 结构体与 `FeatureKind` 枚举 | 头文件 |
| `selftest.h` / `explorercommand.h` | 12 / 27 | 自检接口 / COM IID 兜底声明 | 头文件 |
| `app.rc` / `shell.rc` / `resource.h` | 62 / 34 / 8 | 版本信息、清单、内嵌 DLL、图标、字符串表 | ✅ |
| `app.manifest` | — | `requireAdministrator` + Per-Monitor V2 DPI + 兼容性 GUID | ✅ exe |
| `version.ini` | 5 字节 | **仅构建期**版本号源（`1.6.7`），非运行时配置 | 构建期 |
| `glass_showcase*.cpp` ×3 | 1690 | **死代码**——实验性玻璃效果演示，未被任何构建脚本引用 | ❌ |

**核心源码约 9,700 行**（不含 thirdparty 与死代码）。

### 1.2 目录

| 目录 | 职责 |
|---|---|
| `thirdparty/libcpuid/` | C 语言 CPU 识别库（8 个 .c/.h），**用 gcc（非 g++）单独编译成 .o**，因其用 enum 隐式转 int 等 C 宽松语义 |
| `thirdparty/pci_ids/gpu_pci_table.h` | 静态 GPU PCI ID → 型号名查表（无驱动时识别显卡用） |
| `_archive/` | 8 个历史 exe，不参与构建 |
| `.workbuddy/` | 工作区记忆 |

### 1.3 双产物架构（关键约束）

```
WinSuiteX.exe      requireAdministrator 清单   GUI 管理器，内嵌 DLL 资源
    └─ RCDATA(200) 内嵌 → 运行时释放到 C:\Windows\WinSuiteXShell.dll（回退 System32）
WinSuiteXShell.dll asInvoker（无提权）          in-proc COM 服务器
```

> **根因约束**：带 `requireAdministrator` 的 exe **不能**作为 `InProcServer32` 被 Explorer 加载
> （v5.0 单文件方案因此失败）。这个约束在 Rust 下**依然存在**，与语言无关。

**COM 注册真相（与常见假设不同）**：

| 事实 | 说明 |
|---|---|
| `DllRegisterServer` / `DllUnregisterServer` | **空实现，只返回 `S_OK`**（`explorercommand.cpp:413-414`） |
| **全仓无 `regsvr32` 调用** | 注册全部由管理器直接写注册表完成 |
| `DllGetClassObject` | 对**任意 CLSID** 都返回同一个 `CClassFactory`，运行时按自身 CLSID 读 `HKCR\CLSID\{clsid}\Title\|Command\|Icon` |
| 实际导出符号 | 仅 5 个：`DllCanUnloadNow` / `DllGetClassObject` / `DllRegisterServer` / `DllUnregisterServer` / `ToggleSuperHidden`（objdump 验证，无 `@N` 后缀） |
| vtable 来源 | **非手写**——直接用 MinGW `shobjidl.h` 自带的 `IExplorerCommand` 定义，靠系统头保证布局 |

> ⚠️ **现存隐患**：`EnsureShellDll(FALSE, perUser)` 的 `force` 参数**恒为 FALSE**（`features.cpp:879`），
> 即已存在的 DLL **永不覆盖**——升级路径写好了却从未启用。Rust 重写时应明确版本比对策略。

### 1.4 入口点

| 入口 | 位置 | 说明 |
|---|---|---|
| `wWinMain` | `main.cpp:3558` | 唯一 exe 入口 |
| `DllMain` | `explorercommand.cpp` | COM 服务器入口 |

**命令行分派**（`main.cpp:3576-3620`）：

| 参数 | 行为 | 退出码 |
|---|---|---|
| （无） | 正常 GUI | 消息循环退出码 |
| `--selftest` | 自检 + 弹结果窗（分配 Console） | 失败项数 |
| `--selftest-nogui` | 无头自检，只写 `selftest_report.txt` | 失败项数（0=全通过） |
| `--report "<路径.html>"` | 无头采集 → 导出 HTML → 退出（**批量巡检用**） | 0 成功 / 2 失败 |

启动链路：`GdiplusStartup` → 注册 `NoticeProc` 弹窗类 → 参数分派 → 注册 `DialogProc` 主窗口类
→ `CreateWindowExW(WS_POPUP)` → 按真实 DPI 重算布局 + `ApplyRoundedRegion` → 消息循环
→ 退出时 `GdiplusShutdown` + 释放字体族与阴影缓存。

**UI 消息入口**：`DialogProc`（`main.cpp:2929`）处理 `WM_CREATE / WM_ERASEBKGND / WM_PAINT /
WM_NCHITTEST / WM_MOUSEMOVE / WM_MOUSELEAVE / WM_LBUTTONDOWN / WM_KEYDOWN / WM_DPICHANGED /
WM_TIMER / WM_DESTROY`；另有 `NoticeProc`（`main.cpp:787`）处理自绘提示弹窗。

---

## 2. 功能清单（当前**实际**实现）

### 2.1 三个标签页（`main.cpp`）

| 页 | 默认 | 内容 |
|---|---|---|
| **硬件配置** (`TAB_HW`) | ✅ | 只读展示 OS/计算机/主板/CPU/内存/GPU/硬盘/显示器/网卡/音频/USB |
| **右键增强** (`TAB_MENU`) | | 5 个右键功能的启用/停用卡片 + Win11 经典菜单开关 |
| **磁盘信息** (`TAB_DISK`) | | 分区簇大小/文件系统/容量/可用空间，选中看详情，一键复制，刷新 |

### 2.2 右键菜单功能（`features.cpp` 的 `g_features[]`，数据驱动）

| # | 功能 | 类型 | 目标位置 | 实现要点 |
|---|---|---|---|---|
| 1 | 计算机管理 | `RunCommand` | 此电脑右键 | `mmc.exe compmgmt.msc` |
| 2 | 设备管理器 | `RunCommand` | 此电脑右键 | `mmc.exe devmgmt.msc` |
| 3 | 显示/隐藏系统文件 | `ShellExtension` | 文件夹背景 | 调 DLL 导出 `ToggleSuperHidden()`，无 UAC |
| 4 | 命令提示符 | `RunCommand` | 文件夹/背景/桌面（**3 父路径**） | `cmd /s /k pushd "%V"`，需替换 `%V` |
| 5 | PowerShell | `RunCommand` | 文件夹/背景/桌面（**3 父路径**） | `powershell -NoExit Set-Location '%V'` |

**调用链**：
```
WM_LBUTTONDOWN (main.cpp:3387)
  └─ PtInRect(&g_btnRect[i]) 命中卡片按钮
       └─ Feature_Install / Feature_Uninstall (features.cpp:872 / 887)
            ├─ RunCommand     → InstallRunCommand  (628) / UninstallRunCommand (706)
            │                    └─ RegisterWin11Command (595) 写 CLSID + DelegateExecute
            └─ ShellExtension → InstallShellExtension (752) / UninstallShellExtension (830)
                                 └─ Command = "dll:ToggleSuperHidden"
       └─ ShowNotice 反馈 → NotifyShellChanged() 刷新资源管理器
```

**权限降级策略**：先试 `HKEY_CLASSES_ROOT`（机器级，需管理员），失败则回退 `HKEY_CURRENT_USER`（当前用户）。

### 2.3 硬件采集（`hardware.cpp`，**无 WMI**）

设计原则：**不依赖任何已安装驱动**（网维场景：无盘启动、刚装系统未打驱动）。

| 数据源 | API | 位置 | 采集内容 |
|---|---|---|---|
| **SMBIOS 固件表** | `GetSystemFirmwareTable('RSMB')` + 手工解析 | `149-265` | BIOS / 主板 / CPU / **每根内存条插槽** |
| **CPUID 指令** | `__cpuid_count` 宏 | `274` | CPU 品牌串、厂商 |
| **libcpuid**（FFI） | `cpuid_get_raw_data` / `cpu_identify` | `300-301` | 微架构代号（Ivy Bridge / Zen 3） |
| **SetupAPI** | `SetupDiGetClassDevsW` ×4 组 GUID | `439/714/912/1186` | 显示器、芯片组、GPU 真名、音频/USB 枚举 |
| **DXGI** | `CreateDXGIFactory1` / `IDXGIAdapter1::GetDesc1` | `735-740` | 显卡独立显存 |
| **磁盘 IOCTL** | `DeviceIoControl` `IOCTL_STORAGE_QUERY_PROPERTY` + `IOCTL_DISK_GET_LENGTH_INFO` | `1373-1450` | 型号/序列号/总线类型/容量/SSD 判定 |
| **COM（音频）** | `MMDeviceEnumerator` + `IPropertyStore` | `1267-1303` | 默认播放/录音设备 |
| **NVML**（动态加载） | `LoadLibraryW("nvml.dll")` + 9 个 `GetProcAddress` | `954-1043` | N 卡温度/风扇/功耗/利用率/显存/时钟 |
| **ADL**（动态加载） | `LoadLibraryW("atiadlxx.dll")` / `atiadlxy.dll` | `1061-1105` | A 卡温度/显存/活跃度 |
| **ntdll** | `GetProcAddress("RtlGetVersion")` | `1479` | 真实 OS 版本（避开 manifest 说谎） |
| 其他 | `GetSystemInfo` / `GlobalMemoryStatusEx` / `GetAdaptersAddresses` | — | 逻辑处理器数 / 内存总量 / 网卡 MAC+IP |

调用链：`wWinMain`/`--report` → `HW_Collect(&g_hw)`（`hardware.cpp:1490`）
→ 各 `Collect*` 函数 → 填充 `HardwareInfo` → `DrawHardwarePage` 或 `Report_BuildHtml`。

> 任何一项失败都降级为空串/"未知"，**不崩溃**——这个容错契约迁移时必须保留。

### 2.4 其他已实现功能

| 功能 | 位置 | 说明 |
|---|---|---|
| HTML 报告导出 | `Report_BuildHtml` `main.cpp:2019` | 全量明细导出到桌面并自动打开；`--report` 可无头批量巡检 |
| 磁盘分区信息 | `EnumDisks` `main.cpp:1196` | 簇大小换算式、盘符→PhysicalDriveN 映射、未分区盘展示 |
| Win11 经典菜单切换 | `main.cpp:~3480` | 增删 `HKCU\Software\Classes\CLSID\{86ca1aa0-...}` |
| 一键复制到剪贴板 | `CopyToClipboardW` `main.cpp:608` | 硬件/磁盘页文本 |
| 自检 | `SelfTestMain` `selftest.cpp` | 详见 §5 |
| 旧版残留清理 | `MigrateCleanupLegacy` `features.cpp:947` | 启动静默执行 |

### 2.5 **未实现**（ROADMAP 候选，勿误判为现有功能）

- ❌ 系统清理 / 垃圾清理 / 启动项管理 / 进程管理 / 服务管理
- ❌ 更多管理工具（磁盘管理、服务、事件查看器、组策略）
- ❌ 系统外观个性化（暗色模式、任务栏对齐）
- ❌ 设置持久化与导入导出
- ❌ 托盘常驻

---

## 3. 依赖、配置、IO、并发、平台相关性

### 3.1 外部依赖

| 类别 | 内容 |
|---|---|
| 第三方源码 | **仅 libcpuid**（静态编译进 exe，零运行时依赖） |
| 系统 DLL（静态链接） | shell32, shlwapi, advapi32, user32, ole32, uuid, setupapi, iphlpapi, kernel32, dxgi, dxguid, gdiplus |
| 系统 DLL（**动态加载**） | `nvml.dll`（N 卡驱动自带）、`atiadlxx.dll`/`atiadlxy.dll`（A 卡驱动）、`dwmapi.dll`、`ntdll.dll` |
| 网络 / 第三方服务 | **无** |
| 数据库 | **无** |

### 3.2 配置项

**无运行时配置文件**。所有状态持久化在**注册表**：

| 注册表位置 | 用途 |
|---|---|
| `HKCR\CLSID\{20D04FE0-3AEA-1069-A2D8-08002B30309D}\shell\*` | 此电脑右键（= HKLM 提权） |
| `Directory\shell`, `Directory\Background\shell`, `DesktopBackground\shell` | 文件夹/背景/桌面右键 |
| `HKCR\CLSID\{<每功能 Win11 CLSID>}` | Win11 新菜单 `DelegateExecute` + Title/Command/Icon |
| `HKCU\Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}` | Win11 经典菜单开关 |

`version.ini` 仅构建期，`version_stamp.py` 读取 → 写入 `.rc` 与 `IDS_VERSION`。

### 3.3 文件 IO

| 操作 | 位置 | 路径 |
|---|---|---|
| 写 HTML 报告 | `main.cpp:2311` | `CSIDL_DESKTOPDIRECTORY`（桌面）或 `--report` 指定（支持 UNC） |
| 写自检报告 | `selftest.cpp` | 工作目录 `selftest_report.txt` |
| **释放 DLL** | `features.cpp:386` `EnsureShellDll` | `C:\Windows`（回退 `System32`），per-user 时 `%LOCALAPPDATA%\WinSuiteX` |
| 写图标 .ico | `features.cpp:301-330` | `%LOCALAPPDATA%` / `%COMMONAPPDATA%` 图标存储目录 |
| 读物理磁盘 | `main.cpp:1087/1162/1261` | `\\.\PhysicalDriveN` |
| 读卷 | `main.cpp:1087` | `\\?\Volume{GUID}` |

### 3.4 并发 / 异步

**几乎没有**——这是迁移的重大利好：

- ❌ 无 `CreateThread` / 无线程池 / 无 `std::thread`
- ❌ 无锁、无原子操作（COM 侧 `InterlockedIncrement` 除外）
- ✅ 仅 `WM_TIMER`（`main.cpp:3414/3433` 设 1.3s，`3536` 处理）——清除"已复制/已导出"反馈
- 所有采集（`HW_Collect` / `EnumDisks`）**在 UI 线程同步执行**，靠启动快掩盖耗时

> 迁移含义：Rust 侧不需要 async runtime，不需要 `Send`/`Sync` 设计，单线程模型可直接照搬。
> 但**若未来想把采集挪到后台线程**，则 `HardwareInfo` 需满足 `Send`，而定长数组 POD 天然满足。

### 3.5 平台相关代码（全部 Windows）

| 领域 | 具体点 |
|---|---|
| 窗口 | `WS_POPUP` 无边框 + 自绘阴影/边框 + `ApplyRoundedRegion` 圆角裁剪 + `WM_NCHITTEST` 拖拽 |
| DPI | Per-Monitor V2（`WM_DPICHANGED`），全局 `Scale(v) = MulDiv(v, g_dpi, 96)` |
| GDI+ | `GdiplusStartup`/`Shutdown`、`Graphics`/`LinearGradientBrush`/`GraphicsPath`、自实现 `BoxBlur` 软阴影 |
| 双缓冲 | `CreateDIBSection`（top-down, 32bpp）+ 全窗 `BitBlt`（`main.cpp:2980-2998`、收尾 `~3200`） |
| DWM | **仅** `DwmSetWindowAttribute(DWMWA_WINDOW_CORNER_PREFERENCE=33)`（动态 `LoadLibrary("dwmapi.dll")` + `GetProcAddress`）<br>**无** `DwmExtendFrameIntoClientArea` / `SetWindowCompositionAttribute` / `UpdateLayeredWindow` |
| COM | `IExplorerCommand`（Win11）、`MMDeviceEnumerator`（音频） |
| 清单 | `requireAdministrator` + `dpiAware true/pm` + 4 个 `supportedOS` GUID |

> ### 🎉 对迁移最关键的一条结论
> **「玻璃拟态」不是真 DWM 玻璃，而是 100% 自绘**：紫→蓝 `LinearGradientBrush` 渐变底
> + GDI+ 半透白填充 + 自实现 `BoxBlur` 软阴影 + 1px 半透白描边。
> 全仓**没有** `DwmExtendFrameIntoClientArea` / `SetWindowCompositionAttribute` / `UpdateLayeredWindow`，
> 也没有 `WM_NCCALCSIZE`。
>
> 这意味着：**UI 层不依赖任何 DWM 合成器行为，纯 GDI/GDI+ 绘制指令，可在 Rust 中逐条等效重写**，
> 不需要复刻 DWM 的时序与宿主窗口结构（这是很多"玻璃拟态"项目迁 Rust 失败的主因）。
> 剩下的难度只在"绘制代码量大 + 全局状态多"，属于**工程量**问题，不是**技术不可行**问题。

---

## 4. Rust 迁移高风险点

### 4.1 🔴 全局可变状态（最大风险）

`main.cpp` 有 **30+ 个文件级 `static` 全局**，且被绘制/消息处理交叉读写：

```
g_dpi, g_gdipToken, g_fontFam, g_shadowCount, g_pg
g_cardRect[], g_btnRect[], g_hover[], g_count, g_totalH, g_clientW/H
g_curTab, g_tabRects[], g_tabHover[], g_tabNames[]
g_hw (HardwareInfo), g_hwCollected
g_disks[], g_diskCount, g_selDisk, g_hoverDiskRow, g_hoverDiskRefresh, g_diskRowRect[]
g_hwPlan[], g_hwPlanCount, g_hwRowH, g_hwSecGap
g_minBtnRect, g_closeBtnRect, g_minHover, g_closeHover
g_notice (NoticeCtx), g_copyAtTick, g_copyBtnRect, g_copyBtnHover, g_mouseTracking
g_osString, g_hAppIcon, g_hInst
explorercommand.cpp: g_cLock, g_hInst
```

Rust 对策（按推荐度）：
1. **推荐**：收敛为一个 `AppState` struct，放进窗口 `GWLP_USERDATA`（或 `windows-rs` 的
   `CreateWindowExW` + `SetWindowLongPtr`），消息处理里取回 `&mut`。
2. 次选：`thread_local! { static APP: RefCell<AppState> }`——Win32 窗口本就线程亲和，
   `RefCell` 的运行时借用检查反而能抓到重入 bug（如绘制中触发 `InvalidateRect`）。
3. ❌ 不用 `lazy_static Mutex<...>`：单线程下只会引入死锁风险与噪音。

### 4.2 🔴 对象生命周期与所有权

| 风险点 | 位置 | 说明 |
|---|---|---|
| **`g_pg` 悬垂裸指针** | `main.cpp:146`，`3006` 赋值、`WM_PAINT` 末尾置 NULL | 指向**栈上** `Graphics g(hdc)`，仅 `WM_PAINT` 内有效。Rust 里直接转成 `&Graphics` 即可，但**所有用到 `g_pg` 的 `Draw*` 函数（约 10 处）都要改成显式传参** |
| GDI 对象手动释放 | 全 `Draw*` 函数 | `CreatePen`/`MakeFont` 后 `DeleteObject`；有 `SelectObject` 保存旧对象模式。<br>**实测配平**：`CreatePen`×18 + `CreateSolidBrush`×12 + `MakeFont`×28 = **58 创建 / 58 `DeleteObject`，无泄漏**。<br>但**每帧全量重建**（字体每帧新建再销毁），迁移时应改为**全局字体缓存 + DPI 失效时重建**（顺带解决 `g_hwRowH` 自适应抖动） |
| GDI+ 阴影缓存 | `g_shadows[8]`，`Bitmap*` | 手动 `delete`，`ClearShadows()` 在 DPI 变更时调用。Rust 用 `Option<Bitmap>` 数组 |
| COM 引用计数 | `explorercommand.cpp:210-217`、`CoTaskMemFree` 多处 | 手写 `AddRef/Release`；`GetTitle` 返回 `CoTaskMemAlloc` 内存交 Explorer 释放 |
| **`DiskInfo::typeName` 裸指针** | `main.cpp:~315` | `const WCHAR* typeName` **指向静态字符串**，非拥有。Rust 用 `&'static str` 或 `Cow` |
| NVML/ADL 函数指针 | `hardware.cpp:978-989`, `1067-1074` | 一堆 `PFN_*` 裸函数指针 + `LoadLibrary`/`FreeLibrary` 配对 |
| **`ShowNotice` 嵌套模态循环** | `main.cpp:919`, `NoticeProc` `787` | 用 `GetMessage(NULL,...)` 泵**全线程**消息直到 `g_notice.hWnd==NULL`；<br>**单例 `NoticeCtx` 全局**——嵌套/重入调用会互相覆盖，破坏性退出才清标志。<br>Rust 应改为**真模态**（`IsDialogMessageW` / `DialogBoxParamW`），彻底移出"自绘禁用 + 嵌套泵"模式 |
| **`g_hw` 约 40 KB 静态存储** | `main.cpp:298` | `HardwareInfo` 含大量定长数组（8×`HwCpu` + 16×`HwMemStick` + 8×`HwDisk` + …）。<br>栈/静态区都偏大，Rust 应 `Box::new` 或按需 `Vec` |

**绘制期硬约束（照搬会踩的坑）**：`WM_PAINT` 中**必须先 `BitBlt` 再 `EndPaint`**
（`main.cpp:~3200` 有 v1.0.9 真机 bug 注释）——`EndPaint` 后 `BeginPaint` 返回的 DC 已释放，
再画不会上屏。Rust 重写时这条顺序约束必须用类型或注释固化。

### 4.3 🟠 FFI 边界

| 边界 | 现状 | Rust 方案 |
|---|---|---|
| **libcpuid（C）** | 仅 **2 个 FFI 函数**（`cpuid_get_raw_data` / `cpu_identify`）+ **2 个 POD 结构体**（`cpu_raw_data_t` / `cpu_id_t`）<br>实测 `cpu_id_t` **无 union、无位域**，只有 `int` + `char[…]` → **可直译 `#[repr(C)]`**<br>汇编面仅 `__cpuid` / `__cpuid_count` 两个宏 | ✅ **FFI 面极小，是三个 FFI 里最省力的**。`build.rs` + `cc` crate 编译（正好替代"gcc 单独编译 .o"技巧）。<br>❌ **不能用 `raw-cpuid` / `sysinfo` 替代**：它们不输出微架构代号，<br>而代号库（**实测 471 条**：Intel 261 + AMD 210）正是 v1.2.5 的核心价值 |
| **COM `IExplorerCommand`** | 手写 C++ 类继承，实现 9 个方法 + `IClassFactory` | `windows` crate 有 `IExplorerCommand` 定义，可用 `#[implement]` 宏；**需实测宏生成的 vtable 顺序与 Explorer 期望一致** |
| **NVML / ADL** | 手写 `typedef` 函数指针 + `GetProcAddress` | `libloading` crate，或继续手写 `unsafe extern "system"` 声明 |
| **GDI+** | C++ class 封装（`Graphics`, `Bitmap`, `FontFamily`） | `windows-rs` 的 GDI+ 覆盖**不完整**（`Graphics`/`Bitmap` 部分缺失）。可能需自建 FFI 薄封装 |
| **`--kill-at`** | MinGW 链接参数，stdcall 名称修饰 | Rust `cdylib` 导出需用 `#[no_mangle] extern "system"`，注意符号名一致 |
| 🔴 **SMBIOS 手工解析**（**最大 unsafe 面**） | `hardware.cpp:105-263`：`#pragma pack(1)` 结构体 + **柔性数组** `BYTE data[]` +<br>直接转型读表尾字符串；Type 17 按 **v1.2 `38` / v1.2+ `40`** 双长度解析 | 失败点全是 **UB 而非优雅降级**（未对齐读取 + 裸指针偏移）。<br>Rust 需 `#[repr(C, packed(1))]` + `read_unaligned()` + `from_utf8_lossy`；<br>**建议独立出 `parse_smbios(&[u8]) -> Result<…>` 纯函数，用真实 dump 做 fixture 单测** |
| **GPU PCI 查表** | `thirdparty/pci_ids/gpu_pci_table.h`，实测 **2223 条**，`GpuPciLookup` 二分查找 | 迁移到 `phf` / 静态切片；条数需与上游 libpciids 重新核对（无版本戳） |

### 4.4 🟠 动态特性 / 反射

- **无反射、无 RTTI、无动态代码生成**——C++ 侧很干净。
- 唯一的"动态"是 `DllGetClassObject` 对**任意 CLSID** 都返回同一通用 COM 对象
  （`explorercommand.cpp:19-26` 注释），运行时按自身 CLSID 读注册表拿 Title/Command/Icon。
  这是**数据驱动**设计，Rust 照搬即可，无技术障碍。
- `GetProcAddress(g_hInst, fnA)` 在 DLL 内查找自身导出（`explorercommand.cpp:381`）——
  用于 `dll:ToggleSuperHidden` 进程内调用。Rust 等效实现需注意导出名稳定。

### 4.5 🟠 强依赖特定运行时生态

| 项 | 说明 |
|---|---|
| **MinGW vs MSVC ABI** | 当前用 MinGW g++。Rust 有 `x86_64-pc-windows-gnu` 与 `-msvc` 两种 target。<br>**必须统一**：若 Rust 用 gnu 链接 MinGW 编的 libcpuid .o 可行；混用 msvc 则需 libcpuid 也用 MSVC 编。<br>**建议：Rust 选 `x86_64-pc-windows-gnu`，与现有 MinGW 保持一致** |
| **自绘 UI 无 Rust 等价生态** | 3687 行 GDI 自绘，Rust 无成熟"GDI 自绘控件"库。若引入 egui/iced/GTK<br>**会违背"零依赖单 exe + GDI 自绘"的技术偏好**（见用户 UI 偏好） |
| **构建期 Python** | `version_stamp.py`、`gen_icon.py`、`rename_outputs.py` 参与构建。<br>Rust 可用纯 `build.rs` 替代，但**版本号方案（四段式 + 日期戳）需按现有规范复刻** |

### 4.6 🔴 测试覆盖缺口

`selftest.cpp`（857 行）是**现成的回归测试雏形**，但覆盖严重偏斜：

| 已覆盖 ✅ | 未覆盖 ❌ |
|---|---|
| 注册表结构（Install/Uninstall 到 HKCU 验证） | **硬件采集 `HW_Collect` 全部逻辑**（无一条断言） |
| command 展开（防 `%SystemRoot%` 损坏） | SMBIOS 解析、libcpuid 代号、PCI 查表 |
| Icon 文件结构与字段（ICONDIR/BITMAPINFOHEADER） | `EnumDisks` 磁盘枚举（IOCTL 路径） |
| COM 导出存在性 + `GetTitle` 运行时验证 | **UI 绘制 / 布局 / DPI 缩放 / 命中测试（完全无测试）** |
| Win11 CLSID 注册结构 | GDI+ 渲染、阴影缓存、双缓冲 |
| 多父路径注册（regParent2/3） | 报告 HTML 内容正确性（只验证落盘） |

> **迁移策略**：把 `selftest.cpp` 的 ~80 个 `Check()` 断言**直接转成 Rust `#[test]`**，
> 这是最省事的测试基线。同时**必须补硬件采集的单元测试**（SMBIOS 解析可用 fixture 字节流做纯函数测试）——
> 这是当前最大的质量缺口，也是迁移的最大收益点。

### 4.7 其他风险

| 风险 | 说明 |
|---|---|
| **定长数组 POD 无处不在** | `HardwareInfo` 全是 `WCHAR[128]` 等定长数组，`HW_MAX_*` 宏封顶。<br>Rust 需 `Vec<T>` + 显式截断，或保留定长 `[u16; N]` 以维持内存布局。字符串编码 UTF-16 ↔ `String` 转换要统一在边界 |
| **字符串编码三态** | `char*`（libcpuid/SMBIOS）、`wchar_t*`（Win32）、UTF-8（HTML 输出）。现有代码手工转换 |
| **`wcsstr(lpCmdLine, L"--selftest")`** | 参数解析用子串匹配，非精确解析。`--report` 手工解析引号。Rust 可用 `std::env::args` 更健壮 |
| **C 风格布尔** | `BOOL` (int) / `TRUE`/`FALSE` 满天飞，Rust `bool` 需逐处转换 |
| **资源编译** | `.rc` → Rust 用 `winres` / `embed-resource`；内嵌 DLL 用 `include_bytes!` |
| **释放 DLL 到 C:\Windows** | 需管理员 + 文件占用处理（Explorer 已加载时替换会失败），现有代码有回退但逻辑分散。<br>⚠️ 附带**安全与杀软声誉风险**（往系统目录写 DLL）。Rust 重写建议**默认 per-user `%LOCALAPPDATA%`**，机器级作为显式选项 |
| **中英双语硬编码匹配** | 芯片组/网卡/USB 过滤用 `wcsstr(name, L"主机桥")` 之类**中文子串**匹配（`hardware.cpp:722/1156/1186`）。<br>迁移时**必须原样保留双语**，否则中文系统识别率下降（这是网维场景的刚需） |
| **定长语义建议保留** | 全 UI 依赖 `buf[0] == L'\0'` 判"未采集到"。<br>第一阶段**不要**改成 `Option<String>`，改用 `ArrayString<N>` 或自建 `FixedStr<N>` 保持 `[0]` 语义，避免波及全部渲染代码 |
| 🐛 **疑似笔误（迁移前确认）** | `hardware.cpp:1100` 在 `CollectGpuAdl` 中 `g->vramMB = 0;`，注释却说"留 DXGI 值"。<br>疑似应为"不覆盖"。迁移前需与 Z.W. 确认真实意图，否则会**把 bug 一起翻译过去** |

**采集能力边界（避免迁移时误以为有）**：本模块**无** SMART/健康度、无 MSR、
无 CPU 温度/风扇、无电池。GPU 遥测仅来自 NVML/ADL（需厂商驱动），无驱动时全为 0。

---

## 5. 重构优先级与难度评估

### 5.1 模块评估矩阵

| 模块 | 行数 | 难度 | 耦合度 | 可测性 | 优先级 | 建议 |
|---|---:|---|---|---|---|---|
| **`hardware.cpp`** | 1724 | 🟡 中 | **低**（仅依赖 Win32 API + libcpuid） | **高**（纯函数，可 fixture） | **P0 最先** | ✅ 独立 crate |
| **`features.cpp` + `common.h`** | 1162 | 🟢 中低 | 中（被 UI 调用） | 中（注册表可测） | **P1** | ✅ 独立 crate |
| **`selftest.cpp`** | 857 | 🟢 低 | 低 | — | **P1** | ✅ 转成 `#[test]` |
| **`explorercommand.cpp`** | 444 | 🔴 中高 | **低**（独立 DLL） | 低（需 Explorer） | **P2** | ⚠️ 独立 `cdylib` crate，最后验证 |
| **`hardware.cpp` 的 libcpuid FFI** | — | 🟡 中 | — | 中 | **P2** | `build.rs` + `cc`/`bindgen` |
| **`main.cpp` — 硬件页/磁盘页数据层** | ~1200 | 🟡 中 | 中 | 中 | **P3** | 数据部分可先抽 |
| **`main.cpp` — GDI 自绘 UI** | ~2470 | 🔴 **极高** | **高**（30+ 全局状态） | **极低** | **P4 最后** | ❌ 建议暂缓或混合 |

### 5.2 推荐迁移顺序

#### ⚠️ 前置：迁移前必须先修的两件事

| # | 问题 | 状态 |
|---|---|---|
| 1 | **`build.bat` 缺 `-lgdiplus`** → cmd 下链接失败（`main.cpp` 用 24 处 GDI+ 符号且无 `#pragma comment` 补链）。现有 exe 是用 `build_git.sh` 编的，故未暴露。**没有可用的基线构建，就无法做新旧对拍** | ✅ **已修复**（本次评估中） |
| 2 | **确认 `hardware.cpp:1100` 的 `g->vramMB = 0;`** 是否为笔误 | ⏳ 待 Z.W. 确认 |

#### 里程碑 0 — 建立对拍基线（**最高性价比**）

**先单独迁移 `--report` 无头路径**：它只依赖 `HW_Collect` + `EnumDisks` + `Report_BuildHtml`，
**完全不碰 GUI**。完成后即可：

```bash
WinSuiteX.exe       --report old.html     # C++ 版
winsuitex.exe       --report new.html     # Rust 版
diff <(...) <(...)                        # 逐字段对拍
```

> 这是**唯一能在没有任何 UI 代码的情况下验证正确性**的手段，
> 建议作为第一个可交付里程碑——约覆盖硬件层 100% 的输出面。

#### 阶段 1 — 数据与逻辑层

1. `winsuitex-hardware` crate：`HardwareInfo` 等数据结构 + SMBIOS/CPUID/SetupAPI/IOCTL 采集
2. `winsuitex-registry` crate：注册表读写助手 + `Feature` 模型 + Install/Uninstall
3. 把 `selftest.cpp` 的断言转成 `#[test]`；**补齐硬件采集单测**（SMBIOS dump 做 fixture）

**阶段 2 — FFI 与 COM**
4. libcpuid 通过 `build.rs` + `cc` 编译（或直接 `raw-cpuid` + 自建代号表）
5. `winsuitex-shell` crate（`cdylib`）：`IExplorerCommand` + `ToggleSuperHidden`

**阶段 3 — UI（谨慎）**
6. 若坚持 GDI 自绘：需自建 GDI+ 安全封装，`Graphics`/`Bitmap` 的 RAII 是重点
7. 或接受混合方案：**Rust 做数据层，C++ 保留 UI 层**，用静态库/`cdylib` 边界对接

### 5.3 适合**先行独立迁移**的判断

✅ **强烈建议独立先迁**：`hardware.cpp` + `common.h` + `features.cpp`
- 理由：纯数据采集/注册表操作，**零 UI 耦合**，输入输出明确（系统 → POD / 注册表），
  可用现有 `--report` 做客观对拍验证，且能顺带补上当前最缺的硬件层测试。
- 这三个模块约 **3,000 行**，占核心代码 31%，但**规避了 100% 的 UI 风险**。

⚠️ **建议暂缓**：`main.cpp` 的 GDI 自绘（2,470 行 + 30+ 全局状态）
- 理由：无成熟 Rust 生态支撑，工作量与风险不成正比；且这部分**目前零测试**，
  迁移等于在没有安全网的情况下重写。

### 5.4 关键技术决策（需 Z.W. 拍板）

| 决策点 | 选项 A | 选项 B | 备注 |
|---|---|---|---|
| **Rust toolchain** | `x86_64-pc-windows-msvc`（**推荐**，需装 VS Build Tools） | `x86_64-pc-windows-gnu` | **msvc 更优**：`windows-rs` 官方支持最好，GDI+ flat API 现成，`ComCtl32 v6`（`SetWindowSubclass` 等）无坑，Rust 独有 `windows_subsystem` 属性天然替代 `-mwindows`；libcpuid 用 `cc` crate 编译，不需要 MSVC 项目文件。<br>选 gnu 的理由只有"与现有 MinGW 一致"，但需实测 GDI+ 覆盖度 |
| **UI 层** | 保留 C++ UI + Rust 数据层（混合） | 全量 Rust 重写 GDI 自绘 | 混合方案风险低得多；全量重写需评估 UI 一致性成本 |
| **UI 框架（若全量重写）** | 坚持 GDI 自绘（自建封装） | 引入 egui/iced | **注意**：引入 GUI 框架违背现有"零依赖单 exe + GDI 自绘"偏好 |
| **libcpuid** | 保留 FFI（`build.rs` + `cc`） | 换 `raw-cpuid` + 自建代号表 | 代号库是核心价值（**471 条**：Intel 261 + AMD 210），替换需重造数据。<br>FFI 面仅 2 函数 2 结构体，保留成本极低 |
| **命令行参数**（`-municode` 对应） | `std::env::args()`（`std` 内部走 `GetCommandLineW`）+ `#![windows_subsystem = "windows"]` | 手写 `wWinMain` | Rust 无需 `-municode`；但 **`--report` 的引号解析要重写**（现为 `wcsstr` 取子串，易误判），建议直接用 `args()` 逐项匹配 |
| **资源编译** | `build.rs` + `winres`/`embed-resource`；**内嵌 DLL 改用 `include_bytes!`** | 继续 `.rc` + `windres` | 用 `include_bytes!` 可**消除"必须先编 DLL 再编 exe"的构建顺序耦合**，同时避开 `.rc` 的 `code_page` UTF-8 乱码坑（中文版本信息） |
| **产物体积** | 单 exe（Rust 静态链接，约 1–3 MB） | 维持现状 820 KB | Rust 产物偏大，需实测 `opt-level="z"` + `strip` + `panic=abort` |

---

## 6. 迁移时必须保留的硬约束（不可妥协）

1. **单 exe 双击即跑，零运行时依赖**（网吧/电竞酒店分发场景）
2. **exe 与 COM DLL 必须分离**——带 `requireAdministrator` 的模块无法被 Explorer 作为
   `InProcServer32` 加载（与语言无关的系统约束）
3. **DLL 释放到 `C:\Windows` 需管理员 + 处理文件占用回退**
4. **硬件采集"失败降级不崩溃"契约**——任何一项拿不到数据都退回空/未知
5. **版本号四段式 `主.次.修订.编译日期戳`**（见用户编译规范）
6. **版权信息**：`重庆持玛多网络科技有限公司 / Z.W.`，`LegalCopyright` 用简称
7. **命令行契约不能变**：`--selftest` / `--selftest-nogui` / `--report` 的退出码语义
   （0=成功 / 2=失败 / 失败项数）——已有自动化脚本依赖它做批量巡检

---

*本报告基于 2026-09-14 的代码静态分析。所有行号引用对应当日版本（version.ini `1.6.7`，
app.rc `1.6.7.20260914`）。未实际执行构建或运行测试。*

---

## 7. 团队复核结论（2026-09-18 第二轮机审）

> 本轮由一个 15 人评估 team 推进（挂起后续作 6 个只读 Explore）。目的：把 §1–6 的"规划"与
> "已落地现实"对齐，并补上磁盘通电/健康度专项对拍（df1/df2 真机）。
> 同时修正一处会污染全篇的错误前提（§7.1）。

### 7.1 🔴 重大纠偏：Explore-2 的"无 Rust 工程"前提不成立

Explore-2 报告首句称"本 checkout 无 Rust 工程，无法读取 Rust 源码"，据此把"Rust 现状"整列
标为"推断/缺口"。**实测否决该前提**：

- `rust/` 下存在 4 个 crate（`winsuitex-core/platform/report/cli`）+ 5 个 `Cargo.toml`，全部 `.rs` 模块齐备；
- `model.rs:478/514` 的 `SmartSource`/`DiskSmart` 与 Explore-2-71ed 引用逐字一致；
- `rust/dist/winsuitex.exe` 已部署（与 `dist/winsuitex.exe` 同源，版本 `0.1.0.20260920`）。

**结论**：Explore-2 的 **C++ 侧能力清单（hardware.cpp 行号）可信、予以保留**；其
"Rust 现状"整列不可信，本节用真实文件树 + `ARCHITECTURE.md` 重做覆盖映射（§7.2）。
该 agent 的 C++ 勘察质量本身没问题，只是读到了一个损坏/空的 Rust 视图。

### 7.2 当前真实覆盖度（Rust 模块 → C++ 能力，已实测文件树核对）

| C++ 能力（来源） | Rust 对应模块 | 状态 |
|---|---|---|
| OS/系统（registry/system） | `registry.rs` + `system.rs` | ✅ 产品名/核数/Server/计算机名/用户 |
| SMBIOS BIOS/主板/CPU/内存条（149-265） | `smbios.rs`(core+platform) | ✅ `GetSystemFirmwareTable`+`RSMB`，`packed(1)`+`read_unaligned`+fixture 单测 |
| CPUID + libcpuid 代号（274/300） | `cpuid.rs` | ✅ `build.rs`+`cc` 编 libcpuid，保留 471 条代号库 |
| GPU 型号/显存（468/735） | `gpu.rs` | ✅ DXGI `GetDesc1` + GDI `EnumDisplayDevicesW` 合并去重 |
| **GPU 遥测 NVML/ADL（954-1105）** | `gpu.rs` | ❌ **缺口**：gpu.rs 检索无 `nvml`/`adl` 引用，温度/风扇/功耗/利用率/时钟未做 |
| 显示器 EDID/型号/厂商/尺寸（341-466） | `display.rs`+`edid.rs` | ✅ EDID 解析 + SetupAPI 物理尺寸 |
| 网卡 MAC/IP/虚拟过滤（515-560/1156） | `nic.rs` | ✅ `GetAdaptersAddresses` + 双语虚拟网卡黑名单 |
| USB/音频（1185-1233） | `usb.rs` / `audio.rs` | ✅ audio 走 COM `IMMDeviceEnumerator`+RAII `CoInitialize` |
| 磁盘型号/接口/介质/容量（1373-1450） | `physical_disk.rs` | ✅ IOCTL 三查 |
| **磁盘 SMART/通电/健康** | `physical_disk.rs` | ✅ **超越 C++**（C++ 零实现） |
| 注册表/右键功能 | `registry.rs`+core `feature.rs` | ✅ `FeatureKind` 枚举 + `&[Feature]` 切片已落地 |
| HTML 报告 | `report/lib.rs` | ✅ |
| 自检 `--selftest` | `cli` | ✅ 退出码契约一致 |
| **COM Shell 扩展（WinSuiteXShell.dll）** | — | ⏳ 未做（阶段三决策点） |
| **GUI（3 tab GDI 自绘）** | `winsuitex-ui` | ❌ 未做（阶段三，仅架构，见 §6） |

> 覆盖判定方法：除逐文件核对函数签名外，对"是否真实现"做了定向检索——
> `gpu.rs` 含 `CreateDXGIFactory1`/`GetDesc1`/`EnumDisplayDevicesW`（型号+显存实做），
> 但**无** `nvml`/`adl`（遥测确为缺口）；`smbios.rs` 含 `GetSystemFirmwareTable`+`RSMB`；
> `nic.rs` 含 `GetAdaptersAddresses`+`VIRTUAL_NIC_PATTERNS`；`audio.rs` 含
> `IMMDeviceEnumerator`+`CoInitializeEx` RAII guard；`display.rs` 含 `parse_edid`+SetupAPI。

### 7.3 六份报告要点

- **Explore-1（main.cpp）**：GDI+ 自绘、3 tab、玻璃=纯自绘渐变+BoxBlur（**无 DWM 依赖**，可逐条等效重写）；
  唯一真模态 `ShowNotice` 嵌套消息泵是重构点。建议 `--report` 无头路径为首里程碑（已落地）。
- **Explore-4（COM+构建+版本）**：`build.bat` 缺 `-lgdiplus` 已在首轮评估中修复；COM 可绕开用
  `rundll32`/`CreateProcess`；建议 msvc target（Rust 实际 `x86_64-pc-windows-msvc`，`ARCHITECTURE.md` 确认）。
- **Explore-1-2c03（FFI 审计）**：FFI 安全度 **HIGH**，唯一真实崩溃 bug（hid.rs 堆破坏）已修，
  余 B1-B4 为加固非崩溃。生产就绪。
- **Explore-3（features）**：5 功能，SuperHidden/CmdHere/PowerShellHere 为网维刚需，DeviceManager 演示可砍；
  Rust 设计 `FeatureKind`+`&[Feature]` 切片（`ARCHITECTURE.md §3.3` 已落地）。
- **Explore-2（hardware.cpp）**：C++ 能力清单完整（**无 WMI**，靠 SMBIOS+CPUID+SetupAPI+DXGI+IOCTL+COM+NVML/ADL）。
  Rust 现状列不可信（见 §7.1）。
- **Explore-2-71ed（磁盘 POH/健康）**：C++ 零 SMART（`HwDisk` 无字段、权限模型拦死）；Rust 四路径+三兜底已落地；
  **VMD NVMe 通电时间硬伤**（NVMe log-page 主路径被拦 → 回退 WMI，WMI `PowerOnHours` 常返回 0 → `None`，
  如实标注"未透传"）；**需管理员权限**运行才能拿 SMART 字段。

### 7.4 修订结论

原 §5 的"先数据层后 UI"路线**已被执行并验证**：Rust 数据+CLI 层已落地、已部署、已对拍
（`rust/dist/verify/` 含 cpp↔rust 对拍报告 `compare_result.txt` / `wim/compare_result.txt`）。
**剩余风险收敛为 3 点**：

1. 🔴 **UI 层（winsuitex-ui）**：3687 行 GDI 自绘 + 30+ 全局状态，0 测试。阶段三。
2. 🟠 **Shell COM DLL**：`WinSuiteXShell.dll`（`IExplorerCommand` + `ToggleSuperHidden`）未迁，决策点。
3. 🟠 **GPU 遥测 NVML/ADL**：确认缺口，需补温度/风扇/功耗/利用率。

已知诚实短板：VMD NVMe 通电时间（维持"未透传"标注，不建议做厂商私有通道）。

### 7.5 待 Z.W. 拍板

| 决策 | 选项 A | 选项 B |
|---|---|---|
| UI 路线 | 混合（Rust 数据 + C++ UI，风险低） | 全量 Rust GDI 自绘（一致性高，风险高） |
| Shell DLL | 保留 C++ DLL | Rust `cdylib` |
| VMD 诊断提示 | 报告加"建议关 VMD 以读 NVMe 完整 SMART" | 不加 |
| GPU 遥测 | 补 NVML/ADL（需厂商驱动，无驱动仍 None） | 不补 |
| 产物体积 | Rust 静态链接（约 1–3 MB） | 维持 820KB 预算 |

### 7.6 8 个实现 worker 与 live 代码重叠说明

首轮 team 分配的 8 个实现 worker（report/gpu/nic/usb/disk/cpu 模块 + 数处 bug 修复）
**本轮未续作**：其目标模块在 live 工程已存在并部署，重跑会与已上线代码冲突制造回归。
评估只用 6 个只读 Explore 的结论，故无回归风险。这也是为何 §7.2 覆盖度用的是"实测文件树"
而非"worker 自报"——更可靠。

*§7 基于 2026-09-18 的 6 份只读 Explore 报告 + 直接文件树核对（含 `df1`/`df2` 真机对拍）。*

### 7.7 团队 Lead 建议（续作答复，标"建议/待 Z.W. 确认"）

> 判定口径：两条硬约束 —— ① 单 exe 双击即跑、GDI 自绘零依赖（网吧/电竞酒店分发）；
> ② 双目标（写出好软件 + 往"多赚钱"靠：更好巡检服务 = 更高客单价/复购）。

| 决策 | 我的建议 | 关键理由 |
|---|---|---|
| UI 路线 | **混合（Rust 数据层 + 保留 C++ UI）** | `main.cpp` 3687 行 GDI 自绘 + 30+ 全局状态 + 0 测试；全量 Rust 重写风险高、收益低。混合方案不破"零依赖单 exe"偏好，且数据层已实测落地、对拍通过 |
| Shell DLL | **暂保留 C++ `WinSuiteXShell.dll`** | 双产物硬约束与语言无关（带 `requireAdministrator` 的 exe 不能作 `InProcServer32`），C++ DLL 已上线验证可工作；Rust `cdylib` 重写收益小、风险大，留作阶段三 |
| VMD 诊断提示 | **加** | 低成本；`df1`/`df2` 已实锤 NVMe POH 在 VMD 下拿不到（IOCTL 拦 87 + WMI 计数器给 0）。诚实提示帮客户/运维判断"是否要关 VMD"，契合"更好服务 = 多赚钱" |
| GPU 遥测 | **补 NVML/ADL** | 电竞酒店 GPU 温度/风扇/功耗是核心巡检指标；无厂商驱动时仍回 `None`、不造假；补齐直接提升巡检价值与客单价 |
| 产物体积 | **接受 Rust 静态链接 1–3 MB** | 网吧分发不在乎体积，单 exe 双击即跑即可；820KB 是 C++ 约束，Rust 静态链接无功能副作用 |

**下一步（待用户确认"照此推进"后）**：按"先数据层后 UI"已验证路线，本期可落里程碑 = ① GPU 遥测补 NVML/ADL；② VMD 诊断提示文案（建议关 VMD 以读完整 NVMe SMART）；③ 产物体积放开到 1–3MB。UI 层与 Shell DLL 维持 C++ 混合，不进入本期重构范围。
