# WinSuiteX (Rust) — 架构设计

> 版本：v0.1　日期：2026-09-15
> 工具链：`rustc 1.97.1` / `x86_64-pc-windows-msvc` / edition 2021
> 与 C++ 版并存于 `WinSuiteX/rust/`，便于新旧二进制对拍。

---

## 0. 设计立场

C++ 版的架构是**它那个时代的合理产物**，但有三处不适合直接翻译：

| C++ 做法 | 问题 | Rust 应对 |
|---|---|---|
| 30+ 个文件级 `static` 全局状态 | 隐式耦合，无法单测 | 单一 `AppState` + 消息驱动 |
| 定长 `WCHAR[N]` + `buf[0]==0` 判"未采集" | 空串与未采集语义混淆 | `Option<T>` 由类型系统区分 |
| 布局与命中测试各算一遍（靠注释保证一致） | 易脱节，无测试 | 同一棵布局树产出，纯函数可测 |

**本设计不沿用以上任何一条。** UI 层按 Rust 生态惯用方案（Elm 架构 + 声明式组件树 + `Painter` trait）重新设计。

---

## 1. 模块划分、职责与依赖

### 1.1 Workspace 结构

```
rust/
├── Cargo.toml                    # workspace 根
└── crates/
    ├── winsuitex-core/           # 领域模型 + trait（★ 零平台依赖，100% 可测）
    ├── winsuitex-platform/       # Win32 实现（★ 所有 unsafe 收敛在此）
    ├── winsuitex-report/         # HTML / 文本报告生成（纯函数）
    ├── winsuitex-cli/            # 无头入口 --report / --selftest
    └── winsuitex-ui/             # GUI（最后阶段，Rust 原生设计）
```

### 1.2 依赖方向（严格单向，无环）

```
        ┌──────────────┐
        │ winsuitex-   │  ← 领域模型、trait 定义、错误类型
        │ core         │    不 import windows crate，不碰 Win32
        └──────┬───────┘
               ▲
      ┌────────┼────────┬──────────────┐
      │        │        │              │
┌─────┴────┐ ┌─┴──────────┐ ┌──────────┴───┐
│ platform │ │ report     │ │ (将来) ui    │
│ Win32 impl│ │ 纯函数渲染 │ │ GUI         │
└─────┬────┘ └────────────┘ └──────────────┘
      ▲            ▲               ▲
      └────────────┴───────────────┘
                   │
            ┌──────┴──────┐
            │ winsuitex-  │  ← 二进制装配
            │ cli (bin)   │
            └─────────────┘
```

**铁律**：`core` 不依赖 `platform`；`platform` 依赖 `core` 并实现其 trait。
这条保证 `core` 的测试不需要真实 Windows 环境（CI 友好）。

### 1.3 各 crate 职责

| crate | 职责 | 关键约束 |
|---|---|---|
| **core** | 数据结构（`HardwareInfo`/`Cpu`/`MemStick`/`Disk`/`Feature`…）、`trait` 定义、统一 `Error`、采集诊断 | **禁止**出现 `unsafe`、`windows::`、全局可变状态 |
| **platform** | SMBIOS 解析、CPUID/libcpuid FFI、SetupAPI、磁盘 IOCTL、DXGI、注册表、NVML/ADL、COM 音频 | 所有 `unsafe` 必须带 `// SAFETY:` 注释；对外只暴露 safe API |
| **report** | `HardwareInfo` → HTML / 纯文本 | 纯函数 `&HardwareInfo -> String`，无 IO（写文件由调用方负责） |
| **cli** | 参数解析、`--report` / `--selftest` 流程、退出码 | 退出码语义与 C++ 版严格一致 |
| **ui** | GUI（阶段三） | 见 §6 |

---

## 2. 数据结构与所有权设计

### 2.1 核心原则

