# WinSuiteX 单机软件 UI 设计方案（预案，待审定）

> 状态：仅设计方案，未动手写代码。范围已于 2026-09-23 更正：**产品 = 单机软件**（多站 = 用户发来的 49 台一次性分析批次，健康报告是其交付物，非产品功能）。C++ 管理器 GUI 按代码分工红线**冻结**，本文 UI 均为 Rust 侧产物。

---

## 0. 范围确认（重要更正）

- **产品形态**：单机软件。窗口内显示**主要硬件信息紧凑摘要**；详细信息**导出网页（HTML 报告）**查看。
- **界面分工**（用户的原话）：窗口有尺寸限制 → 能合并的字段合并到一行；深度细节走网页。
- **Rust 侧现状**：采集 / 注册表右键(`feature.rs`) / 自检(`--selftest`) / HTML 报告 已具备。本次 UI 设计对象 = **窗口内的紧凑摘要面板**（管理器 GUI 的概览部分）；GPU 遥测 NVML/ADL、COM Shell 右键扩展按需后补。

---

## 1. 三层界面（磁盘项突破两层，其余仍两层）

| 层 | 载体 | 内容 | 交互 |
|----|------|------|------|
| 概览面板 | 软件窗口（原生，窄） | 主要硬件 + 整体健康状态 + 一键导出/打开网页报告 | 浏览 + 触发导出 |
| 磁盘钻取 | 面板内弹窗（新增，§7） | 选中磁盘的全量 SMART / 健康 / 序列号 / 来源 | 点击或右键磁盘行触发 |
| 详情报告 | `inventory.html`（已有） | 每槽/每盘 SMART、EDID、网卡、外设全量 | 浏览器查看 |

---

## 2. 面板布局（按你定的顺序，多硬件逐个列出）

窗口采用**纵向分类列表**，顺序固定为（与截图一致，UI 内**用图标代替数字编号**）：

**系统 → 处理器 → 主板 → 内存 → 显卡 → 显示器 → 磁盘 → 声卡 → 网卡**

每类左侧一个**分类图标**（见 §2.1），不再显示 1~9 数字。

**多项设备逐条列出**（不合并隐藏）：2 颗 CPU、7 块盘、多网卡都各自占一行，窗口可滚动。每类默认显示字段见 §3。详情（完整 SMART 属性、EDID、全字段）仍在导出的 `inventory.html`；磁盘另提供面板内钻取窗（§7）。

### 2.1 分类图标映射（9 个，编译进二进制）

| 类 | 图标含义 | 资源 |
|----|----------|------|
| 系统 | 窗口/Windows 标 | `assets/icons/system.svg` |
| 处理器 | 芯片（方+引脚） | `assets/icons/cpu.svg` |
| 主板 | 板卡（插槽网格） | `assets/icons/board.svg` |
| 内存 | 内存条（缺口） | `assets/icons/ram.svg` |
| 显卡 | 卡+风扇圆 | `assets/icons/gpu.svg` |
| 显示器 | 显示器 | `assets/icons/monitor.svg` |
| 磁盘 | 硬盘（盘体+孔） | `assets/icons/disk.svg` |
| 声卡 | 喇叭 | `assets/icons/audio.svg` |
| 网卡 | 网络（地球/端口） | `assets/icons/nic.svg` |

图标实现：SVG 源 → 构建期 `include_bytes!` 嵌进 exe（egui 用 `egui::Image` / `Context::load_texture` 加载为纹理，绑 `TextureHandle`）；不依赖外部文件、不引 icon 字体 CDN。草图 §5 已用简化形状示意，正式图标按 §2.1 含义设计。

---

## 3. 每类显示字段（默认集，可逐项增减）

| # | 类 | 显示字段（来自 JSON） | 多项规则 |
|---|----|----------------------|----------|
| 1 | 系统 | OS 名称+版本+构建 · 在线时长 · 开机时间 | 单行 |
| 2 | 处理器 | 每颗：型号(+核心/线程) | 每颗一行 |
| 3 | 主板 | 厂商/型号(占位→`—`) · BIOS 厂商+版本 | 单行 |
| 4 | 内存 | 总容量·已插N/空槽M；每根已插：容量+频率+厂商+(槽位) | 仅列已插，空槽不逐行显示（只计空槽数） |
| 5 | 显卡 | 每块：名称+显存+(遥测温度/功耗)+BMC标注 | 每块一行 |
| 6 | 显示器 | 每台：名称(清洗)+分辨率+刷新率+(尺寸) | 每台一行 |
| 7 | 磁盘 | 每块：介质+接口+型号+容量+健康色+重映射/待映射+通电+温度+(序列号未透传标注) | 每块一行（重点） |
| 8 | 声卡 | 每块：名称；0→无 | 每块一行 |
| 9 | 网卡 | 每块：描述+MAC+IPv4+速率+(up状态) | 每块一行 |

