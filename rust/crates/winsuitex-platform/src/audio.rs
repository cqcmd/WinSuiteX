//! 音频设备采集（Win32 SetupAPI + MMDevice COM）。
//!
//! 两阶段（对齐 C++ `CollectAudioUsb` + `CollectDefaultAudio`，hardware.cpp:1185-1311）：
//! 1. SetupAPI 遍历 MEDIA 类设备 → `AudioDevice { role: 0 }`（逻辑音频端点 `Audio Endpoint`
//!    属 SW 枚举器，已在过滤阶段排除；只收真实音频硬件）。
//! 2. COM `IMMDeviceEnumerator::GetDefaultAudioEndpoint` 取默认播放/录音端点的
//!    `PKEY_Device_FriendlyName`，与已采集名做**归一化双向 contains** 匹配，命中置 role 位。
//!
//! 音频依赖 COM，必须在同一线程 `CoInitializeEx`（STA）。`affinity()` 返回 `MainThread`，
//! 并在 `collect` 内用 RAII guard 配对 `CoInitializeEx`/`CoUninitialize`，任何 COM 失败都
//! 安静降级（role 保持 0），`collect` 永不返回 `Err`。

use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
    SetupDiGetDeviceRegistryPropertyW, DIGCF_ALLCLASSES, DIGCF_PRESENT, HDEVINFO,
    SETUP_DI_REGISTRY_PROPERTY, SPDRP_CLASS, SPDRP_DEVICEDESC, SPDRP_ENUMERATOR_NAME,
    SPDRP_HARDWAREID, SP_DEVINFO_DATA,
};

use crate::peripheral_db::{brand_for_vid, is_usb_audio_vid, model_for_pid};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    eCapture, eConsole, eRender, EDataFlow, IMMDeviceEnumerator, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    COINIT_DISABLE_OLE1DDE, STGM_READ,
};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;

use winsuitex_core::error::Result;
use winsuitex_core::model::{AudioDevice, AudioKind};
use winsuitex_core::traits::{
    wsx_trace_enabled_pub, CollectContext, HardwareSource, ThreadAffinity,
};

use crate::wide_to_string;

/// 音频设备数据源。
pub struct AudioSource;

/// 排障打点：`WSX_TRACE=1` 时输出。**刻意用 stderr**：不带缓冲，写一行落一行，
/// 进程若被硬杀（如堆损坏 0xC0000374）之前的内容也已经落盘。
///
/// 为什么 audio 需要子阶段级打点：它依赖 COM（MMDevice），而开发用虚拟机没有任何
/// MEDIA 类音频设备，`mark_default_endpoints` 在 `devices.is_empty()` 处直接返回，
/// 导致**整条 COM 路径从未被本地测试执行过**。真机一旦走到就崩，只能靠这里逐逐步逼近。
fn atrace(msg: &str) {
    if wsx_trace_enabled_pub() {
        eprintln!("[audio] {}", msg);
    }
}

impl HardwareSource for AudioSource {
    fn name(&self) -> &'static str {
        "audio"
    }

    /// 必须固定在同一线程：COM（MMDevice）要求 apartment 模型。
    fn affinity(&self) -> ThreadAffinity {
        ThreadAffinity::MainThread
    }

    fn collect(&self, ctx: &mut CollectContext) -> Result<()> {
        // 1) SetupAPI 采集 MEDIA 类音频硬件（role 初始 0）。SetupAPI 失败按 C++ 语义
        //    视为「无音频」，返回空列表而非 Err（最佳努力）。
        let mut devices = collect_media_audio();
        atrace(&format!("collect_media_audio -> {} devices", devices.len()));

        // 2) COM 取默认端点并匹配 role。用 RAII guard 配对 CoInitialize/CoUninitialize，
        //    即使提前 return 也不会漏掉 Uninitialize。
        atrace("ComInit::init start");
        let _com = ComInit::init();
        atrace("ComInit::init done");
        mark_default_endpoints(&mut devices);
        atrace("mark_default_endpoints done");

        // 3) 逻辑增强：分类类型（音箱/耳机/耳麦）、蓝牙标记、VID/PID 抽取与型号名补全。
        enrich_audio_devices(&mut devices);

        ctx.set_audio(devices);
        Ok(())
    }
}

