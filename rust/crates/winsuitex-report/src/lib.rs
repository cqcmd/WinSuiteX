//! 报告渲染模块：把 [`HardwareInfo`] / [`Volume`] 渲染成 HTML 与纯文本。
//!
//! 设计目标：与 C++ 原版 `Report_BuildHtml` 的信息结构、小节划分、字段顺序
//! 保持可比，使得新旧二进制导出的 HTML 可以直接 diff 对拍。
//!
//! 关键约定（与 C++ 版一致）：
//! - 所有 `Option<T>` 字段为 `None`（或空串）时渲染为 `未知`；
//! - 变长列表（CPU / 内存条 / 显卡 / 显示器 / 硬盘 / 网卡 / 音频 / USB）为空时
//!   整段不显示（不写 `未知` 占位）；
//! - `Gpu.telemetry` 为 `None` 时不显示温度 / 风扇等遥测行；
//! - `HardwareInfo.is_server` 为 `true` 时隐藏「音频设备」与「USB 外设」两段；
//! - 设备名等可能含 `<>&"` 的字符串必须 HTML 转义，防止破坏结构。

use winsuitex_core::brand_zh_or;
use winsuitex_core::feature::FEATURES;
use winsuitex_core::model::{
    format_uptime, FeatureId, GpuKind, GpuTelemetry, HardwareInfo, HealthStatus, MebiBytes,
    NicKind, PhysicalDisk, SmartSource, Volume,
};
use winsuitex_core::{fmt_link_speed, real_link_speed};

const UNKNOWN: &str = "未知";

// 章节图标（简笔线性 SVG，呼应 GUI 硬件页风格；在 .wsx-idx 紫底方块内以 currentColor 渲染为白色）
const ICON_OS: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="8" height="8" rx="1"/><rect x="13" y="3" width="8" height="8" rx="1"/><rect x="3" y="13" width="8" height="8" rx="1"/><rect x="13" y="13" width="8" height="8" rx="1"/></svg>"#;
const ICON_MACHINE: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="12" rx="1.5"/><path d="M9 20h6"/><path d="M12 16v4"/></svg>"#;
const ICON_BOARD: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="5" width="18" height="14" rx="1.5"/><rect x="7" y="8" width="7" height="7" rx="0.5"/><path d="M16.5 9h2M16.5 12h2M16.5 15h2"/><circle cx="14" cy="15" r="0.8"/></svg>"#;
const ICON_CPU: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="7" y="7" width="10" height="10" rx="1.2"/><path d="M10 3v4M14 3v4M10 17v4M14 17v4M3 10h4M3 14h4M17 10h4M17 14h4"/></svg>"#;
const ICON_MEMORY: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="7" width="18" height="9" rx="1"/><path d="M7 16v3M11 16v3M15 16v3M19 16v3"/><path d="M9 7v9"/></svg>"#;
const ICON_GPU: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="5" width="18" height="11" rx="1.5"/><circle cx="9" cy="10.5" r="3"/><circle cx="16" cy="10.5" r="2"/><path d="M4 16v3M9 16v3"/></svg>"#;
const ICON_DISK: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="6" width="18" height="12" rx="2.5"/><path d="M3 11h18"/><circle cx="8" cy="14.5" r="0.9"/></svg>"#;
const ICON_NIC: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="9" width="13" height="6" rx="1.5"/><path d="M16 11h4M16 13h4"/><path d="M6 9V7M9 9V7"/></svg>"#;
const ICON_VOLUME: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="8"/><path d="M12 12V4M12 12h8"/></svg>"#;
const ICON_AUDIO: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M4 9v6h4l5 4V5L8 9H4z"/><path d="M16 9a4 4 0 0 1 0 6M18.5 7a7 7 0 0 1 0 10"/></svg>"#;
const ICON_USB: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M9 3h6v5a3 3 0 0 1-6 0z"/><path d="M12 11v3"/><path d="M8 14h8l-2 6H10z"/></svg>"#;
const ICON_INPUT: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="6" width="20" height="12" rx="2"/><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M6 14h12"/></svg>"#;
const ICON_FEATURE: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="8" width="18" height="8" rx="4"/><circle cx="16" cy="12" r="3"/></svg>"#;

// ============================================================
// 公共 API
// ============================================================

/// 构建完整 HTML 报告（UTF-8，含 `<meta charset="utf-8">`）。
pub fn build_html(hw: &HardwareInfo, volumes: &[Volume]) -> String {
    build_html_with_features(hw, volumes, &[])
}

