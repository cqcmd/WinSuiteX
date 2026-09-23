//! `winsuitex-gui` —— WinSuiteX 硬件概览面板（egui 即时模式）。
//!
//! 三层界面里的「概览面板」一层（§1）：窗口内显示主要硬件紧凑摘要，
//! 详情走 `inventory.html`，磁盘另提供面板内钻取窗（§7）。
//!
//! 数据来源：优先读 `inventory.json` 快照（与 CLI 同结构）；可选「重新采集」走
//! `winsuitex-platform` 实时采集（需 Windows + 提权才完整）。

use std::path::PathBuf;

use egui_extras::image::load_svg_bytes;
use serde::Deserialize;
use winsuitex_core::model::{
    format_power_on_duration, format_uptime, HardwareInfo, HealthStatus, MebiBytes, Volume,
};

/// 命令行 / 渲染镜像用的落盘快照（字段与 CLI 的 `Snapshot` 对齐）。
#[derive(Deserialize)]
struct Snapshot {
    #[serde(default)]
    app_version: String,
    #[serde(default)]
    build_time: String,
    info: HardwareInfo,
    #[serde(default)]
    volumes: Vec<Volume>,
}

/// 9 个分类（顺序固定，与 UI 设计方案 §2 一致）。
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
enum Cat {
    System,
    Cpu,
    Board,
    Memory,
    Gpu,
    Display,
    Disk,
    Audio,
    Nic,
}

/// 健康严重度（对应 §4 状态色铁律）。
#[derive(Copy, Clone, PartialEq, Eq)]
enum Severity {
    Critical, // 紧急
    Warning,  // 预警
    Healthy,  // 健康
    Unknown,  // 无法评估
}

impl Severity {
    fn color(self) -> egui::Color32 {
        match self {
            Severity::Critical => egui::Color32::from_rgb(229, 72, 77),
            Severity::Warning => egui::Color32::from_rgb(245, 166, 35),
            Severity::Healthy => egui::Color32::from_rgb(45, 170, 90),
            Severity::Unknown => egui::Color32::from_rgb(150, 150, 150),
        }
    }
    fn label(self) -> &'static str {
        match self {
            Severity::Critical => "紧急",
            Severity::Warning => "预警",
            Severity::Healthy => "健康",
            Severity::Unknown => "无法评估",
        }
    }
}

struct App {
    info: HardwareInfo,
    volumes: Vec<Volume>,
    app_version: String,
    json_path: Option<PathBuf>,
    report_path: Option<PathBuf>,
    icons: std::collections::HashMap<Cat, egui::TextureHandle>,
    disk_popup: Option<usize>,
    status: String,
    status_err: bool,
}

// ============================================================
// 格式化辅助
// ============================================================

fn fmt_mb(mb: MebiBytes) -> String {
    let v = mb.get();
    if v >= 1024 {
        format!("{:.0} GB", v / 1024)
    } else {
        format!("{v} MB")
    }
}

fn fmt_bytes(b: u64) -> String {
    const G: u64 = 1 << 30;
    const T: u64 = 1 << 40;
    if b >= T {
        format!("{:.1} TB", b as f64 / T as f64)
    } else if b >= G {
        format!("{:.0} GB", b / G)
    } else {
        format!("{:.0} MB", b >> 20)
    }
}

/// `Option<T>` → 展示串；`None` 返回占位（调用方按 `ok=false` 走灰色）。
fn opt<T: std::fmt::Display>(o: &Option<T>, placeholder: &str) -> (String, bool) {
    match o {
        Some(v) => (v.to_string(), true),
        None => (placeholder.to_string(), false),
    }
}

/// 键值行：标签加粗，值按 `ok` 决定黑色 / 灰色（灰 = 无法评估，守 §4 铁律）。
fn kv(ui: &mut egui::Ui, label: &str, value: &str, ok: bool) {
    ui.horizontal_wrapped(|ui| {
        ui.strong(label);
        ui.label(egui::RichText::new(value).color(if ok {
            ui.style().visuals.text_color()
        } else {
            egui::Color32::GRAY
        }));
    });
}

