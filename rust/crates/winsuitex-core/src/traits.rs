//! trait 抽象与接口边界。
//!
//! 这里是**可测试性**的核心：所有平台能力都通过 trait 暴露，
//! 测试时可注入 mock / in-memory 实现，无需真实 Windows 环境。
//! C++ 版做不到这点（selftest 只能真写 HKCU 再还原）。

use crate::error::Result;
use crate::model::{
    AudioDevice, BoardInfo, Cpu, Display, Gpu, HardwareInfo, InputDevice, MachineInfo, MebiBytes,
    MemStick, Nic, OsInfo, PhysicalDisk, RegRoot, RegValueKind, UsbDevice, Volume,
};

// ============================================================
// 硬件采集
// ============================================================

/// 采集上下文：各数据源只写自己负责的字段。
///
/// 为什么不直接返回数据？因为各源职责不同（有的填 CPU，有的填内存），
/// 无法统一返回类型。用上下文 + 具名写入方法，写错字段编译期即报错。
#[derive(Debug, Default)]
pub struct CollectContext {
    info: HardwareInfo,
    volumes: Vec<Volume>,
}

impl CollectContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn into_parts(self) -> (HardwareInfo, Vec<Volume>) {
        (self.info, self.volumes)
    }

    // ---- 写入方法（具名，避免字段级误操作）----
    pub fn set_os(&mut self, v: OsInfo) {
        self.info.os = Some(v);
    }
    pub fn set_machine(&mut self, v: MachineInfo) {
        self.info.machine = Some(v);
    }
    pub fn set_board(&mut self, v: BoardInfo) {
        self.info.board = Some(v);
    }
    pub fn set_cpus(&mut self, v: Vec<Cpu>) {
        self.info.cpus = v;
    }
    pub fn set_memory_sticks(&mut self, v: Vec<MemStick>) {
        self.info.memory.sticks = v;
    }
    /// 只读访问已采内存条（供 `memory.wmi` 兜底源回填缺失字段用）。
    pub fn memory_sticks(&self) -> &[MemStick] {
        &self.info.memory.sticks
    }
    /// 收 MiB 而非 u64：调用方必须显式构造 `MebiBytes`，杜绝单位误传。
    pub fn set_memory_total(&mut self, v: MebiBytes) {
        self.info.memory.total_mb = Some(v);
    }
    pub fn set_gpus(&mut self, v: Vec<Gpu>) {
        self.info.gpus = v;
    }
    pub fn set_displays(&mut self, v: Vec<Display>) {
        self.info.displays = v;
    }
    pub fn set_nics(&mut self, v: Vec<Nic>) {
        self.info.nics = v;
    }
    pub fn set_disks(&mut self, v: Vec<PhysicalDisk>) {
        self.info.disks = v;
    }
    pub fn set_audio(&mut self, v: Vec<AudioDevice>) {
        self.info.audio = v;
    }
    pub fn set_usb(&mut self, v: Vec<UsbDevice>) {
        self.info.usb = v;
    }
    pub fn set_input(&mut self, v: Vec<InputDevice>) {
        self.info.input_devices = v;
    }
    /// 写入采集进程的提权状态（来自 `process_is_elevated`）。`false` 表示受 UAC 令牌筛选，
    /// NVMe SMART（寿命百分比 / 通电时长）经 VMD / 虚拟层可能读不到，报告需明示。
    pub fn set_admin_elevated(&mut self, v: bool) {
        self.info.admin_elevated = Some(v);
    }
    pub fn set_is_server(&mut self, v: bool) {
        self.info.is_server = v;
    }
    pub fn set_volumes(&mut self, v: Vec<Volume>) {
        self.volumes = v;
    }
}

/// 一个硬件数据源。
///
/// 各实现彼此独立 → 天然可并行（见 ARCHITECTURE.md §5.3）。
pub trait HardwareSource {
    /// 数据源名称，用于诊断信息（如 "smbios" / "disk.ioctl"）。
    fn name(&self) -> &'static str;

    /// 采集并写入 `ctx`。返回 `Err` 不会中断整体采集——错误会被记入诊断。
    fn collect(&self, ctx: &mut CollectContext) -> Result<()>;

