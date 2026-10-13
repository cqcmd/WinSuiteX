//! 领域模型：一台机器的完整硬件快照与右键功能定义。
//!
//! 设计原则：
//! - `Option<T>` 表达「未采集到」，而非 C++ 版的空串/0 哨兵值；
//! - `Vec<T>` 替代定长数组，消除 `HW_MAX_*` 截断；
//! - 全部 owned，无引用、无全局状态。

// `Serialize` 需显式引入作用域：`#[derive(Serialize)]` 才能解析到宏。
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================
// 硬件信息
// ============================================================

/// 以 MiB（2^20 字节）为单位的容量。
///
/// # 为什么需要它
/// 报告层曾把「MB 数值」直接喂给只接受**字节**的格式化函数，于是 16 GB
/// 内存被显示成 "16 KB"。这类单位混淆编译器查不出来，只能靠人工核对每处
/// `* 1024 * 1024` 有没有写对 —— 而漏写的那处恰好就是内存总计。
/// 把「MiB」和「字节」分成不同类型后，此类错误在**编译期**暴露。
///
/// 刻意不实现 `Add`/`Mul` 等算术：容量在本项目里只用于展示与比较，
/// 单位是 MiB 的容量。
///
/// 字段**私有**：早期是 `pub u64`，结果报告层有 6 处直接拿 `.0` 做裸算术 ——
/// 其中 `(vram.0 + 512) / 1024` 那处让 256 MiB 显存显示成 "0 GiB"。
/// newtype 若留着裸字段，等于把防止单位误用这件事交回给调用方的自觉。
/// 需要数值请用 `get()`，需要人类可读串请用 report 层的 `fmt_mb`。
///
/// 不提供算术可以避免"两个 MiB 相乘"这类无意义运算通过编译。
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct MebiBytes(u64);

impl MebiBytes {
    pub const fn new(v: u64) -> Self {
        Self(v)
    }

    /// 取出 MiB 数值。命名 `get()` 而非 `0`，是为了让"裸算术"在 code review 里显眼。
    pub const fn get(self) -> u64 {
        self.0
    }

    /// 转成字节（溢出时饱和到 `u64::MAX`，不 panic）。
    pub const fn to_bytes(self) -> u64 {
        self.0.saturating_mul(1 << 20)
    }

    /// 由字节换算（向下取整）。仅用于 platform 层把 Win32 的字节值收进来，
    /// 业务代码应使用 `MebiBytes::new(n)` 明确构造。
    pub const fn from_bytes_floor(bytes: u64) -> Self {
        Self(bytes >> 20)
    }
}

impl From<u32> for MebiBytes {
    fn from(v: u32) -> Self {
        Self(v as u64)
    }
}

impl From<u64> for MebiBytes {
    fn from(v: u64) -> Self {
        Self(v)
    }
}

/// 一台机器的完整硬件快照。采集完成后即视为不可变事实。
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct HardwareInfo {
    pub os: Option<OsInfo>,
    pub machine: Option<MachineInfo>,
    pub board: Option<BoardInfo>,
    pub cpus: Vec<Cpu>,
    pub memory: MemoryInfo,
    pub gpus: Vec<Gpu>,
    pub displays: Vec<Display>,
    pub nics: Vec<Nic>,
    pub disks: Vec<PhysicalDisk>,
    pub audio: Vec<AudioDevice>,
    pub usb: Vec<UsbDevice>,
    pub input_devices: Vec<InputDevice>,
    pub is_server: bool,
    /// 采集时间（本地时间串 `YYYY-MM-DD HH:MM:SS`），由采集端写入。
    /// 资产报告没有时间等于不可归档 —— 拿到一份旧报告无法判断它是否已过期。
    /// 反序列化旧快照时该字段缺失 → `None`，渲染层跳过，不影响老数据。
    pub collected_at: Option<String>,
    /// 采集进程是否以管理员身份运行。`None`=未检测；`Some(false)`=受 UAC 令牌筛选，
    /// NVMe SMART（寿命百分比 / 通电时长）经 VMD / 虚拟层可能读不到，报告顶部需明示。
    /// 反序列化旧快照时该字段缺失 → `None`，不影响老数据。
    #[serde(default)]
    pub admin_elevated: Option<bool>,
}

impl HardwareInfo {
    pub fn new() -> Self {
        Self::default()
    }

    /// 统一给每块盘的 `smart.power_on_duration` 填上由 `power_on_hours` 换算的可读时长。
    ///
    /// 在 JSON 导出前调用一次即可，无需改动 14 处 `DiskSmart` 构造点——
    /// 构造时该字段恒为 `None`，在此集中填充，避免散落各处的重复换算逻辑漂移。
    pub fn enrich_power_on_durations(&mut self) {
        for disk in &mut self.disks {
            if let Some(smart) = &mut disk.smart {
                smart.power_on_duration = smart.power_on_hours.map(format_power_on_duration);
            }
        }
    }
}

/// 把通电小时数换算为人类可读时长，用于报告渲染与 JSON 导出。
///
/// 单一事实源：HTML 文案（`winsuitex-report`）与 JSON 导出都调用本函数，
/// 避免两套算法漂移。例：42367h → "约 4 年 10 个月"；57254h → "约 6 年 6 个月"；
/// 不足 1 天 → "约 N 小时"。
pub fn format_power_on_duration(hours: u32) -> String {
    let days = hours / 24;
    if days >= 365 {
        let years = days / 365;
        let rem_days = days % 365;
        let months = rem_days / 30;
        if months > 0 {
            format!("约 {years} 年 {months} 个月")
        } else {
            format!("约 {years} 年")
        }
    } else if days >= 1 {
        format!("约 {days} 天")
    } else {
        format!("约 {hours} 小时")
    }
}

