//! 物理硬盘采集：`\\.\PhysicalDriveN` + IOCTL。
//!
//! 设计要点（与 C++ 版 `CollectDisk` 对齐，但用 Rust 的安全抽象重写）：
//! - 枚举用 SetupAPI（`GUID_DEVINTERFACE_DISK`）拿到 `\\.\PhysicalDriveN` 设备路径，
//!   再 `CreateFileW` 打开做 IOCTL；SetupAPI 拿不到时退回直接遍历 `PhysicalDrive0..N`。
//! - 型号/总线类型：`IOCTL_STORAGE_QUERY_PROPERTY` + `StorageDeviceProperty`。
//! - 容量：`IOCTL_DISK_GET_LENGTH_INFO`（最稳，无需管理员）。
//! - 介质（SSD/HDD）：`IOCTL_STORAGE_QUERY_PROPERTY` + `StorageDeviceSeekPenaltyProperty`；
//!   拿不到就 `None`，**不猜**（仅 NVMe 天然 SSD，可作为兜底）。
//! - 盘符 → PhysicalDrive 映射：`IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS`。
//!
//! **最佳努力语义**：本工具常在非提权下运行，`CreateFileW` 打开失败必须降级为 `None`
//! 而非返回 `Err` 让整个采集失败（单盘失败不影响其它盘）。所有 `unsafe` 收敛于本文件。

use std::collections::HashMap;
use std::mem::size_of;
use std::sync::OnceLock;

use windows::core::{IUnknown, Interface, BSTR, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
    SetupDiGetDeviceInterfaceDetailW, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, HDEVINFO,
    SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE,
    IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, OPEN_EXISTING, STORAGE_BUS_TYPE,
};
use windows::Win32::Storage::IscsiDisc::{
    ATA_FLAGS_DATA_IN, ATA_PASS_THROUGH_EX, IOCTL_ATA_PASS_THROUGH,
};
use windows::Win32::System::Ioctl::{
    NVMeDataTypeLogPage, PropertyStandardQuery, ProtocolTypeNvme, StorageDeviceProperty,
    StorageDeviceProtocolSpecificProperty, StorageDeviceSeekPenaltyProperty, DISK_EXTENT,
    GET_LENGTH_INFORMATION, GUID_DEVINTERFACE_DISK, IOCTL_DISK_GET_DRIVE_LAYOUT_EX,
    IOCTL_DISK_GET_LENGTH_INFO, IOCTL_STORAGE_PROTOCOL_COMMAND, IOCTL_STORAGE_QUERY_PROPERTY,
    STORAGE_DEVICE_DESCRIPTOR, STORAGE_PROPERTY_QUERY,
    STORAGE_PROTOCOL_COMMAND_FLAG_ADAPTER_REQUEST, STORAGE_PROTOCOL_COMMAND_LENGTH_NVME,
    STORAGE_PROTOCOL_DATA_DESCRIPTOR, STORAGE_PROTOCOL_SPECIFIC_DATA,
    STORAGE_PROTOCOL_SPECIFIC_NVME_ADMIN_COMMAND, STORAGE_PROTOCOL_STATUS_SUCCESS,
    STORAGE_PROTOCOL_STRUCTURE_VERSION, VOLUME_DISK_EXTENTS,
};
use windows::Win32::System::IO::DeviceIoControl;

// WMI 第三兜底所需：COM 初始化 + WMI 接口 + VARIANT 取值。
// 注意：这些全部走 `windows` crate 原生 COM 绑定，单 exe、零运行时依赖，
// 不引 PowerShell / .NET 子进程（符合本工具「单 exe 零依赖」的项目哲学）。
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoInitializeSecurity, CoSetProxyBlanket, CoTaskMemFree,
    CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, EOAC_NONE,
    RPC_C_AUTHN_LEVEL_PKT_PRIVACY, RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Variant::{
    VariantToStringAlloc, VariantToUInt16, VariantToUInt64, VARIANT,
};
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemContext, IWbemLocator, IWbemServices, WbemLocator,
    WBEM_FLAG_FORWARD_ONLY, WBEM_INFINITE,
};

use crate::{to_wide, wide_to_string};
use winsuitex_core::error::Result;
use winsuitex_core::model::{DiskSmart, HealthStatus, PhysicalDisk, SmartSource, Volume};
use winsuitex_core::traits::{CollectContext, HardwareSource};

// ============================================================
// 句柄 RAII：所有文件/设备句柄的打开与关闭都走这里，避免泄漏
// ============================================================

/// `CreateFileW` 返回的句柄的 RAII 包装，Drop 时 `CloseHandle`。
struct HandleGuard(HANDLE);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        // SAFETY: 内部句柄只能由本模块 `CreateFileW` 成功返回，且本 guard 独占所有权，
        // 不会重复关闭。
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// SetupAPI 设备信息集的 RAII 包装，Drop 时 `SetupDiDestroyDeviceInfoList`。
struct DevInfoGuard(HDEVINFO);

impl Drop for DevInfoGuard {
    fn drop(&mut self) {
        // SAFETY: 句柄由 `SetupDiGetClassDevsW` 成功返回，且本 guard 独占所有权。
        unsafe {
            let _ = SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}

// ============================================================
// IOCTL 辅助
// ============================================================

/// 以只读/0 权限打开设备路径，返回 RAII 句柄。打开失败（权限不足/设备忙）返回 `None`，
/// 由调用方降级——这是本模块「最佳努力」的核心。
fn open_device(path: &str, desired_access: u32) -> Option<HandleGuard> {
    let wide = to_wide(path);
    // SAFETY: wide 在本调用期间有效；安全属性与模板句柄均为 None。
    let h = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            desired_access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    };
    h.ok().map(HandleGuard)
}

/// 取型号/总线类型/序列号（`STORAGE_DEVICE_DESCRIPTOR`）。
///
/// 返回 `(model, serial, bus)`；任何一项拿不到就对应为 `None`/`空`，**不猜**。
fn query_device_descriptor(
    handle: HANDLE,
) -> Option<(String, Option<String>, Option<String>, STORAGE_BUS_TYPE)> {
    // STORAGE_PROPERTY_QUERY 是纯 POD 结构（只有 PropertyId/QueryType 两个字段，无 HSTRING），
    // 可直接用 struct 字面量 + 默认填充初始化，消除 clippy field_reassign_with_default。
    // 不需要 mut：DeviceIoControl 通过指针接收，不会原地修改 q。
    let q = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceProperty,
        QueryType: PropertyStandardQuery,
        ..Default::default()
    };

    // 描述符尾部是变长 ANSI 字符串，缓冲区要大于结构体本身。4096 足够常规磁盘。
    let mut buf = vec![0u8; 4096];
    let mut ret = 0u32;
    // SAFETY: q 与本作用域内 buf 有效；buf 大小 4096 足够常规磁盘描述符。
    let ok = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(&q as *const _ as *const core::ffi::c_void),
            size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
            buf.len() as u32,
            Some(&mut ret),
            None,
        )
    };
    if ok.is_err() || ret == 0 {
        return None;
    }

    // SAFETY: 前 sizeof(STORAGE_DEVICE_DESCRIPTOR) 字节即描述符头；ret 为实际返回字节数。
    let desc = unsafe { &*(buf.as_ptr() as *const STORAGE_DEVICE_DESCRIPTOR) };
    let vendor = read_ansi_at(&buf, desc.VendorIdOffset, ret);
    let product = read_ansi_at(&buf, desc.ProductIdOffset, ret);
    let serial =
        read_ansi_at(&buf, desc.SerialNumberOffset, ret).and_then(|s| sanitize_disk_serial(&s));
    // 固件版本（ProductRevisionOffset）：ATA / NVMe 经 STORAGE_DEVICE_DESCRIPTOR 均可取，
    // 与型号 + 序列号共同构成盘的唯一身份，用于 RMA / 批次隐患比对。
    let firmware = read_ansi_at(&buf, desc.ProductRevisionOffset, ret);

    let mut model = String::new();
    if let Some(v) = &vendor {
        model.push_str(v);
    }
    if let Some(p) = &product {
        if !model.is_empty() {
            model.push(' ');
        }
        model.push_str(p);
    }
    let model = model.trim().to_string();
    Some((model, serial, firmware, desc.BusType))
}

/// 取容量（字节）。失败返回 `None`。
fn query_length(handle: HANDLE) -> Option<u64> {
    let mut gli = GET_LENGTH_INFORMATION { Length: 0 };
    let mut ret = 0u32;
    // SAFETY: gli 在本作用域有效。
    let ok = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_DISK_GET_LENGTH_INFO,
            None,
            0,
            Some(&mut gli as *mut _ as *mut core::ffi::c_void),
            size_of::<GET_LENGTH_INFORMATION>() as u32,
            Some(&mut ret),
            None,
        )
    };
    if ok.is_err() {
        return None;
    }
    Some(gli.Length as u64)
}

/// 取分区表类型（GPT / MBR / RAW）。失败（非提权/IOCTL 被拒）返回 `None`。
///
/// `IOCTL_DISK_GET_DRIVE_LAYOUT_EX` 的输出缓冲头部第一个字段就是
/// `DRIVE_LAYOUT_INFORMATION_EX.PartitionStyle`（一个 `PARTITION_STYLE` 枚举：
/// MBR=0 / GPT=1 / RAW=2）。我们只需这一个 DWORD，故分配一块足够大的缓冲，
/// 直接读前 4 字节小端即可，无需定义可变长结构体——比拼全结构体布局更稳。
fn query_partition_style(handle: HANDLE) -> Option<String> {
    let mut buf: [u8; 4096] = [0u8; 4096];
    let mut ret = 0u32;
    // SAFETY: buf 在本作用域有效，4096 字节足以容纳头部 + 若干分区项。
    let ok = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_DISK_GET_DRIVE_LAYOUT_EX,
            None,
            0,
            Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
            buf.len() as u32,
            Some(&mut ret),
            None,
        )
    };
    if ok.is_err() {
        return None;
    }
    // 头部第一个字段 PartitionStyle（u32 小端）。
    let style = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    match style {
        0 => Some("MBR".to_string()),
        1 => Some("GPT".to_string()),
        2 => Some("RAW".to_string()),
        _ => None,
    }
}

/// 自定义 seek-penalty 描述符：避免不同 SDK 下该结构体定义差异（与 C++ 同思路）。
#[repr(C)]
struct SeekPenalty {
    version: u32,
    size: u32,
    incurs: u8, // Windows BOOLEAN（0 = false）
}

/// 取介质类型：无寻道惩罚 = SSD。失败（驱动不支持/非提权）返回 `None`。
fn query_seek_penalty(handle: HANDLE) -> Option<bool> {
    // 同 query_device_descriptor：纯 POD 结构，直接 struct 字面量 + 默认填充。
    // 不需要 mut：DeviceIoControl 通过指针接收，不会原地修改 q。
    let q = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceSeekPenaltyProperty,
        QueryType: PropertyStandardQuery,
        ..Default::default()
    };

    let mut sp = SeekPenalty {
        version: 0,
        // 部分驱动/存储栈会校验 Size 字段再填充。
        size: size_of::<SeekPenalty>() as u32,
        incurs: 0,
    };
    let mut ret = 0u32;
    // SAFETY: q 与 sp 在本作用域有效；Size 字段已预置。
    let ok = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(&q as *const _ as *const core::ffi::c_void),
            size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            Some(&mut sp as *mut _ as *mut core::ffi::c_void),
            size_of::<SeekPenalty>() as u32,
            Some(&mut ret),
            None,
        )
    };
    if ok.is_err() || ret < size_of::<SeekPenalty>() as u32 {
        return None;
    }
    Some(sp.incurs != 0)
}

/// 总线类型 → 可读字符串。未知值返回 `None`（不猜）。
///
/// 数值对照 windows 0.62 的 `STORAGE_BUS_TYPE` 常量。
fn bus_name(bus: STORAGE_BUS_TYPE) -> Option<&'static str> {
    Some(match bus.0 {
        17 => "NVMe",
        10 => "SAS",
        11 => "SATA",
        7 => "USB",
        2 => "ATA",
        1 => "SCSI",
        8 => "RAID",
        9 => "iSCSI",
        13 => "MMC",
        12 => "SD",
        14 => "虚拟",
        16 => "存储空间",
        19 => "UFS",
        4 => "1394",
        5 => "SSA",
        3 => "ATA",
        15 => "虚拟文件",
        18 => "SCM",
        6 => "Fibre",
        _ => return None,
    })
}

