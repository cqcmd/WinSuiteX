//! 网卡采集（Win32 `GetAdaptersAddresses`）。
//!
//! 设计要点：
//! - 两次调用 `GetAdaptersAddresses`（首轮拿所需缓冲区大小，次轮取数据），
//!   并重试处理 `ERROR_BUFFER_OVERFLOW`（链表在两次调用之间可能变化导致尺寸不足）；
//! - 描述取 `Description`（UTF-16，SDK 已 NUL 结尾），MAC 取 `PhysicalAddress`
//!   的 `PhysicalAddressLength` 字节——**长度为 0 时 `mac = None`**，绝不用空串/0 充数；
//! - IPv4 取自 `FirstUnicastAddress` 链表里 `AF_INET` 的项（可能多个，全部收进 `Vec`），
//!   IPv6 取第一个 `AF_INET6`；跳过 `0.0.0.0` / `::` 这种未配置地址；
//! - `up`：以 `OperStatus == IfOperStatusUp` 表示「已启用/已连接」；
//! - 过滤：跳过 loopback（`IF_TYPE_SOFTWARE_LOOPBACK`）与无 MAC 的适配器
//!   （ISATAP / Teredo / 6to4 等隧道适配器通常 `PhysicalAddressLength == 0`）；
//! - 单适配器解析失败只跳过该项，不中断整体采集（最佳努力语义）。

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_DevNode_Status, CM_Locate_DevNodeW, CM_DEVNODE_STATUS_FLAGS, CM_LOCATE_DEVNODE_NORMAL,
    CM_PROB,
};
use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, GET_ADAPTERS_ADDRESSES_FLAGS, IF_TYPE_SOFTWARE_LOOPBACK,
    IP_ADAPTER_ADDRESSES_LH,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6, SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_SZ,
};

use winsuitex_core::error::{Error, Result};
use winsuitex_core::model::{Nic, NicKind};
use winsuitex_core::traits::{CollectContext, HardwareSource};

/// 虚拟网卡描述黑名单（大小写不敏感的「包含」匹配）。
///
/// 逐条对应 C++ `hardware.cpp` `IsVirtualNic()` 的黑名单；描述为空也视为虚拟
/// （语义见 `is_virtual_nic`）。无全局可变状态，纯静态常量。
const VIRTUAL_NIC_PATTERNS: &[&str] = &[
    "WAN Miniport",           // Windows 拨号/VPN 虚拟适配器
    "Microsoft 6to4",         //
    "Microsoft ISATAP",       //
    "Microsoft Kernel Debug", //
    "Microsoft Teredo",       //
    "Teredo Tunneling",       //
    "RAS Async",              //
    "Microsoft IP-HTTPS",     //
    "Microsoft Wi-Fi Direct", //
    "Bluetooth Device (Personal Area Network)",
    "Npcap",                       //
    "WinPcap",                     // 抓包驱动虚拟网卡
    "TAP-Windows",                 //
    "Software Loopback Interface", //
    "Microsoft Loopback Adapter",  //
    "Hyper-V Virtual Ethernet",    //
    "vEthernet",                   // WSL / Docker / Hyper-V Default Switch
    "VirtualBox",                  // 含 Host-Only / NAT / 桥接虚拟网卡
    "VMware Virtual Ethernet",     //
    "VMnet",                       //
    "Microsoft KM-TEST",           //
    // —— 二次核对补全：现代 VPN / 异地组网 / 抓包驱动 ——
    "Wintun",    // WireGuard 及现代 OpenVPN(Wintun 模式) 底层驱动
    "WireGuard", //
    "ZeroTier",  // 异地组网
    "Tailscale", // 异地组网
    "SoftEther", // 软以太网 VPN
];

/// 判断一个网卡描述是否为虚拟适配器。
///
/// - `None` 或空串 → `true`（与 C++ 老版本 `IsVirtualNic` 一致：描述为空也算虚拟）；
/// - 否则任一黑名单 pattern 命中（大小写不敏感「包含」匹配）→ `true`；
/// - 否则 → `false`（真实物理网卡不被误杀）。
fn is_virtual_nic(desc: Option<&str>) -> bool {
    let Some(desc) = desc else {
        return true;
    };
    if desc.is_empty() {
        return true;
    }
    let haystack = desc.to_ascii_lowercase();
    VIRTUAL_NIC_PATTERNS
        .iter()
        .any(|p| haystack.contains(&p.to_ascii_lowercase()))
}