/// 把任意 `SystemTime`（按 UTC 解释）换算成中国本地时间串 `YYYY-MM-DD HH:MM:SS`。
///
/// 单一事实源：采集时间戳（`local_timestamp_string`）、系统开机时间（`boot_at`）
/// 都走这里，保证报告里所有时间口径一致。中国全境固定 UTC+8 且无夏令时
/// （1992 年后取消），固定偏移即正确。若将来要支持海外机器，需引入 chrono
/// 改为取系统时区 —— 别在这里硬加第二个偏移。
pub fn local_timestamp_from_systemtime(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        + 8 * 3600;
    let (y, m, d, hh, mm, ss) = civil_from_secs(secs);
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02}")
}

/// 当前本地时间，格式 `YYYY-MM-DD HH:MM:SS`（用于给采集快照打时间戳）。
pub fn local_timestamp_string() -> String {
    local_timestamp_from_systemtime(SystemTime::now())
}

/// 把在线时长（秒）换算为人类可读串，用于报告渲染与 JSON 导出。
///
/// 单一事实源：HTML 文案（`winsuitex-report`）与 JSON 导出都调用本函数，
/// 避免两套算法漂移。例：42367s → "约 11 小时 46 分"；259200s → "约 3 天"；
/// 5073600s → "约 58 天 17 小时"；不足 1 分 → "约 N 秒"。
pub fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let minutes = (seconds % 3600) / 60;
    if days >= 1 {
        if hours > 0 {
            format!("约 {days} 天 {hours} 小时")
        } else {
            format!("约 {days} 天")
        }
    } else if hours >= 1 {
        if minutes > 0 {
            format!("约 {hours} 小时 {minutes} 分")
        } else {
            format!("约 {hours} 小时")
        }
    } else if minutes >= 1 {
        format!("约 {minutes} 分")
    } else {
        format!("约 {seconds} 秒")
    }
}

fn civil_from_secs(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    (
        y,
        m,
        d,
        (rem / 3600) as u32,
        (rem % 3600 / 60) as u32,
        (rem % 60) as u32,
    )
}

