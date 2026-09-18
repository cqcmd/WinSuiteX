//! CPU 微架构代号识别。
//!
//! 纯 Rust 实现，通过 `core::arch::x86_64::__cpuid` 读取 CPUID 寄存器，
//! 再配合一张 `(vendor, family, model) -> 代号` 的静态表完成识别。
//!
//! 设计取舍：C++ 版用的是 libcpuid（GNU-C 源码 + gcc 内联汇编），但本仓库
//! Rust 侧是 MSVC 工具链（`x86_64-pc-windows-msvc`），用 `cc` crate 去编
//! 那些 GNU-C 文件会踩 cl.exe 的坑。因此**放弃 FFI / 构建期 C 依赖**，改成
//! 自带静态表 + 寄存器直读，所有逻辑均为 safe（`__cpuid` 在标准库里是安全函数）。
//!
//! # 数据来源与算法依据（2026-09 复核，覆盖至 2026 年已发布/确认的新品）
//! - 微架构对照表：以 **Intel SDM Vol.4 Table 2-1（CPUID Signature Values of
//!   DisplayFamily_DisplayModel）** 为权威基准，并交叉核对 **Linux 内核
//!   `arch/x86/include/asm/intel-family.h`（master，2026 仍持续更新）**、
//!   LibreHardwareMonitor `IntelCpu.cs`、fuchsia `zircon/kernel/lib/arch/cpuid.cc`、
//!   以及 `siomon` `cpu_codenames.rs`（现代桌面/移动/服务器全系）。AMD 部分以
//!   **AMD 官方 Technical Information Portal 泄露清单（InstLatX64 于 2026-08-26
//!   捕获，首份来自 AMD 自身的 Zen 6 路线图确认源）**、ZenStates-Core、wikichip
//!   `amd/cpuid`、siomon 为准。
//! - **2025–2026 新增（本次补全）**：Intel Panther Lake（0xCC / 0xE5）、Arrow
//!   Lake-U（0xB5，原表只覆盖 H/S）、Wildcat Lake（0xD5）、Bartlett Lake（0xD7）、
//!   **Nova Lake（首次离开 Family 6 → Family 18，0x01/0x03）**、Diamond Rapids
//!   （Family 19，服务器）；AMD **Zen 6（Family 1Ah 内 0x80–0x8F / 0xE0–0xE3）**。
//!   上述 Intel 型号均经 `intel-family.h` 逐条核对；AMD 型号经 AMD 门户泄露清单
//!   与 Geekbench 实测 `Family 26 Model 128` 双重确认。
//! - family/model 计算公式：与 Intel SDM Vol.2A 及 libcpuid 一致（见
//!   [`cpuid_family_model_stepping`]）——Intel 与 AMD 通用。
//! - 混合架构（Intel 12 代起的 P-core + E-core）：依据 Intel® Alder Lake
//!   Developer Guide，`CPUID.07H:EDX[15]` 为 Hybrid 标志位。
//!
//! 识别不出来一律返回 `None`，绝不返回空串或 "Unknown" 充数。

use core::arch::x86_64::__cpuid;

/// CPU 厂商。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CpuVendor {
    Intel,
    Amd,
    Unknown,
}

/// 静态识别表的一项。
///
/// `family` / `model` 是 libcpuid 的 `ext_family` / `ext_model`（完整值）：
///
/// - `family = base_family + (base_family == 0xF ? extended_family : 0)`
/// - `model  = (base_family == 0x6 || base_family == 0xF) ? (extended_model << 4 | base_model) : base_model`
///
/// `stepping` 为 `None` 表示不区分具体步进（多数条目）
///
#[derive(Clone, Copy)]
struct Entry {
    vendor: CpuVendor,
    family: u32,
    model: u32,
    stepping: Option<u32>,
    name: &'static str,
}