/// 把 `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}` 形式的 ANSI 字符串解析为 `GUID`。
///
/// 适配器 `AdapterName` 即为此格式；解析失败（格式不对 / 非十六进制）返回 `None`。
/// 纯函数、无 FFI，便于单测。
#[cfg(test)]
use windows::core::GUID;

#[cfg(test)]
fn parse_guid(s: &str) -> Option<GUID> {
    let s = s.trim().trim_start_matches('{').trim_end_matches('}');
    let mut it = s.split('-');
    let d1: u32 = u32::from_str_radix(it.next()?, 16).ok()?;
    let d2: u16 = u16::from_str_radix(it.next()?, 16).ok()?;
    let d3: u16 = u16::from_str_radix(it.next()?, 16).ok()?;
    let d4hex = it.next()?; // 4 个十六进制位 => 2 字节
    let d5hex = it.next()?; // 12 个十六进制位 => 6 字节
    if d4hex.len() != 4 || d5hex.len() != 12 {
        return None;
    }
    let mut data4 = [0u8; 8];
    let mut i = 0usize;
    for pair in d4hex.as_bytes().chunks(2) {
        data4[i] = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
        i += 1;
    }
    for pair in d5hex.as_bytes().chunks(2) {
        data4[i] = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
        i += 1;
    }
    Some(GUID {
        data1: d1,
        data2: d2,
        data3: d3,
        data4,
    })
}

/// `DN_MADEUP`（0x00000200）标记「由系统凭空生成 / 无对应真实硬件」的设备节点，
/// 与 Windows 显示语言无关，可作为物理性的可靠信号。windows crate 未导出该常量，
/// 此处按 cfgmgr32 的权威定义补一份。
const DN_MADEUP: u32 = 0x0000_0200;

/// `CM_Get_DevNode_Status` 返回的状态字中 `DN_MADEUP` 位是否置位。纯函数、便于单测。
fn is_made_up(status: u32) -> bool {
    (status & DN_MADEUP) != 0
}

/// 通过 SetupAPI 查询该网卡对应的设备节点是否被标记为 `DN_MADEUP`
/// （"made-up" / 虚拟占位设备，通常对应非物理的软件或隧道网卡）。
///
/// 这是与描述语言无关的物理性判定：无论 Windows 显示语言如何，
/// 设备节点的 `DN_MADEUP` 状态都一致，可弥补纯英文描述黑名单在非英文系统上
/// 完全失效的问题（见二次核对记录 #3）。
///
/// 链路：`AdapterName`(GUID) → 设备接口符号链接 → 设备实例 ID → devnode → 状态位。
/// 任何一步失败（拿不到 GUID、查不到设备接口、无对应 devnode、权限不足等）
/// 一律返回 `false` 表示「未判定为 made-up」——调用方应回退到描述黑名单判定，
/// 绝不因查询失败而把虚拟网卡误判为物理网卡。
/// 由网卡适配器 GUID 读取其 PnP 设备实例 ID。
///
/// 注册表路径
/// `SYSTEM\CurrentControlSet\Control\Network\{4D36E972-...}\{AdapterGUID}\Connection`
/// 的 `PnpInstanceID` 值即为该网卡对应的设备实例 ID。该路径与值名均为固定英文，
/// 与 Windows 显示语言无关，是语言无关地把 `GetAdaptersAddresses` 适配器关联到
/// SetupAPI 设备节点的可靠桥梁。
///
/// 任何失败（无此键 / 无此值 / 类型不符 / 读出为空）均返回 `None`，调用方回退到黑名单判定。
fn read_pnp_instance_id(adapter_guid: &str) -> Option<String> {
    const NETWORK_CLASS_GUID: &str = "{4D36E972-E325-11CE-BFC1-08002BE10318}";
    let sub = format!(
        "SYSTEM\\CurrentControlSet\\Control\\Network\\{}\\{}\\Connection",
        NETWORK_CLASS_GUID, adapter_guid
    );
    let sub_w: Vec<u16> = sub.encode_utf16().chain(std::iter::once(0)).collect();

    let mut hkey: HKEY = HKEY::default();
    // SAFETY: sub_w 以 NUL 结尾；hkey 由 API 写入，失败时保持默认。
    let rc = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(sub_w.as_ptr()),
            Some(0),
            KEY_READ,
            &mut hkey,
        )
    };
    if rc.0 != 0 {
        return None;
    }

    let value_name: Vec<u16> = "PnpInstanceID"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut buf = [0u16; 256];
    let mut buf_len: u32 = (buf.len() * 2) as u32;
    let mut value_type = REG_SZ;
    // SAFETY: value_name 以 NUL 结尾；buf 容量 >= buf_len；hkey 有效。
    let qc = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(value_name.as_ptr()),
            None,
            Some(&mut value_type),
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut buf_len),
        )
    };
    // SAFETY: hkey 由 RegOpenKeyExW 成功获得，此处关闭。
    let _ = unsafe { RegCloseKey(hkey) };
    if qc.0 != 0 || value_type != REG_SZ {
        return None;
    }
    if buf_len == 0 {
        return None;
    }
    // buf_len 为含结尾 NUL 的字节数；转 String 时去掉末尾 NUL。
    let len = (buf_len / 2) as usize;
    let s = String::from_utf16_lossy(&buf[..len.saturating_sub(1)]);
    let s = s.trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// 通过 SetupAPI 查询该网卡对应的设备节点是否被标记为 `DN_MADEUP`