/// 从缓冲区 `offset` 处读取 NUL 结尾的 ANSI 字符串（描述符中的厂商/型号/序列号字段）。
///
/// - `offset == 0` 表示该字段不存在；
/// - 越界则不读（防止触到 buffer 末尾）。
///
/// 仅做有界切片/UTF-8 解析，无 `unsafe`。
fn read_ansi_at(buf: &[u8], offset: u32, valid_len: u32) -> Option<String> {
    if offset == 0 || offset as usize >= valid_len as usize {
        return None;
    }
    let start = offset as usize;
    let end = buf[start..]
        .iter()
        .position(|&b| b == 0)
        .map(|p| start + p)
        .unwrap_or(buf.len());
    let s = String::from_utf8_lossy(&buf[start..end]).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// 盘序列号清洗：过滤掉「看起来有值、实则不是序列号」的返回。
///
/// 实测依据（SV-20260522YCII，Intel S2600WT2 + 4× NVMe）：
/// `STORAGE_DEVICE_DESCRIPTOR.SerialNumberOffset` 在部分 NVMe 存储栈上取到的不是序列号，
/// 而是 16 字节二进制标识的 hex dump，形如：
/// `3633_5330_5240_1792_0025_3845_0000_0001.`
/// 判伪铁证：三星两块盘与第四块盘**共享同一后缀** `…0025_3845_0000_000X`，
/// 真序列号不可能跨厂商/跨盘复用后缀，故必为存储栈合成标识。
///
/// 这种值直接进报告会误导 RMA 与资产核对，按项目「拿不到就 None，绝不显示假值」原则判为未获取。
///
/// 判定为无效的任一条件：
/// - 含非可打印 ASCII（控制字符）
/// - 长度 > 32（真实盘序列号通常 ≤ 24，dump 串为 33）
/// - 由十六进制字符 + `_`/`.` 分隔构成（真实盘序列号不含下划线）
fn sanitize_disk_serial(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    // 含控制字符 / 非可打印 ASCII → 无效
    if s.bytes().any(|b| !(0x20..=0x7e).contains(&b)) {
        return None;
    }
    // 过长 → 无效
    if s.len() > 32 {
        return None;
    }
    // hex dump 特征：含 `_` 且非十六进制部分只剩 `_` / `.`
    if s.contains('_')
        && s.chars()
            .all(|c| c.is_ascii_hexdigit() || c == '_' || c == '.')
    {
        return None;
    }
    // 收尾：VMD 等存储栈的 `AdapterSerialNumber` 会在真实序列号后追加空格填充 + 控制器后缀
    // （实测 `S63SNE0R401792      _0006`）。真实盘序列号从不含空格，取首个空白分隔段即得盘体打印序列号，
    // 丢弃尾部空格与 `_XXXX` 控制器后缀噪声。无空格的序列号（SATA / 直连 NVMe）原样保留。
    let cleaned: String = s.split_whitespace().next().unwrap_or(s).to_string();
    Some(cleaned)
}

/// 从缓冲区 `off`（字节偏移）处读取 NUL 结尾的 UTF-16 字符串，越界即停。
fn read_wide_in_buf(buf: &[u8], off: usize) -> Option<String> {
    if off + 1 >= buf.len() {
        return None;
    }
    // SAFETY: off 已在 buf 范围内；后续按 max_units 边界读取，不会越界。
    let base = unsafe { buf.as_ptr().add(off) as *const u16 };
    let max_units = (buf.len() - off) / 2;
    let mut len = 0usize;
    while len < max_units {
        // SAFETY: len < max_units，读取不越界。
        let ch = unsafe { *base.add(len) };
        if ch == 0 {
            break;
        }
        len += 1;
    }
    if len == 0 {
        return None;
    }
    // SAFETY: base..base+len 是 buf 内合法 UTF-16 区间。
    let slice = unsafe { std::slice::from_raw_parts(base, len) };
    wide_to_string(slice)
}

// ============================================================
// 设备枚举
// ============================================================

/// 通过 SetupAPI 枚举磁盘设备接口，返回 `\\.\PhysicalDriveN` 路径列表。
fn enumerate_disk_paths() -> Option<Vec<String>> {
    // SAFETY: GUID_DEVINTERFACE_DISK 是链接期常量；flags 组合合法。
    let dev_info = unsafe {
        SetupDiGetClassDevsW(
            Some(&GUID_DEVINTERFACE_DISK),
            None,
            None,
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )
    }
    .ok()?;
    let _guard = DevInfoGuard(dev_info);

    let mut paths = Vec::new();
    let mut index = 0u32;
    loop {
        let mut if_data: SP_DEVICE_INTERFACE_DATA = unsafe { std::mem::zeroed() };
        if_data.cbSize = size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
        // SAFETY: if_data.cbSize 已设置；枚举完返回 Err（ERROR_NO_MORE_ITEMS）即结束。
        if unsafe {
            SetupDiEnumDeviceInterfaces(
                dev_info,
                None,
                &GUID_DEVINTERFACE_DISK,
                index,
                &mut if_data,
            )
        }
        .is_err()
        {
            break;
        }
        index += 1;

        // 先取所需缓冲区大小（预期返回 Err，仅填 req）。
        let mut req = 0u32;
        // SAFETY: req 有效；detail 传 None 仅为查询 size。
        let _ = unsafe {
            SetupDiGetDeviceInterfaceDetailW(dev_info, &if_data, None, 0, Some(&mut req), None)
        };
        if req == 0 {
            continue;
        }
        // SetupAPI 正常恒 >= 8；异常小值（驱动/注册表损坏或被 hook）会让下面写 cbSize 越界。
        if (req as usize) < size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() {
            continue;
        }

        let mut buf = vec![0u8; req as usize];
        // SAFETY: cbSize 必须设为结构体大小（64 位为 8），API 据此定位 DevicePath。
        let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
        unsafe {
            (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
        }
        // SAFETY: buf 容量 = req，detail 指向同一块内存。
        if unsafe {
            SetupDiGetDeviceInterfaceDetailW(dev_info, &if_data, Some(detail), req, None, None)
        }
        .is_err()
        {
            continue;
        }

        // DevicePath 是 detail 结构内的灵活数组，取其真实内存偏移后读取。
        // SAFETY: detail 指向合法 buf；DevicePath 紧随其后。
        let dp = unsafe { (*detail).DevicePath.as_ptr() };
        let off = (dp as usize) - (buf.as_ptr() as usize);
        if let Some(p) = read_wide_in_buf(&buf, off) {
            paths.push(p);
        }
    }
    Some(paths)
}

/// 打开设备并尝试填充所有字段。返回是否成功打开设备句柄。
///
/// 打开失败（非提权/设备忙）时所有 IOCTL 字段保持 `None`，返回 `false`——绝不让整盘采集失败。
fn query_disk(path: &str, disk: &mut PhysicalDisk) -> bool {
    let handle = match open_device(path, GENERIC_READ.0) {
        // 只读打开失败再退回 0 访问，部分只读/被占用设备仍允许查询属性。
        Some(h) => h,
        None => match open_device(path, 0) {
            Some(h) => h,
            None => return false,
        },
    };

    // 型号 + 序列号 + 固件 + 总线类型（失败降级 None）
    if let Some((model, serial, firmware, bus)) = query_device_descriptor(handle.0) {
        if !model.is_empty() {
            disk.model = Some(model);
        }
        disk.serial = if bus_name(bus) == Some("NVMe") {
            // NVMe 的 `SerialNumberOffset` 在部分存储栈上给的不是序列号而是二进制标识
            // （实测 4 块盘拿到 `3633_5330_…_0001.` 且跨盘共享后缀），改走 Identify
            // Controller 取盘自身的权威值；取不到才保留 descriptor 的值（可能已被判 None）。
            query_nvme_serial(path).or(serial)
        } else {
            serial
        };
        disk.firmware = firmware.filter(|f| !f.trim().is_empty());
        disk.iface = bus_name(bus).map(str::to_string);
    }

    // 序列号最后兜底：WMI `MSFT_PhysicalDisk.SerialNumber`（按盘索引）。
    // 仅在前面几条路都拿不到（descriptor 被判废 / NVMe Identify 被存储栈拒）时启用。
    // WMI 与 `DeviceIoControl` 是完全独立的内核通道——VMD 下 NVMe 的
    // `IOCTL_STORAGE_PROTOCOL_COMMAND` 会被整体拒绝（实测 property-query 与
    // protocol-command 双双失败），此时 WMI 是唯一还能拿到真实序列号的路径。
    if disk.serial.is_none() {
        if let Some(idx) = disk_index_from_path(path) {
            let info = collect_storage_reliability().get(&idx);
            // Windows Server 上 NVMe 的 `SerialNumber` 是 EUI64/NGUID，真实打印序列号在
            // `AdapterSerialNumber`（Dell/微软 KBA 确认）。优先用后者，仍无值再退回原 `SerialNumber`。
            let wmi_serial = info
                .and_then(|i| i.adapter_serial.clone())
                .or_else(|| info.and_then(|i| i.serial.clone()));
            match &wmi_serial {
                Some(s) => wmi_dbg(&format!("disk {idx} 序列号取自 WMI 兜底: {s}")),
                None => wmi_dbg(&format!("disk {idx} 序列号：WMI 兜底亦无值")),
            }
            disk.serial = wmi_serial;
        }
    }

    // 容量（失败降级 None）
    if let Some(size) = query_length(handle.0) {
        disk.size_bytes = Some(size);
    }

    // 分区表类型（GPT/MBR/RAW，per-disk）：只读句柄即可，失败降级 None。
    disk.partition_style = query_partition_style(handle.0);

    // 介质类型：无寻道惩罚 = SSD（拿不到就 None，不猜）
    match query_seek_penalty(handle.0) {
        Some(true) => disk.media = Some("HDD".to_string()),
        Some(false) => disk.media = Some("SSD".to_string()),
        None => {
            // NVMe 天然是 SSD，拿不到 seek penalty 时据此兜底（与 C++ 一致）。
            if disk.iface.as_deref() == Some("NVMe") {
                disk.media = Some("SSD".to_string());
            }
        }
    }

    // SMART 通电/健康：必须用 RW 句柄（IOCTL 协议查询/ATA pass-through 要求读写权限），
    // 单独 `open_device(GENERIC_READ|GENERIC_WRITE)` 打开，失败就返回 `None`，
    // **绝不**改动上面那个只读句柄——否则非提权场景连型号/容量都采不到。打开/采集失败
    // 只影响 `smart` 字段，不污染其它字段。
    disk.smart = query_smart(path, &disk.iface);

    // ATA SMART 刻意不填温度（0xC2 属性厂商单位不统一，见 `parse_ata_smart`），但 WMI
    // 可靠性计数器常能给出存储栈校准过的温度。实测 SATA 盘 WMI 明确报了 31°C 却被整条
    // 丢弃，故在此回填，避免丢掉真实读数。`collect_storage_reliability()` 有 OnceLock
    // 缓存，这里重复调用不产生额外 WMI 开销。
    if let Some(smart) = disk.smart.as_mut() {
        if smart.temperature_c.is_none() {
            if let Some(idx) = disk_index_from_path(path) {
                if let Some(info) = collect_storage_reliability().get(&idx) {
                    smart.temperature_c = info.temperature_c;
                }
            }
        }
    }

    // 存储后端识别：盘位于 RAID 控制器 / Storage Spaces / 虚拟磁盘之后时，SMART 经控制器
    // 可能被合成或不可直读，完整数据需 storcli/megacli 等厂商工具。明确标注，避免误当真实数据。
    if let Some(idx) = disk_index_from_path(path) {
        if let Some(backend) = storage_backend_of(idx) {
            disk.storage_backend = Some(backend);
        }
    }

    true
}

// ============================================================
// SMART 采集：通电时间 + 健康度
// ============================================================
//
// 纪律（作者硬性要求，违反等于制造假数据）：
// - 拿不到就 `None`，绝不给 0（"通电 0 小时"像全新盘，比没数据危险）；
// - "读不到"绝不等于"健康"——健康态默认 `Unknown`；
// - `health_percent` 只在有确凿依据时给值（NVMe `PercentageUsed` 换算），ATA 不瞎猜；
// - SMART 需管理员权限，单独 RW 句柄采集，失败只影响 `smart` 字段。

/// 按接口分流 SMART 采集。返回 `Some(DiskSmart)`（无论拿到数据还是明确的不可用例/原因），
/// 仅当接口信息缺失（无从判断）时返回 `None`。
fn query_smart(path: &str, iface: &Option<String>) -> Option<DiskSmart> {
    let iface = iface.as_deref()?;

    // 明确不支持的接口：不浪费 IOCTL，直接给原因（无盘客户机/外置盘常见）。
    let unsupported = match iface {
        "USB" => Some("USB 设备不支持 SMART 直采"),
        "RAID" => Some("RAID 控制器下无法直读物理盘 SMART"),
        "iSCSI" => Some("iSCSI/无盘设备不支持 SMART"),
        "虚拟" | "虚拟文件" => Some("虚拟磁盘不支持 SMART"),
        _ => None,
    };
    if let Some(reason) = unsupported {
        // 先试 WMI 可靠性计数器兜底：USB/iSCSI/虚拟盘通常没有 `MSFT_PhysicalDisk` 条目，
        // 取不到就仍回退到原不可用例，行为不变。
        if let Some(s) = smart_from_wmi_reliability(path) {
            return Some(s);
        }
        return Some(DiskSmart {
            source: SmartSource::None,
            unavailable_reason: Some(reason.to_string()),
            ..Default::default()
        });
    }

    if iface == "NVMe" {
        return query_smart_nvme(path);
    }
    if matches!(iface, "SATA" | "ATA") {
        return query_smart_ata(path);
    }
    // SCSI/SAS 等：NVMe log page 与 ATA pass-through 均不适用。但这些盘常经 VMD / RAID(PERC)
    // 控制器，SMART 透传被封禁；WMI 可靠性计数器走独立于 DeviceIoControl 的内核通道仍可拿回
    // 温度 / 磨损 / 通电时间 / 健康态。先试它，拿不到才标不可直采（真机 R730/PERC 据此补回
    // PowerOnHours=Some(...)）。
    if let Some(s) = smart_from_wmi_reliability(path) {
        return Some(s);
    }
    Some(DiskSmart {
        source: SmartSource::None,
        unavailable_reason: Some("该接口不支持 SMART 直采".to_string()),
        ..Default::default()
    })
}

/// NVMe 路径：先用 `IOCTL_STORAGE_QUERY_PROPERTY`（协议特定）取 SMART/Health log page（LID 0x02）；
/// 该路径在部分企业级/数据中心 NVMe（尤其服务器存储栈）上会直接 `DeviceIoControl` 失败，
/// 此时回落到更通用、更稳的 `IOCTL_STORAGE_PROTOCOL_COMMAND`（NVMe Admin Get Log Page）。
fn query_smart_nvme(path: &str) -> Option<DiskSmart> {
    // 必须 RW 句柄：协议查询与命令下发都需要读写权限。
    let handle = match open_device(path, GENERIC_READ.0 | GENERIC_WRITE.0) {
        Some(h) => h,
        None => {
            return Some(DiskSmart {
                source: SmartSource::None,
                unavailable_reason: Some("需管理员权限（无法以读写方式打开设备）".to_string()),
                ..Default::default()
            })
        }
    };

    // 主路径：property query。Err(code) = IOCTL 本身失败；Ok(None) = IOCTL 成功但无有效 log。
    let mut prop_err: Option<u32> = None;
    match nvme_query_property(&handle) {
        Ok(Some(log)) => return parse_nvme_smart(&log),
        Ok(None) => { /* IOCTL 成功但无有效 log，继续尝试 protocol command */ }
        Err(code) => prop_err = Some(code),
    }

    // 兜底：protocol command（现代 Windows 对企业级 NVMe 更可靠）。
    // 依次尝试两个维度，任一成功即采用：
    //   - NSID：0xFFFFFFFF（广播/全命名空间）与 0（部分控制器拒绝广播，改传 0）；
    //   - Flags：先 0（针对设备），再 ADAPTER_REQUEST（针对适配器/HBA，VMD 场景常需此通道）。
    // 并如实记录最后一次失败细节，便于定位根因。
    let mut proto_diag: Option<String> = None;
    // 三维兜底：NSID（广播/全命名空间 0xFFFFFFFF 或 0）× Flags（设备/适配器）×
    // 命令偏移 cmd_off（sizeof=84 或 offsetof=80）。任一成功即采用，并如实记录失败细节。
    for nsid in [0xFFFF_FFFFu32, 0u32] {
        for flags in [0u32, STORAGE_PROTOCOL_COMMAND_FLAG_ADAPTER_REQUEST] {
            for cmd_off in [84usize, 80usize] {
                match nvme_query_protocol_command(&handle, nsid, flags, cmd_off) {
                    ProtoResult::Ok(log) => return parse_nvme_smart(&log),
                    ProtoResult::Ioctl(code) => {
                        proto_diag = Some(format!("IOCTL 错误码 {code}（cmd_off={cmd_off}）"));
                    }
                    ProtoResult::Status(s) => {
                        proto_diag = Some(format!("命令返回非成功状态 {s}（cmd_off={cmd_off}）"));
                    }
                }
            }
        }
    }

    // 两条路都失败：给出含 Win32 错误码的诊断信息，便于下次捕获定位根因。
    let reason = match (prop_err, &proto_diag) {
        (Some(pc), Some(pr)) => format!(
            "NVMe SMART 采集失败：property-query IOCTL 错误码 {pc}，protocol-command {pr}（可能驱动不支持或盘被虚拟层遮挡）"
        ),
        (Some(pc), None) => format!(
            "NVMe SMART 采集失败：property-query IOCTL 错误码 {pc}，protocol-command 无有效 log"
        ),
        (None, Some(pr)) => format!("NVMe SMART 采集失败：protocol-command {pr}"),
        (None, None) => {
            "NVMe SMART 未返回有效数据（property-query 与 protocol-command 均无有效 log）".to_string()
        }
    };

    // 第三兜底：WMI（命名空间 `ROOT\Microsoft\Windows\Storage`）。本机 VMD 存储栈会拒绝 NVMe 的
    // SMART IOCTL（property-query 与 protocol-command 两条路径均返回 87），但放行这条与
    // DeviceIoControl 完全独立的内核路径。它暴露温度 / 磨损 / 健康态 / 通电时间——
    // 原先结论是「PowerOnHours 恒为空」，但那次实测环境里整条 WMI 计数器路径都取不到数据
    // （温度也空），样本无效；现已在 WMI 确实可用的机器上重新启用：有值则用，无值仍留空。
    if let Some(s) = smart_from_wmi_reliability(path) {
        return Some(s);
    }

    Some(DiskSmart {
        source: SmartSource::None,
        unavailable_reason: Some(reason),
        ..Default::default()
    })
}

/// 把一条 WMI 可靠性信息（`ReliabilityInfo`）转换为 `DiskSmart`。
///
/// 纯逻辑，不碰 WMI/COM，便于单测。纪律（见模块顶部）：
/// - 拿不到就 `None`，绝不给 0——`wear` / `power_on_hours` 真 0 在此再过滤一层，
///   与 `collect_storage_reliability` 采集侧的过滤构成双重保险；
/// - "读不到"绝不等于"健康"——健康态默认 `Unknown`；
/// - `health_percent` / `percentage_used` 只在有确凿 `wear`（非 0）时给值，剩余 = 100 - 已用
///   （已用可超 100，经 `saturating_sub` 饱和到 0）。
fn smart_from_reliability_info(info: &ReliabilityInfo) -> DiskSmart {
    // 优先采用 WMI 上报的健康态；若其未知但确有温度读数，按"有读数=可观测"降级为 Healthy。
    let health_status = if info.health_status != HealthStatus::Unknown {
        info.health_status
    } else if info.temperature_c.is_some() {
        HealthStatus::Healthy
    } else {
        HealthStatus::Unknown
    };
    // wear 真 0 表示"未上报"，按纪律判 None（与采集侧同一套规则，双重保险）。
    let wear = info.wear.filter(|w| *w != 0);
    DiskSmart {
        source: SmartSource::StorageReliabilityCounter,
        temperature_c: info.temperature_c,
        power_on_hours: info.power_on_hours.filter(|h| *h != 0),
        health_status,
        health_percent: wear.map(|w| 100u8.saturating_sub(w)),
        percentage_used: wear,
        unavailable_reason: None,
        ..Default::default()
    }
}

/// 经 WMI 可靠性计数器兜底取 SMART（独立于 DeviceIoControl 的内核通道，
/// ATA/NVMe SMART 透传被封禁时仍可用，如 VMD / RAID(PERC) 盘）。
///
/// 找不到对应盘的 `MSFT_StorageReliabilityCounter` 条目（USB / iSCSI / 虚拟盘通常如此）
/// 时返回 `None`，调用方据此回退到原不可用例。
fn smart_from_wmi_reliability(path: &str) -> Option<DiskSmart> {
    let idx = disk_index_from_path(path)?;
    let info = collect_storage_reliability().get(&idx)?;
    Some(smart_from_reliability_info(info))
}

/// 从 `\\.\PhysicalDriveN` 路径里解析出盘索引 N（与 WMI `MSFT_PhysicalDisk.DeviceID` 对应）。
fn disk_index_from_path(path: &str) -> Option<u32> {
    let name = path.rsplit('\\').next()?; // "PhysicalDriveN"
    let n = name.strip_prefix("PhysicalDrive")?;
    n.parse::<u32>().ok()
}

/// 判断盘是否位于 RAID 控制器 / Storage Spaces / 虚拟磁盘之后（这些场景下
/// `\\.\PhysicalDriveN` 枚举到的可能是控制器合成盘，SMART 不可直读或不可信）。
/// 返回需向用户明示的后端说明；普通 SATA / NVMe 直连盘返回 `None`。
fn storage_backend_of(idx: u32) -> Option<String> {
    let info = collect_storage_reliability().get(&idx)?;
    classify_storage_backend(info.bus_type, info.model.as_deref().unwrap_or(""))
}

/// 依据 `MSFT_PhysicalDisk` 的 `BusType` 与 `Model` 识别存储后端。
///
/// `BusType` 枚举（权威信号）：`8=RAID`、`14=Virtual`、`15=FileBackedVirtual`、
/// `16=StorageSpaces`、`17=NVMe`。型号关键字作辅助（PERC / MEGARAID / SMART ARRAY 为
/// 明确的 RAID 控制器品牌串）。识别到才返回说明，避免对普通盘误报。
fn classify_storage_backend(bus_type: Option<u16>, model: &str) -> Option<String> {
    match bus_type {
        Some(8) => return Some("RAID 控制器后（BusType=RAID）".to_string()),
        Some(14) | Some(15) => return Some("虚拟磁盘后".to_string()),
        Some(16) => return Some("Storage Spaces 虚拟磁盘后".to_string()),
        _ => {}
    }
    let m = model.to_ascii_uppercase();
    if m.contains("PERC") || m.contains("MEGARAID") || m.contains("SMART ARRAY") {
        return Some(format!("RAID 控制器后（{}）", model.trim()));
    }
    None
}

/// NVMe 第三兜底用的可靠性信息（来自 WMI，非 IOCTL）。
#[derive(Clone)]
struct ReliabilityInfo {
    /// 复合温度（摄氏度，已按存储栈单位歧义校准）。
    temperature_c: Option<i16>,
    /// 磨损百分比（仅非 0 时采用；0 可能是"未上报"，按纪律给 None）。
    wear: Option<u8>,
    /// 健康态（取自 `MSFT_PhysicalDisk.HealthStatus`，比"有温度=健康"更确凿）。
    health_status: HealthStatus,
    /// 总线类型（来自 `MSFT_PhysicalDisk.BusType`），用于识别 RAID / 虚拟盘。
    bus_type: Option<u16>,
    /// 型号（来自 `MSFT_PhysicalDisk.Model`），RAID 控制器型号识别用。
    model: Option<String>,
    /// 序列号（来自 `MSFT_PhysicalDisk.SerialNumber`）。
    ///
    /// 存在意义：`STORAGE_DEVICE_DESCRIPTOR` 的序列号在 NVMe / RAID / 虚拟层后常被合成成
    /// hex dump 伪值（已被 `sanitize_disk_serial` 判废），而 NVMe Identify 走的是
    /// `IOCTL_STORAGE_PROTOCOL_COMMAND`——**在 VMD 等存储栈上该 IOCTL 会被整体拒绝**
    /// （实测 property-query 与 protocol-command 双双失败，SMART 只能靠 WMI 兜底）。
    /// WMI 走的是完全独立的内核通道，不受此限制，是这类场景下唯一可靠的序列号来源。
    serial: Option<String>,
    /// NVMe 真实打印序列号（来自 `MSFT_PhysicalDisk.AdapterSerialNumber`）。
    ///
    /// 关键修正（2026-09-18）：Windows Server 2016/2019/2022 上，`MSFT_PhysicalDisk.SerialNumber`
    /// 对 NVMe 返回的是 **EUI64/NGUID**（形如 `3633_5330_…_0001.` 的带下划线串），
    /// 并非盘体打印序列号；**真实打印序列号在 `AdapterSerialNumber` 字段**（Dell/微软 KBA 确认）。
    /// 原代码 WMI 查询未取此字段，导致 4 块 NVMe 序列号全空。此处补取并作为 NVMe 序列号首选。
    adapter_serial: Option<String>,
    /// 通电小时数（来自 `MSFT_StorageReliabilityCounter.PowerOnHours`）。
    ///
    /// 之前按「实测恒为空」放弃了该字段，但那次实测环境里整条 WMI 计数器路径都取不到数据
    /// （连温度都是空），属**样本无效**。现已在 WMI 确实可用的机器上（温度能读到）重新启用：
    /// 有值则用，无值仍如实留空——绝不用钳位/推导值冒充。
    power_on_hours: Option<u32>,
}

/// 通过 WMI 采集每块物理盘的 `MSFT_StorageReliabilityCounter`（温度 / 磨损）与
/// `MSFT_PhysicalDisk.HealthStatus`，按盘索引缓存一次供 `query_smart_nvme` 兜底使用。
///
/// 全程纯 Rust COM/WMI 绑定（`windows` crate），不引 PowerShell / .NET 子进程，
/// 保持单 exe、零运行时依赖。任何一步失败都返回空表（上层继续用 IOCTL 的诊断信息）。
fn collect_storage_reliability() -> &'static HashMap<u32, ReliabilityInfo> {
    static CACHE: OnceLock<HashMap<u32, ReliabilityInfo>> = OnceLock::new();
    CACHE.get_or_init(|| {
        // SAFETY: 标准 COM 初始化；RPC_E_CHANGED_MODE(0x80010106) 表示线程已初始化，沿用即可。
        // CoInitializeEx 返回裸 HRESULT（非 Result），pvReserved 为保留参数（传 None）。
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        wmi_dbg(&format!("CoInitializeEx hr={:#x}", hr.0));
        wmi_dbg(&format!("process_is_elevated = {:?}", process_is_elevated()));
        // 进程级 COM 安全初始化（WMI 客户端必做的一步，之前遗漏 → 真机兜底的 Root Cause）：
        // 未调用时进程沿用 COM 默认鉴权级别，而 ROOT\Microsoft\Windows\Storage 命名空间强制要求
        // RPC_C_AUTHN_LEVEL_PKT_PRIVACY；于是 ConnectServer/ExecQuery「看似成功」，但真正取数（Next）
        // 被 WBEM_E_ACCESS_DENIED 拒绝，即使进程已提权（见 process_is_elevated=Some(true) 仍失败）。
        // 设了进程级默认后，所有经代理的调用（含异步枚举器的内部投递）都使用 PKT_PRIVACY，问题根除。
        // 重复调用返回 RPC_E_ALREADY_INITIALIZED / RPC_E_TOO_LATE（被别的组件已初始化）属正常，忽略。
        let sec_hr = unsafe {
            CoInitializeSecurity(
                None,
                -1, // cAuthSvc=-1：使用系统默认鉴权服务列表
                None,
                None,
                RPC_C_AUTHN_LEVEL_PKT_PRIVACY,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOAC_NONE,
                None,
            )
        };
        wmi_dbg(&format!(
            "CoInitializeSecurity hr={:?}",
            sec_hr.as_ref().map(|_| 0).map_err(|e| e.code().0)
        ));
        // 若 hr 为 RPC_E_CHANGED_MODE（完整值 0x80010106，低 16 位为 0x0106），表示线程早已初始化，
        // 沿用即可（own=false，不抢着 CoUninitialize）。注意只能比低 16 位 0x0106，不能拿 &0xFFFF 的结果
        // 去比对完整值 0x80010106（那样永远不相等）。
        let com = if hr.is_ok() || (hr.0 & 0xFFFF) == 0x0106 {
            Some(ComGuard {
                own: hr.is_ok(),
            })
        } else {
            None
        };
        let _com = match com {
            Some(g) => g,
            None => {
                wmi_dbg("CoInitializeEx 失败，放弃 COM 兜底");
                return HashMap::new();
            }
        };

        // 建立 WMI 连接（本地 ROOT\Microsoft\Windows\Storage）。
        let locator: IWbemLocator =
            match unsafe { CoCreateInstance(&WbemLocator, None::<&IUnknown>, CLSCTX_INPROC_SERVER) } {
                Ok(l) => l,
                Err(e) => {
                    wmi_dbg(&format!("CoCreateInstance(WbemLocator) 失败: {e:?}"));
                    return HashMap::new();
                }
            };
        let ns = BSTR::from("ROOT\\Microsoft\\Windows\\Storage");
        let empty = BSTR::from("");
        let svc = match unsafe {
            locator.ConnectServer(&ns, &empty, &empty, &empty, 0, &empty, None::<&IWbemContext>)
        } {
            Ok(s) => s,
            Err(e) => {
                wmi_dbg(&format!("ConnectServer({ns}) 失败: {e:?}"));
                return HashMap::new();
            }
        };
        wmi_dbg("ConnectServer 成功");
        // 本地连接安全 blanket（必须设置，否则跨单元调用 IWbemServices 代理会返回
        // WBEM_E_ACCESS_DENIED 0x80041003）。按 MS 官方 WMI C++ 示例用明确的 RPC_C_AUTHN_WINNT(10)
        // + RPC_C_AUTHN_LEVEL_CALL(3)，切勿传 RPC_C_AUTHN_DEFAULT(0xFFFFFFFF) 当 dwAuthnSvc（COM 常
        // 协商不出鉴权服务 → 即使 Administrator 也 ACCESS_DENIED，正是真机兜底静默失败的根因）。
        if let Ok(unk) = svc.cast::<IUnknown>() {
            let r = unsafe {
                CoSetProxyBlanket(
                    &unk,
                    10u32, // RPC_C_AUTHN_WINNT：本地 WMI 连接的标准鉴权服务。
                    0u32,  // RPC_C_AUTHZ_NONE
                    PCWSTR::null(),
                    RPC_C_AUTHN_LEVEL_PKT_PRIVACY, // Storage 命名空间强制要求的高隐私级别
                    RPC_C_IMP_LEVEL_IMPERSONATE,
                    None::<*const core::ffi::c_void>,
                    EOAC_NONE,
                )
            };
            wmi_dbg(&format!("CoSetProxyBlanket(svc) = {r:?}"));
        }

        let mut map: HashMap<u32, ReliabilityInfo> = HashMap::new();

        // 1) 物理盘：DeviceID（索引）+ HealthStatus + BusType + Model（用于 RAID / 虚拟盘识别）。
        let disks = wmi_query(
            &svc,
            "SELECT DeviceID, ObjectId, HealthStatus, BusType, Model, SerialNumber, AdapterSerialNumber FROM MSFT_PhysicalDisk",
        );
        wmi_dbg(&format!("MSFT_PhysicalDisk 命中 {} 块", disks.len()));
        for d in &disks {
            let device_id = wmi_get_u16(d, "DeviceID");
            let health = match wmi_get_u16(d, "HealthStatus") {
                Some(0) => HealthStatus::Healthy,
                Some(1) => HealthStatus::Warning,
                Some(2) => HealthStatus::Critical,
                _ => HealthStatus::Unknown,
            };
            let bus_type = wmi_get_u16(d, "BusType");
            let model = wmi_get_bstr(d, "Model");
            // 诊断要点：**必须记清洗前的原始值**。否则无法区分「WMI 根本没返回」与
            // 「返回了但被 sanitize_disk_serial 判废（hex dump / 超长 / 含控制字符）」——
            // 这两种情况的下一步动作完全不同。
            let raw_serial = wmi_get_bstr(d, "SerialNumber");
            let serial = raw_serial.as_deref().and_then(sanitize_disk_serial);
            // NVMe 真实打印序列号（Dell/微软 KBA：SerialNumber 对 NVMe 是 EUI64，真实 SN 在 AdapterSerialNumber）。
            let raw_adapter_sn = wmi_get_bstr(d, "AdapterSerialNumber");
            let adapter_serial = raw_adapter_sn.as_deref().and_then(sanitize_disk_serial);
            wmi_dbg(&format!(
                "MSFT_PhysicalDisk DeviceID={device_id:?}: SerialNumber 原始={raw_serial:?} -> 清洗后={serial:?} | AdapterSerialNumber 原始={raw_adapter_sn:?} -> 清洗后={adapter_serial:?} | UniqueId={:?}",
                wmi_get_bstr(d, "UniqueId")
            ));
            if let Some(id) = device_id {
                map.insert(
                    u32::from(id),
                    ReliabilityInfo {
                        temperature_c: None,
                        wear: None,
                        health_status: health,
                        bus_type,
                        model: model.clone(),
                        serial,
                        adapter_serial,
                        // 计数器（温度 / 磨损 / 通电小时）在这个循环里还没读到，稍后由第 2 步回填。
                        power_on_hours: None,
                    },
                );
            }
        }

        // 2) 每块盘的关联计数器：温度 + 磨损，写回对应索引。
        for d in &disks {
            let Some(object_id) = wmi_get_bstr(d, "ObjectId") else {
                continue;
            };
            let assoc = format!(
                "ASSOCIATORS OF {{MSFT_PhysicalDisk.ObjectId=\"{}\"}} WHERE AssocClass = MSFT_PhysicalDiskToStorageReliabilityCounter ResultClass = MSFT_StorageReliabilityCounter",
                escape_wql(&object_id)
            );
            let counters = wmi_query(&svc, &assoc);
            wmi_dbg(&format!("disk ObjectId={object_id} -> 计数器 {} 条", counters.len()));
            for c in &counters {
                // 温度 0 = 未上报（与 wear/poh、NVMe 开尔文路径同一套"0 即无读数"纪律）：
                // 真机 VM 的 WMI 报 Temperature=0 曾被 `calibrate_temp(0)=0` 收成 Some(0)，
                // 详情卡于是显示"温度 0℃"——0°C 物理不可能，是假数据。`wmi_temp_c` 先滤 0。
                let temp_c = wmi_get_u16(c, "Temperature").and_then(wmi_temp_c);
                let wear = wmi_get_u16(c, "Wear").filter(|w| *w != 0).map(|w| w as u8);
                // 通电小时是 uint64；真 0 表示"未上报"，按纪律判 None（与 wear 同一套规则）。
                let poh = wmi_get_u64(c, "PowerOnHours")
                    .filter(|h| *h != 0)
                    .and_then(|h| u32::try_from(h).ok());
                wmi_dbg(&format!(
                    "  counter ObjectId={object_id}: Temperature={:?} Wear={:?} PowerOnHours={:?} -> poh={poh:?}",
                    wmi_get_u16(c, "Temperature"),
                    wmi_get_u16(c, "Wear"),
                    wmi_get_u64(c, "PowerOnHours")
                ));
                let Some(id) = wmi_get_u16(d, "DeviceID") else {
                    continue;
                };
                if let Some(info) = map.get_mut(&(u32::from(id))) {
                    if temp_c.is_some() {
                        info.temperature_c = temp_c;
                    }
                    if wear.is_some() {
                        info.wear = wear;
                    }
                    if poh.is_some() {
                        info.power_on_hours = poh;
                    }
                }
            }
        }

        wmi_dbg(&format!(
            "collect_storage_reliability 完成: map={} 块",
            map.len()
        ));
        for (idx, info) in &map {
            wmi_dbg(&format!(
                "  disk {idx} | temp_c={:?} | wear={:?} | health={:?}",
                info.temperature_c, info.wear, info.health_status
            ));
        }

        map
    })
}

/// RAII 守卫：仅在「本次调用亲自初始化了 COM」时才 `CoUninitialize`，
/// 避免把别人（或同线程早先）初始化的 COM 引用计数错误归零。
struct ComGuard {
    own: bool,
}
impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.own {
            unsafe { CoUninitialize() };
        }
    }
}

