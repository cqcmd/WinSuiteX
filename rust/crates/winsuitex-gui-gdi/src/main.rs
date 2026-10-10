//! `winsuitex-gui-gdi` —— WinSuiteX 硬件概览面板（纯 Rust + Win32 GDI 自绘）。
//!
//! # 为什么回到 GDI（而不是 egui / core-ui）
//!
//! 这是一次**方向纠偏**。之前两条 GUI 路线都因为"��渲染依赖"或"跨语言桥"
//! 反复踩坑：
//!   - `egui`：5.66 MB + wgpu/D3D12，虚拟机/RDP/基础显示驱动起不来；
//!   - `core-ui`：4.46 MB dll + D3D11，且 JS→宿主桥在 1.5.0 移除，
//!     控件回调要靠 `ui_widget_on_click` 硬接，`v-if` 的 mount/unmount 还会让
//!     `find_by_id` 找不到控件（真机连续踩坑三轮）。
//!
//! GDI 路线同时躲开这三件事：
//!   1. **单 exe、无 dll**（与 C++ 版同为"拷贝即用"，网吧分发友好）；
//!   2. **零 GPU 依赖**——GDI 是 Windows 自带 2D 渲染器，虚拟机/RDP 也能画；
//!   3. **没有跨语言边界**——全在自己手里，交互不会再出现"点了没反应"。
//!
//! 体积预期与 C++ 版同量级（500~700 KB，workspace 已开 `opt-level="z"`+LTO+strip）。
//!
//! # 模块
//!
//! - [`theme`]：视觉语言（Ant Design 配色 + 8px 栅格 + 96-DPI 基准常量）；
//! - [`draw`]：GDI 绘制原语（圆角、渐变、文字、瓦片、双缓冲）；
//! - [`app`]：窗口骨架、消息处理与页面渲染。

// 纯 GUI 子程序：去掉 cargo 默认的控制台窗口（与 winsuitex-gui / -coreui 一致）。
#![windows_subsystem = "windows"]

mod draw;
mod icons;
mod pages;
mod theme;

#[cfg(test)]
mod smoke_tests;

use draw::*;
use theme::*;

use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicI32, Ordering};
#[cfg(feature = "crash-log")]
use std::sync::{Mutex, OnceLock};

// ───────────────────────────────────────────────────────────────────────────
// 崩溃诊断日志（`crash-log` feature 专用，Z.W. 1024×768 真机无界面排查）
//
// release 默认 `panic = "abort"`，`panic_immediate_abort` 既不进 panic hook、
// 也不捕获位置——双击崩溃时 process 直接消失，看不到任何信息。
// 排查走 `--profile diag`（继承 release 体积优化、但 `panic = "unwind"`），
// 再叠 `crash-log` feature：本模块在 exe 同目录写 `winsuitex-gui-gdi.log`，
// ① 每个关键里程碑记一行（崩前最后一行告诉我们走到哪一步）；
// ② panic hook 把 file:line + payload + 调用栈强制落盘。
// 普通 `--release` 不加 feature，下面所有代码编译期消除（零开销、不写文件）。
// ───────────────────────────────────────────────────────────────────────────

/// 日志文件句柄（append 模式；打不开则 `None`，写入静默跳过，绝不反客为主崩主流程）。
#[cfg(feature = "crash-log")]
static CRASH_LOG: OnceLock<Mutex<Option<std::fs::File>>> = OnceLock::new();

/// 日志文件名（`exe 同目录/winsuitex-gui-gdi.log`；目录不可写回退 `%TEMP%`）。
#[cfg(feature = "crash-log")]
fn crash_log_path() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("winsuitex-gui-gdi.log");
        }
    }
    std::env::temp_dir().join("winsuitex-gui-gdi.log")
}

/// 打开日志文件（exe 同目录优先，失败再试临时目录）。
#[cfg(feature = "crash-log")]
fn open_crash_log() -> Option<std::fs::File> {
    if let Ok(f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(crash_log_path())
    {
        return Some(f);
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::env::temp_dir().join("winsuitex-gui-gdi.log"))
        .ok()
}

/// 取得（惰性初始化）日志句柄。
#[cfg(feature = "crash-log")]
fn crash_log_file() -> &'static Mutex<Option<std::fs::File>> {
    CRASH_LOG.get_or_init(|| Mutex::new(open_crash_log()))
}

/// 写一行诊断日志（带 Unix 时间戳）。任何失败都静默吞掉。
#[cfg(feature = "crash-log")]
fn log_line(s: &str) {
    use std::io::Write;
    let m = crash_log_file();
    if let Ok(mut g) = m.lock() {
        if let Some(f) = g.as_mut() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(f, "[{now}] {s}");
            let _ = f.flush();
        }
    }
}

/// 安装崩溃日志：开日志文件 + 装 panic hook（强制抓 file:line + 调用栈）。
#[cfg(feature = "crash-log")]
fn install_crash_logger() {
    // 先开文件，保证即便 hook 没触发，里程碑也已落盘。
    let _ = crash_log_file();
    std::panic::set_hook(Box::new(|info| {
        let loc = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".to_string());
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "<non-string panic payload>".to_string()
        };
        log_line(&format!("PANIC at {loc}: {payload}"));
        // `force_capture` 不受 RUST_BACKTRACE 影响，diag 双击崩溃也必有栈。
        let bt = std::backtrace::Backtrace::force_capture();
        log_line(&format!("BACKTRACE:\n{bt}"));
    }));
    log_line("crash logger installed");
}

/// 无 feature 时编译期彻底消失，连调用点都不产生代码。
#[cfg(not(feature = "crash-log"))]
macro_rules! wlog {
    ($($arg:tt)*) => {{}};
}