/// 主流桌面/服务器微架构表。
///
/// 移植自 libcpuid 的 `cpudb_intel` / `cpudb_amd`，并依据上述权威来源补全了
/// 2017 年以后的全系微架构（Tiger/Rocket/Alder/Raptor Lake、Meteor/Lunar/Arrow
/// Lake、Sapphire/Emerald/Granite Rapids 服务器线，以及 AMD Zen 2~Zen 5），
/// **并进一步补至 2026 年已确认的新品**：Intel Panther Lake / Arrow Lake-U /
/// Wildcat·Bartlett Lake / Nova Lake（Family 18）/ Diamond Rapids（Family 19），
/// AMD Zen 6（Medusa Point / Olympic Ridge / Medusa2）。
#[rustfmt::skip]
static TABLE: &[Entry] = &[
    // ---------- Intel：i486 / Pentium ----------
    Entry { vendor: CpuVendor::Intel, family: 4, model: 0,  stepping: None, name: "i486 DX-25/33" },
    Entry { vendor: CpuVendor::Intel, family: 4, model: 1,  stepping: None, name: "i486 DX-50" },
    Entry { vendor: CpuVendor::Intel, family: 4, model: 2,  stepping: None, name: "i486 SX" },
    Entry { vendor: CpuVendor::Intel, family: 4, model: 3,  stepping: None, name: "i486 DX2" },
    Entry { vendor: CpuVendor::Intel, family: 4, model: 4,  stepping: None, name: "i486 SL" },
    Entry { vendor: CpuVendor::Intel, family: 4, model: 5,  stepping: None, name: "i486 SX2" },
    Entry { vendor: CpuVendor::Intel, family: 4, model: 7,  stepping: None, name: "i486 DX2 WriteBack" },
    Entry { vendor: CpuVendor::Intel, family: 4, model: 8,  stepping: None, name: "i486 DX4" },
    Entry { vendor: CpuVendor::Intel, family: 4, model: 9,  stepping: None, name: "i486 DX4 WriteBack" },
    Entry { vendor: CpuVendor::Intel, family: 5, model: 0,  stepping: None, name: "Pentium A-Step" },
    Entry { vendor: CpuVendor::Intel, family: 5, model: 1,  stepping: None, name: "Pentium" },
    Entry { vendor: CpuVendor::Intel, family: 5, model: 2,  stepping: None, name: "Pentium" },
    Entry { vendor: CpuVendor::Intel, family: 5, model: 3,  stepping: None, name: "Pentium OverDrive" },
    Entry { vendor: CpuVendor::Intel, family: 5, model: 4,  stepping: None, name: "Pentium" },
    Entry { vendor: CpuVendor::Intel, family: 5, model: 7,  stepping: None, name: "Pentium" },
    Entry { vendor: CpuVendor::Intel, family: 5, model: 8,  stepping: None, name: "Pentium MMX" },

    // ---------- Intel：P6 / NetBurst ----------
    Entry { vendor: CpuVendor::Intel, family: 6, model: 0,  stepping: None, name: "Pentium Pro" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 1,  stepping: None, name: "Pentium Pro" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 3,  stepping: None, name: "Pentium II (Klamath)" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 5,  stepping: None, name: "Pentium II (Deschutes)" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 6,  stepping: None, name: "Pentium II (Dixon)" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 7,  stepping: None, name: "Pentium III (Katmai)" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 8,  stepping: None, name: "Pentium III (Coppermine)" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 10, stepping: None, name: "Pentium III (Coppermine)" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 11, stepping: None, name: "Pentium III (Tualatin)" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 9,  stepping: None, name: "Pentium M (Banias)" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 13, stepping: None, name: "Pentium M (Dothan)" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 12, stepping: None, name: "Atom" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 14, stepping: None, name: "Yonah (Core)" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 15, stepping: None, name: "Core 2 (Conroe/Merom)" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 22, stepping: None, name: "Core 2 (Conroe-L/Celeron)" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 23, stepping: None, name: "Penryn (Wolfdale/Yorkfield)" },
    Entry { vendor: CpuVendor::Intel, family: 15, model: 0, stepping: None, name: "Pentium 4 (Willamette)" },
    Entry { vendor: CpuVendor::Intel, family: 15, model: 1, stepping: None, name: "Pentium 4 (Willamette)" },
    Entry { vendor: CpuVendor::Intel, family: 15, model: 2, stepping: None, name: "Pentium 4 (Northwood)" },
    Entry { vendor: CpuVendor::Intel, family: 15, model: 3, stepping: None, name: "Pentium 4 (Prescott)" },
    Entry { vendor: CpuVendor::Intel, family: 15, model: 4, stepping: None, name: "Pentium 4 (Prescott)" },
    Entry { vendor: CpuVendor::Intel, family: 15, model: 6, stepping: None, name: "Pentium 4 (Cedar Mill)" },

    // ---------- Intel：Nehalem / Westmere ----------
    Entry { vendor: CpuVendor::Intel, family: 6, model: 26, stepping: None, name: "Nehalem (Bloomfield/Gainestown)" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 30, stepping: None, name: "Nehalem (Lynnfield)" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 37, stepping: None, name: "Westmere (Clarkdale/Arrandale)" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 44, stepping: None, name: "Westmere (Gulftown)" },

    // ---------- Intel：Sandy Bridge / Ivy Bridge ----------
    Entry { vendor: CpuVendor::Intel, family: 6, model: 42, stepping: None, name: "Sandy Bridge" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 45, stepping: None, name: "Sandy Bridge-E" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 58, stepping: None, name: "Ivy Bridge" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 62, stepping: None, name: "Ivy Bridge-E" },

    // ---------- Intel：Haswell / Broadwell ----------
    Entry { vendor: CpuVendor::Intel, family: 6, model: 60, stepping: None, name: "Haswell" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 69, stepping: None, name: "Haswell" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 70, stepping: None, name: "Haswell" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 63, stepping: None, name: "Haswell-E" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 71, stepping: None, name: "Broadwell" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 61, stepping: None, name: "Broadwell-U" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 79, stepping: None, name: "Broadwell-E" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 86, stepping: None, name: "Broadwell-DE" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 102, stepping: None, name: "Cannon Lake" },

    // ---------- Intel：Skylake 及后续 ----------
    Entry { vendor: CpuVendor::Intel, family: 6, model: 94,  stepping: None, name: "Skylake" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 78,  stepping: None, name: "Skylake" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 85,  stepping: None, name: "Skylake-X (Xeon Scalable)" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 158, stepping: Some(9), name: "Kaby Lake" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 158, stepping: None, name: "Coffee Lake" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 142, stepping: None, name: "Kaby Lake-U" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 126, stepping: None, name: "Ice Lake" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 125, stepping: None, name: "Ice Lake" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 106, stepping: None, name: "Ice Lake-SP" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 108, stepping: None, name: "Ice Lake-SP" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 165, stepping: None, name: "Comet Lake" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 140, stepping: None, name: "Tiger Lake" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 141, stepping: None, name: "Tiger Lake" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 167, stepping: None, name: "Rocket Lake" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 151, stepping: None, name: "Alder Lake" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 154, stepping: None, name: "Alder Lake-P" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 190, stepping: None, name: "Alder Lake" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 183, stepping: None, name: "Raptor Lake" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 186, stepping: None, name: "Raptor Lake-P" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 191, stepping: None, name: "Raptor Lake-S" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 170, stepping: None, name: "Meteor Lake" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 172, stepping: None, name: "Meteor Lake" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 189, stepping: None, name: "Lunar Lake" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 197, stepping: None, name: "Arrow Lake" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 198, stepping: None, name: "Arrow Lake-H" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 143, stepping: None, name: "Sapphire Rapids" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 207, stepping: None, name: "Emerald Rapids" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 173, stepping: None, name: "Granite Rapids" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 174, stepping: None, name: "Granite Rapids" },

    // ---------- Intel 2024–2026 客户端/服务器（family 6 收尾 + 新 family） ----------
    // 来源：Linux intel-family.h (master) 逐条核对。
    Entry { vendor: CpuVendor::Intel, family: 6, model: 181, stepping: None, name: "Arrow Lake-U" },
    Entry { vendor: CpuVendor::Intel, family: 6, model: 204, stepping: None, name: "Panther Lake" },   // 0xCC, Cougar Cove/Darkmont, 酷睿Ultra 300 (CES 2026)
    Entry { vendor: CpuVendor::Intel, family: 6, model: 213, stepping: None, name: "Wildcat Lake" },   // 0xD5, 入门（接替 Twin Lake）
    Entry { vendor: CpuVendor::Intel, family: 6, model: 215, stepping: None, name: "Bartlett Lake" },   // 0xD7, 桌面（Raptor Cove）
    Entry { vendor: CpuVendor::Intel, family: 6, model: 229, stepping: None, name: "Panther Lake-R" },  // 0xE5, 加固型
    // Nova Lake：Intel 20 年来首次离开 Family 6，改用 Family 18（酷睿 Ultra 400，2026 下半年）
    Entry { vendor: CpuVendor::Intel, family: 18, model: 1, stepping: None, name: "Nova Lake" },         // 桌面 S
    Entry { vendor: CpuVendor::Intel, family: 18, model: 3, stepping: None, name: "Nova Lake-L" },       // 移动
    // Diamond Rapids：服务器，Family 19（Panther Cove P-core，2026 H2）
    Entry { vendor: CpuVendor::Intel, family: 19, model: 1, stepping: None, name: "Diamond Rapids" },

    // ================= AMD =================
    // ---------- K5 / K6 / Athlon (K7) ----------
    Entry { vendor: CpuVendor::Amd, family: 5, model: 0,  stepping: None, name: "K5" },
    Entry { vendor: CpuVendor::Amd, family: 5, model: 1,  stepping: None, name: "K5" },
    Entry { vendor: CpuVendor::Amd, family: 5, model: 2,  stepping: None, name: "K5" },
    Entry { vendor: CpuVendor::Amd, family: 5, model: 3,  stepping: None, name: "K5" },
    Entry { vendor: CpuVendor::Amd, family: 5, model: 6,  stepping: None, name: "K6" },
    Entry { vendor: CpuVendor::Amd, family: 5, model: 7,  stepping: None, name: "K6" },
    Entry { vendor: CpuVendor::Amd, family: 5, model: 8,  stepping: None, name: "K6-2" },
    Entry { vendor: CpuVendor::Amd, family: 5, model: 9,  stepping: None, name: "K6-III" },
    Entry { vendor: CpuVendor::Amd, family: 5, model: 13, stepping: None, name: "K6-2+" },
    Entry { vendor: CpuVendor::Amd, family: 6, model: 1,  stepping: None, name: "Athlon (Slot-A)" },
    Entry { vendor: CpuVendor::Amd, family: 6, model: 2,  stepping: None, name: "Athlon (Slot-A)" },
    Entry { vendor: CpuVendor::Amd, family: 6, model: 3,  stepping: None, name: "Duron (Spitfire)" },
    Entry { vendor: CpuVendor::Amd, family: 6, model: 4,  stepping: None, name: "Athlon (Thunderbird)" },
    Entry { vendor: CpuVendor::Amd, family: 6, model: 6,  stepping: None, name: "Athlon XP" },
    Entry { vendor: CpuVendor::Amd, family: 6, model: 7,  stepping: None, name: "Duron (Morgan)" },
    Entry { vendor: CpuVendor::Amd, family: 6, model: 8,  stepping: None, name: "Athlon XP (Thoroughbred)" },
    Entry { vendor: CpuVendor::Amd, family: 6, model: 10, stepping: None, name: "Athlon XP (Barton)" },

    // ---------- K8 (ext family 15) ----------
    Entry { vendor: CpuVendor::Amd, family: 15, model: 3,   stepping: None, name: "Opteron" },
    Entry { vendor: CpuVendor::Amd, family: 15, model: 4,   stepping: None, name: "Athlon 64 FX" },
    Entry { vendor: CpuVendor::Amd, family: 15, model: 8,   stepping: None, name: "Sempron 64" },
    Entry { vendor: CpuVendor::Amd, family: 15, model: 12,  stepping: None, name: "Athlon 64 (Newcastle)" },
    Entry { vendor: CpuVendor::Amd, family: 15, model: 27,  stepping: None, name: "Athlon 64 (San Diego)" },
    Entry { vendor: CpuVendor::Amd, family: 15, model: 28,  stepping: None, name: "Athlon 64 (Venice)" },
    Entry { vendor: CpuVendor::Amd, family: 15, model: 44,  stepping: None, name: "Athlon 64 (Orleans)" },
    Entry { vendor: CpuVendor::Amd, family: 15, model: 47,  stepping: None, name: "Athlon 64 (Venice)" },
    Entry { vendor: CpuVendor::Amd, family: 15, model: 79,  stepping: None, name: "Athlon 64 (Orleans)" },
    Entry { vendor: CpuVendor::Amd, family: 15, model: 95,  stepping: None, name: "Athlon 64 (Manila)" },
    Entry { vendor: CpuVendor::Amd, family: 15, model: 35,  stepping: None, name: "Athlon 64 X2 (Toledo)" },
    Entry { vendor: CpuVendor::Amd, family: 15, model: 43,  stepping: None, name: "Athlon 64 X2 (Windsor)" },
    Entry { vendor: CpuVendor::Amd, family: 15, model: 67,  stepping: None, name: "Athlon 64 X2 (Brisbane)" },
    Entry { vendor: CpuVendor::Amd, family: 15, model: 105, stepping: None, name: "Athlon 64 X2 (Windsor)" },
    Entry { vendor: CpuVendor::Amd, family: 15, model: 107, stepping: None, name: "Sempron Dual (Sherman)" },
    Entry { vendor: CpuVendor::Amd, family: 15, model: 36,  stepping: None, name: "Turion 64 (Lancaster)" },
    Entry { vendor: CpuVendor::Amd, family: 15, model: 72,  stepping: None, name: "Turion X2 (Tyler)" },
    Entry { vendor: CpuVendor::Amd, family: 15, model: 127, stepping: None, name: "Sempron (Sparta)" },

    // ---------- K10 (ext family 16) ----------
    Entry { vendor: CpuVendor::Amd, family: 16, model: 2,  stepping: None, name: "Phenom" },
    Entry { vendor: CpuVendor::Amd, family: 16, model: 4,  stepping: None, name: "Phenom II" },
    Entry { vendor: CpuVendor::Amd, family: 16, model: 5,  stepping: None, name: "Athlon II X4 (Propus)" },
    Entry { vendor: CpuVendor::Amd, family: 16, model: 6,  stepping: None, name: "Athlon II X2 (Regor)" },
    Entry { vendor: CpuVendor::Amd, family: 16, model: 10, stepping: None, name: "Phenom II X6 (Thuban)" },

    // ---------- Llano / Bobcat / Jaguar ----------
    Entry { vendor: CpuVendor::Amd, family: 18, model: 1,   stepping: None, name: "Llano (Fusion)" },
    Entry { vendor: CpuVendor::Amd, family: 20, model: 1,   stepping: None, name: "Brazos (Ontario/Zacate)" },
    Entry { vendor: CpuVendor::Amd, family: 20, model: 2,   stepping: None, name: "Brazos (Dual-core)" },
    Entry { vendor: CpuVendor::Amd, family: 22, model: 0,   stepping: None, name: "Kabini (Jaguar)" },
    Entry { vendor: CpuVendor::Amd, family: 22, model: 48,  stepping: None, name: "Mullins (Puma)" },

    // ---------- Bulldozer 家族 (ext family 21) ----------
    Entry { vendor: CpuVendor::Amd, family: 21, model: 0,  stepping: None, name: "Bulldozer" },
    Entry { vendor: CpuVendor::Amd, family: 21, model: 1,  stepping: None, name: "Bulldozer" },
    Entry { vendor: CpuVendor::Amd, family: 21, model: 2,  stepping: None, name: "Vishera (Piledriver)" },
    Entry { vendor: CpuVendor::Amd, family: 21, model: 48, stepping: None, name: "Kaveri (Steamroller)" },
    Entry { vendor: CpuVendor::Amd, family: 21, model: 56, stepping: None, name: "Godavari" },
    Entry { vendor: CpuVendor::Amd, family: 21, model: 96, stepping: None, name: "Carrizo (Excavator)" },
    Entry { vendor: CpuVendor::Amd, family: 21, model: 101, stepping: None, name: "Bristol Ridge" },
    Entry { vendor: CpuVendor::Amd, family: 21, model: 112, stepping: None, name: "Stoney Ridge" },

    // ---------- Zen 家族 (ext family 23/24/25/26) ----------
    Entry { vendor: CpuVendor::Amd, family: 23, model: 1,   stepping: None, name: "Zen" },
    Entry { vendor: CpuVendor::Amd, family: 23, model: 8,   stepping: None, name: "Zen+" },
    Entry { vendor: CpuVendor::Amd, family: 23, model: 17,  stepping: None, name: "Zen (Raven Ridge)" },
    Entry { vendor: CpuVendor::Amd, family: 23, model: 24,  stepping: None, name: "Zen+ (Picasso)" },
    Entry { vendor: CpuVendor::Amd, family: 23, model: 32,  stepping: None, name: "Zen (Dali)" },
    Entry { vendor: CpuVendor::Amd, family: 23, model: 49,  stepping: None, name: "Zen 2 (Rome)" },
    Entry { vendor: CpuVendor::Amd, family: 23, model: 96,  stepping: None, name: "Zen 2 (Renoir)" },
    Entry { vendor: CpuVendor::Amd, family: 23, model: 104, stepping: None, name: "Zen 2 (Lucienne)" },
    Entry { vendor: CpuVendor::Amd, family: 23, model: 113, stepping: None, name: "Zen 2 (Matisse)" },
    Entry { vendor: CpuVendor::Amd, family: 23, model: 144, stepping: None, name: "Zen 2 (Van Gogh)" },
    Entry { vendor: CpuVendor::Amd, family: 23, model: 152, stepping: None, name: "Zen 2 (Mero)" },
    Entry { vendor: CpuVendor::Amd, family: 24, model: 0,   stepping: None, name: "Dhyana (Hygon)" },
    Entry { vendor: CpuVendor::Amd, family: 25, model: 1,   stepping: None, name: "Zen 3" },
    Entry { vendor: CpuVendor::Amd, family: 25, model: 8,   stepping: None, name: "Zen 3 (Chagall)" },
    Entry { vendor: CpuVendor::Amd, family: 25, model: 33,  stepping: None, name: "Zen 3 (Vermeer)" },
    Entry { vendor: CpuVendor::Amd, family: 25, model: 64,  stepping: None, name: "Zen 3+ (Rembrandt)" },
    Entry { vendor: CpuVendor::Amd, family: 25, model: 68,  stepping: None, name: "Zen 3+ (Rembrandt-R)" },
    Entry { vendor: CpuVendor::Amd, family: 25, model: 80,  stepping: None, name: "Zen 3 (Cezanne)" },
    Entry { vendor: CpuVendor::Amd, family: 25, model: 16,  stepping: None, name: "Zen 4 (Genoa)" },
    Entry { vendor: CpuVendor::Amd, family: 25, model: 17,  stepping: None, name: "Zen 4c (Bergamo)" },
    Entry { vendor: CpuVendor::Amd, family: 25, model: 97,  stepping: None, name: "Zen 4 (Raphael)" },
    Entry { vendor: CpuVendor::Amd, family: 25, model: 116, stepping: None, name: "Zen 4 (Phoenix)" },
    Entry { vendor: CpuVendor::Amd, family: 25, model: 120, stepping: None, name: "Zen 4 (Phoenix 2)" },
    Entry { vendor: CpuVendor::Amd, family: 25, model: 160, stepping: None, name: "Zen 4c (Dense)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 2,   stepping: None, name: "Zen 5 (Strix Halo)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 8,   stepping: None, name: "Zen 5 (Strix Point)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 16,  stepping: None, name: "Zen 5 (Turin)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 17,  stepping: None, name: "Zen 5c (Turin Dense)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 32,  stepping: None, name: "Zen 5 (Granite Ridge)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 36,  stepping: None, name: "Zen 5 (Granite Ridge)" },

    // ---------- Zen 6（ext family 26，2026 起） ----------
    // 来源：AMD 官方 Technical Information Portal 泄露清单（InstLatX64 2026-08-26 捕获）：
    //   Medusa1（移动+AM5）= Models 80h–87h；Olympic Ridge（桌面 AM5）= Models 88h–8Fh；
    //   Medusa2（移动 FP10）= Models E0h–E3h。Geekbench 实测 `Family 26 Model 128` 交叉确认。
    Entry { vendor: CpuVendor::Amd, family: 26, model: 128, stepping: None, name: "Zen 6 (Medusa Point)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 129, stepping: None, name: "Zen 6 (Medusa Point)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 130, stepping: None, name: "Zen 6 (Medusa Point)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 131, stepping: None, name: "Zen 6 (Medusa Point)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 132, stepping: None, name: "Zen 6 (Medusa Point)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 133, stepping: None, name: "Zen 6 (Medusa Point)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 134, stepping: None, name: "Zen 6 (Medusa Point)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 135, stepping: None, name: "Zen 6 (Medusa Point)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 136, stepping: None, name: "Zen 6 (Olympic Ridge)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 137, stepping: None, name: "Zen 6 (Olympic Ridge)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 138, stepping: None, name: "Zen 6 (Olympic Ridge)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 139, stepping: None, name: "Zen 6 (Olympic Ridge)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 140, stepping: None, name: "Zen 6 (Olympic Ridge)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 141, stepping: None, name: "Zen 6 (Olympic Ridge)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 142, stepping: None, name: "Zen 6 (Olympic Ridge)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 143, stepping: None, name: "Zen 6 (Olympic Ridge)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 224, stepping: None, name: "Zen 6 (Medusa2)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 225, stepping: None, name: "Zen 6 (Medusa2)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 226, stepping: None, name: "Zen 6 (Medusa2)" },
    Entry { vendor: CpuVendor::Amd, family: 26, model: 227, stepping: None, name: "Zen 6 (Medusa2)" },
];

