//! 嵌入 Windows 资源（图标 + 版本信息 + 应用清单）到 `winsuitex-gui-gdi.exe`，
//! 并注入编译期版本号供面板版权行使用。
//!
//! 用 MSVC SDK 的 `rc.exe`（非 MinGW windres），规避 5 个让资源静默失效的坑：
//! 1) 必须 `#define VS_VERSION_INFO 1`（rc 找不到 winver.h 会把字面当字符串名）；
//! 2) 链接 app.res 必须用 CARGO_MANIFEST_DIR 绝对路径（链接器 cwd 是 target/release/deps）；
//! 3) FILEVERSION 第四段不能放 YYYYMMDD（>65535 溢出），用 0,1,0,0 占位、完整日期戳放字符串；
//! 4) 改 app.rc 后 cargo 不重链 app.res，靠 rerun-if-changed=app.rc 强制重编；
//! 5) 嵌入清单时加 /MANIFEST:NO，避免与链接器默认清单冲突。

#[cfg(windows)]
fn main() {
    embed_resources();
    emit_version();
}

#[cfg(not(windows))]
fn main() {
    // 非 Windows 平台（交叉编译等）无法嵌入资源，给一个兜底版本避免编译失败。
    println!("cargo:rustc-env=WSX_GUI_VERSION=0.0.0");
}

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

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=app.ico");
    println!("cargo:rerun-if-changed=app.manifest");
}

#[cfg(windows)]
fn emit_version() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let rcfile = std::path::Path::new(&dir).join("app.rc");
    let ver = std::fs::read_to_string(&rcfile)
        .ok()
        .and_then(|s| file_version_from_rc(&s))
        .unwrap_or_else(|| "0.0.0".to_string());
    println!("cargo:rustc-env=WSX_GUI_VERSION={ver}");
    println!("cargo:rerun-if-changed=app.rc");
}

/// 从 app.rc 抽取 `VALUE "FileVersion", "x.y.z.date"` 里的版本字符串。
#[cfg(windows)]
fn file_version_from_rc(rc: &str) -> Option<String> {
    for line in rc.lines() {
        let line = line.trim();
        if line.starts_with("VALUE") && line.contains("FileVersion") {
            let mut in_q = false;
            let mut cur = String::new();
            let mut segs = Vec::new();
            for ch in line.chars() {
                if ch == '"' {
                    if in_q {
                        segs.push(std::mem::take(&mut cur));
                    }
                    in_q = !in_q;
                } else if in_q {
                    cur.push(ch);
                }
            }
            if let Some(v) = segs.last() {
                if !v.is_empty() {
                    return Some(v.clone());
                }
            }
        }
    }
    None
}