/// 有 feature 时写一行格式化日志。
#[cfg(feature = "crash-log")]
macro_rules! wlog {
    ($($arg:tt)*) => {{
        $crate::log_line(&format!($($arg)*));
    }};
}

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::{
    GetDpiForSystem, GetDpiForWindow, SetProcessDpiAwareness, SetProcessDpiAwarenessContext,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, PROCESS_PER_MONITOR_DPI_AWARE,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::*;

// 窗口类名 / 标题
const WND_CLASS: &[u16] = &[
    0x57, 0x69, 0x6E, 0x53, 0x75, 0x69, 0x74, 0x65, 0x58, 0x47, 0x44, 0x49, 0x00,
]; // "WinSuiteXGDI"
const WND_TITLE: &[u16] = &[0x57, 0x69, 0x6E, 0x53, 0x75, 0x69, 0x74, 0x65, 0x58, 0x00]; // "WinSuiteX"

/// 版权行（底部栏展示；与 C++ 版一致的公司署名口径）。
pub const FOOTER_COPYRIGHT: &str = "\u{a9} \u{91cd}\u{5e86}\u{6301}\u{739b}\u{591a}\u{7f51}\u{7edc}\u{6280}\u{672f}\u{6709}\u{9650}\u{516c}\u{53f8} \u{b7} Z.W.";

/// 采集到的硬件数据（启动时填充一次，绘制时只读）。
///
/// 用 `OnceLock` 而非 `static mut`：绘制侧每帧都要读，`static mut` 每次读都是
/// `unsafe`，而 clippy 的 `static_mut_refs` 也会一直报警。`OnceLock` 让"写一次、
/// 之后安全读"这件事由类型系统保证。
static SNAPSHOT: std::sync::OnceLock<winsuitex_core::CollectResult> = std::sync::OnceLock::new();

/// 命中的页签（WM_LBUTTONUP 切页）。
static CUR_TAB: AtomicI32 = AtomicI32::new(0);
/// 导出报告按钮命中态。
static HOVER_EXPORT: AtomicI32 = AtomicI32::new(0);
/// 截图按钮命中态（第二十二轮新增）。
static HOVER_SHOT: AtomicI32 = AtomicI32::new(0);
/// 窗口按钮命中态（0=最小化 1=最大化 2=关闭，-1=无）。
static HOVER_TB: AtomicI32 = AtomicI32::new(-1);
/// 右键增强页当前悬停的开关索引（-1=无）。
static HOVER_SWITCH: AtomicI32 = AtomicI32::new(-1);
/// 磁盘页当前悬停的分区行索引（-1=无，用于 hover 浅底反馈）。
static HOVER_VOL_ROW: AtomicI32 = AtomicI32::new(-1);
/// 窗口是否最大化（0/1）。由 `WM_SIZE` 同步，绘制层只读。
static ZOOMED: AtomicI32 = AtomicI32::new(0);

/// 读悬停的开关索引。
#[inline]
pub fn hover_switch() -> i32 {
    HOVER_SWITCH.load(Ordering::Relaxed)
}

/// 设置悬停的开关索引。
#[inline]
pub fn set_hover_switch(v: i32) {
    HOVER_SWITCH.store(v, Ordering::Relaxed);
}

/// 当前悬停的磁盘分区行索引（-1=无）。
#[inline]
pub fn hover_vol_row() -> i32 {
    HOVER_VOL_ROW.load(Ordering::Relaxed)
}

/// 设置悬停的磁盘分区行索引。
#[inline]
pub fn set_hover_vol_row(v: i32) {
    HOVER_VOL_ROW.store(v, Ordering::Relaxed);
}
/// 内容区垂直滚动偏移（像素，>=0）。内容超出窗口高度时滚轮/拖动改这个。
static SCROLL_Y: AtomicI32 = AtomicI32::new(0);
/// 当前页内容总高度（绘制时算出），用于钳制滚动范围与画滚动条。
static CONTENT_H: AtomicI32 = AtomicI32::new(0);
/// 最近一次绘制时的客户区高度：滚轮消息到来时没有客户区尺寸，靠它换算可视高度。
static LAST_CLIENT_H: AtomicI32 = AtomicI32::new(0);

/// 当前垂直滚动偏移。
#[inline]
pub fn scroll_y() -> i32 {
    SCROLL_Y.load(Ordering::Relaxed)
}

/// 设置滚动偏移（负值归零）。
pub fn set_scroll_y(v: i32) {
    SCROLL_Y.store(v.max(0), Ordering::Relaxed);
}

/// 内容区可视区（页签条下沿到底部栏上沿）。
///
/// 上下边界都走 `pages::content_*` / `body_pad()`，和绘制层同一份真源——
/// 之前这里写死 `scale(TITLE_H + TAB_H)`，frameless 加了阴影留白后就和绘制错开，
/// 裁剪区比可视区矮一截，底部内容被切掉。
fn view_geometry() -> (i32, i32) {
    let top = pages::content_top();
    (
        top,
        LAST_CLIENT_H
            .load(Ordering::Relaxed)
            .saturating_sub(scale(FOOTER_H) + pages::body_pad() + top)
            .max(1),
    )
}

/// 当前可滚动的最大偏移（内容超出可视区的部分）。
fn max_scroll() -> i32 {
    let (top, view_h) = view_geometry();
    (content_h() - top - view_h).max(0)
}

/// 按滚动增量移动并钳制到合法范围，返回实际偏移。
///
/// 注意这里**不用 `fetch_add` 直接累加**：滚动必须先钳制再写回。
/// 先`fetch_add` 后钳制会出现死锁式bug——一旦被钳到 0，下一格又从 0 减，
/// 永远停在顶部（实测踩过）。
pub fn scroll_by(delta: i32) -> i32 {
    let next = (scroll_y() + delta).clamp(0, max_scroll());
    set_scroll_y(next);
    next
}

/// 滚轮格数 → 滚动增量。
///
/// 坐标约定（第十五轮修正）：滚轮**上滚** delta 为正，用户期望"看上面的内容"
/// = `scroll_y` 减小（`on_paint` 用 `OffsetViewportOrgEx(0, -scroll_y)`，
/// `scroll_y` 越大内容越往上移），所以必须**取负**。此前漏了负号，
/// 滚动方向与滚轮整体相反。
#[inline]
fn wheel_scroll_delta(notches: i32) -> i32 {
    -notches * scale(ROW_H)
}

/// 切换页签时把滚动归零（换页还留在半截高度会很怪）。
pub fn reset_scroll() {
    SCROLL_Y.store(0, Ordering::Relaxed);
}

/// 内容总高度。
#[inline]
pub fn content_h() -> i32 {
    CONTENT_H.load(Ordering::Relaxed)
}

/// 设置内容总高度。
pub fn set_content_h(v: i32) {
    CONTENT_H.store(v, Ordering::Relaxed);
}

/// 当前页签索引。
#[inline]
pub fn cur_tab() -> i32 {
    CUR_TAB.load(Ordering::Relaxed)
}

/// 设置当前页签。
#[inline]
pub fn set_cur_tab(v: i32) {
    CUR_TAB.store(v, Ordering::Relaxed);
}

/// 导出按钮是否悬停。
#[inline]
pub fn hover_export() -> bool {
    HOVER_EXPORT.load(Ordering::Relaxed) != 0
}

/// 设置导出按钮悬停态。
#[inline]
pub fn set_hover_export(v: bool) {
    HOVER_EXPORT.store(i32::from(v), Ordering::Relaxed);
}

/// 截图按钮是否悬停（第二十二轮新增）。
#[inline]
pub fn hover_shot() -> bool {
    HOVER_SHOT.load(Ordering::Relaxed) != 0
}

/// 设置截图按钮悬停态。
#[inline]
pub fn set_hover_shot(v: bool) {
    HOVER_SHOT.store(i32::from(v), Ordering::Relaxed);
}

/// 窗口按钮命中索引（-1 = 无）。
#[inline]
pub fn hover_tb() -> i32 {
    HOVER_TB.load(Ordering::Relaxed)
}

/// 设置窗口按钮命中索引。
#[inline]
pub fn set_hover_tb(v: i32) {
    HOVER_TB.store(v, Ordering::Relaxed);
}

/// 读取采集快照（未就绪返回 `None`）。
pub fn snapshot() -> Option<&'static winsuitex_core::CollectResult> {
    SNAPSHOT.get()
}

/// 单实例互斥体名（会话命名空间）。
///
/// 不用 `Global\` 前缀：那需要 `SeCreateGlobalPrivilege`，网吧/电竞酒店的标准用户
/// 下创建会失败，判重就整个失效了；同一用户会话内用默认命名空间足够。
const SINGLE_MUTEX: PCWSTR = w!("WinSuiteX.GDI.SingleInstance");

// 句柄不需要保存：`HANDLE` 是 `Copy` 且无 `Drop`，我们也从不调 `CloseHandle`，
// 所以句柄在本进程内一直开着 —— 互斥体因此存活到进程退出，这正是"已存在"信号的载体。
// （别画蛇添足去 `mem::forget` 或塞 static：前者对 Copy 类型无效会被 clippy 拦，
// 后者过不了 `Sync`——裸指针句柄不是 `Send`/`Sync`。）

