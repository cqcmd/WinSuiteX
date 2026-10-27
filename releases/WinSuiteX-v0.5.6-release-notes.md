# WinSuiteX 硬件概览面板 v0.5.6

> 面向网吧 / 电竞酒店的本地硬件巡检工具 · 纯 Rust + GDI 自绘单 exe

## 本版重点：全面兼容 Windows 7 SP1+

修复 v0.5.5 在 Win7/2008 R2 上「无法定位程序输入点 GetSystemTimePreciseAsFileTime」加载即崩的问题。**自本版起，GUI 与 CLI 两个产物均为单一 exe 通吃 Windows 7 SP1 ~ Windows 11**，老机器不再需要挑版本：

| 兼容关 | 修法 |
|--------|------|
| std 运行时（`api-ms-win-core-synch`）+ UCRT（`api-ms-win-crt-*`） | `x86_64-win7-windows-msvc` 目标 + `-Z build-std` + `+crt-static` 静态 CRT |
| `combase.dll`（Win8+ 才有） | `CoTaskMemFree` 运行时转发到 `ole32.dll` |
| `GetSystemTimePreciseAsFileTime`（Win8+，被模块整块链接连坐引入） | 运行时探测真函数，Win7 回退 `GetSystemTimeAsFileTime` |
| DPI 五符号（Win10 专有，`SetProcessDpiAwarenessContext` 等） | 运行时探测，Win7 回退 `SetProcessDPIAware` / 96 DPI |

- Win10/11 上行为不变：垫片优先走系统真实函数，仅在其不存在时回退。
- 精度说明：Win7 上时间戳退化为普通系统时间精度，对硬件巡检场景无影响。

## 核心特性（延续 0.5.5）

- **单 exe 零依赖**：约 860KB，双击即跑；无 dll、无 GPU、无运行时，VM / RDP 远程会话均可正常显示。
- **4 套皮肤体系**：深空玻璃 / 流畅灰紫 / 琥珀暖调 / 薄荷青竹，共享 token 层保证视觉一致。
- **圆角 AA 抗锯齿 + 控件 ZSUI 复刻**：3× 超采样圆角、开关 / 页签药丸 / WinUI 按钮度量。
- **三页硬件详情**：菜单 / 硬件 / 磁盘——CPU·主板·内存·显卡·网卡·音频·SMART 健康徽标·9 列磁盘明细。
- **命令行伴侣**：`winsuitex.exe` 支持采集中间格式（`--json`）与报告渲染（`--report` / `--render`）。

![4 套皮肤预览](https://github.com/cqcmd/WinSuiteX/releases/download/v0.5.5.20261023/skins-preview.png)

## 版本与发布

| 项 | 值 |
|----|----|
| 版本号 | v0.5.6.20261027（四段式：主.次.修订.日期戳） |
| 发布日期 | 2026-10-27 |
| 构建产物 | `winsuitex-gui-gdi.exe`（GUI 面板）+ `winsuitex.exe`（命令行 v0.1.0） |
| 质量门禁 | fmt 干净 · clippy -D warnings 零警告 · 366 tests 通过 · 资源校验 PASS · PE 导入表零 Win8+ 符号 · Win7 真机实测通过 |

## 使用说明

1. 下载并解压 `WinSuiteX-GUI-v0.5.6.20261027.zip`。
2. 双击 `winsuitex-gui-gdi.exe` 打开面板（Win7 首次启动若被 SmartScreen 拦截，点「仍要运行」即可，exe 未签名）。
3. `winsuitex.exe --help` 查看命令行用法；`inventory.bat` 式批量采集可参考仓库 `dist/` 内脚本。

## 版权

重庆持玛多网络科技有限公司 / Z.W.