字段来源：全部已在 `info.*` 实采（含 `info.board`）。**内存频率 = `info.memory.sticks[].speed_mhz`**（SMBIOS Type17 / WMI 双路实采，单位 MT/s，已具备，无需改采集层）。主板 `vendor/product`=`To be filled by O.E.M.` 等占位串 → **渲染层清洗为 `—`**（不动 JSON/core，守老纪律）。

---

## 4. 空值 / 无法评估 处理规范（沿用，先定死）

| 场景 | 面板呈现 | 状态色 |
|------|----------|--------|
| 主板 `vendor`/`product`=`To be filled by O.E.M.` 等占位 | 显 `—` | 灰 |
| 内存固件回声空槽（`vendor="Dimm1_Manufacturer"`等） | 显「空槽」，不计入已插，容量`—` | 灰 |
| 磁盘 `serial`/`iface`/`media` 空（VMD/老控制器） | `未透传 —` | 灰 |
| `boot_at`/`uptime_seconds`=None | `— / 未采集` | 灰 |
| SMART 全 None | `数据缺失` | 灰（⚪） |
| GPU=BMC/无盘适配器 | `BMC` | 绿 |
| `health_status=Warning`+少量重映射 | `预警` | 橙 |
| 重映射≥50 / 待映射>0 / 介质错误>0 | `紧急` | 红 |

**状态色铁律**：🔴红=紧急 / 🟠橙=预警 / ⚪灰=无法评估 / ✅绿=健康。诚实空值优先于伪造填充。

---

## 5. 草图（见对话内联 SVG：9 类纵向面板）

> 下方 SVG 用真实「千雪电竞(中央广场4幢25楼)」数据绘制，窗口窄、可纵向滚动，多硬件逐条列出。状态色：🔴紧急 / 🟠预警 / ⚪灰=未评估 / ✅绿=健康（SVG 内以实心圆点代替 emoji）。

```
┌─ WinSuiteX — 硬件概览 ──────────────────┐
│ [图标] 系统                                              │
│   Windows Server 2022 Datacenter 21H2                  │
│   在线 —/未采集 · 开机 —/未采集        (灰)            │
│ [图标] 处理器                                           │
│   Intel Xeon E5-2680 v3 · 12 核 / 24 线程              │
│ [图标] 主板                                             │
│   — (厂商/型号未填) · BIOS American Megatrends P8SN016  │
│ [图标] 内存 · 64 GB · 已插 2 / 空槽 6                   │
│   DIMM1  Samsung 32 GB · 2400 MT/s                     │
│   DIMM3  Samsung 32 GB · 2400 MT/s                     │
│   (空槽 6 不逐行显示，仅计入空槽数)                     │
│ [图标] 显卡                                             │
│   Microsoft Basic Render Driver · 16 MB                │
│   ASPEED Graphics Family (WDDM) · BMC      (绿)        │
│ [图标] 显示器                                           │
│   Generic Non-PnP Monitor · 1920×1080 @ 60Hz           │
│ [图标] 磁盘 · 7 块                          (重点)     │
│   ● SanDisk SSD PLUS 120GB · SATA · SSD · 120 GB (绿)  │
│   ● SanDisk SSD PLUS 120GB · SATA · SSD · 120 GB (绿)  │
│   ● INTEL SSDSC2KB019TZ · SATA · SSD · 1.7 TB (橙预警)│
│   ● ST6000NM0115 · SATA · HDD · 5.5 TB · 重映射105632  │
│     /待映射120                              (红紧急)    │
│   ● INTEL SSDPE2KX040T8 · NVMe · SSD · 3.7 TB (绿)    │
│   ● INTEL SSDPE2KX040T8 · NVMe · SSD · 3.7 TB (绿)    │
│   … 其余 1 块 → 见网页                    (灰)         │
│   (点击/右键某盘 → 打开磁盘详情窗，见下)                │
│ [图标] 声卡                                             │
│   无（未检测到）                            (灰)        │
│ [图标] 网卡 · 2                                         │
│   Intel 82599 10 Gigabit Dual Port · 10 Gbps           │
│   Intel 82599 10 Gigabit Dual Port (2) · 10 Gbps       │
├────────────────────────────────────────────┤
│ 整体健康：●1 紧急 · ●1 预警   [打开网页报告 →]        │
└────────────────────────────────────────────┘

磁盘详情窗（点击/右键 ST6000NM0115 触发）：
┌─ 磁盘详情 — ST6000NM0115 ──────────────┐
│ 介质 HDD · 接口 SATA · 容量 5.5 TB        │
│ 健康: ● 紧急                              │
│ 重映射扇区: 105632            (红)        │
│ 待映射扇区: 120              (红)         │
│ 介质错误:   120              (红)         │
│ 通电时长:   ~60000 h (≈6.8 年)           │
│ 温度:       — / 未采集        (灰)        │
│ 序列号:     未透传 —          (灰)        │
│ 数据来源:   S.M.A.R.T.                  │
│ [打开网页报告 →]                         │
└─────────────────────────────────────────┘
```