/// 遍历设备树收录所有 MEDIA 类音频设备（不含 SW 枚举器与黑名单项）。
fn collect_media_audio() -> Vec<AudioDevice> {
    // SAFETY: 同 usb.rs —— 枚举 PRESENT 设备，返回有效 HDEVINFO（失败转 Err）。
    let dev_info =
        match unsafe { SetupDiGetClassDevsW(None, None, None, DIGCF_ALLCLASSES | DIGCF_PRESENT) } {
            Ok(h) => h,
            Err(_) => return Vec::new(),
        };
    // SAFETY: dev_info 有效；Drop 时释放。
    let _guard = DevInfoGuard(dev_info);

    let mut did = SP_DEVINFO_DATA {
        cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
        ..Default::default()
    };

    let mut out = Vec::new();
    let mut idx = 0u32;
    loop {
        // SAFETY: did.cbSize 已初始化；返回 FALSE 表示枚举结束。
        let ok = unsafe { SetupDiEnumDeviceInfo(dev_info, idx, &mut did) }.is_ok();
        if !ok {
            break;
        }
        idx += 1;

        let class = get_dev_prop(dev_info, &did, SPDRP_CLASS);
        let desc = match get_dev_prop(dev_info, &did, SPDRP_DEVICEDESC) {
            Some(d) => d,
            None => continue,
        };
        let enumer = get_dev_prop(dev_info, &did, SPDRP_ENUMERATOR_NAME);

        // 与 C++ 一致的过滤：SW 枚举器 / 黑名单虚拟设备跳过（音频端点即属此类）。
        if enumer.as_deref() == Some("SW") {
            continue;
        }
        if is_not_real_hw(&desc) {
            continue;
        }
        if class.as_deref() != Some("MEDIA") {
            continue;
        }
        if out.iter().any(|d: &AudioDevice| d.name == desc) {
            continue;
        }
        // USB 音频方案在自身硬件 ID 里就带 VID_xxxx&PID_yyyy（无需回溯父节点）。
        let hw_ids = get_dev_hw_ids(dev_info, &did);
        let (vid, pid) = hw_ids
            .first()
            .map(|h| parse_vid_pid_from_hw(h))
            .unwrap_or((None, None));
        let is_bt = is_bluetooth_audio(&desc, enumer.as_deref());
        out.push(AudioDevice {
            name: desc,
            role: 0,
            kind: AudioKind::Unknown,
            is_bluetooth: is_bt,
            vid,
            pid,
            manufacturer: None,
        });
    }
    out
}

/// 取默认播放/录音端点，归一化双向 contains 匹配后写入 role 位。
/// 任一步失败一律 `return`（role 保持 0），绝不向外抛错。
fn mark_default_endpoints(devices: &mut [AudioDevice]) {
    if devices.is_empty() {
        // VM 里永远走这里 —— 这也是为什么 COM 路径在本地从未被真正执行过。
        atrace("mark_default_endpoints: no devices, COM path skipped entirely");
        return;
    }
    // SAFETY: COM 已在本函数调用前由 ComInit 初始化（STA）。
    // CoCreateInstance 创建 MMDeviceEnumerator 单例。
    atrace("CoCreateInstance MMDeviceEnumerator");
    let enumerator: IMMDeviceEnumerator =
        match unsafe { CoCreateInstance(&MMDeviceEnumerator as *const _, None, CLSCTX_ALL) } {
            Ok(e) => {
                atrace("CoCreateInstance ok");
                e
            }
            Err(e) => {
                atrace(&format!("CoCreateInstance FAILED: {}", e));
                return;
            }
        };
    try_mark_role(&enumerator, devices, eRender, 0b01);
    atrace("render role done");
    try_mark_role(&enumerator, devices, eCapture, 0b10);
    atrace("capture role done");
}

