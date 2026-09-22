//! `winsuitex` 命令行入口。
//!
//! 退出码契约与 C++ 版**严格一致**（已有批量巡检脚本依赖）：
//! - `--report` 成功 = 0，失败 = 2
//! - `--selftest` 返回失败项数（0 = 全通过）
//! - 参数错误 = 2

use std::process::ExitCode;

use serde::{Deserialize, Serialize};
use winsuitex_core::feature::FEATURES;
use winsuitex_core::model::{local_timestamp_string, FeatureId, HardwareInfo, MebiBytes, Volume};
use winsuitex_core::traits::{collect_all, wsx_trace_enabled_pub};

/// 快照里的诊断项。core 的 `Diagnostic.source` 是 `&'static str`，不能被
/// 反序列化（无法从读入的缓冲区借出 `'static`），这里用持有 String 的镜像类型。
#[derive(Serialize, Deserialize)]
struct DiagnosticRow {
    source: String,
    message: String,
}

/// Win7 兼容的控制台输出垫片。
///
/// 背景：本项目的 Win7 通用 exe 用 nightly + `panic=abort` 构建。实测在 Win7/Server
/// 2008 R2 上，std 自带的 Windows 控制台 writer（`println!`/`eprintln!` 走的那条路）
/// 走到 `WriteConsoleW` 时会崩溃（0xc0000005 / `panic!("failed printing to stdout")`），
/// 而 `panic=abort` 下任何 panic 都会直接让进程崩——于是 `--version` 一行带中文的
/// 横幅打印就把整个采集流程拖崩（好在 JSON/HTML 走管道重定向，已先落盘）。
///
/// 这里**完全绕开 std 的 stdout/stderr writer**，自己用 Win32 API 写：
/// - 真实控制台 → `WriteConsoleW`（UTF-16，与代码页无关，中文永不失真/不崩溃）；
/// - 被重定向成管道/文件 → `WriteConsoleW` 失败，退回 `WriteFile` 写原始 UTF-8 字节
///   （管道另一端按 UTF-8 解码，与 --json - 的既有约定一致）；
/// - **任何错误都静默忽略**，绝不以写失败为由 panic/崩溃。
#[cfg(windows)]
mod console_out {
    use std::ffi::c_void;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    const STD_OUTPUT_HANDLE: i32 = -11;
    const STD_ERROR_HANDLE: i32 = -12;

    extern "system" {
        fn GetStdHandle(nStdHandle: i32) -> *mut c_void;
        fn WriteConsoleW(
            hConsoleOutput: *mut c_void,
            lpBuffer: *const u16,
            nNumberOfCharsToWrite: u32,
            lpNumberOfCharsWritten: *mut u32,
            lpReserved: *mut c_void,
        ) -> i32;
        fn WriteFile(
            hFile: *mut c_void,
            lpBuffer: *const u8,
            nNumberOfBytesToWrite: u32,
            lpNumberOfBytesWritten: *mut u32,
            lpOverlapped: *mut c_void,
        ) -> i32;
    }

    fn write_to(handle: *mut c_void, text: &str) {
        if handle.is_null() {
            return;
        }
        // 真实控制台：UTF-16 直写，代码页无关，中文安全。
        let wide: Vec<u16> = OsStr::new(text).encode_wide().chain(Some(0)).collect();
        let mut written = 0u32;
        let ok = unsafe {
            WriteConsoleW(
                handle,
                wide.as_ptr(),
                (wide.len() - 1) as u32,
                &mut written,
                core::ptr::null_mut(),
            )
        };
        if ok != 0 {
            return;
        }
        // 非控制台（管道/文件）：写原始 UTF-8 字节。
        let bytes = text.as_bytes();
        let mut w2 = 0u32;
        unsafe {
            let _ = WriteFile(
                handle,
                bytes.as_ptr(),
                bytes.len() as u32,
                &mut w2,
                core::ptr::null_mut(),
            );
        }
    }

    pub fn out(text: &str) {
        unsafe { write_to(GetStdHandle(STD_OUTPUT_HANDLE), text) }
    }
    pub fn err(text: &str) {
        unsafe { write_to(GetStdHandle(STD_ERROR_HANDLE), text) }
    }
}

#[cfg(not(windows))]
mod console_out {
    pub fn out(text: &str) {
        use std::io::Write;
        let _ = std::io::stdout().write_all(text.as_bytes());
    }
    pub fn err(text: &str) {
        use std::io::Write;
        let _ = std::io::stderr().write_all(text.as_bytes());
    }
}