---

## 6. 下一步（实现计划范围，已拍板）

审定后出实现计划，范围包含：
- Rust 原生概览面板（**egui 即时模式，已锁定 2026-09-23**）：零运行时依赖、单 exe 双击即跑，符合网吧/电竞酒店分发场景；UI 内容参考 C++ 管理器但不照搬。
- 9 类分类图标（SVG 源编译进 exe，§2.1）。
- **面板内行右键菜单**（点击/右键硬件行触发）：磁盘行 → 定位该盘 HTML / 复制序列号 / 导出单盘快照；其余类行 → 复制该行文本（参考 C++ 磁盘页「选中详情 + 一键复制」交互）。
- **注册表右键启动器**（`feature.rs`，参考 C++「右键增强」页，见 §6.1）：已有功能不丢，功能清单对齐 C++。
- 磁盘钻取窗（§7，参考 C++「磁盘信息」页，见 §7）。
- HTML 报告复用、字段绑定、空值组件。

多站聚合仅作为你发数据时的**一次性分析脚本**（`audit_servers.py` / `health_report.py`），不进产品。

### 6.1 右键功能（参考 C++「右键增强」页 `g_features[]`）

C++ 管理器在 Explorer 右键注册以下功能（Rust `feature.rs` 对齐，不丢）：

| 功能 | 右键位置 | 命令/类型 |
|------|----------|-----------|
| 计算机管理 | 此电脑右键 | `compmgmt.msc` (RunCommand) |
| 设备管理器 | 此电脑右键 | `devmgmt.msc` (RunCommand，演示可扩展) |
| 显示/隐藏 系统文件 | 文件夹空白处 | SuperHidden 切换 (ShellExtension DLL) |
| 命令提示符 | 文件夹/空白处/桌面右键 | `cmd /k pushd "%V"` |
| PowerShell | 文件夹/空白处/桌面右键 | `powershell -Command Set-Location '%V'` |
| （可扩展）磁盘管理/服务/本地用户和组 | 此电脑右键 | `diskmgmt.msc` / `services.msc` / `lusrmgr.msc` |

参考要点：功能以「模块数组」追加式扩展（`features.cpp`）；Win11 新菜单用委托 CLSID 直显；每个功能带菜单图标（系统 msc/ exe 真实图标或内置终端图标）。Rust 侧 `feature.rs` 复用同一套注册表父路径（`Directory\shell` / `Directory\Background\shell` / `DesktopBackground\shell` / 此电脑 CLSID）。

## 7. 磁盘详情钻取窗（参考 C++「磁盘信息」页）

- **触发**：面板磁盘区点击某盘行，或右键 →「磁盘详情」。
- **C++ 磁盘信息页内容（参考，非照搬）**：7 列表格 `盘符/PhysicalDrive/文件系统/簇大小/类型/总容量/可用` + 选中详情 `型号/总线(busType)/序列号/卷标/簇(每簇扇区×字节/扇区)/文件系统` + 「一键复制」纯文本摘要 + 刷新按钮。它偏**卷/分区**视角。
- **Rust 磁盘详情 = C++ 卷/分区内容 ⊕ Rust SMART 健康块（超集）**：
  - 卷/分区（需**补充采集**，见下）：盘符 · PhysicalDrive 映射 · 文件系统 · 卷标 · 簇大小 · 类型 · 总容量 · 可用空间
  - 物理盘 + SMART（Rust 已有）：介质 / 接口 / 型号 / 容量 / 总线 / 序列号
  - 健康状态 + 严重度（红/橙/绿/灰 同 §4 铁律）
  - 重映射扇区 / 待映射扇区 / 介质错误（坏道扩散判据）
  - 通电时长（`power_on_hours`）+ 折算年
  - 温度（`temperature_c`，无则灰）· 磨损（`percentage_used`，SSD）
  - 数据来源（`source`：S.M.A.R.T. / WMI / 不可得原因 `unavailable_reason`）