/// 对单一 role（播放/录音）取默认端点并匹配。
fn try_mark_role(
    enumerator: &IMMDeviceEnumerator,
    devices: &mut [AudioDevice],
    flow: EDataFlow,
    role_mask: u8,
) {
    let flow_name = if flow == eRender {
        "eRender"
    } else {
        "eCapture"
    };

    // SAFETY: enumerator 有效；无音频设备时返回失败，此处静默降级。
    atrace(&format!("GetDefaultAudioEndpoint {}", flow_name));
    let device = match unsafe { enumerator.GetDefaultAudioEndpoint(flow, eConsole) } {
        Ok(d) => d,
        Err(_) => {
            atrace(&format!("GetDefaultAudioEndpoint FAILED ({})", flow_name));
            return;
        }
    };
    atrace(&format!("GetDefaultAudioEndpoint ok ({})", flow_name));
    // SAFETY: device 有效；打开只读属性库。
    atrace("OpenPropertyStore");
    let props: IPropertyStore = match unsafe { device.OpenPropertyStore(STGM_READ) } {
        Ok(p) => p,
        Err(_) => {
            atrace("OpenPropertyStore FAILED");
            return;
        }
    };
    atrace("OpenPropertyStore ok");
    // SAFETY: GetValue 返回拥有的 PROPVARIANT（Drop 时自动 PropVariantClear）；
    // PKEY_Device_FriendlyName 为有效属性键。
    atrace("GetValue PKEY_Device_FriendlyName");
    let pv = match unsafe { props.GetValue(&PKEY_Device_FriendlyName as *const _) } {
        Ok(v) => v,
        Err(_) => {
            atrace("GetValue FAILED");
            return;
        }
    };
    atrace("GetValue ok");
    // 仅处理 VT_LPWSTR（宽字符串）类型。PROPVARIANT 为 union，字段访问需 unsafe。
    // SAFETY: pv 是 GetValue 返回的已初始化 PROPVARIANT；我们仅读 vt 与 pwszVal 两个
    // 字段，且已先校验 vt == VT_LPWSTR，此时 pwszVal 为有效 NUL 结尾 UTF-16 指针。
    let def_name = unsafe {
        if pv.Anonymous.Anonymous.vt.0 != VT_LPWSTR.0 {
            atrace(&format!(
                "PROPVARIANT vt=0x{:X} (not VT_LPWSTR), skip",
                pv.Anonymous.Anonymous.vt.0
            ));
            return;
        }
        let pwsz = pv.Anonymous.Anonymous.Anonymous.pwszVal;
        if pwsz.is_null() {
            atrace("pwszVal is NULL, skip");
            return;
        }
        match wide_to_string(pwsz.as_wide()) {
            Some(s) => s,
            None => {
                atrace("wide_to_string -> None, skip");
                return;
            }
        }
    };
    atrace(&format!("default endpoint name = {:?}", def_name));

    let def_key = normalize_audio_key(&def_name);
    for dev in devices.iter_mut() {
        let n_key = normalize_audio_key(&dev.name);
        // 双向 contains：跨中英文本地化（如 "扬声器 (HD Audio Device)" vs "HD Audio 设备"）。
        if def_key.contains(&n_key) || n_key.contains(&def_key) {
            dev.role |= role_mask;
            break;
        }
    }
}

/// 读取音频设备的硬件 ID（REG_MULTI_SZ，NUL 分隔、双 NUL 结尾），返回各 ID 字符串。
/// 用于抽取 USB 音频方案的 `VID_xxxx&PID_yyyy`。
fn get_dev_hw_ids(dev_info: HDEVINFO, did: &SP_DEVINFO_DATA) -> Vec<String> {
    // 两步模式：先以 None 缓冲区查询所需字节数，再按真实大小分配，
    // 避免固定 1024 字节在硬件 ID 更长时返回 ERROR_INSUFFICIENT_BUFFER 而整设备被丢弃。
    let mut required = 0u32;
    let _ = unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            dev_info,
            did,
            SPDRP_HARDWAREID,
            None,
            None,
            Some(&mut required),
        )
    };
    if required == 0 {
        return Vec::new();
    }
    let mut buf = vec![0u8; required as usize];
    let ok = unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            dev_info,
            did,
            SPDRP_HARDWAREID,
            None,
            Some(&mut buf),
            None,
        )
    }
    .is_ok();
    if !ok {
        return Vec::new();
    }
    // 转成 u16 序列（REG_MULTI_SZ 为 UTF-16LE）。
    let mut wide: Vec<u16> = Vec::with_capacity(buf.len() / 2);
    let mut i = 0;
    while i + 1 < buf.len() {
        wide.push(u16::from_ne_bytes([buf[i], buf[i + 1]]));
        i += 2;
    }
    let mut ids = Vec::new();
    let mut cur = String::new();
    for w in wide {
        if w == 0 {
            if !cur.is_empty() {
                ids.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(char::from_u32(w as u32).unwrap_or('\u{fffd}'));
        }
    }
    ids
}

