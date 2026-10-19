//! 把编译时刻注入 `WSX_BUILD_TS`，让 `--version` 能打印构建时间。
//!
//! 存在的理由：真机排查时反复出现「我到底跑的是新 exe 还是旧 exe」的疑问，
//! 而文件 mtime 在拷贝到客户机后会被改写，无法作为判据。有了编译时间戳，
//! `--version` 一行输出即可确认。
//!
//! Windows 下另外用 MSVC `rc.exe`（非 MinGW windres）把图标 + 版本信息 +
//! 应用清单嵌入 `winsuitex.exe`——遵循「编译产物必须带图标 + 版权信息」。
//! 规避的 5 个坑与 `winsuitex-gui-gdi` 的 build.rs 相同，那边有详细注释：
//! 1) `#define VS_VERSION_INFO 1`（否则 GetFileVersionInfo 返回 1813）；
//! 2) 链 app.res 用 CARGO_MANIFEST_DIR 绝对路径（链接器 cwd 是 target/release/deps）；
//! 3) FILEVERSION 第四段放 MMDD 而非 YYYYMMDD（16 位字溢出成乱码）；
//! 4) 改 app.rc 靠 rerun-if-changed 强制重链；
//! 5) 嵌清单加 /MANIFEST:NO 避免与链接器默认清单冲突。

use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    // 必须显式列出依赖范围：一旦 emit 了任何 rerun-if-changed，cargo 就只按这张
    // 清单判断是否需要重跑。只写 build.rs 的话，改了采集代码却不会重跑构建脚本
    // → 编译时间戳不更新 → 又变成"分不清新旧 exe"。这里把三个 crate 的源码目录
    // 全列上，任何一处改动都会刷新时间戳。资源文件同理——改了 app.rc 不重链，
    // 文件属性里的版本就是旧的。
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=../winsuitex-core/src");
    println!("cargo:rerun-if-changed=../winsuitex-platform/src");
    println!("cargo:rerun-if-changed=../winsuitex-report/src");
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=app.ico");
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=version.ini");

    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // 手写 UTC 换算，避免引入 chrono/time 依赖（保持零外部依赖、编译快）。
    println!(
        "cargo:rustc-env=WSX_BUILD_TS={} UTC",
        format_ts(secs as i64)
    );
    // 同时给出原始 unix 秒，便于脚本比较新旧。
    println!("cargo:rustc-env=WSX_BUILD_EPOCH={secs}");

    // 本地时间戳 + 日期戳（版本号第四段，遵循项目四段式规范 `主.次.修订.编译日期戳`）。
    // 只在**确实拿到时区偏移**时才输出——否则宁可退回三段式版本号，
    // 也绝不显示一个可能差一天的日期（跨零点编译时尤其容易错，那比不显示更糟）。
    if let Some(off) = local_offset_minutes() {
        let local = secs as i64 + off * 60;
        println!(
            "cargo:rustc-env=WSX_BUILD_TS_LOCAL={} 本地",
            format_ts(local)
        );
        println!("cargo:rustc-env=WSX_BUILD_DATE={}", format_date(local));
    }

    #[cfg(windows)]
    embed_resources();
    #[cfg(windows)]
    check_version_parity();
}

/// 用 MSVC SDK 的 `rc.exe` 编译 app.rc → app.res 并链进 exe。
/// rc.exe 找不到时只 warning 不失败——资源是加分项，采集功能才是本体。
#[cfg(windows)]
fn embed_resources() {
    let candidates = [
        "C:\\Program Files (x86)\\Windows Kits\\10\\bin\\10.0.26100.0\\x64\\rc.exe",
        "C:\\Program Files (x86)\\Windows Kits\\10\\bin\\x64\\rc.exe",
    ];
    let rc = candidates
        .iter()
        .copied()
        .find(|p| std::path::Path::new(*p).exists());
    let Some(rc) = rc else {
        println!("cargo:warning=rc.exe 未找到，跳过图标/清单嵌入（仅影响文件属性显示）");
        return;
    };

    // 链接器在 target/release/deps 运行，必须用清单目录绝对路径。
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let res = std::path::Path::new(&dir).join("app.res");
    let rcfile = std::path::Path::new(&dir).join("app.rc");

    let ok = std::process::Command::new(rc)
        .args([
            "/r",
            "/fo",
            &res.to_string_lossy(),
            &rcfile.to_string_lossy(),
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if ok {
        println!("cargo:rustc-link-arg={}", res.to_string_lossy());
        // 我们自己嵌了 RT_MANIFEST 资源，让链接器不要再生成默认清单（否则两份冲突）。
        println!("cargo:rustc-link-arg=/MANIFEST:NO");
    } else {
        println!("cargo:warning=rc.exe 编译 app.rc 失败，跳过图标/清单嵌入");
    }
}

/// version.ini 的前三段（语义号）必须与 workspace Cargo.toml 的 version 一致：
/// `--version` 输出的是 `CARGO_PKG_VERSION + 构建日期`，而文件属性里的版本来自
/// version.ini → app.rc。两者漂移的话，真机上「属性页一个版本、--version 一个版本」
/// 会比没有版本更糟。这里提前 warning（不 fail：版本漂移不该阻断采集功能构建）。
#[cfg(windows)]
fn check_version_parity() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let ini = std::path::Path::new(&dir).join("version.ini");
    let Some(ini_ver) = std::fs::read_to_string(&ini)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    else {
        println!("cargo:warning=version.ini 缺失或为空，无法核对版本一致性");
        return;
    };
    let pkg = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    if ini_ver != pkg {
        println!(
            "cargo:warning=version.ini({ini_ver}) 与 Cargo.toml({pkg}) 语义号不一致：\
             --version 与文件属性会对不上，请同步两者（version_stamp --set {pkg}）"
        );
    }
}

/// 把 unix 秒格式化为 `YYYY-MM-DD HH:MM:SS`（格里历，纯整数运算；用欧几里得取模以
/// 正确处理负偏移把时间推到 1970 之前的情况）。
fn format_ts(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        y,
        m,
        d,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// 日期戳 `YYYYMMDD`——版本号第四段。
fn format_date(secs: i64) -> String {
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    format!("{y:04}{m:02}{d:02}")
}

/// 本地时区偏移（分钟，`local = UTC + offset`）。
///
/// Windows 下读 `ActiveTimeBias`，其定义是 `UTC = local + bias`，故取负
/// （实测 0xFFFFFE20 → -480 → offset = +480 = UTC+8，与重庆一致）。
/// 取不到就返回 None，由调用方退回三段式版本号——绝不猜。
fn local_offset_minutes() -> Option<i64> {
    let out = std::process::Command::new("reg")
        .args([
            "query",
            r"HKLM\SYSTEM\CurrentControlSet\Control\TimeZoneInformation",
            "/v",
            "ActiveTimeBias",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    // 形如："    ActiveTimeBias    REG_DWORD    0xfffffe20"
    let hex = text.split_whitespace().find(|t| t.starts_with("0x"))?;
    let raw = u32::from_str_radix(&hex[2..], 16).ok()?;
    Some(-(raw as i32 as i64))
}

/// Howard Hinnant 的 civil_from_days 算法：days since 1970-01-01 → (年, 月, 日)。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}
