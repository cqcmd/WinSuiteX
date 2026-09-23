//! 构建期把 `app.rc`（图标 + 版本信息 + 清单）编译进 GUI exe。
//!
//! 守项目铁律：编译产物必须带图标 + 版权信息（署名「持玛多网络 / Z.W.」）。
//! 用 `rc.exe`（Windows SDK，本机已在 `C:\Program Files (x86)\Windows Kits\10\bin\*\x64\rc.exe`）
//! 生成 COFF 资源 `app.res`，再交给链接器嵌入。
//!
//! 守卫：找不到 rc.exe / 编译失败都**静默跳过**，绝不让 GUI 构建因此失败
//! （无 SDK 的极简环境仍可产出无图标 exe，不影响功能与验证）。

#[cfg(windows)]
fn main() {
    // 优先用已知路径下的 rc.exe；找不到再退回 PATH。
    let candidates = [
        "C:\\Program Files (x86)\\Windows Kits\\10\\bin\\10.0.26100.0\\x64\\rc.exe",
        "C:\\Program Files (x86)\\Windows Kits\\10\\bin\\x64\\rc.exe",
    ];
    let rc: Option<String> = candidates
        .iter()
        .copied()
        .find(|p| std::path::Path::new(*p).exists())
        .map(|s| s.to_string())
        .or_else(|| {
            // PATH 里找 rc.exe
            if let Ok(out) = std::process::Command::new("where").arg("rc.exe").output() {
                let s = String::from_utf8_lossy(&out.stdout);
                s.lines().next().map(|l| l.trim().to_string())
            } else {
                None
            }
        });

    let Some(rc) = rc else {
        println!("cargo:warning=rc.exe 未找到，跳过图标/版本资源嵌入（GUI 仍可正常构建）");
        return;
    };

    // 链接器在 target/release/deps 下运行，相对路径找不到 app.res，
    // 必须用清单目录下的绝对路径。
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let res_path = std::path::Path::new(&manifest_dir).join("app.res");
    let rc_path = std::path::Path::new(&manifest_dir).join("app.rc");

    let status = std::process::Command::new(rc)
        .args([
            "/r",
            "/fo",
            &res_path.to_string_lossy(),
            &rc_path.to_string_lossy(),
        ])
        .status();

    match status {
        Ok(s) if s.success() => {
            // 把编译出的 COFF 资源交给链接器（绝对路径）。
            println!("cargo:rustc-link-arg={}", res_path.to_string_lossy());
            println!("cargo:rerun-if-changed=app.rc");
            println!("cargo:rerun-if-changed=app.ico");
            println!("cargo:rerun-if-changed=app.manifest");
        }
        _ => {
            println!("cargo:warning=rc.exe 编译 app.rc 失败，跳过资源嵌入（GUI 仍可正常构建）");
        }
    }
}

#[cfg(not(windows))]
fn main() {
    // 非 Windows 平台（纯开发/CI）无需嵌入 Win32 资源。
}