/// 执行一条 WQL 查询，返回所有 `IWbemClassObject`（失败返回空 Vec）。
/// WMI 调试日志：仅当进程环境变量 `WSX_WMI_DEBUG` 存在时，把消息追加到工作目录下的
/// `wmi_debug.log`。用于在「无法编译验证的真机」上抓取真实失败点（本机 VM 的 WMI 不返回数据，
/// 无法自测），不影响正常路径行为与性能。
fn wmi_dbg_enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("WSX_WMI_DEBUG").is_some())
}

fn wmi_dbg(msg: &str) {
    if !wmi_dbg_enabled() {
        return;
    }
    use std::io::Write as _;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("wmi_debug.log")
    {
        let _ = writeln!(f, "[WMI] {msg}");
    }
}

/// 进程提权状态自检（仅用于 `WSX_WMI_DEBUG` 诊断日志）。
/// `Some(true)`=已完整提权；`Some(false)`=受 UAC 令牌筛选（未提权，即使属 Administrators 组）；
/// `None`=查询失败。用于区分「ACCESS_DENIED 是权限问题」还是「代码/命名空间 DACL 问题」。
pub fn process_is_elevated() -> Option<bool> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    let mut token = windows::Win32::Foundation::HANDLE::default();
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return None;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut ret: u32 = 0;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret,
        )
        .is_ok();
        let _ = CloseHandle(token);
        if !ok {
            return None;
        }
        Some(elevation.TokenIsElevated != 0)
    }
}

