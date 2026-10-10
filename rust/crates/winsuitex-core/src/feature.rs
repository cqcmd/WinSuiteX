//! 右键菜单功能表（数据驱动）与**可测试的**安装/卸载逻辑。
//!
//! 相比 C++ 版的关键改进：
//! - `regParent/regParent2/regParent3` 三个平行字段 → `reg_parents` 切片；
//! - `kind` 与 `command`/`clsid`/`toggleArg` 的松散组合 → 枚举携带数据，
//!   **非法组合在编译期不可构造**；
//! - 安装/卸载逻辑通过 `Registry` trait 操作 → 可用内存实现单测，
//!   而 C++ 版只能"真写 HKCU 再还原"。

use crate::error::Result;
use crate::model::{Feature, FeatureId, FeatureKind, RegRoot, RegValueKind};
use crate::traits::Registry;

// ---- 注册表父路径（与 C++ features.cpp:437 保持一致）----
pub const THIS_PC_SHELL: &str = r"CLSID\{20D04FE0-3AEA-1069-A2D8-08002B30309D}\shell";
pub const DIR_SHELL: &str = r"Directory\shell";
pub const DIR_BG_SHELL: &str = r"Directory\Background\shell";
pub const DESKTOP_BG_SHELL: &str = r"DesktopBackground\shell";

// ---- Win11 新菜单 COM CLSID（与 C++ features.cpp:441-450 保持一致）----
pub const CLSID_WIN11_COMPMGMT: &str = "{245923CB-428A-4AA0-A0C1-3B6A30823500}";
pub const CLSID_WIN11_DEVMGMT: &str = "{25EBD382-8E3E-4D90-9F4E-A7F21A118CE1}";
pub const CLSID_WIN11_SUPERHIDDEN: &str = "{9B27F9A8-2EEA-4F7B-99BD-9B6DCB96AB49}";
pub const CLSID_WIN11_CMDHERE: &str = "{A9160049-1FCF-435D-BEBA-EEAFF59FFB8F}";
pub const CLSID_WIN11_PWSH: &str = "{0F486944-1B88-4059-ADFD-2ACF2469D7F2}";
/// shdocvw 委托 CLSID（SuperHidden 专用）
pub const CLSID_SUPERHIDDEN: &str = "{3D6EAAA5-35CF-4051-95E8-A18A4928CF0D}";

/// 全部右键功能。新增功能 = 在此加一条，**无需改动任何逻辑代码**。
pub const FEATURES: &[Feature] = &[
    Feature {
        id: FeatureId("ComputerManagement"),
        display_name: "计算机管理",
        description: "在“此电脑”右键菜单添加“计算机管理”，一键打开 compmgmt.msc",
        reg_parents: &[THIS_PC_SHELL],
        verb: "ComputerManagement",
        kind: FeatureKind::RunCommand {
            command: r#"mmc.exe "%SystemRoot%\system32\compmgmt.msc""#,
        },
        menu_icon: Some("compmgmt.msc"),
        win11_clsid: Some(CLSID_WIN11_COMPMGMT),
    },
    Feature {
        id: FeatureId("DeviceManager"),
        display_name: "设备管理器",
        description: "演示模块：在“此电脑”右键菜单添加“设备管理器”(devmgmt.msc)",
        reg_parents: &[THIS_PC_SHELL],
        verb: "DeviceManager",
        kind: FeatureKind::RunCommand {
            command: r#"mmc.exe "%SystemRoot%\system32\devmgmt.msc""#,
        },
        menu_icon: Some("devmgmt.msc"),
        win11_clsid: Some(CLSID_WIN11_DEVMGMT),
    },
    Feature {
        id: FeatureId("SuperHidden"),
        display_name: "显示/隐藏 系统文件",
        description: "在文件夹空白处右键一键切换“显示/隐藏 系统文件与扩展名”",
        reg_parents: &[DIR_BG_SHELL],
        verb: "SuperHiddenToggle",
        kind: FeatureKind::ShellExtension {
            toggle_fn: "ToggleSuperHidden",
        },
        menu_icon: None,
        win11_clsid: Some(CLSID_WIN11_SUPERHIDDEN),
    },
    Feature {
        id: FeatureId("CmdHere"),
        display_name: "命令提示符",
        description: "在文件夹或空白处右键一键打开命令提示符，并自动定位到当前文件夹",
        reg_parents: &[DIR_SHELL, DIR_BG_SHELL, DESKTOP_BG_SHELL],
        verb: "CmdHere",
        kind: FeatureKind::RunCommand {
            command: r#"%SystemRoot%\system32\cmd.exe /s /k pushd "%V""#,
        },
        menu_icon: Some("builtin:terminal"),
        win11_clsid: Some(CLSID_WIN11_CMDHERE),
    },
    Feature {
        id: FeatureId("PowerShellHere"),
        display_name: "PowerShell",
        description: "在文件夹或空白处右键一键打开 PowerShell，并自动定位到当前文件夹",
        reg_parents: &[DIR_SHELL, DIR_BG_SHELL, DESKTOP_BG_SHELL],
        verb: "PowerShellHere",
        kind: FeatureKind::RunCommand {
            command: r#"%SystemRoot%\system32\WindowsPowerShell\v1.0\powershell.exe -NoExit -Command "Set-Location -LiteralPath '%V'""#,
        },
        menu_icon: Some("powershell.exe"),
        win11_clsid: Some(CLSID_WIN11_PWSH),
    },
];

