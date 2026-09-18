//! 键盘 / 鼠标采集（Win32 Raw Input + HID）。
//!
//! 为什么用 Raw Input 而不是 SetupAPI：Raw Input 直接枚举「系统已注册为鼠标/键盘」的设备，
//! 天然按 `RIM_TYPE*` 分流，且能拿到真实 HID 设备接口路径，可再用 `HidD_*` 取产品串 / VID / PID，
//! 准确率高于只靠 SetupAPI 类名匹配（后者会把键盘背光的 HID、手柄的 HID 一起捞进来）。
//!
//! 准确率边界（与 `peripheral_db` 一致）：国产电竞键鼠大量共用 ODM VID，单靠 VID 无法定品牌，
//! 因此型号一律优先用运行时 `HidD_GetProductString` 的真实串；VID 只用于回退与 `peripheral_db` 品牌推断。

use std::ffi::c_void;

use windows::core::PCWSTR;
use windows::Win32::Devices::HumanInterfaceDevice::{
    HidD_GetManufacturerString, HidD_GetProductString,
};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::UI::Input::{
    GetRawInputDeviceInfoW, GetRawInputDeviceList, RAWINPUTDEVICELIST, RIDI_DEVICEINFO,
    RIDI_DEVICENAME, RID_DEVICE_INFO, RIM_TYPEHID, RIM_TYPEKEYBOARD, RIM_TYPEMOUSE,
};

use winsuitex_core::error::{Error, Result};
use winsuitex_core::model::{InputDevice, InputKind};
use winsuitex_core::traits::{CollectContext, HardwareSource};

use crate::peripheral_db;
use crate::to_wide;
use crate::wide_to_string;

/// 键盘 / 鼠标数据源。
pub struct HidSource;

impl HardwareSource for HidSource {
    fn name(&self) -> &'static str {
        "hid"
    }

    fn collect(&self, ctx: &mut CollectContext) -> Result<()> {
        match collect_input() {
            Ok(devices) => {
                ctx.set_input(devices);
                Ok(())
            }
            Err(e) => Err(Error::collect("hid", e)),
        }
    }
}

/// 枚举系统鼠标 / 键盘，返回识别结果。
fn collect_input() -> std::result::Result<Vec<InputDevice>, String> {
    // 1) 先取设备数量：pRawInputDeviceList 传 None，返回设备数（失败返回 u32::MAX）。
    let mut num: u32 = 0;
    let cb = std::mem::size_of::<RAWINPUTDEVICELIST>() as u32;
    // SAFETY: prawinputdevicelist=NULL 只查询数量；num 持有有效 u32；cbSize 已填结构体大小。
    let count = unsafe { GetRawInputDeviceList(None, &mut num, cb) };
    if count == u32::MAX {
        return Err("GetRawInputDeviceList(查询数量) 失败".to_string());
    }
    if num == 0 {
        return Ok(Vec::new());
    }

    // 2) 分配缓冲，二次调用取真实列表。
    let mut list = vec![RAWINPUTDEVICELIST::default(); num as usize];
    // SAFETY: list 持有 num 个有效元素；num 在调用后会被改写为实际写入数。
    let got = unsafe { GetRawInputDeviceList(Some(list.as_mut_ptr()), &mut num, cb) };
    if got == u32::MAX {
        return Err("GetRawInputDeviceList(取列表) 失败".to_string());
    }

    let mut raw: Vec<InputDevice> = Vec::new();
    for entry in &list[..num as usize] {
        if let Some(dev) = build_device(entry.hDevice) {
            raw.push(dev);
        }
    }
    // 合并键鼠一体设备（同一 VID:PID 同时被识别为鼠标与键盘 → 合并为一行）。
    Ok(merge_input_devices(raw))
}