/// 执行一条 WQL 查询，返回所有 `IWbemClassObject`（失败返回空 Vec）。
///
/// 采用最通用的半同步枚举模式（`WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY` +
/// `Next(WBEM_INFINITE)`），这是 Microsoft WMI C++ 文档的标准写法，在所有真实 Windows 上可用。
/// 枚举器本身是独立 COM 代理，必须再设一次 proxy blanket，否则 `Next` 跨单元调用会失败。
fn wmi_query(svc: &IWbemServices, wql: &str) -> Vec<IWbemClassObject> {
    let mut out = Vec::new();
    let lang = BSTR::from("WQL");
    let query = BSTR::from(wql);
    let enum_obj = match unsafe {
        svc.ExecQuery(&lang, &query, WBEM_FLAG_FORWARD_ONLY, None::<&IWbemContext>)
    } {
        Ok(e) => e,
        Err(e) => {
            wmi_dbg(&format!("ExecQuery 失败: {e:?} wql={wql}"));
            return out;
        }
    };
    // RETURN_IMMEDIATELY 返回的枚举器本身是独立代理，可能也需要 blanket，否则 Next 跨单元调用
    // 会返回 WBEM_E_ACCESS_DENIED（0x80041003）。注意：0x80041003 是「拒绝访问」不是「非法参数」，
    // 多半是进程未以管理员提权（UAC 令牌筛选）或命名空间 DACL 限制 —— 必须「以管理员身份运行」。
    if let Ok(unk) = enum_obj.cast::<IUnknown>() {
        let r = unsafe {
            CoSetProxyBlanket(
                &unk,
                10u32, // RPC_C_AUTHN_WINNT（同 svc 代理，避免 ACCESS_DENIED）
                0u32,  // RPC_C_AUTHZ_NONE
                PCWSTR::null(),
                RPC_C_AUTHN_LEVEL_PKT_PRIVACY, // Storage 命名空间强制要求的高隐私级别
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None::<*const core::ffi::c_void>,
                EOAC_NONE,
            )
        };
        wmi_dbg(&format!("CoSetProxyBlanket(enum) = {r:?}"));
    }
    loop {
        let mut objs: [Option<IWbemClassObject>; 1] = [None];
        let mut returned: u32 = 0;
        // SAFETY: objs/returned 在本作用域有效；Next 写入至多 1 个对象。
        let hr = unsafe { enum_obj.Next(WBEM_INFINITE, &mut objs, &mut returned) };
        if hr.is_ok() && returned > 0 {
            if let Some(obj) = objs[0].take() {
                out.push(obj);
            }
            continue;
        }
        // hr != S_OK 表示出错或枚举结束。WBEM_S_FALSE(0x40004) / returned==0 = 正常结束；
        // 0x80041003 = WBEM_E_ACCESS_DENIED = 进程未提权或被 DACL 拒绝；其余为其他错误。
        if hr.is_err() {
            let code = hr.0 as u32;
            let kind = if code == 0x8004_1003 {
                "WBEM_E_ACCESS_DENIED(未以管理员身份运行/权限不足)"
            } else {
                "其他错误"
            };
            wmi_dbg(&format!("Next 失败 hr={code:#x} ({kind}) wql={wql}"));
        }
        break;
    }
    out
}

/// 读取 WMI 对象的 `uint16` 属性（Temperature/Wear/DeviceID/HealthStatus 均为 uint16）。
fn wmi_get_u16(obj: &IWbemClassObject, prop: &str) -> Option<u16> {
    let wide = to_wide(prop);
    let pc = PCWSTR(wide.as_ptr());
    let mut var = VARIANT::default();
    // SAFETY: var 在本作用域有效；Get 只写 var。
    if unsafe { obj.Get(pc, 0, &mut var, None, None) }.is_err() {
        return None;
    }
    // SAFETY: var 有效且类型合法；VARIANT 离开作用域时自动 VariantClear（无泄漏）。
    unsafe { VariantToUInt16(&var) }.ok()
}

/// 读取 WMI 对象的 `uint64` 属性（`MSFT_StorageReliabilityCounter.PowerOnHours` 等）。
///
/// 注意：不能复用 `wmi_get_u16`——`VariantToUInt16` 会把超范围值**静默钳位**成 65535，
/// 通电小时数（动辄数万小时）会被污染成看似合理的假数据。必须按真实类型取。
fn wmi_get_u64(obj: &IWbemClassObject, prop: &str) -> Option<u64> {
    let wide = to_wide(prop);
    let pc = PCWSTR(wide.as_ptr());
    let mut var = VARIANT::default();
    // SAFETY: var 在本作用域有效；Get 只写 var。
    if unsafe { obj.Get(pc, 0, &mut var, None, None) }.is_err() {
        return None;
    }
    // SAFETY: var 有效且类型合法；VARIANT 离开作用域时自动 VariantClear（无泄漏）。
    unsafe { VariantToUInt64(&var) }.ok()
}

/// 读取 WMI 对象的 `string` 属性（如 `ObjectId`），经 `VariantToStringAlloc` 转成 Rust 字符串。
fn wmi_get_bstr(obj: &IWbemClassObject, prop: &str) -> Option<String> {
    let wide = to_wide(prop);
    let pc = PCWSTR(wide.as_ptr());
    let mut var = VARIANT::default();
    // SAFETY: var 在本作用域有效；Get 只写 var。
    if unsafe { obj.Get(pc, 0, &mut var, None, None) }.is_err() {
        return None;
    }
    // SAFETY: VariantToStringAlloc 从 variant 生成新分配的宽字符串；PWSTR 用毕需 CoTaskMemFree。
    let pw = unsafe { VariantToStringAlloc(&var) }.ok()?;
    let s = String::from_utf16_lossy(unsafe { pw.as_wide() });
    unsafe { CoTaskMemFree(Some(pw.0 as *mut core::ffi::c_void)) };
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// WMI `StorageReliabilityCounter.Temperature` 的单位在存储栈间不一致：文档写「开尔文」，
/// 但实测 Intel VMD 服务器返回的是摄氏度（29~37 量级）。约定：>= 274 视为开尔文（减 273），
/// 否则当作摄氏度直接采用（运行中的盘不可能 ≥ 274°C）。
fn calibrate_temp(raw: u16) -> i16 {
    if raw >= 274 {
        (i32::from(raw) - 273) as i16
    } else {
        raw as i16
    }
}

/// WMI `MSFT_StorageReliabilityCounter.Temperature` 原始读数 → 摄氏温度。
///
/// **0 = 未上报**：与 `Wear` / `PowerOnHours`、NVMe 开尔文路径（`parse_nvme_smart`）
/// 同一套「0 即无读数」纪律。`calibrate_temp(0)` 会返回 `0`，若不过滤就会被收成
/// `Some(0)`，详情卡于是显示「温度 0℃」——0°C 对运行中的硬盘物理不可能，是假数据。
/// 故这里显式把 0 判为 `None`，宁可留白也不误报。
fn wmi_temp_c(raw: u16) -> Option<i16> {
    if raw == 0 {
        None
    } else {
        Some(calibrate_temp(raw))
    }
}

/// WQL 字符串字面量转义：反斜杠 → `\\`，双引号 → `\"`（用于拼 `ObjectId="..."` 路径）。
fn escape_wql(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(c),
        }
    }
    out
}

/// 通过 `IOCTL_STORAGE_QUERY_PROPERTY` + `StorageDeviceProtocolSpecificProperty` 取 NVMe
/// SMART/Health Information log page（LID 0x02）。
///
/// - `Ok(Some(log))`：拿到 512 字节 log（已从描述符定位并拷贝）；
/// - `Ok(None)`：IOCTL 成功但返回数据无效（全 0 / 长度不足 / 偏移错位）—— 留给上层回落；
/// - `Err(win32_code)`：`DeviceIoControl` 本身失败（权限不足 / 驱动不支持 / 盘被虚拟层遮挡）。
fn nvme_query_property(handle: &HandleGuard) -> std::result::Result<Option<Vec<u8>>, u32> {
    // 输入 = STORAGE_PROPERTY_QUERY（PropertyId=协议特定）+ STORAGE_PROTOCOL_SPECIFIC_DATA。
    // repr(C) 保证布局：q(12) 后紧跟 proto(40)，无额外 padding。
    #[repr(C)]
    struct NvmeSmartQuery {
        q: STORAGE_PROPERTY_QUERY,
        proto: STORAGE_PROTOCOL_SPECIFIC_DATA,
    }
    let req = NvmeSmartQuery {
        q: STORAGE_PROPERTY_QUERY {
            PropertyId: StorageDeviceProtocolSpecificProperty,
            QueryType: PropertyStandardQuery,
            ..Default::default()
        },
        proto: STORAGE_PROTOCOL_SPECIFIC_DATA {
            ProtocolType: ProtocolTypeNvme,
            // DataType 字段是 u32；NVMeDataTypeLogPage 透明包装 i32(=2)，用 try_into 避免 sign-loss cast。
            DataType: NVMeDataTypeLogPage.0.try_into().unwrap_or(2),
            ProtocolDataRequestValue: 2, // SMART/Health Information log page，LID=0x02
            ProtocolDataRequestSubValue: 0xFFFF_FFFF, // 全局 namespace（CNS），不要填 0
            // 协议数据（log page）紧跟在 STORAGE_PROTOCOL_SPECIFIC_DATA 之后。
            ProtocolDataOffset: size_of::<STORAGE_PROTOCOL_SPECIFIC_DATA>() as u32,
            ProtocolDataLength: 512,
            ..Default::default()
        },
    };

    // 输出 = STORAGE_PROTOCOL_DATA_DESCRIPTOR + 512 字节 log page。
    //
    // 注意描述符是 **48** 字节而不是光算 SPSD 的 40：它前面还有 Version / Size
    // 两个 u32（8 字节），定义见 windows 0.62.2 的 Win32/System/Ioctl/mod.rs。
    // 只按 40 算会有两种后果，都很糟：
    //   (a) 驱动要 560 我们只给 552 → ERROR_INSUFFICIENT_BUFFER，NVMe 盘永远显示"不可用"；
    //   (b) 某些存储栈成功返回但数据整体错位 8 字节 → 温度 / 通电 / 健康全是假数值，
    //       而且全 0 哨兵会因错位后字节非零而失效 —— 这类"看起来很正常的假数据"
    //       在"该不该换盘"的判断里比不显示危险得多。
    const LOG_LEN: usize = 512;
    let mut out = vec![0u8; size_of::<STORAGE_PROTOCOL_DATA_DESCRIPTOR>() + LOG_LEN];
    let mut ret = 0u32;
    // SAFETY: req 与 out 在本作用域有效；req 只读、out 只写，长度足够。
    let ok = unsafe {
        DeviceIoControl(
            handle.0,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(&req as *const _ as *const core::ffi::c_void),
            size_of::<NvmeSmartQuery>() as u32,
            Some(out.as_mut_ptr() as *mut core::ffi::c_void),
            u32::try_from(out.len()).unwrap_or(u32::MAX),
            Some(&mut ret),
            None,
        )
    };
    if ok.is_err() {
        // DeviceIoControl 失败时 HRESULT 的低 16 位即原始 Win32 错误码（如 5=ACCESS_DENIED、
        // 50=NOT_SUPPORTED、87=INVALID_PARAMETER、122=INSUFFICIENT_BUFFER）。
        let code = ok.err().map(|e| (e.code().0 as u32) & 0xFFFF).unwrap_or(0);
        return Err(code);
    }

    match locate_nvme_log(&out, ret) {
        Some(log) => Ok(Some(log.to_vec())),
        None => Ok(None),
    }
}

/// `nvme_query_protocol_command` 的细粒度返回：区分「IOCTL 本身失败」「命令返回非成功状态」
/// 与「成功拿到 log」，便于上层在 `unavailable_reason` 里给出确切的 Win32 错误码 / 命令状态码，
/// 而不是把一切失败都含糊地归为"驱动不支持或盘被虚拟层遮挡"。
enum ProtoResult {
    /// 成功拿到 512 字节 NVMe SMART log page。
    Ok(Vec<u8>),
    /// `DeviceIoControl` 本身失败，携带原始 Win32 错误码（低 16 位）。
    Ioctl(u32),
    /// IOCTL 成功但 NVMe Admin 命令返回非成功状态，携带命令状态码。
    Status(u32),
}