fn with_color(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    ui.label(egui::RichText::new(text).color(color));
}

// ============================================================
// 入口
// ============================================================

fn main() -> eframe::Result {
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([440.0, 700.0]),
        ..Default::default()
    };
    eframe::run_native(
        "WinSuiteX — 硬件概览",
        native,
        Box::new(|cc| {
            setup_fonts(&cc.egui_ctx);
            Ok(Box::new(App::new(cc)))
        }),
    )
}

/// 运行时加载系统中文字体（Windows 自带微软雅黑），否则中文会显示成方块。
/// 不依赖任何网络 / 外部字体文件：直接读 `C:\Windows\Fonts\msyh.ttc`。
#[cfg(windows)]
fn setup_fonts(ctx: &egui::Context) {
    // 常见中文字体候选（.ttc 由 ab_glyph 取集合首张脸）。
    let candidates = [
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\msyh.ttf",
        "C:\\Windows\\Fonts\\simsun.ttc",
        "C:\\Windows\\Fonts\\malgun.ttf",
    ];
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            let mut fonts = egui::FontDefinitions::default();
            fonts
                .font_data
                .insert("cjk".to_string(), egui::FontData::from_owned(bytes));
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "cjk".to_string());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .insert(0, "cjk".to_string());
            ctx.set_fonts(fonts);
            return;
        }
    }
}

#[cfg(not(windows))]
fn setup_fonts(_ctx: &egui::Context) {}

impl App {
    fn new(cc: &eframe::CreationContext) -> Self {
        let mut icons = std::collections::HashMap::new();
        let defs: [(Cat, &str, &[u8]); 9] = [
            (
                Cat::System,
                "system",
                include_bytes!("../assets/icons/system.svg"),
            ),
            (Cat::Cpu, "cpu", include_bytes!("../assets/icons/cpu.svg")),
            (
                Cat::Board,
                "board",
                include_bytes!("../assets/icons/board.svg"),
            ),
            (
                Cat::Memory,
                "ram",
                include_bytes!("../assets/icons/ram.svg"),
            ),
            (Cat::Gpu, "gpu", include_bytes!("../assets/icons/gpu.svg")),
            (
                Cat::Display,
                "monitor",
                include_bytes!("../assets/icons/monitor.svg"),
            ),
            (
                Cat::Disk,
                "disk",
                include_bytes!("../assets/icons/disk.svg"),
            ),
            (
                Cat::Audio,
                "audio",
                include_bytes!("../assets/icons/audio.svg"),
            ),
            (Cat::Nic, "nic", include_bytes!("../assets/icons/nic.svg")),
        ];
        for (cat, name, bytes) in defs {
            let uri = format!("icon_{name}");
            if let Ok(img) = load_svg_bytes(bytes) {
                let tex = cc
                    .egui_ctx
                    .load_texture(uri, img, egui::TextureOptions::LINEAR);
                icons.insert(cat, tex);
            }
        }

        // 解析命令行：可选 `--json <path>`；否则默认 `inventory.json`。
        let args: Vec<String> = std::env::args().skip(1).collect();
        let json_arg = args
            .iter()
            .position(|a| a == "--json")
            .and_then(|i| args.get(i + 1).cloned());

        let (info, volumes, app_version, json_path, report_path, status, status_err) =
            match load_json(json_arg.as_deref()) {
                Ok(s) => {
                    let jp = json_arg.clone().or_else(|| Some("inventory.json".into()));
                    let rp = jp.as_ref().map(|p| PathBuf::from(p).with_extension("html"));
                    let status = format!("采集版本 {} · 采集时间 {}", s.app_version, s.build_time);
                    (
                        s.info,
                        s.volumes,
                        s.app_version,
                        jp.map(PathBuf::from),
                        rp,
                        status,
                        false,
                    )
                }
                Err(e) => (
                    HardwareInfo::default(),
                    Vec::new(),
                    String::new(),
                    None,
                    None,
                    e,
                    true,
                ),
            };

        Self {
            info,
            volumes,
            app_version,
            json_path,
            report_path,
            icons,
            disk_popup: None,
            status,
            status_err,
        }
    }

