//! Windows 注册表实现（`core::traits::Registry`）。
//!
//! 相比 C++ 版 `common.h` 的改进：
//! - **句柄用 RAII 管理**（`OwnedKey` + `Drop`），杜绝 C++ 里分支提前 return 忘记
//!   `RegCloseKey` 导致的句柄泄漏；
//! - "键不存在视为成功"这类**最佳努力语义集中在 `delete_tree`**，不被调用方各自解释；
//! - 错误带上下文（`Error::Registry { op, message }`），不像 C++ 只返回 BOOL。

use std::ptr;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
    RegSetValueExW, HKEY, HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ,
    KEY_SET_VALUE, REG_DWORD, REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE, REG_SAM_FLAGS, REG_SZ,
    REG_VALUE_TYPE,
};

use crate::to_wide;
use winsuitex_core::error::{Error, Result};
use winsuitex_core::model::{RegRoot, RegValueKind};
use winsuitex_core::traits::Registry;

/// 注册表键句柄的 RAII 包装：离开作用域自动关闭。
struct OwnedKey(HKEY);

impl Drop for OwnedKey {
    fn drop(&mut self) {
        // SAFETY: 句柄由本模块独占创建并持有，Drop 时无人再使用它。
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn root_hkey(root: RegRoot) -> HKEY {
    match root {
        RegRoot::ClassesRoot => HKEY_CLASSES_ROOT,
        RegRoot::CurrentUser => HKEY_CURRENT_USER,
        RegRoot::LocalMachine => HKEY_LOCAL_MACHINE,
    }
}

/// 打开已存在的键。
fn open(root: RegRoot, path: &str, access: REG_SAM_FLAGS) -> Result<OwnedKey> {
    let wide = to_wide(path);
    let mut out = HKEY::default();
    // SAFETY: wide 在本函数内保持有效；out 是合法的 HKEY 输出位。
    let err = unsafe {
        RegOpenKeyExW(
            root_hkey(root),
            PCWSTR(wide.as_ptr()),
            None,
            access,
            &mut out,
        )
    };
    if err != ERROR_SUCCESS {
        return Err(Error::registry(
            "open",
            format!("{} (错误码 {})", path, err.0),
        ));
    }
    Ok(OwnedKey(out))
}

/// 打开键，不存在则创建（用于写入）。
fn create(root: RegRoot, path: &str, access: REG_SAM_FLAGS) -> Result<OwnedKey> {
    let wide = to_wide(path);
    let mut out = HKEY::default();
    // SAFETY: 同上；PCWSTR::null() 表示不指定 class。
    let err = unsafe {
        RegCreateKeyExW(
            root_hkey(root),
            PCWSTR(wide.as_ptr()),
            None,
            PCWSTR(ptr::null()),
            REG_OPTION_NON_VOLATILE,
            access,
            None,
            &mut out,
            None,
        )
    };
    if err != ERROR_SUCCESS {
        return Err(Error::registry(
            "create",
            format!("{} (错误码 {})", path, err.0),
        ));
    }
    Ok(OwnedKey(out))
}

/// 生产用注册表实现。
pub struct WinRegistry;

impl Registry for WinRegistry {
    fn read_string(&self, root: RegRoot, path: &str, name: Option<&str>) -> Result<Option<String>> {
        let key = match open(root, path, KEY_READ) {
            Ok(k) => k,
            Err(_) => return Ok(None), // 键不存在 = 未取到，不是错误
        };

        let name_wide = name.map(to_wide);
        let name_pc = name_wide
            .as_ref()
            .map_or_else(|| PCWSTR(ptr::null()), |w| PCWSTR(w.as_ptr()));

        // 第一次调用只取所需字节数
        let mut size: u32 = 0;
        // SAFETY: name_pc 指向本函数内有效的宽字符串；size 为输出参数。
        let err = unsafe { RegQueryValueExW(key.0, name_pc, None, None, None, Some(&mut size)) };
        if err != ERROR_SUCCESS || size == 0 {
            return Ok(None);
        }

        let mut buf = vec![0u8; size as usize];
        let mut written = size;
        // SAFETY: buf 容量 >= size，写入不会越界。
        let err = unsafe {
            RegQueryValueExW(
                key.0,
                name_pc,
                None,
                None,
                Some(buf.as_mut_ptr()),
                Some(&mut written),
            )
        };
        if err != ERROR_SUCCESS {
            return Ok(None);
        }

        // UTF-16LE → String，去掉结尾的 NUL
        let u16s: Vec<u16> = buf[..written as usize]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        Ok(Some(
            String::from_utf16_lossy(&u16s)
                .trim_end_matches('\0')
                .to_string(),
        ))
    }

    fn write_string(
        &mut self,
        root: RegRoot,
        path: &str,
        name: Option<&str>,
        value: &str,
        kind: RegValueKind,
    ) -> Result<()> {
        let key = create(root, path, KEY_SET_VALUE)?;
        let val_wide = to_wide(value);
        let name_wide = name.map(to_wide);
        let name_pc = name_wide
            .as_ref()
            .map_or_else(|| PCWSTR(ptr::null()), |w| PCWSTR(w.as_ptr()));

        let ty: REG_VALUE_TYPE = match kind {
            RegValueKind::Sz => REG_SZ,
            RegValueKind::ExpandSz => REG_EXPAND_SZ,
        };

        // SAFETY: val_wide 在本函数内有效；长度 = 元素数 × 2 字节（含结尾 NUL）。
        let bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(val_wide.as_ptr().cast::<u8>(), val_wide.len() * 2)
        };
        // SAFETY: 同上，bytes 与 REG_* 类型匹配（REG_SZ/REG_EXPAND_SZ 需要含 NUL 的宽串）。
        let err = unsafe { RegSetValueExW(key.0, name_pc, None, ty, Some(bytes)) };
        if err != ERROR_SUCCESS {
            return Err(Error::registry(
                "write_string",
                format!("{} (错误码 {})", path, err.0),
            ));
        }
        Ok(())
    }

    fn write_dword(
        &mut self,
        root: RegRoot,
        path: &str,
        name: Option<&str>,
        value: u32,
    ) -> Result<()> {
        let key = create(root, path, KEY_SET_VALUE)?;
        let name_wide = name.map(to_wide);
        let name_pc = name_wide
            .as_ref()
            .map_or_else(|| PCWSTR(ptr::null()), |w| PCWSTR(w.as_ptr()));
        let bytes = value.to_le_bytes();
        // SAFETY: REG_DWORD 需要 4 字节小端数据。
        let err = unsafe { RegSetValueExW(key.0, name_pc, None, REG_DWORD, Some(&bytes)) };
        if err != ERROR_SUCCESS {
            return Err(Error::registry(
                "write_dword",
                format!("{} (错误码 {})", path, err.0),
            ));
        }
        Ok(())
    }

    fn delete_tree(&mut self, root: RegRoot, path: &str) -> Result<()> {
        let wide = to_wide(path);
        // SAFETY: wide 在本函数内有效。
        let err = unsafe { RegDeleteTreeW(root_hkey(root), PCWSTR(wide.as_ptr())) };
        // 最佳努力：键/路径不存在 = 目标状态已达成，视为成功（与 C++ DeleteRegTreeKey 一致）
        if err == ERROR_SUCCESS || err == ERROR_FILE_NOT_FOUND || err == ERROR_PATH_NOT_FOUND {
            return Ok(());
        }
        Err(Error::registry(
            "delete_tree",
            format!("{} (错误码 {})", path, err.0),
        ))
    }

    fn delete_value(&mut self, root: RegRoot, path: &str, name: &str) -> Result<()> {
        let key = open(root, path, KEY_SET_VALUE)?;
        let name_wide = to_wide(name);
        // SAFETY: name_wide 在本函数内有效。
        let err = unsafe { RegDeleteValueW(key.0, PCWSTR(name_wide.as_ptr())) };
        if err == ERROR_SUCCESS || err == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        Err(Error::registry(
            "delete_value",
            format!("{}\\{} (错误码 {})", path, name, err.0),
        ))
    }

    fn exists(&self, root: RegRoot, path: &str) -> bool {
        open(root, path, KEY_READ).is_ok()
    }
}