/// （"made-up" / 虚拟占位设备，通常对应非物理的软件或隧道网卡）。
///
/// 这是与描述语言无关的物理性判定：无论 Windows 显示语言如何，
/// 设备节点的 `DN_MADEUP` 状态都一致，可弥补纯英文描述黑名单在非英文系统上
/// 完全失效的问题（见二次核对记录 #3）。
///
/// 链路：`AdapterName`(GUID) → 注册表 `PnpInstanceID` → 设备实例 ID → devnode → 状态位。
/// 任何一步失败（拿不到 GUID、查不到实例 ID、无对应 devnode、权限不足等）
/// 一律返回 `false` 表示「未判定为 made-up」——调用方应回退到描述黑名单判定，
/// 绝不因查询失败而把虚拟网卡误判为物理网卡。
fn is_made_up_adapter(adapter: &IP_ADAPTER_ADDRESSES_LH) -> bool {
    // 1) AdapterName 是形如 "{GUID}" 的 ANSI 字符串，即注册表子路径段。
    // SAFETY: adapter.AdapterName 由系统填充，生命周期与 adapter 一致。
    let guid = unsafe {
        if adapter.AdapterName.is_null() {
            return false;
        }
        let cstr = std::ffi::CStr::from_ptr(adapter.AdapterName.0 as *const std::ffi::c_char);
        match cstr.to_str() {
            Ok(s) => s.to_string(),
            Err(_) => return false,
        }
    };

    // 2) 经由注册表 PnpInstanceID 取出设备实例 ID。
    let Some(instance_id) = read_pnp_instance_id(&guid) else {
        return false;
    };
    let id_w: Vec<u16> = instance_id
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    // 3) 定位 devnode 并查状态。
    let mut devinst: u32 = 0;
    // SAFETY: id_w 以 NUL 结尾；devinst 有效可写。
    let rc = unsafe {
        CM_Locate_DevNodeW(
            &mut devinst,
            PCWSTR(id_w.as_ptr()),
            CM_LOCATE_DEVNODE_NORMAL,
        )
    };
    if rc.0 != 0 {
        return false;
    }
    let mut status = CM_DEVNODE_STATUS_FLAGS(0);
    let mut problem = CM_PROB(0);
    // SAFETY: status / problem 有效可写；devinst 由 CM_Locate_DevNodeW 成功获得。
    let rc = unsafe { CM_Get_DevNode_Status(&mut status, &mut problem, devinst, 0) };
    if rc.0 != 0 {
        return false;
    }
    is_made_up(status.0)
}

/// 网卡数据源。
pub struct NicSource;

