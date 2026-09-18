//! EDID 解析：从注册表读取显示器 EDID 二进制，补全物理尺寸 / 序列号 / 生产日期。
//!
//! 设计要点：
//! - EDID 是显示器的"身份证"，含厂商、型号、序列号、生产周年、物理尺寸（在详细时序
//!   描述符里）。这些字段 GDI 不暴露，只能解析 EDID。
//! - EDID 缓存在注册表
//!   `HKLM\SYSTEM\CurrentControlSet\Enum\MONITOR\<pnp>\<instance>\Device Parameters\EDID`。
//! - 解析失败（无 EDID / 字段为 0 / 字节越界）一律返回 `None`，不伪造、不 panic。
//! - EDID 是增强信息，缺了不影响主链路（分辨率已能拿到），因此只做"有则补"，
//!   不会让采集因 EDID 缺失而失败。

use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_BINARY,
};

/// EDID 解析结果（仅含我们关心的增强字段）。
#[derive(Debug, Default)]
pub(crate) struct EdidInfo {
    pub serial: Option<String>,
    pub manufacture_year: Option<u16>,
    pub manufacture_week: Option<u8>,
    pub size_inch: Option<String>,
}

/// 把监视器 `DeviceID`（`MONITOR\PNP\INSTANCE` 等）转成注册表路径读取 EDID 二进制。
///
/// 返回原始 EDID 字节（base block，通常 128 字节）；读不到返回 `None`。
pub(crate) fn read_edid_bytes(device_id: &str) -> Option<Vec<u8>> {
    // 去掉可能的前导 `\\?\`（少数环境 DeviceID 带这个前缀）。
    let id = device_id.strip_prefix(r"\\?\").unwrap_or(device_id);
    // 注册表路径：SYSTEM\CurrentControlSet\Enum\<DeviceID>\Device Parameters
    let sub_key = format!(r"SYSTEM\CurrentControlSet\Enum\{id}\Device Parameters");
    let sub_key_w: Vec<u16> = sub_key.encode_utf16().chain(std::iter::once(0)).collect();

    let mut hkey: HKEY = HKEY::default();
    // SAFETY: sub_key_w 以 NUL 结尾；hkey 由 API 写入，失败时保持默认。
    let rc = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(sub_key_w.as_ptr()),
            Some(0),
            KEY_READ,
            &mut hkey,
        )
    };
    if rc.0 != 0 {
        return None;
    }

    // 读取 EDID 值（REG_BINARY）。
    let value_name: Vec<u16> = "EDID".encode_utf16().chain(std::iter::once(0)).collect();
    let mut buf = [0u8; 256];
    let mut buf_size: u32 = buf.len() as u32;
    let mut value_type = REG_BINARY;
    // SAFETY: value_name 以 NUL 结尾；buf 容量 >= buf_size；hkey 有效。
    let qc = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(value_name.as_ptr()),
            None,
            Some(&mut value_type),
            Some(buf.as_mut_ptr()),
            Some(&mut buf_size),
        )
    };
    // SAFETY: hkey 由 RegOpenKeyExW 成功获得，此处关闭。
    let _ = unsafe { RegCloseKey(hkey) };
    if qc.0 != 0 || value_type != REG_BINARY {
        return None;
    }
    if buf_size == 0 {
        return None;
    }
    Some(buf[..buf_size as usize].to_vec())
}

/// EDID 头校验：标准 EDID 以 `00 FF FF FF FF FF FF 00` 开头。
fn is_valid_edid(edid: &[u8]) -> bool {
    const HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];
    edid.len() >= 128 && edid[..8] == HEADER
}

/// 从描述符的 ASCII 区（最多 13 字节）解出可打印字符串，遇 NUL/换行截断。
fn descriptor_ascii(bytes: &[u8]) -> String {
    let mut s = String::new();
    for &b in bytes.iter().take(13) {
        if b == 0x00 || b == 0x0A {
            break;
        }
        if (0x20..=0x7E).contains(&b) {
            s.push(b as char);
        }
    }
    s.trim().to_string()
}