/// 注册表前缀：`HKCU` 需落在 `Software\Classes\` 下（对应 C++ `ClassesPrefix`）。
fn prefix(root: RegRoot) -> &'static str {
    match root {
        RegRoot::CurrentUser => r"Software\Classes\",
        RegRoot::ClassesRoot => "",
        RegRoot::LocalMachine => "",
    }
}

// ---- Shell DLL（SuperHidden 等所有 Win11 COM 直显功能的服务器）----
//
// C++ 版把 WinSuiteXShell.dll 以 RCDATA 内嵌进 exe，安装时释放（EnsureShellDll，
// features.cpp:381）；本实现同一策略：内嵌字节 + 落盘。DLL 由 C++ 构建链产出
// （build.bat [2/5]，explorercommand.cpp，无提权清单——否则 Explorer 拒载），
// 源件存于 `assets/`，升级 DLL = 覆盖该文件后重编。
static SHELL_DLL: &[u8] = include_bytes!("../assets/WinSuiteXShell.dll");
pub const SHELL_DLL_NAME: &str = "WinSuiteXShell.dll";

/// Shell DLL 的落盘路径（对齐 C++ `GetShellDllPath`）：
/// - per-user(HKCU)：`%LOCALAPPDATA%\WinSuiteX\WinSuiteXShell.dll`（标准用户可写，
///   同用户上下文的 Explorer 可加载），极端回退 exe 同目录；
/// - 机器级：`%SystemRoot%\WinSuiteXShell.dll`（已有 System32 副本则优先用之）。
pub fn shell_dll_path(root: RegRoot) -> std::path::PathBuf {
    if matches!(root, RegRoot::CurrentUser) {
        if let Ok(la) = std::env::var("LOCALAPPDATA") {
            if !la.is_empty() {
                return std::path::Path::new(&la)
                    .join("WinSuiteX")
                    .join(SHELL_DLL_NAME);
            }
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                return dir.join(SHELL_DLL_NAME);
            }
        }
    }
    let win = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let p = std::path::Path::new(&win).join(SHELL_DLL_NAME);
    if p.exists() {
        return p;
    }
    std::path::Path::new(&win)
        .join("System32")
        .join(SHELL_DLL_NAME)
}

/// 把 DLL 释放到指定目录（已存在则直接复用，对齐 C++ `EnsureShellDll(force=FALSE)`）。
/// 独立成 `dir` 参数是为了可测（tempdir 单测，不碰真实 LOCALAPPDATA）。
fn release_dll_into(dir: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    let path = dir.join(SHELL_DLL_NAME);
    if path.exists() {
        return Ok(path);
    }
    std::fs::create_dir_all(dir)?;
    std::fs::write(&path, SHELL_DLL)?;
    Ok(path)
}

/// 确保 Shell DLL 已就位并返回其**绝对路径**（注册表 InProcServer32 / rundll32
/// 回退命令都要写绝对路径——之前写相对名，Explorer 加载不到，SuperHidden 因此
/// “点了没反应”，见第十七轮）。失败（磁盘不可写等）返回 Err，调用方必须可见。
pub fn ensure_shell_dll(root: RegRoot) -> std::io::Result<std::path::PathBuf> {
    let path = shell_dll_path(root);
    match path.parent() {
        Some(dir) => release_dll_into(dir),
        None => Ok(path),
    }
}