impl HardwareSource for NicSource {
    fn name(&self) -> &'static str {
        "nic"
    }

    fn collect(&self, ctx: &mut CollectContext) -> Result<()> {
        match collect_nics() {
            Ok(nics) => {
                ctx.set_nics(nics);
                Ok(())
            }
            Err(e) => Err(Error::collect("nic", e)),
        }
    }
}

/// 枚举本机所有「物理/以太网」网卡。
fn collect_nics() -> std::result::Result<Vec<Nic>, String> {
    // family = 0 (AF_UNSPEC)：同时枚举 IPv4 与 IPv6 单播地址。
    let flags = GET_ADAPTERS_ADDRESSES_FLAGS(0);

    // 初始给一块保守的缓冲区；若不够，API 会在 size 里回写所需字节数，
    // 我们据此扩容后重试（最多 3 次，防止无限循环）。
    let mut size: u32 = 1024;
    // 用 Vec<u64> 分配，保证缓冲区 8 字节对齐：IP_ADAPTER_ADDRESSES_LH 含指针/句柄，
    // 要求其对齐到指针宽度（x64 下 8 字节）。Vec<u8> 仅保证 1 字节对齐，
    // 若以 `&*cur` 直接读该结构会产生未定义行为（目前"碰巧"因堆块 16 字节对齐而正常）。
    let mut buf: Vec<u64> = vec![0u64; (size as usize).div_ceil(8)];
    let mut ret: u32 = 0;

    for _ in 0..3 {
        // SAFETY: `buf` 持有 `buf.len() * 8` 字节的有效可写内存，且按 u64 对齐
        // （满足 IP_ADAPTER_ADDRESSES_LH 的对齐要求）；`adapters` 指向其首部，
        // GetAdaptersAddresses 会在其中写入链表。链表节点的 Next 指针仍指向同一块
        // buf 内部；迭代期间 buf 不会被移动，且每次 resize 后都会在此重新取指针
        // （Vec 重新分配会使旧指针失效，绝不跨 resize 缓存指针）。
        // 传给 API 的字节长度按 `size`（API 以字节理解）维持不变。
        let adapters = buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
        ret = unsafe { GetAdaptersAddresses(0, flags, None, Some(adapters), &mut size) };
        if ret == NO_ERROR.0 {
            break;
        } else if ret == ERROR_BUFFER_OVERFLOW.0 {
            // API 已把 size 更新为所需字节数；扩容到该尺寸（至少翻倍以防抖动）。
            let need_bytes = size as usize;
            let need_words = need_bytes.div_ceil(8);
            if need_words > buf.len() {
                buf.resize(need_words, 0);
            } else {
                // API 未给出更大尺寸（抖动）：翻倍缓冲区，并把输入 size 同步为字节数。
                buf.resize(buf.len() * 2, 0);
                size = (buf.len() * 8) as u32;
            }
        } else {
            return Err(format!("GetAdaptersAddresses 失败，错误码 {ret}"));
        }
    }

    if ret != NO_ERROR.0 {
        return Err(format!("GetAdaptersAddresses 重试后仍失败，错误码 {ret}"));
    }

    // SAFETY: 此时 buf 中已是一份有效的 IP_ADAPTER_ADDRESSES_LH 链表；
    // 我们沿 Next 遍历，所有节点都位于 buf 内部，且 buf 在本函数返回前保持存活、不被移动。
    let mut nics = Vec::new();
    let mut raw_count = 0usize;
    let mut virtual_skipped = 0usize;
    let mut cur = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    while !cur.is_null() {
        let adapter = unsafe { &*cur };
        raw_count += 1;
        // 描述：取 Description（设备描述串，如 "Red Hat VirtIO Ethernet Adapter"）。
        // 先过虚拟网卡黑名单，命中则跳过（不计入结果）。
        let desc = unsafe { pwstr_to_string(adapter.Description) };
        // 语言无关的虚拟性判定：描述黑名单（英文）命中，或 SetupAPI 标记 DN_MADEUP
        // （非英文 Windows 上描述黑名单会整张失效，DN_MADEUP 仍能兜底过滤软件/隧道网卡）。
        let made_up = is_made_up_adapter(adapter);
        if is_virtual_nic(desc.as_deref()) || made_up {
            virtual_skipped += 1;
            cur = adapter.Next;
            continue;
        }
        if let Some(nic) = parse_adapter(adapter, desc) {
            nics.push(nic);
        }
        cur = adapter.Next;
    }
    eprintln!(
        "[nic] 适配器链表节点数(过滤前): {raw_count}, 被虚拟网卡过滤跳过: {virtual_skipped}, \
         采集到的网卡数(全部过滤后): {}",
        nics.len()
    );
    Ok(nics)
}