/// 按 (vendor, family, model) 查表返回基础微架构名。
///
/// 纯函数，不依赖任何平台 API —— CI 跑在 ARM 上也能直接单测查表逻辑。
/// 识别不到返回 `None`。
pub fn codename_for(
    vendor: CpuVendor,
    family: u32,
    model: u32,
    stepping: Option<u32>,
) -> Option<&'static str> {
    // 优先精确匹配（含步进），否则回退到不区分步进的同 (family, model) 条目。
    let mut fallback = None;
    for e in TABLE {
        if e.vendor == vendor && e.family == family && e.model == model {
            if e.stepping == stepping {
                return Some(e.name);
            }
            if e.stepping.is_none() {
                fallback = Some(e.name);
            }
        }
    }
    fallback
}

/// 从 CPU 品牌串里推断后缀（Xeon / EPYC / Core i7 / Ryzen 9 等）。
///
/// 仅当能判定时才返回 `Some`，绝不编造。
fn brand_suffix(brand: &str) -> Option<&'static str> {
    if brand.contains("Xeon") {
        return Some("Xeon");
    }
    if brand.contains("EPYC") {
        return Some("EPYC");
    }
    if brand.contains("Ryzen Threadripper") {
        return Some("Threadripper");
    }
    for (needle, tag) in [
        ("Ryzen 9", "Ryzen 9"),
        ("Ryzen 7", "Ryzen 7"),
        ("Ryzen 5", "Ryzen 5"),
        ("Ryzen 3", "Ryzen 3"),
    ] {
        if brand.contains(needle) {
            return Some(tag);
        }
    }
    if brand.contains("Ryzen") {
        return Some("Ryzen");
    }
    for (needle, tag) in [
        ("Core(TM) i9", "Core i9"),
        ("Core(TM) i7", "Core i7"),
        ("Core(TM) i5", "Core i5"),
        ("Core(TM) i3", "Core i3"),
        ("Core i9", "Core i9"),
        ("Core i7", "Core i7"),
        ("Core i5", "Core i5"),
        ("Core i3", "Core i3"),
    ] {
        if brand.contains(needle) {
            return Some(tag);
        }
    }
    for (needle, tag) in [
        ("Core m7", "Core m7"),
        ("Core m5", "Core m5"),
        ("Core m3", "Core m3"),
    ] {
        if brand.contains(needle) {
            return Some(tag);
        }
    }
    if brand.contains("Celeron") {
        return Some("Celeron");
    }
    if brand.contains("Pentium") {
        return Some("Pentium");
    }
    None
}