/// 功能是否已安装（HKCR 或 HKCU 任一存在即视为已安装）。
pub fn is_installed(reg: &dyn Registry, f: &Feature) -> bool {
    [RegRoot::ClassesRoot, RegRoot::CurrentUser]
        .iter()
        .any(|&root| {
            f.reg_parents.iter().any(|parent| {
                let key = format!("{}{}\\{}", prefix(root), parent, f.verb);
                reg.exists(root, &key)
            })
        })
}

/// 安装一个功能。
///
/// `expand`：环境变量展开（如 `%SystemRoot%` → `C:\Windows`）。由调用方注入，
/// 便于测试；生产环境传平台实现。
///
/// `dll_path`：Shell DLL 绝对路径（`ensure_shell_dll` 的返回值）。所有带
/// `win11_clsid` 的功能都需要（InProcServer32 指向它），SuperHidden 的 rundll32
/// 回退命令也用它。测试传 `None` → 退化写相对文件名（只验证注册表结构）。
///
/// 注意：C++ 版会先把 `%SystemRoot%` 展开再写入，以杜绝 Explorer 把 `%S`
/// 当格式符解析成 "1ystemRoot" 的损坏问题——本实现沿用该策略。
pub fn install(
    reg: &mut dyn Registry,
    f: &Feature,
    root: RegRoot,
    expand: &dyn Fn(&str) -> String,
    dll_path: Option<&std::path::Path>,
) -> Result<()> {
    let dll = dll_path
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| SHELL_DLL_NAME.to_string());
    for parent in f.reg_parents {
        let key = format!("{}{}\\{}", prefix(root), parent, f.verb);
        reg.write_string(root, &key, None, f.display_name, RegValueKind::Sz)?;

        match f.kind {
            FeatureKind::RunCommand { command } => {
                // 展开环境变量后再存盘
                let cmd = expand(command);
                let cmd_key = format!("{}\\command", key);
                reg.write_string(root, &cmd_key, None, &cmd, RegValueKind::Sz)?;
            }
            FeatureKind::ShellExtension { toggle_fn } => {
                // rundll32 回退命令（绝对路径）：Win11 新菜单没走 COM 时，
                // Explorer 靠这个子键执行切换——没有它就会把 verb 默认值
                // （显示名文字）当文件去“打开”而报错（C++ InstallShellExtension
                // 路径 A 的“关键回退”，第十七轮补齐）。
                let cmd_key = format!("{}\\command", key);
                let rundll = format!(r#"rundll32 "{}",{}"#, dll, toggle_fn);
                reg.write_string(root, &cmd_key, None, &rundll, RegValueKind::Sz)?;
            }
        }

        // Win11 新菜单直显：DelegateExecute 指向 COM 服务器
        if let Some(clsid) = f.win11_clsid {
            reg.write_string(root, &key, Some("DelegateExecute"), clsid, RegValueKind::Sz)?;
            register_win11_clsid(reg, f, root, clsid, expand, &dll)?;
        }
    }
    Ok(())
}

/// 注册 Win11 CLSID：`InProcServer32`（**绝对路径**）+ `Title`/`Command`。
fn register_win11_clsid(
    reg: &mut dyn Registry,
    f: &Feature,
    root: RegRoot,
    clsid: &str,
    expand: &dyn Fn(&str) -> String,
    dll: &str,
) -> Result<()> {
    let base = format!("{}CLSID\\{}", prefix(root), clsid);
    reg.write_string(root, &base, None, f.display_name, RegValueKind::Sz)?;
    reg.write_string(
        root,
        &format!("{}\\InProcServer32", base),
        None,
        dll,
        RegValueKind::Sz,
    )?;
    reg.write_string(
        root,
        &format!("{}\\InProcServer32", base),
        Some("ThreadingModel"),
        "Apartment",
        RegValueKind::Sz,
    )?;
    reg.write_string(root, &base, Some("Title"), f.display_name, RegValueKind::Sz)?;

    if let FeatureKind::RunCommand { command } = f.kind {
        let cmd = expand(command);
        reg.write_string(root, &base, Some("Command"), &cmd, RegValueKind::Sz)?;
    } else if let FeatureKind::ShellExtension { toggle_fn } = f.kind {
        reg.write_string(
            root,
            &base,
            Some("Command"),
            &format!("dll:{}", toggle_fn),
            RegValueKind::Sz,
        )?;
    }
    Ok(())
}

