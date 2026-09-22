//! 系统信息源：OS 版本、计算机/用户名、物理内存总量、是否服务器。
//!
//! OS 版本从注册表读取（与 C++ 版一致），避开 `VerifyVersionInfo` 受 manifest
//! 影响"说谎"的问题。架构直接用 Rust 的 `std::env::consts::ARCH`，无需 Win32 API。

use std::mem::size_of;

use windows::Win32::System::SystemInformation::{
    GetSystemInfo, GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX, SYSTEM_INFO,
};

use crate::registry::WinRegistry;
use winsuitex_core::error::{Error, Result};
use winsuitex_core::model::{MachineInfo, MebiBytes, OsInfo, RegRoot};
use winsuitex_core::traits::{CollectContext, HardwareSource, Registry};

/// 把 `std::env::consts::ARCH` 归一成 C++ 版报告用的写法。
///
/// Rust 给的是 LLVM 三元组的 `x86_64` / `aarch64`，而 C++ 版输出 `x64` / `ARM64`。
/// 报告要能与旧版 HTML 直接 diff，所以在这里统一，而不是在渲染层做字符串替换。
fn arch_label() -> String {
    match std::env::consts::ARCH {
        "x86_64" => "x64".to_string(),
        "x86" => "x86".to_string(),
        "aarch64" => "ARM64".to_string(),
        other => other.to_string(),
    }
}

/// 逻辑处理器数。失败返回 `None` —— 拿不到就是拿不到，不编造。
pub(crate) fn logical_processors() -> Option<u32> {
    let mut si = SYSTEM_INFO::default();
    // SAFETY: si 是栈上的有效结构体，GetSystemInfo 只写入、不读取未初始化字段。
    unsafe { GetSystemInfo(&mut si) };
    let n = si.dwNumberOfProcessors;
    (n > 0).then_some(n)
}

/// 系统已开机时长（秒），经 `GetTickCount64` 取得（自系统启动以来的毫秒数）。
///
/// 跨所有 Windows 版本可用（含 Win7 / Server 2008 R2），且**不需要 COM / WMI**，
/// 不引入 `CoInitialize` 相关的堆损坏风险（见 `win7_compat.rs` 的纪律）。
/// 49.7 天回绕对资产报告无影响（我们只展示"约 N 天"）。
pub(crate) fn uptime_seconds() -> Option<u64> {
    // SAFETY: GetTickCount64 无参数、无副作用，返回自启动起的毫秒计数（u64）。
    let ms = unsafe { GetTickCount64() };
    Some(ms / 1000)
}

/// 系统开机时间（本地时间串 `YYYY-MM-DD HH:MM:SS`），由 `now − uptime` 推算。
///
/// 不查 `Win32_OperatingSystem.LastBootUpTime`（WMI/COM），避免在无盘服务器 / 老系统上
/// 因 COM 初始化引入额外失败面；`GetTickCount64` 即可满足资产盘点精度需求。
/// 推算依赖本机时钟准确，对"多久没重启"这类判断足够。
pub(crate) fn boot_time_local() -> Option<String> {
    // SAFETY: 同上，GetTickCount64 无参数、无副作用。
    let ms = unsafe { GetTickCount64() };
    let now = std::time::SystemTime::now();
    now.checked_sub(std::time::Duration::from_millis(ms))
        .map(winsuitex_core::model::local_timestamp_from_systemtime)
}

const CUR_VER: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
const PRODUCT_OPTIONS: &str = r"SYSTEM\CurrentControlSet\Control\ProductOptions";

pub struct SystemSource {
    reg: WinRegistry,
}

impl SystemSource {
    pub fn new() -> Self {
        Self { reg: WinRegistry }
    }
}

impl Default for SystemSource {
    fn default() -> Self {
        Self::new()
    }
}

impl HardwareSource for SystemSource {
    fn name(&self) -> &'static str {
        "system"
    }

    fn collect(&self, ctx: &mut CollectContext) -> Result<()> {
        let lm = RegRoot::LocalMachine;

        // DisplayVersion（Win10 20H2+）优先，回退 ReleaseId（老版本）
        let version = self
            .reg
            .read_string(lm, CUR_VER, Some("DisplayVersion"))
            .unwrap_or(None)
            .or_else(|| {
                self.reg
                    .read_string(lm, CUR_VER, Some("ReleaseId"))
                    .unwrap_or(None)
            });

        let build = self
            .reg
            .read_string(lm, CUR_VER, Some("CurrentBuild"))
            .unwrap_or(None)
            .or_else(|| {
                self.reg
                    .read_string(lm, CUR_VER, Some("CurrentBuildNumber"))
                    .unwrap_or(None)
            });

        ctx.set_os(OsInfo {
            product: self
                .reg
                .read_string(lm, CUR_VER, Some("ProductName"))
                .unwrap_or(None),
            version,
            build,
            arch: Some(arch_label()),
            logical_processors: logical_processors(),
            boot_at: boot_time_local(),
            uptime_seconds: uptime_seconds(),
        });

        // 计算机名/用户名：环境变量与 GetComputerNameW/GetUserNameW 同源，
        // 用 env 可省掉两个 FFI 调用（结果等价）。
        ctx.set_machine(MachineInfo {
            computer_name: std::env::var("COMPUTERNAME").ok(),
            user_name: std::env::var("USERNAME").ok(),
        });

        let product_type = self
            .reg
            .read_string(lm, PRODUCT_OPTIONS, Some("ProductType"))
            .unwrap_or(None);
        ctx.set_is_server(matches!(
            product_type.as_deref(),
            Some("ServerNT") | Some("LanmanNT")
        ));

        // 物理内存总量
        // MEMORYSTATUSEX 是纯数值结构（u32/u64/u16/u32，无 HSTRING 字段），
        // 可直接 struct 字面量 + 默认填充初始化，消除 clippy field_reassign_with_default。
        let mut ms = MEMORYSTATUSEX {
            dwLength: size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        // SAFETY: ms 是本函数栈上的有效结构体，dwLength 已正确初始化。
        unsafe { GlobalMemoryStatusEx(&mut ms) }
            .map_err(|e| Error::collect("system", format!("GlobalMemoryStatusEx: {e}")))?;
        ctx.set_memory_total(MebiBytes::from_bytes_floor(ms.ullTotalPhys));

        Ok(())
    }
}