    /// 线程亲和性：COM apartment 相关的源必须在主线程执行。
    fn affinity(&self) -> ThreadAffinity {
        ThreadAffinity::Any
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadAffinity {
    /// 可在任意线程执行
    Any,
    /// 必须固定在同一线程（如需要 CoInitialize 的 COM 调用）
    MainThread,
}

/// 采集编排器：串行跑完所有源，收集诊断。
///
/// 单个源返回 `Err` 只记降级、不中断整体；但**进程级硬崩溃无法被 Rust 捕获**
/// （访问违规不走 panic，stdout 缓冲区内容也会全部丢失）。
/// 因此提供 `WSX_TRACE` / `WSX_SKIP` 两个环境变量用于真机排障，见下方两个函数。
pub fn collect_all(sources: &[Box<dyn HardwareSource>]) -> crate::error::CollectResult {
    let trace = wsx_trace_enabled();
    let skipped = wsx_skip_list();
    let mut result = crate::error::CollectResult::default();
    let mut ctx = CollectContext::new();

    for src in sources {
        let name = src.name();

        if skipped.iter().any(|s| s == name) {
            // 不静默：跳过也是一次降级，报告里必须看得见，否则用户不知道这一节为什么空。
            result.push_diagnostic(name, "已按 WSX_SKIP 跳过该采集源".to_string());
            if trace {
                eprintln!("[collect] -- {} SKIPPED by WSX_SKIP", name);
            }
            continue;
        }

        if trace {
            eprintln!("[collect] >> {}", name);
        }
        match src.collect(&mut ctx) {
            Ok(()) => {
                if trace {
                    eprintln!("[collect] << {} ok", name);
                }
            }
            Err(e) => {
                if trace {
                    eprintln!("[collect] << {} ERR: {}", name, e);
                }
                result.push_diagnostic(name, e.to_string());
            }
        }
    }

    let (info, volumes) = ctx.into_parts();
    result.info = info;
    result.volumes = volumes;
    result
}

/// 环境变量 `WSX_TRACE=1`：把每个采集源的执行进度打到 **stderr**。
///
/// 为什么必须是 stderr 且必须逐行：Rust 的 stderr **不带缓冲**，每写一行立刻落盘；
/// 而 stdout 带缓冲（LineWriter），进程被硬杀时缓冲区内容全部丢失。
/// 某些机器上个别采集源会因为驱动/固件返回异常数据触发访问违规（0xC0000005），
/// 进程瞬间消失——既没有 panic 消息，也没有错误输出，只剩一个 0 字节的 stdout 重定向文件。
/// 此时 **崩溃前最后一行 trace** 就是唯一的破案线索（指认是哪个源把进程弄死的）。
fn wsx_trace_enabled() -> bool {
    std::env::var("WSX_TRACE")
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// 环境变量 `WSX_SKIP=a.b,c.d`：跳过指定采集源（逗号分隔，匹配 `HardwareSource::name()`）。
///
/// 用途：已知某个源在特定机器上会崩溃时，仍能在该机器上产出报告，
/// 而不是整份报告都拿不到（网吧环境下这是刚需——先出报告，再慢慢修 bug）。
/// 被跳过的源会记入诊断项，报告显示降级，绝不静默。
fn wsx_skip_list() -> Vec<String> {
    std::env::var("WSX_SKIP")
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// CLI 侧埋点用：判断本次是否开启了逐源 trace。
pub fn wsx_trace_enabled_pub() -> bool {
    wsx_trace_enabled()
}

// ============================================================
// 注册表（让破坏性操作可测）
// ============================================================

/// 注册表访问抽象。
///
/// - 生产实现：`winsuitex_platform::registry::WinRegistry`
/// - 测试实现：内存 `HashMap` 模拟 → Install/Uninstall/幂等性全部可单测
pub trait Registry {
    /// 读：不可变借用
    fn read_string(&self, root: RegRoot, path: &str, name: Option<&str>) -> Result<Option<String>>;

    /// 写：需要可变借用（编译器强制，杜绝只读实现意外改写）
    fn write_string(
        &mut self,
        root: RegRoot,
        path: &str,
        name: Option<&str>,
        value: &str,
        kind: RegValueKind,
    ) -> Result<()>;

    fn write_dword(
        &mut self,
        root: RegRoot,
        path: &str,
        name: Option<&str>,
        value: u32,
    ) -> Result<()>;

    /// 删除整棵子树。**键不存在视为成功**（对应 C++ `DeleteRegTreeKey` 的最佳努力语义）。
    fn delete_tree(&mut self, root: RegRoot, path: &str) -> Result<()>;

    fn delete_value(&mut self, root: RegRoot, path: &str, name: &str) -> Result<()>;

    fn exists(&self, root: RegRoot, path: &str) -> bool;
}