/// 控制台诊断输出：行为等同 `println!` 但走 [`console_out`]（绕开 std writer，绝不 panic）。
macro_rules! wsx_println {
    ($($arg:tt)*) => {{
        let mut __s = format!($($arg)*);
        __s.push('\n');
        console_out::out(&__s);
    }};
}
/// 控制台诊断输出：行为等同 `print!` 但走 [`console_out`]。
macro_rules! wsx_print {
    ($($arg:tt)*) => {{
        console_out::out(&format!($($arg)*));
    }};
}
/// 控制台诊断输出：行为等同 `eprintln!` 但走 [`console_out`]（stderr）。
macro_rules! wsx_eprintln {
    ($($arg:tt)*) => {{
        let mut __s = format!($($arg)*);
        __s.push('\n');
        console_out::err(&__s);
    }};
}

/// 真机排障埋点：`WSX_TRACE=1` 时把阶段进度打到 **stderr**（无缓冲）。
/// 用途见 core::traits 的说明 —— 进程被硬杀时这是唯一能留下线索的手段。
fn tprintln(msg: &str) {
    if wsx_trace_enabled_pub() {
        wsx_eprintln!("[phase] {}", msg);
    }
}

/// 落盘的采集快照。
///
/// 字段全部 `#[serde(default)]`：别的机器上老版本导出的 JSON（没有 features）
/// 也要能被新版本读回来渲染，否则"拷回 JSON 再比对"这条链路会断。
#[derive(Serialize, Deserialize)]
struct Snapshot {
    /// 采集端 exe 的版本号（四段式 `主.次.修订.编译日期戳`）。
    ///
    /// 存在理由：快照要跨机器归档、事后回看。不带版本就无从判断「这份结果是在某个
    /// 修复之前还是之后采的」——这正是本项目反复踩的坑（分不清新旧 exe）。
    #[serde(default)]
    app_version: String,
    /// 构建时间（本地时间字符串），与 `app_version` 同源，便于脚本比对新旧。
    #[serde(default)]
    build_time: String,
    info: HardwareInfo,
    volumes: Vec<Volume>,
    #[serde(default)]
    diagnostics: Vec<DiagnosticRow>,
    /// 右键功能启用状态。存成 `String` 而非 `FeatureId`：`FeatureId` 持有
    /// `&'static str`，serde 无法从一次性读入的缓冲区借出 `'static` 生命周期。
    #[serde(default)]
    features: Vec<(String, bool)>,
}

impl Snapshot {
    /// 快照里的功能名 → `FeatureId`。
    ///
    /// 只认 `FEATURES` 表里登记过的 id：JSON 是外部输入，未知项直接丢弃，
    /// 不去凭空构造 `FeatureId`（那需要 `Box::leak` 之类的手段，不值得）。
    fn feature_ids(&self) -> Vec<(FeatureId, bool)> {
        self.features
            .iter()
            .filter_map(|(name, on)| {
                FEATURES
                    .iter()
                    .find(|f| f.id.0 == name)
                    .map(|f| (f.id, *on))
            })
            .collect()
    }
}

const USAGE: &str = "\
WinSuiteX — Windows 硬件信息 / 右键增强工具

用法:
  winsuitex --report <路径.html>    无头采集并导出 HTML 报告（批量巡检用）
  winsuitex --json <路径.json>      导出结构化采集快照（跨机器比对 / 批量汇总用）
                                 路径可用 - 表示输出到标准输出（由调用方重定向落盘，
                                 可绕开网吧按进程写盘判定的网维/安全拦截）
  winsuitex --render <入.json> <出.html>
                                    把采集快照渲染成 HTML（在别的机器采集、回来比对用）
  winsuitex --selftest              运行自检，返回失败项数
  winsuitex --version               打印版本号（排查新旧 exe 混用问题用）
  winsuitex --fs-probe [路径]       文件系统探针：诊断写出的文件被神秘删除（网吧环境排查用）
  winsuitex --debug-smbios          打印 SMBIOS 结构清单与解析结果（排查采集缺失）
  winsuitex --dump-smbios <路径>    导出原始 SMBIOS 表字节（用于生成测试夹具）
  winsuitex --help                  显示本帮助