/// 通过 `IOCTL_STORAGE_PROTOCOL_COMMAND` 发送 NVMe Admin **Get Log Page**（LID=0x02，
/// SMART/Health Information），是 Windows 上取 NVMe SMART 最通用、最稳的路径，
/// 能覆盖 property-query 失败的企业级/数据中心盘。
///
/// `nsid` 为要查询的命名空间：多数控制器接受广播 NSID `0xFFFFFFFF`，但部分控制器会拒绝，
/// 这时上层会改传 `0` 重试。`flags` 为 `STORAGE_PROTOCOL_COMMAND` 的 Flags 字段：
/// 0=针对具体设备下发；`STORAGE_PROTOCOL_COMMAND_FLAG_ADAPTER_REQUEST`=针对适配器（HBA/VMD
/// 控制器）下发——VMD / 板载 NVMe 固件场景下，NVMe 透传常需走适配器通道才能被存储栈放行。
fn nvme_query_protocol_command(
    handle: &HandleGuard,
    nsid: u32,
    flags: u32,
    cmd_off: usize,
) -> ProtoResult {
    nvme_query_protocol_command_ex(
        handle,
        flags,
        cmd_off,
        &build_nvme_get_log_page_cmd(nsid),
        512,
    )
}

/// `nvme_query_protocol_command` 的通用形式：命令内容与返回数据长度由调用方给定。
///
/// 抽出是为了复用同一套透传通道承载不同 NVMe Admin 命令——Get Log Page 取 SMART 返回
/// 512 字节，Identify Controller 取序列号返回 4096 字节——避免为每条命令复制整套手工
/// 布局逻辑（那段布局最脆弱，复制即埋雷）。
fn nvme_query_protocol_command_ex(
    handle: &HandleGuard,
    flags: u32,
    cmd_off: usize,
    cmd: &[u8; 64],
    data_len: usize,
) -> ProtoResult {
    const CMD_LEN: usize = STORAGE_PROTOCOL_COMMAND_LENGTH_NVME as usize; // NVMe 命令固定 64 字节

    // 关键布局修正（曾导致 IOCTL 返回 317）：
    // windows 0.62.2 的 `STORAGE_PROTOCOL_COMMAND` 漏掉了 `DataFromDeviceBufferOffset` 与
    // `FixedProtocolReturnData2` 两个 ULONG，又把 `Reserved1` 声明成 `[u32; 3]`，导致它的
    // `Command` 字段落在偏移 76、size_of=80，与本机驱动的**原生 Version=1 布局**（19 个 ULONG
    // 头部=80 字节，`Command` 在偏移 80，size_of=84）错开 4~8 字节。按 crate 的 `size_of`/
    // `&Command` 拼 buffer 会被驱动在提交阶段拒绝（ERROR_MR_MID_NOT_FOUND=317）。
    // 因此这里**手工按原生布局**拼 buffer，完全不依赖 crate 结构体的尺寸。
    //
    // 命令放置位置（cmd_off）由调用方决定，是本机存储栈最挑剔的一点，两种约定都试
    // （见 query_smart_nvme 的循环）：
    //   - cmd_off = 84：命令放在 `sizeof(STRUCTURE)` 之后（= 84），与 smartmontools 及微软官方
    //     NVMe 透传样例一致，此时 Length 也填 84；
    //   - cmd_off = 80：命令放在 `offsetof(Command)` 处（= 80，紧贴头部），Length 填 80。
    // 两者都合法，不同存储栈/驱动只认其中一种，故逐一尝试。
    let data_off = cmd_off + CMD_LEN; // 84+64=148 或 80+64=144
    let total = data_off + data_len; // 660/656（SMART）或 4204/4200（Identify）

    let mut buf = vec![0u8; total];
    // 写头部 u32 字段（小端），偏移严格对应 winioctl.h 的 Version=1 布局。
    let mut w = |off: usize, v: u32| {
        buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
    };
    w(0, STORAGE_PROTOCOL_STRUCTURE_VERSION); // Version
    w(4, cmd_off as u32); // Length = 结构体头部尺寸（84 或 80，与命令放置方式一致）
    w(8, ProtocolTypeNvme.0 as u32); // ProtocolType（STORAGE_PROTOCOL_TYPE 是 i32 新类型）
    w(12, flags); // Flags（0 = 针对设备；ADAPTER_REQUEST = 针对适配器/HBA）
    w(16, 0); // ReturnStatus
    w(20, 0); // ErrorCode
    w(24, CMD_LEN as u32); // CommandLength
    w(28, 0); // ErrorInfoLength
    w(32, 0); // DataToDeviceTransferLength
    w(36, data_len as u32); // DataFromDeviceTransferLength
    w(40, 10); // TimeOutValue（秒）
    w(44, 0); // ErrorInfoOffset
    w(48, 0); // DataToDeviceBufferOffset
    w(52, data_off as u32); // DataFromDeviceBufferOffset
    w(56, STORAGE_PROTOCOL_SPECIFIC_NVME_ADMIN_COMMAND); // CommandSpecific（NVMe Admin 命令）
                                                         // 60..76 的 Reserved0 / FixedProtocolReturnData / FixedProtocolReturnData2 / Reserved1 保持 0

    // 64 字节 NVMe Admin 命令：Get Log Page（opcode 0x02），LID=0x02（SMART/Health），
    // NUMD=127（= 128 dwords = 512 字节），NSID 由调用方指定（0xFFFFFFFF 或 0）。
    // 命令由调用方给定（Get Log Page 或 Identify Controller），直接拷到头部之后。
    buf[cmd_off..cmd_off + CMD_LEN].copy_from_slice(cmd);

    let mut ret = 0u32;
    // SAFETY: buf 同时作为入参（命令/数据偏移）与出参（设备返回数据），长度足够（total 已含全部）。
    let ok = unsafe {
        DeviceIoControl(
            handle.0,
            IOCTL_STORAGE_PROTOCOL_COMMAND,
            Some(buf.as_ptr() as *const core::ffi::c_void),
            u32::try_from(buf.len()).unwrap_or(u32::MAX),
            Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
            u32::try_from(buf.len()).unwrap_or(u32::MAX),
            Some(&mut ret),
            None,
        )
    };
    if ok.is_err() {
        // DeviceIoControl 失败时 HRESULT 的低 16 位即原始 Win32 错误码（如 5=ACCESS_DENIED、
        // 50=NOT_SUPPORTED、87=INVALID_PARAMETER、122=INSUFFICIENT_BUFFER、317=设备无法处理请求）。
        let code = ok.err().map(|e| (e.code().0 as u32) & 0xFFFF).unwrap_or(0);
        return ProtoResult::Ioctl(code);
    }

    // 命令层面状态：ReturnStatus 在头部偏移 16（u32），成功须为
    // STORAGE_PROTOCOL_STATUS_SUCCESS(1)；非成功（如 NOT_SUPPORTED/INVALID_REQUEST）
    // 视为无数据，不解析。
    let return_status = u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]);
    if return_status != STORAGE_PROTOCOL_STATUS_SUCCESS {
        return ProtoResult::Status(return_status);
    }
    if data_off + data_len > buf.len() {
        // 防御性：收包缓冲不足（理论上 total 已含 data_off+data_len，应不可达）。
        return ProtoResult::Ioctl(122); // ERROR_INSUFFICIENT_BUFFER
    }
    ProtoResult::Ok(buf[data_off..data_off + data_len].to_vec())
}

/// 构造 64 字节 NVMe Admin **Get Log Page** 命令（SMART/Health Information，LID=0x02）。
///
/// 纯函数，便于写回归测试钉死字节布局。布局参照 NVMe 1.3/1.4 规范：
/// - byte 0: Opcode = 0x02（Get Log Page）
/// - bytes 4..8: NSID（由调用方指定，通常为 `0xFFFFFFFF` 或 `0`）
/// - CDW10 (**bytes 40..44**): LID=0x02(bits 7:0) | NUMD=127(bits 31:16)（128 dwords = 512 字节）
/// - 其余字段保留为 0（PRP 由 miniport 据缓冲映射，无需我们填）
///
/// ⚠️ 偏移修正（曾写错）：NVMe 命令前 40 字节是 CDW0/NSID/保留×2/MPTR/PRP1/PRP2（各占
/// DW0~DW9），**CDW10 从 byte 40 开始**。原实现写在 bytes 32..36 —— 那其实是 **PRP2** 的地盘，
/// 后果是：LID/NUMD 全部落在 PRP2 里，而真正的 CDW10 恒为 0 → 变成「取 LID=0 的 Supported
/// Log Pages、长度 0」的畸形命令，SMART/Health 数据永远取不到。此处按规范改正。
fn build_nvme_get_log_page_cmd(nsid: u32) -> [u8; 64] {
    let mut cmd = [0u8; 64];
    cmd[0] = 0x02; // Opcode: Get Log Page
    cmd[4..8].copy_from_slice(&nsid.to_le_bytes()); // NSID = 调用方指定
                                                    // CDW10: LID(8 位)=0x02, NUMD(16 位高位)=127（dword 数-1）
    let dw10 = (127u32 << 16) | 0x02u32;
    cmd[40..44].copy_from_slice(&dw10.to_le_bytes());
    cmd
}

/// 构造 64 字节 NVMe Admin **Identify Controller** 命令（opcode 0x06，CNS=0x01）。
///
/// 这是盘自身维护的权威身份数据（序列号/型号/固件），比 `STORAGE_DEVICE_DESCRIPTOR`
/// 更可信——后者在部分存储栈（实测 Intel VMD + 企业 U.2）会返回 16 字节二进制标识的
/// hex dump 而非序列号。
///
/// 布局（NVMe 1.3/1.4 规范）：
/// - byte 0: Opcode = 0x06（Identify）
/// - bytes 4..8: NSID（同 SMART 路径，广播或 0 都试）
/// - CDW10 (**bytes 40..44**): CNS = 0x01（Identify Controller）位于 bits 7:0，CNTID = 0
/// - 返回数据 4096 字节，序列号位于 bytes 4..24
///
/// ⚠️ 同上：CDW10 在 bytes 40..44，不是 32..36（那是 PRP2）。
fn build_nvme_identify_controller_cmd(nsid: u32) -> [u8; 64] {
    let mut cmd = [0u8; 64];
    cmd[0] = 0x06; // Opcode: Identify
    cmd[4..8].copy_from_slice(&nsid.to_le_bytes()); // NSID
    let dw10 = 0x01u32; // CNS=1（Controller），CNTID=0
    cmd[40..44].copy_from_slice(&dw10.to_le_bytes());
    cmd
}

/// 从 Identify Controller 的 4096 字节返回数据里解析序列号（bytes 4..24，20 字节 ASCII）。
///
/// 该字段以空格或 NUL 填充，需截断尾部填充；全 0 表示没真拿到数据，按"未获取"处理而非空串。
fn parse_nvme_identify_serial(ident: &[u8]) -> Option<String> {
    const SN_OFF: usize = 4;
    const SN_LEN: usize = 20;
    if ident.len() < SN_OFF + SN_LEN {
        return None;
    }
    // 全 0 = 内核没真返回数据，绝不解析成空串冒充"有值"。
    if ident.iter().all(|&b| b == 0) {
        return None;
    }
    let s: String = ident[SN_OFF..SN_OFF + SN_LEN]
        .iter()
        .copied()
        .take_while(|&b| b != 0) // NUL 之前为有效内容
        .filter(|&b| (0x20..=0x7e).contains(&b)) // 只留可打印 ASCII
        .map(char::from)
        .collect();
    let s = s.trim().to_string();
    // 最后防线：仍可能是 hex dump 形态的伪值，交回统一清洗。
    sanitize_disk_serial(&s)
}

/// 经 NVMe Identify Controller 取权威序列号；拿不到返回 None（不猜、不兜假值）。
///
/// 重试维度与 `query_smart_nvme` 完全一致（NSID × Flags × cmd_off）——VMD / 企业盘场景
/// 下这套兜底是必要的，原注释已说明 VMD 常需 ADAPTER_REQUEST 通道才放行透传。
fn query_nvme_serial(path: &str) -> Option<String> {
    // 必须 **RW 句柄**：与 `query_smart_nvme` / `query_smart_ata` 同理，
    // `IOCTL_STORAGE_PROTOCOL_COMMAND` 下发 Admin 命令需要读写权限，只读句柄会被拒。
    // （首版复用 `query_disk` 的只读句柄，实测 4 块 NVMe 全部取不到序列号。）
    let handle = match open_device(path, GENERIC_READ.0 | GENERIC_WRITE.0) {
        Some(h) => h,
        None => {
            wmi_dbg(&format!("Identify 序列号：打开 RW 句柄失败 path={path}"));
            return None;
        }
    };
    // 记录最后一次失败细节（借 wmi_dbg，同一 `WSX_WMI_DEBUG` 开关），用于区分
    // 「我方命令布局 / 权限问题」与「存储栈（如 VMD）根本不放行 NVMe 透传」。
    let mut last = String::from("无可用诊断");
    for nsid in [0xFFFF_FFFFu32, 0u32] {
        let cmd = build_nvme_identify_controller_cmd(nsid);
        for flags in [0u32, STORAGE_PROTOCOL_COMMAND_FLAG_ADAPTER_REQUEST] {
            for cmd_off in [84usize, 80usize] {
                match nvme_query_protocol_command_ex(&handle, flags, cmd_off, &cmd, 4096) {
                    ProtoResult::Ok(data) => match parse_nvme_identify_serial(&data) {
                        Some(s) => {
                            wmi_dbg(&format!(
                                "Identify 序列号成功 path={path} nsid={nsid:#x} flags={flags} cmd_off={cmd_off} -> {s}"
                            ));
                            return Some(s);
                        }
                        None => {
                            last = format!(
                                "数据已返回但序列号无效/全 0（nsid={nsid:#x} cmd_off={cmd_off}）"
                            )
                        }
                    },
                    ProtoResult::Ioctl(code) => {
                        last = format!(
                            "IOCTL 错误码 {code}（nsid={nsid:#x} flags={flags} cmd_off={cmd_off}）"
                        )
                    }
                    ProtoResult::Status(s) => {
                        last = format!(
                        "命令返回非成功状态 {s}（nsid={nsid:#x} flags={flags} cmd_off={cmd_off}）"
                    )
                    }
                }
            }
        }
    }
    wmi_dbg(&format!("Identify 序列号失败 path={path}: {last}"));
    None
}

/// 从 `IOCTL_STORAGE_QUERY_PROPERTY` 的输出缓冲里定位 NVMe log page。
///
/// 输出是 `STORAGE_PROTOCOL_DATA_DESCRIPTOR`（**48** 字节 = Version + Size + SPSD40）
/// 后跟 log page。起点必须取回显的 `ProtocolDataOffset`（它是相对描述符起始的偏移），
/// **不能硬编码** —— 曾因只按 SPSD(40) 算漏掉 8 字节头，结果要么驱动报
/// `ERROR_INSUFFICIENT_BUFFER`（NVMe 永远显示"不可用"），要么某些存储栈返回成功
/// 但数据整体错位 8 字节，产出「通电 xx 小时 · 剩余 yy%」这种完美伪装的成功数据，
/// 且全 0 哨兵因错位后字节非零而失效。抽成纯函数就是为了能对此写回归测试。
fn locate_nvme_log(out: &[u8], ret: u32) -> Option<&[u8]> {
    const LOG_LEN: usize = 512;
    let desc_len = size_of::<STORAGE_PROTOCOL_DATA_DESCRIPTOR>();
    if out.len() < desc_len {
        return None;
    }
    // SAFETY: 已校验 out 至少有描述符那么长；read_unaligned 避免对齐要求。
    let echo = unsafe {
        core::ptr::read_unaligned(out.as_ptr() as *const STORAGE_PROTOCOL_DATA_DESCRIPTOR)
    };
    let data_off = usize::try_from(echo.ProtocolSpecificData.ProtocolDataOffset).unwrap_or(0);
    let avail = usize::try_from(ret).unwrap_or(0);
    let end = data_off.checked_add(LOG_LEN)?;
    if data_off < desc_len || end > out.len() || avail < end {
        return None;
    }
    Some(&out[data_off..end])
}