- **⚠️ 采集缺口（唯一新增成本）**：当前 Rust `info.disks[]` 只有 SMART/物理盘字段，**缺盘符/文件系统/可用空间/簇大小/卷标**。要复刻 C++ 磁盘信息内容，需在 Rust 采集端**新增卷/分区枚举**：`GetLogicalDriveStrings` → `GetDriveType`/`GetDiskFreeSpace`(可用/簇) → `DeviceIoControl(IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS)`(盘符→PhysicalDrive) + `IOCTL_STORAGE_QUERY_PROPERTY`(型号/总线/序列号)。即复用 C++ `main.cpp`/`hardware.cpp` 的 Windows API 路径，移植到 `winsuitex-platform`。
- **采纳 C++ 交互**：「一键复制」纯文本摘要（仿 `Disk_BuildText`，含选中盘型号/总线/序列号/卷标/簇/文件系统）+ 刷新按钮；空值沿用 §4 诚实灰，不伪造。
- **与 HTML 关系**：本窗是「单盘聚焦」快捷视图；`inventory.html` 仍是全量。窗内「打开网页报告」按钮跳转该盘锚点。
- **空值**：沿用 §4，诚实灰，不伪造。

---

## 8. 实现计划（egui 即时模式，已审定范围）

> 落地准则：先出面板+图标+字段绑定（可跑通），再补磁盘钻取窗、行右键菜单、注册表启动器对齐、卷/分区采集缺口。多站聚合不进产品（见 §6）。

### 8.1 工程脚手架（新建 `winsuitex-gui` crate）

- 新增 workspace 成员 `rust/winsuitex-gui/`（`Cargo.toml` 加 `egui`、`eframe`；特性同其它 crate，零 unsafe、零 unwrap 生产代码）。
- 入口 `main.rs`：`eframe::run_native` 起窗口；窗口标题 `WinSuiteX — 硬件概览`；尺寸初值约 `420 × 640`，可缩放、可纵向滚动。
- 图标资源：`assets/icons/{system,cpu,board,ram,gpu,monitor,disk,audio,nic}.svg`（按 §2.1 含义设计）→ 构建期 `include_bytes!` 读入，`Context::load_texture` 转 `TextureHandle` 缓存（一次加载，复用 9 个）。
- 数据来源：复用现有采集 CLI 的 `Inventory`（`winsuitex-core` model）；GUI 启动时调用 `winsuitex-platform` 采集，或读取 `inventory.json` 快照（二者择一，优先"读 JSON 快照"以最快打通面板，再接实时采集）。
- 二进制产物仍走 `dist/winsuitex.exe` 同套版本戳/图标/版权（`version_stamp.py` + `app.rc`），GUI 与 CLI 可共用或另出 `winsuitex-gui.exe`——**待你拍板产物命名**。

### 8.2 面板渲染（§2 + §3 字段绑定）

滚动 `ScrollArea` 内纵向排 9 个分类块，顺序固定 `系统→处理器→主板→内存→显卡→显示器→磁盘→声卡→网卡`，每块：
- 左：`TextureHandle` 分类图标 + 分类名（粗体）；右：健康汇总点（该类若有红/橙，显示最严重色；全绿/无则灰）。
- 字段按 §3 逐行 `Label`，多项设备各占一行（CPU/盘/网卡/显示器/声卡/显卡逐条；内存仅列已插）。

**逐类字段绑定（键 → `info.*` 路径）**：

| 类 | 绑定字段 | 备注 |
|----|----------|------|
| 系统 | `info.os.name/version/build` · `info.boot_at` · `info.uptime_seconds` | 二者 None → §4 灰「—/未采集」 |
| 处理器 | `info.cpu[].model` (+`cores`/`threads`) | 每颗一行 |
| 主板 | `info.board.vendor/product`(+清洗) · `info.board.bios_vendor/version` | 占位串→`—` |
| 内存 | `info.memory.total_bytes` · `已插N/空槽M` · `info.memory.sticks[]`（仅已插：`capacity`+`speed_mhz`+`vendor`+(`slot`)） | **空槽不逐行**；频率=`speed_mhz`(MT/s) |
| 显卡 | `info.gpu[].name`+`vram`+(遥测 `temperature_c`/`power_w`)+`source`标注 | BMC/无盘→绿 |
| 显示器 | `info.monitors[].name`(清洗)+`resolution`+`refresh_rate`+(`size_inch`) | 每台一行 |
| 磁盘 | `info.disks[]`：介质+接口+型号+容量+`health_status`色+重映射/待映射+通电+温度+(序列号未透传灰) | 每块一行（重点） |
| 声卡 | `info.audio[].name`；空→「无（未检测到）」灰 | 每块一行 |
| 网卡 | `info.nics[].desc`+`mac`+`ipv4`+`speed`+(`up`) | 每块一行 |