/// 链路速率规范化（纯函数、便于单测）。
///
/// Windows 用 `IF_MAX_LINK_SPEED`（= `(ULONG64)-1` = `u64::MAX`）表示「速率不可得」，
/// 且**未连接**（`up == false`）时本就没有协商速率可言。两种情形一律按未知（`None`）处理。
///
/// 绝不能把哨兵值当真实速率写进报告 —— 真机实测（2026-09-16 服务器）已踩过：
/// 未连接的 Intel I350 / Mellanox 口被 Windows 填成 `u64::MAX`，
/// 旧实现只判 `> 0` 导致报告里出现「18446744073.7 Gbps」这种鬼值。
fn link_speed(transmit_link_speed: u64, up: bool) -> Option<u64> {
    if !up || transmit_link_speed == 0 || transmit_link_speed == u64::MAX {
        None
    } else {
        Some(transmit_link_speed)
    }
}

/// 把一个适配器结构解析为 `Nic`；被过滤掉或解析失败时返回 `None`。
///
/// `desc` 为调用方预先取出的描述串（已做过虚拟网卡黑名单过滤）。
fn parse_adapter(adapter: &IP_ADAPTER_ADDRESSES_LH, desc: Option<String>) -> Option<Nic> {
    // 1) 跳过软件回环适配器。
    if adapter.IfType == IF_TYPE_SOFTWARE_LOOPBACK {
        return None;
    }

    // 2) 跳过没有 MAC 的适配器（隧道/虚拟适配器通常如此）。
    let mac = format_mac(&adapter.PhysicalAddress, adapter.PhysicalAddressLength)?;

    // 3) 单播地址链表：分别收集 IPv4（多个）与第一个 IPv6。
    let mut ipv4: Vec<String> = Vec::new();
    let mut ipv6: Option<String> = None;
    let mut ua = adapter.FirstUnicastAddress;
    while !ua.is_null() {
        // SAFETY: ua 来自链表，指向有效的 IP_ADAPTER_UNICAST_ADDRESS_LH。
        let u = unsafe { &*ua };
        let sa = u.Address.lpSockaddr;
        if !sa.is_null() {
            // SAFETY: sa 由系统填充，生命周期与 adapter 一致（位于我们持有的 buf 中）。
            let family = unsafe { (*sa).sa_family };
            if family == AF_INET {
                if let Some(s) = unsafe { ipv4_string(sa) } {
                    if s != "0.0.0.0" {
                        ipv4.push(s);
                    }
                }
            } else if family == AF_INET6 && ipv6.is_none() {
                if let Some(s) = unsafe { ipv6_string(sa) } {
                    if s != "::" {
                        ipv6 = Some(s);
                    }
                }
            }
        }
        ua = u.Next;
    }

    // 5) 是否已连接/启用。
    let up = adapter.OperStatus == IfOperStatusUp;

    // 6) 链路速率：经 `link_speed` 规范化（过滤哨兵 + 未连接情形）。
    let speed_bps = link_speed(adapter.TransmitLinkSpeed, up);

    Some(Nic {
        desc: desc.clone(),
        mac: Some(mac),
        ipv4,
        ipv6,
        up,
        speed_bps,
        kind: NicKind::Physical,
        vendor: desc
            .as_deref()
            .and_then(winsuitex_core::nic_vendor_from_desc),
    })
}

/// 把 `PhysicalAddress` 的前 `len` 字节格式化为 `XX-XX-...` 大写十六进制。
/// 长度为 0 或越界时返回 `None`。
fn format_mac(addr: &[u8; 8], len: u32) -> Option<String> {
    let len = len as usize;
    if len == 0 || len > addr.len() {
        return None;
    }
    let parts: Vec<String> = addr[..len].iter().map(|b| format!("{b:02X}")).collect();
    Some(parts.join("-"))
}