/// 合并同一物理输入设备的多份 HID 集合。
///
/// 背景：键鼠一体设备（带指点设备的键盘、无线键鼠套装共用一个接收器）会被 Raw Input
/// 枚举成**两个集合**——一个鼠标、一个键盘——但二者共享同一个 VID:PID。若直接逐条
/// 渲染，会被数成"2 鼠标 + 1 键盘"，实际只有「1 键盘(自带指点) + 1 鼠标」。
///
/// 真机案例：某机 A4Tech `09DA:FA90` 的键盘集合与指点集合共用同一 VID:PID，旧逻辑
/// 数成 2 鼠标 + 1 键盘，实际 1 键盘(自带指点) + 1 鼠标(罗技)。
///
/// 规则：
/// - 同一 VID:PID 既报鼠标又报键盘 → 合并为一行，按两套集合的产品串判定主类型（见 [`merge_combo`]）；
/// - 同 VID:PID 同 kind（如两台同型号鼠标）→ 视为同一台，跳过（沿用原设计：同型号不重复计数）；
/// - VID/PID 任一缺失 → 退化为 `(kind, name, vid, pid)` 去重，避免 `(None, None)` 误并所有无名设备。
pub fn merge_input_devices(devices: Vec<InputDevice>) -> Vec<InputDevice> {
    let mut out: Vec<InputDevice> = Vec::with_capacity(devices.len());
    for dev in devices {
        if dev.vid.is_some() && dev.pid.is_some() {
            let vkey = (dev.vid.clone(), dev.pid.clone());
            if let Some(existing) = out
                .iter_mut()
                .find(|d| (d.vid.clone(), d.pid.clone()) == vkey)
            {
                // 鼠标 + 键盘 同 VID:PID → 键鼠一体/套装设备；按两套集合的产品串判定主类型与显示名。
                if existing.kind != dev.kind {
                    *existing = merge_combo(std::mem::take(existing), dev);
                } else if existing.name == "未知输入设备" {
                    // 同 kind 同 VID:PID（如两台同型号鼠标）→ 视为同一台；型号为空时用更具体的补上。
                    existing.name = dev.name.clone();
                }
                continue;
            }
        } else {
            // 无 VID/PID：用原 (kind, name, vid, pid) 去重，避免 (None,None) 误并。
            let key = (dev.kind, dev.name.clone(), dev.vid.clone(), dev.pid.clone());
            if out
                .iter()
                .any(|d| (d.kind, d.name.clone(), d.vid.clone(), d.pid.clone()) == key)
            {
                continue;
            }
        }
        out.push(dev);
    }
    out
}

/// 合并同一 VID:PID 的「鼠标 + 键盘」两个 HID 集合，返回最终呈现的一条记录。
///
/// 主类型（键盘 / 鼠标 / 键盘/鼠标）判定见 [`combo_primary_kind`]。显示名优先用更具体的
/// 那一个（非通用名），避免被「未知输入设备 / HID 兼容鼠标」之类的空壳名覆盖真型号。
fn merge_combo(a: InputDevice, b: InputDevice) -> InputDevice {
    let (keyboard, mouse) = match (a.kind, b.kind) {
        (InputKind::Keyboard, InputKind::Mouse) => (a, b),
        (InputKind::Mouse, InputKind::Keyboard) => (b, a),
        // 不应发生（combo 必为一鼠一键），兜底保留 a。
        _ => return a,
    };
    let primary = combo_primary_kind(&keyboard, &mouse);
    let mut merged = keyboard;
    merged.kind = primary;
    if is_generic_name(&merged.name) && !is_generic_name(&mouse.name) {
        merged.name = mouse.name.clone();
    }
    merged
}

/// 由「键盘集合 + 鼠标集合」两个显示名判定键鼠一体设备的主类型。
///
/// - 双方都有具体且不同的型号名（如罗技套装两半各有真名）→ 视为真·键鼠套装，
///   保留 `Combo`（键盘/鼠标）让盘点者自行判断；
/// - 仅键盘集合有具体名（最常见：带指点设备的键盘，OS 把其指点部分当鼠标枚举）→ 主类型=键盘；
/// - 仅鼠标集合有具体名（键盘集合是通用名）→ 主类型=鼠标；
/// - 两者都通用/相同（产品串取不到，如 A4Tech 09DA:FA90 两集合都退化成「A4Tech (VID:PID)」）
///   → 默认主类型=键盘：带指点设备的键盘是绝对主流，也更贴合「1 键盘 + 1 鼠标」的实机盘点口径。
fn combo_primary_kind(keyboard: &InputDevice, mouse: &InputDevice) -> InputKind {
    let kb_specific = !is_generic_name(&keyboard.name);
    let mou_specific = !is_generic_name(&mouse.name);
    if kb_specific && mou_specific && keyboard.name != mouse.name {
        return InputKind::Combo;
    }
    if kb_specific {
        InputKind::Keyboard
    } else if mou_specific {
        InputKind::Mouse
    } else {
        InputKind::Keyboard
    }
}