| 原则 | 做法 |
|---|---|
| **用类型表达"未采集"** | `Option<String>` 而非空串哨兵。编译器强制处理 `None`，杜绝 C++ 版"空串到底是没采到还是真为空"的歧义 |
| **拥有而非借用** | `collect()` 返回 owned `HardwareInfo`，采集过程中不共享、不加锁 |
| **动态而非定长** | `Vec<T>` 替代 `HwGpu[4]` 等定长数组。C++ 的 `HW_MAX_*` 截断（8 卡机器只能显示 4 张）自然消失 |
| **无 `Arc`/`Mutex`** | 单线程采集 + 单线程 UI，不需要共享所有权。若将来要并行采集，用 `thread::scope` 传 `&mut` 分片，仍不需要 `Arc` |

### 2.2 数据模型

```rust
// core/src/model.rs

/// 一台机器的完整硬件快照。采集完成后即为不可变事实。
#[derive(Debug, Clone, Default)]
pub struct HardwareInfo {
    pub os: Option<OsInfo>,
    pub machine: Option<MachineInfo>,
    pub board: Option<BoardInfo>,       // 主板 + BIOS
    pub cpus: Vec<Cpu>,                 // 替代 HwCpu[8] + cpuCount
    pub memory: MemoryInfo,             // 汇总 + 每根内存条
    pub gpus: Vec<Gpu>,                 // 替代 HwGpu[4]
    pub displays: Vec<Display>,
    pub nics: Vec<Nic>,
    pub disks: Vec<PhysicalDisk>,
    pub audio: Vec<AudioDevice>,
    pub usb: Vec<UsbDevice>,
    pub is_server: bool,
}

#[derive(Debug, Clone)]
pub struct Cpu {
    pub socket: Option<String>,
    pub model: Option<String>,
    pub vendor: Option<String>,
    pub codename: Option<String>,       // libcpuid 微架构代号
    pub cores: Option<u32>,             // 物理核
    pub threads: Option<u32>,           // 逻辑核
    pub speed_mhz: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Gpu {
    pub name: String,
    pub vram_mb: Option<u32>,
    pub vendor: Option<String>,         // 板卡品牌（AIB），如华硕/微星/七彩虹；来自 SubSysId 或名称兜底
    pub telemetry: Option<GpuTelemetry>, // None = 无驱动/不支持（整块缺失，而非全填 0）
}

/// GPU 遥测：只有真的拿到数据才 Some。
/// 替代 C++ 版「tempC=0 表示未识别」的哨兵值设计。
#[derive(Debug, Clone)]
pub struct GpuTelemetry {
    pub temp_c: Option<i32>,
    pub fan_pct: Option<u32>,
    pub power_w: Option<u32>,
    pub util_pct: Option<u32>,
    pub mem_used_mb: Option<u32>,
    pub clock_mhz: Option<u32>,
}
```

**与 C++ 版的语义差异（重要）**：

| C++ | Rust | 理由 |
|---|---|---|
| `WCHAR name[128]`，空串 = 未采集 | `Option<String>` | 类型区分「未采集」与「采集到空」 |
| `int tempC; // 0=未识别` | `Option<GpuTelemetry>` 整块 `None` | 消除哨兵值歧义（0℃ 与"未识别"不再混淆） |
| `HwGpu gpus[4]` + `gpuCount` | `Vec<Gpu>` | 不再有 8 卡机器显示 4 张的截断 |
| 全局 `static HardwareInfo g_hw` | 函数返回值，owned | 可测、可并行、无隐式依赖 |

### 2.3 所有权与生命周期边界

```
采集阶段（一次性，无共享）
──────────────────────────
  SmbiosSource::collect()  ──move──▶ Vec<Cpu>
  DiskSource::collect()    ──move──▶ Vec<PhysicalDisk>
  ...                                     │
                                          ▼
                              HardwareInfo { .. }  ← owned，返回给调用方
                                          │
报告阶段（只读借用，零 clone）
──────────────────────────
  fn build_html(hw: &HardwareInfo) -> String   ← 借用，不复制数据
                                          │
CLI 阶段（拥有，写出）
──────────────────────────
  std::fs::write(path, html)   ← html 是 owned String，直接写出
```

**三条生命周期规则**：

1. **采集函数签名固定为 `fn collect(&self) -> Result<T>`**（返回 owned，不返回引用）。
   避免"数据由谁持有"的模糊地带——C++ 版 `HW_Collect(HardwareInfo* out)` 的 out 参数模式在 Rust 里没有存在理由。