/// 从硬件 ID（如 `USB\VID_0D8C&PID_01DA\...`）抽取 `(VID, PID)`（4 位十六进制，大写）。
fn parse_vid_pid_from_hw(hw_id: &str) -> (Option<String>, Option<String>) {
    let up = hw_id.to_ascii_uppercase();
    let vid = up.find("VID_").and_then(|i| {
        let s = &up[i + 4..];
        let end = s.find(|c: char| !c.is_ascii_hexdigit()).unwrap_or(s.len());
        if end == 4 {
            Some(s[..4].to_string())
        } else {
            None
        }
    });
    let pid = up.find("PID_").and_then(|i| {
        let s = &up[i + 4..];
        let end = s.find(|c: char| !c.is_ascii_hexdigit()).unwrap_or(s.len());
        if end == 4 {
            Some(s[..4].to_string())
        } else {
            None
        }
    });
    (vid, pid)
}

/// 是否蓝牙音频设备：设备名或枚举器含 Bluetooth / 蓝牙 / BTHENUM。
fn is_bluetooth_audio(name: &str, enumerator: Option<&str>) -> bool {
    let n = name.to_ascii_lowercase();
    if n.contains("bluetooth") || n.contains("蓝牙") {
        return true;
    }
    if let Some(e) = enumerator {
        let e = e.to_ascii_uppercase();
        if e.starts_with("BTHENUM") || e.contains("BLUETOOTH") {
            return true;
        }
    }
    false
}

/// 是否为「通用占位名」——命中则可用 PID_MODEL 种子的真实型号名覆盖。
fn is_generic_audio_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("usb audio")
        || n.contains("audio device")
        || n.contains("high definition")
        || n.contains("realtek")
        || n.contains("声卡")
        || n.contains("device")
        || n.contains("设备")
}

/// 启发式分类音频设备类型（音箱 / 耳机 / 耳麦 / 未知）。
fn classify_audio_kind(name: &str, role: u8, vid: Option<&str>) -> AudioKind {
    let n = name.to_ascii_lowercase();
    if n.contains("耳麦") || n.contains("headset") {
        return AudioKind::Headset;
    }
    if n.contains("耳机") || n.contains("headphone") {
        return AudioKind::Headphone;
    }
    if n.contains("音箱") || n.contains("speaker") || n.contains("扬声器") {
        return AudioKind::Speaker;
    }
    // 同时是默认播放与录音 → 带麦克风，判为耳麦。
    if role & 0b11 == 0b11 {
        return AudioKind::Headset;
    }
    // 蓝牙音频多为耳机/耳麦。
    if is_bluetooth_audio(name, None) {
        return AudioKind::Headphone;
    }
    // USB 声卡方案（C-Media/Realtek 等）多为游戏耳机。
    if vid.map(is_usb_audio_vid).unwrap_or(false) || n.contains("usb") {
        return AudioKind::Headphone;
    }
    // 板载集成声卡 → 音箱（扬声器 / 线路输出）。
    if n.contains("high definition")
        || n.contains("realtek")
        || n.contains("声卡")
        || n.contains("集成")
    {
        return AudioKind::Speaker;
    }
    // 已知外设品牌的音频设备（且非通用名）→ 多为游戏耳机/耳麦。
    if vid.and_then(brand_for_vid).is_some() && !is_generic_audio_name(name) {
        return AudioKind::Headphone;
    }
    AudioKind::Unknown
}

/// 对采集到的音频设备做逻辑增强：类型分类、厂商、型号名补全。
/// 依赖 `mark_default_endpoints` 写入的 `role`（影响耳麦判定）。
fn enrich_audio_devices(devices: &mut [AudioDevice]) {
    for dev in devices.iter_mut() {
        // 厂商：VID → 英文品牌 key（板载 HDA 无 VID 时为 None）。
        // 统一存英文（如 "HP"/"Razer"），中文译名交给报告层 vendor_label 统一加括号，
        // 与 GPU/CPU/主板等 structured 源一致（JSON 存英文，渲染层出「英文 (中文)」）。
        dev.manufacturer = dev
            .vid
            .as_ref()
            .and_then(|v| brand_for_vid(v).map(|s| s.to_string()));
        // 型号名补全：仅当名称是通用占位名时，用 PID_MODEL 种子（来自 Drives INF）覆盖。
        if let (Some(v), Some(p)) = (&dev.vid, &dev.pid) {
            if let Some(m) = model_for_pid(v, p) {
                if is_generic_audio_name(&dev.name) {
                    dev.name = m.to_string();
                }
            }
        }
        // 类型分类（仅 Unknown 时推导，避免覆盖已被显式信息命中的情况）。
        if dev.kind == AudioKind::Unknown {
            dev.kind = classify_audio_kind(&dev.name, dev.role, dev.vid.as_deref());
        }
    }
}