/// 显示名是否为「通用名」（无法据此判定设备具体型号，宜被更具体的名覆盖）。
fn is_generic_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    const GENERIC: &[&str] = &[
        "未知输入设备",
        "hid-compliant",
        "hid 兼容",
        "hid兼容",
        "usb 输入设备",
        "usb input",
        "鼠标",
        "键盘",
        "标准",
        "兼容",
        "generic",
        "pointer",
        "pointing",
        "device",
    ];
    n.is_empty() || GENERIC.iter().any(|g| n.contains(g)) || n.contains("hid")
}

/// 由单个 Raw Input 设备句柄构造 `InputDevice`（取不到则 `None`）。
///
/// 仅做 FFI 取数（设备名 + 设备信息），纯逻辑交给 [`resolve_device`]。
fn build_device(h: HANDLE) -> Option<InputDevice> {
    let name = get_device_name(h)?;
    let info = get_device_info(h);
    resolve_device(&name, info.as_ref())
}

/// 由设备名 + `RID_DEVICE_INFO` 解析出 `InputDevice`（纯逻辑，可单测）。
///
/// 型号解析优先级：运行时 `HidD_GetProductString` 产品串 >
/// `peripheral_db::model_for_pid` 离线种子（VID+PID→真实型号）>
/// 品牌 + VID:PID > 兜底。产品串需要再开一次 HID 句柄读取，失败时回退到种子表。
fn resolve_device(name: &str, info: Option<&RID_DEVICE_INFO>) -> Option<InputDevice> {
    // 判定类型：鼠标 / 键盘 / 其它（手柄、传感器等，丢弃）。
    let kind = classify(info)?;

    // VID / PID：优先 HidD_GetAttributes，回退名称解析。
    let (attr_vid, attr_pid) = match info {
        Some(i) if i.dwType == RIM_TYPEHID => {
            let h = unsafe { i.Anonymous.hid };
            (Some(h.dwVendorId), Some(h.dwProductId))
        }
        _ => (None, None),
    };
    let (name_vid, name_pid) = parse_vid_pid(name);

    let vid = attr_vid.map(|v| format!("{v:04X}")).or(name_vid);
    let pid = attr_pid.map(|v| format!("{v:04X}")).or(name_pid);

    // 用设备名打开 HID 句柄，取产品串 / 厂商串（最准的型号来源）。
    let handle_path = name.replace('#', "\\");
    let product = open_hid(&handle_path).and_then(|guard| get_hid_product_string(guard.0));
    let manufacturer_hid =
        open_hid(&handle_path).and_then(|guard| get_hid_manufacturer_string(guard.0));

    // 厂商：HID 厂商串 > peripheral_db 由 VID 推断。
    // 注意：由 VID 推断出的英文品牌 key 要再过一遍 `brand_display` 翻译成中文，
    // 否则键鼠只会输出英文（如 SEMICO / SiGma Micro）。HID 自身提供的厂商串
    // 不走此路径，避免改动原始标识。
    let manufacturer = manufacturer_hid.or_else(|| {
        vid.as_ref().and_then(|v| {
            peripheral_db::brand_for_vid(v)
                .map(|s| peripheral_db::brand_display(s).to_string())
        })
    });

    // 离线种子：VID+PID → 真实型号（来自 Drives 各厂商驱动 INF）。
    let seed_model = match (&vid, &pid) {
        (Some(v), Some(p)) => peripheral_db::model_for_pid(v, p).map(|s| s.to_string()),
        _ => None,
    };

    // 型号优先级：运行时产品串 > PID_MODEL 种子 > 品牌+VID/PID > 兜底。
    let display_name = match product {
        Some(p) if !p.is_empty() => p,
        _ => match seed_model {
            Some(m) => m,
            None => match (&manufacturer, &vid, &pid) {
                (Some(m), Some(v), Some(p)) => format!("{m} ({v}:{p})"),
                (Some(m), _, _) => m.clone(),
                // 品牌未知但 VID:PID 拿到了 —— 不能丢。真机实测有键盘 VID 1A2C:6004
                // 既没有 HID 产品串、品牌表也没收录，旧代码显示"未知输入设备"，
                // 运维拿到手无从下手。带上 VID:PID 至少可以直接查型号。
                (None, Some(v), Some(p)) => format!("未知输入设备 ({v}:{p})"),
                _ => "未知输入设备".to_string(),
            },
        },
    };

    // 蓝牙设备：设备路径含 `BTHENUM`（蓝牙枚举根键），无线键鼠/耳机单列标记。
    let is_bluetooth = is_bluetooth_device(name);

    Some(InputDevice {
        name: display_name,
        kind,
        vid,
        pid,
        manufacturer,
        is_bluetooth,
        location: None,
    })
}