/// 读取 leaf 0 的厂商字符串。
#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
fn read_vendor_string() -> Option<String> {
    let r = __cpuid(0);
    let mut bytes = Vec::with_capacity(12);
    bytes.extend_from_slice(&r.ebx.to_le_bytes());
    bytes.extend_from_slice(&r.edx.to_le_bytes());
    bytes.extend_from_slice(&r.ecx.to_le_bytes());
    match String::from_utf8(bytes) {
        Ok(s) if !s.is_empty() => Some(s),
        _ => None,
    }
}

/// 读取 leaf 0x80000002..0x80000004 拼接而成的品牌串（无则 `None`）。
#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
fn read_brand_string() -> Option<String> {
    // 先确认扩展 leaf 最高支持到 0x80000004。
    let max = __cpuid(0x80000000).eax;
    if max < 0x80000004 {
        return None;
    }
    let mut bytes = Vec::with_capacity(48);
    for leaf in [0x80000002u32, 0x80000003, 0x80000004] {
        let r = __cpuid(leaf);
        bytes.extend_from_slice(&r.eax.to_le_bytes());
        bytes.extend_from_slice(&r.ebx.to_le_bytes());
        bytes.extend_from_slice(&r.ecx.to_le_bytes());
        bytes.extend_from_slice(&r.edx.to_le_bytes());
    }
    let raw = String::from_utf8_lossy(&bytes);
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// 解析 CPUID leaf 1 的 (vendor, family, model, stepping)。
///
/// 公式与 Intel SDM Vol.2A / libcpuid 一致：
/// - `family = base_family == 0xF ? base_family + extended_family : base_family`
///
///   （Intel 与 AMD 通用；AMD 的 15/16/21/23/25/26 系 `base_family` 恒为 0xF，
///   靠 `extended_family` 区分，因此这条规则对两家都成立）
///
/// - `model  = (base_family == 0x6 || base_family == 0xF) ? (extended_model << 4 | base_model) : base_model`
///
///   （仅 family 6 / 15 才把扩展 model 位纳入，其它 family 只用 base model ——
///   对齐 fuchsia `CpuidVersionInfo::model()` 与 libcpuid 行为）
#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
fn cpuid_family_model_stepping() -> Option<(CpuVendor, u32, u32, u32)> {
    let vendor_str = read_vendor_string()?;
    let vendor = if vendor_str == "GenuineIntel" {
        CpuVendor::Intel
    } else if vendor_str == "AuthenticAMD" {
        CpuVendor::Amd
    } else {
        CpuVendor::Unknown
    };
    let eax = __cpuid(1).eax;
    let base_family = (eax >> 8) & 0xF;
    let base_model = (eax >> 4) & 0xF;
    let xmodel = (eax >> 16) & 0xF;
    let xfamily = (eax >> 20) & 0xFF;
    let stepping = eax & 0xF;
    let family = if base_family == 0xF {
        base_family + xfamily
    } else {
        base_family
    };
    let model = if base_family == 0x6 || base_family == 0xF {
        (xmodel << 4) | base_model
    } else {
        base_model
    };
    Some((vendor, family, model, stepping))
}

/// 检测 Intel 混合架构（12 代起的 P-core + E-core）。
///
/// 依据 Intel® Alder Lake Developer Guide：`CPUID.07H:EDX[15]` 为 Hybrid 标志位。
/// 返回 `Some(true)` 表示混合架构；`Some(false)` 表示非混合或 leaf 7 不可用；
/// `None` 仅在无法读取厂商时返回（正常不会）。
#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
fn intel_is_hybrid() -> Option<bool> {
    // 先确认支持 leaf 7（CPUID.0:EAX >= 7）。
    let max = __cpuid(0).eax;
    if max < 7 {
        return Some(false);
    }
    // CPUID.07H.0H:EDX[15] = Hybrid（Intel）。该位在 AMD 上为保留位，
    // 因此调用方仅在 Intel 时采用此值。
    let r = __cpuid(7);
    Some((r.edx & (1u32 << 15)) != 0)
}

/// 返回本机 CPU 的微架构代号（如 `"Broadwell-E (Xeon)"` / `"Zen 3 (Ryzen 7, hybrid)"`）。
///
/// 识别不出来返回 `None`。在非 x86_64 或非 Windows 环境下安全返回 `None`，
/// 不会 panic（CPUID 调用被 `#[cfg]` 隔离，跨平台单测亦可编译）。
pub fn cpu_codename() -> Option<String> {
    #[cfg(all(target_arch = "x86_64", target_os = "windows"))]
    {
        let (vendor, family, model, stepping) = cpuid_family_model_stepping()?;
        let brand = read_brand_string();
        // 基础代号（优先精确步进匹配，回退不区分步进的同型号条目）。
        let base = codename_for(vendor, family, model, Some(stepping))?;
        let mut notes: Vec<&str> = Vec::new();
        if let Some(suffix) = brand.as_deref().and_then(brand_suffix) {
            notes.push(suffix);
        }
        // Intel 12 代起的混合架构：标注 hybrid，便于识别 P-core/E-core 机型。
        if vendor == CpuVendor::Intel && intel_is_hybrid() == Some(true) {
            notes.push("hybrid");
        }
        if notes.is_empty() {
            Some(base.to_string())
        } else {
            Some(format!("{} ({})", base, notes.join(", ")))
        }
    }
    #[cfg(not(all(target_arch = "x86_64", target_os = "windows")))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_intel_broadwell_e() {
        // 本机基准答案对应的查表项。
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 79, None),
            Some("Broadwell-E")
        );
    }

    #[test]
    fn lookup_intel_modern() {
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 42, None),
            Some("Sandy Bridge")
        );
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 58, None),
            Some("Ivy Bridge")
        );
        assert_eq!(codename_for(CpuVendor::Intel, 6, 60, None), Some("Haswell"));
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 63, None),
            Some("Haswell-E")
        );
        assert_eq!(codename_for(CpuVendor::Intel, 6, 94, None), Some("Skylake"));
        // Kaby/Coffee Lake 用步进区分。
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 158, Some(9)),
            Some("Kaby Lake")
        );
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 158, Some(12)),
            Some("Coffee Lake")
        );
        assert_eq!(
            codename_for(CpuVendor::Intel, 15, 0, None),
            Some("Pentium 4 (Willamette)")
        );
    }

    /// 现代 Intel 微架构对照（与 Intel SDM Vol.4 Table 2-1 一致）。
    #[test]
    fn lookup_intel_modern_microarchitectures() {
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 125, None),
            Some("Ice Lake")
        );
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 106, None),
            Some("Ice Lake-SP")
        );
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 140, None),
            Some("Tiger Lake")
        );
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 167, None),
            Some("Rocket Lake")
        );
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 151, None),
            Some("Alder Lake")
        );
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 183, None),
            Some("Raptor Lake")
        );
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 191, None),
            Some("Raptor Lake-S")
        );
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 170, None),
            Some("Meteor Lake")
        );
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 189, None),
            Some("Lunar Lake")
        );
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 197, None),
            Some("Arrow Lake")
        );
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 143, None),
            Some("Sapphire Rapids")
        );
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 207, None),
            Some("Emerald Rapids")
        );
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 173, None),
            Some("Granite Rapids")
        );
    }

    #[test]
    fn lookup_amd() {
        assert_eq!(codename_for(CpuVendor::Amd, 16, 4, None), Some("Phenom II"));
        assert_eq!(codename_for(CpuVendor::Amd, 21, 1, None), Some("Bulldozer"));
        assert_eq!(codename_for(CpuVendor::Amd, 23, 1, None), Some("Zen"));
        assert_eq!(
            codename_for(CpuVendor::Amd, 23, 113, None),
            Some("Zen 2 (Matisse)")
        );
        // libcpuid 当时还没收录的 Zen 3。
        assert_eq!(
            codename_for(CpuVendor::Amd, 25, 33, None),
            Some("Zen 3 (Vermeer)")
        );
    }

    /// 现代 AMD 微架构对照（Zen 2~Zen 5，来源 ZenStates-Core / wikichip / siomon）。
    #[test]
    fn lookup_amd_modern_microarchitectures() {
        // Zen 2 移动/APU
        assert_eq!(
            codename_for(CpuVendor::Amd, 23, 96, None),
            Some("Zen 2 (Renoir)")
        );
        assert_eq!(
            codename_for(CpuVendor::Amd, 23, 104, None),
            Some("Zen 2 (Lucienne)")
        );
        // Zen 3+ 移动
        assert_eq!(
            codename_for(CpuVendor::Amd, 25, 64, None),
            Some("Zen 3+ (Rembrandt)")
        );
        assert_eq!(
            codename_for(CpuVendor::Amd, 25, 80, None),
            Some("Zen 3 (Cezanne)")
        );
        // Zen 4 桌面 / 服务器 / 移动（Ryzen 7000/9000 AM5 最常见）
        assert_eq!(
            codename_for(CpuVendor::Amd, 25, 16, None),
            Some("Zen 4 (Genoa)")
        );
        assert_eq!(
            codename_for(CpuVendor::Amd, 25, 97, None),
            Some("Zen 4 (Raphael)")
        );
        assert_eq!(
            codename_for(CpuVendor::Amd, 25, 116, None),
            Some("Zen 4 (Phoenix)")
        );
        // Zen 5 桌面 / 服务器（Ryzen 9000 / EPYC 9005）
        assert_eq!(
            codename_for(CpuVendor::Amd, 26, 32, None),
            Some("Zen 5 (Granite Ridge)")
        );
        assert_eq!(
            codename_for(CpuVendor::Amd, 26, 16, None),
            Some("Zen 5 (Turin)")
        );
        assert_eq!(
            codename_for(CpuVendor::Amd, 26, 8, None),
            Some("Zen 5 (Strix Point)")
        );
    }

    /// 2025–2026 Intel 新品（来源：Linux intel-family.h master，逐条核对）。
    #[test]
    fn lookup_intel_2025_2026_microarchitectures() {
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 181, None),
            Some("Arrow Lake-U")
        );
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 204, None),
            Some("Panther Lake")
        );
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 213, None),
            Some("Wildcat Lake")
        );
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 215, None),
            Some("Bartlett Lake")
        );
        assert_eq!(
            codename_for(CpuVendor::Intel, 6, 229, None),
            Some("Panther Lake-R")
        );
        // Nova Lake：首款离开 Family 6 → Family 18。
        assert_eq!(
            codename_for(CpuVendor::Intel, 18, 1, None),
            Some("Nova Lake")
        );
        assert_eq!(
            codename_for(CpuVendor::Intel, 18, 3, None),
            Some("Nova Lake-L")
        );
        // Diamond Rapids：服务器 Family 19。
        assert_eq!(
            codename_for(CpuVendor::Intel, 19, 1, None),
            Some("Diamond Rapids")
        );
    }

    /// 2026 AMD Zen 6（来源：AMD 门户泄露清单 + Geekbench 实测 Family 26 Model 128）。
    #[test]
    fn lookup_amd_zen6_microarchitectures() {
        assert_eq!(
            codename_for(CpuVendor::Amd, 26, 128, None),
            Some("Zen 6 (Medusa Point)")
        );
        assert_eq!(
            codename_for(CpuVendor::Amd, 26, 135, None),
            Some("Zen 6 (Medusa Point)")
        );
        assert_eq!(
            codename_for(CpuVendor::Amd, 26, 136, None),
            Some("Zen 6 (Olympic Ridge)")
        );
        assert_eq!(
            codename_for(CpuVendor::Amd, 26, 143, None),
            Some("Zen 6 (Olympic Ridge)")
        );
        assert_eq!(
            codename_for(CpuVendor::Amd, 26, 224, None),
            Some("Zen 6 (Medusa2)")
        );
        assert_eq!(
            codename_for(CpuVendor::Amd, 26, 227, None),
            Some("Zen 6 (Medusa2)")
        );
    }

    #[test]
    fn unknown_returns_none() {
        assert_eq!(codename_for(CpuVendor::Unknown, 99, 99, None), None);
        assert_eq!(codename_for(CpuVendor::Intel, 0, 0, None), None);
    }

    #[test]
    fn real_machine() {
        let name = cpu_codename();
        eprintln!("codename = {:?}", name);
        assert!(name.is_some(), "本机应至少识别出一个代号");
    }
}