/// 解析 NVMe SMART/Health Information log page（512 字节，NVMe 1.3 布局）。
fn parse_nvme_smart(log: &[u8]) -> Option<DiskSmart> {
    // 全 0 / 过短 = 内核没真返回数据，绝不能解析成"全新盘"——给原因而非假健康。
    if log.len() < 512 || log.iter().all(|&b| b == 0) {
        return Some(DiskSmart {
            source: SmartSource::None,
            unavailable_reason: Some("未返回有效 SMART 数据".to_string()),
            ..Default::default()
        });
    }
    let rd_u16 = |off: usize| u16::from_le_bytes([log[off], log[off + 1]]);
    let rd_u64 = |off: usize| {
        u64::from_le_bytes([
            log[off],
            log[off + 1],
            log[off + 2],
            log[off + 3],
            log[off + 4],
            log[off + 5],
            log[off + 6],
            log[off + 7],
        ])
    };

    let critical_warning = log[0];
    let temp_kelvin = rd_u16(0x01);
    let available_spare = log[0x03];
    let available_spare_threshold = log[0x04];
    let percentage_used = log[0x05];

    let power_on_hours_raw = rd_u64(0x80); // PowerOnHours
    let power_cycles_raw = rd_u64(0x70); // PowerCycles
    let unsafe_shutdowns_raw = rd_u64(0x90); // UnsafeShutdowns
    let media_errors_raw = rd_u64(0xA0); // MediaAndDataIntegrityErrors
    let data_units_written = rd_u64(0x30); // DataUnitsWritten（低 8 字节即可）

    // 128-bit 字段取低 8 字节；0 视为"没拿到"（绝不显示 0 小时，见纪律）。
    // 用 try_from 避免截断 cast（u64->u32），超范围（不可能的天文数字）按拿不到处理。
    let power_on_hours = if power_on_hours_raw == 0 {
        None
    } else {
        u32::try_from(power_on_hours_raw).ok()
    };
    let power_cycles = if power_cycles_raw == 0 {
        None
    } else {
        u32::try_from(power_cycles_raw).ok()
    };
    let unsafe_shutdowns = if unsafe_shutdowns_raw == 0 {
        None
    } else {
        Some(unsafe_shutdowns_raw)
    };
    let media_errors = if media_errors_raw == 0 {
        None
    } else {
        Some(media_errors_raw)
    };
    // DataUnitsWritten 单位 = 1000 个 512B 扇区，换算字节必须 × 512000（直接当字节差 1000 倍）。
    let host_written_bytes = if data_units_written == 0 {
        None
    } else {
        Some(data_units_written * 512_000)
    };
    // 复合温度开尔文，减 273 得摄氏；0 开尔文 = 未上报，不给 -273°C 的鬼值。
    let temperature_c = if temp_kelvin == 0 {
        None
    } else {
        i16::try_from(i32::from(temp_kelvin) - 273).ok()
    };
    // 厂商未上报寿命字段的典型表现是 AvailableSpare 与 Threshold 同时为 0。
    // 此时若因 critical_warning==0 就判 Healthy，等于把"没数据"伪装成"全新盘"，
    // 还会顺带算出"剩余 100%"这种更唬人的假数值 —— 全 0 哨兵只挡得住整页全零，
    // 挡不住这种"字段未上报"的情形。
    let wear_reported = !(available_spare == 0 && available_spare_threshold == 0);
    // PercentageUsed 是"已用"寿命，可超 100（255 饱和）。
    let percentage_used_v = if wear_reported {
        Some(percentage_used)
    } else {
        None
    };
    // 剩余 = 100 - 已用，已用超 100 饱和到 0。
    let health_percent = percentage_used_v.map(|u| {
        let rem = 100u16.saturating_sub(u16::from(u.min(100)));
        u8::try_from(rem).unwrap_or(100)
    });

    let health_status = if critical_warning != 0 || available_spare < available_spare_threshold {
        // CriticalWarning / 备用空间耗尽是确凿的坏信号，即使寿命字段未上报也要报。
        HealthStatus::Critical
    } else if !wear_reported {
        // 没有寿命依据 → Unknown，绝不能落到 Healthy。
        HealthStatus::Unknown
    } else if percentage_used >= 90 || temperature_c.is_some_and(|t| t >= 80) {
        HealthStatus::Warning
    } else {
        HealthStatus::Healthy
    };

    Some(DiskSmart {
        source: SmartSource::NvmeLogPage,
        power_on_hours,
        power_on_duration: None,
        power_cycles,
        health_status,
        health_percent,
        percentage_used: percentage_used_v,
        temperature_c,
        media_errors,
        unsafe_shutdowns,
        host_written_bytes,
        reallocated_sectors: None,
        pending_sectors: None,
        unavailable_reason: None,
    })
}

/// ATA 路径：`IOCTL_ATA_PASS_THROUGH` + SMART READ DATA（Features=0xD0 / Command=0xB0）。
fn query_smart_ata(path: &str) -> Option<DiskSmart> {
    let handle = match open_device(path, GENERIC_READ.0 | GENERIC_WRITE.0) {
        Some(h) => h,
        None => {
            return Some(DiskSmart {
                source: SmartSource::None,
                unavailable_reason: Some("需管理员权限".to_string()),
                ..Default::default()
            })
        }
    };

    // 输入缓冲 = ATA_PASS_THROUGH_EX + 512 字节数据缓冲（数据缓冲紧跟结构体之后）。
    let data_off = size_of::<ATA_PASS_THROUGH_EX>();
    let mut apt = ATA_PASS_THROUGH_EX {
        Length: u16::try_from(size_of::<ATA_PASS_THROUGH_EX>()).unwrap_or(0),
        AtaFlags: ATA_FLAGS_DATA_IN.try_into().unwrap_or(0), // 读回数据
        DataTransferLength: 512,
        TimeOutValue: 10,
        DataBufferOffset: data_off, // 数据缓冲相对结构体起始的偏移
        ..Default::default()
    };
    // SMART READ DATA 任务文件寄存器（CylLow=0x4F/CylHigh=0xC2 是 SMART 命令签名）：
    // Features=0xD0, SectorCount=0x01, SectorNumber=0x01, CylLow=0x4F, CylHigh=0xC2, DriveHead=0xA0, Command=0xB0
    apt.CurrentTaskFile = [0xD0, 0x01, 0x01, 0x4F, 0xC2, 0xA0, 0xB0, 0x00];

    let mut buf = vec![0u8; data_off + 512];
    // SAFETY: apt 在本作用域有效，整块拷进 buf 头部。
    unsafe {
        std::ptr::copy_nonoverlapping(&apt as *const _ as *const u8, buf.as_mut_ptr(), data_off);
    }
    let mut ret = 0u32;
    // SAFETY: buf 同时作为入参（任务文件）与出参（SMART 数据 + 回显寄存器），长度足够。
    let ok = unsafe {
        DeviceIoControl(
            handle.0,
            IOCTL_ATA_PASS_THROUGH,
            Some(buf.as_ptr() as *const core::ffi::c_void),
            u32::try_from(buf.len()).unwrap_or(u32::MAX),
            Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
            u32::try_from(buf.len()).unwrap_or(u32::MAX),
            Some(&mut ret),
            None,
        )
    };
    // IOCTL_ATA_PASS_THROUGH 惯例只回**结构体**长度，512 字节数据写在
    // DataBufferOffset 指向处，未必计入 BytesReturned。若要求 ret >= data_off+512，
    // ATA 路径会恒失败（所有 ATA 盘永远显示"不可用"）。
    // 这里只要求至少回了结构体，数据真伪交给 parse_ata_smart 的内容校验
    // （found_any + 全 0 哨兵）—— 那里才是真正能判断"有没有拿到数据"的地方。
    if ok.is_err() {
        // 取 Win32 错误码以便诊断（如 5=ACCESS_DENIED、50=NOT_SUPPORTED）。
        let code = ok.err().map(|e| (e.code().0 as u32) & 0xFFFF).unwrap_or(0);
        return Some(DiskSmart {
            source: SmartSource::None,
            unavailable_reason: Some(format!(
                "需管理员权限或未返回 SMART 数据（ATA IOCTL 错误码 {code}）"
            )),
            ..Default::default()
        });
    }
    if ret < u32::try_from(data_off).unwrap_or(u32::MAX) {
        return Some(DiskSmart {
            source: SmartSource::None,
            unavailable_reason: Some("未返回完整 SMART 数据（ATA 回传长度不足）".to_string()),
            ..Default::default()
        });
    }

    // 命令完成后，CurrentTaskFile 回显最终寄存器；LBA Mid/High 的"预测失效"签名
    // 是 0x2C/0xF4（或 0xF4/0x2C，不同资料顺序），命中即整机寿命将尽。
    // CurrentTaskFile 在结构体字节偏移 40，bCylLowReg=index3、bCylHighReg=index4。
    let cyl_low = buf[40 + 3];
    let cyl_high = buf[40 + 4];
    let overall_failing =
        (cyl_low == 0x2C && cyl_high == 0xF4) || (cyl_low == 0xF4 && cyl_high == 0x2C);

    let data = &buf[data_off..data_off + 512];
    parse_ata_smart(data, overall_failing)
}

/// 解析 ATA SMART RETURN DATA（512 字节）：属性表从 offset 2 起，最多 30 条，每条 12 字节。
///
/// 每条布局：`[0]=ID, [1-2]=flags, [3]=Normalized, [4]=Worst, [5-10]=Raw(6 字节 LE), [11]=保留`。
/// 注意 raw 单位各厂商不统一（小时/半小时/分钟都有），所以**不**据此猜剩余寿命。
fn parse_ata_smart(data: &[u8], overall_failing: bool) -> Option<DiskSmart> {
    if data.len() < 2 + 30 * 12 {
        return Some(DiskSmart {
            source: SmartSource::None,
            unavailable_reason: Some("未返回有效 SMART 数据".to_string()),
            ..Default::default()
        });
    }

    let mut poh: Option<u32> = None;
    let mut realloc: Option<u64> = None;
    let mut pending: Option<u32> = None;
    let mut uncorr: Option<u64> = None;
    // 0x0C Power Cycle Count 是 ATA SMART **标准属性**（此前误判为"无标准字段"而未解析，
    // 与温度同属"数据在属性表里却没取"）。通电次数对二手盘/网吧盘估值有参考价值。
    let mut cycles: Option<u32> = None;
    let mut found_any = false;
    let mut any_critical_raw = false;

    for i in 0..30 {
        let off = 2 + i * 12;
        let id = data[off];
        if id == 0 {
            continue; // 空槽
        }
        found_any = true;
        let raw = u64::from_le_bytes([
            data[off + 5],
            data[off + 6],
            data[off + 7],
            data[off + 8],
            data[off + 9],
            data[off + 10],
            0,
            0,
        ]);
        match id {
            0x09 => poh = u32::try_from(raw).ok(), // Power_On_Hours（超范围按拿不到）
            0x0C => cycles = u32::try_from(raw).ok(), // Power_Cycle_Count（标准属性）
            0x05 => {
                realloc = Some(raw);
                if raw > 0 {
                    any_critical_raw = true;
                }
            }
            0xC5 => {
                pending = u32::try_from(raw).ok();
                if raw > 0 {
                    any_critical_raw = true;
                }
            }
            0xC6 => {
                uncorr = Some(raw);
                if raw > 0 {
                    any_critical_raw = true;
                }
            }
            _ => {}
        }
    }

    // 全空属性表（含全 0 buffer）= 没真拿到数据，绝不判成"健康"。
    if !found_any {
        return Some(DiskSmart {
            source: SmartSource::None,
            unavailable_reason: Some("未返回有效 SMART 数据".to_string()),
            ..Default::default()
        });
    }

    // 健康依据：整机预测失效 → Critical；否则任一严重属性(raw>0) → Warning；否则 Healthy。
    // ATA 不填 health_percent（无确凿剩余寿命来源）。
    let health_status = if overall_failing {
        HealthStatus::Critical
    } else if any_critical_raw {
        HealthStatus::Warning
    } else {
        HealthStatus::Healthy
    };

    Some(DiskSmart {
        source: SmartSource::AtaSmart,
        // 真 0 小时也按"没拿到"处理，绝不显示 0（纪律 #1）。
        power_on_hours: poh.filter(|&h| h != 0),
        power_on_duration: None,
        // 真 0 也按"没拿到"处理（纪律 #1）：SMART 原始值 0 常表示属性未更新/不支持。
        power_cycles: cycles.filter(|&c| c != 0),
        health_status,
        health_percent: None,
        percentage_used: None,
        temperature_c: None, // ATA 温度属性(0xC2)厂商单位不统一，跳过
        media_errors: uncorr,
        unsafe_shutdowns: None,
        host_written_bytes: None,
        reallocated_sectors: realloc,
        pending_sectors: pending,
        unavailable_reason: None,
    })
}

/// 解析 `\\.\PhysicalDriveN` / `\\?\PhysicalDriveN` 中的 N。
fn parse_drive_index(path: &str) -> Option<u32> {
    let lower = path.to_ascii_lowercase();
    let marker = lower.find("physicaldrive")?;
    lower[marker + "physicaldrive".len()..]
        .trim()
        .parse::<u32>()
        .ok()
}

/// 采集全部物理硬盘。单个盘失败已在 `query_disk` 内降级，不会让整体返回 `Err`。
fn collect_physical_disks() -> Vec<PhysicalDisk> {
    let mut disks = Vec::new();

    // 主路径：SetupAPI 枚举磁盘接口 → \\.\PhysicalDriveN。
    if let Some(paths) = enumerate_disk_paths() {
        for path in paths {
            if let Some(idx) = parse_drive_index(&path) {
                let mut disk = PhysicalDisk {
                    index: idx,
                    ..Default::default()
                };
                query_disk(&path, &mut disk);
                disks.push(disk);
            }
        }
    }

    // 兜底：SetupAPI 拿不到（极少见）时，直接遍历 PhysicalDrive0..63。
    if disks.is_empty() {
        for i in 0..64u32 {
            let path = format!("\\\\.\\PhysicalDrive{}", i);
            let mut disk = PhysicalDisk {
                index: i,
                ..Default::default()
            };
            // 什么都读不到（空读卡器槽等）就跳过，避免列出无效盘。
            if !query_disk(&path, &mut disk) && disk.model.is_none() && disk.size_bytes.is_none() {
                continue;
            }
            disks.push(disk);
        }
    }

    disks
}

// ============================================================
// 对外 API
// ============================================================

/// 物理硬盘数据源。
pub struct PhysicalDiskSource;

impl HardwareSource for PhysicalDiskSource {
    fn name(&self) -> &'static str {
        "physical_disk"
    }

    fn collect(&self, ctx: &mut CollectContext) -> Result<()> {
        // 写入提权状态：决定报告是否提示"NVMe SMART 可能不完整"。
        ctx.set_admin_elevated(process_is_elevated() == Some(true));
        let disks = collect_physical_disks();
        ctx.set_disks(disks);
        // 最佳努力：单项失败已降级为 None，不返回 Err（与 C++ 行为一致）。
        Ok(())
    }
}

/// 把盘符 `Volume` 映射到 `PhysicalDriveN`（`Volume.disk_index`）。
///
/// 通过 `CreateFileW("\\\\.\\C:")` + `IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS` 取
/// `DISK_EXTENT.DiskNumber`。跨盘卷（extents > 1）取第一个盘号（best-effort）。
/// 取不到则保持 `None`（不修改 `disk.rs` 的默认）。
pub fn map_volumes_to_disks(volumes: &mut [Volume]) {
    for vol in volumes.iter_mut() {
        let letter = vol.letter.trim_end_matches(':').to_ascii_uppercase();
        if letter.is_empty() {
            continue;
        }
        let path = format!("\\\\.\\{}:", letter);
        // 卷句柄用 0 访问 + FILE_SHARE 即可做只读 IOCTL，通常无需管理员。
        // 打开失败（极少见/无权限）→ 保持 None，不报错。
        if let Some(h) = open_device(&path, 0) {
            if let Some(disk_number) = query_volume_disk_number(h.0) {
                vol.disk_index = Some(disk_number);
            }
        }
    }
}