/// 只允许一个实例：已有实例在跑就**把它激活到前台**，本进程静默退出（不弹窗、不提示）。
///
/// 为什么要 Mutex + FindWindow **双保险**：
/// - 只用 `FindWindowW` 判重会漏掉「两个进程同时启动、都还没建窗口」的竞态——
///   两边都通过检查，于是起两个窗口；
/// - 只用 Mutex 则拿不到已有窗口的句柄，没法"切"过去，只能干巴巴地退出。
///
/// 所以 Mutex 负责原子判重，FindWindow 负责激活。
///
/// 返回 `true` = 已有实例已被激活，调用方应立即退出（不再建窗口、不再采集）。
unsafe fn single_instance_or_activate() -> bool {
    // SAFETY: 名字是静态常量；句柄故意 `forget`——进程退出时系统自动关闭，
    // 存活期内一直"开着"，别的进程才能判出「已存在」。
    // ⚠️ `bInitialOwner = false` 是必须：传 true 时第二个进程会在 `CreateMutexW`
    // 里阻塞等待首个进程释放（等于把第二次双击卡住），我们只想要"存在性"判定。
    if CreateMutexW(None, false, SINGLE_MUTEX).is_err() {
        // 建不出来（极端环境）→ 不拦启动：宁可双开，也不能让用户打不开工具。
        return false;
    }
    if GetLastError() != ERROR_ALREADY_EXISTS {
        return false;
    }
    // 已有实例：等它把窗口建出来（刚双击时可能还在采集 WMI），最多 ~1.5s。
    for _ in 0..30 {
        // windows 0.62 的 `FindWindowW` 返回 `Result<HWND>`：找不到窗口走 Err，
        // 不是返回空 HWND（老写法 `hwnd.0.is_null()` 在这里编译不过）。
        if let Ok(hwnd) = FindWindowW(PCWSTR(WND_CLASS.as_ptr()), None) {
            bring_to_front(hwnd);
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    // 等不到窗口：前一个实例正在退出，本进程接着正常启动。
    false
}

/// 把已有实例的窗口切到前台（最小化时先还原）。
unsafe fn bring_to_front(hwnd: HWND) {
    // SAFETY: hwnd 来自 FindWindowW；ShowWindow/SetForegroundWindow 对窗口无副作用约束。
    // 最小化态必须先 SW_RESTORE，否则 SetForegroundWindow 只点亮任务栏按钮、窗口不出现。
    let _ = if IsIconic(hwnd).as_bool() {
        ShowWindow(hwnd, SW_RESTORE)
    } else {
        ShowWindow(hwnd, SW_SHOW)
    };
    // 进程刚启动时系统允许设置一次前台窗口——这是"双击 exe 抢前台"的唯一窗口期；
    // 被拒（比如前台正被别的窗口锁住）时补一次 BringWindowToTop 再试。
    if !SetForegroundWindow(hwnd).as_bool() {
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
    }
}

fn main() {
    // SAFETY: 全部是对 Win32 API 的正确调用；字符串均为 NUL 结尾的静态常量，
    // 句柄在同一函数内有效（窗口创建后由消息循环持有）。
    unsafe {
        // 崩溃诊断日志最先装：哪怕后面单实例判重 / DPI / 采集任何一步崩，
        // 日志文件也已打开、hook 已就位，双击崩溃也能拿到 file:line + 栈。
        // 无 `crash-log` feature 时函数/宏都不编译（这里是空操作）。
        #[cfg(feature = "crash-log")]
        install_crash_logger();
        wlog!("main entered; cwd={:?}", std::env::current_dir().ok());

        // 单实例：第二次运行时把已跑的实例切到前台，本进程直接退出。
        // 放在最前面——抢在 WMI 采集之前返回，用户感知是"立刻切过去"，不是"卡两秒"。
        if single_instance_or_activate() {
            wlog!("single-instance: another instance activated, exiting");
            return;
        }
        wlog!("single-instance: first instance, continuing");
        // DPI 感知：声明 per-monitor v2，否则 Windows 把本进程当成 96-DPI，
        // 用 DPI 虚拟化把整窗位图拉伸到物理像素 —— 这是"窗口边缘/文字/开关发虚"
        // 的根因（非 DPI 感知进程画在 96 逻辑像素，系统再位图放大到真实分辨率）。
        // 声明后 GDI 按原生分辨率绘制，scale() 再把 96-DPI 基准布局换算到物理像素。
        // v2 在 WM_DPICHANGED 时自动拿到最新 DPI，跨屏拖动也清晰；v2 不可用时回退
        // 到 per-monitor v1（网吧/电竞酒店多为 Win10+，v2 普遍可用）。
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
            .or_else(|_| SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE));
        wlog!("dpi awareness set; system dpi={}", GetDpiForSystem());

        // 初始 DPI 用主屏 DPI：让 CreateWindowExW 一开始就按真实物理尺寸建窗口，
        // 而不是等 WM_DPICHANGED（后者在创建后才可能触发；窗口宽度不会被 OnPaint
        // 校正，缺少这一步会导致高分屏下窗口过窄、右侧内容被裁）。
        set_dpi(GetDpiForSystem().max(96) as u32);
        // scale(96) 反推当前 DPI（96*原DPI/96 取整），用于日志确认缩放基准。
        wlog!("set_dpi -> dpi≈{}", theme::scale(96));

        let Ok(hinst) = GetModuleHandleW(None) else {
            return;
        };
        // WNDCLASSEXW 用结构体字面量一次成型：逐字段赋值 `Default::default()`
        // 之后改会被 clippy 的 field_reassign_with_default 拦下，且容易漏字段。
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinst.into(),
            lpszClassName: PCWSTR(WND_CLASS.as_ptr()),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return;
        }

        // 采集：复用平台引擎（与 CLI / egui GUI 同一条链路）。
        wlog!("collect start");
        let _ = SNAPSHOT.set(collect());
        wlog!("collect done");

        // 初始 DPI：虚拟机/高分屏下让布局一开始就是对的，而不是等 WM_DPICHANGED。
        // 窗口尺寸先按工作区钳制：小屏（如 1024×768 工作区 ≈ 738 高）下窗口高不会
        // 超过工作区，标题栏/底栏留在屏内、内容靠滚动；大屏仍取设计尺寸（零回归）。
        let (init_w, init_h) = (max_win_w(), max_win_h());
        wlog!(
            "CreateWindowExW: init_w={init_w} init_h={init_h} win_w={} win_h={}",
            scale(WIN_W),
            scale(WIN_H)
        );
        if let Ok(hwnd) = CreateWindowExW(
            WS_EX_APPWINDOW,
            PCWSTR(WND_CLASS.as_ptr()),
            PCWSTR(WND_TITLE.as_ptr()),
            // ⚠️ frameless（WS_POPUP），不是 `WS_OVERLAPPEDWINDOW`：后者带系统标题栏，
            // 会和自绘的紫色顶栏叠成"双标题栏"（第一版就栽在这，截图里一眼可见）。
            // 代价是拖动窗口 / 边缘缩放 / Aero 摇 Maximize 全要自己实现，
            // 见 `WM_NCHITTEST` + `WM_NCLBUTTONDOWN` 两个分支。
            WS_POPUP,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            init_w + scale(SHADOW_PAD) * 2,
            init_h + scale(SHADOW_PAD) * 2,
            None,
            None,
            Some(hinst.into()),
            None,
        ) {
            set_dpi(GetDpiForWindow(hwnd).max(96) as u32);
            apply_rounded_region(hwnd);
            center_window(hwnd);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = UpdateWindow(hwnd);
            wlog!(
                "window created + shown; hwnd={hwnd:?} dpi={}",
                theme::scale(96)
            );

            // 消息循环
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

/// 按当前窗口尺寸重建圆角 Region（外缘与自绘边框同心）。
///
/// frameless 窗口没有系统边框，圆角只能自己做：`CreateRoundRectRgn` +
/// `SetWindowRgn`。SetWindowRgn 成功后 HRGN 归系统所有，重复调用自动释放旧的，
/// 不泄漏（每次 resize 都要调，所以这条性质很关键）。
fn apply_rounded_region(hwnd: HWND) {
    // SAFETY: 全部是对 Win32 API 的正确调用；HRGN 创建后立即交给 SetWindowRgn。
    unsafe {
        let mut rc = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rc);
        let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
        if w <= 0 || h <= 0 {
            return;
        }
        wlog!("apply_rounded_region: w={w} h={h}");
        let r = scale(WIN_RADIUS + SHADOW_PAD);
        // windows 0.62：`CreateRoundRectRgn` 直接返回 HRGN（不是 Result）。
        let hrgn = CreateRoundRectRgn(0, 0, w + 1, h + 1, r * 2, r * 2);
        if hrgn.is_invalid() {
            return;
        }
        let _ = SetWindowRgn(hwnd, Some(hrgn), true);
    }
}

/// 工作区（任务栏除外）。
///
/// per-monitor DPI 感知进程里 `SPI_GETWORKAREA` 返回的是**物理像素**，
/// 与 `GetWindowRect` / `scale()` 同一坐标空间（都按真实分辨率算），
/// 所以可以直接拿来比较、钳制窗口尺寸。
/// 拿不到就返回 `None`——调用方各自回退到设计尺寸，只是不做钳制、位置略偏。
fn work_area() -> Option<RECT> {
    // 诊断：用 `WSX_FAKE_WORKAREA=WxH` 强制模拟小屏（复现 1024×768 真机
    // `work_area < WIN_H` 几何崩溃用）。仅 `crash-log` feature 编译，普通
    // release 不受影响；不设该环境变量时走真实工作区（真机路径不变）。
    #[cfg(feature = "crash-log")]
    {
        if let Ok(s) = std::env::var("WSX_FAKE_WORKAREA") {
            if let Some((w, h)) = s.split_once('x') {
                if let (Ok(w), Ok(h)) = (w.trim().parse::<i32>(), h.trim().parse::<i32>()) {
                    if w > 0 && h > 0 {
                        return Some(RECT {
                            left: 0,
                            top: 0,
                            right: w,
                            bottom: h,
                        });
                    }
                }
            }
        }
    }
    // SAFETY: `pvparam` 传 `&mut work` 指针，回调不涉及 COM，是该 API 的标准用法。
    unsafe {
        let mut work = RECT::default();
        let ptr: *mut RECT = &mut work;
        // `fWinIni` 读取工作区时不生效，传 0（typed enum 不能直接给整数字面量）。
        let flags = SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0);
        if SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(ptr.cast()), flags).is_ok() {
            Some(work)
        } else {
            None
        }
    }
}

/// 窗口宽/高上限 = `min(设计尺寸, 工作区尺寸)`。
///
/// 设计尺寸 `WIN_W`/`WIN_H` 是 96-DPI 基准、经 `scale()` 换算到物理像素。
/// 当工作区比设计尺寸小（如 1024×768 屏 ≈ 738 高 < 948）时按工作区钳制，
/// 让标题栏/底栏都留在屏内、内容靠滚动条；余量充足（1080p/2K/4K）时仍取设计尺寸，
/// 行为与之前完全一致（零回归）。
fn max_win_w() -> i32 {
    match work_area() {
        Some(wk) => scale(WIN_W).min(wk.right - wk.left),
        None => scale(WIN_W),
    }
}
fn max_win_h() -> i32 {
    match work_area() {
        Some(wk) => scale(WIN_H).min(wk.bottom - wk.top),
        None => scale(WIN_H),
    }
}