### 8.3 空值组件（§4 铁律）

- 抽一个小部件 `status_dot(&severity)` → 红/橙/绿/灰圆点；`empty_value(text)` → 灰字 `—` / `未采集` / `数据缺失` / `未透传 —`。
- 所有未采集/占位一律走 `empty_value`，**绝不伪造填充**（守老纪律）。

### 8.4 行右键菜单（§6 范围）

- 用 `egui` 右键 `response.context_menu(...)`（或 `Popup`）：对整行 `response.interact(...).context_menu`。
- **磁盘行**：子项 `定位网页报告该盘`（打开 `inventory.html` 锚点）· `复制序列号`（空→复制「未透传」）· `导出单盘快照`（写 `disk_<model>_<sn>.json` 到 `dist/`）。
- **其它类行**：子项 `复制该行文本`（纯文本，仿 C++「一键复制」）。
- 触发方式：磁盘行额外支持左键点击 → 打开 §8.5 钻取窗。

### 8.5 磁盘钻取窗（§7 超集）

- `egui` 独立 `Window`（可关闭）；内容 = C++ 卷/分区（§7 缺口补齐后）⊕ Rust SMART：
  - 卷/分区：盘符·PhysicalDrive 映射·文件系统·卷标·簇大小·类型·总容量·可用空间
  - 物理+SMART：介质/接口/型号/容量/总线/序列号·健康色·重映射/待映射/介质错误·通电时长(折年)·温度·磨损(`percentage_used`，SSD)·来源(`source`/`unavailable_reason`)
- 按钮：`一键复制`（纯文本摘要，仿 `Disk_BuildText`：型号/总线/序列号/卷标/簇/文件系统）+ `刷新`（重采该盘）+ `打开网页报告`（跳锚点）。
- 空值走 §8.3 灰。

### 8.6 注册表右键启动器（`feature.rs`，§6.1 对齐 C++）

- 既有 `feature.rs` 保留，功能清单对齐 C++ `g_features[]`：计算机管理/设备管理器(`compmgmt.msc`/`devmgmt.msc` 此电脑右键)·显示隐藏系统文件(SuperHidden 切换)·命令提示符(`cmd /k pushd "%V"`)·PowerShell·（可扩展）磁盘管理/服务/本地用户和组。
- 注册表父路径对齐：`Directory\shell` / `Directory\Background\shell` / `DesktopBackground\shell` / 此电脑 CLSID；不丢已有功能。
- 与面板行右键区分：本项是 **Explorer 文件/此电脑右键增强**（系统级），面板行右键是 **软件内交互**（§8.4）。

### 8.7 采集缺口（唯一新增成本，§7 标红）

为复刻 C++ 磁盘信息页「卷/分区」视角，需在 `winsuitex-platform` 新增枚举并并入 `info.disks[]`：
- `GetLogicalDriveStrings` → `GetDriveType` / `GetDiskFreeSpace`（可用/簇大小）
- `DeviceIoControl(IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS)`：盘符→`PhysicalDrive` 映射
- `DeviceIoControl(IOCTL_STORAGE_QUERY_PROPERTY)`：型号/总线/序列号/介质
- 落点：在 `physical_disk.rs` 同模块新增 `collect_volumes()`，结构挂到 `Disk` 上的 `volume` 子结构（盘符/fs/cluster_bytes/free_bytes/vol_label），**不破坏现有 SMART 字段与老 JSON 兼容性**（新增可选字段）。
- 本机 VM 无 NVMe/多盘——**验证以真机数据或官方文档为准**，不在 VM 编造。

### 8.8 校验 / 交付

- 门禁沿用 Rust 纪律：`cargo fmt --check` / `clippy --all-targets --all-features -- -D warnings` / `cargo test --workspace` 全绿；新增 `winsuitex-gui` 用例覆盖「空值组件」「内存仅列已插」「磁盘钻取窗字段合并」。
- 用「千雪电竞」真实 JSON 跑通面板 + 钻取窗，截图核对 §4 状态色与 §5 草图一致；演示真机磁盘（含 ST6000NM0115 重映射 105632 红紧急）验证钻取。
- 产物：`dist/` 仍只留 6 类文件（§dist 整洁纪律）；GUI 临时构建物裁剪备份只留最近 2 个。
- 多站聚合：仅 `audit_servers.py`/`health_report.py` 一次性脚本，不进产品。