/// Howard Hinnant 的 civil_from_days 算法：epoch days → (年, 月, 日)。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct OsInfo {
    pub product: Option<String>,
    pub version: Option<String>, // DisplayVersion，如 "22H2"
    pub build: Option<String>,
    pub arch: Option<String>,
    /// 逻辑处理器数（`GetSystemInfo.dwNumberOfProcessors`）。
    /// 与 C++ 版报告的「逻辑处理器」一致，独立成字段而非塞进 arch 字符串。
    pub logical_processors: Option<u32>,
    /// 系统开机时间（本地时间串 `YYYY-MM-DD HH:MM:SS`），由采集端经 `GetTickCount64`
    /// 推算（now − uptime）。资产盘点里判断「这台服务器多久没重启过」很关键：
    /// 长期不重启可能暗示无人维护 / 潜在隐患（如补丁未生效、内存泄漏累积）。
    /// 反序列化旧快照时该字段缺失 → `None`，渲染层跳过，不影响老数据。
    #[serde(default)]
    pub boot_at: Option<String>,
    /// 系统已开机时长（秒），与 `boot_at` 同源（uptime = now − boot_at）。
    /// 仅用于展示 / 导出；渲染层用 `format_uptime` 换算成人类可读串。
    /// 反序列化旧快照时该字段缺失 → `None`，不影响老数据。
    #[serde(default)]
    pub uptime_seconds: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct MachineInfo {
    pub computer_name: Option<String>,
    pub user_name: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct BoardInfo {
    pub vendor: Option<String>,
    pub product: Option<String>,
    /// 主板序列号（SMBIOS Type 2 偏移 0x07）。保修 / RMA 溯源关键字段。
    /// `#[serde(default)]`：老版本 JSON 无此字段时反序列化为 `None`，不破坏存档。
    #[serde(default)]
    pub serial: Option<String>,
    pub chipset: Option<String>,
    pub bios_vendor: Option<String>,
    pub bios_version: Option<String>,
    pub bios_date: Option<String>,
}

/// BIOS 发布日期归一化：`MM/DD/YYYY` → `YYYY-MM-DD`。
///
/// SMBIOS Type 0 的 BIOS Release Date 多为美式 `04/03/2026`，中文报告/界面里
/// 直接显示需要二次判读（"02/24" 是 2 月还是 24 年？）。归一成 `2026-04-03`
/// 后顺序一致、无需脑内换算。非该格式（已含 `-` 或未知写法）原样返回。
pub fn normalize_bios_date(s: &str) -> String {
    let b = s.as_bytes();
    if b.len() == 10 && b[2] == b'/' && b[5] == b'/' {
        let (m, d, y) = (&s[0..2], &s[3..5], &s[6..10]);
        let all_digit = |t: &str| t.bytes().all(|c| c.is_ascii_digit());
        if all_digit(m) && all_digit(d) && all_digit(y) {
            return format!("{y}-{m}-{d}");
        }
    }
    s.to_string()
}

/// 单颗物理 CPU（SMBIOS Type 4）。
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Cpu {
    pub socket: Option<String>,
    pub model: Option<String>,
    pub vendor: Option<String>,
    /// 微架构代号（libcpuid），如 "Ivy Bridge" / "Zen 3"
    pub codename: Option<String>,
    pub cores: Option<u32>,
    pub threads: Option<u32>,
    pub speed_mhz: Option<u32>,
}

/// 内存：汇总 + 每根内存条。
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct MemoryInfo {
    pub total_mb: Option<MebiBytes>,
    pub sticks: Vec<MemStick>,
}

/// 单根内存条（SMBIOS Type 17）。
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct MemStick {
    pub locator: Option<String>, // 插槽位置（Device Locator），如 "DIMM_A1"
    pub bank: Option<String>,    // 通道/插槽组（Bank Locator）
    /// 内存条序列号（SMBIOS Type 17 偏移 0x18）。保修 / RMA 溯源关键字段。
    /// `#[serde(default)]`：老版本 JSON 无此字段时反序列化为 `None`，不破坏存档。
    #[serde(default)]
    pub serial: Option<String>,
    pub part_no: Option<String>,
    pub vendor: Option<String>,
    pub mem_type: Option<String>, // DDR4 / DDR5 ...
    pub capacity_mb: Option<MebiBytes>,
    pub speed_mhz: Option<u32>,
}

impl MemStick {
    /// 插槽是否实插。
    ///
    /// 故意不存 `populated: bool` 字段：它与 `capacity_mb` 会表达同一件事，
    /// 两个字段可能互相矛盾（非法状态可表示）。SMBIOS 里空槽的 Size 字段为 0，
    /// 因此容量缺失即空槽，用方法派生出来即可。
    pub fn is_populated(&self) -> bool {
        self.capacity_mb.is_some()
    }
}

/// GPU 适配器类型，区分真实物理显卡与虚拟/软件渲染器。
///
/// 间接显示驱动（向日葵 OrayIddDriver、RDP 的 Microsoft Remote Display Adapter 等）
/// 与软件渲染器（WARP / Microsoft Basic Render Driver）没有真实硬件，却在枚举里
/// 与独显/核显并列出现；不分类会让用户把虚拟卡当成独显，误导硬件盘点。
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GpuKind {
    /// 真实物理 GPU（独显或核显），来自 DXGI 非软件适配器。
    #[default]
    Physical,
    /// 间接/虚拟显示驱动（OrayIddDriver、RDP、USB 虚拟屏等），非真实显卡。
    Virtual,
    /// 软件渲染器（WARP / Microsoft Basic Render Driver），无真实硬件。
    Software,
}

impl GpuKind {
    /// 中文展示标签。
    pub fn label(&self) -> &'static str {
        match self {
            GpuKind::Physical => "物理显卡",
            GpuKind::Virtual => "虚拟/间接显示",
            GpuKind::Software => "软件渲染器",
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Gpu {
    pub name: String,
    pub vram_mb: Option<MebiBytes>,
    /// 板卡品牌（AIB 厂商，如华硕/微星/七彩虹），与芯片厂（NVIDIA/AMD/Intel）不同。
    /// 来自 DXGI 子系统厂商 ID（`DXGI_ADAPTER_DESC1.SubSysId` 低 16 位）或名称关键词兜底；
    /// 老 JSON 无此字段时反序列化为 `None`。
    #[serde(default)]
    pub vendor: Option<String>,
    /// 适配器类型：物理显卡 / 虚拟或间接显示 / 软件渲染器（见 `GpuKind`）。
    /// 用于报告里把虚拟卡、软件渲染器与真实独显区分开，避免误导硬件盘点。
    #[serde(default)]
    pub kind: GpuKind,
    /// 遥测：仅当真的拿到 NVML/ADL 数据时才为 `Some`。
    /// 替代 C++ 版「tempC == 0 表示未识别」的哨兵值设计。
    pub telemetry: Option<GpuTelemetry>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct GpuTelemetry {
    pub temp_c: Option<i32>,
    pub fan_pct: Option<u32>,
    pub power_w: Option<u32>,
    pub util_pct: Option<u32>,
    pub mem_used_mb: Option<MebiBytes>,
    pub clock_mhz: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Display {
    pub name: Option<String>,
    pub vendor: Option<String>,
    pub size_inch: Option<String>,
    /// 序列号（EDID 解析，可选）。
    #[serde(default)]
    pub serial: Option<String>,
    /// 生产年份（EDID：1990 + 年份偏移；可选）。
    #[serde(default)]
    pub manufacture_year: Option<u16>,
    /// 生产周（1–53；EDID 解析，可选）。
    #[serde(default)]
    pub manufacture_week: Option<u8>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub refresh_hz: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NicKind {
    #[default]
    Physical,
    Virtual,
    Software,
}
impl NicKind {
    pub fn label(&self) -> &'static str {
        match self {
            NicKind::Physical => "物理网卡",
            NicKind::Virtual => "虚拟/软件网卡",
            NicKind::Software => "软件回环",
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Nic {
    pub desc: Option<String>,
    pub mac: Option<String>,
    pub ipv4: Vec<String>,
    pub ipv6: Option<String>,
    pub up: bool,
    /// 链路速率（单位 bits/s，取自 `IP_ADAPTER_ADDRESSES_LH::TransmitLinkSpeed`）。
    /// 0 / 缺失一律视为未知（未连接或速率不可得），仅用于报告展示。
    #[serde(default)]
    pub speed_bps: Option<u64>,
    /// 网卡类型：物理网卡 / 虚拟或软件网卡 / 软件回环（见 `NicKind`）。
    /// 用于报告里把 VPN / 虚拟交换机 / 隧道等软件网卡与真实物理网卡区分开，
    /// 避免硬件盘点被 Hyper-V、WSL、WireGuard 等噪声污染。
    #[serde(default)]
    pub kind: NicKind,
    /// 厂商（从 `desc` 启发式提取：Realtek / Intel / Broadcom 等）。
    /// 仅用于报告中文展示；`desc` 本身保持原始完整名称。
    #[serde(default)]
    pub vendor: Option<String>,
}

/// 从网卡描述串启发式提取厂商名（英文 key，报告层再做中文映射）。
///
/// 只收描述里稳定出现、可被唯一归属的厂商词；匹配大小写不敏感、取首个命中。
/// 未命中返回 `None`（报告层按"未知"处理）。这是展示辅助字段，不影响 `desc` 本身。
///
/// 放在 core 而非 platform：本函数只做纯字符串判断、无 FFI。放 core 的好处是
/// 报告渲染层在 `Nic.vendor` 缺失时也能从 `desc` 兜底推导 —— 兼容老 JSON
/// （采集期尚未填充 vendor 字段），无需回真机重采即可让存档报告显示中文厂商名。
pub fn nic_vendor_from_desc(desc: &str) -> Option<String> {
    const VENDORS: &[&str] = &[
        "Realtek",
        "Intel",
        "Broadcom",
        "Qualcomm",
        "MediaTek",
        "Marvell",
        "Atheros",
        "TP-LINK",
        "TP-Link",
        "D-Link",
        "D-LINK",
        "Killer",
        "Microsoft",
        "Ralink",
        "Rivet",
        "ASUS",
        "ASUSTeK",
        "EDIMAX",
        "Mercury",
        "Netgear",
        "Tenda",
        "Huawei",
        "ZTE",
        "Xiaomi",
        "Mellanox",
    ];
    let d = desc.trim();
    for v in VENDORS {
        if d.len() >= v.len()
            && d[..v.len()].eq_ignore_ascii_case(v)
            // 词边界：厂商词后不能紧跟字母/数字（避免 "Realtekx" 误命中 "Realtek"）。
            // 允许后接空格、'('（如 "Intel(R) ..."）、'-' 等分隔符或串尾。
            && (v.len() == d.len() || !d.as_bytes()[v.len()].is_ascii_alphanumeric())
        {
            return Some(v.to_string());
        }
    }
    None
}

/// 自动地址识别（GUI 与报告共用单一数据源）。
///
/// - IPv4 `169.254.0.0/16`：**APIPA** —— 网卡开了 DHCP 却拿不到地址时 Windows 自动分配的
///   私有自动配置地址（典型："每张卡看起来都有 IP"，实则场地 DHCP 没通）。
/// - IPv6 `fe80::/10`：**链路本地单播** —— 协议栈一启用即自动生成，无需配置。
///
/// 用途：资产巡检时把"拿不到 DHCP 的兜底地址"与配置好的业务 IP 区分开。
pub fn auto_ip_tag(ip: &str) -> Option<&'static str> {
    let t = ip.trim();
    if t.starts_with("169.254.") {
        return Some("APIPA 自动地址");
    }
    if t.to_ascii_lowercase().starts_with("fe80:") {
        return Some("链路本地");
    }
    None
}

/// 网卡是否需要把「自动地址」也显示出来（GUI 与报告共用单一数据源，2026-09-16 Z.W. 定）。
///
/// - 有真实地址（非自动）→ `false`：只显示真实地址；
/// - 一个真实地址都没有 **且已连接** → `true`：显示自动地址（带标注），
///   因为"连上了却没拿到真实 IP"本身就是值得提示的异常；
/// - 一个真实地址都没有 **且未连接** → `false`：未连接已由状态表达，无需自动地址刷屏。
pub fn nic_should_show_auto_ip(n: &Nic) -> bool {
    let has_real_v4 = n.ipv4.iter().any(|ip| auto_ip_tag(ip).is_none());
    let has_real_v6 = match &n.ipv6 {
        Some(v) => !v.is_empty() && auto_ip_tag(v).is_none(),
        None => false,
    };
    !(has_real_v4 || has_real_v6) && n.up
}

/// 网卡 IPv4 展示列表（GUI 与报告共用单一数据源）。
///
/// 优先只显示真实地址；仅「已连接但没有任何真实地址」时才把 APIPA 自动地址也列入，
/// 避免把"拿不到 DHCP 的兜底地址"误当成配置好的业务 IP（见 [`nic_should_show_auto_ip`]）。
pub fn nic_ipv4_for_display(n: &Nic) -> Vec<String> {
    let show_auto = nic_should_show_auto_ip(n);
    n.ipv4
        .iter()
        .filter(|ip| auto_ip_tag(ip).is_none() || show_auto)
        .cloned()
        .collect()
}

/// 链路速率是否为有效可读值：0 / `u64::MAX` 都是哨兵（未连接或速率不可得），不渲染。
pub fn real_link_speed(bps: u64) -> bool {
    bps > 0 && bps != u64::MAX
}

/// 把链路速率（bits/s）格式化为人读串：`1 Gbps` / `2.5 Gbps` / `100 Mbps` 等。
/// 十进制（Mbps/Gbps 行业惯例就是 10^6 / 10^9，非 2^20 / 2^30）。
pub fn fmt_link_speed(bps: u64) -> String {
    const G: f64 = 1_000_000_000.0;
    const M: f64 = 1_000_000.0;
    const K: f64 = 1_000.0;
    let v = bps as f64;
    if v >= G {
        format!("{} Gbps", trim_decimal(v / G))
    } else if v >= M {
        format!("{} Mbps", trim_decimal(v / M))
    } else if v >= K {
        format!("{} Kbps", trim_decimal(v / K))
    } else {
        format!("{bps} bps")
    }
}

/// 最多 1 位小数，去掉多余的 `.0`（如 `1.0` -> `1`、`2.5` -> `2.5`）。
fn trim_decimal(f: f64) -> String {
    let s = format!("{f:.1}");
    match s.strip_suffix(".0") {
        Some(stripped) => stripped.to_string(),
        None => s,
    }
}

#[cfg(test)]
mod timestamp_tests {
    use super::*;

    #[test]
    fn civil_from_days_matches_known_dates() {
        // 手写历法最容易错，用几个有代表性的日期钉死：
        // epoch、epoch 前一天（负 era，考验 div_euclid）、以及两个闰年的 2/29。
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    #[test]
    fn local_timestamp_has_expected_shape() {
        // 不断言具体时刻（会随运行时间变），只钉格式 YYYY-MM-DD HH:MM:SS。
        let s = local_timestamp_string();
        assert_eq!(s.len(), 19, "实际: {s}");
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
        assert_eq!(&s[13..14], ":");
        assert_eq!(&s[16..17], ":");
        assert!(
            s[..4].parse::<u32>().is_ok(),
            "年份应可解析为数字，实际: {s}"
        );
    }

    #[test]
    fn local_timestamp_from_systemtime_uses_fixed_offset() {
        // 钉一个已知 UTC 时刻，验证固定 UTC+8（无夏令时）换算正确。
        // 2024-01-01 00:00:00 UTC = 2024-01-01 08:00:00 中国本地。
        use std::time::{Duration, UNIX_EPOCH};
        let t = UNIX_EPOCH + Duration::from_secs(1_704_067_200);
        assert_eq!(
            local_timestamp_from_systemtime(t),
            "2024-01-01 08:00:00",
            "中国本地时间须为 UTC+8"
        );
    }

    #[test]
    fn normalize_bios_date_reorders_us_format() {
        // SMBIOS Type 0 多为美式 MM/DD/YYYY，中文界面需归一化成 YYYY-MM-DD。
        assert_eq!(normalize_bios_date("04/03/2026"), "2026-04-03");
        assert_eq!(normalize_bios_date("11/13/2025"), "2025-11-13");
    }

    #[test]
    fn normalize_bios_date_passthrough_when_not_us() {
        // 已是 ISO 格式或未知写法，原样返回，避免误改。
        assert_eq!(normalize_bios_date("2023-01-01"), "2023-01-01");
        assert_eq!(normalize_bios_date("Unknown"), "Unknown");
        assert_eq!(normalize_bios_date(""), "");
    }

    #[test]
    fn format_uptime_buckets() {
        // 与 system 在线时长报告口径一致：天/小时/分/秒 分桶。
        assert_eq!(format_uptime(0), "约 0 秒");
        assert_eq!(format_uptime(59), "约 59 秒");
        assert_eq!(format_uptime(60), "约 1 分");
        assert_eq!(format_uptime(3600), "约 1 小时");
        assert_eq!(format_uptime(3660), "约 1 小时 1 分");
        assert_eq!(format_uptime(86400), "约 1 天");
        // 259200s = 恰好 3 天（无余小时）。
        assert_eq!(format_uptime(259200), "约 3 天");
        // 5073600s = 58 天余 17 小时（58*86400 + 17*3600 = 5073600）。
        assert_eq!(format_uptime(5073600), "约 58 天 17 小时");
    }
}

#[cfg(test)]
mod nic_vendor_tests {
    use super::*;

    #[test]
    fn nic_vendor_from_desc_picks_known_vendor() {
        assert_eq!(
            nic_vendor_from_desc("Realtek PCIe GBE Family Controller").as_deref(),
            Some("Realtek")
        );
        assert_eq!(
            nic_vendor_from_desc("Intel(R) Ethernet Connection Adapter").as_deref(),
            Some("Intel")
        );
        assert_eq!(
            nic_vendor_from_desc("Mellanox ConnectX-3 Ethernet Adapter").as_deref(),
            Some("Mellanox")
        );
        // 厂商词必须位于词首且后接空格/结尾，避免误命中（如 "Realtekx"）
        assert_eq!(nic_vendor_from_desc("SomeRealtekClone"), None);
        assert_eq!(nic_vendor_from_desc("Unknown Network Adapter"), None);
    }
}

#[cfg(test)]
mod nic_ip_tests {
    use super::*;

    fn nic(up: bool, ipv4: &[&str], ipv6: Option<&str>) -> Nic {
        Nic {
            desc: Some("Realtek PCIe GBE".into()),
            ipv4: ipv4.iter().map(|s| s.to_string()).collect(),
            ipv6: ipv6.map(|s| s.to_string()),
            up,
            ..Default::default()
        }
    }

    #[test]
    fn auto_ip_tag_detects_apipa_and_link_local() {
        assert_eq!(auto_ip_tag("169.254.1.2"), Some("APIPA 自动地址"));
        assert_eq!(auto_ip_tag("fe80::1"), Some("链路本地"));
        assert_eq!(auto_ip_tag("FE80::ABCD"), Some("链路本地"));
        assert_eq!(auto_ip_tag("192.168.1.10"), None);
        assert_eq!(auto_ip_tag("10.0.0.5"), None);
    }

    #[test]
    fn ipv4_for_display_shows_all_real_ips() {
        // 多 IPv4 全部保留（面板/报告据此显示多 IP）。
        let n = nic(true, &["192.168.1.10", "10.0.0.5"], None);
        assert_eq!(
            nic_ipv4_for_display(&n),
            vec!["192.168.1.10".to_string(), "10.0.0.5".to_string()]
        );
    }

    #[test]
    fn ipv4_for_display_hides_apipa_when_real_present() {
        // 有真实地址时，APIPA 兜底地址隐藏。
        let n = nic(true, &["192.168.1.10", "169.254.1.2"], None);
        assert_eq!(nic_ipv4_for_display(&n), vec!["192.168.1.10".to_string()]);
    }

    #[test]
    fn ipv4_for_display_shows_apipa_when_only_auto_and_up() {
        // 已连接但只拿到 APIPA → 显示（带标注由渲染层做），提示异常。
        let n = nic(true, &["169.254.1.2"], None);
        assert_eq!(nic_ipv4_for_display(&n), vec!["169.254.1.2".to_string()]);
    }

    #[test]
    fn ipv4_for_display_hides_apipa_when_only_auto_and_down() {
        // 未连接 → 不显示自动地址（状态已表达）。
        let n = nic(false, &["169.254.1.2"], None);
        assert!(nic_ipv4_for_display(&n).is_empty());
    }
}

/// SMART 数据采集来源（哪条 API 路径拿到了数据）。
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SmartSource {
    /// 没拿到（未尝试 / 接口不支持 / 未提权）
    #[default]
    None,
    /// NVMe Get Log Page（LID 0x02）：`IOCTL_STORAGE_QUERY_PROPERTY` + `StorageDeviceProtocolSpecificProperty`
    NvmeLogPage,
    /// ATA SMART READ DATA：`IOCTL_ATA_PASS_THROUGH`，Features=0xD0 / Command=0xB0
    AtaSmart,
    /// WMI `MSFT_StorageReliabilityCounter`（命名空间 `ROOT\Microsoft\Windows\Storage`）。
    /// NVMe 在 VMD/部分企业级存储栈下 `DeviceIoControl` 被拦时，走这条与 IOCTL 完全独立的
    /// 内核路径兜底，至少能拿回温度 / 健康态（通电时间经此路径也拿不到，如实留空）。
    StorageReliabilityCounter,
}

/// 硬盘健康状态。**绝不能默认成 Healthy**——拿不到就是 `Unknown`。
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HealthStatus {
    /// 拿不到 / 无法判定
    #[default]
    Unknown,
    Healthy,
    Warning,
    Critical,
}

/// 单盘 SMART 快照。**所有数值字段都是 `Option`：拿不到就 `None`，绝不给 0。**
///
/// 纪律（作者硬性要求）：
/// - "通电 0 小时"看起来像全新盘，比没数据危险，所以解析失败/全零 buffer 一律 `None`；
/// - "读不到"绝不等于"健康"，故 `health_status` 默认 `Unknown`；
/// - `health_percent`（剩余寿命）只在有确凿依据时给值——目前仅 NVMe `PercentageUsed`
///   换算（剩余 = 100 - 已用，已用可超 100 需饱和到 0）；ATA 路径不瞎猜，恒为 `None`。
// `#[serde(default)]`：将来新增字段时，旧 JSON 里的 `smart` 对象不会因为缺字段
// 而整体解析失败（区别于 `PhysicalDisk.smart` 整体缺失 —— 那个 Option 已天然容忍）。
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct DiskSmart {
    /// 数据来源（哪条采集路径）。
    pub source: SmartSource,
    /// 通电时间（小时）。NVMe 取 PowerOnHours(0x80)；ATA 取属性 0x09。
    pub power_on_hours: Option<u32>,
    /// 通电时长（人类可读），由 `power_on_hours` 换算（如 "约 4 年 10 个月"）。
    /// 仅用于展示/导出，原始裸小时数仍以 `power_on_hours` 为准；二者同源，不独立采集。
    /// 构造时恒为 `None`，由 `HardwareInfo::enrich_power_on_durations` 在导出前统一填充。
    pub power_on_duration: Option<String>,
    /// 通电次数。仅 NVMe(0x70) 提供。
    pub power_cycles: Option<u32>,
    /// 健康状态。读不到就是 `Unknown`，绝不默认 `Healthy`。
    pub health_status: HealthStatus,
    /// 剩余寿命百分比。**仅在有确凿依据时给值**（NVMe `PercentageUsed` 换算），ATA 恒为 `None`。
    pub health_percent: Option<u8>,
    /// NVMe 已用寿命百分比，可 >100（255 饱和）。
    pub percentage_used: Option<u8>,
    /// 复合温度（摄氏度）。NVMe 取 0x01（开尔文减 273）；ATA 厂商单位不统一，恒为 `None`。
    pub temperature_c: Option<i16>,
    /// 介质/数据完整性错误数。NVMe(0xA0) / ATA 属性 0xC6。
    pub media_errors: Option<u64>,
    /// 不安全关机次数。仅 NVMe(0x90)。
    pub unsafe_shutdowns: Option<u64>,
    /// 主机已写入字节数。NVMe DataUnitsWritten(0x30) × 512000。
    pub host_written_bytes: Option<u64>,
    /// 重映射扇区数。ATA 属性 0x05。
    pub reallocated_sectors: Option<u64>,
    /// 待映射（重映射待定）扇区数。ATA 属性 0xC5。
    pub pending_sectors: Option<u32>,
    /// 拿不到的原因（"需管理员权限" / "USB 设备不支持" / ...）。渲染层据此明确提示，绝不编造。
    pub unavailable_reason: Option<String>,
}

/// 单块物理硬盘（`\\.\PhysicalDriveN`，IOCTL 读取）。
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct PhysicalDisk {
    pub index: u32,
    pub model: Option<String>,
    pub iface: Option<String>, // NVMe / SATA / USB ...
    pub media: Option<String>, // SSD / HDD
    pub serial: Option<String>,
    /// 固件版本（来自 `STORAGE_DEVICE_DESCRIPTOR.ProductRevisionOffset`，ATA / NVMe 均可得）。
    /// 资产盘点中配合型号 + 序列号构成完整身份，用于 RMA / 批次隐患比对。
    /// 反序列化旧 JSON（无此字段）默认 `None`，不报错。
    #[serde(default)]
    pub firmware: Option<String>,
    pub size_bytes: Option<u64>,
    /// SMART 健康/通电快照。旧 JSON（无此字段）反序列化时默认 `None`，不报错。
    #[serde(default)]
    pub smart: Option<DiskSmart>,
    /// 存储后端说明：盘位于 RAID 控制器 / Storage Spaces / 虚拟磁盘之后时填写，
    /// 提示完整 SMART 需 storcli/megacli 等厂商工具，避免把控制器的合成数据误当真实。
    /// 反序列化旧 JSON（无此字段）默认 `None`，不报错。
    #[serde(default)]
    pub storage_backend: Option<String>,
}

/// 音频设备类型，由名称 / 角色 / VID 启发式判定（见 platform `audio.rs`）。
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AudioKind {
    /// 未识别 / 无法判定
    #[default]
    Unknown,
    /// 音箱 / 扬声器（板载集成声卡、外接音箱）
    Speaker,
    /// 耳机（无麦）
    Headphone,
    /// 耳麦（带麦克风）
    Headset,
}

impl AudioKind {
    /// 中文显示名。
    pub fn label(self) -> &'static str {
        match self {
            AudioKind::Unknown => "未知",
            AudioKind::Speaker => "音箱",
            AudioKind::Headphone => "耳机",
            AudioKind::Headset => "耳麦",
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct AudioDevice {
    pub name: String,
    /// bit 0 = 默认播放，bit 1 = 默认录音，0 = 未识别
    pub role: u8,
    /// 设备类型：音箱 / 耳机 / 耳麦 / 未知（启发式分类）。
    #[serde(default)]
    pub kind: AudioKind,
    /// 是否蓝牙音频设备（设备名/枚举器含 Bluetooth / 蓝牙 / BTHENUM）。
    #[serde(default)]
    pub is_bluetooth: bool,
    /// USB 音频方案的 VID（从 `SPDRP_HARDWAREID` 抽取；板载 HDA 为 None）。
    #[serde(default)]
    pub vid: Option<String>,
    /// USB 音频方案的 PID。
    #[serde(default)]
    pub pid: Option<String>,
    /// 厂商（VID 推断或 HID 厂商串）。
    #[serde(default)]
    pub manufacturer: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct UsbDevice {
    pub name: String,
    pub class: Option<String>,
}

/// 输入设备类型（键盘 / 鼠标 / 键鼠一体），由 Raw Input 的 `RIM_TYPE*` 判定。
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InputKind {
    #[default]
    Keyboard,
    Mouse,
    /// 键鼠一体设备：同一 VID:PID 同时被识别为鼠标与键盘（如带指点设备的键盘、
    /// 或无线键鼠套装共用一个接收器）。采集层会把两集合合并为一行，避免被数成
    /// "N 鼠标 + 1 键盘" 而虚增数量。
    Combo,
}

impl InputKind {
    /// 中文显示名。
    pub fn label(&self) -> &'static str {
        match self {
            InputKind::Keyboard => "键盘",
            InputKind::Mouse => "鼠标",
            InputKind::Combo => "键盘/鼠标",
        }
    }
}

/// 键盘 / 鼠标识别结果（Win32 Raw Input + HID）。
///
/// 字段说明：
/// - `name`：优先用 HID 产品串（`HidD_GetProductString`）的真实型号；取不到时回退到
///   `品牌 + VID/PID` 或设备路径。
/// - `vid` / `pid`：从设备名 `VID_xxxx&PID_yyyy` 解析（大写十六进制）。
/// - `manufacturer`：HID 厂商串，或 peripheral_db 由 VID 推断的品牌。
/// - `is_bluetooth`：是否蓝牙设备（设备路径含 `BTHENUM`）。无线键鼠/耳机绕不开，单列标记。
/// - `location`：物理端口（暂未填，留作后续 SetupAPI 增强）。
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct InputDevice {
    pub name: String,
    pub kind: InputKind,
    pub vid: Option<String>,
    pub pid: Option<String>,
    pub manufacturer: Option<String>,
    #[serde(default)]
    pub is_bluetooth: bool,
    pub location: Option<String>,
}

/// 磁盘分区信息（对应 C++ 的 `DiskInfo`）。
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Volume {
    pub letter: String, // "C:"
    pub type_name: Option<String>,
    pub fs_name: Option<String>,
    pub sectors_per_cluster: Option<u32>,
    pub bytes_per_sector: Option<u32>,
    pub cluster_bytes: Option<u32>,
    pub total_bytes: Option<u64>,
    pub free_bytes: Option<u64>,
    pub vol_label: Option<String>,
    /// 主 PhysicalDrive 编号
    pub disk_index: Option<u32>,
}

// ============================================================
// 右键菜单功能（数据驱动）
// ============================================================

/// 功能内部标识。用 newtype 避免与任意字符串混用。
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FeatureId(pub &'static str);

/// 功能类型。**枚举携带各自所需的数据**，非法组合无法构造。
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureKind {
    /// 静态 verb：`shell\<verb>\command = 命令行`
    RunCommand { command: &'static str },
    /// 调用 Shell DLL 导出的切换函数（无 UAC）
    ShellExtension { toggle_fn: &'static str },
}

/// 一个右键菜单功能。
///
/// 相比 C++ 版：`regParent/regParent2/regParent3` 三个平行字段合并为
/// `reg_parents` 切片；`kind` 与 `command`/`clsid`/`toggleArg` 的
/// 松散组合改为枚举携带数据，非法状态在编译期即不可表示。
// 只有 Serialize：`&'static str` / `&'static [&'static str]` 无法从一次性读入的
// 缓冲区借出 'static 生命周期。它是 FEATURES 静态表项，本就不需要反序列化。
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feature {
    pub id: FeatureId,
    pub display_name: &'static str,
    pub description: &'static str,
    /// 1~3 个注册表父路径（相对 `HKEY_CLASSES_ROOT`）
    pub reg_parents: &'static [&'static str],
    pub verb: &'static str,
    pub kind: FeatureKind,
    pub menu_icon: Option<&'static str>,
    /// Win11 新菜单直显用的 COM CLSID
    pub win11_clsid: Option<&'static str>,
}

/// 注册表根键（抽象为平台无关，便于测试）。
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegRoot {
    ClassesRoot,  // HKCR（提权后等价 HKLM\Software\Classes）
    CurrentUser,  // HKCU\Software\Classes
    LocalMachine, // HKLM（只读场景：OS 版本、ProductType 等）
}

/// 注册表值类型。
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegValueKind {
    Sz,
    ExpandSz,
}

#[cfg(test)]
mod power_on_duration_tests {
    use super::*;

    #[test]
    fn format_power_on_duration_buckets() {
        // 边界分桶：年/月/天/小时。
        assert_eq!(format_power_on_duration(0), "约 0 小时");
        assert_eq!(format_power_on_duration(23), "约 23 小时");
        assert_eq!(format_power_on_duration(24), "约 1 天");
        // 入参是小时：365 小时 = 15 天，而非 1 年。
        assert_eq!(format_power_on_duration(365), "约 15 天");
        // 365 天（8760 小时）→ 恰好 1 年、无余月。
        assert_eq!(format_power_on_duration(365 * 24), "约 1 年");
        // 42367h → 1765 天 → 4 年余 305 天 ≈ 10 个月。
        assert_eq!(format_power_on_duration(42367), "约 4 年 10 个月");
        // 57254h → 2385 天 → 6 年余 195 天 ≈ 6 个月。
        assert_eq!(format_power_on_duration(57254), "约 6 年 6 个月");
    }

    #[test]
    fn enrich_fills_duration_from_hours_and_leaves_none() {
        // 构造含两块盘的 HardwareInfo：一块有通电小时、一块无（如 VMD 下 NVMe）。
        let mut info = HardwareInfo {
            disks: vec![
                PhysicalDisk {
                    index: 0,
                    smart: Some(DiskSmart {
                        source: SmartSource::AtaSmart,
                        power_on_hours: Some(42367),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                PhysicalDisk {
                    index: 2,
                    smart: Some(DiskSmart {
                        source: SmartSource::StorageReliabilityCounter,
                        power_on_hours: None, // VMD 不暴露通电时间
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        // 富化前：duration 应为 None。
        assert_eq!(
            info.disks[0].smart.as_ref().unwrap().power_on_duration,
            None
        );

        info.enrich_power_on_durations();

        // 有小时数的盘：填上可读时长。
        assert_eq!(
            info.disks[0]
                .smart
                .as_ref()
                .unwrap()
                .power_on_duration
                .as_deref(),
            Some("约 4 年 10 个月")
        );
        // 拿不到通电时间的盘：duration 仍 None，绝不编造。
        assert_eq!(
            info.disks[1].smart.as_ref().unwrap().power_on_duration,
            None
        );
        // 原始小时数不受影响。
        assert_eq!(
            info.disks[0].smart.as_ref().unwrap().power_on_hours,
            Some(42367)
        );
    }
}