/// 首次居中：放到工作区正中（`SWP_NOSIZE`，不触发 resize）。
fn center_window(hwnd: HWND) {
    // SAFETY: `pvparam` 传 `&mut work` 指针，回调不涉及 COM，是该 API 的标准用法。
    unsafe {
        let mut work = RECT::default();
        // windows 0.62：`SystemParametersInfoW` 返回 `Result<()>`（不是 BOOL），
        // 第三参要 `Option<*mut c_void>`，第四参是 typed flags 枚举。
        // 拿不到工作区就不居中 —— 只是位置略偏，不影响功能。
        let ptr: *mut RECT = &mut work;
        // `fWinIni` 读取工作区时不生效，传0（typed enum 不能直接给整数字面量）。
        let flags = SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0);
        if SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(ptr.cast()), flags).is_err() {
            return;
        }
        // 诊断：用 `WSX_FAKE_WORKAREA` 覆盖 SPI 拿到的工作区，复现窄工作区
        // （Z.W. 1024×768 竖任务栏，工作区宽≈窗口宽→`clamp(0,-2)`）。
        #[cfg(feature = "crash-log")]
        if let Some(fake) = work_area() {
            work = fake;
        }
        let mut rc = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rc);
        let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
        wlog!("center_window: w={w} h={h}");
        // 工作区正中。但 `w`/`h` 已被 `max_win_*` 钳到工作区，正常情况 `y >= work.top`；
        // 仍做 clamp 兜底，保证窗口任何情况下都留在工作区内（标题栏不出屏顶、底栏不出屏底），
        // 否则小屏上居中会把顶部栏推出屏外、既看不见也拖不动（Z.W. 报的 1024×768 现象）。
        // `saturating_sub` 防极端小屏（w/h 反过来大于工作区）时 `clamp(min>max)` 在 debug panic。
        // 上界用「右/下边缘坐标」`work.right - w` 再 `.max(work.left)` 兜底：
        // 窗口比工作区宽时 `work.right - w < work.left`，`.max` 把上界拉回 `work.left`，
        // 退化为「贴左/贴上」；绝不会出现 `clamp(min > max)`。
        // ⚠️ 不能用 `saturating_sub`：它只防止下溢到 i32::MIN，不会把 `-2` 变成 `0`，
        // 旧版因此仍会 panic（"min > max. min=0 max=-2"，Z.W. 1024×768 竖任务栏真机）。
        let x = (work.left + ((work.right - work.left) - w) / 2)
            .clamp(work.left, (work.right - w).max(work.left));
        let y = (work.top + ((work.bottom - work.top) - h) / 2)
            .clamp(work.top, (work.bottom - h).max(work.top));
        let _ = SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER);
    }
}

/// 命中测试：frameless 窗口自己实现拖动区与边缘缩放。
///
/// 返回 `HT*` 常量（已 cast 成 `i32`，调用方再转 `isize` 给 `LRESULT`）。
///
/// 判定顺序有讲究：
/// 1. **最大化时不返回缩放边**——否则用户能把窗口拖出屏幕外找不回来；
/// 2. **窗口按钮区必须返回 `HTCLIENT`**：否则点"关闭"会被系统当成拖标题栏，
///    变成"按住关闭键能拖窗口"；
/// 3. 标题栏空白 → `HTCAPTION`（系统拖动）；
/// 4. 四边 + 四角 → 缩放码。缩放带取 6px，比系统默认略宽一点，触屏/鼠标都好抓。
///
/// 坐标是**屏幕坐标**（`WM_NCHITTEST` 的约定），要先减去窗口原点变客户区坐标。
fn nc_hit_test(hwnd: HWND, sx: i32, sy: i32) -> i32 {
    // SAFETY: 只读窗口几何 + IsZoomed 状态查询，均不修改任何状态。
    unsafe {
        let mut rc = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rc);
        let x = sx - rc.left;
        let y = sy - rc.top;
        let w = rc.right - rc.left;
        let h = rc.bottom - rc.top;
        wlog!("nc_hit_test: x={x} y={y} w={w} h={h}");
        if w <= 0 || h <= 0 || x < 0 || y < 0 || x >= w || y >= h {
            return HTCLIENT as i32;
        }

        if IsZoomed(hwnd).as_bool() {
            return HTCLIENT as i32;
        }

        let bw = scale(46);
        let right = pages::content_right(w);
        // 窗口按钮两键：命中就交回客户端区，让 WM_LBUTTONUP 去处理最小化/关闭。
        if y < pages::body_pad() + scale(TITLE_H) && x >= right - bw * 2 {
            return HTCLIENT as i32;
        }
        // 标题栏（且不在按钮区）→ 拖动区。
        if y < pages::body_pad() + scale(TITLE_H) {
            return HTCAPTION as i32;
        }

        // 边缘缩放：6px 带，从窗口外缘往里算。
        let e = scale(6);
        let left = x < e;
        let right = x >= w - e;
        let top = y < e;
        let bottom = y >= h - e;
        match (top, bottom, left, right) {
            (true, _, true, _) => HTTOPLEFT as i32,
            (true, _, _, true) => HTTOPRIGHT as i32,
            (_, true, true, _) => HTBOTTOMLEFT as i32,
            (_, true, _, true) => HTBOTTOMRIGHT as i32,
            (true, _, _, _) => HTTOP as i32,
            (_, true, _, _) => HTBOTTOM as i32,
            (_, _, true, _) => HTLEFT as i32,
            (_, _, _, true) => HTRIGHT as i32,
            _ => HTCLIENT as i32,
        }
    }
}

/// 采集硬件（阻塞，约数秒）。
fn collect() -> winsuitex_core::CollectResult {
    let sources = winsuitex_platform::default_sources();
    let mut r = winsuitex_core::traits::collect_all(&sources);
    winsuitex_platform::link_volumes_to_disks(&mut r.volumes);
    // 诊断：用 `WSX_FAKE_AUDIO=N` 注入 N 个假声卡，复现真机 audio=4 的硬件页崩溃路径。
    // 仅 `crash-log` feature 生效，普通 release 不受影响。
    #[cfg(feature = "crash-log")]
    {
        if let Ok(s) = std::env::var("WSX_FAKE_AUDIO") {
            if let Ok(n) = s.trim().parse::<usize>() {
                use winsuitex_core::model::{AudioDevice, AudioKind};
                for i in 0..n {
                    r.info.audio.push(AudioDevice {
                        name: format!("Fake Audio Device #{i}"),
                        role: 0,
                        kind: AudioKind::Unknown,
                        is_bluetooth: false,
                        vid: None,
                        pid: None,
                        manufacturer: Some("FakeVendor".into()),
                    });
                }
            }
        }
    }
    r.info.enrich_power_on_durations();
    r.info.collected_at = Some(winsuitex_core::model::local_timestamp_string());
    wlog!(
        "collect result: cpus={} boards={} gpus={} mem_sticks={} nics={} audio={} disks={} volumes={} diagnostics={}",
        r.info.cpus.len(),
        r.info.board.as_ref().map(|_| 1).unwrap_or(0),
        r.info.gpus.len(),
        r.info.memory.sticks.len(),
        r.info.nics.len(),
        r.info.audio.len(),
        r.info.disks.len(),
        r.volumes.len(),
        r.diagnostics.len(),
    );
    r
}