/// 取卷所在的第一个物理盘号（跨盘卷取第一个）。
///
/// # 为什么用 `VOLUME_DISK_EXTENTS` 强类型，而不是手算字节偏移
/// 早期实现把 extents 起始偏移当成 `size_of::<u32>()`（=4），但 `DISK_EXTENT`
/// 内部含 `LARGE_INTEGER`（8 字节对齐），**实际起始偏移是 8** —— 读到的其实是
/// 4 字节 padding，恒为 0。表现就是「所有盘符都映射到 PhysicalDrive0」。
/// 把布局交给 SDK 结构体 + 编译器，这类偏移 bug 不可能再发生。
fn query_volume_disk_number(handle: HANDLE) -> Option<u32> {
    const MAX_EXTENTS: usize = 16;
    // 用 u64 数组分配，保证缓冲区 8 字节对齐（DISK_EXTENT 含 LARGE_INTEGER）。
    let words = (size_of::<VOLUME_DISK_EXTENTS>() + MAX_EXTENTS * size_of::<DISK_EXTENT>()) / 8 + 1;
    let mut buf = vec![0u64; words];
    let byte_len = buf.len() * 8;
    let mut ret = 0u32;

    // SAFETY: buf 以 u64 分配（8 字节对齐）且容量足够；DeviceIoControl 只写入，
    // 返回后按实际长度 ret 解析。
    let ok = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS,
            None,
            0,
            Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
            byte_len as u32,
            Some(&mut ret),
            None,
        )
    };
    if ok.is_err() || (ret as usize) < size_of::<VOLUME_DISK_EXTENTS>() {
        return None;
    }

    // SAFETY: 内核已按 VOLUME_DISK_EXTENTS 布局填充；NumberOfDiskExtents 与
    // Extents 实际项数一致，下面用返回值夹住上界，不会越界。
    unsafe {
        let vde = &*(buf.as_ptr() as *const VOLUME_DISK_EXTENTS);
        let n = (vde.NumberOfDiskExtents as usize).min(MAX_EXTENTS);
        if n == 0 {
            return None;
        }
        // Extents 在 SDK 里声明为 [DISK_EXTENT; 1]（柔性数组），按实际项数切片。
        let extents = std::slice::from_raw_parts(vde.Extents.as_ptr(), n);
        Some(extents[0].DiskNumber)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winsuitex_core::model::Volume;
    use winsuitex_core::traits::HardwareSource;

    #[test]
    fn physical_disk_collect_ok() {
        let src = PhysicalDiskSource;
        let mut ctx = CollectContext::new();
        // 最佳努力：本应返回 Ok，不应因个别盘失败而 Err。
        let res = src.collect(&mut ctx);
        assert!(
            res.is_ok(),
            "PhysicalDiskSource.collect 应返回 Ok（best-effort，不因单项失败而 Err）"
        );
        let (info, _vols) = ctx.into_parts();
        let disks = info.disks;
        // 本机（QEMU/KVM 虚拟机）至少有 2 块物理盘。
        assert!(!disks.is_empty(), "本机至少应枚举到 1 块物理盘");
        for d in &disks {
            eprintln!(
                "PhysicalDrive{} | model={:?} | iface={:?} | media={:?} | size={:?} bytes | serial={:?}",
                d.index, d.model, d.iface, d.media, d.size_bytes, d.serial
            );
        }
    }

    #[test]
    fn volume_mapping_finds_disk() {
        // 构造一组常见盘符，调用映射函数，断言至少一个拿到 Some(index)。
        let mut vols = vec![
            Volume {
                letter: "C:".to_string(),
                ..Default::default()
            },
            Volume {
                letter: "D:".to_string(),
                ..Default::default()
            },
        ];
        map_volumes_to_disks(&mut vols);
        for v in &vols {
            eprintln!("{} -> disk_index={:?}", v.letter, v.disk_index);
        }
        assert!(
            vols.iter().any(|v| v.disk_index.is_some()),
            "至少一个盘符应映射到 PhysicalDriveN"
        );
    }

    #[test]
    fn wmi_storage_reliability_smoke() {
        // 回归守卫：collect_storage_reliability 必须「无 panic、返回 HashMap」且不泄漏 COM。
        // 本机 VM 的 WMI 不返回数据（环境限制），故不强制非空；真机（VMD 服务器）上应拿到温度/健康。
        let map = collect_storage_reliability();
        assert!(map.len() <= 64, "盘数量应在合理上界内");
    }

    #[test]
    fn wmi_helper_units() {
        // 纯函数单测：温度校准与 WQL 转义（不依赖 COM/WMI，可在任意环境跑）。
        // 验证「疑似开尔文的大值需减 273」「小值视为 °C」以及反斜杠/双引号转义。
        assert_eq!(calibrate_temp(30), 30); // 30 → 视为 °C
        assert_eq!(calibrate_temp(300), 27); // 300 K − 273 = 27 °C
        assert_eq!(calibrate_temp(273), 273); // 边界：<274 按 °C 原样返回（实现约定）
        assert_eq!(escape_wql("a\\b\"c"), "a\\\\b\\\"c");
        assert_eq!(escape_wql("no_special"), "no_special");
    }

    #[test]
    fn wmi_temp_zero_is_not_zero_celsius() {
        // 回归：VM / 部分存储栈 WMI 报 Temperature=0（未上报），绝不能显示"温度 0℃"。
        assert_eq!(wmi_temp_c(0), None, "0 = 未上报，绝不是 0°C");
        assert_eq!(wmi_temp_c(30), Some(30), "正常 °C 读数照常校准");
        assert_eq!(wmi_temp_c(300), Some(27), "疑似开尔文的大值减 273");
    }

    #[test]
    fn disk_serial_rejects_hex_dump_and_keeps_real_serials() {
        // 实测（SV-20260522YCII，Intel S2600WT2 + 4× NVMe）采集到的伪序列号：
        // 三星两块盘与第四块盘共享后缀 `…0025_3845_0000_000X`，是存储栈合成的 16 字节标识，
        // 不是序列号 —— 必须判为未获取，绝不能进报告误导 RMA。
        assert_eq!(
            sanitize_disk_serial("3633_5330_5240_1792_0025_3845_0000_0001."),
            None
        );
        assert_eq!(
            sanitize_disk_serial("0100_0000_0000_0000_5CD2_E4FB_E063_5651."),
            None
        );
        // 含控制字符 / 超长 → 无效
        assert_eq!(sanitize_disk_serial("ABCD\u{1}EFGH"), None);
        assert_eq!(sanitize_disk_serial(&"A".repeat(33)), None);
        assert_eq!(sanitize_disk_serial(""), None);
        assert_eq!(sanitize_disk_serial("   "), None);

        // 真实序列号必须保留（SATA 盘实测值 + 常见厂商格式）
        assert_eq!(
            sanitize_disk_serial("2CHMBDEP"),
            Some("2CHMBDEP".to_string())
        );
        // 西数带连字符格式
        assert_eq!(
            sanitize_disk_serial("WD-WX11A80T7365"),
            Some("WD-WX11A80T7365".to_string())
        );
        // 纯十六进制但无下划线分隔符（真序列号也可能是 hex 形态），应保留
        assert_eq!(
            sanitize_disk_serial("S5SVNB0M123456X"),
            Some("S5SVNB0M123456X".to_string())
        );
        // 前后空白应被 trim
        assert_eq!(
            sanitize_disk_serial("  2CHMBDEP  "),
            Some("2CHMBDEP".to_string())
        );
    }

    #[test]
    fn adapter_serial_collapses_vmd_padding_and_suffix() {
        // 捕获自 SV-20260522YCII 真机（08:45 修复版）：VMD 存储栈的 AdapterSerialNumber
        // 在真实序列号后追加空格填充 + 控制器后缀（_XXXX）。取首个空白分隔段即得盘体打印序列号。
        assert_eq!(
            sanitize_disk_serial("S63SNE0R401792      _0006"),
            Some("S63SNE0R401792".to_string())
        );
        assert_eq!(
            sanitize_disk_serial("PHAO348000GA960AGN  _0000"),
            Some("PHAO348000GA960AGN".to_string())
        );
        assert_eq!(
            sanitize_disk_serial("S612NE0X106944      _0041"),
            Some("S612NE0X106944".to_string())
        );
        // 无空格的序列号（SATA / 直连 NVMe）必须原样保留，不被误截
        assert_eq!(
            sanitize_disk_serial("2CHMBDEP"),
            Some("2CHMBDEP".to_string())
        );
        assert_eq!(
            sanitize_disk_serial("S5SVNB0M123456X"),
            Some("S5SVNB0M123456X".to_string())
        );
    }

    #[test]
    fn nvme_identify_controller_cmd_layout() {
        // 钉死 Identify Controller 命令字节布局（NVMe 1.3/1.4），防止改错导致取到脏数据。
        let cmd = build_nvme_identify_controller_cmd(0xFFFF_FFFF);
        assert_eq!(cmd.len(), 64);
        assert_eq!(cmd[0], 0x06, "opcode 必须是 Identify(0x06)");
        assert_eq!(
            &cmd[4..8],
            &0xFFFF_FFFFu32.to_le_bytes(),
            "NSID 位于 bytes 4..8"
        );
        // CDW10 位于 **bytes 40..44**（DW0~DW9 共 40 字节之后）。
        // 这里顺带钉死"PRP2 区间（bytes 32..40）必须为 0"——历史上曾把 CDW10 误写进这里。
        let dw10 = u32::from_le_bytes([cmd[40], cmd[41], cmd[42], cmd[43]]);
        assert_eq!(
            dw10 & 0xFF,
            0x01,
            "CDW10 低 8 位 CNS 必须为 1（Controller）"
        );
        assert_eq!(dw10 >> 16, 0x0000, "CNTID 必须为 0");
        assert!(cmd[1..4].iter().all(|&b| b == 0), "保留字段应为 0");
        assert!(
            cmd[8..40].iter().all(|&b| b == 0),
            "MPTR/PRP1/PRP2 区间须为 0（CDW10 不得落在此处）"
        );
        assert!(cmd[44..64].iter().all(|&b| b == 0), "CDW11+ 须为 0");
    }

    #[test]
    fn nvme_identify_serial_parsing() {
        // 序列号在 bytes 4..24，空格填充
        let mut ident = vec![0u8; 4096];
        ident[4..13].copy_from_slice(b"S63UNX0R1");
        for b in ident[13..24].iter_mut() {
            *b = b' ';
        }
        assert_eq!(
            parse_nvme_identify_serial(&ident),
            Some("S63UNX0R1".to_string())
        );

        // NUL 填充应正确截断
        let mut ident2 = vec![0u8; 4096];
        ident2[4..12].copy_from_slice(b"2CHMBDEP");
        assert_eq!(
            parse_nvme_identify_serial(&ident2),
            Some("2CHMBDEP".to_string())
        );

        // 全 0 = 内核没真返回数据，必须 None，绝不返回空串冒充"有值"
        assert_eq!(parse_nvme_identify_serial(&vec![0u8; 4096]), None);
        // 长度不足 → None
        assert_eq!(parse_nvme_identify_serial(&[0u8; 16]), None);
        // 伪值（hex dump 形态，20 字节）应被最后防线 `sanitize_disk_serial` 挡掉
        let mut ident3 = vec![0u8; 4096];
        ident3[4..24].copy_from_slice(b"3633_5330_5240_17920");
        assert_eq!(parse_nvme_identify_serial(&ident3), None);
    }
}

/// SMART 解析逻辑的单元测试：用合成 512 字节 buffer 钉死偏移量/换算/健康判定/哨兵情形。
/// 这些纯解析函数最容易写错，且真机（尤其非提权）往往采不到，必须用合成数据锁死。
#[cfg(test)]
mod smart_tests {
    use super::*;
    use winsuitex_core::model::{DiskSmart, HealthStatus, SmartSource};

    /// 造一个全 0 的 NVMe log page。
    fn zero_nvme() -> [u8; 512] {
        [0u8; 512]
    }

    /// 造一个指定字段的 NVMe log page（其余为 0）。
    fn nvme(mut set: impl FnMut(&mut [u8; 512])) -> [u8; 512] {
        let mut b = [0u8; 512];
        set(&mut b);
        b
    }

    /// 合成 `IOCTL_STORAGE_QUERY_PROPERTY` 的输出缓冲：描述符（含回显的
    /// ProtocolDataOffset）+ 512 字节 log page，用于验证缓冲定位逻辑。
    fn nvme_out(data_off: u32, log: &[u8; 512]) -> Vec<u8> {
        let desc_len = size_of::<STORAGE_PROTOCOL_DATA_DESCRIPTOR>();
        let mut v = vec![0u8; desc_len + 512];
        let mut desc = STORAGE_PROTOCOL_DATA_DESCRIPTOR::default();
        desc.ProtocolSpecificData.ProtocolDataOffset = data_off;
        desc.ProtocolSpecificData.ProtocolDataLength = 512;
        // SAFETY: 目标区间长度等于描述符大小且已分配，源为有效结构体。
        unsafe {
            core::ptr::copy_nonoverlapping(
                &desc as *const _ as *const u8,
                v.as_mut_ptr(),
                desc_len,
            );
        }
        let off = data_off as usize;
        if off + 512 <= v.len() {
            v[off..off + 512].copy_from_slice(log);
        }
        v
    }

    #[test]
    fn locate_nvme_log_uses_descriptor_offset() {
        // 防倒退：输出是 **48** 字节描述符 + log page，起点取回显的
        // ProtocolDataOffset。曾硬编码 40（只算 SPSD 漏掉 8 字节头），
        // 会让数据整体错位却仍能解析出"看起来完全正常"的假数值。
        let mut log = [0u8; 512];
        log[0x80..0x88].copy_from_slice(&12345u64.to_le_bytes());
        let out = nvme_out(48, &log);
        let got = locate_nvme_log(&out, out.len() as u32).expect("应定位到 log page");
        let s = parse_nvme_smart(got).unwrap();
        assert_eq!(s.power_on_hours, Some(12345));
    }

    #[test]
    fn locate_nvme_log_rejects_short_buffer() {
        // 描述符说 offset=48，但缓冲只有旧的 40+512=552 → 必须拒绝，
        // 绝不能退回去解析错位 8 字节的数据。
        let log = [0u8; 512];
        let out = nvme_out(48, &log);
        assert!(
            locate_nvme_log(&out[..552], 552).is_none(),
            "缓冲不足时必须返回 None，不能解析错位数据"
        );
    }

    #[test]
    fn locate_nvme_log_rejects_offset_inside_header() {
        // offset 小于描述符长度（如旧的 40）说明布局理解有误 → 拒绝而不是将就。
        let log = [0u8; 512];
        let out = nvme_out(40, &log);
        assert!(
            locate_nvme_log(&out, out.len() as u32).is_none(),
            "offset 落在描述符头部内时必须拒绝"
        );
    }

    #[test]
    fn nvme_wear_not_reported_is_unknown() {
        // 防倒退：AvailableSpare 与 Threshold 同时为 0 = 厂商未上报寿命字段。
        // 此时不能因 critical_warning==0 就判 Healthy 并算出"剩余 100%" ——
        // 那等于把"没数据"伪装成"全新盘"，比不显示危险得多。
        // 故意写入通电时间使 buffer 非全 0，绕开全 0 哨兵以精确命中这个分支。
        let buf = nvme(|b| {
            b[0x80..0x88].copy_from_slice(&5000u64.to_le_bytes());
        });
        let s = parse_nvme_smart(&buf).unwrap();
        assert_eq!(
            s.health_status,
            HealthStatus::Unknown,
            "寿命字段未上报时必须 Unknown，不能是 Healthy"
        );
        assert_eq!(s.health_percent, None, "未上报时不能算出剩余 100%");
        assert_eq!(s.percentage_used, None);
        // 但其它确实拿到了的字段照常解析。
        assert_eq!(s.power_on_hours, Some(5000));
    }

