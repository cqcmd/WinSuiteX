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
/// 注意：C++ 版会先把 `%SystemRoot%` 展开再写入，以杜绝 Explorer 把 `%S`
/// 当格式符解析成 "1ystemRoot" 的损坏问题——本实现沿用该策略。
pub fn install(
    reg: &mut dyn Registry,
    f: &Feature,
    root: RegRoot,
    expand: &dyn Fn(&str) -> String,
) -> Result<()> {
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
                let cmd = format!("dll:{}", toggle_fn);
                let cmd_key = format!("{}\\command", key);
                reg.write_string(root, &cmd_key, None, &cmd, RegValueKind::Sz)?;
            }
        }

        // Win11 新菜单直显：DelegateExecute 指向 COM 服务器
        if let Some(clsid) = f.win11_clsid {
            reg.write_string(root, &key, Some("DelegateExecute"), clsid, RegValueKind::Sz)?;
            register_win11_clsid(reg, f, root, clsid, expand)?;
        }
    }
    Ok(())
}

/// 注册 Win11 CLSID：`InProcServer32` + `Title`/`Command`/`Icon`。
fn register_win11_clsid(
    reg: &mut dyn Registry,
    f: &Feature,
    root: RegRoot,
    clsid: &str,
    expand: &dyn Fn(&str) -> String,
) -> Result<()> {
    let base = format!("{}CLSID\\{}", prefix(root), clsid);
    reg.write_string(root, &base, None, f.display_name, RegValueKind::Sz)?;
    reg.write_string(
        root,
        &format!("{}\\InProcServer32", base),
        None,
        "WinSuiteXShell.dll",
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
        install(&mut reg, f, RegRoot::ClassesRoot, &no_expand).unwrap();
        assert!(is_installed(&reg, f));
    }

    #[test]
    fn install_expands_environment_variables() {
        // 复刻 C++ 版防损坏策略：%SystemRoot% 必须在写盘前展开
        let mut reg = InMemoryRegistry::default();
        let f = &FEATURES[0];
        let expand = |s: &str| s.replace("%SystemRoot%", "C:\\Windows");
        install(&mut reg, f, RegRoot::ClassesRoot, &expand).unwrap();

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
        install(&mut reg, f, RegRoot::ClassesRoot, &no_expand).unwrap();
        for p in f.reg_parents {
            assert!(reg.exists(RegRoot::ClassesRoot, &format!("{}\\{}", p, f.verb)));
        }
    }

    #[test]
    fn uninstall_clears_everything() {
        let mut reg = InMemoryRegistry::default();
        let f = &FEATURES[3];
        install(&mut reg, f, RegRoot::ClassesRoot, &no_expand).unwrap();
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
        install(&mut reg, f, RegRoot::CurrentUser, &no_expand).unwrap();
        assert!(reg.exists(
            RegRoot::CurrentUser,
            &format!(r"Software\Classes\{}\{}", THIS_PC_SHELL, f.verb)
        ));
    }

    // 确保 model 里的 Volume 未被误删（编译期引用）
    #[allow(dead_code)]
    fn _assert_volume_exists(_v: Volume) {}
}