排障用环境变量（针对进程跑到一半就没了 / stdout 重定向 0 字节的真机问题）:
  WSX_TRACE=1                       把每个采集源的执行进度打到 stderr（逐行、无缓冲）。
                                     若进程被硬杀，崩溃前最后一行即指明是哪个源引起的。
  WSX_SKIP=<源名>[,<源名>...]        跳过指定采集源。源名以 WSX_TRACE=1 的输出为准
                                     （smbios / memory.wmi / system / gpu.dxgi / display.gdi / nic /
                                       disk / physical_disk / usb / hid / audio）。
                                     用于某源在特定机器上崩溃时仍能出报告，跳过会记为降级。
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        wsx_print!("{}", USAGE);
        return ExitCode::SUCCESS;
    }

    if args.iter().any(|a| a == "--version" || a == "-v") {
        wsx_println!("winsuitex {}  (built {})", version_string(), build_time());
        return ExitCode::SUCCESS;
    }

    if let Some(pos) = args.iter().position(|a| a == "--report") {
        let path = match args.get(pos + 1) {
            Some(p) if p == "-" || !p.starts_with('-') => p.clone(),
            _ => "WinSuiteX_report.html".to_string(),
        };
        return run_report(&path);
    }

    if let Some(pos) = args.iter().position(|a| a == "--render") {
        let input = args.get(pos + 1).cloned().unwrap_or_default();
        let output = args.get(pos + 2).cloned().unwrap_or_default();
        if input.is_empty() || output.is_empty() {
            wsx_eprintln!(
                "--render 需要两个参数：输入 JSON 与输出 HTML。\n\n{}",
                USAGE
            );
            return ExitCode::from(2);
        }
        return run_render(&input, &output);
    }

    if let Some(pos) = args.iter().position(|a| a == "--json") {
        let path = match args.get(pos + 1) {
            Some(p) if p == "-" || !p.starts_with('-') => p.clone(),
            _ => "WinSuiteX.json".to_string(),
        };
        return run_json(&path);
    }

    // 排查采集缺失时的第一手段：看固件到底给了哪些结构
    if args.iter().any(|a| a == "--debug-smbios") {
        return debug_smbios();
    }

    if let Some(pos) = args.iter().position(|a| a == "--dump-smbios") {
        let path = match args.get(pos + 1) {
            Some(p) if !p.starts_with('-') => p.clone(),
            _ => "_smbios.bin".to_string(),
        };
        return dump_smbios(&path);
    }

    if args.iter().any(|a| a == "--selftest") {
        return run_selftest();
    }

    if let Some(pos) = args.iter().position(|a| a == "--fs-probe") {
        let path = match args.get(pos + 1) {
            Some(p) if !p.starts_with('-') => p.clone(),
            _ => "fs_probe.txt".to_string(),
        };
        return fs_probe(&path);
    }

    wsx_eprintln!("未识别的参数。\n\n{}", USAGE);
    ExitCode::from(2)
}