/// 窗口是否处于最大化（绘制层用它切"最大化/还原"图标）。
///
/// 存成 `AtomicI32` 而不是每帧问 `IsZoomed`：绘制是每帧调用，
/// 而 `IsZoomed` 需要 HWND，而绘制层签名里没有窗口句柄。
/// 由 `WM_SIZE` 同步——那是系统通知尺寸/状态变化的唯一入口。
pub fn is_zoomed() -> bool {
    ZOOMED.load(Ordering::Relaxed) != 0
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // 诊断：每个消息都记一行，崩溃日志最后一行就是崩前处理的消息（定位用）。
    wlog!("wnd_proc msg={msg}");
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            on_paint(hwnd, hdc);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_SIZE => {
            // 同步最大化状态（绘制层靠它切"最大化/还原"图标）。
            ZOOMED.store(i32::from(IsZoomed(hwnd).as_bool()), Ordering::Relaxed);
            // 尺寸变了：圆角 Region 必须跟着重建，否则 resize 后角是方的。
            apply_rounded_region(hwnd);
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_NCHITTEST => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            LRESULT(nc_hit_test(hwnd, x, y) as isize)
        }
        WM_NCLBUTTONDOWN => {
            // 命中标题栏空白处 → 交给系统走拖动循环（DefWindowProc 发 WM_MOVING
            // 并最终 WM_EXITSIZEMOVE）。frameless 窗口没有系统标题栏，
            // 不转发的话窗口完全拖不动。
            if wparam.0 as i32 == HTCAPTION as i32 {
                let _ = DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            // DPI 变了：换 scale 基准 + 用系统建议的窗口尺寸，避免拉伸变形。
            // WM_DPICHANGED 的 wparam 高 16 位是新 DPI（低 16 位是旧 DPI）
            let dpi = ((wparam.0 >> 16) & 0xFFFF) as u32;
            set_dpi(dpi.max(96));
            wlog!(
                "WM_DPICHANGED: new dpi={} (scale(96)={})",
                dpi,
                theme::scale(96)
            );
            if lparam.0 != 0 {
                let prc = &*(lparam.0 as *const RECT);
                // 系统建议的窗口尺寸先按工作区钳制：把窗口从大屏拖到小屏（DPI 随之变小）
                // 时，系统给的建议尺寸可能还撑得比小屏工作区大，直接套用会出屏。
                // 正常屏上建议尺寸本就 ≤ 上限，钳制是 no-op（零回归）。
                let nw = (prc.right - prc.left).min(max_win_w());
                let nh = (prc.bottom - prc.top).min(max_win_h());
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    prc.left,
                    prc.top,
                    nw,
                    nh,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            apply_rounded_region(hwnd);
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            // 限制最小/最大尺寸：
            // - 最小：窗口被缩到内容放不下（表格只剩一列）；
            // - 最大：只限工作区大小，否则 frameless 窗口最大化时会盖住任务栏
            //   （没有系统边框去避让，PM_SHOWFULLSCREEN 让位会失效）。
            // 最小尺寸也钳到工作区：极小屏（如 1024×600 上网本）工作区比默认最小还小，
            // 不钳的话窗口会被系统强制拉到最小尺寸、反而超出工作区（底部出屏）。
            let mmi = &mut *(lparam.0 as *mut MINMAXINFO);
            mmi.ptMinTrackSize.x = scale(MARGIN * 6);
            mmi.ptMinTrackSize.y = scale(480);
            // SAFETY: 在 wnd_proc 的 unsafe 上下文里，读写调用方给的 MINMAXINFO。
            unsafe {
                let mut work = RECT::default();
                let ptr: *mut RECT = &mut work;
                let flags = SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0);
                if SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(ptr.cast()), flags).is_ok() {
                    let ww = work.right - work.left;
                    let wh = work.bottom - work.top;
                    mmi.ptMinTrackSize.x = mmi.ptMinTrackSize.x.min(ww);
                    mmi.ptMinTrackSize.y = mmi.ptMinTrackSize.y.min(wh);
                    mmi.ptMaxTrackSize.x = ww;
                    mmi.ptMaxTrackSize.y = wh;
                    wlog!("WM_GETMINMAXINFO: work_area={ww}x{wh}");
                }
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1), // 背景由 WM_PAINT 全量绘制
        WM_NCLBUTTONDBLCLK => {
            // 双击标题栏 → 最大化/还原切换。frameless 窗口没有系统双击行为，
            // 用户预期（Aero 习惯）要自己补上。
            if wparam.0 as i32 == HTCAPTION as i32 {
                // SAFETY: 只读窗口状态 + 发系统命令，无前置条件。
                unsafe {
                    let cmd = if IsZoomed(hwnd).as_bool() {
                        SC_RESTORE
                    } else {
                        SC_MAXIMIZE
                    };
                    let _ = DefWindowProcW(hwnd, WM_SYSCOMMAND, WPARAM(cmd as usize), lparam);
                }
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            // 增量在 wparam 的高 16 位（有符号，单位 1/120 格）。
            let notches = (wparam.0 >> 16) as i16 as i32;
            // 一格滚一整行，符合 Win11 触控板/鼠标的默认手感；
            // 方向约定（上滚看上面 = scroll_y 减小 = 取负）见 `wheel_scroll_delta`。
            scroll_by(wheel_scroll_delta(notches));
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            // 注册 WM_MOUSELEAVE：鼠标离窗后系统会补发一条离开消息。
            // ⚠️ TME_LEAVE 是**一次性**的——leave 触发后注册即失效，
            // 所以每次 mousemove 都要重新注册（Win32 惯例，不是浪费）。
            // 没有这一段，鼠标移出窗口后 hover 高亮会**永久残留**：
            // 移出后不会再有 WM_MOUSEMOVE 来把 hover 清成 -1
            // （真机截图里按钮区域那块浅灰底，一半是这个 + WIN_CACHED_W 错位叠加）。
            // SAFETY: 结构体是标准布局，hwndTrack 为本窗口句柄。
            unsafe {
                let tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&tme as *const TRACKMOUSEEVENT as *mut TRACKMOUSEEVENT);
            }
            on_mouse_move(hwnd, lparam);
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            // 鼠标离开窗口：清掉全部 hover 态，否则高亮残留到下次鼠标进来。
            set_hover_export(false);
            set_hover_shot(false);
            set_hover_tb(-1);
            set_hover_switch(-1);
            set_hover_vol_row(-1);
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            on_click(hwnd, lparam);
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// 整窗绘制：双缓冲 → 背景 → 顶栏 → 页签 → 内容页 → 底部栏。
unsafe fn on_paint(hwnd: HWND, hdc: HDC) {
    wlog!("on_paint enter (top)");
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;
    if w <= 0 || h <= 0 {
        wlog!("on_paint early-return: w={w} h={h}");
        return;
    }

    let Some(bb) = Backbuffer::new(w, h) else {
        wlog!("on_paint: Backbuffer::new failed (w={w} h={h})");
        return;
    };
    let mdc = bb.mem;
    wlog!(
        "on_paint enter: w={w} h={h} cur_tab={} scroll={} dpi={}",
        cur_tab(),
        scroll_y(),
        theme::scale(96)
    );
    // 底色先铺满（阴影区外沿也是这个色，不铺会露出未初始化的 DIB 内存）
    fill(
        mdc,
        &RECT {
            left: 0,
            top: 0,
            right: w,
            bottom: h,
        },
        C_BG,
    );

    // 字体（临时创建，用完即删）
    let f_title = font(FONT_UI, -scale(16), FW_BOLD.0 as i32);
    let f_body = font(FONT_UI, -scale(14), 400);
    let f_sub = font(FONT_UI, -scale(13), 400);
    let f_tiny = font(FONT_UI, -scale(12), 400);
    let f_section = font(FONT_UI, -scale(15), FW_BOLD.0 as i32);

    // frameless 窗口没有系统阴影/边框，先自绘外框，再叠内容。
    pages::draw_window_frame(mdc, w, h);

    // ⚠️ 把真实客户区宽度同步给命中测试。`hit_test` / `export_btn_rect` 用
    // `WIN_CACHED_W` 定位窗口按钮与导出按钮，而窗口按钮**绘制**位置用本函数的
    // `w`——两处必须同一份值。之前 `WIN_CACHED_W` 初始化后**从没更新过**，
    // 恒为 593：窗口一 resize / DPI 一缩放，hover 高亮就画到与鼠标错位的位置
    // （真机截图里按钮区域那块浅灰底就是错位的 hover fill 残留）。
    pages::WIN_CACHED_W.store(w, Ordering::Relaxed);
    pages::WIN_CACHED_H.store(h, Ordering::Relaxed);

    // 顶栏（坐在整窗渐变上）+ 窗口按钮 + 页签条
    pages::draw_titlebar(mdc, w, h, f_title, f_sub);
    pages::draw_tabbar(mdc, f_sub);
    wlog!("on_paint: titlebar+tabbar drawn");

    // 内容区可视高度（页签条下沿到底部栏上沿）。
    LAST_CLIENT_H.store(h, Ordering::Relaxed);
    let view_top = pages::content_top();
    let view_h = (h - scale(FOOTER_H) - pages::body_pad() - view_top).max(1);

    // 页面绘制的公共入参（DC / 尺寸 / 快照 / 字体组打包，见 `pages::PageCtx`）。
    let snap = snapshot();
    let ctx = |hdc: HDC| pages::PageCtx {
        hdc,
        w,
        h,
        snap,
        scroll: scroll_y(),
        f_sec: f_section,
        f_body,
        f_sub,
        f_tiny,
    };

    // 第一趟：1x1 哑 DC 量内容总高（绘制会被裁到1px 外，等于只跑布局不落像素）。
    // 这样页面绘制函数不需要"干跑"开关，滚动条比例也拿得到真值。
    let total = match Backbuffer::new(1, 1) {
        // 正常路径：用 1x1 内存 DC 跑一遍布局，返回内容底部 y。
        Some(measure) => match cur_tab() {
            1 => pages::draw_menu_page(&ctx(measure.mem)),
            2 => pages::draw_disk_page(&ctx(measure.mem)),
            _ => pages::draw_hw_page(&ctx(measure.mem)),
        },
        // 极端内存压力下 DIB 建不出来：当作内容刚好一屏，不滚（宁可不滚也不崩）。
        None => {
            set_content_h(view_top);
            view_top
        }
    };
    set_content_h(total);
    // —— 磁盘页按分区数自适应窗口高度（第九轮）——
    // 仅磁盘页（tab 2）按内容量缩/放窗口高度，其它页保持默认 `WIN_H`。
    // 未最大化时才调（最大化态下窗口应占满工作区，不能自作主张缩）。
    // `total` 是内容底边（客户坐标，已含顶栏+页签），加底部栏即理想客户高度；
    // 下限 `DISK_MIN_H` 防分区极少时窗口畸形，上限 `WIN_H` 让多分区时靠滚动。
    // 非磁盘页恒为 `WIN_H`，于是切回硬件/右键页窗口自动长回默认高。
    // 收敛性：resize 触发 WM_SIZE→本帧后重绘，下一帧 `desired==h` 不再 resize。
    if !is_zoomed() {
        // 高度上限从固定 `scale(WIN_H)` 改为 `max_win_h()`（=min(设计高, 工作区)）：
        // 小屏（1024×768 工作区 ≈ 738 高 < 948）上窗口高不会超过工作区，
        // 标题栏/底栏留在屏内、内容靠滚动；大屏仍取设计尺寸（零回归）。
        let cap = max_win_h();
        let desired = if cur_tab() == 2 {
            // 间隙 16：详情卡真实底边（返回值）到底栏顶要留出可见净空——
            // 之前是 8，footer 顶与卡片底边**相切**，视觉上两个框融在一起
            // （Z.W. 报告的"下面两个框重叠"，16 分区实机复现确认）。
            (total + scale(FOOTER_H) + scale(16)).clamp(scale(DISK_MIN_H), cap)
        } else {
            cap
        };
        let cur = h;
        if (cur - desired).abs() > scale(2) {
            let mut rc = RECT::default();
            if GetWindowRect(hwnd, &mut rc).is_ok() {
                let nw = rc.right - rc.left;
                // 顶左不变、只改高度：分区少→窗口向下收矮；分区多→clamp 到 WIN_H 靠滚动。
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    rc.left,
                    rc.top,
                    nw,
                    desired,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                apply_rounded_region(hwnd);
            }
        }
    }
    // 钳制滚动范围：内容不足一屏时归零，超出时不能滚过头（底部露白很难看）。
    let max_scroll = max_scroll();
    if scroll_y() > max_scroll {
        set_scroll_y(max_scroll);
    }

    // 第二趟：把 DC 视口上移 scroll_y 像素，实现滚动；用 SaveDC/RestoreDC 成对保存
    // clip 与视口，避免污染后续的底部栏绘制。
    let _saved = SaveDC(mdc);
    let _ = IntersectClipRect(mdc, 0, view_top, w, view_top + view_h);
    let _ = OffsetViewportOrgEx(mdc, 0, -scroll_y(), None);
    let _ = match cur_tab() {
        1 => pages::draw_menu_page(&ctx(mdc)),
        2 => pages::draw_disk_page(&ctx(mdc)),
        _ => pages::draw_hw_page(&ctx(mdc)),
    };
    wlog!("on_paint: content page drawn (tab={})", cur_tab());
    let _ = RestoreDC(mdc, _saved);

    // 固定头部（不随滚动）：硬件页标题+导出按钮、磁盘页说明文字。
    // 必须在 RestoreDC 之后画——头部用屏幕坐标，且要盖在滚动内容上、不被滚动裁剪区裁掉
    // （第十八轮修复：以前头部画在滚动变换内，真机溢出滚动后被裁到页签条下缘）。
    match cur_tab() {
        0 => pages::draw_hw_header(mdc, w, h, f_section, f_body),
        2 => pages::draw_disk_header(mdc, w, h, f_body, snap.map_or(0, |r| r.volumes.len())),
        _ => {}
    }
    wlog!("on_paint: fixed header drawn (tab={})", cur_tab());

    // 右侧滚动条（仅内容溢出时画）。
    if max_scroll > 0 {
        pages::draw_scrollbar(mdc, w, h, view_top, view_h, scroll_y(), max_scroll);
    }

    // 底部提示条 + 版权
    pages::draw_footer(mdc, w, h, f_sub, f_tiny);
    wlog!("on_paint: footer drawn; about to present");

    // blit 到窗口
    bb.present(hdc);

    for f in [f_title, f_body, f_sub, f_tiny, f_section] {
        if !font_invalid(f) {
            let _ = DeleteObject(f.into());
        }
    }
}

/// 截屏：把**整个硬件页**（含滚动后才看得到的分组）渲成一张长图，存 PNG 到桌面并打开。
///
/// ## 为什么不是"把窗口截下来"
/// 窗口只有 948 高，滚动后面的内容不在任何一帧里——普通截屏（BitBlt 客户区）
/// 只能拿到可视区。这里改为**按内容总高另开一张离屏位图**，用与 `on_paint`
/// 完全相同的绘制函数、`scroll = 0` 重画一遍，于是所有分组都在一张图上。
/// 高度来自同一趟布局（1x1 哑 DC 量 `draw_hw_page` 返回值），不会与界面实际布局漂移。
///
/// 画布包含顶栏/页签/头部卡片/底栏，但存盘前**只裁出正文**（见第 5 步）。
///
/// 保存路径与 [`pages::export_report`] 一致（exe 同目录 + 时间戳 + 用默认程序打开）：
/// 不弹"另存为"对话框——网吧维护现场要的是"点了就拿到图"，多一步选择就是负担。
unsafe fn capture_hw_page() {
    if cur_tab() != 0 || snapshot().is_none() {
        return;
    }
    let w = pages::WIN_CACHED_W
        .load(Ordering::Relaxed)
        .max(scale(WIN_W));

    let f_title = font(FONT_UI, -scale(16), FW_BOLD.0 as i32);
    let f_body = font(FONT_UI, -scale(14), 400);
    let f_sub = font(FONT_UI, -scale(13), 400);
    let f_tiny = font(FONT_UI, -scale(12), 400);
    let f_section = font(FONT_UI, -scale(15), FW_BOLD.0 as i32);

    // 1) 量内容总高：与 on_paint 同一套路（1x1 哑 DC 只跑布局）。
    let total = match Backbuffer::new(1, 1) {
        Some(m) => pages::draw_hw_page(&pages::PageCtx {
            hdc: m.mem,
            w,
            h: 1,
            snap: snapshot(),
            scroll: 0,
            f_sec: f_section,
            f_body,
            f_sub,
            f_tiny,
        }),
        None => 0,
    };
    if total <= 0 {
        return;
    }
    // 长图高度 = 内容底边 + 底栏 + 一点净空（与窗口自适应用的间隙一致）。
    let img_h = total + scale(FOOTER_H) + scale(16);
    let Some(bb) = Backbuffer::new(w, img_h) else {
        return;
    };
    let mdc = bb.mem;

    // 2) 整窗渐变底 + 外框（与 on_paint 顺序一致，观感=被"拉长"的窗口截图）
    fill(
        mdc,
        &RECT {
            left: 0,
            top: 0,
            right: w,
            bottom: img_h,
        },
        C_BG,
    );
    pages::draw_window_frame(mdc, w, img_h);
    pages::draw_titlebar(mdc, w, img_h, f_title, f_sub);
    pages::WIN_CACHED_H.store(img_h, Ordering::Relaxed);
    pages::draw_tabbar(mdc, f_sub);
    // 3) 头部固定层 + 完整内容（scroll=0 → 从头画到尾，不再受可视区裁剪）
    pages::draw_hw_header(mdc, w, img_h, f_section, f_body);
    pages::draw_hw_page(&pages::PageCtx {
        hdc: mdc,
        w,
        h: img_h,
        snap: snapshot(),
        scroll: 0,
        f_sec: f_section,
        f_body,
        f_sub,
        f_tiny,
    });
    // 4) 底栏压在最下面（长图的底就是内容底）
    pages::draw_footer(mdc, w, img_h, f_sub, f_tiny);

    // 5) 存 PNG（目录策略与导出报告共用 `pages::save_output`：exe 同目录 → 桌面回退）
    //
    // 只截**框里的正文**：上下裁掉外壳（`hw_body_top` 起、内容底边止），左右裁到
    // 玻璃卡边界（`hw_card_x`）。窗口顶栏（最小化/关闭）、页签、头部卡片（那两个
    // 按钮在图片里点不了、纯误导）、底栏版权行、以及左右窗边留白都不是硬件信息。
    // 做法上**不改绘制路径**——画布仍是完整的窗口长图，只在存盘前按行列裁，
    // 这样长图布局与界面永远同一套算法（几何真源 `hw_body_top` / `hw_card_x`）。
    let ts = winsuitex_core::model::local_timestamp_string().replace([':', ' '], "-");
    let top = pages::hw_body_top(w);
    let (left, right) = pages::hw_card_x(w);
    let crop_h = (total - top).max(1);
    let crop_w = (right - left).max(1);
    let path = match bb.pixels() {
        Some(px) => {
            let stride = (w * 4) as usize;
            let x0 = left as usize * 4;
            let row_w = crop_w as usize * 4;
            let y_end = top + crop_h;
            if y_end as usize * stride <= px.len() && x0 + row_w <= stride {
                // 逐行拷：DIB 行距是 `stride`，裁列必须重排成紧凑的新缓冲。
                let mut body = Vec::with_capacity(row_w * crop_h as usize);
                for row in top..y_end {
                    let s = row as usize * stride + x0;
                    body.extend_from_slice(&px[s..s + row_w]);
                }
                pages::save_output(&format!("WinSuiteX_硬件截图_{ts}.png"), |p| {
                    save_png(p, crop_w, crop_h, &body)
                })
            } else {
                None
            }
        }
        None => None,
    };
    drop(bb);

    for f in [f_title, f_body, f_sub, f_tiny, f_section] {
        if !font_invalid(f) {
            let _ = DeleteObject(f.into());
        }
    }

    match path {
        Some(p) => {
            open_in_default(&p);
        }
        None => pages::show_error("截图保存失败：exe 目录与桌面都不可写，或 PNG 编码不可用"),
    }
}

/// 把 BGRA 像素用 GDI+ 编码成 PNG。
///
/// 走系统自带 `gdiplus.dll` 而不是引 `png`/`flate2` crate：单 exe 分发是硬需求，
/// 引入压缩库会让 exe 胖 ~200KB，而 GDI+ 是 Windows 自带的（Win10/11 必有）。
/// `Ok` 判定用 `Status(0)`：GDI+ 的 `Status::Ok` 就是 0。
unsafe fn save_png(path: &std::path::Path, w: i32, h: i32, px: &[u8]) -> bool {
    use windows::core::GUID;
    use windows::Win32::Graphics::GdiPlus::{
        GdipCreateBitmapFromScan0, GdipDisposeImage, GdipSaveImageToFile, GdiplusShutdown,
        GdiplusStartup, GdiplusStartupInput, GpBitmap, GpImage,
    };
    // GDI+ 的 `PixelFormat32bppARGB`（gdipluspixelformats.h：10 | 32<<8 | Alpha | GDI
    // | Canonical = 0x26200A）。windows-rs 未导出该常量，按官方值写死并注明出处。
    const PIXEL_FMT_32BPP_ARGB: i32 = 0x26200A;

    let mut token = 0usize;
    let input = GdiplusStartupInput {
        GdiplusVersion: 1, // 必须是 1，Default 给的 0 会让 GdiplusStartup 返回 InvalidParameter
        ..Default::default()
    };
    if GdiplusStartup(&mut token, &input, std::ptr::null_mut()).0 != 0 {
        return false;
    }

    // ⚠️ 必须把 alpha 填成不透明（0xFF）。
    // DIB 是 32bpp，但 GDI 的 COLORREF 只有 24 位色——`FillRect` 写 32 位时高字节
    // 恒为 0，于是整块像素 A=0。交给 GDI+ 的 `PixelFormat32bppARGB` 会被当成
    // **全透明图**：RGB 内容其实一分不少，但看图器/微信/浏览器打开是一片空白
    // （真机反馈"截图没有内容"，第二十二轮 b 修复）。
    let mut opaque: Vec<u8> = px.to_vec();
    for p in opaque.chunks_exact_mut(4) {
        p[3] = 0xFF; // BGRA 内存序：第 4 字节是 A
    }

    let mut bmp: *mut GpBitmap = std::ptr::null_mut();
    // 位图是自上而下 DIB（BGRA），与 PixelFormat32bppARGB 的内存序一致，可直接交进去。
    let st = GdipCreateBitmapFromScan0(
        w,
        h,
        w * 4,
        PIXEL_FMT_32BPP_ARGB,
        Some(opaque.as_ptr()),
        &mut bmp,
    );
    let mut ok = st.0 == 0 && !bmp.is_null();
    if ok {
        // PNG 编码器 CLSID（GDI+ 内置，windows-rs 未导出常量，按官方 GUID 写死）。
        let clsid_png = GUID::from_values(
            0x557C_F406,
            0x1A04,
            0x11D3,
            [0x9A, 0x73, 0x00, 0x00, 0xF8, 0x1E, 0xF3, 0x2E],
        );
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let st2 = GdipSaveImageToFile(
            bmp as *mut GpImage,
            windows::core::PCWSTR(wide.as_ptr()),
            &clsid_png,
            std::ptr::null(),
        );
        ok = st2.0 == 0;
        let _ = GdipDisposeImage(bmp as *mut GpImage);
    }
    GdiplusShutdown(token);
    ok
}

/// 鼠标移动：更新悬停态（导出按钮 / 窗口按钮）。
unsafe fn on_mouse_move(hwnd: HWND, lparam: LPARAM) {
    let x = (lparam.0 & 0xFFFF) as i32;
    let screen_y = ((lparam.0 >> 16) & 0xFFFF) as i32;
    // 同 on_click：内容区缓存是内容坐标系，需扣掉滚动偏移。
    let y = screen_y - scroll_y();
    // 头部按钮：0=导出报告 / 1=截图 / -1=无（第二十二轮起两颗按钮共用这一个索引）。
    // ⚠️ 必须用**屏幕坐标** `hit_header` 判（与 on_click 同款）：头部按钮钉在固定层、
    // 不随滚动移动；若用内容坐标 `y = screen_y - scroll` 会随滚动带偏，
    // 拉到底时 hover 高亮也跟着失踪（和点击失效同源）。
    let btn = pages::hit_header(x, screen_y);
    // 窗口按钮（标题栏）在固定顶层、用屏幕坐标，与 on_click 取 .1 一致。
    let tb = pages::hit_test(x, y).1;
    // 开关悬停只在右键页算：其它页的开关缓存是空的，命中恒None。
    let sw = if cur_tab() == 1 {
        pages::switch_at(x, y).map_or(-1, |i| i as i32)
    } else {
        -1
    };
    // 磁盘分区行悬停只在磁盘页算：用 VOL_ROWS 缓存做命中测试（与点击同款，
    // 避免布局算法漂移）。选中行本身有实底，不需要 hover 反馈，故只记录非选中行。
    let vr = if cur_tab() == 2 {
        pages::vol_row_at(x, y).map_or(-1, |i| i as i32)
    } else {
        -1
    };
    let changed = hover_export() != (btn == 0)
        || hover_shot() != (btn == 1)
        || hover_tb() != tb
        || hover_switch() != sw
        || hover_vol_row() != vr;
    set_hover_export(btn == 0);
    set_hover_shot(btn == 1);
    set_hover_tb(tb);
    set_hover_switch(sw);
    set_hover_vol_row(vr);
    if changed {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

/// 左键点击：处理窗口按钮 / 页签 / 导出按钮。
unsafe fn on_click(hwnd: HWND, lparam: LPARAM) {
    let x = (lparam.0 & 0xFFFF) as i32;
    let screen_y = ((lparam.0 >> 16) & 0xFFFF) as i32;
    // ⚠️ 内容区绘制用了 `OffsetViewportOrgEx(0, -scroll_y)`，所以**缓存里存的是
    // 内容坐标系**的矩形；鼠标坐标是窗口坐标系。命中前必须减掉 scroll_y。
    // 这个 bug 很隐蔽：开关明明画在那里，点了没反应（首次实测就踩了，
    // 矩形top=203 而点击 y=246，正好差 43px=滚动量）。
    let y = screen_y - scroll_y();

    // 头部按钮（固定层，不随滚动）→ 必须用**屏幕坐标** screen_y 判。
    // ⚠️ 不能传内容坐标 `y`：`y = screen_y - scroll`，滚动越多偏越多，
    // 拉到底时按钮彻底点不动（与 hover 同理）。
    let hh = pages::hit_header(x, screen_y);
    if hh >= 0 {
        if hh == 0 {
            pages::export_report();
        } else {
            capture_hw_page();
        }
        return;
    }

    // 窗口按钮（标题栏区）。标题栏/页签在滚动区之外，用屏幕坐标判断。
    // `tb` 来自 `hit_test`：窗口按钮在固定顶层，其命中不受滚动影响（x 不变、
    // y 恒 < TITLE_H），可继续用内容坐标 y 取。
    let tb = pages::hit_test(x, y).1;
    if screen_y < pages::body_pad() + scale(TITLE_H) {
        match tb {
            0 => {
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
                return;
            }
            1 => {
                let _ = DestroyWindow(hwnd);
                return;
            }
            _ => {}
        }
        return;
    }

    // 页签切换（同样在滚动区外，用屏幕坐标）
    if screen_y < pages::content_top() {
        let idx = pages::tab_at(x);
        if idx >= 0 && idx != cur_tab() {
            set_cur_tab(idx);
            reset_scroll();
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
        return;
    }

    // 头部按钮已在上面处理；此处只处理内容区（开关 / 磁盘行），用内容坐标 y。

    // 右键增强页：点开关 = 真启用/禁用
    if cur_tab() == 1 {
        if let Some(i) = pages::switch_at(x, y) {
            toggle_feature(i);
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
        return;
    }

    // 磁盘页：点行 = 选中该分区（详情卡跟随）
    if cur_tab() == 2 {
        if let Some(i) = pages::vol_row_at(x, y) {
            pages::set_sel_vol(i as i32);
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }
}

/// 切换第 `idx` 个右键增强功能（`slot 0` = 经典菜单，`slot 1..=5` = FEATURES）。
///
/// 逻辑直接复用 `winsuitex_core::feature`，与 CLI / HTML 报告读同一份注册表，
/// 不会出现"GUI 说已启用、报告说未启用"的分裂。
fn toggle_feature(idx: usize) {
    use winsuitex_core::feature::{install, is_installed, uninstall, FEATURES};
    use winsuitex_core::model::RegRoot;

    // slot 0 = 经典右键菜单（绘制顺序置顶，但语义上仍是 Win11 的 CLSID 开关）。
    // 注册表写完后**必须重启 explorer** 才生效（SHChangeNotify 对 shell 扩展
    // 注册覆盖无效）——这就是"启用经典菜单不生效"的根因（第十六轮）。
    if idx == 0 {
        let now_on = pages::classic_menu_on();
        pages::set_classic_menu(!now_on);
        restart_explorer();
        return;
    }
    let f_idx = idx - 1;
    let Some(f) = FEATURES.get(f_idx) else {
        return;
    };
    let mut reg = winsuitex_platform::WinRegistry;
    // Shell DLL 先就位（对齐 C++ Feature_Install 入口的 EnsureShellDll）：
    // Win11 COM 直显的 InProcServer32 与 SuperHidden 的 rundll32 回退都写它的
    // 绝对路径。已存在则零开销复用；释放失败必须让人看见（第十七轮）。
    let dll = match winsuitex_core::feature::ensure_shell_dll(RegRoot::CurrentUser) {
        Ok(p) => Some(p),
        Err(e) => {
            pages::show_error(&format!("Shell 组件释放失败：{e}"));
            return;
        }
    };
    let result = if is_installed(&reg, f) {
        uninstall(&mut reg, f, RegRoot::CurrentUser)
    } else {
        install(
            &mut reg,
            f,
            RegRoot::CurrentUser,
            &expand_env,
            dll.as_deref(),
        )
    };
    if let Err(e) = result {
        // 失败必须让人看见：静默失败会让人以为"开关坏了"。
        pages::show_error(&format!("操作失败：{e}"));
    }
    // 成功后**不广播 SHChangeNotify**：这些是 `Directory\shell` 等静态注册表
    // 右键项，explorer 构建右键菜单时动态读取，无需通知；C++ 版从不广播而
    // 功能一直正常。之前每次切换都广播 SHCNE_ASSOCCHANGED，桌面/任务栏会
    // 整体重刷一下（Z.W. 报告"点一下就刷新"），纯属有害的行为偏离。
}

/// 重启资源管理器（explorer.exe）。
///
/// `{86ca1aa0}` 经典菜单覆盖**只有 explorer 重新启动才会读取**——这是
/// "启用经典菜单不生效"的唯一根因，没有任何轻量通知能替代。
/// 实现：`GetShellWindow` 找到 shell 进程 → 终止；winlogon 的
/// `AutoRestartShell=1`（系统默认）会自动拉起新 explorer；稍等后若 shell
/// 仍缺位（自动重启失败），再显式启动一次双保险——幂等，不会开出重复 shell。
pub fn restart_explorer() {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    use windows::Win32::UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId};
    // SAFETY: 全部为无副作用的 Win32 查询/句柄操作；句柄在本函数内闭合。
    unsafe {
        let shell = GetShellWindow();
        if !shell.is_invalid() {
            let mut pid = 0u32;
            let _ = GetWindowThreadProcessId(shell, Some(&mut pid));
            if pid != 0 {
                if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                    let _ = TerminateProcess(h, 0);
                    let _ = CloseHandle(h);
                }
            }
        }
    }
    // 等 shell 退出 + winlogon 自动重启（通常 <1s）。
    std::thread::sleep(std::time::Duration::from_millis(600));
    // SAFETY: 常量串。
    unsafe {
        if GetShellWindow().is_invalid() {
            // shell 仍未恢复 → 显式拉起（open_in_default 走 ShellExecuteW，
            // explorer.exe 经 App Paths 注册解析，无需全路径）。
            open_in_default(std::path::Path::new("explorer.exe"));
        }
    }
}

/// 展开 `%SystemRoot%` 等环境变量（`feature::install` 写的注册表值里带 `%...%`）。
fn expand_env(src: &str) -> String {
    let w_src: Vec<u16> = src.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: 两次调用都传同一有效NUL 结尾串；第二次的 buf 长度来自第一次探测。
    unsafe {
        let need = windows::Win32::System::Environment::ExpandEnvironmentStringsW(
            windows::core::PCWSTR(w_src.as_ptr()),
            None,
        );
        if need == 0 {
            return src.to_string();
        }
        let mut buf = vec![0u16; need as usize];
        let got = windows::Win32::System::Environment::ExpandEnvironmentStringsW(
            windows::core::PCWSTR(w_src.as_ptr()),
            Some(&mut buf),
        );
        if got == 0 || got > need {
            return src.to_string();
        }
        // 去掉尾部 NUL 再转回 String。
        String::from_utf16_lossy(&buf[..(got as usize).saturating_sub(1)])
    }
}

/// 用系统默认程序打开文件（`ShellExecuteW`）。
///
/// 返回是否真正拉起了关联程序——`ShellExecuteW` 返回值 ≤ 32 即失败（错误码见
/// `ShellExecuteW` 文档，如 `SE_ERR_NOASSOC`=31 无关联程序、`SE_ERR_ACCESSDENIED`=5）。
/// 失败要**显式提示**，不能静默：之前截图/报告生成成功却"没自动打开"，根因就藏在
/// 被 `_ =` 吞掉的返回值里，用户完全看不到原因。
fn open_in_default(path: &std::path::Path) -> bool {
    let wpath: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let op: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: 两个字符串均以 NUL 结尾并在本调用期间存活；参数为 NULL 表示用默认关联。
    let hinst = unsafe {
        windows::Win32::UI::Shell::ShellExecuteW(
            None,
            windows::core::PCWSTR(op.as_ptr()),
            windows::core::PCWSTR(wpath.as_ptr()),
            None::<&windows::core::PCWSTR>,
            None::<&windows::core::PCWSTR>,
            SW_SHOWNORMAL,
        )
    };
    // `HINSTANCE` 是句柄包装；经典 Win32 约定：返回值 > 32 表示成功。
    let ok = (hinst.0 as isize) > 32;
    if !ok {
        pages::show_error(&format!(
            "无法自动打开文件：{}\n（可能无关联程序，或该目录受限）",
            path.display()
        ));
    }
    ok
}

#[cfg(test)]
mod wheel_dir_tests {
    //! 第十五轮：滚轮方向约定必须钉死。
    //!
    //! VM 的窗口内容撑不满一屏（无法目测滚动），这条约定又曾被写反过一次，
    //! 所以把"格数 → 滚动增量"的符号收进纯函数并在这里钉死：
    //! 上滚（delta 正）必须减小 `scroll_y`（看上面的内容），下滚反之。
    //! 测试里 DPI 静态量保持默认 96，`scale(n) == n`。

    use super::wheel_scroll_delta;

    #[test]
    fn wheel_up_decreases_scroll_offset() {
        assert!(
            wheel_scroll_delta(1) < 0,
            "上滚应减小 scroll_y（看上面的内容），实际 {}",
            wheel_scroll_delta(1)
        );
    }

    #[test]
    fn wheel_down_increases_scroll_offset() {
        assert!(
            wheel_scroll_delta(-1) > 0,
            "下滚应增大 scroll_y（看下面的内容），实际 {}",
            wheel_scroll_delta(-1)
        );
    }

    #[test]
    fn one_notch_scrolls_exactly_one_row() {
        assert_eq!(wheel_scroll_delta(1), -super::theme::ROW_H);
        assert_eq!(wheel_scroll_delta(-3), 3 * super::theme::ROW_H);
    }
}