    #[test]
    fn nvme_parses_power_on_hours_and_temp() {
        let buf = nvme(|b| {
            // PowerOnHours(0x80) = 12345 小时
            b[0x80..0x88].copy_from_slice(&12345u64.to_le_bytes());
            // 复合温度 300K -> 27°C
            b[0x01..0x03].copy_from_slice(&300u16.to_le_bytes());
            // AvailableSpare=100, Threshold=10（健康）
            b[0x03] = 100;
            b[0x04] = 10;
            // PercentageUsed=12 -> 剩余 88%
            b[0x05] = 12;
            // PowerCycles(0x70)=55
            b[0x70..0x78].copy_from_slice(&55u64.to_le_bytes());
            // UnsafeShutdowns(0x90)=7
            b[0x90..0x98].copy_from_slice(&7u64.to_le_bytes());
            // DataUnitsWritten(0x30)=1000 -> 1000*512000 = 512_000_000 字节
            b[0x30..0x38].copy_from_slice(&1000u64.to_le_bytes());
        });
        let s = parse_nvme_smart(&buf).unwrap();
        assert_eq!(s.source, SmartSource::NvmeLogPage);
        assert_eq!(s.power_on_hours, Some(12345), "通电小时数应解析为 12345");
        assert_eq!(s.temperature_c, Some(27), "300K 应换算为 27°C");
        assert_eq!(s.percentage_used, Some(12));
        assert_eq!(s.health_percent, Some(88), "剩余寿命 = 100-12 = 88%");
        assert_eq!(s.health_status, HealthStatus::Healthy);
        assert_eq!(s.power_cycles, Some(55));
        assert_eq!(s.unsafe_shutdowns, Some(7));
        assert_eq!(
            s.host_written_bytes,
            Some(512_000_000),
            "DataUnitsWritten 须 ×512000（不是直接当字节）"
        );
        // 未设置的字段保持 None，绝不能变 0
        assert_eq!(s.media_errors, None);
        assert_eq!(s.unavailable_reason, None);
    }

    #[test]
    fn nvme_percentage_used_over_100_saturates() {
        let buf = nvme(|b| {
            b[0x05] = 255; // 已用 255% -> 剩余 0%
            b[0x03] = 100;
            b[0x04] = 10;
        });
        let s = parse_nvme_smart(&buf).unwrap();
        assert_eq!(s.percentage_used, Some(255));
        assert_eq!(s.health_percent, Some(0), "已用超 100 应饱和到剩余 0%");
    }

    #[test]
    fn nvme_critical_warning_triggers_critical() {
        let buf = nvme(|b| {
            b[0x00] = 0x01; // CriticalWarning bit0（可用空间低）
            b[0x03] = 100;
            b[0x04] = 10;
        });
        let s = parse_nvme_smart(&buf).unwrap();
        assert_eq!(
            s.health_status,
            HealthStatus::Critical,
            "CriticalWarning!=0 应判 Critical"
        );
    }

    #[test]
    fn nvme_high_percentage_used_triggers_warning() {
        let buf = nvme(|b| {
            b[0x05] = 95; // 已用 95% -> Warning
            b[0x03] = 100;
            b[0x04] = 10;
        });
        let s = parse_nvme_smart(&buf).unwrap();
        assert_eq!(s.health_status, HealthStatus::Warning);
        assert_eq!(s.health_percent, Some(5));
    }

    #[test]
    fn nvme_all_zero_buffer_is_not_power_on_zero() {
        // 哨兵：全 0 buffer 绝不能解析成"通电 0 小时 / 全新盘"。
        let s = parse_nvme_smart(&zero_nvme()).unwrap();
        assert_eq!(s.power_on_hours, None, "全 0 buffer 绝不能是通电 0 小时");
        assert_eq!(s.temperature_c, None);
        assert_eq!(s.health_status, HealthStatus::Unknown, "读不到绝不等于健康");
        assert!(
            s.unavailable_reason.is_some(),
            "全 0 应给出不可用的原因，而非假数据"
        );
    }

    #[test]
    fn nvme_zero_kelvin_not_negative_temp() {
        let buf = nvme(|b| {
            b[0x03] = 100;
            b[0x04] = 10;
            // 温度字段为 0 -> 未上报，不给 -273°C
        });
        let s = parse_nvme_smart(&buf).unwrap();
        assert_eq!(s.temperature_c, None, "0 开尔文应判为未上报，绝不给 -273°C");
    }

    #[test]
    fn nvme_get_log_page_cmd_layout() {
        // 纯函数，钉死 Get Log Page 命令字节布局：opcode / NSID / LID / NUMD。
        // 以广播 NSID(0xFFFFFFFF) 为样例；NSID 由入参决定，故只断言入参被如实写入。
        let cmd = build_nvme_get_log_page_cmd(0xFFFF_FFFFu32);
        assert_eq!(cmd.len(), 64, "NVMe 命令须为 64 字节");
        assert_eq!(cmd[0], 0x02, "Opcode 须为 Get Log Page(0x02)");
        // NSID 须等于入参
        assert_eq!(
            &cmd[4..8],
            &0xFFFF_FFFFu32.to_le_bytes(),
            "NSID 须等于传入的命名空间"
        );
        // CDW10: LID=0x02 (bits 7:0), NUMD=127 (bits 31:16) → 128 dwords = 512 字节
        // 位于 **bytes 40..44**；bytes 32..40 是 PRP2，须保持为 0。
        let dw10 = u32::from_le_bytes([cmd[40], cmd[41], cmd[42], cmd[43]]);
        assert_eq!(dw10 & 0xFF, 0x02, "CDW10 低 8 位 LID 须为 0x02");
        assert_eq!(
            (dw10 >> 16) & 0xFFFF,
            127,
            "CDW10 高位 NUMD 须为 127（128 dwords = 512B）"
        );
        // PRP/MPTR/CID 不填，其余须全 0
        assert!(cmd[1..4].iter().all(|&b| b == 0), "fused/CID 须为 0");
        assert!(
            cmd[8..40].iter().all(|&b| b == 0),
            "MPTR/PRP1/PRP2 须为 0（CDW10 不得落在此处）"
        );
        assert!(cmd[44..64].iter().all(|&b| b == 0), "CDW11+ 须为 0");
    }

    // ---- ATA ----

    fn ata(mut attrs: impl FnMut(&mut [u8; 512])) -> [u8; 512] {
        let mut b = [0u8; 512];
        attrs(&mut b);
        b
    }

    /// 在属性表偏移处写一个属性条目（offset 2 起，每条 12 字节）。
    fn set_attr(b: &mut [u8; 512], slot: usize, id: u8, raw: u64) {
        let off = 2 + slot * 12;
        b[off] = id;
        b[off + 3] = 100; // Normalized
        b[off + 4] = 100; // Worst
        let r = raw.to_le_bytes();
        b[off + 5..off + 11].copy_from_slice(&r[..6]);
    }

    #[test]
    fn ata_parses_power_on_hours_and_realloc() {
        let buf = ata(|b| {
            set_attr(b, 0, 0x09, 12345); // Power_On_Hours
            set_attr(b, 1, 0x05, 3); // Reallocated sectors
        });
        let s = parse_ata_smart(&buf, false).unwrap();
        assert_eq!(s.source, SmartSource::AtaSmart);
        assert_eq!(s.power_on_hours, Some(12345));
        assert_eq!(s.reallocated_sectors, Some(3));
        assert_eq!(
            s.health_status,
            HealthStatus::Warning,
            "有重映射扇区应 Warning"
        );
        // ATA 不猜剩余寿命
        assert_eq!(s.health_percent, None);
        assert_eq!(s.percentage_used, None);
    }

    #[test]
    fn ata_parses_power_cycle_count() {
        // 0x0C Power Cycle Count 是 ATA SMART **标准属性**；此前误注为"无标准字段"未解析，
        // 与温度同属"数据在属性表里却没取"。通电次数对二手盘 / 网吧盘估值有参考价值。
        let buf = ata(|b| {
            set_attr(b, 0, 0x09, 12345); // Power_On_Hours
            set_attr(b, 1, 0x0C, 678); // Power_Cycle_Count
        });
        let s = parse_ata_smart(&buf, false).unwrap();
        assert_eq!(s.power_cycles, Some(678), "0x0C 应解析为通电次数");
        assert_eq!(s.power_on_hours, Some(12345));
    }

    #[test]
    fn ata_zero_power_cycles_treated_as_unavailable() {
        // 纪律 #1：真 0 按"没拿到"处理，绝不显示 0（SMART 原始 0 常表示属性未更新/不支持）。
        let buf = ata(|b| {
            set_attr(b, 0, 0x09, 100);
            set_attr(b, 1, 0x0C, 0);
        });
        let s = parse_ata_smart(&buf, false).unwrap();
        assert_eq!(s.power_cycles, None, "0 次应视为未获取，不显示 0");
    }

    #[test]
    fn ata_pending_sector_warning() {
        let buf = ata(|b| {
            set_attr(b, 0, 0x09, 5000);
            set_attr(b, 1, 0xC5, 1); // 待映射扇区
        });
        let s = parse_ata_smart(&buf, false).unwrap();
        assert_eq!(s.pending_sectors, Some(1));
        assert_eq!(s.health_status, HealthStatus::Warning);
    }

    #[test]
    fn ata_overall_failing_is_critical() {
        let buf = ata(|b| {
            set_attr(b, 0, 0x09, 100);
        });
        // 即便属性都好，整机预测失效签名命中也应 Critical
        let s = parse_ata_smart(&buf, true).unwrap();
        assert_eq!(s.health_status, HealthStatus::Critical);
    }

    #[test]
    fn ata_all_zero_buffer_not_healthy() {
        // 哨兵：全 0 属性表（含全 0 buffer）绝不能解析成"健康"。
        let s = parse_ata_smart(&[0u8; 512], false).unwrap();
        assert_eq!(
            s.power_on_hours, None,
            "全 0 buffer 无 0x09 属性 -> 通电 None"
        );
        assert_eq!(s.health_status, HealthStatus::Unknown, "读不到绝不等于健康");
        assert!(s.unavailable_reason.is_some());
    }

    #[test]
    fn ata_real_zero_hours_not_shown_as_zero() {
        // 即便真的解析到 0x09 且 raw=0，也按"没拿到"处理（纪律 #1）。
        let buf = ata(|b| {
            set_attr(b, 0, 0x09, 0);
        });
        let s = parse_ata_smart(&buf, false).unwrap();
        assert_eq!(s.power_on_hours, None, "真 0 小时也不显示 0");
    }

    // ---- 接口分流 ----

    #[test]
    fn dispatch_usb_tries_wmi_first() {
        // 接口分流（需求 #4）：USB 等 unsupported 分支先试 WMI 可靠性计数器兜底。
        // WMI 有条目则走 StorageReliabilityCounter（source 非 None、无原因）；
        // 无条目则回退到原 USB 不可用例（source=None，reason 含 "USB"）。
        // 沙箱/QEMU 下 PhysicalDrive0 通常确有 WMI 条目，故实际多为前者。
        let s = query_smart(r"\\.\PhysicalDrive0", &Some("USB".to_string())).unwrap();
        if s.source == SmartSource::StorageReliabilityCounter {
            assert!(
                s.unavailable_reason.is_none(),
                "WMI 兜底成功则不应有不可用原因"
            );
        } else {
            assert_eq!(s.source, SmartSource::None);
            assert!(
                s.unavailable_reason.unwrap().contains("USB"),
                "无 WMI 条目才回退到 USB 不可用例"
            );
        }
    }

    #[test]
    fn dispatch_unknown_iface_is_none() {
        // 接口信息缺失：无从判断，返回 None（不显示任何健康行）。
        assert!(query_smart(r"\\.\PhysicalDrive0", &None).is_none());
    }

    #[test]
    fn disk_smart_struct_default_is_unknown() {
        // 默认健康态必须是 Unknown（不是 Healthy），序列化旧 JSON 才安全。
        let s = DiskSmart::default();
        assert_eq!(s.health_status, HealthStatus::Unknown);
        assert_eq!(s.source, SmartSource::None);
    }

    // ---- WMI 可靠性计数器兜底（纯逻辑，不依赖真实 WMI）----

    /// 构造一条 WMI 可靠性信息，便于单元测试纯转换逻辑。
    fn reliability(
        temperature_c: Option<i16>,
        wear: Option<u8>,
        health_status: HealthStatus,
        power_on_hours: Option<u32>,
    ) -> ReliabilityInfo {
        ReliabilityInfo {
            temperature_c,
            wear,
            health_status,
            bus_type: None,
            model: None,
            serial: None,
            adapter_serial: None,
            power_on_hours,
        }
    }

    #[test]
    fn wmi_health_status_three_levels() {
        // 1) WMI 显式报健康态（非 Unknown）：直接采用，无需温度佐证。
        let s = smart_from_reliability_info(&reliability(None, None, HealthStatus::Healthy, None));
        assert_eq!(s.health_status, HealthStatus::Healthy);

        // 2) WMI 未知 + 确有温度读数：降级判 Healthy（"有读数=可观测"）。
        let s =
            smart_from_reliability_info(&reliability(Some(32), None, HealthStatus::Unknown, None));
        assert_eq!(s.health_status, HealthStatus::Healthy);

        // 3) WMI 未知 + 无温度：必须是 Unknown，绝不能默认 Healthy。
        let s = smart_from_reliability_info(&reliability(None, None, HealthStatus::Unknown, None));
        assert_eq!(s.health_status, HealthStatus::Unknown, "读不到绝不等于健康");
    }

    #[test]
    fn wmi_health_percent_and_percentage_used_conversion() {
        // wear=12 -> 剩余 88%，已用 12%。
        let s = smart_from_reliability_info(&reliability(
            Some(33),
            Some(12),
            HealthStatus::Unknown,
            Some(1150),
        ));
        assert_eq!(s.percentage_used, Some(12));
        assert_eq!(s.health_percent, Some(88), "剩余寿命 = 100 - 已用");
        assert_eq!(s.power_on_hours, Some(1150), "R730/PERC 真机应补回通电小时");
        assert_eq!(s.source, SmartSource::StorageReliabilityCounter);

        // wear 超 100（如 255）：剩余寿命饱和到 0%。
        let s =
            smart_from_reliability_info(&reliability(None, Some(255), HealthStatus::Unknown, None));
        assert_eq!(s.percentage_used, Some(255));
        assert_eq!(s.health_percent, Some(0), "已用超 100 须饱和到剩余 0%");
    }

    #[test]
    fn wmi_zero_wear_and_zero_poh_treated_as_none() {
        // 纪律 #1：wear 真 0、power_on_hours 真 0 都按"没拿到"处理，绝不给 0 / 100。
        let s = smart_from_reliability_info(&reliability(
            None,
            Some(0),
            HealthStatus::Warning,
            Some(0),
        ));
        assert_eq!(s.percentage_used, None, "wear=0 不能当已用 0%");
        assert_eq!(s.health_percent, None, "wear=0 不能算剩余 100%");
        assert_eq!(s.power_on_hours, None, "真 0 小时也不显示 0");
        // 但 WMI 显式报的 Warning 仍应保留（健康态来自 WMI，不在此过滤）。
        assert_eq!(s.health_status, HealthStatus::Warning);

        // 全缺字段：健康态默认 Unknown，不瞎猜。
        let s = smart_from_reliability_info(&reliability(None, None, HealthStatus::Unknown, None));
        assert_eq!(s.health_status, HealthStatus::Unknown);
        assert_eq!(s.health_percent, None);
    }
}
