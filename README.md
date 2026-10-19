# WinSuiteX

[![Release](https://img.shields.io/github/v/release/cqcmd/WinSuiteX?label=%E6%9C%80%E6%96%B0%E7%89%88%E6%9C%AC&color=2563EB)](https://github.com/cqcmd/WinSuiteX/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/cqcmd/WinSuiteX/total?label=%E4%B8%8B%E8%BD%BD%E9%87%8F&color=16A34A)](https://github.com/cqcmd/WinSuiteX/releases)
[![Platform](https://img.shields.io/badge/%E5%B9%B3%E5%8F%B0-Win10%20%2F%2011%20%2F%20Server-0078D7)](https://github.com/cqcmd/WinSuiteX/releases/latest)

Windows Windows 套件，纯 C++ / Win32 GDI 自绘，单 exe 双击即跑，无运行时依赖。

**📦 [下载最新版](https://github.com/cqcmd/WinSuiteX/releases/latest)**（解压后以管理员身份运行）

> **⚠️ 两条产品线，版本号独立编排（2026-10 起）**：
> - **C++ 版**（1.7.x 线）自 2026-09-26 起**冻结**，只作只读参照，不再更新；下方「当前版本」一行是它的最后状态。
> - **Rust 线是当前主推**：面板 `winsuitex-gui-gdi.exe`（0.4.x 线，最新 **v0.4.8.20261019**）+ 命令行 `winsuitex.exe`（0.1.0.x 线，最新 **v0.1.0.20261019**），Rust workspace 五 crate，纯 Rust + GDI 自绘单 exe（零 dll、零 GPU）。Rust 线各轮变更见 [CHANGELOG](CHANGELOG.md) 顶部「Rust 重写线」。

当前版本（C++ 冻结线最后状态）：**v1.1.4.20260910**（v1.0.12 真机目测 hotfix：内存行间距 / 显示器加型号 / 音频同时显播放+录音）　※ 本地已构建；远程 Release 待网络恢复后发布

## 功能

主窗口顶部三个标签页：

- **硬件配置**（默认页）：只展示**常规信息**，一眼看完本机配置。
  - 常规字段：操作系统 / 计算机 / 主板 / 处理器 / 内存 / 显卡 / **硬盘** / 显示器 / 网卡 / 音频 / USB 外设。
  - **驱动未装也能识别**（SMBIOS 固件表 + CPUID 指令 + SetupAPI 设备树 + 磁盘 IOCTL，无盘 / 刚装系统也能看到正确型号）。
  - 硬盘信息经 `\\.\PhysicalDriveN` 的 `IOCTL_STORAGE_QUERY_PROPERTY` + `IOCTL_DISK_GET_LENGTH_INFO` 读取，可区分 NVMe / SATA / SAS / USB 与 SSD / HDD，**不依赖任何驱动或 WMI**。
  - 顶部「**导出报告**」把全部明细（BIOS、每根内存插槽、每颗 CPU、全部网卡 MAC/IPv4/IPv6、每块硬盘、每个 USB 外设、分区与簇大小、右键功能启用状态）导出为一份 HTML 到桌面并自动打开。
  - Server 系统自动隐藏音频与 USB 外设两段；音频 / USB 只列**真实识别到的硬件**，识别不到就整段不显示（不写"未知"占位）。
- **右键增强**：下表 4 个右键菜单功能的启用 / 停用开关。
- **磁盘信息**：磁盘工具，列出所有分区的簇大小、文件系统、总容量与可用空间；点选某行可在下方看到该分区的簇大小换算式（每簇 N 扇区 × M 字节/扇区）与已用百分比；右上角「刷新」可重新枚举（插拔 U 盘后可见），右侧「一键复制」可带走全文。原独立工具「磁盘簇字节大小查看器」已并入本页。

| 功能 | 目标位置 | 说明 |
|------|----------|------|
| 计算机管理 | 此电脑右键 | 打开 `compmgmt.msc`（计算机管理） |
| 设备管理器 | 此电脑右键 | 打开 `devmgmt.msc`（设备管理器） |
| 命令提示符 | 文件夹 / 桌面空白 | 打开终端并 `pushd` 进入当前目录 |
| 显示 / 隐藏 系统文件 | 文件夹背景 | 一键切换资源管理器对受保护系统文件的显隐（SuperHidden 模块） |
| 硬件配置 | 主窗口「硬件配置」页 | 常规硬件信息（驱动未装也能识别），驱动零依赖 |
| 硬件报告导出 | 「硬件配置」页右上角按钮 / `--report` 参数 | 全量明细导出为 HTML（UTF-8），保存到桌面并自动打开 |
| 磁盘信息 | 主窗口「磁盘信息」页 | 各分区簇大小 / 类型 / 文件系统 / 总容量 / 可用空间，点行看详情，一键复制 |

> Win11 新菜单直显：计算机管理 / 设备管理器 / 命令提示符 通过通用 `IExplorerCommand` COM 服务器直绘入 Win11 新右键菜单，无需「显示更多选项」；SuperHidden 模块经文件夹背景右键生效。重启资源管理器后全部生效。

## 架构

混合架构（v5.1）：

- **`WinSuiteX.exe`**：管理器 GUI，带 `requireAdministrator` 清单（写 `HKLM` 弹 UAC）。
- **`WinSuiteXShell.dll`**：进程内 COM 服务器（`IExplorerCommand` + SuperHidden 切换），**无提权清单**（asInvoker）。以资源 RCDATA(200) **内嵌进 exe**，启用功能时由 `EnsureShellDll()` 释放到 `C:\Windows\WinSuiteXShell.dll`（失败回退 `System32`）。
- Explorer 以中完整性级别运行，无法加载带提权清单的 InProcServer32，故 COM 服务器必须独立且无提权清单。

> ⚠️ 关键根因：带 `requireAdministrator` 的 exe 不能作为 InProcServer32 被 Explorer 加载（v5.0 单文件方案因此失败），故 v5.1 拆回独立 DLL。

## 兼容性

- 系统：Windows 10 / 11 / Server 2016 / 2019 / 2022（64 位）。
- 64 位产物与 64 位 Explorer 位数一致，无 WOW64 镜像注册问题。
- 必须带「桌面体验」角色（Server Core 无 Explorer）。
- 实测：Windows Server 2022 (Desktop Experience) 右键项正常显示与工作。

## 构建

需要 MinGW-w64（64 位）与 Python 3。在 Git Bash 中：

```bash
cd WinSuiteX
export PATH="/c/msys64/mingw64/bin:$PATH"
PY="python.exe"   # 或你的 python 路径
"$PY" version_stamp.py && \
windres -i shell.rc -o shellres.o && \
g++ -O2 -Wall -DBUILD_SHELL_DLL -shared -finput-charset=UTF-8 -DUNICODE -D_UNICODE \
    explorercommand.cpp shellres.o -o WinSuiteXShell.dll -lshell32 -lshlwapi -ladvapi32 -luser32 -lole32 -luuid -static -Wl,--kill-at -Wl,--dynamicbase,--nxcompat -s && \
windres -i app.rc -o res.o && \
g++ -O2 -Wall -mwindows -municode -DUNICODE -D_UNICODE -finput-charset=UTF-8 \
    main.cpp features.cpp selftest.cpp hardware.cpp res.o -o build_main.exe -lshell32 -lshlwapi -ladvapi32 -luser32 -lole32 -luuid -lsetupapi -liphlpapi -lkernel32 -static -Wl,--kill-at -Wl,--dynamicbase,--nxcompat -Wl,--gc-sections -s && \
"$PY" rename_outputs.py && \
./WinSuiteX.exe --selftest-nogui
```

## 命令行参数

| 参数 | 说明 |
|------|------|
| `--selftest` | 跑完整自检并弹结果窗（日志写入 `selftest_report.txt`） |
| `--selftest-nogui` | 无头自检，只写文件 + 返回退出码（0=全通过），适合 CI |
| `--report "<路径.html>"` | **无头导出 HTML 硬件报告**，不弹任何窗口，写文件后退出（0=成功 / 2=失败）。可放进开机脚本或域脚本做批量巡检 |

批量巡检示例（把各机房机器的硬件报告集中到共享目录）：

```bat
WinSuiteX.exe --report "\\server\share\reports\%COMPUTERNAME%.html"
```

构建脚本 `build.bat` 等价于上述流程（版本号采用四段式 `主.次.修订.编译日期戳`，每次编译自动递增修订号并写入当天日期戳，详见 CHANGELOG）。

## 自检

`WinSuiteX.exe --selftest-nogui` 无头运行，报告写入 `selftest_report.txt`，验证 COM 服务器导出、图标结构、注册表写回等。

## 目录说明

```
main.cpp            GUI 与主窗口绘制
features.cpp        图标生成、COM 服务器释放、注册表操作
explorercommand.cpp COM 服务器（IExplorerCommand / ToggleSuperHidden），仅编进 DLL
selftest.cpp        无头自检
common.h            公共工具（注册表读写等）
*.rc / resource.h   资源与版本
version_stamp.py    版本号自增
rename_outputs.py   重命名中间产物为 WinSuiteX.exe
build.bat           一键构建
```