/// IPv4 单播地址 → 点分十进制串。
///
/// # SAFETY
/// `sa` 必须指向一个 `sa_family == AF_INET` 的 `SOCKADDR_IN`。
unsafe fn ipv4_string(sa: *mut SOCKADDR) -> Option<String> {
    let in4 = &*(sa as *const SOCKADDR_IN);
    let b = in4.sin_addr.S_un.S_un_b;
    Some(format!("{}.{}.{}.{}", b.s_b1, b.s_b2, b.s_b3, b.s_b4))
}

/// IPv6 单播地址 → 冒分十六进制串（8 组，每组 2 字节小写）。
///
/// # SAFETY
/// `sa` 必须指向一个 `sa_family == AF_INET6` 的 `SOCKADDR_IN6`。
unsafe fn ipv6_string(sa: *mut SOCKADDR) -> Option<String> {
    let in6 = &*(sa as *const SOCKADDR_IN6);
    let bytes = in6.sin6_addr.u.Byte;
    let groups: Vec<String> = bytes
        .chunks(2)
        .map(|c| format!("{:02x}{:02x}", c[0], c[1]))
        .collect();
    Some(groups.join(":"))
}

/// `PWSTR` → `Option<String>`（NUL 结尾、尾部截断、空串归为 `None`）。
///
/// # SAFETY
/// `p` 必须是一个有效的 NUL 结尾 UTF-16 指针，且在本函数调用期间保持有效。
unsafe fn pwstr_to_string(p: PWSTR) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let wide = unsafe { p.as_wide() };
    crate::wide_to_string(wide)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 校验 MAC 形如 `XX-XX-XX-XX-XX-XX`（大写十六进制、`-` 分隔）。
    fn mac_format_ok(m: &str) -> bool {
        let parts: Vec<&str> = m.split('-').collect();
        if parts.len() != 6 {
            return false;
        }
        parts.iter().all(|p| {
            p.len() == 2
                && p.bytes()
                    .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
        })
    }

    #[test]
    fn is_virtual_nic_real_wifi_direct_is_filtered() {
        // 真实机器上实际遇到的那条噪声条目。
        assert!(is_virtual_nic(Some(
            "Microsoft Wi-Fi Direct Virtual Adapter"
        )));
    }

    #[test]
    fn is_virtual_nic_real_wired_not_filtered() {
        // 真实有线网卡不能被误杀。
        assert!(!is_virtual_nic(Some("Realtek PCIe GBE Family Controller")));
    }

    #[test]
    fn is_virtual_nic_real_wireless_not_filtered() {
        // 真实无线网卡不能被误杀。
        assert!(!is_virtual_nic(Some(
            "Realtek 8188GU Wireless LAN 802.11n USB NIC"
        )));
    }

    #[test]
    fn is_virtual_nic_none_is_virtual() {
        assert!(is_virtual_nic(None));
    }

    #[test]
    fn is_virtual_nic_empty_is_virtual() {
        assert!(is_virtual_nic(Some("")));
    }

    #[test]
    fn is_virtual_nic_case_insensitive() {
        assert!(is_virtual_nic(Some("microsoft wi-fi direct")));
    }

    #[test]
    fn is_virtual_nic_modern_overlay_filtered() {
        // 二次核对补全项：WireGuard / ZeroTier / Tailscale / SoftEther / Wintun 必须被过滤
        // （网吧/电竞酒店常用异地组网与 VPN，漏掉会污染"物理网卡"列表）。
        assert!(is_virtual_nic(Some("WireGuard Tunnel")));
        assert!(is_virtual_nic(Some("ZeroTier Virtual Port")));
        assert!(is_virtual_nic(Some("Tailscale Tunnel")));
        assert!(is_virtual_nic(Some("SoftEther VPN Adapter")));
        assert!(is_virtual_nic(Some("Wintun")));
    }

    #[test]
    fn is_virtual_nic_virtualbox_broadened() {
        // VirtualBox 的 NAT / 桥接虚拟网卡（不只 Host-Only）也应被过滤。
        assert!(is_virtual_nic(Some(
            "VirtualBox Host-Only Ethernet Adapter"
        )));
        assert!(is_virtual_nic(Some("VirtualBox NAT Ethernet Adapter")));
    }

    #[test]
    fn collect_returns_ok_and_matches_benchmark() {
        let src = NicSource;
        let mut ctx = CollectContext::new();
        let r = src.collect(&mut ctx);
        assert!(r.is_ok(), "collect() 返回 Err: {:?}", r.err());

        let (info, _volumes) = ctx.into_parts();
        let nics = info.nics;
        assert!(!nics.is_empty(), "本机至少应采集到 1 个网卡");

        let mut found_valid_mac = false;
        for n in &nics {
            if let Some(mac) = &n.mac {
                assert!(mac_format_ok(mac), "MAC 格式不符: {mac}");
                found_valid_mac = true;
            }
            eprintln!(
                "NIC desc={:?} mac={:?} ipv4={:?} ipv6={:?} up={}",
                n.desc, n.mac, n.ipv4, n.ipv6, n.up
            );
        }
        assert!(found_valid_mac, "至少应有一个 MAC 格式正确的网卡");
    }

    #[test]
    fn link_speed_unknown_sentinel_is_none() {
        // Windows 的 IF_MAX_LINK_SPEED 哨兵（u64::MAX）绝不能当真实速率。
        // 2026-09-16 真机教训：未连接网口被填成 u64::MAX，旧实现判 >0 就采信，
        // 报告里出现「18446744073.7 Gbps」。
        assert_eq!(link_speed(u64::MAX, true), None);
        assert_eq!(link_speed(u64::MAX, false), None);
    }

    #[test]
    fn link_speed_zero_is_none() {
        assert_eq!(link_speed(0, true), None);
    }

    #[test]
    fn link_speed_down_adapter_is_none() {
        // 未连接时没有协商速率，即便驱动给了数值也不采信。
        assert_eq!(link_speed(1_000_000_000, false), None);
    }

    #[test]
    fn link_speed_real_value_when_up() {
        assert_eq!(link_speed(1_000_000_000, true), Some(1_000_000_000));
        // 真机实测：Mellanox ConnectX-3 协商到 40 Gbps
        assert_eq!(link_speed(40_000_000_000, true), Some(40_000_000_000));
    }

    #[test]
    fn is_made_up_detects_dn_madeup_bit() {
        // DN_MADEUP = 0x00000200（bit 9）。
        assert!(is_made_up(0x0000_0200));
        assert!(is_made_up(0x0000_0201)); // 同时置位其它位
        assert!(!is_made_up(0x0000_0000));
        assert!(!is_made_up(0x0000_0100)); // DN_HAS_PROBLEM，非 made-up
    }

    #[test]
    fn parse_guid_handles_braced_and_bare() {
        let g1 = parse_guid("{CAC88484-7515-4C03-82E6-71A87ABAC361}").unwrap();
        // 与 GUID_DEVINTERFACE_NET 同值：data1=0xCAC88484, data4[0]=0x82, data4[7]=0x61
        assert_eq!(g1.data1, 0xCAC8_8484);
        assert_eq!(g1.data4[0], 0x82);
        assert_eq!(g1.data4[7], 0x61);

        let g2 = parse_guid("CAC88484-7515-4C03-82E6-71A87ABAC361").unwrap();
        assert_eq!(g1, g2);

        assert!(parse_guid("not-a-guid").is_none());
        assert!(parse_guid("{bad").is_none());
        assert!(parse_guid("").is_none());
    }

    #[test]
    fn nic_vendor_from_desc_picks_known_vendor() {
        assert_eq!(
            winsuitex_core::nic_vendor_from_desc("Realtek PCIe GBE Family Controller").as_deref(),
            Some("Realtek")
        );
        assert_eq!(
            winsuitex_core::nic_vendor_from_desc("Intel(R) Ethernet Connection Adapter").as_deref(),
            Some("Intel")
        );
        // 厂商词必须位于词首且后接空格/结尾，避免误命中（如 "Realtekx"）
        assert_eq!(
            winsuitex_core::nic_vendor_from_desc("SomeRealtekClone"),
            None
        );
        assert_eq!(
            winsuitex_core::nic_vendor_from_desc("Unknown Network Adapter"),
            None
        );
    }
}