/// 解析 EDID base block，提取序列号 / 生产周年 / 物理尺寸（英寸）。
pub(crate) fn parse_edid(edid: &[u8]) -> EdidInfo {
    let mut info = EdidInfo::default();
    if !is_valid_edid(edid) {
        return info;
    }

    // 生产周年：byte16 = 周（0=未标），byte17 = 年份偏移（1990+）。
    let week = edid[16];
    let year_off = edid[17];
    if year_off != 0 && year_off != 0xFF {
        let year = 1990u16 + year_off as u16;
        if (1990..=2255).contains(&year) {
            info.manufacture_year = Some(year);
            if (1..=53).contains(&week) {
                info.manufacture_week = Some(week);
            }
        }
    }

    // 序列号（32-bit，byte12-15，规范规定 LSB first 即小端）。
    // 0xFFFFFFFF 是未编程/不存在，与 0 一并排除。
    // 描述符 0xFF 的 ASCII 序列号会覆盖它（更可读）。
    let serial32 = u32::from_le_bytes([edid[12], edid[13], edid[14], edid[15]]);
    if serial32 != 0 && serial32 != 0xFFFF_FFFF {
        info.serial = Some(serial32.to_string());
    }

    // 遍历 4 个描述符块（每块 18 字节，起始于 54/72/90/108）。
    // - 详细时序描述符（像素时钟非 0）→ 物理尺寸（mm）。
    // - 标签 0xFF → 序列号 ASCII（13 字节，从 +5 起）。
    for base in [54u32, 72, 90, 108] {
        let b = base as usize;
        if b + 18 > edid.len() {
            continue;
        }
        let pixel_clock = u16::from_le_bytes([edid[b], edid[b + 1]]);
        if pixel_clock == 0 {
            // Monitor Descriptor；第 4 字节（索引 b+3）为子类型标签。
            if edid[b + 3] == 0xFF {
                let s = descriptor_ascii(&edid[b + 5..b + 18]);
                if !s.is_empty() {
                    info.serial = Some(s);
                }
            }
        } else {
            // 详细时序：物理尺寸（mm）。按 EDID 1.3/1.4 的 18 字节详细描述符布局：
            //   b+12 = H image size 低 8 位
            //   b+13 = V image size 低 8 位
            //   b+14 = bit7:4 H image size 高 4 位 / bit3:0 V image size 高 4 位
            // 注意 b+11 是 V/H 同步信号的 offset/pulse width 高位，与尺寸无关。
            // 曾误用 b+11 取高位，等于拿同步时序拼尺寸：常见 0x2x 时会算出
            // 约 530×270mm → 23"，落在合理区间里输出一个很像真的错值。
            let h_mm = (((edid[b + 14] >> 4) & 0x0F) as u32) << 8 | edid[b + 12] as u32;
            let v_mm = ((edid[b + 14] & 0x0F) as u32) << 8 | edid[b + 13] as u32;
            if h_mm > 0 && v_mm > 0 {
                let diag_mm = ((h_mm * h_mm + v_mm * v_mm) as f64).sqrt();
                let inches = (diag_mm / 25.4).round() as u32;
                if (10..=120).contains(&inches) {
                    info.size_inch = Some(format!("{}\"", inches));
                }
            }
        }
    }

    info
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一块合法的 24" 1920×1080 显示器 EDID（合成，用于验证解析逻辑）。
    fn fake_edid_24in() -> Vec<u8> {
        let mut e = vec![0u8; 128];
        // 头
        e[0..8].copy_from_slice(&[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]);
        // 32-bit 序列号（小端，规范规定 LSB first）= 4660
        e[12..16].copy_from_slice(&0x0000_1234u32.to_le_bytes());
        // 生产周 12 / 年偏移 33（→ 2023）
        e[16] = 0x0C;
        e[17] = 0x21;
        // EDID 版本 1.4
        e[18] = 0x01;
        e[19] = 0x04;
        // 描述符 #1（base 54）：详细时序，24" 1920×1080 活动区 ≈ 531×299 mm。
        // 像素时钟非 0（LE 0x0100）→ 详细时序。
        e[54] = 0x00;
        e[55] = 0x01;
        // b+12(=66) / b+13(=67) = 宽高 mm 低 8 位；
        // b+14(=68) 高半字节 H 高 4 位(531>>8=2)，低半字节 V 高 4 位(299>>8=1) → 0x21
        e[66] = 531u32 as u8; // 0x13
        e[67] = 299u32 as u8; // 0x2B
        e[68] = 0x21;
        // 描述符 #2（base 72）：序列号 ASCII（标签 0xFF）。
        e[72] = 0x00;
        e[73] = 0x00;
        e[75] = 0xFF;
        let sn = b"SN1234567\0\0\0\0\0";
        e[77..77 + sn.len()].copy_from_slice(sn);
        e
    }

    #[test]
    fn parse_edid_extracts_serial_year_size() {
        let e = fake_edid_24in();
        let info = parse_edid(&e);
        // 32-bit 序列号被描述符 ASCII 覆盖。
        assert_eq!(info.serial.as_deref(), Some("SN1234567"));
        assert_eq!(info.manufacture_year, Some(2023));
        assert_eq!(info.manufacture_week, Some(12));
        // 531×299 mm → 对角线 ≈ 24"。
        assert_eq!(info.size_inch.as_deref(), Some("24\""));
    }

    #[test]
    fn edid_size_high_nibble_comes_from_b14_not_b11() {
        // 防倒退：物理尺寸的高 4 位在 b+14，不是 b+11。
        // b+11 是 V/H 同步信号的 offset/pulse width 高位，与尺寸无关。
        // 这里故意让 b+11 = 0x21（旧实现会据此拼出 531×299mm → "24\"" 的假尺寸），
        // 而真正的尺寸字节 b+12/13/14 全为 0（厂商未标尺寸）→ 必须判为 None。
        // 若哪天有人把 b+14 改回 b+11，这条会立刻红。
        let mut e = fake_edid_24in();
        e[66] = 0x00; // b+12 H 低 8 位
        e[67] = 0x00; // b+13 V 低 8 位
        e[68] = 0x00; // b+14 高 nibble
        e[65] = 0x21; // b+11：同步位，不该被当成尺寸高位
        let info = parse_edid(&e);
        assert_eq!(
            info.size_inch, None,
            "厂商未标尺寸时必须为 None，不能拿 b+11 的同步位拼出假尺寸"
        );
    }

    #[test]
    fn parse_edid_serial_is_little_endian() {
        // 序列号 32-bit 是 LSB first。0x00001234 小端落盘为 [34,12,00,00]，
        // 大端实现会把它读成 0x34120000 = 872939520 —— 这条钉死字节序。
        let mut e = fake_edid_24in();
        // 去掉 ASCII 序列号描述符，让 32-bit 序列号生效。
        e[75] = 0x00;
        let info = parse_edid(&e);
        assert_eq!(info.serial.as_deref(), Some("4660"));
    }

    #[test]
    fn parse_edid_rejects_garbage() {
        let info = parse_edid(&[0u8; 128]);
        assert!(info.serial.is_none());
        assert!(info.manufacture_year.is_none());
        assert!(info.size_inch.is_none());
    }

    #[test]
    fn parse_edid_short_buffer_is_safe() {
        // 比 base block 短的切片不能 panic。
        let info = parse_edid(&[0xFFu8; 10]);
        assert!(info.serial.is_none());
    }
}