    /// 整体健康（按磁盘严重度汇总）。
    fn overall_severity(&self) -> Severity {
        let mut worst = Severity::Healthy;
        for d in &self.info.disks {
            let s = disk_severity(d);
            if s == Severity::Critical {
                return Severity::Critical;
            }
            if s == Severity::Warning {
                worst = Severity::Warning;
            }
            if s == Severity::Unknown && worst == Severity::Healthy {
                worst = Severity::Unknown;
            }
        }
        worst
    }
}

fn load_json(arg: Option<&str>) -> Result<Snapshot, String> {
    let path = arg.unwrap_or("inventory.json");
    let text = std::fs::read_to_string(path).map_err(|_| {
        format!("未找到 {path}：请先运行 winsuitex --json 生成，或用 --json 指定路径")
    })?;
    let snap: Snapshot =
        serde_json::from_str(&text).map_err(|e| format!("解析 {path} 失败：{e}"))?;
    Ok(snap)
}

fn disk_severity(d: &winsuitex_core::model::PhysicalDisk) -> Severity {
    match d.smart.as_ref().map(|s| s.health_status) {
        Some(HealthStatus::Critical) => Severity::Critical,
        Some(HealthStatus::Warning) => Severity::Warning,
        Some(HealthStatus::Healthy) => Severity::Healthy,
        _ => Severity::Unknown,
    }
}

// ============================================================
// 渲染
// ============================================================

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(4.0);
            // 顶部状态条
            ui.horizontal(|ui| {
                ui.heading("WinSuiteX 硬件概览");
                if let Some(v) = self.app_version_if_present() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.small(v);
                    });
                }
            });
            ui.separator();

            egui::ScrollArea::vertical().show(ui, |ui| {
                self.render_system(ui);
                self.render_cpu(ui);
                self.render_board(ui);
                self.render_memory(ui);
                self.render_gpu(ui);
                self.render_display(ui);
                self.render_disk(ui);
                self.render_audio(ui);
                self.render_nic(ui);
            });

            ui.separator();
            // 底部：整体健康 + 打开网页报告
            ui.horizontal(|ui| {
                let sev = self.overall_severity();
                with_color(ui, &format!("整体健康：● {}", sev.label()), sev.color());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("打开网页报告 →").clicked() {
                        self.open_report();
                    }
                });
            });
            if self.status_err {
                ui.colored_label(egui::Color32::from_rgb(229, 72, 77), &self.status);
            } else {
                ui.small(&self.status);
            }
            // 数据来源
            if let Some(p) = &self.json_path {
                ui.small(format!("数据来源：{}", p.display()));
            }

            // 磁盘钻取窗
            self.render_disk_popup(ctx);
        });
    }
}

impl App {
    fn app_version_if_present(&self) -> Option<&str> {
        if self.app_version.is_empty() {
            None
        } else {
            Some(&self.app_version)
        }
    }