/// 由 `RID_DEVICE_INFO` 判定是否为鼠标 / 键盘。
fn classify(info: Option<&RID_DEVICE_INFO>) -> Option<InputKind> {
    let info = info?;
    match info.dwType {
        RIM_TYPEMOUSE => Some(InputKind::Mouse),
        RIM_TYPEKEYBOARD => Some(InputKind::Keyboard),
        RIM_TYPEHID => {
            // 仅收「Generic Desktop」页下的鼠标(0x02)/键盘(0x06)，过滤手柄、传感器等。
            let hid = unsafe { info.Anonymous.hid };
            if hid.usUsagePage == 0x01 {
                match hid.usUsage {
                    0x02 => Some(InputKind::Mouse),
                    0x06 => Some(InputKind::Keyboard),
                    _ => None,
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

/// `RIDI_DEVICENAME` 查询所得 `size` 对应的 **u16 缓冲区元素数**。
///
/// MSDN 明示该命令下 `*pcbSize` 是**字符数**而非字节数，因此直接 `size + 1`
/// （+1 留给 NUL 终止符）。历史 bug 曾写成 `size / 2 + 1`（误当字节），
/// 只分到所需的一半，而第二次调用又把 `size` 原样传回作字符容量，
/// 导致 API 写入约 `2 × size` 字节 → 堆损坏（0xC0000374）。
/// 单独成函数是为了让这条不变量可单测，防止回归。
fn device_name_buf_len(size_chars: u32) -> usize {
    size_chars as usize + 1
}

/// 取设备接口路径（形如 `\\?\HID#VID_xxxx&PID_yyyy...`）。
fn get_device_name(h: HANDLE) -> Option<String> {
    let mut size: u32 = 0;
    // SAFETY: pData=None 只查询所需字节数；size 持有有效 u32。
    let _ = unsafe { GetRawInputDeviceInfoW(Some(h), RIDI_DEVICENAME, None, &mut size) };
    if size == 0 {
        return None;
    }
    // 【关键】RIDI_DEVICENAME 的 `size` 单位是**字符数**，不是字节数。
    // MSDN 原文：「For this uiCommand only, the value in *pcbSize is the character
    // count (not the byte count).」
    // 因此缓冲区必须按 `size + 1` 个 u16 分配（多 1 个容纳 NUL），**不能**除以 2。
    // 曾经写成 `(size / 2) + 1`：只分到需要的一半，而第二次调用又把 `size` 原样传回，
    // API 认为可写 `size` 个字符（= 2×size 字节），实际只有 size+2 字节 → 堆损坏
    // （真机表现：进程被 0xC0000374 STATUS_HEAP_CORRUPTION 直接终止，无 panic 消息）。
    // 只在「本机确实有 HID 设备」时才触发（无设备时上面已早退），故本地测试从未覆盖。
    // 尾部留 8 个哨兵单元：若 API 写了比声明容量更多的数据，这里会被改写。
    // 宁可当场发现并放弃该设备，也不要让越界写悄悄踩坏堆（堆损坏要到很久之后
    // 才被检测到，排查成本极高 —— 这次的 0xC0000374 就是代价）。
    const CANARY: u16 = 0xA5A5;
    let declared = size as usize;
    let mut buf = vec![0u16; device_name_buf_len(size) + 8];
    for slot in &mut buf[declared + 1..] {
        *slot = CANARY;
    }
    // SAFETY: 第二次调用 pData 指向 declared+9 个 u16 的有效缓冲；`size` 保持为第一次
    // 查询得到的字符容量（API 对 RIDI_DEVICENAME 按字符解读），写入含 NUL 的串。
    let ret = unsafe {
        GetRawInputDeviceInfoW(
            Some(h),
            RIDI_DEVICENAME,
            Some(buf.as_mut_ptr() as *mut c_void),
            &mut size,
        )
    };
    if ret == u32::MAX {
        return None;
    }
    // 注意：调用后 `size` 可能被 API 改写，因此一律用调用前记录的 `declared` 做边界判断。
    if buf[declared + 1..].iter().any(|&c| c != CANARY) {
        eprintln!(
            "[hid] GetRawInputDeviceInfoW 写越界：声明 {} 字符，哨兵被改写，丢弃该设备",
            declared
        );
        return None;
    }
    wide_to_string(&buf)
}

/// 取设备信息（含类型与 HID usage）。
fn get_device_info(h: HANDLE) -> Option<RID_DEVICE_INFO> {
    let mut info = RID_DEVICE_INFO {
        cbSize: std::mem::size_of::<RID_DEVICE_INFO>() as u32,
        ..Default::default()
    };
    let mut size = info.cbSize;
    // SAFETY: pData 指向有效 RID_DEVICE_INFO（cbSize 已设）；pcbsize 传入结构体大小。
    let ret = unsafe {
        GetRawInputDeviceInfoW(
            Some(h),
            RIDI_DEVICEINFO,
            Some(&mut info as *mut _ as *mut c_void),
            &mut size,
        )
    };
    if ret == u32::MAX {
        None
    } else {
        Some(info)
    }
}

/// 判断设备是否为蓝牙设备：原始输入设备路径含 `BTHENUM`（蓝牙枚举根键）。
///
/// 例：`\\?\BTHENUM\{00001124-...}_VID_xxxx&PID_yyyy\...`。大小写不敏感。
fn is_bluetooth_device(name: &str) -> bool {
    name.to_ascii_uppercase().contains("BTHENUM")
}

/// 从设备名解析 `VID_xxxx` / `PID_yyyy`（大小写不敏感）。
fn parse_vid_pid(name: &str) -> (Option<String>, Option<String>) {
    let name = name.to_ascii_uppercase();
    let vid = name.find("VID_").and_then(|i| {
        let s = &name[i + 4..];
        let end = s.find(|c: char| !c.is_ascii_hexdigit())?;
        let code = &s[..end];
        if code.len() == 4 {
            Some(code.to_ascii_uppercase())
        } else {
            None
        }
    });
    let pid = name.find("PID_").and_then(|i| {
        let s = &name[i + 4..];
        let end = s.find(|c: char| !c.is_ascii_hexdigit())?;
        let code = &s[..end];
        if code.len() == 4 {
            Some(code.to_ascii_uppercase())
        } else {
            None
        }
    });
    (vid, pid)
}

/// 用设备接口路径打开 HID 句柄（RAII 守卫，离开作用域自动 `CloseHandle`）。
///
/// 访问方式两级回退，保证「绝不丢设备」：
/// 1. `GENERIC_READ` —— `HidD_GetProductString` 底层走
///    `DeviceIoControl(IOCTL_HID_GET_*_STRING)`，绝大多数真实 HID 设备要求此权限；
///    用 0 打开虽能拿到句柄，但取产品串会被拒绝并返回空（2026-09-15 真机盘点：型号串全空）。
/// 2. `access=0` 回退 —— 若设备正被其它进程独占、拒绝 `GENERIC_READ` 打开，
///    退一步仍拿到句柄（仅取不到产品串），至少认得出 VID:PID/品牌，不整条丢失。
fn open_hid(path: &str) -> Option<HandleGuard> {
    let w = to_wide(path);
    // SAFETY: 仅打开一个已存在的设备接口，FILE_SHARE_READ|WRITE 允许其它程序同时访问，不独占设备。
    let pcwstr = PCWSTR::from_raw(w.as_ptr());
    for access in [GENERIC_READ.0, 0u32] {
        let h = unsafe {
            CreateFileW(
                pcwstr,
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        };
        if let Ok(h) = h {
            if h.0 != HANDLE::default().0 {
                return Some(HandleGuard(h));
            }
        }
    }
    None
}

/// 取 HID 产品串（真实型号，UTF-16 缓冲）。
fn get_hid_product_string(h: HANDLE) -> Option<String> {
    let mut buf = vec![0u16; 128];
    // SAFETY: buffer 持有 128 个 u16（256 字节）有效内存；bufferlength 以字节计。
    let ok = unsafe {
        HidD_GetProductString(h, buf.as_mut_ptr() as *mut c_void, (buf.len() * 2) as u32)
    };
    if ok {
        wide_to_string(&buf)
    } else {
        None
    }
}

/// 取 HID 厂商串。
fn get_hid_manufacturer_string(h: HANDLE) -> Option<String> {
    let mut buf = vec![0u16; 128];
    // SAFETY: 同上。
    let ok = unsafe {
        HidD_GetManufacturerString(h, buf.as_mut_ptr() as *mut c_void, (buf.len() * 2) as u32)
    };
    if ok {
        wide_to_string(&buf)
    } else {
        None
    }
}

/// `HANDLE` RAII 守卫：离开作用域即 `CloseHandle`。
struct HandleGuard(HANDLE);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        // SAFETY: self.0 来自成功的 CreateFileW，是有效句柄。
        if !self.0 .0.is_null() {
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Input::{
        RID_DEVICE_INFO, RID_DEVICE_INFO_0, RID_DEVICE_INFO_HID, RIM_TYPEMOUSE,
    };

    #[test]
    fn parse_vid_pid_basic() {
        assert_eq!(
            parse_vid_pid(r"\\?\HID#VID_046D&PID_C52B&MI_00#8&1a2b#&{guid}"),
            (Some("046D".to_string()), Some("C52B".to_string()))
        );
        assert_eq!(
            parse_vid_pid(r"\\?\HID#vid_258a&pid_1007#abc"),
            (Some("258A".to_string()), Some("1007".to_string()))
        );
        assert_eq!(parse_vid_pid("no vid here"), (None, None));
    }

    #[test]
    fn build_device_uses_pid_model_seed_without_product_string() {
        // 构造一个鼠标设备名（含 VID_1038&PID_0472），无运行时产品串。
        // open_hid 在测试主机上会失败，因此走到 PID_MODEL 种子兜底。
        let name = r"\\?\HID#VID_1038&PID_0472&MI_00#8&1a2b#&{guid}";
        let info = RID_DEVICE_INFO {
            cbSize: std::mem::size_of::<RID_DEVICE_INFO>() as u32,
            dwType: RIM_TYPEMOUSE,
            Anonymous: RID_DEVICE_INFO_0 {
                mouse: windows::Win32::UI::Input::RID_DEVICE_INFO_MOUSE::default(),
            },
        };
        let dev = resolve_device(name, Some(&info));
        assert!(dev.is_some(), "鼠标设备应被识别");
        let dev = dev.unwrap();
        assert_eq!(dev.kind, InputKind::Mouse);
        // 种子命中：型号应为真实名，而非 "HID 兼容鼠标" 之类的通用名。
        assert_eq!(dev.name, "SteelSeries Rival 150 Gaming Mouse");
        assert_eq!(dev.vid.as_deref(), Some("1038"));
        assert_eq!(dev.pid.as_deref(), Some("0472"));
    }

    #[test]
    fn bluetooth_detection() {
        assert!(is_bluetooth_device(
            r"\\?\BTHENUM\{00001124-0000-1000-8000-00805f9b34fb}_VID_046D&PID_B010\..."
        ));
        // 大小写不敏感。
        assert!(is_bluetooth_device(r"\\?\bthenum\{...}\VID_1234&PID_5678"));
        // 普通 USB HID 不算蓝牙。
        assert!(!is_bluetooth_device(
            r"\\?\HID#VID_046D&PID_C52B&MI_00#8&1a2b#&{guid}"
        ));
    }

    #[test]
    fn classify_hid_usage() {
        // 构造一个 HID 鼠标（Generic Desktop / Mouse）。
        let hid = RID_DEVICE_INFO_HID {
            dwVendorId: 0,
            dwProductId: 0,
            dwVersionNumber: 0,
            usUsagePage: 0x01,
            usUsage: 0x02,
        };
        let mut info = RID_DEVICE_INFO {
            cbSize: std::mem::size_of::<RID_DEVICE_INFO>() as u32,
            dwType: RIM_TYPEHID,
            Anonymous: RID_DEVICE_INFO_0 { hid },
        };
        assert_eq!(classify(Some(&info)), Some(InputKind::Mouse));

        // HID 键盘。
        info.Anonymous.hid.usUsage = 0x06;
        assert_eq!(classify(Some(&info)), Some(InputKind::Keyboard));

        // HID 手柄（Generic Desktop / Game Pad 0x05）—— 丢弃。
        info.Anonymous.hid.usUsage = 0x05;
        assert_eq!(classify(Some(&info)), None);

        // 非 HID 鼠标 / 键盘直判。
        info.dwType = RIM_TYPEMOUSE;
        assert_eq!(classify(Some(&info)), Some(InputKind::Mouse));
        info.dwType = RIM_TYPEKEYBOARD;
        assert_eq!(classify(Some(&info)), Some(InputKind::Keyboard));
    }

    #[test]
    fn device_name_buffer_is_sized_in_characters_not_bytes() {
        // RIDI_DEVICENAME 的 size 单位是字符数：90 个字符的名字必须有 >= 90 个 u16
        // （外加 NUL）。若退化成按字节计算会得到 46，API 按 90 字符写入即越界。
        assert_eq!(device_name_buf_len(90), 91);
        assert!(device_name_buf_len(90) >= 90);
        // 真实设备接口路径长度参考：约 90~110 字符。
        assert!(device_name_buf_len(110) >= 110);
        // 边界：0 也要能安全分配（调用方已提前拦截 size==0，这里只保证不 panic）。
        assert_eq!(device_name_buf_len(0), 1);
    }

    #[test]
    fn collect_runs_without_panic() {
        let src = HidSource;
        let mut ctx = CollectContext::new();
        let r = src.collect(&mut ctx);
        assert!(r.is_ok(), "hid collect() 返回 Err: {:?}", r.err());

        let (info, _volumes) = ctx.into_parts();
        for d in &info.input_devices {
            eprintln!(
                "INPUT kind={:?} name={:?} vid={:?} pid={:?} mfr={:?}",
                d.kind, d.name, d.vid, d.pid, d.manufacturer
            );
        }
        eprintln!("输入设备总数: {}", info.input_devices.len());
    }

    #[test]
    fn combo_device_merges_mouse_and_keyboard() {
        // 真实案例：A4Tech 09DA:FA90 的键盘集合与指点集合共用同一 VID:PID。
        // 旧逻辑会数成「2 鼠标 + 1 键盘」，实际只有 1 键盘(自带指点) + 1 鼠标(罗技)。
        let devices = vec![
            InputDevice {
                name: "A4Tech USB Mouse".into(),
                kind: InputKind::Mouse,
                vid: Some("09DA".into()),
                pid: Some("FA90".into()),
                manufacturer: Some("A4Tech".into()),
                is_bluetooth: false,
                location: None,
            },
            InputDevice {
                name: "A4Tech USB Keyboard".into(),
                kind: InputKind::Keyboard,
                vid: Some("09DA".into()),
                pid: Some("FA90".into()),
                manufacturer: Some("A4Tech".into()),
                is_bluetooth: false,
                location: None,
            },
            InputDevice {
                name: "Logitech G".into(),
                kind: InputKind::Mouse,
                vid: Some("046D".into()),
                pid: Some("C077".into()),
                manufacturer: Some("Logitech".into()),
                is_bluetooth: false,
                location: None,
            },
        ];
        let merged = merge_input_devices(devices);
        assert_eq!(
            merged.len(),
            2,
            "应合并为 2 台：A4Tech 键鼠一体 + 罗技鼠标，实际: {:?}",
            merged
                .iter()
                .map(|d| (d.name.clone(), d.kind))
                .collect::<Vec<_>>()
        );
        let a4 = merged
            .iter()
            .find(|d| d.vid.as_deref() == Some("09DA"))
            .unwrap();
        assert_eq!(
            a4.kind,
            InputKind::Combo,
            "A4Tech 应识别为键鼠一体（键盘/鼠标）"
        );
        let logi = merged
            .iter()
            .find(|d| d.vid.as_deref() == Some("046D"))
            .unwrap();
        assert_eq!(logi.kind, InputKind::Mouse, "罗技应保持为鼠标");
    }

    #[test]
    fn a4tech_combo_with_identical_fallback_names_is_keyboard() {
        // 真机：A4Tech 09DA:FA90 两集合的产品串都取不到，名字退化成相同的
        // "A4Tech (09DA:FA90)"。此时无法靠产品串区分，按主流默认主类型=键盘。
        let devices = vec![
            InputDevice {
                name: "A4Tech (09DA:FA90)".into(),
                kind: InputKind::Mouse,
                vid: Some("09DA".into()),
                pid: Some("FA90".into()),
                manufacturer: Some("A4Tech".into()),
                is_bluetooth: false,
                location: None,
            },
            InputDevice {
                name: "A4Tech (09DA:FA90)".into(),
                kind: InputKind::Keyboard,
                vid: Some("09DA".into()),
                pid: Some("FA90".into()),
                manufacturer: Some("A4Tech".into()),
                is_bluetooth: false,
                location: None,
            },
        ];
        let merged = merge_input_devices(devices);
        assert_eq!(merged.len(), 1, "应合并为一台");
        assert_eq!(
            merged[0].kind,
            InputKind::Keyboard,
            "无产品串时默认主类型=键盘"
        );
        assert_eq!(merged[0].name, "A4Tech (09DA:FA90)");
    }

    #[test]
    fn real_combo_set_with_distinct_names_stays_combo() {
        // 真·键鼠套装：键盘与鼠标两半各有具体真名（且不同）→ 保留「键盘/鼠标」让盘点者自行判断。
        let devices = vec![
            InputDevice {
                name: "Logitech Keyboard K270".into(),
                kind: InputKind::Keyboard,
                vid: Some("046D".into()),
                pid: Some("C52B".into()),
                manufacturer: Some("Logitech".into()),
                is_bluetooth: false,
                location: None,
            },
            InputDevice {
                name: "Logitech Mouse M185".into(),
                kind: InputKind::Mouse,
                vid: Some("046D".into()),
                pid: Some("C52B".into()),
                manufacturer: Some("Logitech".into()),
                is_bluetooth: false,
                location: None,
            },
        ];
        let merged = merge_input_devices(devices);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].kind, InputKind::Combo, "两半各有真名 → 键盘/鼠标");
    }

    #[test]
    fn combo_with_only_mouse_specific_is_mouse() {
        // 键盘集合是通用名、鼠标集合有具体真名 → 主类型=鼠标。
        let devices = vec![
            InputDevice {
                name: "HID 兼容键盘".into(),
                kind: InputKind::Keyboard,
                vid: Some("1234".into()),
                pid: Some("5678".into()),
                manufacturer: None,
                is_bluetooth: false,
                location: None,
            },
            InputDevice {
                name: "Logitech G Pro".into(),
                kind: InputKind::Mouse,
                vid: Some("1234".into()),
                pid: Some("5678".into()),
                manufacturer: Some("Logitech".into()),
                is_bluetooth: false,
                location: None,
            },
        ];
        let merged = merge_input_devices(devices);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].kind, InputKind::Mouse, "仅鼠标有真名 → 鼠标");
    }

    #[test]
    fn two_identical_mice_collapse_to_one() {
        // 同 VID:PID 同 kind（两台同型号鼠标）→ 视为同一台，跳过（沿用原设计）。
        let devices = vec![
            InputDevice {
                name: "Logitech M100".into(),
                kind: InputKind::Mouse,
                vid: Some("046D".into()),
                pid: Some("C077".into()),
                manufacturer: Some("Logitech".into()),
                is_bluetooth: false,
                location: None,
            },
            InputDevice {
                name: "Logitech M100".into(),
                kind: InputKind::Mouse,
                vid: Some("046D".into()),
                pid: Some("C077".into()),
                manufacturer: Some("Logitech".into()),
                is_bluetooth: false,
                location: None,
            },
        ];
        let merged = merge_input_devices(devices);
        assert_eq!(merged.len(), 1, "两台同型号鼠标应合并为一台");
        assert_eq!(merged[0].kind, InputKind::Mouse);
    }

    #[test]
    fn devices_without_vid_pid_dedup_by_kind_and_name() {
        // 无 VID/PID 设备：(None,None) 不应误并不同设备。
        let devices = vec![
            InputDevice {
                name: "未知输入设备 (1A2C:6004)".into(),
                kind: InputKind::Keyboard,
                vid: None,
                pid: None,
                manufacturer: None,
                is_bluetooth: false,
                location: None,
            },
            InputDevice {
                name: "未知输入设备 (1A2C:6004)".into(),
                kind: InputKind::Keyboard,
                vid: None,
                pid: None,
                manufacturer: None,
                is_bluetooth: false,
                location: None,
            },
            InputDevice {
                name: "别的键盘".into(),
                kind: InputKind::Keyboard,
                vid: None,
                pid: None,
                manufacturer: None,
                is_bluetooth: false,
                location: None,
            },
        ];
        let merged = merge_input_devices(devices);
        // 两个同名同 kind → 合并 1；另一个不同名 → 保留。共 2。
        assert_eq!(
            merged.len(),
            2,
            "无 VID/PID 时按 (kind,name) 去重，不应 (None,None) 全并: {:?}",
            merged
        );
    }
}