/// 卸载：删除所有父路径下的 verb 键，以及 Win11 CLSID 键。
/// 键不存在视为成功（最佳努力，与 C++ `DeleteRegTreeKey` 语义一致）。
pub fn uninstall(reg: &mut dyn Registry, f: &Feature, root: RegRoot) -> Result<()> {
    for parent in f.reg_parents {
        let key = format!("{}{}\\{}", prefix(root), parent, f.verb);
        reg.delete_tree(root, &key)?;
    }
    if let Some(clsid) = f.win11_clsid {
        reg.delete_tree(root, &format!("{}CLSID\\{}", prefix(root), clsid))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Volume;
    use std::collections::HashMap;

    /// 内存注册表：让破坏性操作可测。这是 C++ 版做不到的关键能力。
    #[derive(Default)]
    struct InMemoryRegistry(HashMap<(RegRoot, String, Option<String>), String>);

    impl Registry for InMemoryRegistry {
        fn read_string(
            &self,
            root: RegRoot,
            path: &str,
            name: Option<&str>,
        ) -> Result<Option<String>> {
            Ok(self
                .0
                .get(&(root, path.to_string(), name.map(str::to_string)))
                .cloned())
        }
        fn write_string(
            &mut self,
            root: RegRoot,
            path: &str,
            name: Option<&str>,
            value: &str,
            _kind: RegValueKind,
        ) -> Result<()> {
            self.0.insert(
                (root, path.to_string(), name.map(str::to_string)),
                value.to_string(),
            );
            Ok(())
        }
        fn write_dword(
            &mut self,
            root: RegRoot,
            path: &str,
            name: Option<&str>,
            value: u32,
        ) -> Result<()> {
            self.0.insert(
                (root, path.to_string(), name.map(str::to_string)),
                value.to_string(),
            );
            Ok(())
        }
        fn delete_tree(&mut self, root: RegRoot, path: &str) -> Result<()> {
            let p = format!("{}\\", path);
            self.0
                .retain(|(r, k, _), _| !(*r == root && (k == path || k.starts_with(&p))));
            Ok(())
        }
        fn delete_value(&mut self, root: RegRoot, path: &str, name: &str) -> Result<()> {
            self.0
                .remove(&(root, path.to_string(), Some(name.to_string())));
            Ok(())
        }
        fn exists(&self, root: RegRoot, path: &str) -> bool {
            self.0.keys().any(|(r, k, _)| *r == root && k == path)
        }
    }

    fn no_expand(s: &str) -> String {
        s.to_string()
    }

    #[test]
    fn install_then_is_installed() {
        let mut reg = InMemoryRegistry::default();
        let f = &FEATURES[0];
        assert!(!is_installed(&reg, f));
        install(&mut reg, f, RegRoot::ClassesRoot, &no_expand, None).unwrap();
        assert!(is_installed(&reg, f));
    }

    #[test]
    fn install_expands_environment_variables() {
        // 复刻 C++ 版防损坏策略：%SystemRoot% 必须在写盘前展开
        let mut reg = InMemoryRegistry::default();
        let f = &FEATURES[0];
        let expand = |s: &str| s.replace("%SystemRoot%", "C:\\Windows");
        install(&mut reg, f, RegRoot::ClassesRoot, &expand, None).unwrap();

        let key = format!("{}\\{}\\command", THIS_PC_SHELL, f.verb);
        let stored = reg
            .read_string(RegRoot::ClassesRoot, &key, None)
            .unwrap()
            .expect("command 应已写入");
        assert!(
            !stored.contains("%SystemRoot%"),
            "残留未展开变量: {}",
            stored
        );
        assert!(stored.contains(r"C:\Windows\system32\compmgmt.msc"));
    }

    #[test]
    fn multi_parent_feature_writes_all_paths() {
        let mut reg = InMemoryRegistry::default();
        let f = &FEATURES[3]; // CmdHere，3 个父路径
        assert_eq!(f.reg_parents.len(), 3);
        install(&mut reg, f, RegRoot::ClassesRoot, &no_expand, None).unwrap();
        for p in f.reg_parents {
            assert!(reg.exists(RegRoot::ClassesRoot, &format!("{}\\{}", p, f.verb)));
        }
    }

    #[test]
    fn uninstall_clears_everything() {
        let mut reg = InMemoryRegistry::default();
        let f = &FEATURES[3];
        install(&mut reg, f, RegRoot::ClassesRoot, &no_expand, None).unwrap();
        uninstall(&mut reg, f, RegRoot::ClassesRoot).unwrap();
        assert!(!is_installed(&reg, f));
        // Win11 CLSID 也应清掉
        assert!(!reg.exists(
            RegRoot::ClassesRoot,
            &format!("CLSID\\{}", f.win11_clsid.unwrap())
        ));
    }

    #[test]
    fn uninstall_is_idempotent() {
        // 幂等：对未安装的功能调用卸载不应报错
        let mut reg = InMemoryRegistry::default();
        let f = &FEATURES[0];
        uninstall(&mut reg, f, RegRoot::ClassesRoot).unwrap();
        uninstall(&mut reg, f, RegRoot::ClassesRoot).unwrap();
    }

    #[test]
    fn hkcu_gets_software_classes_prefix() {
        let mut reg = InMemoryRegistry::default();
        let f = &FEATURES[0];
        install(&mut reg, f, RegRoot::CurrentUser, &no_expand, None).unwrap();
        assert!(reg.exists(
            RegRoot::CurrentUser,
            &format!(r"Software\Classes\{}\{}", THIS_PC_SHELL, f.verb)
        ));
    }

    // ---- 第十七轮：SuperHidden（ShellExtension）注册表结构 + DLL 释放 ----

    #[test]
    fn superhidden_writes_rundll_fallback_and_abs_inproc() {
        // 根因复现：之前 verb\command 被写成无效的 "dll:ToggleSuperHidden"、
        // InProcServer32 写相对名 → Explorer 加载不到，功能“点了没反应”。
        // 对齐 C++：verb\command = rundll32 "绝对路径",ToggleSuperHidden；
        // CLSID\InProcServer32 = 绝对路径；CLSID\Command = "dll:ToggleSuperHidden"。
        let mut reg = InMemoryRegistry::default();
        let f = &FEATURES[2]; // SuperHidden
        assert!(matches!(f.kind, FeatureKind::ShellExtension { .. }));
        let dll = r"C:\Users\z\AppData\Local\WinSuiteX\WinSuiteXShell.dll";
        install(
            &mut reg,
            f,
            RegRoot::CurrentUser,
            &no_expand,
            Some(std::path::Path::new(dll)),
        )
        .unwrap();

        let pre = r"Software\Classes\";
        let verb_key = format!("{pre}{}\\{}", DIR_BG_SHELL, f.verb);
        // verb 默认值必须是显示名文字（绝不能是 CLSID 字符串，否则 Win11 新菜单直接显示 GUID）
        assert_eq!(
            reg.read_string(RegRoot::CurrentUser, &verb_key, None)
                .unwrap()
                .as_deref(),
            Some(f.display_name)
        );
        // rundll32 回退命令带绝对路径
        let cmd = reg
            .read_string(RegRoot::CurrentUser, &format!("{verb_key}\\command"), None)
            .unwrap()
            .expect("rundll32 回退命令应已写入");
        assert!(
            cmd.starts_with(r#"rundll32 ""#)
                && cmd.contains(dll)
                && cmd.ends_with(",ToggleSuperHidden"),
            "rundll32 回退命令不对: {cmd}"
        );
        // CLSID：InProcServer32 绝对路径 + COM 内调用命令格式
        let clsid_base = format!("{pre}CLSID\\{}", f.win11_clsid.unwrap());
        assert_eq!(
            reg.read_string(
                RegRoot::CurrentUser,
                &format!("{clsid_base}\\InProcServer32"),
                None
            )
            .unwrap()
            .as_deref(),
            Some(dll)
        );
        assert_eq!(
            reg.read_string(RegRoot::CurrentUser, &clsid_base, Some("Command"))
                .unwrap()
                .as_deref(),
            Some("dll:ToggleSuperHidden")
        );
    }

    #[test]
    fn release_dll_writes_embedded_bytes_and_is_reusable() {
        let dir = std::env::temp_dir().join(format!("winsuitex_dlltest_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let p1 = release_dll_into(&dir).expect("首次释放应成功");
        assert!(p1.exists());
        assert_eq!(
            std::fs::metadata(&p1).unwrap().len(),
            SHELL_DLL.len() as u64
        );
        // 二次调用：已存在直接复用（不重写，幂等）
        let p2 = release_dll_into(&dir).expect("复用应成功");
        assert_eq!(p1, p2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // 确保 model 里的 Volume 未被误删（编译期引用）
    #[allow(dead_code)]
    fn _assert_volume_exists(_v: Volume) {}
}