/// 四段式版本号：`主.次.修订.编译日期戳(YYYYMMDD)`（本地日期，遵循项目统一规范）。
///
/// 存在理由：`0.1.0` 这个语义号长期不变，真机排查时反复出现「我跑的到底是不是新 exe」
/// 的疑问。把编译日期戳放进版本号，扫一眼就能分辨。
/// 日期戳取不到时区偏移时退回三段式——**绝不显示可能差一天的日期**。
fn version_string() -> String {
    match option_env!("WSX_BUILD_DATE") {
        Some(d) if !d.is_empty() => format!("{}.{}", env!("CARGO_PKG_VERSION"), d),
        _ => env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// 构建时间：本地时间优先，拿不到偏移时退回 UTC（并明确标注 UTC）。
fn build_time() -> &'static str {
    match option_env!("WSX_BUILD_TS_LOCAL") {
        Some(s) if !s.is_empty() => s,
        _ => env!("WSX_BUILD_TS", "unknown"),
    }
}

/// 导出**机器可读**的采集快照。
///
/// 存在理由：HTML 是给人看的，跨机器核对 / 批量汇总需要结构化数据。
/// 在别的机器上跑 `winsuitex --json out.json`，把文件带回来即可逐字段比对，
/// 比解析 HTML 精确得多 —— 这是"在真机上验证准确性"的主要载体。
fn run_json(path: &str) -> ExitCode {
    let sources = winsuitex_platform::default_sources();
    tprintln(&format!(
        "winsuitex {} built {} | run_json: {} sources",
        version_string(),
        build_time(),
        sources.len()
    ));
    let mut result = collect_all(&sources);
    // 未提权：追加一条诊断，使 JSON diagnostics 也提示 NVMe SMART 可能不完整。
    if result.info.admin_elevated == Some(false) {
        result.push_diagnostic(
            "system",
            "未以管理员身份运行：NVMe SMART 寿命百分比/通电时长可能不完整",
        );
    }
    // 采集时间在采集端打进快照（而不是渲染时取当前时间）——
    // 否则拿一份旧 JSON 重新渲染，报告会显示"刚刚采集"，反而误导。
    result.info.collected_at = Some(local_timestamp_string());
    tprintln("run_json: collect_all done");
    winsuitex_platform::link_volumes_to_disks(&mut result.volumes);
    tprintln("run_json: link_volumes_to_disks done");

    // 统一补上通电时长可读字符串（供 JSON 导出；HTML 文案另算，互不影响）。
    // 构造时该字段恒为 `None`，在此集中填充，避免散落各采集路径。
    result.info.enrich_power_on_durations();

    // 功能状态一起存进快照：否则回到开发机用 --render 复现报告时，
    // 这一节会因为读不到目标机的注册表而全部退化成"未启用"，对拍无意义。
    let features = feature_states();
    tprintln("run_json: feature_states done");

    let snapshot = Snapshot {
        app_version: version_string(),
        build_time: build_time().to_string(),
        info: result.info,
        volumes: result.volumes,
        diagnostics: result
            .diagnostics
            .iter()
            .map(|d| DiagnosticRow {
                source: d.source.to_string(),
                message: d.message.clone(),
            })
            .collect(),
        features: features
            .into_iter()
            .map(|(id, on)| (id.0.to_string(), on))
            .collect(),
    };

    let json = match serde_json::to_string_pretty(&snapshot) {
        Ok(s) => s,
        Err(e) => {
            wsx_eprintln!("序列化失败: {}", e);
            return ExitCode::from(2);
        }
    };

    // STDOUT 模式：把快照直接写到标准输出，由调用方用 cmd 重定向落盘。
    // winsuitex.exe 自身不调用 CreateFile 写文件，从而绕开"按本进程写盘"判定的
    // 网维/安全驱动拦截（网吧环境常见）。用 write_all 直接落原始 UTF-8 字节，
    // 不走 print! 的 locale 转码，保证文件内容与 --json <文件> 逐字节一致。
    if path == "-" {
        use std::io::Write;
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        if let Err(e) = lock.write_all(json.as_bytes()) {
            wsx_eprintln!("写入 stdout 失败: {}", e);
            return ExitCode::from(2);
        }
        let _ = lock.flush();
        wsx_eprintln!("JSON 已生成（已输出到 stdout，由调用方重定向落盘）");
        return ExitCode::SUCCESS;
    }

    match write_verified(path, json.as_bytes()) {
        Ok(()) => {
            wsx_println!("JSON 已写入: {}", path);
            wsx_println!(
                "采集摘要: CPU {} 颗 / 内存条 {} 根 / 显卡 {} 块 / 显示器 {} 台 / \
                 物理盘 {} 块 / 分区 {} 个 / 网卡 {} 个 / 键鼠 {} 个 / USB {} 个 / 音频 {} 个",
                snapshot.info.cpus.len(),
                snapshot.info.memory.sticks.len(),
                snapshot.info.gpus.len(),
                snapshot.info.displays.len(),
                snapshot.info.disks.len(),
                snapshot.volumes.len(),
                snapshot.info.nics.len(),
                snapshot.info.input_devices.len(),
                snapshot.info.usb.len(),
                snapshot.info.audio.len(),
            );
            if !snapshot.diagnostics.is_empty() {
                for d in &snapshot.diagnostics {
                    wsx_eprintln!("[warn] 采集降级 [{}]: {}", d.source, d.message);
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            wsx_eprintln!("写入 JSON 失败: {}", e);
            ExitCode::from(2)
        }
    }
}

/// 写文件并在返回 Ok 前**立即回读校验**（文件存在且非空）。
///
/// 为什么不能只信 `fs::write` 的 Ok：部分网吧环境（网维大师/安全卫士的桌面清理、
/// 还原驱动）会在写入后的极短时间内删除或清空非白名单程序新建的文件——
/// fs::write 返回 Ok 只代表当时内核接收了写入，不代表文件真的落盘可读。
/// 实测案例（2026-09-15，真机桌面目录）：exe 报"已写入"，数秒后 `start` 报找不到文件。
fn write_verified(path: &str, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("写入失败: {}", e))?;
    match std::fs::metadata(path) {
        Ok(m) if m.len() > 0 => Ok(()),
        Ok(_) => Err("写入返回成功但文件为空（极可能被安全/网维软件拦截清空）".to_string()),
        Err(e) => Err(format!(
            "写入返回成功但文件立即不可见（极可能被安全/网维软件立即删除）: {}",
            e
        )),
    }
}

/// 文件系统探针：定位"exe 写出的文件被神秘删除"的根因（常见于网吧网维/安全软件）。
///
/// 两组对照实验：
/// - A. 新建文件（完全模拟 `winsuitex --json` 的写入行为）
/// - B. 覆盖一个已存在的空文件（模拟 .bat 预建占位、exe 原地改写）
///
/// 从"新建即删 vs 覆盖存活"可判断删除是「针对新文件创建」还是「针对本进程写盘」；
/// 另记录文件存活时长，区分"瞬间删（写入即删）"与"延迟删（过滤驱动周期扫描）"。
fn fs_probe(path: &str) -> ExitCode {
    use std::time::{Duration, Instant};
    let intervals = [0u64, 100, 250, 500, 1000, 2000, 5000];

    let test = |label: &str, create_new: bool| -> String {
        let _ = std::fs::remove_file(path);
        if !create_new {
            // 预建空占位（由本进程写，模拟 bat 的 type nul 占位）
            let _ = std::fs::write(path, b"");
        }
        let _start = Instant::now();
        if write_verified(path, b"fs-probe-content").is_err() {
            return format!("[{}] write_verified 失败（写入即被拦截）", label);
        }
        let mut result = format!("[{}] 写入后 t=0 存在", label);
        let mut vanished: Option<u64> = None;
        for &ms in &intervals {
            std::thread::sleep(Duration::from_millis(ms));
            match std::fs::metadata(path) {
                Ok(_) => result.push_str(&format!(", t={}ms 仍在", _start.elapsed().as_millis())),
                Err(_) => {
                    vanished = Some(ms);
                    break;
                }
            }
        }
        let _ = std::fs::remove_file(path);
        match vanished {
            Some(v) => format!("{} => 约 {}ms 后被删除（延迟删/周期扫描）", result, v),
            None => format!("{} => 5s 后仍存活（未被删）", result),
        }
    };

    wsx_println!("{}", test("A 新建", true));
    wsx_println!("{}", test("B 覆盖", false));
    wsx_println!("fs-probe 完成。");
    ExitCode::SUCCESS
}

/// 把采集快照渲染成 HTML —— 在别的机器上采集、回到开发机复现报告用。
///
/// 存在理由：跨机器核对的工作流是「目标机 `--json` → 拷回 → 与 C++ 报告逐段 diff」。
/// 只带 JSON 回来时必须在**当前机器**把它还原成同样的 HTML，比对才有意义；
/// 重新采集一遍是拿不到目标机数据的。
fn run_render(input: &str, output: &str) -> ExitCode {
    let text = match std::fs::read_to_string(input) {
        Ok(t) => t,
        Err(e) => {
            wsx_eprintln!("读取快照失败: {}", e);
            return ExitCode::from(2);
        }
    };
    let mut snapshot: Snapshot = match serde_json::from_str(&text) {
        Ok(s) => s,
        Err(e) => {
            wsx_eprintln!("解析快照失败（不是本工具导出的 JSON？）: {}", e);
            return ExitCode::from(2);
        }
    };
    // 渲染端也做一次线程数校正：老快照可能带着固件脏数据（threads < cores），
    // 用操作系统逻辑处理器总数恢复正确值，保证报告与采集端口径一致。
    let lp = snapshot.info.os.as_ref().and_then(|o| o.logical_processors);
    winsuitex_core::smbios::sanitize_thread_counts(&mut snapshot.info.cpus, lp);
    // 渲染端也合并键鼠一体设备：老快照（本次修复前采集的 JSON）里 A4Tech 09DA:FA90
    // 被数成"2 鼠标 + 1 键盘"，重新渲染时一并纠正为 1 键盘/鼠标 + 1 鼠标，无需回真机重采。
    snapshot.info.input_devices =
        winsuitex_platform::merge_input_devices(snapshot.info.input_devices);
    // 渲染端轻量语义校验：拦截硬性损坏/畸形的存档快照，避免渲染出误导性报告。
    if let Err(code) = validate_render(&snapshot) {
        return code;
    }
    let html = winsuitex_report::build_html_with_features(
        &snapshot.info,
        &snapshot.volumes,
        &snapshot.feature_ids(),
    );
    match write_verified(output, html.as_bytes()) {
        Ok(()) => {
            wsx_println!("报告已写入: {}", output);
            ExitCode::SUCCESS
        }
        Err(e) => {
            wsx_eprintln!("写入报告失败: {}", e);
            ExitCode::from(2)
        }
    }
}

/// 渲染端轻量语义校验：拦截明显损坏/畸形的存档快照，避免渲染出误导性报告。
///
/// 返回 `Ok(())` 表示可继续渲染（软问题已打印警告）；`Err(ExitCode)` 表示命中硬性损坏，
/// 应中止渲染。设计上**宁可误报警告也不静默渲染垃圾** —— 与采集层"拿不到就是 None"一致。
fn validate_render(snapshot: &Snapshot) -> Result<(), ExitCode> {
    let mut hard = false;
    let info = &snapshot.info;

    // 内存总计：OS 口径为 0 明显是脏数据（真实机器不可能 0 内存）。
    if info.memory.total_mb == Some(MebiBytes::new(0)) {
        wsx_eprintln!("⚠ 校验告警：内存总计为 0，快照可能损坏");
        hard = true;
    }
    // 各盘容量：size_bytes == Some(0) 是畸形（盘一定有非零容量）。
    for (i, d) in info.disks.iter().enumerate() {
        if d.size_bytes == Some(0) {
            wsx_eprintln!("⚠ 校验告警：PhysicalDrive{} size_bytes=0，快照可能损坏", i);
            hard = true;
        }
    }
    // CPU 线程数必须 >= 核心数（sanitize 后应已纠正，这里兜底拦截残余）。
    for (i, c) in info.cpus.iter().enumerate() {
        if let (Some(cores), Some(threads)) = (c.cores, c.threads) {
            if cores > 0 && threads < cores {
                wsx_eprintln!(
                    "⚠ 校验告警：CPU #{} 线程数({}) < 核心数({})，请重新采集",
                    i + 1,
                    threads,
                    cores
                );
                hard = true;
            }
        }
    }
    // 软校验：OS 逻辑处理器数 (>0) 与各 CPU threads 之和不一致时仅提示，不阻断。
    if let Some(lp) = info.os.as_ref().and_then(|o| o.logical_processors) {
        let sum_threads: u32 = info.cpus.iter().filter_map(|c| c.threads).sum();
        if lp > 0 && sum_threads > 0 && lp != sum_threads {
            wsx_eprintln!(
                "⚠ 校验提示：OS 逻辑处理器={} 与 CPU 线程合计={} 不一致（不影响渲染）",
                lp,
                sum_threads
            );
        }
    }

    if hard {
        wsx_eprintln!("快照校验未通过：存在硬性损坏字段，停止渲染以免误导");
        Err(ExitCode::from(2))
    } else {
        Ok(())
    }
}

/// 逐个读注册表，判定右键功能的启用状态。
///
/// 读失败（权限/键不存在）按「未启用」处理：状态未知时宁可显示未启用，
/// 也不要假装已启用 —— 这与采集层"拿不到就是 None"是同一条原则。
fn feature_states() -> Vec<(FeatureId, bool)> {
    let reg = winsuitex_platform::WinRegistry;
    winsuitex_core::feature::FEATURES
        .iter()
        .map(|f| (f.id, winsuitex_core::feature::is_installed(&reg, f)))
        .collect()
}

/// 采集 → 生成 HTML → 写文件。
///
/// 采集永不因硬件失败而整体失败（与 C++ 版一致），但降级会打印到 stderr
/// ——这是 C++ 版缺失的可观测性。
fn run_report(path: &str) -> ExitCode {
    tprintln(&format!(
        "winsuitex {} built {}",
        version_string(),
        build_time()
    ));
    let sources = winsuitex_platform::default_sources();
    let mut result = collect_all(&sources);
    tprintln("run_report: collect_all done");
    // 采集完再补盘符 → PhysicalDriveN 的映射（需要 IOCTL，故放在最后）
    winsuitex_platform::link_volumes_to_disks(&mut result.volumes);
    tprintln("run_report: link_volumes_to_disks done");

    if !result.is_complete() {
        for d in &result.diagnostics {
            wsx_eprintln!("[warn] 采集降级 [{}]: {}", d.source, d.message);
        }
    }

    let features = feature_states();
    tprintln("run_report: feature_states done");
    let html = winsuitex_report::build_html_with_features(&result.info, &result.volumes, &features);
    tprintln(&format!("run_report: html rendered, {} bytes", html.len()));

    // STDOUT 模式：把报告直接写到标准输出，由调用方用 cmd 重定向落盘。
    // 同 run_json 的规避思路：winsuitex.exe 不调用 CreateFile，绕开"按进程写盘"拦截。
    // 用 write_all 落原始 UTF-8 字节，保证与 --report <文件> 逐字节一致。
    if path == "-" {
        use std::io::Write;
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        if let Err(e) = lock.write_all(html.as_bytes()) {
            wsx_eprintln!("写入 stdout 失败: {}", e);
            return ExitCode::from(2);
        }
        let _ = lock.flush();
        wsx_eprintln!("报告已生成（已输出到 stdout，由调用方重定向落盘）");
        wsx_eprintln!(
            "采集摘要: CPU {} 颗 / 内存条 {} 根 / 显卡 {} 块 / 显示器 {} 台 / \
             物理盘 {} 块 / 分区 {} 个 / 网卡 {} 个 / 键鼠 {} 个 / USB {} 个 / 音频 {} 个",
            result.info.cpus.len(),
            result.info.memory.sticks.len(),
            result.info.gpus.len(),
            result.info.displays.len(),
            result.info.disks.len(),
            result.volumes.len(),
            result.info.nics.len(),
            result.info.input_devices.len(),
            result.info.usb.len(),
            result.info.audio.len(),
        );
        if !result.is_complete() {
            wsx_eprintln!("注意: 有 {} 项采集降级", result.diagnostics.len());
        }
        return ExitCode::SUCCESS;
    }

    match write_verified(path, html.as_bytes()) {
        Ok(()) => {
            wsx_println!("报告已写入: {}", path);
            wsx_println!(
                "采集摘要: CPU {} 颗 / 内存条 {} 根 / 显卡 {} 块 / 显示器 {} 台 / \
                 物理盘 {} 块 / 分区 {} 个 / 网卡 {} 个 / 键鼠 {} 个 / USB {} 个 / 音频 {} 个",
                result.info.cpus.len(),
                result.info.memory.sticks.len(),
                result.info.gpus.len(),
                result.info.displays.len(),
                result.info.disks.len(),
                result.volumes.len(),
                result.info.nics.len(),
                result.info.input_devices.len(),
                result.info.usb.len(),
                result.info.audio.len(),
            );
            if !result.is_complete() {
                wsx_println!(
                    "注意: 有 {} 项采集降级（详见上方 [warn]）",
                    result.diagnostics.len()
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            wsx_eprintln!("写入报告失败: {}", e);
            ExitCode::from(2)
        }
    }
}

/// 打印 SMBIOS 结构清单 + 解析结果。
///
/// 存在理由：硬件采集"少了一块"时，最难的是判断**是固件没给，还是我们解析错了**。
/// 这个子命令把两者分开 —— 结构清单来自固件，解析结果来自我们的代码。
fn debug_smbios() -> ExitCode {
    let raw = match winsuitex_platform::smbios::read_raw() {
        Ok(r) => r,
        Err(e) => {
            wsx_eprintln!("读取 SMBIOS 失败: {}", e);
            return ExitCode::from(2);
        }
    };

    wsx_println!("原始表: {} 字节", raw.len());
    wsx_println!("\n结构清单 (固件提供):");
    for (i, (typ, len)) in winsuitex_core::smbios::list_structures(&raw)
        .into_iter()
        .enumerate()
    {
        wsx_println!("  [{:2}] type={:<3} len={:<3}", i, typ, len);
    }

    match winsuitex_core::smbios::parse(&raw) {
        Ok(d) => {
            wsx_println!(
                "\n解析结果: CPU {} 颗 / 内存条 {} 根",
                d.cpus.len(),
                d.mem_sticks.len()
            );
            for (i, c) in d.cpus.iter().enumerate() {
                wsx_println!(
                    "  CPU{}: {} cores={:?} threads={:?} {}MHz",
                    i,
                    c.model.as_deref().unwrap_or("<无型号>"),
                    c.cores,
                    c.threads,
                    c.speed_mhz.unwrap_or(0)
                );
            }
            for (i, m) in d.mem_sticks.iter().enumerate() {
                wsx_println!(
                    "  DIMM{}: {} MB {} {} @ {}MHz",
                    i,
                    m.capacity_mb
                        .map(|v| v.get().to_string())
                        .unwrap_or_else(|| "<空槽>".into()),
                    m.mem_type.as_deref().unwrap_or("<未知类型>"),
                    m.locator.as_deref().unwrap_or("<无定位>"),
                    m.speed_mhz.unwrap_or(0)
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            wsx_eprintln!("解析失败: {}", e);
            ExitCode::from(2)
        }
    }
}

/// 导出原始 SMBIOS 字节（`--dump-smbios <路径>`）。
///
/// 用途：把真实机器的表存成 `winsuitex-core/tests/fixtures/` 下的夹具，
/// 让"某台机器解析不出内存条"这类 bug 可以固化成永久回归测试。
fn dump_smbios(path: &str) -> ExitCode {
    match winsuitex_platform::smbios::read_raw()
        .map_err(|e| e.to_string())
        .and_then(|raw| std::fs::write(path, &raw).map_err(|e| e.to_string()))
    {
        Ok(()) => {
            wsx_println!("原始 SMBIOS 已写入: {}", path);
            ExitCode::SUCCESS
        }
        Err(e) => {
            wsx_eprintln!("导出失败: {}", e);
            ExitCode::from(2)
        }
    }
}

/// 自检：在真实机器上验证采集链路可用。
/// 返回失败项数作为退出码（0 = 全通过），与 C++ 版 `--selftest` 一致。
fn run_selftest() -> ExitCode {
    let mut pass = 0u32;
    let mut fail = 0u32;

    let mut check = |name: &str, ok: bool| {
        if ok {
            pass += 1;
            wsx_println!("[PASS] {}", name);
        } else {
            fail += 1;
            wsx_println!("[FAIL] {}", name);
        }
    };

    let sources = winsuitex_platform::default_sources();
    let mut result = collect_all(&sources);
    // 与 `--report` 走同一条链路：也要补盘符 → PhysicalDriveN 映射，
    // 否则自检验证的不是真实采集结果（曾漏掉这行导致误报 FAIL）。
    winsuitex_platform::link_volumes_to_disks(&mut result.volumes);
    let hw = &result.info;

    check(
        "OS 产品名可获取",
        hw.os.as_ref().and_then(|o| o.product.clone()).is_some(),
    );
    check(
        "架构可获取",
        hw.os.as_ref().and_then(|o| o.arch.clone()).is_some(),
    );
    check(
        "内存总量 > 0",
        hw.memory.total_mb.is_some_and(|v| v.get() > 0),
    );
    check("至少识别到 1 颗 CPU", !hw.cpus.is_empty());
    check("至少枚举到 1 个分区", !result.volumes.is_empty());
    check("主板信息可获取（SMBIOS）", hw.board.is_some());
    check("至少识别到 1 块物理盘", !hw.disks.is_empty());
    check("至少识别到 1 个网卡", !hw.nics.is_empty());
    // CPUID 微架构代号（QEMU 虚拟机报告 Broadwell-E，真实机器应为对应微架构）
    check(
        "CPU 微架构代号可识别",
        hw.cpus.iter().any(|c| c.codename.is_some()),
    );
    // 分区 → PhysicalDrive 映射：至少有一个盘符号拿到了盘号
    check(
        "分区可映射到物理盘",
        result.volumes.iter().any(|v| v.disk_index.is_some()),
    );

    // 报告渲染不应为空白
    let html = winsuitex_report::build_html(hw, &result.volumes);
    check("HTML 报告非空且含 <html>", html.contains("<html"));
    check(
        "HTML 含分区小节",
        result.volumes.is_empty() || html.contains("分区"),
    );

    wsx_println!("\n自检汇总：通过 {}，失败 {}", pass, fail);

    // 与 C++ 版一致：失败项数即退出码
    if fail > 255 {
        ExitCode::from(255)
    } else {
        ExitCode::from(fail as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_string_follows_four_segment_scheme() {
        // 四段式 `主.次.修订.编译日期戳(YYYYMMDD)`；取不到时区偏移时退回三段式。
        // 两种都合法，但**不可以是其它形态**——否则真机就分不清新旧的 exe 了。
        let v = version_string();
        let segs: Vec<&str> = v.split('.').collect();
        assert!(
            v.starts_with(env!("CARGO_PKG_VERSION")),
            "版本号应以语义号开头: {v}"
        );
        assert!(segs.len() == 3 || segs.len() == 4, "版本号段数异常: {v}");
        if segs.len() == 4 {
            assert_eq!(segs[3].len(), 8, "日期戳须为 YYYYMMDD: {v}");
            assert!(
                segs[3].chars().all(|c| c.is_ascii_digit()),
                "日期戳须全数字: {v}"
            );
        }
    }

    #[test]
    fn build_time_is_never_empty() {
        assert!(!build_time().is_empty(), "构建时间不应为空");
    }
}