/// COM 初始化 RAII guard：配对 `CoInitializeEx`/`CoUninitialize`。
struct ComInit {
    need_uninit: bool,
}

impl ComInit {
    fn init() -> Self {
        // SAFETY: 以 STA + 禁用 OLE1 DDE 初始化当前线程 COM；Drop 时对应 CoUninitialize。
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        ComInit {
            need_uninit: hr.is_ok(),
        }
    }
}

impl Drop for ComInit {
    fn drop(&mut self) {
        if self.need_uninit {
            // SAFETY: 与本次成功的 CoInitializeEx 配对（仅本线程）。
            unsafe { CoUninitialize() };
        }
    }
}

/// 读取设备字符串属性（REG_SZ）。详见 usb.rs 内同名函数的 SAFETY 约定。
fn get_dev_prop(
    dev_info: HDEVINFO,
    did: &SP_DEVINFO_DATA,
    prop: SETUP_DI_REGISTRY_PROPERTY,
) -> Option<String> {
    // 两步模式：先以 None 缓冲区查询所需字节数，再按真实大小分配，
    // 避免固定 512 字节在属性更长时返回 ERROR_INSUFFICIENT_BUFFER 而整设备被丢弃。
    let mut required = 0u32;
    let _ = unsafe {
        SetupDiGetDeviceRegistryPropertyW(dev_info, did, prop, None, None, Some(&mut required))
    };
    if required == 0 {
        return None;
    }
    let mut buf = vec![0u8; required as usize];
    let ok = unsafe {
        SetupDiGetDeviceRegistryPropertyW(dev_info, did, prop, None, Some(&mut buf), None)
    }
    .is_ok();
    if !ok {
        return None;
    }
    let mut wide: Vec<u16> = Vec::with_capacity(buf.len() / 2);
    let mut i = 0;
    while i + 1 < buf.len() {
        wide.push(u16::from_ne_bytes([buf[i], buf[i + 1]]));
        i += 2;
    }
    wide_to_string(&wide)
}

/// `HDEVINFO` RAII 守卫。
struct DevInfoGuard(HDEVINFO);

impl Drop for DevInfoGuard {
    fn drop(&mut self) {
        // SAFETY: self.0 是有效的 HDEVINFO。
        let _ = unsafe { SetupDiDestroyDeviceInfoList(self.0) };
    }
}

/// 虚拟 / 软件 / 总线占位设备黑名单（同 usb.rs，与 C++ `IsNotRealHw` 对齐）。
const NOT_REAL_HW: &[&str] = &[
    "Audio Endpoint",
    "音频端点",
    "软件合成器",
    "Wavetable",
    "Microsoft GS",
    "Root Hub",
    "根集线器",
    "Generic USB Hub",
    "通用集线器",
    "USB Composite Device",
    "USB 复合设备",
    "系统设备",
];

/// 设备名是否命中黑名单（大小写不敏感子串匹配）。
fn is_not_real_hw(desc: &str) -> bool {
    NOT_REAL_HW.iter().any(|bad| contains_ci(desc, bad))
}

/// ASCII 大小写不敏感子串匹配，非 ASCII 原样比对。
fn contains_ci(haystack: &str, needle: &str) -> bool {
    let h: Vec<char> = haystack.chars().map(|c| c.to_ascii_lowercase()).collect();
    let n: Vec<char> = needle.chars().map(|c| c.to_ascii_lowercase()).collect();
    if n.is_empty() {
        return false;
    }
    h.windows(n.len()).any(|w| w == n.as_slice())
}