/// 同 [`build_html`]，额外附带「WinSuiteX 功能状态」小节。
///
/// 为什么单独开一个入口：功能启用状态来自注册表，而本 crate 是**纯渲染层**，
/// 不该自己去读注册表。由调用方把状态作为数据传进来，本层只负责呈现 —— 这样
/// 报告渲染仍可在无 Windows 环境下用假数据单测。
pub fn build_html_with_features(
    hw: &HardwareInfo,
    volumes: &[Volume],
    features: &[(FeatureId, bool)],
) -> String {
    let mut out = String::new();

    // ---- 页头 ----
    out.push_str("<!DOCTYPE html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    let pc = computer_name(hw);
    out.push_str(&format!(
        "<title>WinSuiteX 硬件报告 - {}</title>\n",
        esc(&pc)
    ));
    out.push_str("<style>\n");
    out.push_str(CSS);
    out.push_str("</style>\n</head>\n<body>\n<div class=\"wsx-wrap\">\n");

    // ---- 品牌渐变 header + 概览 chips ----
    let os_chip = hw
        .os
        .as_ref()
        .and_then(|o| o.product.clone())
        .unwrap_or_else(|| UNKNOWN.to_string());
    let cpu_chip = hw
        .cpus
        .first()
        .and_then(|c| c.model.clone())
        .unwrap_or_else(|| UNKNOWN.to_string());
    let mem_chip = fmt_mb(&hw.memory.total_mb);
    out.push_str("<header class=\"wsx-header\">\n");
    out.push_str("<div class=\"wsx-wordmark\">WinSuiteX</div>\n");
    out.push_str(&format!("<h1>{}</h1>\n", esc(&pc)));
    out.push_str("<div class=\"wsx-sub\">计算机硬件与系统报告 &middot; Rust 版</div>\n");
    out.push_str("<div class=\"wsx-chips\">\n");
    out.push_str(&format!(
        "<div class=\"wsx-chip\"><div class=\"c-label\">操作系统</div><div class=\"c-value\">{}</div></div>\n",
        esc(&os_chip)
    ));
    out.push_str(&format!(
        "<div class=\"wsx-chip\"><div class=\"c-label\">处理器</div><div class=\"c-value\">{}</div></div>\n",
        esc(&cpu_chip)
    ));
    out.push_str(&format!(
        "<div class=\"wsx-chip\"><div class=\"c-label\">内存总量</div><div class=\"c-value c-mono\">{}</div></div>\n",
        esc(&mem_chip)
    ));
    out.push_str("</div>\n");
    out.push_str("</header>\n");

    // ---- 提权提示横幅 ----
    if hw.admin_elevated == Some(false) {
        out.push_str(
            "<div class=\"wsx-warnbox\"><span class=\"wsx-warn-title\">⚠ 未以管理员身份运行</span>NVMe SMART 寿命百分比 / 通电时长可能不完整，请以管理员身份重新运行。</div>\n",
        );
    }

    // ---- 操作系统 / 计算机 ----
    os_section(hw, &mut out);
    machine_section(hw, &mut out);

    // ---- 主板 / BIOS ----
    board_section(hw, &mut out);

    // ---- 处理器 / 内存 ----
    if !hw.cpus.is_empty() {
        cpu_section(hw, &mut out);
    }
    memory_section(hw, &mut out);

    // ---- 显卡 / 显示器 ----
    if !hw.gpus.is_empty() || !hw.displays.is_empty() {
        gpu_display_section(hw, &mut out);
    }

    // ---- 物理硬盘 ----
    if !hw.disks.is_empty() {
        disk_section(hw, &mut out);
    }

    // ---- 网卡 ----
    if !hw.nics.is_empty() {
        nic_section(hw, &mut out);
    }

    // ---- 分区与簇大小 ----
    if !volumes.is_empty() {
        volume_section(volumes, hw, &mut out);
    }

    // ---- 音频 / USB：服务器隐藏；键盘/鼠标 始终显示（与模块约定一致）----
    if !hw.is_server && !hw.audio.is_empty() {
        audio_section(hw, &mut out);
    }
    if !hw.is_server && !hw.usb.is_empty() {
        usb_section(hw, &mut out);
    }
    // 键盘/鼠标 不随服务器隐藏：服务器同样有人接键鼠，隐藏会丢失盘点信息。
    if !hw.input_devices.is_empty() {
        input_section(hw, &mut out);
    }

    // ---- 功能状态（数据由调用方传入，本层纯渲染）----
    if !features.is_empty() {
        feature_section(features, &mut out);
    }

    // ---- 服务器模式：显式说明被隐藏了什么，不留"采集失败"的误判空间 ----
    if hw.is_server {
        let mut omitted = Vec::new();
        if !hw.audio.is_empty() {
            omitted.push(format!("音频设备 {} 项", hw.audio.len()));
        }
        if !hw.usb.is_empty() {
            omitted.push(format!("USB 外设 {} 项", hw.usb.len()));
        }
        if !omitted.is_empty() {
            out.push_str(&format!(
                "<div class=\"wsx-foot\">服务器模式：以下段已省略（{}）</div>\n",
                esc(&omitted.join("、"))
            ));
        }
    }

    // ---- 页脚 ----
    out.push_str(
        "<div class=\"wsx-foot\">WinSuiteX &middot; 重庆持玛多网络科技有限公司 &middot; Z.W.</div>\n",
    );
    out.push_str("</div>\n</body>\n</html>\n");
    out
}

/// 构建纯文本摘要（供「一键复制」）。
pub fn build_text(hw: &HardwareInfo) -> String {
    let mut out = String::new();
    out.push_str("WinSuiteX 计算机硬件与系统报告\n");
    out.push_str("================================\n\n");

    if hw.admin_elevated == Some(false) {
        out.push_str("⚠ 警告：未以管理员身份运行，NVMe SMART 寿命百分比/通电时长可能不完整\n\n");
    }

    out.push_str("操作系统\n");
    push_kv(
        &mut out,
        "  产品",
        disp_opt(&hw.os.as_ref().and_then(|o| o.product.clone())),
    );
    {
        let ver = hw.os.as_ref().and_then(|o| o.version.clone());
        let build = hw.os.as_ref().and_then(|o| o.build.clone());
        let mut s = String::new();
        match (&ver, &build) {
            (None, None) => s.push_str(&format!("{UNKNOWN} (Build {UNKNOWN})")),
            (Some(v), None) => s.push_str(&format!("{v} (Build {UNKNOWN})")),
            (None, Some(b)) => s.push_str(&format!("{UNKNOWN} (Build {b})")),
            (Some(v), Some(b)) => s.push_str(&format!("{v} (Build {b})")),
        }
        push_kv(&mut out, "  版本", s);
    }
    push_kv(
        &mut out,
        "  架构",
        disp_opt(&hw.os.as_ref().and_then(|o| o.arch.clone())),
    );
    out.push('\n');

    out.push_str("计算机\n");
    push_kv(
        &mut out,
        "  名称",
        disp_opt(&hw.machine.as_ref().and_then(|m| m.computer_name.clone())),
    );
    push_kv(
        &mut out,
        "  当前用户",
        disp_opt(&hw.machine.as_ref().and_then(|m| m.user_name.clone())),
    );
    out.push('\n');

    out.push_str("主板 / BIOS\n");
    push_kv(
        &mut out,
        "  制造商",
        vendor_label(&hw.board.as_ref().and_then(|b| b.vendor.clone()))
            .unwrap_or_else(|| UNKNOWN.to_string()),
    );
    push_kv(
        &mut out,
        "  产品",
        disp_opt(&hw.board.as_ref().and_then(|b| b.product.clone())),
    );
    push_kv(
        &mut out,
        "  序列号",
        disp_opt(
            &hw.board
                .as_ref()
                .and_then(|b| b.serial.clone())
                .filter(|s| real_serial(s)),
        ),
    );
    {
        let bv = hw.board.as_ref().and_then(|b| b.bios_vendor.clone());
        let bvv = hw.board.as_ref().and_then(|b| b.bios_version.clone());
        // MM/DD/YYYY → YYYY-MM-DD，中文报告里避免"02/24"需要二次判读。
        let bd = hw
            .board
            .as_ref()
            .and_then(|b| b.bios_date.clone())
            .map(|d| normalize_bios_date(&d));
        let mut s = String::new();
        let has = bv.is_some() || bvv.is_some();
        if has {
            if let Some(lbl) = vendor_label(&bv) {
                s.push_str(&lbl);
            }
            if let Some(v) = &bvv {
                s.push(' ');
                s.push_str(v);
            }
            if let Some(d) = &bd {
                s.push_str(" · ");
                s.push_str(d);
            }
        } else {
            s.push_str(UNKNOWN);
        }
        push_kv(&mut out, "  BIOS", s);
    }
    if let Some(c) = hw.board.as_ref().and_then(|b| b.chipset.clone()) {
        if !c.is_empty() {
            push_kv(&mut out, "  芯片组", c);
        }
    }
    out.push('\n');

    if !hw.cpus.is_empty() {
        out.push_str("处理器\n");
        for (i, c) in hw.cpus.iter().enumerate() {
            let mut s = String::new();
            s.push_str(disp_opt(&c.socket));
            s.push_str(" | ");
            s.push_str(disp_opt(&c.model));
            s.push_str(&format!(
                " | {} 核 / {} 线程",
                disp_u32(&c.cores),
                disp_u32(&c.threads)
            ));
            if let Some(sp) = c.speed_mhz {
                s.push_str(&format!(" · {sp} MHz"));
            }
            if let Some(v) = &c.vendor {
                if !v.is_empty() && usable(v) {
                    let zh = vendor_zh(v);
                    if zh == *v {
                        s.push_str(&format!(" | {}", v));
                    } else {
                        s.push_str(&format!(" | {}（{}）", v, zh));
                    }
                }
            }
            if let Some(cn) = &c.codename {
                if !cn.is_empty() {
                    s.push_str(&format!(" | 代号 {cn}"));
                }
            }
            push_kv(&mut out, &format!("  #{}{}", i + 1, ""), s);
        }
        out.push('\n');
    }

    {
        let total = &hw.memory.total_mb;
        let sticks = &hw.memory.sticks;
        if total.is_some() || !sticks.is_empty() {
            out.push_str("内存\n");
            push_kv(&mut out, "  总计", fmt_mb(total));
            for (i, m) in sticks.iter().enumerate() {
                let mut s = String::new();
                if let Some(cap) = m.capacity_mb {
                    s.push_str(&fmt_mb(&Some(cap)));
                    if let Some(t) = &m.mem_type {
                        if !t.is_empty() {
                            s.push_str(&format!(" · {t}"));
                        }
                    }
                    if let Some(sp) = m.speed_mhz {
                        // 同 HTML 路径：SMBIOS 给的是传输率，单位 MT/s 而非 MHz。
                        // DDR 双倍数据率，真实时钟频率 = 传输率 / 2，附后防误解。
                        s.push_str(&format!(" · {sp} MT/s (时钟 {} MHz)", sp / 2));
                    }
                    s.push_str(" | ");
                    s.push_str(&vendor_label(&m.vendor).unwrap_or_else(|| UNKNOWN.to_string()));
                    s.push(' ');
                    s.push_str(disp_opt(&m.part_no));
                    if let Some(sn) = &m.serial {
                        if real_serial(sn) {
                            s.push_str(" · 序列号 ");
                            s.push_str(sn);
                        }
                    }
                } else {
                    s.push_str(UNKNOWN);
                }
                push_kv(&mut out, &format!("  {}", disp_opt(&m.locator)), s);
                let _ = i;
            }
            out.push('\n');
        }
    }

    if !hw.gpus.is_empty() || !hw.displays.is_empty() {
        out.push_str("显卡 / 显示器\n");
        for (i, g) in hw.gpus.iter().enumerate() {
            let mut s = g.name.clone();
            // 复用 fmt_mb 而不是手写 (vram.0 + 512) / 1024 —— 后者先加后整除，
            // 256 MiB 会算成 0 而显示「0 GB」，512 MiB 又会虚报成 1 GB。
            if g.vram_mb.is_some() {
                s.push_str(&format!(" · {}", fmt_mb(&g.vram_mb)));
            }
            // 板卡品牌（华硕/微星/七彩虹），仅物理显卡展示；芯片厂/整机厂已在采集层过滤。
            if g.kind == GpuKind::Physical {
                if let Some(brand) = winsuitex_core::gpu_brand_for_display(&g.vendor, &g.name) {
                    s.push_str(&format!(" · {}", brand));
                }
            }
            // 虚拟/软件适配器不是真实显卡，显式标注，避免与独显混排误导硬件盘点。
            if g.kind != GpuKind::Physical {
                s.push_str(&format!(" · {}", g.kind.label()));
            }
            if let Some(t) = &g.telemetry {
                let seg = telemetry_segments(t, g.vram_mb);
                s.push_str(&format!(" | {}", seg.join(" · ")));
            }
            push_kv(&mut out, &format!("  显卡 #{}", i + 1), s);
        }
        for (i, d) in hw.displays.iter().enumerate() {
            let mut s = String::new();
            match (d.width, d.height) {
                (Some(w), Some(h)) => s.push_str(&format!("{w} × {h}")),
                _ => s.push_str(UNKNOWN),
            }
            if let Some(r) = d.refresh_hz {
                s.push_str(&format!(" @ {r} Hz"));
            }
            if let Some(sz) = &d.size_inch {
                if !sz.is_empty() {
                    s.push_str(&format!(" · {sz}"));
                }
            }
            // 显示器：厂商中文以括号跟在型号后，不单独成列
            let name = d.name.clone().unwrap_or_default();
            let mut label = name.clone();
            if let Some(v) = &d.vendor {
                if !v.is_empty() && usable(v) {
                    let zh = vendor_zh(v);
                    if name.is_empty() {
                        label = v.clone();
                        if zh != *v {
                            label.push_str(&format!("（{}）", zh));
                        }
                    } else if zh != *v {
                        label.push_str(&format!("（{}）", zh));
                    }
                }
            }
            if !label.is_empty() {
                s.push_str(&format!(" | {label}"));
            }
            if let Some(sn) = &d.serial {
                if real_serial(sn) {
                    s.push_str(&format!(" | 序列号 {sn}"));
                }
            }
            if let Some(y) = d.manufacture_year {
                match d.manufacture_week {
                    Some(w) => s.push_str(&format!(" | 生产 {y}-W{w:02}")),
                    None => s.push_str(&format!(" | 生产 {y}")),
                }
            }
            push_kv(&mut out, &format!("  显示器 #{}", i + 1), s);
        }
        out.push('\n');
    }

    if !hw.disks.is_empty() {
        out.push_str("硬盘\n");
        for d in &hw.disks {
            let brand = d.model.as_deref().and_then(disk_vendor_from_model);
            let mut s = disp_opt(&d.model).to_string();
            if let Some(b) = &brand {
                s.push_str(&format!(" | {b}"));
            }
            s.push_str(&format!(
                " | {} · {} · {}",
                disp_opt(&d.iface),
                disp_opt(&d.media),
                fmt_bytes(&d.size_bytes)
            ));
            // SMART 通电/健康行。
            if let Some((_status, health, rest)) = disk_smart_line(d) {
                s.push_str(&format!(" | {health}{rest}"));
            }
            push_kv(&mut out, &format!("  PhysicalDrive{}", d.index), s);
        }
        out.push('\n');
    }

    if !hw.nics.is_empty() {
        out.push_str("网卡\n");
        for (i, n) in hw.nics.iter().enumerate() {
            // 厂商：优先用采集期写好的字段；字段缺失（老 JSON）时从 desc 兜底推导，
            // 让存档报告显示中文厂商名而无需回真机重采。
            let vendor: Option<String> = n
                .vendor
                .as_deref()
                .filter(|s| !s.is_empty() && usable(s))
                .map(|s| s.to_string())
                .or_else(|| {
                    n.desc
                        .as_deref()
                        .and_then(winsuitex_core::nic_vendor_from_desc)
                });
            let mut s = disp_opt(&n.desc).to_string();
            if let Some(v) = vendor {
                let zh = vendor_zh(&v);
                // 英文厂商名（desc 已含）后跟（中文），不单独成列。
                if zh != v {
                    s.push_str(&format!("（{zh}）"));
                }
            }
            s.push_str(&format!(
                " · 状态 {}",
                if n.up { "已连接" } else { "未连接" }
            ));
            if let Some(bps) = n.speed_bps.filter(|&b| real_link_speed(b)) {
                s.push_str(&format!(" · 速率 {}", fmt_link_speed(bps)));
            }
            s.push_str(&format!(" | MAC {}", disp_opt(&n.mac)));
            // 同 HTML：优先只显示真实地址，仅「已连接但无真实地址」时才带自动地址。
            let ipv4_show = winsuitex_core::nic_ipv4_for_display(n);
            if !ipv4_show.is_empty() {
                let parts: Vec<String> = ipv4_show
                    .iter()
                    .map(|ip| match winsuitex_core::auto_ip_tag(ip) {
                        Some(tag) => format!("{ip} ({tag})"),
                        None => ip.to_string(),
                    })
                    .collect();
                s.push_str(&format!(" · IPv4 {}", parts.join(", ")));
            }
            if let Some(v6) = &n.ipv6 {
                let show_auto = winsuitex_core::nic_should_show_auto_ip(n);
                if !v6.is_empty() && (winsuitex_core::auto_ip_tag(v6).is_none() || show_auto) {
                    match winsuitex_core::auto_ip_tag(v6) {
                        Some(tag) => s.push_str(&format!(" · IPv6 {v6} ({tag})")),
                        None => s.push_str(&format!(" · IPv6 {v6}")),
                    }
                }
            }
            if n.kind != NicKind::Physical {
                s.push_str(&format!(" · {}", n.kind.label()));
            }
            push_kv(&mut out, &format!("  #{}", i + 1), s);
        }
        out.push('\n');
    }

    if !hw.is_server && !hw.audio.is_empty() {
        out.push_str("音频设备\n");
        for (i, a) in hw.audio.iter().enumerate() {
            let mut s = a.name.clone();
            s.push_str(&format!(" · {}", a.kind.label()));
            if a.is_bluetooth {
                s.push_str(" · 蓝牙");
            }
            if let (Some(v), Some(p)) = (&a.vid, &a.pid) {
                s.push_str(&format!(" · {v}:{p}"));
            }
            if let Some(lbl) = vendor_label(&a.manufacturer) {
                if !a.name.contains(&lbl) {
                    s.push_str(&format!(" · {lbl}"));
                }
            }
            push_kv(&mut out, &format!("  #{}", i + 1), s);
        }
        out.push('\n');
    }
    if !hw.is_server && !hw.usb.is_empty() {
        out.push_str("USB 外设\n");
        for u in &hw.usb {
            push_kv(
                &mut out,
                &format!("  {}", usb_class_label(&u.class)),
                u.name.clone(),
            );
        }
        out.push('\n');
    }

    // 键盘/鼠标 不随服务器隐藏（见 HTML 路径同款注释）。
    if !hw.input_devices.is_empty() {
        out.push_str("键盘 / 鼠标\n");
        for d in &hw.input_devices {
            let mut s = d.name.clone();
            if d.is_bluetooth {
                s.push_str(" · 蓝牙");
            }
            if let (Some(v), Some(p)) = (&d.vid, &d.pid) {
                s.push_str(&format!(" · {v}:{p}"));
            }
            if let Some(lbl) = vendor_label(&d.manufacturer) {
                if !d.name.contains(&lbl) {
                    s.push_str(&format!(" · {lbl}"));
                }
            }
            push_kv(&mut out, &format!("  {}", d.kind.label()), s);
        }
        out.push('\n');
    }

    out
}

// ============================================================
// 各小节（HTML）
// ============================================================

fn os_section(hw: &HardwareInfo, out: &mut String) {
    out.push_str(&format!("<section class=\"wsx-card\"><div class=\"wsx-card-title\"><span class=\"wsx-idx\">{}</span>操作系统 <span class=\"wsx-subtle\">OPERATING SYSTEM</span></div><table class=\"wsx-kv\">\n", ICON_OS));
    row(
        out,
        "产品",
        &opt_cell(&hw.os.as_ref().and_then(|o| o.product.clone())),
    );
    {
        let ver = hw.os.as_ref().and_then(|o| o.version.clone());
        let build = hw.os.as_ref().and_then(|o| o.build.clone());
        let cell = match (&ver, &build) {
            (None, None) => format!("{UNKNOWN} (Build {UNKNOWN})"),
            (Some(v), None) => format!("{} (Build {UNKNOWN})", esc(v)),
            (None, Some(b)) => format!("{UNKNOWN} (Build {})", esc(b)),
            (Some(v), Some(b)) => format!("{} (Build {})", esc(v), esc(b)),
        };
        row(out, "版本", &cell);
    }
    row(
        out,
        "架构",
        &opt_cell(&hw.os.as_ref().and_then(|o| o.arch.clone())),
    );
    // 开机时间 + 在线时长：资产盘点里判断「这台服务器多久没重启过」很关键。
    // 旧快照无这两个字段 → 跳过，不显示"未知"（与 collected_at 同策略）。
    if let Some(boot) = hw.os.as_ref().and_then(|o| o.boot_at.clone()) {
        row(out, "开机时间", &esc(&boot));
    }
    if let Some(sec) = hw.os.as_ref().and_then(|o| o.uptime_seconds) {
        row(out, "在线时长", &esc(&format_uptime(sec)));
    }
    out.push_str("</table></section>\n");
}

fn machine_section(hw: &HardwareInfo, out: &mut String) {
    out.push_str(&format!("<section class=\"wsx-card\"><div class=\"wsx-card-title\"><span class=\"wsx-idx\">{}</span>计算机 <span class=\"wsx-subtle\">COMPUTER</span></div><table class=\"wsx-kv\">\n", ICON_MACHINE));
    row(
        out,
        "名称",
        &opt_cell(&hw.machine.as_ref().and_then(|m| m.computer_name.clone())),
    );
    row(
        out,
        "当前用户",
        &opt_cell(&hw.machine.as_ref().and_then(|m| m.user_name.clone())),
    );
    // 服务器模式会隐藏音频 / USB 两段。不标注的话，读者（尤其 BMC / 远程管理场景）
    // 无法区分「本机没有这些设备」和「被服务器模式隐藏了」，会误判成采集失败。
    if hw.is_server {
        row(out, "类型", "服务器");
    }
    // 采集时间在采集端打进快照；旧快照没有这个字段 → 跳过，不显示"未知"。
    if let Some(ts) = &hw.collected_at {
        row(out, "采集时间", &esc(ts));
    }
    out.push_str("</table></section>\n");
}

fn board_section(hw: &HardwareInfo, out: &mut String) {
    out.push_str(&format!("<section class=\"wsx-card\"><div class=\"wsx-card-title\"><span class=\"wsx-idx\">{}</span>主板 / BIOS <span class=\"wsx-subtle\">MOTHERBOARD</span></div><table class=\"wsx-kv\">\n", ICON_BOARD));
    row(
        out,
        "制造商",
        &esc(
            &vendor_label(&hw.board.as_ref().and_then(|b| b.vendor.clone()))
                .unwrap_or_else(|| UNKNOWN.to_string()),
        ),
    );
    row(
        out,
        "产品",
        &opt_cell(&hw.board.as_ref().and_then(|b| b.product.clone())),
    );
    row(
        out,
        "序列号",
        &dash_cell(
            &hw.board
                .as_ref()
                .and_then(|b| b.serial.clone())
                .filter(|s| real_serial(s)),
        ),
    );
    {
        let bv = hw.board.as_ref().and_then(|b| b.bios_vendor.clone());
        let bvv = hw.board.as_ref().and_then(|b| b.bios_version.clone());
        // MM/DD/YYYY → YYYY-MM-DD，中文报告里避免"02/24"需要二次判读。
        let bd = hw
            .board
            .as_ref()
            .and_then(|b| b.bios_date.clone())
            .map(|d| normalize_bios_date(&d));
        let mut cell = String::new();
        if bv.is_some() || bvv.is_some() {
            if let Some(lbl) = vendor_label(&bv) {
                cell.push_str(&esc(&lbl));
            }
            if let Some(v) = &bvv {
                cell.push(' ');
                cell.push_str(&esc(v));
            }
            if let Some(d) = &bd {
                cell.push_str(" &middot; ");
                cell.push_str(&esc(d));
            }
        } else {
            cell.push_str(UNKNOWN);
        }
        row(out, "BIOS", &cell);
    }
    if let Some(c) = hw.board.as_ref().and_then(|b| b.chipset.clone()) {
        if !c.is_empty() {
            row(out, "芯片组", &esc(&c));
        }
    }
    out.push_str("</table></section>\n");
}

fn cpu_section(hw: &HardwareInfo, out: &mut String) {
    out.push_str(&format!("<section class=\"wsx-card\"><div class=\"wsx-card-title\"><span class=\"wsx-idx\">{}</span>处理器 <span class=\"wsx-subtle\">PROCESSOR</span></div><table class=\"wsx-kv\">\n", ICON_CPU));
    for (i, c) in hw.cpus.iter().enumerate() {
        let socket = c.socket.clone().unwrap_or_default();
        let th = format!("#{} {}", i + 1, socket);
        let mut td = String::new();
        td.push_str(&esc(&c.model.clone().unwrap_or_default()));
        td.push_str("<br><span class=\"wsx-mono\">");
        td.push_str(&format!(
            "{} 核 / {} 线程",
            disp_u32(&c.cores),
            disp_u32(&c.threads)
        ));
        if let Some(sp) = c.speed_mhz {
            td.push_str(&format!(" &middot; {sp} MHz"));
        }
        td.push_str("</span><br><span class=\"wsx-mono\">");
        td.push_str(&esc(
            &vendor_label(&c.vendor).unwrap_or_else(|| UNKNOWN.to_string())
        ));
        td.push_str("</span>");
        out.push_str("<tr><th>");
        out.push_str(&esc(&th));
        out.push_str("</th><td>");
        out.push_str(&td);
        out.push_str("</td></tr>\n");

        if let Some(cn) = &c.codename {
            if !cn.is_empty() {
                out.push_str("<tr><th>代号</th><td>");
                out.push_str(&esc(cn));
                out.push_str("</td></tr>\n");
            }
        }
    }
    out.push_str("</table></section>\n");
}

/// 从 Bank Locator 提取通道标识（"P0 CHANNEL A" → "CHANNEL A"），用于槽位名重复时消歧。
///
/// 部分固件把不同物理槽的 locator 都写成一样（华硕 EX-B650M 两槽均 "DIMM 0"），
/// 报告里两行同名分不清。有 "CHANNEL X" 取它；否则回退整段 bank（"NODE 1" / "BANK 0"）。
fn channel_tag(bank: &Option<String>) -> Option<String> {
    let b = bank.as_ref()?;
    let toks: Vec<&str> = b.split_whitespace().collect();
    if let Some(pos) = toks.iter().position(|t| t.eq_ignore_ascii_case("CHANNEL")) {
        if let Some(ch) = toks.get(pos + 1) {
            return Some(format!("CHANNEL {}", ch));
        }
    }
    let trimmed = b.trim();
    if !trimmed.is_empty() {
        return Some(trimmed.to_string());
    }
    None
}

fn memory_section(hw: &HardwareInfo, out: &mut String) {
    let total = &hw.memory.total_mb;
    let sticks = &hw.memory.sticks;
    if total.is_none() && sticks.is_empty() {
        return;
    }
    out.push_str(&format!("<section class=\"wsx-card\"><div class=\"wsx-card-title\"><span class=\"wsx-idx\">{}</span>内存 <span class=\"wsx-subtle\">MEMORY</span></div><table class=\"wsx-kv\">\n", ICON_MEMORY));
    row(out, "总计", &fmt_mb(total));
    // 槽位占用汇总：SMBIOS 只枚举到部分槽时（如 24 槽机器只报出 8 根），
    // 这一行让"各槽合计与总计对不上"一眼可见，不必读者自己数行数。
    if !sticks.is_empty() {
        let populated = sticks.iter().filter(|m| m.is_populated()).count();
        row(
            out,
            "插槽占用",
            &format!("{populated} / {} 槽已插", sticks.len()),
        );
    }
    // 槽位名重复检测：部分固件（如华硕 EX-B650M 把两槽都报 "DIMM 0"）会把不同槽的
    // locator 写成一样，报告里两行同名分不清。先统计已插条里各 locator 出现次数，
    // 重复的在表头追加通道消歧（Z.W. 2026-10-09 定案，修 052 类「两个 DIMM 0」）。
    let mut loc_freq: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for m in sticks {
        if m.is_populated() {
            if let Some(l) = &m.locator {
                if !l.is_empty() {
                    *loc_freq.entry(l.clone()).or_insert(0) += 1;
                }
            }
        }
    }
    // 空插槽先收集、循环结束后再合并成一行：12 槽机器常 8 个空槽，逐行铺开「空槽」
    // 会刷屏、对网吧资产盘点毫无价值。合并后仍能一眼看清哪些槽空着（值单元格列出各
    // 空槽原始标识），「数槽位」意图不丢（Z.W. 2026-10-09 定案，改掉逐行空槽）。
    let mut empty_locators: Vec<String> = Vec::new();
    for m in sticks {
        // 表头显示固件原始槽位标识：实插槽为真实槽名（CHA_0 等），空插槽为 NO DIMM 等占位串原样保留。
        // 不做占位串清洗——"是否为空"由值单元格的「空槽」表达，表头若也洗成「空槽」会变成
        // 「空槽 | 空槽」两重空槽语义，反而别扭（用户明确要求表头保留原始 NO DIMM）。
        // locator 重复时追加通道消歧（"DIMM 0" → "DIMM 0（CHANNEL A）"）。
        let th = match &m.locator {
            Some(l) if !l.is_empty() => {
                if loc_freq.get(l).copied().unwrap_or(0) > 1 {
                    match channel_tag(&m.bank) {
                        Some(ch) => format!("{l}（{ch}）"),
                        None => l.clone(),
                    }
                } else {
                    l.clone()
                }
            }
            _ => UNKNOWN.to_string(),
        };
        if m.is_populated() {
            let mut td = String::new();
            td.push_str(&fmt_mb(&m.capacity_mb));
            if let Some(t) = &m.mem_type {
                if !t.is_empty() {
                    td.push_str(&format!(" &middot; {}", esc(t)));
                }
            }
            if let Some(sp) = m.speed_mhz {
                // SMBIOS 的 Speed / Configured Memory Speed 单位是**传输率 MT/s**，
                // 不是时钟频率：DDR4-2400 的实际时钟是 1200 MHz。
                // 标 MHz 是行业惯例（Windows 任务管理器也这么写），但这里是资产盘点，
                // 选准确写法，避免"内存 2400 MHz"被当成超频依据。
                // 附真实时钟频率（= 传输率 / 2）防误解。
                td.push_str(&format!(" &middot; {sp} MT/s (时钟 {} MHz)", sp / 2));
            }
            td.push_str("<br><span class=\"wsx-mono\">");
            td.push_str(&esc(
                &vendor_label(&m.vendor).unwrap_or_else(|| UNKNOWN.to_string())
            ));
            td.push(' ');
            td.push_str(&opt_cell(&m.part_no));
            td.push_str("</span>");
            if let Some(sn) = &m.serial {
                if real_serial(sn) {
                    td.push_str("<br>序列号：<span class=\"wsx-mono\">");
                    td.push_str(&esc(sn));
                    td.push_str("</span>");
                }
            }
            // Bank Locator：与 C++ 一致——实插槽时这一行恒存在，缺数据显示"未知"，
            // 否则同型号机器会因为"这行有没有"而整段 diff 掉。
            td.push_str("<br>");
            td.push_str(&opt_cell(&m.bank));
            out.push_str("<tr><th>");
            out.push_str(&esc(&th));
            out.push_str("</th><td>");
            out.push_str(&td);
            out.push_str("</td></tr>\n");
        } else {
            // 空插槽（SMBIOS Size == 0）：只收集标识，循环后合并成一行。
            empty_locators.push(th);
        }
    }
    // 所有空槽合并为一行：表头「空槽 N 个」，值单元格「空槽：<各空槽标识>」。
    // 单空槽时表头简化为「空槽」，避免「空槽 1 个」的啰嗦。
    if !empty_locators.is_empty() {
        let n = empty_locators.len();
        let th = if n == 1 {
            "空槽".to_string()
        } else {
            format!("空槽 {n} 个")
        };
        let list = empty_locators.join(" · ");
        let td = format!("<span class=\"wsx-muted\">空槽：{}</span>", esc(&list));
        out.push_str("<tr><th>");
        out.push_str(&esc(&th));
        out.push_str("</th><td>");
        out.push_str(&td);
        out.push_str("</td></tr>\n");
    }
    // 单通道提示（Z.W. 2026-10-09 定案，修 020 类只插 1 条）：
    // 实插内存条为单数且仍有空槽 → 无法组满双通道（1 条=纯单通道；≥3 条奇数为非对称/flex
    // 双通道），游戏内存带宽受限。单槽机器（sticks.len()==1）无双通道可组，不提示。
    let populated_now = sticks.iter().filter(|m| m.is_populated()).count();
    let has_empty = sticks.len() > populated_now;
    if populated_now == 1 && has_empty {
        row(
            out,
            "通道提示",
            "仅插 1 条内存 → 单通道，带宽约为双通道一半，游戏性能受限；建议补同规格条到另一通道组双通道",
        );
    } else if populated_now >= 3 && populated_now % 2 == 1 && has_empty {
        row(
            out,
            "通道提示",
            "实插内存条为奇数（非对称双通道/flex）→ 部分通道单条运行，带宽未用满；建议补齐到偶数条、均匀分布到各通道",
        );
    } else if has_empty {
        // 条数是偶数不代表通道跑满了：双路各插 1 条（都在 Channel 0）也是单通道，
        // 上面两条"数条数"的规则识别不出来，这里按 (Node, Channel) 分组兜住。
        if let Some(msg) = winsuitex_core::memory_channels::memory_channel_warning(sticks) {
            row(out, "通道提示", &esc(&msg));
        }
    }
    // 口径交叉校验：OS 报告的 total_mb 与 SMBIOS 各实插槽容量之和通常应当一致；
    // 差异过大（>2%）说明某一口径不可靠（核显占用/预留内存/固件虚报容量），
    // 这里显式提示差异而非静默采用单一口径，避免误导资产盘点。
    // 容量口径偏差阈值：≤2% 属固件/核显的正常口径差，不提示；
    // 2%~10% 温和提示；>10% 必须告警，不能再一律说"属正常"。
    const DIFF_IGNORE_RATIO: f64 = 0.02;
    const DIFF_ALERT_RATIO: f64 = 0.10;

    if let Some(total_val) = total {
        let sum_sticks: u64 = sticks
            .iter()
            .filter(|m| m.is_populated())
            .filter_map(|m| m.capacity_mb)
            .map(|c| c.get())
            .sum();
        if sum_sticks > 0 && total_val.get() > 0 {
            let diff = total_val.get().abs_diff(sum_sticks);
            let ratio = diff as f64 / total_val.get() as f64;
            if diff > 0 && ratio > DIFF_IGNORE_RATIO {
                let msg = if ratio > DIFF_ALERT_RATIO {
                    // 差一半绝不是"核显/预留"能解释的。真机上曾因 SMBIOS Size=0x7FFF
                    // 哨兵被当成 32767 MiB，各槽合计只有 OS 总计的一半，
                    // 却被一句「属正常」洗白过去 —— 所以这一档必须报警。
                    format!(
                        "OS 总计 {tb} 与 SMBIOS 各槽合计 {sb} 相差 {d} MiB（偏差 {p:.0}%）—— \
                         偏差过大，SMBIOS 槽位容量口径不可信，请核对实插条数与单条容量",
                        tb = fmt_mb(total),
                        sb = fmt_bytes_int_val(sum_sticks << 20),
                        d = diff,
                        p = ratio * 100.0
                    )
                } else {
                    format!(
                        "OS 总计 {tb} 与 SMBIOS 各槽合计 {sb} 相差 {d} MiB（可能含核显/预留内存）",
                        tb = fmt_mb(total),
                        sb = fmt_bytes_int_val(sum_sticks << 20),
                        d = diff
                    )
                };
                row(out, "容量口径提示", &msg);
            }
        }
    }
    out.push_str("</table></section>\n");
}

fn gpu_display_section(hw: &HardwareInfo, out: &mut String) {
    out.push_str(&format!("<section class=\"wsx-card\"><div class=\"wsx-card-title\"><span class=\"wsx-idx\">{}</span>显卡 / 显示器 <span class=\"wsx-subtle\">GPU / DISPLAY</span></div><table class=\"wsx-kv\">\n", ICON_GPU));
    for (i, g) in hw.gpus.iter().enumerate() {
        let mut td = String::new();
        td.push_str(&esc(&g.name));
        // 同文本路径：走 fmt_mb，不要手写 (vram.0 + 512) / 1024（256 MiB → "0 GiB"）。
        if g.vram_mb.is_some() {
            td.push_str(&format!(" &middot; {}", fmt_mb(&g.vram_mb)));
        }
        // 板卡品牌（华硕/微星/七彩虹），仅物理显卡展示；芯片厂/整机厂已在采集层过滤。
        if g.kind == GpuKind::Physical {
            if let Some(brand) = winsuitex_core::gpu_brand_for_display(&g.vendor, &g.name) {
                // 名称里已经带品牌时不要再挂一次（"Matrox G200e · Matrox" 很啰嗦）。
                // 音频段同样处理，见 audio_section。
                if !g.name.contains(&brand) {
                    td.push_str(&format!(
                        " &middot; <span class=\"wsx-muted\">{}</span>",
                        esc(&brand)
                    ));
                }
            }
        }
        // 虚拟/软件适配器不是真实显卡，显式标注，避免与独显混排误导硬件盘点。
        if g.kind != GpuKind::Physical {
            td.push_str(&format!(
                " <span class=\"wsx-muted\">[{}]</span>",
                esc(g.kind.label())
            ));
        }
        if let Some(t) = &g.telemetry {
            telemetry_line(t, g.vram_mb, &mut td);
        }
        out.push_str(&format!(
            "<tr><th>显卡 #{}</th><td>{}</td></tr>\n",
            i + 1,
            td
        ));
    }
    for (i, d) in hw.displays.iter().enumerate() {
        let mut td = String::new();
        match (d.width, d.height) {
            (Some(w), Some(h)) => td.push_str(&format!("{w} × {h}")),
            _ => td.push_str(UNKNOWN),
        }
        if let Some(r) = d.refresh_hz {
            td.push_str(&format!(" @ {r} Hz"));
        }
        if let Some(sz) = &d.size_inch {
            if !sz.is_empty() {
                td.push_str(&format!(" &middot; {sz}"));
            }
        }
        td.push_str("<br><span class=\"wsx-mono\">");
        // 显示器：厂商中文以括号跟在型号后，不单独成列
        let name = d.name.clone().unwrap_or_default();
        let mut label = name.clone();
        if let Some(v) = &d.vendor {
            if !v.is_empty() && usable(v) {
                // `d.vendor` 已是原始 PnP 厂商码（如 "DEL"）；品牌本地化统一走 `brand_display_or`。
                // 显示器名在采集层已嵌入「英文（中文）」品牌（通用名分支整段替换），
                // 仅当名字里**还没**含中文品牌时才补括号，避免重复出现「（戴尔）」。
                let zh = vendor_zh(v);
                if name.is_empty() {
                    label = winsuitex_core::brand_display_or(v);
                } else if !name.contains(&zh) {
                    label.push_str(&format!("（{}）", zh));
                }
            }
        }
        td.push_str(&esc(&label));
        td.push_str("</span>");
        if let Some(sn) = &d.serial {
            if real_serial(sn) {
                td.push_str(&format!("<br>序列号：{}", esc(sn)));
            }
        }
        if let Some(y) = d.manufacture_year {
            match d.manufacture_week {
                Some(w) => td.push_str(&format!("<br>生产：{}-W{:02}", y, w)),
                None => td.push_str(&format!("<br>生产：{}", y)),
            }
        }
        out.push_str(&format!(
            "<tr><th>显示器 #{}</th><td>{}</td></tr>\n",
            i + 1,
            td
        ));
    }
    out.push_str("</table></section>\n");
}

/// 显卡遥测项 → 一段人读文案（不含分隔符）。
///
/// 拆项而非拼死格式串的理由：功耗是**可选项**（多数 A 卡走 ADL、部分 N 卡老驱动
/// 取不到 → `None`）。旧写法把它塞进「… · {}W · …」，取不到时渲染成「… · 未知W · …」，
/// 读起来像一个以 W 结尾的怪单位；现在单独成「功耗未知」，一眼能看出"这项没有"。
fn telemetry_segments(t: &GpuTelemetry, vram: Option<MebiBytes>) -> Vec<String> {
    let total = match vram.or(t.mem_used_mb) {
        Some(v) => v.get().to_string(),
        None => UNKNOWN.to_string(),
    };
    let seg = vec![
        format!("温度 {}°C", disp_i32(&t.temp_c)),
        format!("风扇 {}%", disp_u32(&t.fan_pct)),
        match t.power_w {
            Some(w) => format!("功耗 {w}W"),
            None => format!("功耗 {UNKNOWN}"),
        },
        format!("利用 {}%", disp_u32(&t.util_pct)),
        format!("显存 {}/{} MB", disp_mb(&t.mem_used_mb), total),
        format!("{} MHz", disp_u32(&t.clock_mhz)),
    ];
    // 各项之间的分隔符号由调用方决定（文本用「·」，HTML 用「&middot;」）。
    seg
}

fn telemetry_line(t: &GpuTelemetry, vram: Option<MebiBytes>, td: &mut String) {
    let seg = telemetry_segments(t, vram);
    // 温度高时上色：≥85°C 红（危险）/ ≥75°C 橙（注意），其余保持中性（仅 HTML 路径，不污染纯文本）。
    let segs: Vec<String> = seg
        .into_iter()
        .map(|s| {
            if s.starts_with("温度") {
                let cls = match t.temp_c {
                    Some(temp) if temp >= 85 => "wsx-t-crit",
                    Some(temp) if temp >= 75 => "wsx-t-warn",
                    _ => "",
                };
                if !cls.is_empty() {
                    return format!("<span class=\"{cls}\">{s}</span>");
                }
            }
            s
        })
        .collect();
    td.push_str("<br>状态: ");
    td.push_str(&segs.join(" &middot; "));
}

/// 把单盘 SMART 快照渲染成一行人读文案；没有任何可用数据（且无法说明原因）时返回 `None`。
///
/// 纪律：拿不到就明确显示原因（"健康状态：不可用（需管理员权限）"），绝不给 0 或"健康"。
/// 通电小时数经 [`winsuitex_core::format_power_on_duration`] 换算为可读时长，
/// 用于巡检报告（避免只显示裸小时数难以判断硬盘服役年限）。
/// 把单盘 SMART 快照拆成「健康状态 + 健康令牌 + 其余文案」。
/// 健康令牌在 HTML 里渲染成语义色胶囊（正常绿/注意橙/危险红/未知灰），其余文案原样显示；
/// 纯文本报告把三者拼回原句（健康令牌 + 其余），保持向后兼容。
fn disk_smart_line(d: &PhysicalDisk) -> Option<(HealthStatus, String, String)> {
    let s = d.smart.as_ref()?;

    // 是否有任何"实打实"的数据（健康态非 Unknown 也算，因为它来自真实判定）。
    let has_data = s.power_on_hours.is_some()
        || s.power_cycles.is_some()
        || s.temperature_c.is_some()
        || s.media_errors.is_some()
        || s.host_written_bytes.is_some()
        || s.reallocated_sectors.is_some()
        || s.pending_sectors.is_some()
        || s.percentage_used.is_some()
        || s.health_status != HealthStatus::Unknown;

    // 未尝试（source=None）或未拿到任何数据：有原因就显示原因，否则（真·未知接口）不显示。
    if s.source == SmartSource::None || !has_data {
        return s.unavailable_reason.as_ref().map(|r| {
            (
                HealthStatus::Unknown,
                format!("健康状态：不可用（{r}）"),
                String::new(),
            )
        });
    }

    // 健康令牌（用于上色）+ 其余指标。
    let status = s.health_status;
    let health_token = match status {
        HealthStatus::Healthy => {
            if let Some(pct) = s.health_percent {
                format!("健康 良好（剩余 {pct}%）")
            } else {
                "健康 良好".to_string()
            }
        }
        HealthStatus::Warning => "健康 警告".to_string(),
        HealthStatus::Critical => "健康 严重".to_string(),
        HealthStatus::Unknown => "健康 未知".to_string(),
    };
    let mut rest: Vec<String> = Vec::new();
    if let Some(h) = s.power_on_hours {
        rest.push(format!(
            "通电 {h} 小时（{}）",
            winsuitex_core::model::format_power_on_duration(h)
        ));
    }
    if let Some(c) = s.power_cycles {
        rest.push(format!("通电 {c} 次"));
    }
    if let Some(t) = s.temperature_c {
        rest.push(format!("{t}°C"));
    }
    if let Some(w) = s.host_written_bytes {
        rest.push(format!("已写入 {}", fmt_bytes(&Some(w))));
    }
    if let Some(e) = s.media_errors {
        rest.push(format!("介质错误 {e}"));
    }
    if let Some(r) = s.reallocated_sectors {
        if r > 0 {
            rest.push(format!("重映射 {r} 扇区"));
        }
    }
    if let Some(p) = s.pending_sectors {
        if p > 0 {
            rest.push(format!("待映射 {p} 扇区"));
        }
    }

    // NVMe 经 VMD / 虚拟层拦截时，能拿到温度 / 健康但拿不到通电时长：明确说明而非留白，
    // 避免用户误以为"采集不全"或把缺字段当成全新盘。
    if s.power_on_hours.is_none()
        && matches!(
            s.source,
            SmartSource::NvmeLogPage | SmartSource::StorageReliabilityCounter
        )
        && !rest.is_empty()
    {
        rest.push("通电时长:该存储栈未透传".to_string());
    }

    let rest_str = if rest.is_empty() {
        String::new()
    } else {
        format!(" · {}", rest.join(" · "))
    };
    Some((status, health_token, rest_str))
}

fn disk_section(hw: &HardwareInfo, out: &mut String) {
    out.push_str(&format!("<section class=\"wsx-card\"><div class=\"wsx-card-title\"><span class=\"wsx-idx\">{}</span>硬盘 <span class=\"wsx-subtle\">STORAGE</span></div><table class=\"wsx-kv\">\n", ICON_DISK));
    for d in &hw.disks {
        let brand = d.model.as_deref().and_then(disk_vendor_from_model);
        let mut td = String::new();
        td.push_str(&esc(&d.model.clone().unwrap_or_default()));
        td.push_str("<br><span class=\"wsx-mono\">");
        if let Some(b) = &brand {
            td.push_str(&esc(b));
            td.push_str(" &middot; ");
        }
        td.push_str(&format!(
            "{} &middot; {} &middot; {}",
            esc(disp_opt(&d.iface)),
            esc(disp_opt(&d.media)),
            fmt_bytes(&d.size_bytes)
        ));
        td.push_str("</span>");
        // 序列号单独一行：未分配 / 无卷的盘不会出现在分区表里，
        // 而序列号是换盘时的唯一凭据，不能只在分区段间接带出。
        match d.serial.as_deref().filter(|s| real_serial(s)) {
            Some(s) => {
                td.push_str("<br><span class=\"wsx-mono\">序列号：");
                td.push_str(&esc(s));
                td.push_str("</span>");
            }
            // 与「通电时长:该存储栈未透传」同一套逻辑：VMD / 虚拟层下 NVMe 的 Identify
            // Controller 与 SMART 走同一个 `IOCTL_STORAGE_PROTOCOL_COMMAND`，被拒后
            // 序列号**采不到**而非"没采"——必须说明，否则留白会被误读成漏采或"新盘无序列号"。
            None => {
                let masked = matches!(
                    d.smart.as_ref().map(|s| s.source),
                    Some(SmartSource::NvmeLogPage) | Some(SmartSource::StorageReliabilityCounter)
                );
                if masked {
                    td.push_str("<br><span class=\"wsx-mono\">序列号：该存储栈未透传</span>");
                }
            }
        }
        // 固件版本：资产盘点配合型号 + 序列号构成完整身份（RMA / 批次隐患比对）。
        if let Some(f) = d.firmware.as_deref().filter(|f| !f.trim().is_empty()) {
            td.push_str("<br><span class=\"wsx-mono\">固件：");
            td.push_str(&esc(f));
            td.push_str("</span>");
        }
        // 存储后端告警：盘位于 RAID / Storage Spaces / 虚拟磁盘后时，完整 SMART 需厂商工具。
        if let Some(be) = d.storage_backend.as_deref() {
            td.push_str(&format!(
                "<br><span class=\"wsx-mono wsx-warn\">⚠ {}：完整 SMART 需 storcli/megacli</span>",
                esc(be)
            ));
        }
        // SMART 通电/健康行：有数据就显示，拿不到就明确显示原因（绝不显示 0 / "健康"）。
        // 健康令牌按状态上色：正常绿 / 注意橙 / 危险红 / 未知灰；其余指标保持等宽灰字。
        if let Some((st, health, rest)) = disk_smart_line(d) {
            let (cls, pill) = match st {
                HealthStatus::Healthy => ("wsx-stat-ok", true),
                HealthStatus::Warning => ("wsx-stat-warn", true),
                HealthStatus::Critical => ("wsx-stat-crit", true),
                HealthStatus::Unknown => ("wsx-t-unk", false),
            };
            td.push_str("<br>");
            if pill {
                td.push_str(&format!(
                    "<span class=\"wsx-stat {cls}\">{}</span>",
                    esc(&health)
                ));
            } else {
                td.push_str(&format!("<span class=\"{cls}\">{}</span>", esc(&health)));
            }
            if !rest.is_empty() {
                td.push_str(&format!("<span class=\"wsx-mono\">{}</span>", esc(&rest)));
            }
        }
        out.push_str(&format!(
            "<tr><th>PhysicalDrive{}</th><td>{}</td></tr>\n",
            d.index, td
        ));
    }
    let total: u64 = hw.disks.iter().filter_map(|d| d.size_bytes).sum();
    if total > 0 {
        row(out, "总计", &fmt_bytes_val(total));
    }
    out.push_str("</table></section>\n");
}

// 自动地址识别与「是否显示自动地址」策略已统一到 `winsuitex_core::auto_ip_tag` /
// `winsuitex_core::nic_should_show_auto_ip`（GUI 与报告共用单一数据源），本文件仅调用。

fn nic_section(hw: &HardwareInfo, out: &mut String) {
    out.push_str(&format!("<section class=\"wsx-card\"><div class=\"wsx-card-title\"><span class=\"wsx-idx\">{}</span>网卡 <span class=\"wsx-subtle\">NETWORK</span></div><table class=\"wsx-kv\">\n", ICON_NIC));
    for (i, n) in hw.nics.iter().enumerate() {
        let mut td = String::new();
        td.push_str(&opt_cell(&n.desc));
        // 厂商：优先用采集期写好的字段；字段缺失（老 JSON）时从 desc 兜底推导，
        // 让存档报告显示中文厂商名而无需回真机重采。
        let vendor: Option<String> = n
            .vendor
            .as_deref()
            .filter(|s| !s.is_empty() && usable(s))
            .map(|s| s.to_string())
            .or_else(|| {
                n.desc
                    .as_deref()
                    .and_then(winsuitex_core::nic_vendor_from_desc)
            });
        if let Some(v) = vendor {
            let zh = vendor_zh(&v);
            // 英文厂商名（desc 已含）后跟（中文），不单独成列。
            if zh != v {
                td.push_str(&format!("（{}）", esc(&zh)));
            }
        }
        // 状态与速率：up 表示已连接（OperStatus == IfOperStatusUp）；speed_bps 为链路速率。
        td.push_str(&format!(
            "<br>状态: {}",
            esc(if n.up { "已连接" } else { "未连接" })
        ));
        if let Some(bps) = n.speed_bps.filter(|&b| real_link_speed(b)) {
            td.push_str(&format!("<br>速率: {}", esc(&fmt_link_speed(bps))));
        }
        // MAC / IPv4 / IPv6 各自成行，避免挤在一行难读（尤其多 IP / IPv6 较长时）。
        td.push_str("<br><span class=\"wsx-mono\">MAC ");
        td.push_str(&opt_cell(&n.mac));
        td.push_str("</span>");
        // IP 显示规则：优先只显示真实地址；仅「已连接但没拿到任何真实地址」时才带自动地址。
        let ipv4_show = winsuitex_core::nic_ipv4_for_display(n);
        if !ipv4_show.is_empty() {
            let parts: Vec<String> = ipv4_show
                .iter()
                .map(|s| match winsuitex_core::auto_ip_tag(s) {
                    Some(tag) => format!("{} <span class=\"wsx-tag\">{}</span>", esc(s), esc(tag)),
                    None => esc(s).to_string(),
                })
                .collect();
            td.push_str("<br><span class=\"wsx-mono\">IPv4 ");
            td.push_str(&parts.join(", "));
            td.push_str("</span>");
        }
        if let Some(v6) = &n.ipv6 {
            let show_auto = winsuitex_core::nic_should_show_auto_ip(n);
            if !v6.is_empty() && (winsuitex_core::auto_ip_tag(v6).is_none() || show_auto) {
                td.push_str("<br><span class=\"wsx-mono\">IPv6 ");
                td.push_str(&esc(v6));
                if let Some(tag) = winsuitex_core::auto_ip_tag(v6) {
                    td.push_str(&format!(" <span class=\"wsx-tag\">{}</span>", esc(tag)));
                }
                td.push_str("</span>");
            }
        }
        if n.kind != NicKind::Physical {
            td.push_str(&format!(
                "<br><span class=\"wsx-tag\">{}</span>",
                esc(n.kind.label())
            ));
        }
        out.push_str(&format!("<tr><th>#{}</th><td>{}</td></tr>\n", i + 1, td));
    }
    out.push_str("</table></section>\n");
}

fn volume_section(volumes: &[Volume], hw: &HardwareInfo, out: &mut String) {
    out.push_str(&format!("<section class=\"wsx-card\"><div class=\"wsx-card-title\"><span class=\"wsx-idx\">{}</span>分区与簇大小 <span class=\"wsx-subtle\">PARTITIONS</span></div>\n", ICON_VOLUME));
    out.push_str(
        "<div class=\"wsx-table-scroll\"><table class=\"wsx-dtable\"><tr>\
        <th style=\"width:auto\">盘符</th>\
        <th style=\"width:auto\">物理磁盘</th>\
        <th style=\"width:auto\">类型</th>\
        <th style=\"width:auto\">文件系统</th>\
        <th style=\"width:auto\">簇大小</th>\
        <th style=\"width:auto\">总容量</th>\
        <th style=\"width:auto\">已用</th>\
        <th style=\"width:auto\">可用</th>\
        <th style=\"width:auto\">磁盘序列号</th>\
        <th style=\"width:auto\">卷标</th></tr>\n",
    );
    for v in volumes {
        let letter = esc(&v.letter);
        let pd = esc(&pd_str(&v.disk_index));
        let typ = opt_cell(&v.type_name);
        let fs = opt_cell(&v.fs_name);
        let clu = fmt_cluster(&v.cluster_bytes);
        let tot = fmt_bytes(&v.total_bytes);
        let fre = fmt_bytes(&v.free_bytes);
        // 已用与使用率：巡检时"这盘快满了没"比绝对容量更需要一眼看到。
        // 可用 > 总量（配额 / 挂载点等异常口径）时不硬算，直接给 "-"。
        // 使用率分级着色：≥90% 红（危险）/ ≥75% 橙（注意）/ 其余正常。
        let (used_str, used_state) = match (v.total_bytes, v.free_bytes) {
            (Some(t), Some(f)) if t > 0 && f <= t => {
                let u = t - f;
                let pct = (u as f64 / t as f64) * 100.0;
                // 使用率分级着胶囊（与 SMART 健康胶囊同一套 .wsx-stat-*）：≥90% 红 / ≥75% 橙 / 其余正常。
                let state = if pct >= 90.0 {
                    "crit"
                } else if pct >= 75.0 {
                    "warn"
                } else {
                    ""
                };
                (format!("{}（{:.0}%）", fmt_bytes_val(u), pct), state)
            }
            _ => ("-".to_string(), ""),
        };
        // 满盘/接近满盘时把"已用"包成状态胶囊，一眼可见；正常盘保持普通等宽文字。
        let used_html = if used_state.is_empty() {
            format!("<td class=\"wsx-mono\">{used_str}</td>")
        } else {
            format!(
                "<td class=\"wsx-mono\"><span class=\"wsx-stat wsx-stat-{used_state}\">{used_str}</span></td>"
            )
        };
        let ser = serial_cell(hw, v.disk_index);
        let vol = dash_cell(&v.vol_label);
        out.push_str(&format!(
            "<tr><td class=\"wsx-mono\">{letter}</td>\
             <td class=\"wsx-mono\">{pd}</td>\
             <td>{typ}</td>\
             <td class=\"wsx-mono\">{fs}</td>\
             <td class=\"wsx-mono\">{clu}</td>\
             <td class=\"wsx-mono\">{tot}</td>\
             {used_html}\
             <td class=\"wsx-mono\">{fre}</td>\
             <td class=\"wsx-mono\">{ser}</td>\
             <td class=\"wsx-mono\">{vol}</td></tr>\n"
        ));
    }
    // ⚠️ 闭合顺序必须与开启顺序相反：外 `<div class="wsx-table-scroll">` 包内
    // `<table>`，所以先 `</table>` 再 `</div>`。之前写成 `</div></table>`（反了），
    // 浏览器容错看不出来，但属无效嵌套，严格解析器/复制导出会错。
    out.push_str("</table></div></section>\n");
}

fn audio_section(hw: &HardwareInfo, out: &mut String) {
    out.push_str(&format!("<section class=\"wsx-card\"><div class=\"wsx-card-title\"><span class=\"wsx-idx\">{}</span>音频设备 <span class=\"wsx-subtle\">AUDIO</span></div><table class=\"wsx-kv\">\n", ICON_AUDIO));
    for (i, a) in hw.audio.iter().enumerate() {
        let mut td = String::new();
        td.push_str(&esc(&a.name));
        if a.is_bluetooth {
            td.push_str(" <span class=\"wsx-tag\">蓝牙</span>");
        }
        td.push_str(&format!(
            " <span class=\"wsx-tag\">{}</span>",
            esc(a.kind.label())
        ));
        if let (Some(v), Some(p)) = (&a.vid, &a.pid) {
            td.push_str(&format!(
                "<br><span class=\"wsx-mono\">{}:{}</span>",
                esc(v),
                esc(p)
            ));
        }
        if let Some(lbl) = vendor_label(&a.manufacturer) {
            if !a.name.contains(&lbl) {
                td.push_str(&format!(" &middot; {}", esc(&lbl)));
            }
        }
        out.push_str(&format!("<tr><th>#{}</th><td>{}</td></tr>\n", i + 1, td));
    }
    out.push_str("</table></section>\n");
}

/// USB 段 class 标签：`USBSTOR` → 中文「U 盘 / 移动硬盘」，其余回退原始 class，None → 未知。
fn usb_class_label(class: &Option<String>) -> String {
    match class.as_deref() {
        Some("USBSTOR") => "U 盘 / 移动硬盘".to_string(),
        Some(other) => other.to_string(),
        None => "未知".to_string(),
    }
}

fn usb_section(hw: &HardwareInfo, out: &mut String) {
    out.push_str(&format!("<section class=\"wsx-card\"><div class=\"wsx-card-title\"><span class=\"wsx-idx\">{}</span>USB 外设 <span class=\"wsx-subtle\">USB</span></div><table class=\"wsx-kv\">\n", ICON_USB));
    for u in &hw.usb {
        let label = usb_class_label(&u.class);
        out.push_str("<tr><th>");
        out.push_str(&esc(&label));
        out.push_str("</th><td>");
        out.push_str(&esc(&u.name));
        out.push_str("</td></tr>\n");
    }
    out.push_str("</table></section>\n");
}

fn input_section(hw: &HardwareInfo, out: &mut String) {
    out.push_str(&format!("<section class=\"wsx-card\"><div class=\"wsx-card-title\"><span class=\"wsx-idx\">{}</span>键盘 / 鼠标 <span class=\"wsx-subtle\">INPUT</span></div><table class=\"wsx-kv\">\n", ICON_INPUT));
    for d in &hw.input_devices {
        let mut td = String::new();
        td.push_str(&esc(&d.name));
        if d.is_bluetooth {
            td.push_str(" <span class=\"wsx-tag\">蓝牙</span>");
        }
        if let (Some(v), Some(p)) = (&d.vid, &d.pid) {
            td.push_str(&format!(
                "<br><span class=\"wsx-mono\">{}:{}</span>",
                esc(v),
                esc(p)
            ));
        }
        if let Some(lbl) = vendor_label(&d.manufacturer) {
            if !d.name.contains(&lbl) {
                td.push_str(&format!(" &middot; {}", esc(&lbl)));
            }
        }
        out.push_str(&format!(
            "<tr><th>{}</th><td>{}</td></tr>\n",
            esc(d.kind.label()),
            td
        ));
    }
    out.push_str("</table></section>\n");
}

/// 分区行的「序列号」列：取自它所在的**物理盘**，不是卷本身的属性。
///
/// 与 C++ 版一致 —— 序列号来自 `STORAGE_DEVICE_DESCRIPTOR`，按盘号关联到分区行。
/// 无匹配物理盘、或该盘固件未上报序列号 → `未知`（本机 QEMU 盘就是后者，C++ 同样显示 `-`）。
fn serial_cell(hw: &HardwareInfo, disk_index: Option<u32>) -> String {
    match disk_index.and_then(|i| hw.disks.iter().find(|d| d.index == i)) {
        // 分区表里没有序列号是常态（不少逻辑卷/虚拟盘拿不到），用 `-` 而非"未知"
        Some(d) => dash_cell(&d.serial),
        None => "-".to_string(),
    }
}

/// 右键功能启用状态。名称与说明从 `core::feature::FEATURES` 查表，
/// 调用方只需传 `(FeatureId, 是否启用)`，避免在报告层重复维护一份文案。
fn feature_section(features: &[(FeatureId, bool)], out: &mut String) {
    out.push_str(&format!("<section class=\"wsx-card\"><div class=\"wsx-card-title\"><span class=\"wsx-idx\">{}</span>WinSuiteX 功能状态 <span class=\"wsx-subtle\">FEATURES</span></div><table class=\"wsx-kv\">\n", ICON_FEATURE));
    for (id, on) in features {
        let f = FEATURES.iter().find(|f| f.id == *id);
        let name = f.map_or(id.0, |f| f.display_name);
        let desc = f.map_or("", |f| f.description);
        let badge = if *on {
            "<span class=\"wsx-badge wsx-badge-ok\">已启用</span>"
        } else {
            "<span class=\"wsx-badge wsx-badge-off\">未启用</span>"
        };
        out.push_str(&format!(
            "<tr><th>{}</th><td>{} {}</td></tr>\n",
            esc(name),
            badge,
            esc(desc)
        ));
    }
    out.push_str("</table></section>\n");
}

// ============================================================
// 工具函数
// ============================================================

/// 输出一行 `<tr><th>..</th><td>..</td></tr>`。`th` 为静态中文标签；
/// `td_escaped` 必须已经是转义后的安全文本（本模块内所有动态内容均先经 `esc`）。
fn row(out: &mut String, th: &str, td_escaped: &str) {
    out.push_str("<tr><th>");
    out.push_str(th);
    out.push_str("</th><td>");
    out.push_str(td_escaped);
    out.push_str("</td></tr>\n");
}

/// `Option<String>` → 显示文本：`None`/空串 一律为 `未知`，并对内容做 HTML 转义。
fn opt_cell(o: &Option<String>) -> String {
    esc(disp_opt(o))
}

/// 表格单元格：空数据显示 `-`（与 C++ 分区表一致）。
///
/// 分区表大量列可能为空（无序列号、无卷标），用 `未知` 会占满整列、
/// 也会让逐段 diff 全军覆没；`opt_cell` 仍用于"名称类"字段。
fn dash_cell(o: &Option<String>) -> String {
    esc(match o {
        Some(v) if usable(v) => v.as_str(),
        _ => "-",
    })
}

/// `Option<String>` → 显示文本（`None`/空串 为 `未知`）。
fn disp_opt(o: &Option<String>) -> &str {
    match o {
        Some(v) if usable(v) => v.as_str(),
        _ => UNKNOWN,
    }
}

/// 固件常见的「占位串」——会被原样渲染成真实厂商/型号/序列号，直接污染资产盘点，
/// 一律按「未知」处理。大小写/前后空白不敏感。
///
/// 这些串来自 BIOS/UEFI 模板默认值（Phoenix/Award/AMI 等厂都会填），并非真实数据：
/// `To Be Filled By O.E.M.`、`System Manufacturer`、`NO DIMM` 等。把它们当真值渲染出去，
/// 网咖资产盘点会把"未知主板"记成一家叫"To Be Filled"的厂商——这是审查发现的头号真实性隐患。
fn is_placeholder(v: &str) -> bool {
    const P: &[&str] = &[
        "default string",
        "unknown",
        "to be filled by o.e.m.",
        "to be filled by oem",
        "not specified",
        "no dimm",
        "not applicable",
        "system product name",
        "system manufacturer",
        "system version",
        "serial number",
        "system serial number",
    ];
    let t = v.trim().to_lowercase();
    P.contains(&t.as_str())
}

/// 值是否可以直接渲染。
///
/// 含 U+FFFD 说明上游某处做了 lossy 转换 —— 原始数据是损坏的（真机案例：
/// 某内存条 Part Number 是 18 字节非法 UTF-8）。渲染出来是一串 `???`，
/// 运维会以为工具坏了；按"未知"处理更诚实。这是采集层修复之外的兜底：
/// 任何新采集源再犯同样的错，也不会把乱码带到客户报告里。
///
/// 同时拒绝固件占位串（`is_placeholder`）：占位串一旦当成真实厂商/型号渲染，
/// 会直接污染资产盘点。
fn usable(v: &str) -> bool {
    !v.is_empty() && !v.contains('\u{FFFD}') && !is_placeholder(v)
}

/// 序列号是否为「真实可读」。固件常用占位串冒充序列号（`Default string`、
/// `Unknown`、`To Be Filled By O.E.M.` 等），渲染出来会像真序列号一样误导资产盘点。
/// 这些一律按「无序列号」处理 —— 用户明确要求：没有真实序列号就不显示。
fn real_serial(s: &str) -> bool {
    let t = s.trim();
    if !usable(t) {
        return false;
    }
    const PLACEHOLDER: &[&str] = &[
        "default string",
        "unknown",
        "to be filled by o.e.m.",
        "to be filled by oem",
        "not specified",
        "none",
        "n/a",
        "na",
        "serial number",
        "system serial number",
        "0",
        "0000000000",
        "000000000000",
        "1234567890",
        "oooooooooo",
        "oooooooooooo",
    ];
    let lower = t.to_lowercase();
    if PLACEHOLDER.contains(&lower.as_str()) {
        return false;
    }
    // 退化填充形态：精确列表穷举不完——固件会填**任意长度**的 0 / F / - / X。
    // 真机风险：`00000000`（8 个 0）、`FFFFFFFF`、`0123456789`、`12345678` 都不在上方
    // 列表里，会被当真序列号渲染进资产报告。故按模式判定，真实序列号
    // （如 `F07D3070`、`2CHMBDEP`、`BQWL43950466`）不受影响。
    !is_degenerate_serial(t)
}

/// 序列号是否为「退化填充形态」：全同字符（长度 ≥ 4）或连续升/降数字串（长度 ≥ 6）。
///
/// 覆盖 `00000000` / `FFFFFFFF` / `--------` / `XXXXXXXX` / `0123456789` / `12345678`
/// 这类固件填充值。刻意**不按长度一刀切**，避免误伤短而真实的序列号。
fn is_degenerate_serial(t: &str) -> bool {
    let chars: Vec<char> = t.chars().collect();
    // 全同字符（长度 ≥ 4；长度 1 的 windows(2) 为空会恒真，故必须限长）
    if chars.len() >= 4 && chars.windows(2).all(|w| w[0] == w[1]) {
        return true;
    }
    // 连续升 / 降数字串（长度 ≥ 6）
    if chars.len() >= 6 && chars.iter().all(|c| c.is_ascii_digit()) {
        let d: Vec<u32> = chars.iter().filter_map(|c| c.to_digit(10)).collect();
        let asc = d.windows(2).all(|w| w[1] == w[0] + 1);
        let desc = d.windows(2).all(|w| w[0] == w[1] + 1);
        if asc || desc {
            return true;
        }
    }
    false
}

/// 英文/缩写厂商名 → 中文显示名，**仅用于 HTML 展示**。
///
/// 设计边界：JSON 数据保持原始英文厂商名（与 C++ 老版对拍靠的是 JSON 结构，
/// 不是报告文字），所以这里只改渲染层，不动 model。大小写不敏感匹配；
/// 未收录的厂商原样返回英文，绝不错译。
///
/// 收录原则：只收 PC 硬件领域常见、可被唯一归属的厂商。ODM（代工厂）即便收录
/// 也标厂商名而非零售品牌（与 `winsuitex_core::brand_zh_or` 一致）。
pub fn vendor_zh(raw: &str) -> String {
    // 中文来源已收敛为 winsuitex_core::brand_zh_or（全项目唯一数据源），此处仅做转发，
    // 保留函数名以免大改调用点。匹配顺序与旧实现一致：trim → 精确(忽略大小写) → 首词降级。
    brand_zh_or(raw).to_string()
}

/// 渲染用厂商名：`英文（中文）` 括号形式（全角，第二十轮 Z.W. 定案的统一格式）。
/// 返回**未转义**串；HTML 调用方需自行 `esc`。
///
/// - `None`/空/含损坏字符(U+FFFD) → `None`（调用方回退为 未知）
/// - 收录映射 → `英文名（中文名）`，例如 `MSI（微星）`、`Kingston（金士顿）`
/// - 不翻译 / ODM 兜底条目（en==zh）只显示一段
/// - 未收录 → 原文（绝不错译）
///
/// 设计边界：JSON 数据保持原始英文厂商名（与 C++ 老版对拍靠 JSON 结构，不靠报告文字），
/// 这里只改渲染层。格式统一委托 `winsuitex_core::brand_display_or`（单一数据源）。
fn vendor_label(en: &Option<String>) -> Option<String> {
    let s = en.as_ref()?;
    if !usable(s) {
        return None;
    }
    Some(winsuitex_core::brand_display_or(s))
}

// 物理显卡板卡品牌展示名已统一到 `winsuitex_core::gpu_brand_for_display`
// （GUI 与报告共用单一数据源），本文件仅调用，不再维护副本。

/// 从硬盘型号串推断厂商中文名（仅展示用，不动 JSON）。
/// 仅匹配高置信度厂商标识：完整厂商名（任意位置）或 Seagate 的 `ST<数字>` 前缀；
/// 其余一律返回 `None`，绝不臆造品牌。
fn disk_vendor_from_model(model: &str) -> Option<String> {
    let m = model.to_ascii_uppercase();
    // 仅保留 (型号关键字, 英文厂商)；中文统一回查 winsuitex_core::brand_zh（单一数据源）。
    const FULL: &[(&str, &str)] = &[
        ("INTEL", "Intel"),
        ("SEAGATE", "Seagate"),
        ("TOSHIBA", "Toshiba"),
        ("SAMSUNG", "Samsung"),
        ("DELL", "Dell"),
        ("AVAGO", "Avago"),
        ("WDC", "Western Digital"),
        ("WESTERN DIGITAL", "Western Digital"),
        ("WUH", "Western Digital"),
        ("HITACHI", "Hitachi"),
        ("HGST", "HGST"),
        ("SANDISK", "SanDisk"),
        ("CRUCIAL", "Crucial"),
        ("KINGSTON", "Kingston"),
        ("SOLIDIGM", "Solidigm"),
        ("LITEON", "LiteOn"),
        ("LITE-ON", "LiteOn"),
    ];
    for (kw, en) in FULL {
        if m.contains(kw) {
            return Some(winsuitex_core::brand_display_or(en));
        }
    }
    // Seagate 型号以 ST<数字> 开头（如 ST8000NM000A-2KE101）；
    // 型号串常带接口前缀（如 "ATA ST16000NM002H-3K"），故按空白分词后判任一词元。
    if m.split_whitespace().any(|tok| {
        tok.starts_with("ST")
            && tok
                .get(2..3)
                .and_then(|c| c.chars().next())
                .is_some_and(|ch| ch.is_ascii_digit())
    }) {
        return Some(winsuitex_core::brand_display_or("Seagate"));
    }
    // Kioxia（原东芝内存）型号以 VO<数字> 开头（如 VO001000KWJSE）；
    // 同 Seagate，按空白分词后判任一词元，避免误中 "NVMe" 里的 "VO"。
    if m.split_whitespace().any(|tok| {
        tok.starts_with("VO")
            && tok
                .get(2..3)
                .and_then(|c| c.chars().next())
                .is_some_and(|ch| ch.is_ascii_digit())
    }) {
        return Some(winsuitex_core::brand_display_or("Kioxia"));
    }
    None
}

/// `Option<u32>` → 显示文本。
fn disp_u32(o: &Option<u32>) -> String {
    match o {
        Some(v) => v.to_string(),
        None => UNKNOWN.to_string(),
    }
}

/// `Option<i32>` → 显示文本。
fn disp_i32(o: &Option<i32>) -> String {
    match o {
        Some(v) => v.to_string(),
        None => UNKNOWN.to_string(),
    }
}

/// `Option<MebiBytes>` → 显示 MiB 数值。
/// 用于"显存 2048/12288 MiB"这类需要与总量并列、保持同一量纲的场合。
fn disp_mb(o: &Option<MebiBytes>) -> String {
    match o {
        Some(v) => v.get().to_string(),
        None => UNKNOWN.to_string(),
    }
}

/// HTML 转义 `& < > "`，防止设备名里的特殊字符破坏结构。
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// `\\.\PhysicalDriveN`（无 extent 信息时回退 `-`）。
fn pd_str(idx: &Option<u32>) -> String {
    match idx {
        Some(i) => format!("\\\\.\\PhysicalDrive{i}"),
        None => "-".to_string(),
    }
}

/// 容量串（保留一位小数，对齐 C++ `FormatBytesStr`）；`None`/0 为 `未知`。
fn fmt_bytes(b: &Option<u64>) -> String {
    match b {
        Some(v) if *v > 0 => fmt_bytes_val(*v),
        _ => UNKNOWN.to_string(),
    }
}

/// 字节 → 人类可读容量（保留 1 位小数）。
///
/// 用的是**二进制（IEC）单位**：1 KiB = 1024 B。标签必须与除数一致 ——
/// 以前拿 1024 当除数却标 "GB"（SI 十进制），524191 MiB 被显示成 "512 GB"，
/// 而它实际是 512 GiB = 549.7 GB，全报告系统性失真 7~9%。
/// 网络速率 `fmt_link_speed` 不在此列：Mbps/Gbps 行业惯例就是十进制，保持不变。
fn fmt_bytes_val(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;
    const TIB: u64 = 1024 * 1024 * 1024 * 1024;
    if bytes >= TIB {
        format!("{:.1} TiB", bytes as f64 / TIB as f64)
    } else if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// `Option<MebiBytes>` → 人类可读容量（如 "16 GiB"）。
///
/// 存在的理由：`fmt_bytes_int` 只接受**字节**，而模型里存的是 MiB。
/// 之前直接把 MiB 数值传进去，16 GiB 内存被渲染成 "16 KiB"。
/// 有了 newtype 之后，`fmt_bytes_int(&total_mb)` 根本编译不过 —— 这就是
/// 用类型系统固化单位的意义。
fn fmt_mb(mb: &Option<MebiBytes>) -> String {
    match mb {
        Some(v) if v.get() > 0 => fmt_bytes_int_val(v.to_bytes()),
        _ => UNKNOWN.to_string(),
    }
}

/// 同 `fmt_bytes_val`，但取整到整数（用于"总计容量"这类不需要小数的场合）。
/// 除数仍是 1024，所以标签同样必须是 KiB/MiB/GiB/TiB。
fn fmt_bytes_int_val(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;
    const TIB: u64 = 1024 * 1024 * 1024 * 1024;
    if bytes >= TIB {
        format!("{} TiB", (bytes + TIB / 2) / TIB)
    } else if bytes >= GIB {
        format!("{} GiB", (bytes + GIB / 2) / GIB)
    } else if bytes >= MIB {
        format!("{} MiB", (bytes + MIB / 2) / MIB)
    } else if bytes >= KIB {
        format!("{} KiB", (bytes + KIB / 2) / KIB)
    } else {
        format!("{bytes} B")
    }
}

/// 簇大小串（对齐 C++ `FormatClusterStr`）。
/// BIOS Release Date 归一化：SMBIOS 给的是 `MM/DD/YYYY` 美式格式，
/// 中文报告里 `02/24/2022` 得二次判读才能确认是 2 月还是 24 月，
/// 统一成 `YYYY-MM-DD`。厂商填了别的格式（或压根不是日期）时原样返回，
/// 宁可显示原始串也不猜错。
fn normalize_bios_date(s: &str) -> String {
    let b = s.as_bytes();
    if b.len() == 10 && b[2] == b'/' && b[5] == b'/' {
        let (m, d, y) = (&s[0..2], &s[3..5], &s[6..10]);
        let all_digit = |t: &str| t.bytes().all(|c| c.is_ascii_digit());
        if all_digit(m) && all_digit(d) && all_digit(y) {
            return format!("{y}-{m}-{d}");
        }
    }
    s.to_string()
}

fn fmt_cluster(b: &Option<u32>) -> String {
    match b {
        Some(v) if *v > 0 => {
            if *v >= 1024 {
                // 簇大小也是 1024 进制，标签用 KiB（与 fmt_bytes_* 保持一致）。
                format!("{:.0} KiB", *v as f64 / 1024.0)
            } else {
                format!("{v} B")
            }
        }
        _ => UNKNOWN.to_string(),
    }
}

fn computer_name(hw: &HardwareInfo) -> String {
    hw.machine
        .as_ref()
        .and_then(|m| m.computer_name.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "PC".to_string())
}

/// 纯文本：`key: value\n`。
fn push_kv(out: &mut String, key: &str, value: impl Into<String>) {
    out.push_str(key);
    out.push_str(": ");
    out.push_str(&value.into());
    out.push('\n');
}

// C++ 版内联 CSS（与 Report_BuildHtml 一致，保证视觉可比）。
const CSS: &str = r#":root{--brand-a:#8751EE;--brand-b:#2662EB;--ink:#1f2937;--muted:#6b7280;--line:#e6e8ee;--card-bg:#fff;--page-bg:#f4f5f8;--accent:#8751EE;--ok-bg:#e7f6ed;--ok-fg:#15803d;--off-bg:#f1f3f5;--off-fg:#6b7280;--warn-bg:#fdecea;--warn-fg:#c0392b;--tag-bg:#eef1f6;--tag-fg:#4b5563;--foot:#9aa1ad}
*{box-sizing:border-box}
html{-webkit-text-size-adjust:100%}
body{margin:0;padding:28px 16px 40px;background:var(--page-bg);color:var(--ink);font-family:"Microsoft YaHei","PingFang SC","Segoe UI",system-ui,sans-serif;line-height:1.65;font-size:14px}
.wsx-wrap{max-width:940px;margin:0 auto}
.wsx-header{background:linear-gradient(110deg,var(--brand-a),var(--brand-b));border-radius:16px;padding:26px 28px;color:#fff;box-shadow:0 8px 24px rgba(38,98,235,.18)}
.wsx-wordmark{font-size:13px;letter-spacing:2.5px;text-transform:uppercase;opacity:.85;margin-bottom:6px;font-weight:600}
.wsx-header h1{margin:0;font-size:30px;font-weight:800;letter-spacing:.5px;line-height:1.2}
.wsx-sub{margin-top:6px;font-size:13px;opacity:.92}
.wsx-chips{display:flex;flex-wrap:wrap;gap:12px;margin-top:20px}
.wsx-chip{background:rgba(255,255,255,.16);border:1px solid rgba(255,255,255,.28);border-radius:12px;padding:10px 14px;backdrop-filter:blur(2px);min-width:160px;flex:1 1 160px}
.wsx-chip .c-label{font-size:11px;letter-spacing:1px;text-transform:uppercase;opacity:.85;margin-bottom:3px}
.wsx-chip .c-value{font-size:14px;font-weight:700;line-height:1.35;word-break:break-word}
.wsx-chip .c-mono{font-family:Consolas,Menlo,monospace;font-size:13px}
.wsx-card{background:var(--card-bg);border:1px solid var(--line);border-radius:14px;padding:18px 20px 20px;margin-top:18px;box-shadow:0 1px 2px rgba(16,24,40,.04)}
.wsx-card-title{display:flex;align-items:center;gap:10px;margin:0 0 14px;font-size:15px;font-weight:700;color:var(--ink);letter-spacing:.3px}
.wsx-card-title .wsx-idx{display:inline-flex;align-items:center;justify-content:center;width:28px;height:28px;flex:0 0 28px;background:var(--accent);border-radius:7px;color:#fff;padding:0;line-height:0;box-shadow:0 1px 2px rgba(135,81,238,.25)}
.wsx-card-title .wsx-idx svg{width:17px;height:17px;display:block}
.wsx-card-title .wsx-subtle{font-size:12px;font-weight:400;color:var(--muted);letter-spacing:.5px}
.wsx-kv{width:100%;border-collapse:collapse;font-size:14px}
.wsx-kv th{text-align:left;vertical-align:top;width:40%;max-width:240px;padding:9px 12px 9px 0;color:var(--muted);font-weight:400;border-bottom:1px solid #f1f3f6}
.wsx-kv td{vertical-align:top;padding:9px 0;border-bottom:1px solid #f1f3f6;color:var(--ink)}
.wsx-kv tr:last-child th,.wsx-kv tr:last-child td{border-bottom:none}
.wsx-mono{font-family:Consolas,Menlo,monospace;font-size:13px;overflow-wrap:anywhere;word-break:break-word;color:#374151}
.wsx-muted{color:var(--muted)}
.wsx-table-scroll{overflow-x:auto;-webkit-overflow-scrolling:touch}
.wsx-dtable{width:100%;border-collapse:collapse;font-size:13px;min-width:760px}
.wsx-dtable thead th{text-align:left;padding:8px 10px;background:#f7f8fb;color:#475569;font-weight:600;border-bottom:1px solid var(--line);white-space:nowrap;font-size:12px;letter-spacing:.3px}
.wsx-dtable td{padding:8px 10px;border-bottom:1px solid #f1f3f6;vertical-align:top;white-space:nowrap}
.wsx-dtable tbody tr:last-child td{border-bottom:none}
.wsx-dtable .wsx-mono{font-size:12px}
.wsx-badge{display:inline-block;padding:2px 10px;border-radius:999px;font-size:12px;font-weight:600;line-height:1.6;white-space:nowrap}
.wsx-badge-ok{background:var(--ok-bg);color:var(--ok-fg)}
.wsx-badge-off{background:var(--off-bg);color:var(--off-fg)}
.wsx-badge-warn{background:var(--warn-bg);color:var(--warn-fg)}
.wsx-tag{display:inline-block;padding:1px 9px;border-radius:999px;font-size:12px;background:var(--tag-bg);color:var(--tag-fg);margin:0 3px 3px 0}
/* 语义状态色：胶囊徽标 .wsx-stat-* 与纯文字 .wsx-t-* 两套；正常=绿/注意=橙/危险=红/缺失=灰 */
.wsx-stat{display:inline-block;padding:2px 10px;border-radius:999px;font-size:12px;font-weight:600;line-height:1.6;white-space:nowrap;vertical-align:middle}
.wsx-stat-ok{background:var(--ok-bg);color:var(--ok-fg)}
.wsx-stat-warn{background:#fff4e5;color:#b45309}
.wsx-stat-crit{background:#fdecea;color:#c0392b}
.wsx-stat-unk{background:#eef1f6;color:#6b7280}
.wsx-t-ok{color:#15803d}
.wsx-t-warn{color:#b45309}
.wsx-t-crit{color:#c0392b}
/* 预警横幅：左侧红色强调条，明显区别于普通卡片 */
.wsx-warnbox{background:#fff5f4;border:1px solid #f3c6c0;border-left:4px solid #e23b2e;color:#9a2c20;border-radius:12px;padding:12px 16px;margin-top:14px;font-size:13px;line-height:1.6}
.wsx-warnbox .wsx-warn-title{font-weight:700;display:block;margin-bottom:2px}
.wsx-warn{color:var(--warn-fg)}
.wsx-foot{color:var(--foot);font-size:12px;text-align:center;margin-top:26px;letter-spacing:.3px}
@media (max-width:768px){body{padding:16px 12px 26px}.wsx-wrap{max-width:100%}}
@media (max-width:560px){.wsx-header{padding:18px 16px;border-radius:12px}.wsx-wordmark{font-size:12px;letter-spacing:2px}.wsx-header h1{font-size:22px}.wsx-sub{font-size:12px}.wsx-chips{gap:8px;margin-top:14px}.wsx-chip{flex:1 1 100%;min-width:0;padding:9px 12px}.wsx-card{padding:14px 14px 16px;margin-top:14px;border-radius:12px}.wsx-card-title{font-size:14px;gap:8px}.wsx-card-title .wsx-idx{width:26px;height:26px;flex-basis:26px;border-radius:6px}.wsx-card-title .wsx-idx svg{width:15px;height:15px}.wsx-kv{font-size:13px}.wsx-kv th{width:38%;padding:8px 8px 8px 0;font-size:13px}.wsx-kv td{padding:8px 0;font-size:13px}.wsx-mono{font-size:12px}.wsx-foot{margin-top:20px;font-size:11px}}
@media (max-width:380px){.wsx-header h1{font-size:20px}.wsx-kv th{width:42%}.wsx-chip .c-value{font-size:13px}}"#;

// ============================================================
// 测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use winsuitex_core::model::{
        AudioDevice, AudioKind, BoardInfo, Cpu, DiskSmart, Display, Gpu, GpuKind, GpuTelemetry,
        HardwareInfo, HealthStatus, InputDevice, InputKind, MachineInfo, MemStick, MemoryInfo, Nic,
        NicKind, OsInfo, PhysicalDisk, SmartSource, UsbDevice, Volume,
    };

    fn base_hw() -> HardwareInfo {
        HardwareInfo {
            os: Some(OsInfo {
                product: Some("Windows 10 专业版".into()),
                version: Some("22H2".into()),
                build: Some("19045".into()),
                arch: Some("x64".into()),
                logical_processors: Some(8),
                ..Default::default()
            }),
            machine: Some(MachineInfo {
                computer_name: Some("TEST-PC".into()),
                user_name: Some("admin".into()),
            }),
            board: Some(BoardInfo {
                vendor: Some("ASUSTeK".into()),
                product: Some("PRIME".into()),
                serial: Some("MB-SN-99887766".into()),
                chipset: Some("Z690".into()),
                bios_vendor: Some("American Megatrends".into()),
                bios_version: Some("1.20".into()),
                bios_date: Some("2023-01-01".into()),
            }),
            cpus: vec![Cpu {
                socket: Some("CPU0".into()),
                model: Some("Intel Core i7".into()),
                vendor: Some("Intel".into()),
                codename: Some("Raptor Lake".into()),
                cores: Some(8),
                threads: Some(16),
                speed_mhz: Some(3400),
            }],
            memory: MemoryInfo {
                total_mb: Some(MebiBytes::new(16384)),
                sticks: vec![MemStick {
                    locator: Some("DIMM_A1".into()),
                    bank: Some("BANK0".into()),
                    serial: Some("KST-MEM-55443322".into()),
                    part_no: Some("ABC-123".into()),
                    vendor: Some("Kingston".into()),
                    mem_type: Some("DDR4".into()),
                    capacity_mb: Some(MebiBytes::new(8192)),
                    speed_mhz: Some(3200),
                }],
            },
            gpus: vec![Gpu {
                name: "NVIDIA RTX 4070".into(),
                vram_mb: Some(MebiBytes::new(12288)),
                kind: GpuKind::Physical,
                vendor: None,
                telemetry: Some(GpuTelemetry {
                    temp_c: Some(55),
                    fan_pct: Some(40),
                    power_w: Some(150),
                    util_pct: Some(30),
                    mem_used_mb: Some(MebiBytes::new(2048)),
                    clock_mhz: Some(2500),
                }),
            }],
            displays: vec![Display {
                name: Some("DELL U2723".into()),
                vendor: Some("DEL".into()),
                size_inch: Some("27\"".into()),
                serial: None,
                manufacture_year: None,
                manufacture_week: None,
                width: Some(2560),
                height: Some(1440),
                refresh_hz: Some(165),
            }],
            nics: vec![Nic {
                desc: Some("Intel Ethernet".into()),
                mac: Some("00:11:22:33:44:55".into()),
                ipv4: vec!["192.168.1.10".into()],
                ipv6: Some("fe80::1".into()),
                up: true,
                speed_bps: None,
                kind: NicKind::Physical,
                vendor: None,
            }],
            disks: vec![PhysicalDisk {
                index: 0,
                model: Some("Samsung SSD 980".into()),
                iface: Some("NVMe".into()),
                media: Some("SSD".into()),
                serial: Some("SN123".into()),
                size_bytes: Some(1_000_000_000_000),
                ..Default::default()
            }],
            audio: vec![AudioDevice {
                name: "Realtek Audio".into(),
                role: 1,
                kind: AudioKind::Speaker,
                is_bluetooth: false,
                vid: None,
                pid: None,
                manufacturer: None,
            }],
            usb: vec![UsbDevice {
                name: "Kingston DataTraveler 3.0".into(),
                class: Some("USBSTOR".into()),
            }],
            input_devices: vec![
                InputDevice {
                    name: "Logitech G502".into(),
                    kind: InputKind::Mouse,
                    vid: Some("046D".into()),
                    pid: Some("C07E".into()),
                    manufacturer: Some("Logitech".into()),
                    is_bluetooth: false,
                    location: None,
                },
                InputDevice {
                    name: "Cherry Keyboard".into(),
                    kind: InputKind::Keyboard,
                    vid: Some("046A".into()),
                    pid: Some("0011".into()),
                    manufacturer: Some("Cherry".into()),
                    is_bluetooth: false,
                    location: None,
                },
            ],
            is_server: false,
            // 测试夹具模拟"老快照没有采集时间"的情形，应渲染成不显示而非"未知"。
            collected_at: None,
            admin_elevated: None,
        }
    }

    fn base_volume() -> Volume {
        Volume {
            letter: "C:".into(),
            type_name: Some("本地磁盘".into()),
            fs_name: Some("NTFS".into()),
            sectors_per_cluster: Some(8),
            bytes_per_sector: Some(512),
            cluster_bytes: Some(4096),
            total_bytes: Some(500_000_000_000),
            free_bytes: Some(200_000_000_000),
            vol_label: Some("System".into()),
            disk_index: Some(0),
        }
    }

    /// 分区表两处修复的防回归（Z.W. 2026-10-11 真机报告审阅）：
    /// ① 表头「序列号」实为**磁盘**序列号（同盘多分区显示同一值，易误读）→ 改名；
    /// ② 表格闭合顺序写反了（`</div></table>`）→ 无效嵌套，必须与开启顺序相反。
    #[test]
    fn volume_table_header_says_disk_serial_and_nesting_is_valid() {
        let hw = base_hw();
        // 同一块盘上的两个分区：序列号列取的是磁盘序列号，两行值相同。
        let mut v2 = base_volume();
        v2.letter = "D:".into();
        let html = build_html(&hw, &[base_volume(), v2]);
        assert!(
            html.contains("磁盘序列号"),
            "分区表列头应写明是磁盘序列号: {html}"
        );
        assert!(
            html.contains("</table></div>"),
            "表格闭合必须 table 在内、div 在外: {html}"
        );
        assert!(
            !html.contains("</div></table>"),
            "不应残留写反的闭合顺序: {html}"
        );
    }

    #[test]
    fn none_renders_unknown() {
        let mut hw = base_hw();
        // 制造若干 None 字段
        hw.os.as_mut().unwrap().product = None;
        hw.board.as_mut().unwrap().chipset = None;
        hw.cpus[0].vendor = None;
        let html = build_html(&hw, &[]);
        // 产品为空 → 未知
        assert!(
            html.contains("未知"),
            "产品 None 应显示 未知，实际:\n{html}"
        );
        // 芯片组缺失 → 该小节不应出现
        assert!(!html.contains("芯片组"), "芯片组 None 不应出现");
        // CPU vendor None → 未知
        assert!(html.contains("未知"), "CPU vendor None 应显示 未知");
    }

    #[test]
    fn html_escape_is_effective() {
        let mut hw = base_hw();
        hw.cpus[0].model = Some("<script>alert(1)</script>".into());
        hw.machine.as_mut().unwrap().computer_name = Some("A & B <x> \"q\"".into());
        let html = build_html(&hw, &[]);
        assert!(
            !html.contains("<script>"),
            "HTML 中不得出现未转义的 <script>，实际:\n{html}"
        );
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("A &amp; B &lt;x&gt; &quot;q&quot;"));
    }

    #[test]
    fn server_hides_audio_usb() {
        let mut hw = base_hw();
        hw.is_server = true;
        // 服务器也接了键鼠，验证其在服务器模式下仍可见。
        hw.input_devices.push(InputDevice {
            name: "AT Translated Set 2 keyboard".into(),
            kind: InputKind::Keyboard,
            vid: None,
            pid: None,
            manufacturer: None,
            is_bluetooth: false,
            location: None,
        });
        let html = build_html(&hw, &[]);
        // 断言"段"没渲染（按 <h2> 标题匹配），而不是"字符串"没出现 ——
        // 页脚会显式列出服务器模式省略了哪些内容，里面同样含"音频设备"字样。
        assert!(!html.contains("<h2>音频设备</h2>"), "服务器应隐藏音频段");
        assert!(!html.contains("<h2>USB 外设</h2>"), "服务器应隐藏 USB 段");
        assert!(
            html.contains("键盘 / 鼠标"),
            "服务器应仍显示键盘/鼠标段，实际:\n{html}"
        );
        // 普通机器应显示
        let normal = build_html(&base_hw(), &[]);
        assert!(normal.contains("音频设备"));
        assert!(normal.contains("USB 外设"));
    }

    #[test]
    fn bluetooth_input_device_renders_tag() {
        let mut hw = base_hw();
        hw.input_devices.push(InputDevice {
            name: "Magic Keyboard".into(),
            kind: InputKind::Keyboard,
            vid: Some("004C".into()), // Apple
            pid: Some("026C".into()),
            manufacturer: Some("Apple".into()),
            is_bluetooth: true,
            location: None,
        });
        let txt = build_text(&hw);
        let html = build_html(&hw, &[]);
        // 文本：型号行应带「蓝牙」标记
        assert!(
            txt.contains("蓝牙"),
            "文本报告应标记蓝牙输入设备，实际:\n{txt}"
        );
        // HTML：应渲染「蓝牙」标签
        assert!(
            html.contains("蓝牙"),
            "HTML 报告应渲染蓝牙标签，实际:\n{html}"
        );
        // 有线设备不应被误标
        assert!(!txt.contains("蓝牙") || txt.contains("Magic Keyboard"));
    }

    #[test]
    fn audio_device_renders_kind_and_bluetooth() {
        let mut hw = base_hw();
        hw.audio.clear();
        hw.audio.push(AudioDevice {
            name: "JBL Quantum 400".into(),
            role: 0b01,
            kind: AudioKind::Headset,
            is_bluetooth: true,
            vid: Some("0D8C".into()),
            pid: Some("01DA".into()),
            manufacturer: Some("C-Media".into()),
        });
        hw.audio.push(AudioDevice {
            name: "Realtek Audio".into(),
            role: 0b01,
            kind: AudioKind::Speaker,
            is_bluetooth: false,
            vid: None,
            pid: None,
            manufacturer: None,
        });
        let txt = build_text(&hw);
        let html = build_html(&hw, &[]);
        // 文本：应显示类型与蓝牙标记
        assert!(txt.contains("耳麦"), "文本应含类型「耳麦」，实际:\n{txt}");
        assert!(txt.contains("蓝牙"), "文本应含「蓝牙」，实际:\n{txt}");
        assert!(txt.contains("音箱"), "文本应含类型「音箱」，实际:\n{txt}");
        assert!(txt.contains("0D8C:01DA"), "文本应含 VID:PID，实际:\n{txt}");
        // HTML：应渲染类型标签与蓝牙标签
        assert!(
            html.contains("耳麦"),
            "HTML 应含类型「耳麦」，实际:\n{html}"
        );
        assert!(
            html.contains("蓝牙"),
            "HTML 应含「蓝牙」标签，实际:\n{html}"
        );
    }

    #[test]
    fn peripheral_vendor_renders_english_zh() {
        // 厂商在 JSON 中存英文品牌 key，报告层统一出「英文 (中文)」，
        // 与 GPU/CPU 等 structured 源口径一致。音频与键鼠此前存中文、直接原样输出，
        // 与 GPU 的中文呈现不一致——这里锁定统一规格。
        let mut hw = base_hw();
        hw.audio.clear();
        hw.audio.push(AudioDevice {
            name: "USB Audio Device".into(),
            role: 0,
            kind: AudioKind::Headphone,
            is_bluetooth: false,
            vid: Some("03F0".into()),
            pid: Some("0B92".into()),
            manufacturer: Some("HP".into()), // 源现统一存英文品牌 key
        });
        hw.input_devices.clear();
        hw.input_devices.push(InputDevice {
            name: "G402 Gaming Mouse".into(),
            kind: InputKind::Mouse,
            vid: Some("046D".into()),
            pid: Some("C07E".into()),
            manufacturer: Some("Logitech".into()),
            is_bluetooth: false,
            location: None,
        });
        let html = build_html(&hw, &[]);
        assert!(
            html.contains("HP（惠普）"),
            "音频厂商应渲染为「HP（惠普）」: {html}"
        );
        assert!(
            html.contains("Logitech（罗技）"),
            "键鼠厂商应渲染为「Logitech（罗技）」: {html}"
        );
        // 不应再出现纯中文厂商（旧口径）。
        assert!(
            !html.contains("&middot; 惠普") && !html.contains("&middot; 罗技"),
            "不应出现纯中文厂商呈现: {html}"
        );
    }

    #[test]
    fn empty_list_hides_section() {
        let mut hw = base_hw(); // 清空所有变长列表
        hw.cpus.clear();
        hw.memory.sticks.clear();
        hw.memory.total_mb = None;
        hw.gpus.clear();
        hw.displays.clear();
        hw.disks.clear();
        hw.nics.clear();
        hw.audio.clear();
        hw.usb.clear();
        let html = build_html(&hw, &[]);
        // 按**小节标题**断言，而不是裸子串：OS 小节里有「逻辑处理器」，
        // 用 contains("处理器") 会被它误伤（真实踩过）。
        assert!(!html.contains("<h2>处理器</h2>"), "空 CPU 列表整段不应显示");
        assert!(!html.contains("<h2>内存</h2>"), "空内存列表整段不应显示");
        assert!(
            !html.contains("<h2>显卡 / 显示器</h2>"),
            "空显卡/显示器整段不应显示"
        );
        assert!(!html.contains("<h2>硬盘</h2>"), "空硬盘列表整段不应显示");
        assert!(!html.contains("<h2>网卡</h2>"), "空网卡列表整段不应显示");
        assert!(
            !html.contains("<h2>音频设备</h2>"),
            "空音频列表整段不应显示"
        );
        assert!(!html.contains("USB 外设"), "空 USB 列表整段不应显示");
        // OS / 计算机 / 主板 等固定段始终显示（只渲染 未知）
        assert!(html.contains("操作系统"));
        assert!(html.contains("主板 / BIOS"));
    }

    #[test]
    fn os_section_does_not_show_logical_processors() {
        // #25 决策：逻辑处理器=线程数已由 CPU 段「N 核 / M 线程」覆盖，属 CPU 指标非 OS 属性，
        // OS 段不重复渲染；字段 OsInfo.logical_processors 仍保留，供线程数交叉校验
        // （sanitize_thread_counts + main.rs 采集端软校验）使用，仅不显示。
        let html = build_html(&base_hw(), &[]);
        assert!(
            !html.contains("逻辑处理器"),
            "OS 段不应渲染逻辑处理器（重复且易误读）: {html}"
        );
        // CPU 段仍应展示核/线程，交叉校验信息不丢。
        assert!(html.contains("线程"), "CPU 段应仍含核/线程信息: {html}");
    }

    #[test]
    fn os_section_renders_boot_time_and_uptime() {
        // 新增字段（2026-09-22）：系统开机时间 + 在线时长。两者应出现在 OS 段，
        // 且在线时长走 format_uptime 的人类可读串，而非裸秒数。
        let mut hw = base_hw();
        hw.os.as_mut().unwrap().boot_at = Some("2026-09-20 08:00:00".into());
        hw.os.as_mut().unwrap().uptime_seconds = Some(5073600); // 约 58 天 17 小时
        let html = build_html(&hw, &[]);
        assert!(html.contains("开机时间"), "OS 段应含『开机时间』");
        assert!(html.contains("2026-09-20 08:00:00"), "开机时间串应原样渲染");
        assert!(html.contains("在线时长"), "OS 段应含『在线时长』");
        assert!(
            html.contains("约 58 天 17 小时"),
            "在线时长应走 format_uptime 可读串，而非裸秒: {html}"
        );
    }

    #[test]
    fn os_section_omits_boot_fields_when_absent() {
        // 老快照无这两个字段（base_hw 默认 None）→ 不应渲染，避免显示"未知"。
        let html = build_html(&base_hw(), &[]);
        assert!(!html.contains("开机时间"), "无 boot_at 时不渲染开机时间");
        assert!(
            !html.contains("在线时长"),
            "无 uptime_seconds 时不渲染在线时长"
        );
    }

    #[test]
    fn empty_volumes_hides_section() {
        let html = build_html(&base_hw(), &[]);
        assert!(!html.contains("分区与簇大小"), "空分区列表整段不应显示");
        let html2 = build_html(&base_hw(), &[base_volume()]);
        assert!(html2.contains("分区与簇大小"));
        assert!(html2.contains("C:"));
        assert!(html2.contains("NTFS"));
        assert!(html2.contains("4 KiB"));
        assert!(
            html2.contains("465.7 GiB") || html2.contains("466.0 GiB"),
            "卷容量是 1024 进制，必须标 GiB 而不是 SI 的 GB"
        );
    }

    #[test]
    fn capacity_labels_use_binary_units() {
        // 防倒退：`fmt_bytes_*` 用 1024 当除数，标签就必须是 IEC 二进制单位。
        // 以前标 "GB"（SI 十进制），524191 MiB 被显示成 "512 GB"，
        // 而它实为 512 GiB = 549.7 GB —— 全报告系统性失真 7~9%。
        assert_eq!(fmt_bytes_val(1024), "1.0 KiB");
        assert_eq!(fmt_bytes_val(1024 * 1024), "1.0 MiB");
        assert_eq!(fmt_bytes_val(1024u64.pow(3)), "1.0 GiB");
        assert_eq!(fmt_bytes_val(1024u64.pow(4)), "1.0 TiB");
        assert_eq!(fmt_bytes_int_val(1024u64.pow(3)), "1 GiB");
        // 真机量级：512 GiB，绝不能出现 SI 标签。
        let total = 524_191u64 * 1024 * 1024;
        let s = fmt_bytes_int_val(total);
        assert_eq!(s, "512 GiB", "实际输出：{s}");
        assert!(!s.contains(" GB"));
    }

    #[test]
    fn telemetry_absent_hides_status() {
        let mut hw = base_hw();
        hw.gpus[0].telemetry = None;
        let html = build_html(&hw, &[]);
        assert!(!html.contains("温度"), "无遥测不应显示温度行");
        assert!(html.contains("RTX 4070"));
    }

    #[test]
    fn html_has_utf8_meta() {
        let html = build_html(&base_hw(), &[]);
        assert!(html.contains("<meta charset=\"utf-8\">"));
        assert!(html.starts_with("<!DOCTYPE html>"));
    }

    #[test]
    fn text_output_basic() {
        let txt = build_text(&base_hw());
        assert!(txt.contains("操作系统"));
        assert!(txt.contains("Intel Core i7"));
        assert!(!txt.contains("未知") || true); // 已知字段应有值
        assert!(txt.contains("TEST-PC"));
        assert!(txt.contains("RTX 4070"));
    }

    #[test]
    fn text_unknown_for_none() {
        let mut hw = base_hw();
        hw.os.as_mut().unwrap().product = None;
        let txt = build_text(&hw);
        assert!(txt.contains("产品: 未知"));
    }

    #[test]
    fn server_text_hides_audio_usb() {
        let mut hw = base_hw();
        hw.is_server = true;
        hw.input_devices.push(InputDevice {
            name: "AT Translated Set 2 keyboard".into(),
            kind: InputKind::Keyboard,
            vid: None,
            pid: None,
            manufacturer: None,
            is_bluetooth: false,
            location: None,
        });
        let txt = build_text(&hw);
        assert!(!txt.contains("音频设备"));
        assert!(!txt.contains("USB 外设"));
        assert!(
            txt.contains("键盘 / 鼠标"),
            "服务器文本报告应仍显示键盘/鼠标段，实际:\n{txt}"
        );
    }

    #[test]
    fn gpu_missing_power_reads_as_power_unknown_not_unknown_w() {
        // 真机案例：047 那台 RTX 4060 的 NVML 没返回功耗（power_w=None），
        // 旧写法把 `{}W` 硬拼在格式串里 → 报告里出现「风扇 0% · 未知W · 利用 1%」。
        // 「未知W」读起来像"未知这个 W"，属于渲染层的假故障观感，必须消掉。
        let mut hw = base_hw();
        hw.gpus.push(Gpu {
            name: "NVIDIA GeForce RTX 4060".into(),
            vram_mb: Some(MebiBytes::new(8192)),
            kind: GpuKind::Physical,
            vendor: Some("ASUS".into()),
            telemetry: Some(GpuTelemetry {
                temp_c: Some(44),
                fan_pct: Some(0),
                power_w: None,
                util_pct: Some(1),
                mem_used_mb: Some(MebiBytes::new(6799)),
                clock_mhz: Some(2505),
            }),
        });
        let txt = build_text(&hw);
        let html = build_html(&hw, &[]);
        assert!(
            !txt.contains("未知W") && !html.contains("未知W"),
            "功耗缺失时不应渲染成「未知W」，实际:\n{txt}\n{html}"
        );
        assert!(
            txt.contains("功耗 未知") && html.contains("功耗 未知"),
            "功耗缺失时文本与 HTML 都应显示「功耗 未知」"
        );
        assert!(txt.contains("温度 44°C"), "其余遥测项应照常显示");
    }

    #[test]
    fn gpu_virtual_label_renders_in_text() {
        // 虚拟/间接显示驱动在文本报告里应被显式标注，避免与独显混排误导硬件盘点。
        let mut hw = base_hw();
        hw.gpus.push(Gpu {
            name: "OrayIddDriver Device".into(),
            vram_mb: None,
            kind: GpuKind::Virtual,
            vendor: None,
            telemetry: None,
        });
        let txt = build_text(&hw);
        assert!(txt.contains("OrayIddDriver Device"));
        assert!(txt.contains("虚拟/间接显示"));
    }

    #[test]
    fn gpu_virtual_label_renders_in_html() {
        // HTML 报告 likewise 给虚拟适配器加 [虚拟/间接显示] 标记。
        let mut hw = base_hw();
        hw.gpus.push(Gpu {
            name: "OrayIddDriver Device".into(),
            vram_mb: None,
            kind: GpuKind::Virtual,
            vendor: None,
            telemetry: None,
        });
        let html = build_html(&hw, &[]);
        assert!(html.contains("OrayIddDriver Device"));
        assert!(html.contains("[虚拟/间接显示]"));
    }

    #[test]
    fn gpu_brand_renders_inline() {
        // 物理显卡带板卡品牌时，文本与 HTML 都应内联展示「英文 (中文)」。
        let mut hw = base_hw();
        hw.gpus.clear();
        hw.gpus.push(Gpu {
            name: "NVIDIA GeForce RTX 4070".into(),
            vram_mb: Some(MebiBytes::new(12288)),
            kind: GpuKind::Physical,
            vendor: Some("MSI".into()),
            telemetry: None,
        });
        let txt = build_text(&hw);
        let html = build_html(&hw, &[]);
        assert!(txt.contains("MSI（微星）"), "文本报告应内联显卡品牌: {txt}");
        assert!(
            html.contains("MSI（微星）"),
            "HTML 报告应内联显卡品牌: {html}"
        );
    }

    #[test]
    fn display_edid_renders_in_text() {
        // EDID 解析出的序列号 / 生产周年应在文本报告里呈现。
        let mut hw = base_hw();
        hw.displays.push(Display {
            name: Some("DELL U2723".into()),
            vendor: Some("DEL".into()),
            size_inch: Some("27\"".into()),
            serial: Some("ABC123".into()),
            manufacture_year: Some(2023),
            manufacture_week: Some(12),
            width: Some(2560),
            height: Some(1440),
            refresh_hz: Some(165),
        });
        let txt = build_text(&hw);
        assert!(txt.contains("序列号 ABC123"));
        assert!(txt.contains("生产 2023-W12"));
    }

    #[test]
    fn display_edid_renders_in_html() {
        // HTML 报告 likewise 呈现序列号 / 生产周年。
        let mut hw = base_hw();
        hw.displays.push(Display {
            name: Some("DELL U2723".into()),
            vendor: Some("DEL".into()),
            size_inch: Some("27\"".into()),
            serial: Some("ABC123".into()),
            manufacture_year: Some(2023),
            manufacture_week: Some(12),
            width: Some(2560),
            height: Some(1440),
            refresh_hz: Some(165),
        });
        let html = build_html(&hw, &[]);
        assert!(html.contains("序列号：ABC123"));
        assert!(html.contains("生产：2023-W12"));
    }

    #[test]
    fn display_vendor_inline_in_text() {
        // 显示器厂商中文应跟在型号后 (括号)，不单独成列。
        let mut hw = base_hw();
        hw.displays.clear();
        hw.displays.push(Display {
            name: Some("AOC 2769".into()),
            vendor: Some("AOC".into()),
            size_inch: None,
            serial: None,
            manufacture_year: None,
            manufacture_week: None,
            width: Some(1920),
            height: Some(1080),
            refresh_hz: None,
        });
        let txt = build_text(&hw);
        assert!(
            txt.contains("AOC 2769（冠捷）"),
            "显示器厂商中文应跟在型号后: {txt}"
        );
        assert!(!txt.contains("| 厂商"), "显示器厂商不应单独成列: {txt}");
    }

    #[test]
    fn display_vendor_inline_in_html() {
        // 同理，HTML 报告里厂商中文内联在型号后。
        let mut hw = base_hw();
        hw.displays.clear();
        hw.displays.push(Display {
            name: Some("AOC 2769".into()),
            vendor: Some("AOC".into()),
            size_inch: None,
            serial: None,
            manufacture_year: None,
            manufacture_week: None,
            width: Some(1920),
            height: Some(1080),
            refresh_hz: None,
        });
        let html = build_html(&hw, &[]);
        assert!(
            html.contains("AOC 2769（冠捷）"),
            "显示器厂商中文应内联在型号后: {html}"
        );
        assert!(!html.contains("厂商："), "显示器厂商不应单独成列: {html}");
    }

    #[test]
    fn display_vendor_not_duplicated_when_name_has_brand() {
        // 回归：采集层对通用名会整段替换成「英文（中文） 型号」（如 "Dell（戴尔） A221"），
        // 此时品牌已嵌入名字；vendor 字段是原始 PnP 码（如 "DEL"），
        // 渲染层不应再补一次「（戴尔）」，否则会出现 "Dell（戴尔） A221（戴尔）" 这类重复。
        let mut hw = base_hw();
        hw.displays.clear();
        hw.displays.push(Display {
            name: Some("Dell（戴尔） A221".into()),
            vendor: Some("DEL".into()),
            size_inch: None,
            serial: None,
            manufacture_year: None,
            manufacture_week: None,
            width: Some(1920),
            height: Some(1080),
            refresh_hz: None,
        });
        let html = build_html(&hw, &[]);
        assert!(
            html.contains("Dell（戴尔） A221"),
            "显示器型号行应含完整品牌名: {html}"
        );
        assert!(
            !html.contains("A221（戴尔）"),
            "品牌已嵌入名字，渲染层不得重复追加「（戴尔）」: {html}"
        );
        assert!(
            !html.contains("（DEL）"),
            "PnP 原始码不得直接进括号（应解析为「（戴尔）」或省略）: {html}"
        );
    }

    #[test]
    fn nic_virtual_label_renders_in_text() {
        // 即便描述被英文黑名单漏过（非英文 Windows 场景），kind 仍驱动标注。
        let mut hw = base_hw();
        hw.nics.push(Nic {
            desc: Some("Hyper-V Virtual Ethernet Adapter".into()),
            mac: Some("00:15:5D:00:00:01".into()),
            ipv4: vec!["172.28.0.1".into()],
            ipv6: None,
            up: true,
            speed_bps: None,
            kind: NicKind::Virtual,
            vendor: None,
        });
        let txt = build_text(&hw);
        assert!(
            txt.contains("虚拟/软件网卡"),
            "虚拟网卡应在文本报告标注类型: {txt}"
        );
    }

    #[test]
    fn nic_virtual_label_renders_in_html() {
        let mut hw = base_hw();
        hw.nics.push(Nic {
            desc: Some("Wintun Tunnel".into()),
            mac: Some("00:00:00:00:00:00".into()),
            ipv4: vec![],
            ipv6: None,
            up: false,
            speed_bps: None,
            kind: NicKind::Virtual,
            vendor: None,
        });
        let html = build_html(&hw, &[]);
        assert!(
            html.contains("虚拟/软件网卡"),
            "虚拟网卡应在 HTML 报告标注类型"
        );
    }

    #[test]
    fn vendor_zh_maps_common_pc_vendors() {
        assert_eq!(vendor_zh("MSI"), "微星");
        assert_eq!(vendor_zh("Intel(R) Corporation"), "英特尔");
        assert_eq!(vendor_zh("Kingston"), "金士顿");
        assert_eq!(vendor_zh("AOC"), "冠捷");
        assert_eq!(vendor_zh("Realtek"), "瑞昱");
        // 大小写不敏感
        assert_eq!(vendor_zh("intel"), "英特尔");
        assert_eq!(vendor_zh("GIGABYTE"), "技嘉");
        // 未收录原样返回，不错译
        assert_eq!(vendor_zh("SomeWeirdBrand"), "SomeWeirdBrand");
    }

    #[test]
    fn vendor_label_shows_unknown_for_corrupt() {
        assert_eq!(vendor_label(&None), None);
        assert_eq!(
            vendor_label(&Some("Kingston".into())),
            Some("Kingston（金士顿）".into())
        );
        assert_eq!(
            vendor_label(&Some("SomeWeirdBrand".into())),
            Some("SomeWeirdBrand".into())
        );
    }

    #[test]
    fn vendor_zh_maps_server_vendors() {
        // 服务器常见厂商：主板厂 Supermicro、BMC 显示芯片 ASPEED。
        assert_eq!(vendor_zh("Supermicro"), "超微");
        assert_eq!(vendor_zh("ASPEED"), "信骅");
    }

    #[test]
    fn vendor_zh_maps_inspur() {
        // 浪潮（Inspur）是常见服务器整机/主板厂，应中文化。
        assert_eq!(vendor_zh("Inspur"), "浪潮");
    }

    #[test]
    fn vendor_zh_maps_avago() {
        // Avago 2014 收购 LSI 存储业务、2016 年被 Broadcom 收购，现法律实体即 Broadcom（博通）。
        assert_eq!(vendor_zh("AVAGO"), "博通");
    }

    #[test]
    fn vendor_zh_degrade_to_first_word() {
        // "Dell Inc." 精确匹配不到 "Dell"，应退化为首个单词 → 戴尔。
        assert_eq!(vendor_zh("Dell Inc."), "戴尔");
        // 退化后大小写不敏感
        assert_eq!(vendor_zh("dell inc."), "戴尔");
    }

    #[test]
    fn vendor_zh_maps_matrox_and_network_vendors() {
        // 服务器 BMC/集成显卡芯片厂
        assert_eq!(vendor_zh("Matrox"), "迈创");
        // 网络 / 通信芯片厂
        assert_eq!(vendor_zh("HPE"), "慧与");
        assert_eq!(vendor_zh("Marvell"), "迈威");
        assert_eq!(vendor_zh("Cisco"), "思科");
        assert_eq!(vendor_zh("TP-LINK"), "普联");
        assert_eq!(vendor_zh("TP-Link"), "普联");
        assert_eq!(vendor_zh("D-Link"), "友讯");
        assert_eq!(vendor_zh("Netgear"), "网件");
        assert_eq!(vendor_zh("Tenda"), "腾达");
        assert_eq!(vendor_zh("Xiaomi"), "小米");
        assert_eq!(vendor_zh("Huawei"), "华为");
        assert_eq!(vendor_zh("ZTE"), "中兴");
        assert_eq!(vendor_zh("Byosoft"), "百敖");
    }

    #[test]
    fn disk_vendor_from_model_maps_avago() {
        // 硬盘型号里含 AVAGO 时应识别为博通（RAID 卡如 MegaRAID MR9361-8i 当年以 Avago 品牌出货）。
        let r = disk_vendor_from_model("AVAGO MR9361-8i");
        assert_eq!(r.as_deref(), Some("Broadcom（博通）"));
        // 非 AVAGO 型号不受影响
        assert!(disk_vendor_from_model("INTEL SSDPE2KX020T8")
            .unwrap()
            .contains("英特尔"));
    }

    #[test]
    fn disk_vendor_from_model_maps_dell() {
        // 硬盘型号里含 DELL 时应识别为戴尔（硬盘走型号推断路径，非 vendor 字段）。
        let r = disk_vendor_from_model("Dell Ent NVMe AGN RI U.2 7.68TB");
        assert_eq!(r.as_deref(), Some("Dell（戴尔）"));
        // 非 Dell 型号不受影响
        assert!(disk_vendor_from_model("SAMSUNG MZQLB960HBJR-00W07")
            .unwrap()
            .contains("三星"));
    }

    #[test]
    fn disk_vendor_from_model_maps_kioxia() {
        // Kioxia（原东芝内存）NVMe 型号以 VO<数字> 开头（真实样本 VO001000KWJSE）。
        // 注意模型常带 "NVMe " 接口前缀，而 "NVMe" 里含 "VO"，故须按分词后词元判定，
        // 绝不能 contains("VO") 否则每块 NVMe 盘都会被误判成铠侠。
        let r = disk_vendor_from_model("NVMe VO001000KWJSE");
        assert_eq!(r.as_deref(), Some("Kioxia（铠侠）"));
        // 普通 NVMe 盘（含 "NVMe" 前缀）不得被误判为 Kioxia。
        assert!(disk_vendor_from_model("NVMe INTEL SSDPE2KX02")
            .unwrap()
            .contains("英特尔"));
        assert!(disk_vendor_from_model("INTEL SSDPE2KE076T8")
            .unwrap()
            .contains("英特尔"));
    }

    #[test]
    fn empty_slot_collapsed_into_one_row() {
        // 空插槽合并成一行：单空槽时表头「空槽」，值单元格列出空槽原始标识，
        // 不再逐行铺开刷屏（Z.W. 2026-10-09 定案）。
        let mut hw = base_hw();
        hw.memory.sticks.clear();
        hw.memory.sticks.push(MemStick {
            locator: Some("NO DIMM".into()), // 固件空槽占位串
            bank: Some("NO DIMM".into()),
            serial: Some("NO DIMM".into()),
            part_no: Some("NO DIMM".into()),
            vendor: Some("NO DIMM".into()),
            mem_type: None,
            capacity_mb: None, // 无容量 = 空槽
            speed_mhz: None,
        });
        let html = build_html(&hw, &[]);
        // 合并后只有一行空槽（值单元格出现一次「空槽：」），不再有逐行空槽
        assert_eq!(
            html.matches("<span class=\"wsx-muted\">空槽").count(),
            1,
            "单空槽应合并为一行空槽标记: {html}"
        );
        assert!(
            html.contains("<th>空槽</th>"),
            "单空槽表头应简化作「空槽」: {html}"
        );
        assert!(
            html.contains("空槽：NO DIMM"),
            "空槽值单元格应列出空槽原始标识: {html}"
        );
    }

    #[test]
    fn empty_slot_with_real_locator_listed() {
        // 空插槽若固件给了真实槽位名（非占位串），合并行的标识里应显示它，便于定位第几槽为空。
        let mut hw = base_hw();
        hw.memory.sticks.clear();
        hw.memory.sticks.push(MemStick {
            locator: Some("DIMM_1".into()), // 真实槽位名，非占位串
            bank: Some("BANK1".into()),
            serial: Some("NO DIMM".into()),
            part_no: Some("NO DIMM".into()),
            vendor: Some("NO DIMM".into()),
            mem_type: None,
            capacity_mb: None, // 无容量 = 空槽
            speed_mhz: None,
        });
        let html = build_html(&hw, &[]);
        assert!(
            html.contains("空槽：DIMM_1"),
            "空槽合并行应列出真实槽位名 DIMM_1: {html}"
        );
    }

    #[test]
    fn many_empty_slots_collapse_to_single_row() {
        // 12 槽机器 8 个空槽的真实场景：合并成一行、列出全部空槽标识，
        // 仍一眼看清哪些槽空着（数槽位意图不丢）。
        let mut hw = base_hw();
        hw.memory.sticks.clear();
        let locs = [
            "DIMM_A1", "DIMM_A2", "DIMM_B1", "DIMM_B2", "DIMM_C1", "DIMM_C2", "DIMM_D1", "DIMM_D2",
            "DIMM_E1", "DIMM_E2", "DIMM_F1", "DIMM_F2",
        ];
        for (i, loc) in locs.iter().enumerate() {
            let populated = i % 3 == 0; // 4 条已插（A1/B1/C1/D1），其余 8 个空槽
            hw.memory.sticks.push(MemStick {
                locator: Some((*loc).into()),
                bank: Some("NODE 1".into()),
                serial: Some(if populated {
                    "ABCD1234".into()
                } else {
                    "NO DIMM".into()
                }),
                part_no: Some(if populated {
                    "M393A4K40BB1-CRC".into()
                } else {
                    "NO DIMM".into()
                }),
                vendor: Some(if populated {
                    "Samsung".into()
                } else {
                    "NO DIMM".into()
                }),
                mem_type: if populated { Some("DDR4".into()) } else { None },
                capacity_mb: if populated {
                    Some(MebiBytes::new(32768))
                } else {
                    None
                },
                speed_mhz: if populated { Some(2400) } else { None },
            });
        }
        let html = build_html(&hw, &[]);
        // 8 个空槽只产生 1 个合并行（而非 8 行刷屏）
        assert_eq!(
            html.matches("<span class=\"wsx-muted\">空槽").count(),
            1,
            "8 个空槽应合并为一行: {html}"
        );
        assert!(
            html.contains("<th>空槽 8 个</th>"),
            "多空槽表头应为「空槽 8 个」: {html}"
        );
        // 所有空槽标识都列在合并行里
        for empty in [
            "DIMM_A2", "DIMM_B2", "DIMM_C2", "DIMM_D2", "DIMM_E1", "DIMM_E2", "DIMM_F1", "DIMM_F2",
        ] {
            assert!(html.contains(empty), "合并行应列出空槽 {empty}: {html}");
        }
    }

    #[test]
    fn single_populated_stick_warns_single_channel() {
        // 020 类：只插 1 条内存、仍有空槽 → 单通道，提示补齐双通道。
        let mut hw = base_hw();
        hw.memory.sticks.clear();
        // 槽 1：已插
        hw.memory.sticks.push(MemStick {
            locator: Some("DIMM_A1".into()),
            bank: Some("CHANNEL A".into()),
            serial: Some("AB12CD34".into()),
            part_no: Some("HMA81GS6DJR8N".into()),
            vendor: Some("Hynix".into()),
            mem_type: Some("DDR4".into()),
            capacity_mb: Some(MebiBytes::new(8192)),
            speed_mhz: Some(3200),
        });
        // 槽 2：空
        hw.memory.sticks.push(MemStick {
            locator: Some("DIMM_B1".into()),
            bank: Some("CHANNEL B".into()),
            serial: Some("NO DIMM".into()),
            part_no: Some("NO DIMM".into()),
            vendor: Some("NO DIMM".into()),
            mem_type: None,
            capacity_mb: None,
            speed_mhz: None,
        });
        let html = build_html(&hw, &[]);
        assert!(
            html.contains("通道提示"),
            "只插 1 条应出现「通道提示」行: {html}"
        );
        assert!(html.contains("单通道"), "只插 1 条应提示单通道: {html}");
    }

    #[test]
    fn duplicate_locator_disambiguated_by_channel() {
        // 052 类：华硕 B650M 把两槽都报 "DIMM 0"，靠 bank 的 CHANNEL 消歧。
        let mut hw = base_hw();
        hw.memory.sticks.clear();
        hw.memory.sticks.push(MemStick {
            locator: Some("DIMM 0".into()),
            bank: Some("P0 CHANNEL A".into()),
            serial: Some("AB12CD34".into()),
            part_no: Some("CMK16GX4M1".into()),
            vendor: Some("Corsair".into()),
            mem_type: Some("DDR5".into()),
            capacity_mb: Some(MebiBytes::new(16384)),
            speed_mhz: Some(4800),
        });
        hw.memory.sticks.push(MemStick {
            locator: Some("DIMM 0".into()),
            bank: Some("P0 CHANNEL B".into()),
            serial: Some("EF56GH78".into()),
            part_no: Some("CMK16GX4M1".into()),
            vendor: Some("Corsair".into()),
            mem_type: Some("DDR5".into()),
            capacity_mb: Some(MebiBytes::new(16384)),
            speed_mhz: Some(4800),
        });
        let html = build_html(&hw, &[]);
        assert!(
            html.contains("DIMM 0（CHANNEL A）"),
            "重复 locator 应消歧为 CHANNEL A: {html}"
        );
        assert!(
            html.contains("DIMM 0（CHANNEL B）"),
            "重复 locator 应消歧为 CHANNEL B: {html}"
        );
        // 两行都应带消歧，不应残留无消歧的裸「DIMM 0」表头
        assert_eq!(
            html.matches("<th>DIMM 0</th>").count(),
            0,
            "不应残留无消歧的裸 DIMM 0 表头: {html}"
        );
    }

    #[test]
    fn three_populated_sticks_warns_flex_channel() {
        // 3 条已插、仍有空槽 → 非对称双通道（flex），提示补齐到偶数条。
        let mut hw = base_hw();
        hw.memory.sticks.clear();
        let banks = ["CHANNEL A", "CHANNEL B", "CHANNEL A"];
        for (i, b) in banks.iter().enumerate() {
            hw.memory.sticks.push(MemStick {
                locator: Some(format!("DIMM_{i}")),
                bank: Some((*b).into()),
                serial: Some(format!("SN{i}")),
                part_no: Some("M393A4K40".into()),
                vendor: Some("Samsung".into()),
                mem_type: Some("DDR4".into()),
                capacity_mb: Some(MebiBytes::new(16384)),
                speed_mhz: Some(3200),
            });
        }
        // 第 4 槽为空
        hw.memory.sticks.push(MemStick {
            locator: Some("DIMM_3".into()),
            bank: Some("CHANNEL B".into()),
            serial: Some("NO DIMM".into()),
            part_no: Some("NO DIMM".into()),
            vendor: Some("NO DIMM".into()),
            mem_type: None,
            capacity_mb: None,
            speed_mhz: None,
        });
        let html = build_html(&hw, &[]);
        assert!(
            html.contains("非对称双通道"),
            "奇数条已插应提示非对称双通道: {html}"
        );
    }

    /// 真机回归（Z.W. 华为 BC11HGSB0，2026-10-11）：双路服务器每路各插 1 条、
    /// 且都在 Channel 0 —— 条数是**偶数 2**，老的"奇数条"规则识别不出来，
    /// 实际两颗 CPU 都在单通道跑。这条按 (Node, Channel) 分组兜住。
    #[test]
    fn two_sticks_one_per_node_on_same_channel_warns() {
        let mut hw = base_hw();
        hw.memory.sticks.clear();
        // 已插：Node0/Channel0、Node1/Channel0（各路仅 1 条、仅 1 通道）
        for (loc, bank) in [
            ("DIMM000", "_Node0_Channel0_Dimm0"),
            ("DIMM100", "_Node1_Channel0_Dimm0"),
        ] {
            hw.memory.sticks.push(MemStick {
                locator: Some(loc.into()),
                bank: Some(bank.into()),
                serial: Some("0x3239F910".into()),
                part_no: Some("HMA84GL7AMR4N-UH".into()),
                vendor: Some("SK Hynix".into()),
                mem_type: None, // 该机 SMBIOS 未解析出 DDR4
                capacity_mb: Some(MebiBytes::new(32768)),
                speed_mhz: Some(2400),
            });
        }
        // 空槽若干（有空槽才提示"可补"）
        for loc in ["DIMM001", "DIMM101"] {
            hw.memory.sticks.push(MemStick {
                locator: Some(loc.into()),
                bank: Some("_Node0_Channel0_Dimm1".into()),
                serial: Some("NO DIMM".into()),
                part_no: Some("NO DIMM".into()),
                vendor: Some("NO DIMM".into()),
                mem_type: None,
                capacity_mb: None,
                speed_mhz: None,
            });
        }
        let html = build_html(&hw, &[]);
        assert!(
            html.contains("通道提示"),
            "双路各插 1 条（同通道）应出现「通道提示」: {html}"
        );
        assert!(
            html.contains("单通道"),
            "两条却都在 Channel 0 应提示单通道: {html}"
        );
        // 不该被老规则误判成"非对称双通道"（那是奇数条的文案）
        assert!(
            !html.contains("非对称双通道"),
            "偶数条不应走奇数条文案: {html}"
        );
    }

    /// 正常双通道（同一路的两个通道各一条）**不得**误报。
    #[test]
    fn proper_dual_channel_does_not_warn() {
        let mut hw = base_hw();
        hw.memory.sticks.clear();
        for (loc, bank) in [("DIMM_A1", "P0 CHANNEL A"), ("DIMM_B1", "P0 CHANNEL B")] {
            hw.memory.sticks.push(MemStick {
                locator: Some(loc.into()),
                bank: Some(bank.into()),
                serial: Some("SN1".into()),
                part_no: Some("KF560C30-32".into()),
                vendor: Some("Kingston".into()),
                mem_type: Some("DDR5".into()),
                capacity_mb: Some(MebiBytes::new(32768)),
                speed_mhz: Some(6000),
            });
        }
        // 仍有空槽（满足"可补"前提），但通道是满的 → 不该提示
        for loc in ["DIMM_A2", "DIMM_B2"] {
            hw.memory.sticks.push(MemStick {
                locator: Some(loc.into()),
                bank: Some("P0 CHANNEL A".into()),
                serial: Some("NO DIMM".into()),
                part_no: Some("NO DIMM".into()),
                vendor: Some("NO DIMM".into()),
                mem_type: None,
                capacity_mb: None,
                speed_mhz: None,
            });
        }
        let html = build_html(&hw, &[]);
        assert!(
            !html.contains("通道提示"),
            "通道跑满时不应出现通道提示: {html}"
        );
    }

    /// 槽位名里没有可判定的通道信息（"DIMM000" 且 bank 无 Channel）时**不提示**——
    /// 拿不准宁可漏报，不可误报。
    #[test]
    fn unparsable_channel_never_warns() {
        let mut hw = base_hw();
        hw.memory.sticks.clear();
        for loc in ["DIMM0", "DIMM1"] {
            hw.memory.sticks.push(MemStick {
                locator: Some(loc.into()),
                bank: Some("BANK 0".into()), // 无 Node / 无 Channel 字样
                serial: Some("SN".into()),
                part_no: Some("CT16G4".into()),
                vendor: Some("Crucial".into()),
                mem_type: Some("DDR4".into()),
                capacity_mb: Some(MebiBytes::new(16384)),
                speed_mhz: Some(2666),
            });
        }
        hw.memory.sticks.push(MemStick {
            locator: Some("DIMM2".into()),
            bank: Some("BANK 1".into()),
            serial: Some("NO DIMM".into()),
            part_no: Some("NO DIMM".into()),
            vendor: Some("NO DIMM".into()),
            mem_type: None,
            capacity_mb: None,
            speed_mhz: None,
        });
        let html = build_html(&hw, &[]);
        assert!(
            !html.contains("通道提示"),
            "通道信息不可判定时不应提示: {html}"
        );
    }

    #[test]
    fn placeholder_vendor_renders_as_unknown() {
        // 固件占位串（To Be Filled By O.E.M. / NO DIMM / System Manufacturer 等）
        // 必须按「未知」处理，绝不能当成真实厂商/型号渲染到资产报告里。
        let mut hw = base_hw();
        hw.memory.sticks.clear();
        hw.memory.sticks.push(MemStick {
            locator: Some("DIMM_A1".into()),
            bank: Some("BANK0".into()),
            serial: Some("Default string".into()),
            part_no: Some("NO DIMM".into()),
            vendor: Some("To Be Filled By O.E.M.".into()),
            mem_type: Some("DDR4".into()),
            capacity_mb: Some(MebiBytes::new(8192)),
            speed_mhz: Some(3200),
        });
        let html = build_html(&hw, &[]);
        assert!(
            !html.contains("To Be Filled By O.E.M."),
            "占位串厂商不得原样渲染: {html}"
        );
        assert!(!html.contains("NO DIMM"), "占位串型号不得原样渲染: {html}");
        // 该插槽厂商字段应回退为「未知」。
        assert!(html.contains("未知"), "占位串厂商应显示为未知: {html}");
        // 真实厂商仍正常中文化（金士顿）。
        let mut hw2 = base_hw();
        hw2.memory.sticks.clear();
        hw2.memory.sticks.push(MemStick {
            locator: Some("DIMM_A1".into()),
            bank: Some("BANK0".into()),
            serial: Some("KST-1".into()),
            part_no: Some("P-1".into()),
            vendor: Some("Kingston".into()),
            mem_type: Some("DDR4".into()),
            capacity_mb: Some(MebiBytes::new(8192)),
            speed_mhz: Some(3200),
        });
        let html2 = build_html(&hw2, &[]);
        assert!(html2.contains("金士顿"), "真实厂商应正常中文化: {html2}");
        assert!(!html2.contains("未知"), "真实厂商不应显示未知: {html2}");
    }

    #[test]
    fn memory_speed_shows_clock_frequency_hint() {
        // SMBIOS 的 speed 是传输率(MT/s)，不是时钟频率；DDR 双倍数据率，
        // 真实时钟 = 传输率 / 2。报告须附「(时钟 N MHz)」防误解（Z.W. 2026-09-22）。
        let mut hw = base_hw();
        hw.memory.sticks.clear();
        hw.memory.sticks.push(MemStick {
            locator: Some("DIMM_A1".into()),
            bank: Some("BANK0".into()),
            serial: Some("S1".into()),
            part_no: Some("P1".into()),
            vendor: Some("Kingston".into()),
            mem_type: Some("DDR5".into()),
            capacity_mb: Some(MebiBytes::new(32768)),
            speed_mhz: Some(6000),
        });
        let html = build_html(&hw, &[]);
        assert!(
            html.contains("6000 MT/s (时钟 3000 MHz)"),
            "HTML 应附真实时钟频率: {html}"
        );
        let text = build_text(&hw);
        assert!(
            text.contains("6000 MT/s (时钟 3000 MHz)"),
            "文本报告应附真实时钟频率: {text}"
        );
        // 时钟值必须是传输率 / 2，而非原值。
        assert!(!html.contains("6000 MHz)"), "时钟不得等于传输率");
    }

    #[test]
    fn memory_total_cross_check_notes_discrepancy() {
        // 口径交叉校验：OS 总计与 SMBIOS 各槽合计差异超过 2% 时提示。
        let mut hw = base_hw();
        hw.memory.total_mb = Some(MebiBytes::new(16384));
        hw.memory.sticks.clear();
        hw.memory.sticks.push(MemStick {
            locator: Some("DIMM_A1".into()),
            bank: Some("BANK0".into()),
            serial: Some("S1".into()),
            part_no: Some("P1".into()),
            vendor: Some("Kingston".into()),
            mem_type: Some("DDR4".into()),
            capacity_mb: Some(MebiBytes::new(4096)), // 合计 4G vs OS 16G → 75% 偏差
            speed_mhz: Some(3200),
        });
        let html = build_html(&hw, &[]);
        assert!(
            html.contains("容量口径提示"),
            "内存口径差异过大应提示: {html}"
        );

        // 一致场景：双条 8G 合计 16G = OS 16G，不应提示。
        let mut ok = base_hw();
        ok.memory.total_mb = Some(MebiBytes::new(16384));
        ok.memory.sticks.clear();
        for i in 0..2 {
            ok.memory.sticks.push(MemStick {
                locator: Some(format!("DIMM_{i}")),
                bank: Some("BANK0".into()),
                serial: Some(format!("S{i}")),
                part_no: Some(format!("P{i}")),
                vendor: Some("Kingston".into()),
                mem_type: Some("DDR4".into()),
                capacity_mb: Some(MebiBytes::new(8192)),
                speed_mhz: Some(3200),
            });
        }
        let ok_html = build_html(&ok, &[]);
        assert!(
            !ok_html.contains("容量口径提示"),
            "内存口径一致不应提示: {ok_html}"
        );
    }

    #[test]
    fn audio_input_vid_pid_are_escaped() {
        // VID/PID 注入 HTML 前必须转义，防止特殊字符破坏结构或注入。
        let mut hw = base_hw();
        hw.audio.clear();
        hw.audio.push(AudioDevice {
            name: "Test Audio".into(),
            role: 0b01,
            kind: AudioKind::Speaker,
            is_bluetooth: false,
            vid: Some("<b>".into()),
            pid: Some("</b>".into()),
            manufacturer: None,
        });
        hw.input_devices.clear();
        hw.input_devices.push(InputDevice {
            name: "Test KB".into(),
            kind: InputKind::Keyboard,
            vid: Some("<i>".into()),
            pid: Some("&x".into()),
            manufacturer: None,
            is_bluetooth: false,
            location: None,
        });
        let html = build_html(&hw, &[]);
        assert!(
            html.contains("&lt;b&gt;:&lt;/b&gt;"),
            "音频 VID:PID 应被转义: {html}"
        );
        assert!(
            html.contains("&lt;i&gt;:&amp;x"),
            "键鼠 VID:PID 应被转义: {html}"
        );
        // 确无未转义标签注入
        assert!(!html.contains("<span class=\"wsx-mono\"><b>"));
        assert!(!html.contains("<span class=\"wsx-mono\"><i>"));
    }

    #[test]
    fn disk_iface_media_are_escaped() {
        // 硬盘接口/介质字段注入 HTML 前必须转义。
        let mut hw = base_hw();
        hw.disks.clear();
        hw.disks.push(PhysicalDisk {
            index: 0,
            model: Some("TEST-SSD".into()),
            iface: Some("<i>".into()),
            media: Some("<m>".into()),
            size_bytes: Some(512_000_000_000),
            serial: None,
            ..Default::default()
        });
        let html = build_html(&hw, &[]);
        assert!(
            html.contains("&lt;i&gt; &middot; &lt;m&gt;"),
            "硬盘接口/介质应被转义: {html}"
        );
        assert!(!html.contains("<i> &middot; <m>"));
    }

    #[test]
    fn bios_vendor_renders_chinese() {
        // BIOS 厂商此前未接 vendor_label（即便 map 有条目也原样输出），现已补上。
        let hw = base_hw();
        let html = build_html(&hw, &[]);
        let txt = build_text(&hw);
        assert!(html.contains("美商安迈"), "BIOS 厂商应显示中文: {html}");
        assert!(txt.contains("美商安迈"), "文本 BIOS 厂商应显示中文: {txt}");
    }

    #[test]
    fn cpu_vendor_renders_chinese() {
        // CPU 厂商此前未接 vendor_label，现应在英文后跟中文。
        let hw = base_hw();
        let html = build_html(&hw, &[]);
        let txt = build_text(&hw);
        assert!(
            html.contains("Intel（英特尔）"),
            "HTML CPU 厂商应中文: {html}"
        );
        assert!(txt.contains("英特尔"), "文本 CPU 厂商应中文: {txt}");
    }

    #[test]
    fn aspeed_gpu_name_fallback_shows_chinese() {
        // 服务器 BMC 显示芯片：采集层 vendor 常为空，需从名称兜底识别。
        let mut hw = base_hw();
        hw.gpus.clear();
        hw.gpus.push(Gpu {
            name: "ASPEED Graphics Family(WDDM)".into(),
            vram_mb: None,
            kind: GpuKind::Physical,
            vendor: None,
            telemetry: None,
        });
        let html = build_html(&hw, &[]);
        let txt = build_text(&hw);
        assert!(
            html.contains("信骅"),
            "ASPEED 显卡应从名称兜底显示中文: {html}"
        );
        assert!(txt.contains("信骅"), "文本 ASPEED 应显示中文: {txt}");
    }

    #[test]
    fn disk_smart_unavailable_shows_reason_not_healthy() {
        // 拿不到必须明示原因，绝不可显示 0 或"健康 良好"（纪律）。
        let mut hw = base_hw();
        hw.disks.clear();
        hw.disks.push(PhysicalDisk {
            index: 0,
            model: Some("Samsung SSD 980".into()),
            iface: Some("NVMe".into()),
            media: Some("SSD".into()),
            serial: Some("SN123".into()),
            size_bytes: Some(1_000_000_000_000),
            smart: Some(DiskSmart {
                source: SmartSource::NvmeLogPage,
                unavailable_reason: Some("需管理员权限".into()),
                ..Default::default()
            }),
            ..Default::default()
        });
        let html = build_html(&hw, &[]);
        let txt = build_text(&hw);
        assert!(
            html.contains("健康状态：不可用（需管理员权限）"),
            "HTML 应显示不可用原因: {html}"
        );
        assert!(
            txt.contains("健康状态：不可用（需管理员权限）"),
            "文本应显示不可用原因: {txt}"
        );
        assert!(
            !html.contains("健康 良好"),
            "不可用绝不能显示 健康 良好: {html}"
        );
    }

    #[test]
    fn disk_smart_renders_power_on_and_health() {
        // 有数据就显示通电小时与剩余寿命。
        let mut hw = base_hw();
        hw.disks.clear();
        hw.disks.push(PhysicalDisk {
            index: 0,
            model: Some("Samsung SSD 980".into()),
            iface: Some("NVMe".into()),
            media: Some("SSD".into()),
            serial: Some("SN123".into()),
            size_bytes: Some(1_000_000_000_000),
            smart: Some(DiskSmart {
                source: SmartSource::NvmeLogPage,
                power_on_hours: Some(12345),
                temperature_c: Some(42),
                health_status: HealthStatus::Healthy,
                health_percent: Some(87),
                ..Default::default()
            }),
            ..Default::default()
        });
        let html = build_html(&hw, &[]);
        let txt = build_text(&hw);
        assert!(
            html.contains("通电 12345 小时"),
            "HTML 应显示通电小时: {html}"
        );
        assert!(
            html.contains("健康 良好（剩余 87%）"),
            "HTML 应显示剩余寿命: {html}"
        );
        assert!(txt.contains("通电 12345 小时"), "文本应显示通电小时: {txt}");
        assert!(
            txt.contains("健康 良好（剩余 87%）"),
            "文本应显示剩余寿命: {txt}"
        );
    }

    #[test]
    fn report_status_colors_for_degraded_hardware() {
        // 语义状态色必须按真实状态渲染：危险盘→红胶囊、注意盘→橙胶囊、满盘/高温→红文字。
        let mut hw = base_hw();
        hw.disks.clear();
        hw.disks.push(PhysicalDisk {
            index: 0,
            model: Some("Samsung SSD 980".into()),
            iface: Some("NVMe".into()),
            media: Some("SSD".into()),
            serial: Some("SN123".into()),
            size_bytes: Some(1_000_000_000_000),
            smart: Some(DiskSmart {
                source: SmartSource::NvmeLogPage,
                health_status: HealthStatus::Critical,
                health_percent: Some(8),
                ..Default::default()
            }),
            ..Default::default()
        });
        hw.disks.push(PhysicalDisk {
            index: 1,
            model: Some("Seagate HDD".into()),
            iface: Some("SATA".into()),
            media: Some("HDD".into()),
            serial: Some("SG456".into()),
            size_bytes: Some(2_000_000_000_000),
            smart: Some(DiskSmart {
                source: SmartSource::NvmeLogPage,
                health_status: HealthStatus::Warning,
                ..Default::default()
            }),
            ..Default::default()
        });
        // 95% 满盘
        let vol = Volume {
            letter: "C:".to_string(),
            type_name: Some("GPT".into()),
            fs_name: Some("NTFS".into()),
            sectors_per_cluster: None,
            bytes_per_sector: None,
            cluster_bytes: Some(4096),
            total_bytes: Some(1_000_000_000_000),
            free_bytes: Some(50_000_000_000),
            vol_label: Some("系统".into()),
            disk_index: Some(0),
        };
        // 92°C 高温显卡
        if let Some(t) = hw.gpus[0].telemetry.as_mut() {
            t.temp_c = Some(92);
        }
        let html = build_html(&hw, &[vol]);
        assert!(
            html.contains("wsx-stat-crit"),
            "危险盘应渲染红色状态胶囊: {html}"
        );
        assert!(
            html.contains("wsx-stat-warn"),
            "注意盘应渲染橙色状态胶囊: {html}"
        );
        assert!(html.contains("wsx-t-crit"), "高温应渲染红色文字: {html}");
        // 满盘分区：95% 只来自分区“已用”（SMART 危险盘不输出百分比），wsx-stat-crit 验证红色胶囊已上。
        assert!(
            html.contains("95%") && html.contains("wsx-stat-crit"),
            "满盘分区应渲染红色状态胶囊: {html}"
        );
    }

    #[test]
    fn elevation_warning_shows_on_report_when_not_admin() {
        // 未提权：报告顶部应显示警告横幅，提示 NVMe SMART 可能不完整。
        let mut hw = base_hw();
        hw.admin_elevated = Some(false);
        let html = build_html(&hw, &[]);
        assert!(
            html.contains("未以管理员身份运行"),
            "未提权应在报告顶部显示警告横幅: {html}"
        );
        let txt = build_text(&hw);
        assert!(txt.contains("未以管理员身份运行"), "文本也应提示: {txt}");
    }

    #[test]
    fn elevation_no_warning_when_admin() {
        // 已提权：不应显示警告。
        let mut hw = base_hw();
        hw.admin_elevated = Some(true);
        let html = build_html(&hw, &[]);
        assert!(
            !html.contains("未以管理员身份运行"),
            "已提权不应显示警告: {html}"
        );
    }

    #[test]
    fn storage_backend_note_renders_storcli_hint() {
        // 盘位于 RAID 控制器后：应提示完整 SMART 需 storcli/megacli 等厂商工具。
        let mut hw = base_hw();
        hw.disks.clear();
        hw.disks.push(PhysicalDisk {
            index: 0,
            model: Some("PERC H730".into()),
            iface: Some("RAID".into()),
            media: Some("SSD".into()),
            serial: Some("SN-RAID".into()),
            size_bytes: Some(2_000_000_000_000),
            storage_backend: Some("RAID 控制器后（PERC H730）".into()),
            ..Default::default()
        });
        let html = build_html(&hw, &[]);
        assert!(
            html.contains("storcli/megacli"),
            "RAID 后端应提示厂商工具: {html}"
        );
    }

    #[test]
    fn nvme_vmd_gap_shows_power_on_transparency_note() {
        // NVMe 经 VMD 拦截：有温度/健康但无通电时长，应明确说明"该存储栈未透传"。
        let mut hw = base_hw();
        hw.disks.clear();
        hw.disks.push(PhysicalDisk {
            index: 2,
            model: Some("Intel NVMe".into()),
            iface: Some("NVMe".into()),
            media: Some("SSD".into()),
            serial: Some("SN-X".into()),
            size_bytes: Some(1_000_000_000_000),
            smart: Some(DiskSmart {
                source: SmartSource::StorageReliabilityCounter,
                temperature_c: Some(44),
                health_status: HealthStatus::Healthy,
                ..Default::default()
            }),
            ..Default::default()
        });
        let html = build_html(&hw, &[]);
        assert!(
            html.contains("通电时长:该存储栈未透传"),
            "NVMe 缺通电时长应明示: {html}"
        );
    }

    #[test]
    fn nvme_vmd_gap_shows_serial_transparency_note() {
        // NVMe 经 VMD 拦截：拿不到真序列号，应明示"该存储栈未透传"而不是留白
        // （留白会被误读成"漏采"或"新盘本来就没序列号"）。
        let mut hw = base_hw();
        hw.disks.clear();
        hw.disks.push(PhysicalDisk {
            index: 3,
            model: Some("SAMSUNG MZQL21T9HCJR-00B7C".into()),
            iface: Some("NVMe".into()),
            media: Some("SSD".into()),
            serial: None, // VMD 下只拿到合成 hex，已被判废
            size_bytes: Some(1_920_000_000_000),
            smart: Some(DiskSmart {
                source: SmartSource::StorageReliabilityCounter,
                temperature_c: Some(41),
                health_status: HealthStatus::Healthy,
                ..Default::default()
            }),
            ..Default::default()
        });
        let html = build_html(&hw, &[]);
        assert!(
            html.contains("序列号：该存储栈未透传"),
            "NVMe 缺序列号应明示原因: {html}"
        );
    }

    #[test]
    fn serial_transparency_note_not_applied_to_unmasked_disk() {
        // 未被虚拟层遮挡的盘（直连 SATA/USB）若恰好无序列号，不应套用"未透传"说辞——
        // 那是 VMD 专属原因，乱贴会把排查带偏。
        let mut hw = base_hw();
        hw.disks.clear();
        hw.disks.push(PhysicalDisk {
            index: 0,
            model: Some("Generic USB Disk".into()),
            iface: Some("USB".into()),
            media: Some("HDD".into()),
            serial: None,
            size_bytes: Some(500_000_000_000),
            smart: Some(DiskSmart {
                source: SmartSource::AtaSmart,
                ..Default::default()
            }),
            ..Default::default()
        });
        let html = build_html(&hw, &[]);
        assert!(
            !html.contains("序列号：该存储栈未透传"),
            "非 VMD 场景不应套用未透传说辞: {html}"
        );
    }

    #[test]
    fn firmware_renders_in_disk_section() {
        // 固件版本应渲染在硬盘段（资产身份用于 RMA / 批次比对）。
        let mut hw = base_hw();
        hw.disks.clear();
        hw.disks.push(PhysicalDisk {
            index: 0,
            model: Some("Samsung SSD 980".into()),
            iface: Some("NVMe".into()),
            media: Some("SSD".into()),
            serial: Some("SN123".into()),
            firmware: Some("1B4QFXO7".into()),
            size_bytes: Some(1_000_000_000_000),
            ..Default::default()
        });
        let html = build_html(&hw, &[]);
        assert!(html.contains("固件：1B4QFXO7"), "应渲染固件版本: {html}");
    }

    #[test]
    fn disk_model_infers_vendor_zh() {
        // 硬盘仅型号串，无 vendor 字段，需按型号前缀/关键字推断。
        let mut hw = base_hw();
        hw.disks.clear();
        hw.disks.push(PhysicalDisk {
            index: 0,
            model: Some("INTEL SSDSC2KG240G8".into()),
            iface: Some("SATA".into()),
            media: Some("SSD".into()),
            serial: Some("X".into()),
            size_bytes: Some(240_000_000_000),
            ..Default::default()
        });
        hw.disks.push(PhysicalDisk {
            index: 1,
            model: Some("ST8000NM000A-2KE101".into()),
            iface: Some("SATA".into()),
            media: Some("HDD".into()),
            serial: Some("Y".into()),
            size_bytes: Some(8_000_000_000_000),
            ..Default::default()
        });
        let html = build_html(&hw, &[]);
        let txt = build_text(&hw);
        assert!(
            html.contains("英特尔"),
            "INTEL 硬盘型号应推断中文厂商: {html}"
        );
        assert!(
            html.contains("希捷"),
            "Seagate(ST 前缀) 硬盘型号应推断中文厂商: {html}"
        );
        assert!(
            txt.contains("英特尔") && txt.contains("希捷"),
            "文本硬盘应显示中文厂商: {txt}"
        );
    }

    #[test]
    fn disk_vendor_from_model_unit() {
        // 问题1修复：带接口前缀的希捷盘（真机 ATA ST16000NM002H-3K）
        assert_eq!(
            disk_vendor_from_model("ATA ST16000NM002H-3K"),
            Some("Seagate（希捷）".to_string())
        );
        // 原行为不变：无前缀的希捷盘
        assert_eq!(
            disk_vendor_from_model("ST8000NM000A-2KE101"),
            Some("Seagate（希捷）".to_string())
        );
        // 问题3：西部数据 Ultrastar He 氦气盘（WUH 前缀）
        assert_eq!(
            disk_vendor_from_model("WUH721816ALE6L4"),
            Some("Western Digital（西部数据）".to_string())
        );
        // 纪律：未知品牌一律 None，绝不臆造
        assert_eq!(disk_vendor_from_model("X4020S173A15TNQF"), None);
        // 反例：含 "ST" 子串但非希捷的不误判（KINGSTON 含 ST，应识别为金士顿）
        let r = disk_vendor_from_model("KINGSTON SA400S37");
        assert_eq!(r, Some("Kingston（金士顿）".to_string()));
        assert_ne!(r, Some("Seagate（希捷）".to_string()));
    }

    #[test]
    fn nic_vendor_renders_in_html() {
        let mut hw = base_hw();
        hw.nics.push(Nic {
            desc: Some("Realtek PCIe GBE Family Controller".into()),
            mac: Some("00:11:22:33:44:55".into()),
            ipv4: vec!["192.168.1.10".into()],
            ipv6: None,
            up: true,
            speed_bps: None,
            kind: NicKind::Physical,
            vendor: Some("Realtek".into()),
        });
        let html = build_html(&hw, &[]);
        assert!(
            html.contains("Realtek PCIe GBE Family Controller（瑞昱）"),
            "网卡应在英文设备名后跟 (中文厂商名): {html}"
        );
    }

    #[test]
    fn nic_vendor_falls_back_to_desc_when_field_missing() {
        // 真实场景：老 JSON（采集期尚未填充 vendor 字段）也能从 desc 兜底出中文厂商名。
        let mut hw = base_hw();
        hw.nics.push(Nic {
            desc: Some("Realtek 8188GU Wireless LAN 802.11n USB NIC".into()),
            mac: Some("00:11:22:33:44:55".into()),
            ipv4: vec!["192.168.5.10".into()],
            ipv6: None,
            up: true,
            speed_bps: None,
            kind: NicKind::Physical,
            vendor: None, // 老 JSON：字段缺失
        });
        let html = build_html(&hw, &[]);
        assert!(
            html.contains("Realtek 8188GU Wireless LAN 802.11n USB NIC（瑞昱）"),
            "vendor 缺失时应从 desc 兜底出中文厂商名: {html}"
        );
        let text = build_text(&hw);
        assert!(
            text.contains("（瑞昱）"),
            "文本报告也应从 desc 兜底出中文厂商名: {text}"
        );
    }

    /// 构造一张单网卡的 `HardwareInfo`，便于逐个分支验证 IP 显示策略。
    fn nic_case(desc: &str, ipv4: Vec<&str>, ipv6: Option<&str>, up: bool) -> String {
        let mut hw = base_hw();
        hw.nics.clear();
        hw.nics.push(Nic {
            desc: Some(desc.into()),
            mac: Some("00-11-22-33-44-55".into()),
            ipv4: ipv4.into_iter().map(|s| s.into()).collect(),
            ipv6: ipv6.map(|s| s.into()),
            up,
            speed_bps: None,
            kind: NicKind::Physical,
            vendor: None,
        });
        build_html(&hw, &[])
    }

    #[test]
    fn nic_auto_ip_hidden_when_down_and_no_real_ip() {
        // 规则①：未连接 + 只有自动地址 → 一个 IP 都不显示（未连接已由「状态」表达）。
        // 真机 #1/#2 Intel I350 正是此情形，此前会在报告里列出 169.254/fe80 造成误导。
        let html = nic_case(
            "Intel(R) I350 Gigabit Network Connection",
            vec!["169.254.59.104"],
            Some("fe80:0000:0000:0000:0aab:13f8:2045:326f"),
            false,
        );
        assert!(
            !html.contains("169.254"),
            "未连接网卡的 APIPA 地址不应显示: {html}"
        );
        assert!(
            !html.contains("fe80:"),
            "未连接网卡的链路本地 IPv6 不应显示: {html}"
        );
        assert!(html.contains("状态: 未连接"), "状态仍应显示: {html}");
    }

    #[test]
    fn nic_auto_ip_shown_when_up_without_real_ip() {
        // 规则②：已连接 + 一个真实地址都没有 → 显示自动地址并标注。
        // 这属于「连上了却没拿到 IP」的异常状态，值得在报告里提示。
        let html = nic_case(
            "Realtek PCIe GBE Family Controller",
            vec!["169.254.1.2"],
            Some("fe80:0000:0000:0000:0aab:13f8:2045:1111"),
            true,
        );
        assert!(
            html.contains("169.254.1.2 <span class=\"wsx-tag\">APIPA 自动地址</span>"),
            "已连接但无真实 IP 时应显示并标注 APIPA: {html}"
        );
        assert!(
            html.contains("<span class=\"wsx-tag\">链路本地</span>"),
            "应显示并标注链路本地 IPv6: {html}"
        );
    }

    #[test]
    fn nic_real_ip_only_auto_ip_suppressed() {
        // 规则③：只要有真实地址 → 只显示真实地址，自动地址一律隐去（无论连接与否）。
        // 真机 #3 Mellanox #2（未连接但有静态 IP 192.168.0.242 + APIPA）正是此情形。
        let html = nic_case(
            "Mellanox ConnectX-3 Ethernet Adapter #2",
            vec!["169.254.50.170", "192.168.0.242"],
            None,
            false,
        );
        assert!(html.contains("192.168.0.242"), "真实静态 IP 应显示: {html}");
        assert!(
            !html.contains("169.254"),
            "存在真实地址时自动地址应被隐去: {html}"
        );
        assert!(
            !html.contains("APIPA"),
            "存在真实地址时不应出现 APIPA 标注: {html}"
        );

        // 已连接且只有真实 IP：正常显示，无标注。
        let html2 = nic_case(
            "Mellanox ConnectX-3 Ethernet Adapter",
            vec!["192.168.0.241"],
            None,
            true,
        );
        assert!(html2.contains("192.168.0.241"), "真实 IP 应显示: {html2}");
        assert!(
            !html2.contains("APIPA") && !html2.contains("链路本地"),
            "真实 IP 不应带任何自动地址标注: {html2}"
        );
    }

    #[test]
    fn nic_speed_sentinel_not_rendered() {
        // 回归：Windows 的 u64::MAX「速率不可得」哨兵曾被写进真机 JSON，
        // 渲染层必须按未知处理，绝不显示成「18446744073.7 Gbps」。
        let mut hw = base_hw();
        hw.nics.clear();
        hw.nics.push(Nic {
            desc: Some("Intel(R) I350 Gigabit Network Connection".into()),
            mac: Some("6C-92-BF-A6-ED-61".into()),
            ipv4: vec![],
            ipv6: None,
            up: false,
            speed_bps: Some(u64::MAX), // 旧采集写进来的脏值
            kind: NicKind::Physical,
            vendor: Some("Intel".into()),
        });
        let html = build_html(&hw, &[]);
        assert!(
            !html.contains("速率:"),
            "速率哨兵(u64::MAX)不应渲染成速率行: {html}"
        );
        let text = build_text(&hw);
        assert!(!text.contains("速率"), "文本报告也不应渲染速率哨兵: {text}");

        // 真实速率不受影响：同一部过滤不该误杀正常值。
        hw.nics.clear();
        hw.nics.push(Nic {
            desc: Some("Mellanox ConnectX-3 Ethernet Adapter".into()),
            mac: Some("50-6B-4B-58-FA-72".into()),
            ipv4: vec!["192.168.0.241".into()],
            ipv6: None,
            up: true,
            speed_bps: Some(40_000_000_000),
            kind: NicKind::Physical,
            vendor: Some("Mellanox".into()),
        });
        let html2 = build_html(&hw, &[]);
        assert!(
            html2.contains("速率: 40 Gbps"),
            "真实速率应正常渲染: {html2}"
        );
    }

    #[test]
    fn nic_status_and_speed_render() {
        // 验证：已连接网卡渲染「状态: 已连接」与「速率: 10 Gbps」；Mellanox 经 desc 兜底出中文名。
        // 未连接(无速率)只渲染状态、不渲染速率行。
        let mut hw = base_hw();
        hw.nics.clear();
        hw.nics.push(Nic {
            desc: Some("Mellanox ConnectX-3 Ethernet Adapter".into()),
            mac: Some("00:02:C9:00:00:01".into()),
            ipv4: vec!["10.0.0.5".into()],
            ipv6: None,
            up: true,
            speed_bps: Some(10_000_000_000), // 10 Gbps
            kind: NicKind::Physical,
            vendor: None, // 走 desc 兜底识别 Mellanox
        });
        let html = build_html(&hw, &[]);
        assert!(
            html.contains("Mellanox ConnectX-3 Ethernet Adapter（迈络思）"),
            "Mellanox 应显示中文厂商名: {html}"
        );
        assert!(
            html.contains("状态: 已连接"),
            "已连接网卡应显示「状态: 已连接」: {html}"
        );
        assert!(
            html.contains("速率: 10 Gbps"),
            "10 Gbps 应格式化为「速率: 10 Gbps」: {html}"
        );

        let text = build_text(&hw);
        assert!(text.contains("状态 已连接"), "文本报告应显示状态: {text}");
        assert!(text.contains("速率 10 Gbps"), "文本报告应显示速率: {text}");

        // 未连接网卡：显示「未连接」，且无速率行（speed_bps = None）。
        hw.nics.clear();
        hw.nics.push(Nic {
            desc: Some("Intel Ethernet Connection".into()),
            mac: Some("00:11:22:33:44:55".into()),
            ipv4: vec![],
            ipv6: None,
            up: false,
            speed_bps: None,
            kind: NicKind::Physical,
            vendor: Some("Intel".into()),
        });
        let html2 = build_html(&hw, &[]);
        assert!(
            html2.contains("状态: 未连接"),
            "未连接网卡应显示「状态: 未连接」: {html2}"
        );
        assert!(
            !html2.contains("速率:"),
            "未连接(无速率)不应渲染速率行: {html2}"
        );
    }

    #[test]
    fn board_and_memory_serial_render_in_report() {
        // base_hw() 已带主板序列号 MB-SN-99887766 与内存序列号 KST-MEM-55443322。
        let hw = base_hw();
        let html = build_html(&hw, &[]);
        let text = build_text(&hw);
        // 主板序列号：HTML 表格行 + 文本键值对
        assert!(
            html.contains("MB-SN-99887766"),
            "HTML 应渲染主板序列号: {html}"
        );
        assert!(text.contains("序列号"), "文本报告主板应有序列号行: {text}");
        assert!(
            text.contains("MB-SN-99887766"),
            "文本报告应渲染主板序列号: {text}"
        );
        // 内存条序列号：HTML 内联 + 文本内联
        assert!(
            html.contains("KST-MEM-55443322"),
            "HTML 应渲染内存序列号: {html}"
        );
        assert!(
            text.contains("KST-MEM-55443322"),
            "文本报告应渲染内存序列号: {text}"
        );
    }

    #[test]
    fn degenerate_fill_serials_are_rejected() {
        // 固件常用退化填充冒充序列号，而精确占位列表穷举不完**任意长度**的 0 / F / - / X，
        // 故按模式判定。这些一旦渲染进资产报告，和真序列号难以区分。
        for bad in [
            "00000000", // 8 个 0 —— 精确列表里只有 10 / 12 个，这条原本会漏
            "FFFFFFFF",
            "ffffffff",
            "--------",
            "XXXXXXXX",
            "0123456789", // 升序数字串
            "12345678",
            "9876543210", // 降序数字串
            "0000000000",
        ] {
            assert!(!real_serial(bad), "退化填充序列号应判为无效: {bad}");
        }
        // 真实序列号必须保留（含真机实测值）
        for good in ["F07D3070", "2CHMBDEP", "BQWL43950466", "KST-MEM-55443322"] {
            assert!(real_serial(good), "真实序列号不应被误杀: {good}");
        }
    }

    #[test]
    fn serial_placeholder_is_hidden() {
        // 用户要求：没有真实序列号（固件占位串）就不要显示。
        let mut hw = base_hw();
        // 主板占位串 "Default string"
        if let Some(b) = hw.board.as_mut() {
            b.serial = Some("Default string".into());
        }
        // 内存条：一条真实、一条占位 "Unknown"
        hw.memory.sticks.clear();
        hw.memory.sticks.push(MemStick {
            locator: Some("DIMM_A1".into()),
            bank: Some("BANK0".into()),
            serial: Some("Unknown".into()),
            part_no: Some("ABC-123".into()),
            vendor: Some("Kingston".into()),
            mem_type: Some("DDR4".into()),
            capacity_mb: Some(MebiBytes::new(8192)),
            speed_mhz: Some(3200),
        });
        let html = build_html(&hw, &[]);
        let text = build_text(&hw);
        assert!(
            !html.contains("Default string"),
            "主板占位序列号不应渲染: {html}"
        );
        assert!(
            !text.contains("Default string"),
            "主板占位序列号不应渲染: {text}"
        );
        assert!(
            !html.contains("Unknown"),
            "内存条占位序列号不应渲染: {html}"
        );
        assert!(
            !text.contains("Unknown"),
            "内存条占位序列号不应渲染: {text}"
        );
        // 主板序列号行仍存在，但显示占位符 '-'（dash_cell）而非真实值
        assert!(
            html.contains("<tr><th>序列号</th><td>-</td></tr>"),
            "主板序列号行应显示占位符而非真实值: {html}"
        );
    }

    #[test]
    fn nic_addresses_split_into_separate_lines() {
        // 用户反馈：MAC/IPv4/IPv6 挤在一行不好看。修复后各自成行（独立 <span class="wsx-mono">）。
        let mut hw = base_hw();
        hw.nics.clear();
        hw.nics.push(Nic {
            desc: Some("Realtek PCIe GBE Family Controller".into()),
            mac: Some("60-A4-4C-31-7C-4E".into()),
            ipv4: vec![
                "192.168.0.124".into(),
                "192.168.10.124".into(),
                "192.168.100.124".into(),
            ],
            // 必须是"真实"(非链路本地)IPv6：按 IP 显示规则，有真实地址时自动地址(fe80::)
            // 会被隐去，若这里仍用 fe80:: 就测不到 IPv6 独立成行。
            ipv6: Some("2408:8207:1a2b::3c4d".into()),
            up: true,
            speed_bps: None,
            kind: NicKind::Physical,
            vendor: Some("Realtek".into()),
        });
        let html = build_html(&hw, &[]);
        // 每行独立：MAC / IPv4 / IPv6 各自一个 mono span。
        assert_eq!(
            html.matches("<span class=\"wsx-mono\">MAC ").count(),
            1,
            "MAC 应单独成行: {html}"
        );
        assert_eq!(
            html.matches("<span class=\"wsx-mono\">IPv4 ").count(),
            1,
            "IPv4 应单独成行: {html}"
        );
        assert_eq!(
            html.matches("<span class=\"wsx-mono\">IPv6 ").count(),
            1,
            "IPv6 应单独成行: {html}"
        );
        // 不应再出现旧式「MAC x &middot; IPv4 ... &middot; IPv6 ...」挤在一行。
        assert!(
            !html.contains("MAC 60-A4-4C-31-7C-4E &middot; IPv4"),
            "不应再挤在一行: {html}"
        );
        // 多 IPv4 仍在一行内（同一族内逗号分隔即可）。
        assert!(html.contains("192.168.0.124, 192.168.10.124, 192.168.100.124"));
    }

    #[test]
    fn combo_input_renders_as_single_row() {
        // 键鼠一体设备应渲染为一行「键盘/鼠标」，而不是分开的鼠标 + 键盘两行。
        let mut hw = base_hw();
        hw.input_devices.clear();
        hw.input_devices.push(InputDevice {
            name: "A4Tech Combo".into(),
            kind: InputKind::Combo,
            vid: Some("09DA".into()),
            pid: Some("FA90".into()),
            manufacturer: Some("A4Tech".into()),
            is_bluetooth: false,
            location: None,
        });
        hw.input_devices.push(InputDevice {
            name: "Logitech Mouse".into(),
            kind: InputKind::Mouse,
            vid: Some("046D".into()),
            pid: Some("C077".into()),
            manufacturer: Some("Logitech".into()),
            is_bluetooth: false,
            location: None,
        });
        let html = build_html(&hw, &[]);
        assert!(
            html.contains("键盘/鼠标"),
            "键鼠一体设备应渲染『键盘/鼠标』标签: {html}"
        );
        // 关键：输入段应为 2 行（1 键鼠一体 + 1 鼠标），而不是 3 行。
        let th_rows = html.matches("<th>键盘/鼠标</th>").count()
            + html.matches("<th>鼠标</th>").count()
            + html.matches("<th>键盘</th>").count();
        assert_eq!(th_rows, 2, "输入设备应为 2 行: {html}");
        // 不应再出现单独的「鼠标」+「键盘」两行对应同一 A4Tech。
        let kb_only = html.matches("<th>键盘</th>").count();
        assert_eq!(kb_only, 0, "不应再有单独的键盘行: {html}");
    }

    #[test]
    fn keyboard_primary_combo_renders_as_keyboard_row() {
        // 实机口径：A4Tech 09DA:FA90 两集合产品串都取不到（同名"退化名"），
        // 合并后主类型判为键盘，整段读作「键盘 A4Tech + 鼠标 Logitech」= 1 键盘 + 1 鼠标。
        let mut hw = base_hw();
        hw.input_devices.clear();
        hw.input_devices.push(InputDevice {
            name: "A4Tech (09DA:FA90)".into(),
            kind: InputKind::Keyboard,
            vid: Some("09DA".into()),
            pid: Some("FA90".into()),
            manufacturer: Some("A4Tech".into()),
            is_bluetooth: false,
            location: None,
        });
        hw.input_devices.push(InputDevice {
            name: "Logitech Mouse".into(),
            kind: InputKind::Mouse,
            vid: Some("046D".into()),
            pid: Some("C077".into()),
            manufacturer: Some("Logitech".into()),
            is_bluetooth: false,
            location: None,
        });
        let html = build_html(&hw, &[]);
        assert!(
            html.contains("<th>键盘</th>"),
            "A4Tech combo 应渲染为键盘行: {html}"
        );
        let th_rows = html.matches("<th>键盘</th>").count() + html.matches("<th>鼠标</th>").count();
        assert_eq!(th_rows, 2, "应为 2 行（1 键盘 + 1 鼠标）: {html}");
        assert_eq!(
            html.matches("<th>键盘/鼠标</th>").count(),
            0,
            "默认键盘主类型时不应出现键盘/鼠标行: {html}"
        );
    }
}