2. **报告生成只借用 `&HardwareInfo`**，不 clone。整个报告路径零数据复制。
3. **无 `'static` 泄漏、无 `Box::leak`**。C++ 版 `g_features[]` 是 `const wchar_t*` 静态表，
   Rust 侧对应 `&'static str` 或编译期常量数组（见 §3.3），天然 `'static` 无需泄漏。

### 2.4 明确避免的 clone

| 场景 | 错误做法 | 正确做法 |
|---|---|---|
| 子采集器结果汇总 | `hw.cpus = cpus.clone()` | 直接 move：`hw.cpus = cpus` |
| 报告生成 | `hw.clone()` 后格式化 | `&hw` 借用 |
| 字符串拼接 | `format!("{}{}", a.clone(), b)` | `write!` 到 `String` |
| 注册表读取 | 返回 `String` 再转 `Vec<u16>` 再转回 | 在 FFI 边界一次性转成 `String` |

---

## 3. trait 抽象与接口边界

### 3.1 采集器 trait（可测试性的关键）

```rust
// core/src/traits.rs

/// 一个硬件数据源。所有平台实现都实现此 trait。
pub trait HardwareSource {
    /// 数据源名称，用于诊断信息（如 "smbios" / "disk.ioctl"）
    fn name(&self) -> &'static str;

    /// 采集并写入 ctx。失败不中断整体采集（返回 Err 会被记入诊断）。
    fn collect(&self, ctx: &mut CollectContext) -> Result<(), CollectError>;
}
```

**为什么是"写入 ctx"而不是"返回数据"**：
- 各源职责不同（有的填 CPU，有的填内存），无法统一返回类型
- `CollectContext` 内部是 `HardwareInfo` 的构建器，各源只写自己负责的字段
- 类型安全：构建器方法按字段命名，写错字段编译期报错

```rust
pub struct CollectContext { info: HardwareInfo }

impl CollectContext {
    pub fn set_cpus(&mut self, cpus: Vec<Cpu>) { self.info.cpus = cpus; }
    pub fn push_gpu(&mut self, gpu: Gpu)       { self.info.gpus.push(gpu); }
    // ...
}
```

**测试收益**：注入 `MockSource`（返回 fixture 数据）即可测试整个采集编排逻辑，
**无需真实硬件**——这是 C++ 版完全做不到的事。

### 3.2 注册表 trait（让破坏性操作可测）

```rust
// core/src/traits.rs

pub trait Registry {
    fn read_string(&self, root: RegRoot, path: &str, name: Option<&str>)
        -> Result<Option<String>, RegistryError>;
    fn write_string(&self, root: RegRoot, path: &str, name: Option<&str>,
                    value: &str, kind: RegValueKind) -> Result<(), RegistryError>;
    fn write_dword(&self, root: RegRoot, path: &str, name: Option<&str>,
                   value: u32) -> Result<(), RegistryError>;
    fn delete_tree(&self, root: RegRoot, path: &str) -> Result<(), RegistryError>;
    fn exists(&self, root: RegRoot, path: &str) -> bool;
}
```

- 生产实现：`platform::registry::WinRegistry`
- **测试实现：`InMemoryRegistry`（`HashMap` 模拟）** → 右键菜单的
  Install/Uninstall/幂等性/卸载清理 全部可单测

> 这一条直接补上 C++ 版最大的测试盲区。C++ 的 selftest 只能"真写 HKCU 再还原"，
> Rust 版可以在内存里跑完整逻辑。

### 3.3 功能（右键菜单）的数据驱动

C++ 版用 `g_features[]` 静态数组（数据驱动，这个设计是好的）。
Rust 侧保留数据驱动，但用类型强化：