    /// 分类标题行：图标 + 名称 + 右侧严重度色点。
    fn category_header(&self, ui: &mut egui::Ui, cat: Cat, title: &str, sev: Severity) {
        ui.horizontal(|ui| {
            if let Some(tex) = self.icons.get(&cat) {
                ui.image(egui::load::SizedTexture::new(
                    tex.id(),
                    egui::vec2(18.0, 18.0),
                ));
            }
            ui.strong(title);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new("●").color(sev.color()).strong());
            });
        });
    }

    fn render_system(&self, ui: &mut egui::Ui) {
        self.category_header(ui, Cat::System, "系统", Severity::Unknown);
        if let Some(os) = &self.info.os {
            let (prod, ok_p) = opt(&os.product, "未知");
            let (ver, ok_v) = opt(&os.version, "未知");
            let (build, _ok_b) = opt(&os.build, "未知");
            kv(
                ui,
                "系统",
                &format!("{prod} {ver} (build {build})"),
                ok_p && ok_v,
            );
            let (boot, ok_boot) = opt(&os.boot_at, "—/未采集");
            kv(ui, "开机时间", &boot, ok_boot);
            let uptime = os
                .uptime_seconds
                .map(format_uptime)
                .unwrap_or_else(|| "—/未采集".into());
            kv(ui, "在线时长", &uptime, os.uptime_seconds.is_some());
        } else {
            kv(ui, "系统", "未知", false);
        }
        ui.add_space(6.0);
    }

    fn render_cpu(&self, ui: &mut egui::Ui) {
        self.category_header(ui, Cat::Cpu, "处理器", Severity::Unknown);
        if self.info.cpus.is_empty() {
            kv(ui, "CPU", "未识别到", false);
        }
        for (i, c) in self.info.cpus.iter().enumerate() {
            let model = c.model.clone().unwrap_or_else(|| "未知型号".into());
            let cores = c.cores.map(|n| n.to_string()).unwrap_or_else(|| "?".into());
            let threads = c
                .threads
                .map(|n| n.to_string())
                .unwrap_or_else(|| "?".into());
            let codename = c.codename.as_deref().unwrap_or("");
            let line = if codename.is_empty() {
                format!("{} · {} 核 / {} 线程", model, cores, threads)
            } else {
                format!("{} · {} 核 / {} 线程 · {}", model, cores, threads, codename)
            };
            kv(ui, &format!("CPU{}", i + 1), &line, c.model.is_some());
        }
        ui.add_space(6.0);
    }

    fn render_board(&self, ui: &mut egui::Ui) {
        self.category_header(ui, Cat::Board, "主板", Severity::Unknown);
        if let Some(b) = &self.info.board {
            let (vendor, ok_v) = opt(&b.vendor, "—");
            let (product, ok_p) = opt(&b.product, "—");
            kv(
                ui,
                "厂商/型号",
                &format!("{vendor} {product}"),
                ok_v && ok_p,
            );
            let (bv, ok_bv) = opt(&b.bios_vendor, "未知");
            let (bvv, ok_bvv) = opt(&b.bios_version, "未知");
            kv(ui, "BIOS", &format!("{bv} {bvv}"), ok_bv && ok_bvv);
        } else {
            kv(ui, "主板", "未知", false);
        }
        ui.add_space(6.0);
    }

    fn render_memory(&self, ui: &mut egui::Ui) {
        self.category_header(ui, Cat::Memory, "内存", Severity::Unknown);
        let total = self
            .info
            .memory
            .total_mb
            .map(fmt_mb)
            .unwrap_or_else(|| "未知".into());
        let sticks = &self.info.memory.sticks;
        let populated: Vec<_> = sticks.iter().filter(|s| s.is_populated()).collect();
        let empty = sticks.len() - populated.len();
        kv(
            ui,
            "总容量",
            &format!("{} · 已插 {} / 空槽 {}", total, populated.len(), empty),
            self.info.memory.total_mb.is_some(),
        );
        // 仅列已插内存条（空槽不逐行，§3）
        for s in &populated {
            let cap = s.capacity_mb.map(fmt_mb).unwrap_or_else(|| "?".into());
            let (spd, ok_spd) = opt(&s.speed_mhz.map(|m| format!("{m} MT/s")), "频率未知");
            let vendor = s.vendor.as_deref().unwrap_or("未知厂商");
            let slot = s.locator.as_deref().unwrap_or("");
            let line = if slot.is_empty() {
                format!("{vendor} {cap} · {spd}")
            } else {
                format!("{vendor} {cap} · {spd} · {slot}")
            };
            kv(ui, "  ", &line, ok_spd);
        }
        ui.add_space(6.0);
    }

    fn render_gpu(&self, ui: &mut egui::Ui) {
        self.category_header(ui, Cat::Gpu, "显卡", Severity::Unknown);
        if self.info.gpus.is_empty() {
            kv(ui, "显卡", "未识别到", false);
        }
        for (i, g) in self.info.gpus.iter().enumerate() {
            let vram = g.vram_mb.map(fmt_mb).unwrap_or_else(|| "?".into());
            let mut line = format!("{} · {}", g.name, vram);
            if let Some(t) = &g.telemetry {
                if let Some(temp) = t.temp_c {
                    line.push_str(&format!(" · {}°C", temp));
                }
                if let Some(pw) = t.power_w {
                    line.push_str(&format!(" · {}W", pw));
                }
            }
            let ok = g.name != "Microsoft Basic Render Driver"
                && g.kind != winsuitex_core::model::GpuKind::Software;
            kv(ui, &format!("GPU{}", i + 1), &line, ok);
        }
        ui.add_space(6.0);
    }

    fn render_display(&self, ui: &mut egui::Ui) {
        self.category_header(ui, Cat::Display, "显示器", Severity::Unknown);
        if self.info.displays.is_empty() {
            kv(ui, "显示器", "未识别到", false);
        }
        for (i, d) in self.info.displays.iter().enumerate() {
            let name = d.name.clone().unwrap_or_else(|| "未知显示器".into());
            let res = match (d.width, d.height) {
                (Some(w), Some(h)) => format!("{}×{}", w, h),
                _ => "分辨率未知".to_string(),
            };
            let hz = d
                .refresh_hz
                .map(|r| format!(" @ {}Hz", r))
                .unwrap_or_default();
            let line = format!("{} · {}{}", name, res, hz);
            kv(ui, &format!("显示器{}", i + 1), &line, d.name.is_some());
        }
        ui.add_space(6.0);
    }

    fn render_disk(&mut self, ui: &mut egui::Ui) {
        self.category_header(
            ui,
            Cat::Disk,
            &format!("磁盘 ({} 块)", self.info.disks.len()),
            Severity::Unknown,
        );
        if self.info.disks.is_empty() {
            kv(ui, "磁盘", "未识别到", false);
        }
        // 按索引迭代：避免循环体内的不可变借用与右键闭包的 &mut self 冲突。
        let n = self.info.disks.len();
        for i in 0..n {
            // 先把这一行需要的字段克隆成拥有值，循环体内不再持有对 self 的借用。
            let (model, iface, media, size, sev, reallocated, pending, serial) = {
                let d = &self.info.disks[i];
                let model = d.model.clone().unwrap_or_else(|| "未知型号".into());
                let iface = d.iface.clone().unwrap_or_else(|| "?".into());
                let media = d.media.clone().unwrap_or_else(|| "?".into());
                let size = d.size_bytes.map(fmt_bytes).unwrap_or_else(|| "?".into());
                let sev = disk_severity(d);
                let (reallocated, pending) = match &d.smart {
                    Some(s) => (
                        s.reallocated_sectors.filter(|&r| r > 0),
                        s.pending_sectors.filter(|&p| p > 0),
                    ),
                    None => (None, None),
                };
                let serial = d.serial.clone().unwrap_or_else(|| "未透传".into());
                (model, iface, media, size, sev, reallocated, pending, serial)
            };
            let mut line = format!("{} · {} · {} · {}", model, iface, media, size);
            if let Some(r) = reallocated {
                line.push_str(&format!(" · 重映射{}", r));
            }
            if let Some(p) = pending {
                line.push_str(&format!(" · 待映射{}", p));
            }
            // 整行可点击（左键打开钻取窗）/ 右键菜单
            let resp = ui
                .horizontal(|ui| {
                    ui.label(egui::RichText::new("●").color(sev.color()));
                    ui.label(egui::RichText::new(line.clone()));
                })
                .response;
            if resp.clicked() {
                self.disk_popup = Some(i);
            }
            resp.context_menu(|ui| {
                if ui.button("磁盘详情").clicked() {
                    self.disk_popup = Some(i);
                    ui.close_menu();
                }
                if ui.button("复制序列号").clicked() {
                    ui.output_mut(|o| o.copied_text = serial.clone());
                    ui.close_menu();
                }
                if ui.button("导出单盘快照").clicked() {
                    self.export_disk_snapshot(i);
                    ui.close_menu();
                }
            });
        }
        ui.add_space(6.0);
    }

    fn render_audio(&self, ui: &mut egui::Ui) {
        self.category_header(ui, Cat::Audio, "声卡", Severity::Unknown);
        if self.info.audio.is_empty() {
            kv(ui, "声卡", "无（未检测到）", false);
        }
        for (i, a) in self.info.audio.iter().enumerate() {
            kv(ui, &format!("声卡{}", i + 1), &a.name, !a.name.is_empty());
        }
        ui.add_space(6.0);
    }

    fn render_nic(&self, ui: &mut egui::Ui) {
        self.category_header(ui, Cat::Nic, "网卡", Severity::Unknown);
        if self.info.nics.is_empty() {
            kv(ui, "网卡", "未识别到", false);
        }
        for (i, n) in self.info.nics.iter().enumerate() {
            let desc = n.desc.clone().unwrap_or_else(|| "未知网卡".into());
            let mac = n.mac.clone().unwrap_or_else(|| "?".into());
            let ipv4 = if n.ipv4.is_empty() {
                "无IP".to_string()
            } else {
                n.ipv4.join(", ")
            };
            let speed = n
                .speed_bps
                .map(|b| format!("{} Mbps", b / 1_000_000))
                .unwrap_or_else(|| "速率未知".into());
            let line = format!("{} · {} · {} · {}", desc, mac, ipv4, speed);
            kv(ui, &format!("网卡{}", i + 1), &line, n.desc.is_some());
        }
        ui.add_space(6.0);
    }

    fn render_disk_popup(&mut self, ctx: &egui::Context) {
        if let Some(i) = self.disk_popup {
            let d = match self.info.disks.get(i) {
                Some(d) => d.clone(),
                None => {
                    self.disk_popup = None;
                    return;
                }
            };
            let model = d.model.clone().unwrap_or_else(|| "未知型号".into());
            let sev = disk_severity(&d);
            let mut open = true;
            egui::Window::new(format!("磁盘详情 — {model}"))
                .open(&mut open)
                .resizable(true)
                .show(ctx, |ui| {
                    let media = d.media.clone().unwrap_or_else(|| "?".into());
                    let iface = d.iface.clone().unwrap_or_else(|| "?".into());
                    let size = d.size_bytes.map(fmt_bytes).unwrap_or_else(|| "?".into());
                    kv(
                        ui,
                        "介质/接口",
                        &format!("{media} / {iface}"),
                        d.media.is_some(),
                    );
                    kv(ui, "容量", &size, d.size_bytes.is_some());
                    with_color(ui, &format!("健康：● {}", sev.label()), sev.color());
                    if let Some(smart) = &d.smart {
                        if let Some(r) = smart.reallocated_sectors {
                            kv(ui, "重映射扇区", &r.to_string(), r > 0);
                        }
                        if let Some(p) = smart.pending_sectors {
                            kv(ui, "待映射扇区", &p.to_string(), p > 0);
                        }
                        if let Some(m) = smart.media_errors {
                            kv(ui, "介质错误", &m.to_string(), m > 0);
                        }
                        if let Some(h) = smart.power_on_hours {
                            kv(ui, "通电时长", &format_power_on_duration(h), true);
                        } else {
                            kv(ui, "通电时长", "—/未采集", false);
                        }
                        if let Some(t) = smart.temperature_c {
                            kv(ui, "温度", &format!("{t} °C"), true);
                        } else {
                            kv(ui, "温度", "—/未采集", false);
                        }
                        if let Some(u) = smart.percentage_used {
                            kv(ui, "磨损(已用)", &format!("{u}%"), true);
                        }
                        let src_str = match smart.source {
                            winsuitex_core::model::SmartSource::NvmeLogPage => {
                                "NVMe Log".to_string()
                            }
                            winsuitex_core::model::SmartSource::AtaSmart => "ATA SMART".to_string(),
                            winsuitex_core::model::SmartSource::StorageReliabilityCounter => {
                                "WMI ReliabilityCounter".to_string()
                            }
                            winsuitex_core::model::SmartSource::None => "未采集".to_string(),
                        };
                        let (src, ok_src) = opt(&Some(src_str), "未采集");
                        kv(ui, "数据来源", &src, ok_src);
                    } else {
                        kv(ui, "SMART", "数据缺失", false);
                    }
                    // 卷/分区信息（§7：C++ 卷/分区 ⊕ Rust SMART 超集）
                    // 在函数闭包内联过滤，避免持有对 self.volumes 的借用与下方
                    // `self.disk_popup = None` 的可变借用冲突。
                    let vols: Vec<Volume> = self
                        .volumes
                        .iter()
                        .filter(|v| v.disk_index == Some(d.index))
                        .cloned()
                        .collect();
                    if vols.is_empty() {
                        kv(ui, "分区", "未枚举（采集缺口）", false);
                    } else {
                        for v in &vols {
                            let fs = v.fs_name.clone().unwrap_or_else(|| "?".into());
                            let free = v.free_bytes.map(fmt_bytes).unwrap_or_else(|| "?".into());
                            let total = v.total_bytes.map(fmt_bytes).unwrap_or_else(|| "?".into());
                            kv(
                                ui,
                                &v.letter,
                                &format!("{} · {} · 可用 {}", fs, total, free),
                                true,
                            );
                        }
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("一键复制").clicked() {
                            ui.output_mut(|o| o.copied_text = self.disk_copy_text(i));
                        }
                        if ui.button("打开网页报告").clicked() {
                            self.open_report();
                        }
                    });
                });
            if !open {
                self.disk_popup = None;
            }
        }
    }

    fn disk_copy_text(&self, i: usize) -> String {
        let d = match self.info.disks.get(i) {
            Some(d) => d,
            None => return String::new(),
        };
        let mut s = String::new();
        s.push_str(&format!("型号: {}\n", d.model.clone().unwrap_or_default()));
        s.push_str(&format!("接口: {}\n", d.iface.clone().unwrap_or_default()));
        s.push_str(&format!(
            "序列号: {}\n",
            d.serial.clone().unwrap_or_else(|| "未透传".into())
        ));
        if let Some(smart) = &d.smart {
            if let Some(h) = smart.power_on_hours {
                s.push_str(&format!("通电: {}\n", format_power_on_duration(h)));
            }
            if let Some(r) = smart.reallocated_sectors {
                s.push_str(&format!("重映射: {r}\n"));
            }
        }
        s
    }

    fn export_disk_snapshot(&mut self, i: usize) {
        let d = match self.info.disks.get(i) {
            Some(d) => d,
            None => return,
        };
        let name = d
            .model
            .clone()
            .unwrap_or_else(|| format!("disk{}", d.index));
        let sn = d.serial.clone().unwrap_or_else(|| "nosn".into());
        let out = format!("disk_{}_{}.json", sanitize(&name), sanitize(&sn));
        match serde_json::to_string_pretty(d) {
            Ok(json) => match std::fs::write(&out, json) {
                Ok(()) => {
                    self.status = format!("已导出单盘快照：{out}");
                    self.status_err = false;
                }
                Err(e) => {
                    self.status = format!("导出失败：{e}");
                    self.status_err = true;
                }
            },
            Err(e) => {
                self.status = format!("序列化失败：{e}");
                self.status_err = true;
            }
        }
    }

    #[cfg(windows)]
    fn open_report(&self) {
        if let Some(p) = &self.report_path {
            if p.exists() {
                let _ = std::process::Command::new("cmd")
                    .args(["/c", "start", "", &p.to_string_lossy()])
                    .spawn();
            }
        }
    }

    #[cfg(not(windows))]
    fn open_report(&self) {
        // 非 Windows：仅记录路径，不实际打开。
    }
}

/// 文件名清洗：去掉路径非法字符。
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}