/// 音频名归一化（对齐 C++ `NormalizeAudioKey`，hardware.cpp:1240-1262）：
/// 转小写、去空格/括号/连字符/下划线，再剥掉中英文 "device"/"设备"。
fn normalize_audio_key(s: &str) -> String {
    let lower: String = s
        .chars()
        .filter(|c| !matches!(c, ' ' | '(' | ')' | '-' | '_'))
        .flat_map(|c| c.to_lowercase())
        .collect();
    // 剥掉 device / 设备（大小写已统一为小写）。
    let mut out = String::with_capacity(lower.len());
    let bytes = lower.as_str();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with("device") {
            i += "device".len();
        } else if bytes[i..].starts_with("设备") {
            i += "设备".len();
        } else {
            // i 始终停在字符边界（循环起点与 i += ch.len_utf8() 都保证），
            // 故 i < bytes.len() 时必存在完整字符；用 chars().next() 取值而非索引切片。
            let ch = bytes[i..].chars().next();
            match ch {
                Some(ch) => {
                    out.push(ch);
                    i += ch.len_utf8();
                }
                // 防御分支：正常不可达，避免 unwrap 在边界条件下 panic。
                None => break,
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use winsuitex_core::traits::HardwareSource;

    #[test]
    fn collect_returns_ok_and_prints() {
        let src = AudioSource;
        let mut ctx = CollectContext::new();
        let r = src.collect(&mut ctx);
        assert!(r.is_ok(), "audio collect() 返回 Err: {:?}", r.err());

        let (info, _volumes) = ctx.into_parts();
        // 不假设本机一定有音频设备（虚拟机无 MEDIA 类硬件时列表为空）。
        for d in &info.audio {
            eprintln!("AUDIO name={:?} role={}", d.name, d.role);
        }
        eprintln!("音频设备总数: {}", info.audio.len());
    }

    #[test]
    fn normalize_audio_key_strips_device_and_spaces() {
        assert_eq!(
            normalize_audio_key("High Definition Audio 设备"),
            "highdefinitionaudio"
        );
        assert_eq!(normalize_audio_key("扬声器 (Device)"), "扬声器");
        assert_eq!(normalize_audio_key("USB-Audio"), "usbaudio");
    }

    #[test]
    fn parse_vid_pid_from_hw_id() {
        assert_eq!(
            parse_vid_pid_from_hw("USB\\VID_0D8C&PID_01DA\\1&2&3"),
            (Some("0D8C".to_string()), Some("01DA".to_string()))
        );
        assert_eq!(
            parse_vid_pid_from_hw("HDAUDIO\\FUNC_01&VEN_10EC&DEV_0892"),
            (None, None)
        );
        assert_eq!(
            parse_vid_pid_from_hw("USB\\VID_1038&PID_1251&MI_00"),
            (Some("1038".to_string()), Some("1251".to_string()))
        );
    }

    #[test]
    fn bluetooth_audio_detection() {
        assert!(is_bluetooth_audio("JBL Quantum 400 (Bluetooth)", None));
        assert!(is_bluetooth_audio("蓝牙耳机", None));
        assert!(is_bluetooth_audio("Headphones", Some("BTHENUM\\...")));
        assert!(!is_bluetooth_audio("Realtek Audio", Some("HDAUDIO\\...")));
    }

    #[test]
    fn classify_audio_kind_heuristics() {
        assert_eq!(
            classify_audio_kind("SteelSeries Arctis 5", 0b01, Some("1038")),
            AudioKind::Headphone
        );
        assert_eq!(
            classify_audio_kind("Realtek High Definition Audio", 0b01, None),
            AudioKind::Speaker
        );
        assert_eq!(
            classify_audio_kind("USB Advanced Audio Device", 0b01, Some("0E6A")),
            AudioKind::Headphone
        );
        // 同时默认播放+录音 → 耳麦
        assert_eq!(
            classify_audio_kind("Some Mic Device", 0b11, None),
            AudioKind::Headset
        );
        // 板载无关键字 → 未知
        assert_eq!(
            classify_audio_kind("AC97 Audio", 0b01, None),
            AudioKind::Unknown
        );
    }

    #[test]
    fn generic_audio_name_detection() {
        assert!(is_generic_audio_name("USB Audio Device"));
        assert!(is_generic_audio_name("High Definition Audio 设备"));
        assert!(!is_generic_audio_name("SteelSeries Arctis 5 Game"));
        assert!(!is_generic_audio_name("Cougar Phontum Pro UIX"));
    }
}