```rust
// core/src/feature.rs

pub enum FeatureKind {
    /// 静态 verb：shell\<verb>\command = 命令行
    RunCommand { command: &'static str },
    /// 调用 DLL 导出的切换函数
    ShellExtension { toggle_fn: &'static str },
}

pub struct Feature {
    pub id: FeatureId,                 // newtype，见下
    pub display_name: &'static str,
    pub description: &'static str,
    pub reg_parents: &'static [&'static str],  // 1~3 个父路径，替代 regParent/2/3 三字段
    pub verb: &'static str,
    pub kind: FeatureKind,             // 枚举替代「tag + 可选字段」，非法组合不可表示
    pub menu_icon: Option<&'static str>,
    pub win11_clsid: Option<&'static str>,
}
```

**类型系统表达不变量**（用户要求的核心）：

| C++ 的隐患 | Rust 的编译期保证 |
|---|---|
| `kind` 是枚举但 `command`/`clsid`/`toggleArg` 都是独立可选字段，<br>可以组合出非法状态（如 RunCommand 却填了 toggleArg） | `FeatureKind` 枚举**携带**各自需要的数据，<br>非法组合**无法构造** |
| `regParent`/`regParent2`/`regParent3` 三个平行字段 | `&'static [&'static str]` 切片，数量任意且统一处理 |
| `id` 是裸 `const wchar_t*`，拼错编译通过 | `FeatureId` newtype，配合常量定义 |

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FeatureId(pub &'static str);   // newtype：避免与任意字符串混用
```

### 3.4 报告渲染 trait

```rust
// report/src/lib.rs
pub trait ReportFormatter {
    fn format(&self, hw: &HardwareInfo) -> String;
}
// 实现：HtmlFormatter / TextFormatter
```

纯函数 + trait = 渲染逻辑可测（喂 fixture `HardwareInfo`，断言输出片段）。

---

## 4. 错误处理与状态流转

### 4.1 统一错误类型

```rust
// core/src/error.rs
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("注册表操作失败 [{op}]: {source}")]
    Registry { op: &'static str, #[source] source: RegistryError },

    #[error("硬件采集失败 [{source_name}]: {reason}")]
    Collect { source_name: &'static str, reason: String },

    #[error("资源释放失败: {0}")]
    ResourceInstall(String),

    #[error("IO 失败: {0}")]
    Io(#[from] std::io::Error),

    #[error("参数错误: {0}")]
    Args(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

### 4.2 「最佳努力」语义 —— 不静默吞错

C++ 版的最大问题：**采集失败被静默吞掉**（`pInit() != 0 → return`，无任何记录）。
Rust 侧保留"不因单项失败而崩溃"，但**必须留下痕迹**：

```rust
/// 采集诊断：记录哪些源失败、为什么失败。
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub source: &'static str,
    pub message: String,
}

pub struct CollectResult {
    pub info: HardwareInfo,        // 部分成功的硬件信息
    pub diagnostics: Vec<Diagnostic>,  // 失败项（不为空即表示有降级）
}

impl CollectResult {
    /// 全部源都成功
    pub fn is_complete(&self) -> bool { self.diagnostics.is_empty() }
}
```

**规则**：
- 单个 `HardwareSource::collect()` 返回 `Err` → 记入 `diagnostics`，**继续采集其余源**
- `collect_all()` **永不因硬件采集失败而返回 `Err`**（与 C++ 行为一致）
- 但 `diagnostics` 非空时，`--report` 可在 HTML 里输出"部分数据缺失"提示（C++ 版无此能力）

> 这是**行为兼容 + 可观测性增强**：既保住"失败降级不崩溃"契约，又不再静默。

### 4.3 状态流转（CLI）

```
启动
 │
 ├─ 解析参数 ──失败──▶ ExitCode::ARGS(2) + stderr 提示
 │
 ├─ --report "<path>"
 │     │
 │     ├─ collect_all()          （永不失败，可能带 diagnostics）
 │     ├─ HtmlFormatter::format(&hw)  （纯函数）
 │     ├─ fs::write(path, html)  ──失败──▶ 2
 │     └─ diagnostics 非空 → stderr 警告（不影响退出码）
 │           └─▶ ExitCode::SUCCESS(0)
 │
 ├─ --selftest[-nogui]
 │     └─ 跑断言 → 失败项数作为退出码（0=全通过）
 │
 └─ （无参数）→ 启动 UI（阶段三）
```

**退出码契约与 C++ 版严格一致**（已有批量巡检脚本依赖）：

| 场景 | 码 |
|---|---|
| `--report` 成功 | 0 |
| `--report` 失败 | 2 |
| `--selftest` 全通过 | 0 |
| `--selftest` 有失败 | 失败项数 |

---

## 5. 并发模型

### 5.1 现状与结论

C++ 版**完全没有并发**（仅 `WM_TIMER`），实测采集 < 300ms。
**盲目引入并发是负收益**，但架构必须预留。

### 5.2 分层设计

| 层 | 并发策略 | 理由 |
|---|---|---|
| **采集** | **默认串行**，可选并行 | 耗时已足够短；并行会引入 COM apartment 复杂度 |
| **报告生成** | 串行纯函数 | 纯计算，无共享 |
| **UI** | **单线程 + 消息传递** | Win32 窗口本就线程亲和（消息循环在固定线程） |

### 5.3 采集并行（可选，架构已支持）

`HardwareSource` 彼此独立，天然可并行。若将来要启用：

```rust
// 方案：scoped threads（无 'static 要求，无 Arc）
std::thread::scope(|s| {
    let handles: Vec<_> = sources.iter()
        .map(|src| s.spawn(move || src.collect()))  // 每个源产出自己的部分结果
        .collect();
    // 收集结果，merge 进 HardwareInfo
});
```

**关键取舍：用"每个任务产出自己的数据 + move 回主线程"消除共享，而不是加锁。**
→ 无需 `Arc<Mutex<HardwareInfo>>`，无锁竞争，无死锁。

**约束**：音频采集（COM `MMDeviceEnumerator`）**必须在同一线程完成**
`CoInitializeEx` / `CoUninitialize`（apartment 线程亲和），故音频源标记为
`ThreadAffinity::MainThread`，编排器保证其不参与并行。

### 5.4 UI 线程模型

```
OS 消息循环线程 ──▶ Win32 消息 ──▶ 转成 Msg ──▶ update(&mut AppState, Msg)
                                                      │
                                                      ├─ 产生 Effect（写文件/注册表/复制）
                                                      └─ 触发 view(&AppState) 重绘
```

- **无任何共享可变状态**：`AppState` 由 UI 线程独占
- **耗时操作**（注册表写入、硬件采集）若需要后台化：
  用 `std::sync::mpsc` 把结果送回主线程，主线程 `update` 消费
  ——`AppState` 始终只有一个所有者

### 5.5 为什么不用 async

- 无 IO 密集 / 无大量并发连接
- 引入 tokio 会让单 exe 体积与复杂度上升，违背"零依赖轻量"目标
- 结论：**不用 async，用线程 + 消息传递**（若真需要并发）

---

## 6. UI 层设计（Rust 原生，不照搬 C++）

> 阶段三实现，此处先定架构，确保不沿用 C++ 的全局 RECT 数组 + 双重计算模式。

### 6.1 Elm 架构

```rust
struct AppState {
    tab: Tab,
    hw: Option<HardwareInfo>,
    disks: Vec<Disk>,
    selected_disk: Option<usize>,
    hover: Option<HitId>,
    notice: Option<Notice>,
    // ... 全部状态在此，无全局 static
}

enum Msg {
    SwitchTab(Tab),
    ToggleFeature(FeatureId),
    SelectDisk(usize),
    ExportReport,
    CopyToClipboard,
    MouseMove(Point), MouseLeave, Click(Point),
    Quit,
}

fn update(state: &mut AppState, msg: Msg) -> Vec<Effect>;
fn view(state: &AppState) -> Element;      // 声明式组件树，每帧重建
```

**收益**：`Msg` 是 enum，新增消息时编译器**强制**所有 `match` 处理它
（C++ 的 switch 漏 case 只能靠人肉 review）。

### 6.2 布局与命中测试同源（消除 C++ 的一类 bug）

```rust
/// 布局是【纯函数】：(组件树, 可用区域) -> 布局树
fn layout(el: &Element, bounds: Rect, dpi: u32) -> LayoutTree;

/// 布局树每个节点自带命中 ID —— 命中测试直接查树，不再各算一遍
struct LayoutNode { rect: Rect, hit: Option<HitId>, children: Vec<LayoutNode> }
fn hit_test(tree: &LayoutTree, pt: Point) -> Option<HitId>;
```

C++ 版靠注释"布局与命中检测共用同一份 RECT"来保证一致；
Rust 版**结构上不可能不一致**——两者读同一棵树。

**且 `layout` 是纯函数 → 可单元测试**（DPI 96/125/150 各断言一次）。

### 6.3 Painter trait（让绘制可测）

```rust
pub trait Painter {
    fn fill_round_rect(&mut self, r: Rect, radius: f32, color: Color);
    fn stroke_round_rect(&mut self, r: Rect, radius: f32, color: Color, width: f32);
    fn fill_gradient(&mut self, r: Rect, from: Color, to: Color, vertical: bool);
    fn draw_text(&mut self, r: Rect, text: &str, style: TextStyle);
    fn draw_blurred_shadow(&mut self, r: Rect, radius: f32, blur: f32, color: Color);
}
```

- 生产：`GdiPainter`（GDI+ / 纯 GDI，见 §6.5）
- **测试：`RecordingPainter`** —— 记录指令序列，断言"画了什么、在哪画"

> 这补上 C++ 版"UI 绘制零测试"的盲区：布局、命中、绘制三件事全部可测。

### 6.4 与 C++ 版的对照

| 维度 | C++ 版 | Rust 版 |
|---|---|---|
| 状态 | 30+ 全局 static | 单一 `AppState` |
| 控件 | 全局 RECT 数组 + `PtInRect` | 组件树 + `LayoutTree` |
| 布局/命中 | 两处计算，靠注释同步 | 同一棵树，结构上一致 |
| 事件 | `switch (msg)` + 全局标志 | `Msg` enum + 穷尽匹配 |
| 绘制 | 直接调 GDI，无法断言 | `Painter` trait，可录制断言 |
| DPI | 全局 `g_dpi` + `Scale()` | 布局函数显式传 `dpi` 参数 |
| GDI 对象 | 每帧新建/销毁 58 个 | RAII + 缓存（字体按 DPI 缓存） |
| 可测性 | 0% | 布局/命中/绘制均可单测 |

### 6.5 渲染后端取舍（待阶段三拍板）

| 方案 | 优点 | 缺点 |
|---|---|---|
| 纯 GDI（不用 GDI+） | 零额外依赖，windows crate 覆盖好 | 需自实现渐变与模糊（C++ 版的 `BoxBlur` 可直接移植） |
| GDI+ flat API | 与 C++ 版视觉一致 | `windows` crate 的 `Win32_Graphics_GdiPlus` 覆盖度需实测 |
| `tiny-skia` | 纯 Rust、抗锯齿好、可测 | 引入依赖，产物变大 |

倾向：**先纯 GDI + 自绘渐变/模糊**，保持"零依赖单 exe"；视觉调优阶段再评估。

---

## 7. 验证策略

| 层级 | 手段 |
|---|---|
| 单元 | `cargo test`：SMBIOS 解析（fixture 字节流）、布局、命中、报告渲染、注册表逻辑（`InMemoryRegistry`） |
| 集成 | `cargo test -- --ignored`：真实机器采集，断言"关键字段非空" |
| 端到端 | **新旧对拍**：`WinSuiteX.exe --report old.html` vs `winsuitex.exe --report new.html`，逐字段 diff |
| 回归 | `--selftest` 移植 C++ 版约 80 个断言 |

---

## 8. 当前未决事项

| # | 事项 | 待谁定 |
|---|---|---|
| 1 | `hardware.cpp:1100` `g->vramMB = 0;` 是否笔误 | Z.W. |
| 2 | UI 渲染后端（纯 GDI / GDI+ / tiny-skia） | 阶段三实测后定 |
| 3 | 是否保留 `WinSuiteXShell.dll` 为 Rust `cdylib`（还是继续用 C++ 版 DLL） | 阶段三 |
| 4 | 产物体积预算（Rust 静态链接后 vs 现有 820KB） | 实测后定 |
