// ============================================================
// hardware.cpp - 硬件信息采集实现（WinSuiteX）
//
// 核心目标：**驱动未安装时也能准确识别硬件**。
// 网吧/电竞酒店的无盘机器、刚装完系统还没打驱动的机器，用 WMI 或
// DXGI 看到的全是 "Standard VGA" / "PCI Device" 这类占位名。
// 本实现绕开驱动层，直接从以下来源读取：
//   - SMBIOS 固件表（GetSystemFirmwareTable 'RSMB'）：BIOS/主板/CPU/内存条
//   - CPUID 指令：CPU 品牌串与厂商（硬件寄存器，绝对可靠）
//   - SetupAPI 设备枚举树：设备节点存在即可读描述，不要求驱动已加载
//   - 注册表：OS 版本、安装日期
// 任一项失败都退回"未知"，绝不崩溃、绝不阻塞。
// ============================================================
#define _WIN32_WINNT 0x0601
#include "hardware.h"
#include <strsafe.h>
#include <iphlpapi.h>     // GetAdaptersInfo
#include <setupapi.h>     // SetupDi*
#include <winioctl.h>     // IOCTL_STORAGE_QUERY_PROPERTY / IOCTL_DISK_GET_LENGTH_INFO
#include <cpuid.h>        // __cpuid / __get_cpuid
// v1.2.5：CPU 微架构代号识别（libcpuid，静态源码随项目编译，零运行时依赖）
#include "libcpuid.h"
// v1.2.6：GPU PCI VEN/DEV -> 名称映射表（2223 条，自动生成自 pci.ids）
#include "thirdparty/pci_ids/gpu_pci_table.h"
#include <stdlib.h>
#include <time.h>
#include <math.h>          // sqrt（显示器尺寸勾股）
// v1.0.12：默认音频设备采集（IMMDeviceEnumerator COM API）
#include <mmdeviceapi.h>
#include <functiondiscoverykeys_devpkey.h>
// v1.1.8：显卡显存（DXGI）+ 显示器物理尺寸（SetupAPI DEVPKEY_Monitor_PhysicalSize）
#include <dxgi.h>
#include <devpkey.h>
#include <devpropdef.h>

#ifndef VER_NT_SERVER
#define VER_NT_SERVER 3
#endif

// ---- 常用中文（\uXXXX 转义，规避 MinGW 宽字符/代码页差异） ----
#define S_UNKNOWN  L"\u672a\u77e5"                     // 未知
#define S_EMPTY    L"\u7a7a\u69fd"                     // 空槽
#define S_OS       L"\u64cd\u4f5c\u7cfb\u7edf"         // 操作系统
#define S_MACHINE  L"\u8ba1\u7b97\u673a"               // 计算机
#define S_BOARD    L"\u4e3b\u677f"                     // 主板
#define S_CPU      L"\u5904\u7406\u5668"               // 处理器
#define S_MEM      L"\u5185\u5b58"                     // 内存
#define S_GPU      L"\u663e\u5361"                     // 显卡
#define S_DISP     L"\u663e\u793a\u5668"               // 显示器
#define S_NIC      L"\u7f51\u5361"                     // 网卡
#define S_AUDIO    L"\u97f3\u9891\u8bbe\u5907"         // 音频设备
#define S_USB      L"\u0055\u0053\u0042\u0020\u5916\u8bbe"  // USB 外设
#define S_VENDOR   L"\u5236\u9020\u5546"               // 制造商
#define S_PRODUCT  L"\u4ea7\u54c1"                     // 产品
#define S_VERSION  L"\u7248\u672c"                     // 版本
#define S_MODEL    L"\u578b\u53f7"                     // 型号
#define S_CORE     L"\u6838\u5fc3"                     // 核心
#define S_THREAD   L"\u7ebf\u7a0b"                     // 线程
#define S_FREQ     L"\u9891\u7387"                     // 频率
#define S_CODENAME L"\u4ee3\u53f7"                     // 代号（CPU 微架构代号）
#define S_CAP      L"\u5bb9\u91cf"                     // 容量
#define S_SLOT     L"\u63d2\u69fd"                     // 插槽
#define S_ARCH     L"\u67b6\u6784"                     // 架构
#define S_USER     L"\u5f53\u524d\u7528\u6237"         // 当前用户
#define S_RES      L"\u5206\u8fa8\u7387"               // 分辨率
#define S_REFRESH  L"\u5237\u65b0\u7387"               // 刷新率
#define S_DISK     L"\u786c\u76d8"                     // 硬盘
#define S_TOTAL    L"\u603b\u8ba1"                     // 总计
#define S_INSERTED L"\u5df2\u63d2"                     // 已插

// ============================================================
// 通用辅助
// ============================================================
static void PutUnknown(WCHAR* dst, size_t n) { StringCchCopyW(dst, n, S_UNKNOWN); }

// SMBIOS 字符串（ASCII/UTF-8）-> 宽字符，自动 trim 首尾空白；空串写"未知"
static void PutSmbStr(WCHAR* dst, size_t n, const char* src) {
    if (!src) { PutUnknown(dst, n); return; }
    while (*src == ' ' || *src == '\t' || *src == '\r' || *src == '\n') src++;
    if (!*src) { PutUnknown(dst, n); return; }
    if (MultiByteToWideChar(CP_UTF8, 0, src, -1, dst, (int)n) <= 0) { PutUnknown(dst, n); return; }
    size_t len = wcslen(dst);
    while (len > 0 && (dst[len - 1] == L' ' || dst[len - 1] == L'\t')) dst[--len] = 0;
}

static BOOL RegReadStr(HKEY root, const WCHAR* sub, const WCHAR* val, WCHAR* out, DWORD nChars) {
    out[0] = 0;
    HKEY k = NULL;
    LONG r = RegOpenKeyExW(root, sub, 0, KEY_READ, &k);
    if (r != ERROR_SUCCESS) return FALSE;
    DWORD sz = nChars * sizeof(WCHAR), ty = 0;
    r = RegQueryValueExW(k, val, NULL, &ty, (LPBYTE)out, &sz);
    RegCloseKey(k);
    if (r != ERROR_SUCCESS) { out[0] = 0; return FALSE; }
    return TRUE;
}

// ============================================================
// SMBIOS（固件表，不依赖驱动）
// ============================================================
#pragma pack(push, 1)
typedef struct _RawSMBIOS {
    BYTE  Used20CallingMethod;
    BYTE  MajorVersion;
    BYTE  MinorVersion;
    BYTE  DmiRevision;
    DWORD Length;
    BYTE  TableData[1];
} RawSMBIOS;
#pragma pack(pop)

// 取结构中第 idx 个字符串（1-based，0 表示无）
// end：SMBIOS 表的结束边界（来自 GetSystemFirmwareTable 分配的 buffer + need），
//      防止截断的表让 strlen/MultiByteToWideChar 越界读到堆外
static const char* SmbStr(const BYTE* rec, BYTE idx, const BYTE* end) {
    if (!idx) return NULL;
    const char* s = (const char*)(rec + rec[1]);
    if ((const BYTE*)s >= end) return NULL;
    for (BYTE i = 1; i < idx; i++) {
        if (!*s) return NULL;
        // 跳到下一个 C 串；同时校验还在 buffer 内
        while ((const BYTE*)s < end && *s) s++;
        if ((const BYTE*)s >= end) return NULL;
        s++;   // 跳过 NUL 终止符
    }
    return (*s) ? s : NULL;
}

// v1.1.9：内存类型枚举（SMBIOS Type 17 偏移 0x12 = Memory Type）。
// 枚举值严格按 DMTF SMBIOS DSP0134：0x18=DDR3 / 0x1A=DDR4 / 0x1B=LPDDR …
// 抽成非 static 函数便于 selftest 直接断言映射（避免字节→字符串映射回归成假成功）。
const WCHAR* HwMemTypeToStr(BYTE mt) {
    switch (mt) {
    case 0x12: return L"DDR";            // DDR
    case 0x13: return L"DDR2";           // DDR2
    case 0x14: return L"DDR2 FB-DIMM";   // DDR2 FB-DIMM
    case 0x18: return L"DDR3";           // DDR3
    case 0x19: return L"FBD2";           // FBD2
    case 0x1A: return L"DDR4";           // DDR4
    case 0x1B: return L"LPDDR";          // LPDDR
    case 0x1C: return L"LPDDR2";         // LPDDR2
    case 0x1D: return L"LPDDR3";         // LPDDR3
    case 0x1E: return L"LPDDR4";         // LPDDR4
    case 0x22: return L"DDR5";           // DDR5
    default:   return L"";
    }
}

static void ParseSmbios(HardwareInfo* hw) {
    UINT need = GetSystemFirmwareTable('RSMB', 0, NULL, 0);
    if (!need || need > (16u << 20)) return;                 // 缺表或异常大小直接放弃
    BYTE* buf = (BYTE*)malloc(need);
    if (!buf) return;
    UINT got = GetSystemFirmwareTable('RSMB', 0, buf, need);
    if (got != need || got < sizeof(RawSMBIOS)) { free(buf); return; }

    RawSMBIOS* raw = (RawSMBIOS*)buf;
    BYTE* p   = raw->TableData;
    BYTE* end = buf + got;
    int cpuIdx = 0, memIdx = 0;

    while (p + 4 <= end) {
        BYTE type = p[0];
        BYTE len  = p[1];
        if (len < 4 || p + len > end) break;                 // 结构损坏，停止解析

        switch (type) {
        case 0:   // BIOS Information
            if (len >= 0x12) {
                PutSmbStr(hw->biosVendor,  _countof(hw->biosVendor),  SmbStr(p, p[0x04], end));
                PutSmbStr(hw->biosVersion, _countof(hw->biosVersion), SmbStr(p, p[0x05], end));
                PutSmbStr(hw->biosDate,    _countof(hw->biosDate),    SmbStr(p, p[0x08], end));
            }
            break;
        case 2:   // Base Board Information（主板）
            if (len >= 0x08) {
                PutSmbStr(hw->boardVendor,  _countof(hw->boardVendor),  SmbStr(p, p[0x04], end));
                PutSmbStr(hw->boardProduct, _countof(hw->boardProduct), SmbStr(p, p[0x05], end));
            }
            break;
        case 4:   // Processor Information（每颗物理 CPU 一条，多路服务器有多条）
            if (cpuIdx < HW_MAX_CPU && len >= 0x1A) {
                HwCpu* c = &hw->cpus[cpuIdx];
                PutSmbStr(c->socket, _countof(c->socket), SmbStr(p, p[0x04], end));
                PutSmbStr(c->vendor, _countof(c->vendor), SmbStr(p, p[0x07], end));
                PutSmbStr(c->name,   _countof(c->name),   SmbStr(p, p[0x10], end));
                // 频率：Current Speed(0x16) 优先，回退 Max Speed(0x14)
                c->speedMHz = 0;
                if (len >= 0x18) c->speedMHz = *(WORD*)(p + 0x16);
                if (!c->speedMHz && len >= 0x16) c->speedMHz = *(WORD*)(p + 0x14);
                // 核心/线程：SMBIOS 3.0+ 的 WORD 字段优先（支持 >255 核），回退 2.6 的 BYTE
                // SMBIOS Type 4 偏移：
                //   0x2A = Core Count 2 (WORD)
                //   0x2C = Core Enabled 2 (WORD)
                //   0x2E = Thread Count 2 (WORD) — 注意：要 Thread Count 2 需 len>=0x30
                c->cores = c->threads = 0;
                if (len >= 0x26) {
                    BYTE c1 = p[0x23], e1 = p[0x24], t1 = p[0x25];
                    if (c1 && c1 != 0xFF) c->cores   = c1;
                    if (e1 && e1 != 0xFF) c->cores   = e1;   // 优先 Core Enabled
                    if (t1 && t1 != 0xFF) c->threads = t1;
                }
                if (len >= 0x2C) {
                    WORD c2 = *(WORD*)(p + 0x2A);
                    if (c2 && c2 != 0xFFFF) c->cores = c2;
                }
                if (len >= 0x30) {
                    WORD t2 = *(WORD*)(p + 0x2E);
                    if (t2 && t2 != 0xFFFF) c->threads = t2;
                }
                if (!c->cores   && len >= 0x24) c->cores   = p[0x23];
                if (!c->threads && len >= 0x26) c->threads = p[0x25];
                cpuIdx++;
            }
            break;
        case 17:  // Memory Device（每根内存条一条）
            if (memIdx < HW_MAX_MEM && len >= 0x15) {
                HwMemStick* m = &hw->mem[memIdx];
                WORD size = *(WORD*)(p + 0x0C);
                m->capacityMB = 0;
                m->populated  = FALSE;
                // Size 是 WORD，能表达的最大值是 0x7FFF = 32767 MB。单条超过 32GB
                // 放不下，固件会置 0x7FFF 并把真实容量放进 Extended Size(0x1C, DWORD,
                // 单位 MB)；0xFFFF 同样表示走扩展字段/未知。两者都必须读 Extended Size。
                // 漏判 0x7FFF 不会报错，只是把哨兵 32767 当成真实容量 —— 真机 8×64GB
                // 服务器曾全部显示成 32GB，OS 总计 512GB 与之差一倍才暴露。
                // 判定与 Rust 版 smbios.rs::parse_mem 保持一致（Linux dmi_scan.c 同理）。
                if (size == 0xFFFF || size == 0x7FFF) {
                    // 2.7+ 用 Extended Size（DWORD，单位 MB）表达 >=32GB 的条子
                    if (len >= 0x20) {
                        DWORD ext = *(DWORD*)(p + 0x1C);
                        if (ext && ext != 0xFFFFFFFF) m->capacityMB = ext & 0x7FFFFFFF;
                    }
                } else if (size & 0x8000) {
                    m->capacityMB = (size & 0x7FFF) >> 10;   // 位15=1 表示单位 KB
                } else if (size) {
                    m->capacityMB = size;                    // 位15=0 表示单位 MB
                }
                if (m->capacityMB) m->populated = TRUE;
                // 频率：Configured Speed(0x20) 优先，回退 Speed(0x15)
                m->speedMHz = 0;
                if (len >= 0x22) m->speedMHz = *(WORD*)(p + 0x20);
                if (!m->speedMHz && len >= 0x17) m->speedMHz = *(WORD*)(p + 0x15);
                PutSmbStr(m->locator, _countof(m->locator), SmbStr(p, p[0x10], end));
                if (len >= 0x12) PutSmbStr(m->bank,   _countof(m->bank),   SmbStr(p, p[0x11], end));
                if (len >= 0x18) PutSmbStr(m->vendor, _countof(m->vendor), SmbStr(p, p[0x17], end));
                if (len >= 0x1B) PutSmbStr(m->partNo, _countof(m->partNo), SmbStr(p, p[0x1A], end));
                // v1.1.8：内存类型（SMBIOS Type 17 偏移 0x12 = Memory Type，偏移正确；枚举见 HwMemTypeToStr）
                if (len >= 0x13) {
                    const WCHAR* t = HwMemTypeToStr(p[0x12]);
                    if (t[0]) StringCchCopyW(m->memType, _countof(m->memType), t);
                }
                memIdx++;
            }
            break;
        }
        // 前进：跳过字符串区（以连续两个 0 结束）
        BYTE* s = p + len;
        while (s + 1 < end && !(s[0] == 0 && s[1] == 0)) s++;
        if (s + 2 > end) break;
        p = s + 2;
    }
    hw->cpuCount = cpuIdx;
    hw->memCount = memIdx;
    free(buf);
}

// ============================================================
// CPUID（硬件寄存器，SMBIOS 不可用时的兜底）
// ============================================================
static void CpuIdFill(WCHAR* brandOut, size_t nBrand, WCHAR* vendorOut, size_t nVendor) {
    PutUnknown(brandOut, nBrand);
    PutUnknown(vendorOut, nVendor);
    unsigned int a = 0, b = 0, c = 0, d = 0;
    if (!__get_cpuid(0x80000000u, &a, &b, &c, &d)) return;
    if (a < 0x80000004u) return;                      // 不支持扩展品牌串
    char brand[49] = {0};
    unsigned int r[4];
    for (int i = 0; i < 3; i++) {
        __cpuid_count(0x80000002u + i, 0, r[0], r[1], r[2], r[3]);
        memcpy(brand + i * 16, r, 16);
    }
    const char* s = brand;
    while (*s == ' ') s++;                            // 去掉前导空格
    if (*s) PutSmbStr(brandOut, nBrand, s);
    // 厂商串：leaf 0 的 EBX/EDX/ECX
    if (__get_cpuid(0u, &a, &b, &c, &d)) {
        char v[13] = {0};
        memcpy(v + 0, &b, 4); memcpy(v + 4, &d, 4); memcpy(v + 8, &c, 4);
        PutSmbStr(vendorOut, nVendor, v);
    }
}

// ============================================================
// CPU 微架构代号（libcpuid，v1.2.5）
// ------------------------------------------------------------
// SMBIOS 的 Processor Version 只给到"型号+频率"（如 "Intel(R) Core(TM) i7-3770K"），
// 拿不到微架构代号（Ivy Bridge / Zen 3 / Skylake ...）。libcpuid 用 CPUID 的
// Family/Model/Stepping + 扩展特性位做精确识别，输出 cpu_codename。
// 这里是纯用户态静态代码（随项目编译），无驱动、无运行时依赖；识别失败优雅降级。
// ============================================================
static void CollectCpuCodename(WCHAR* codenameOut, size_t nCodename) {
    codenameOut[0] = 0;                       // 默认空，UI 侧看到空就不显示代号行
    struct cpu_raw_data_t raw;
    struct cpu_id_t id;
    if (cpuid_get_raw_data(&raw) < 0) return; // 读 CPUID 寄存器失败
    if (cpu_identify(&raw, &id) < 0) return;  // 无法识别架构
    if (!id.cpu_codename[0]) return;          // 识别到了但代号为空（过于老旧/未知 CPU）
    // cpu_codename 是 ASCII 架构代号（"Ivy Bridge" / "Zen 3" ...），直接复用 PutSmbStr 转宽字符
    PutSmbStr(codenameOut, nCodename, id.cpu_codename);
    // 防御：PutSmbStr 空串会写"未知"——识别到却不该当"未知"展示，故空值回退成置空
    if (wcscmp(codenameOut, S_UNKNOWN) == 0) codenameOut[0] = 0;
}

// ============================================================
// 显卡 / 显示器（GDI 层，不依赖显卡驱动）
// ============================================================
static BOOL GpuDup(const HardwareInfo* hw, const WCHAR* name) {
    for (int i = 0; i < hw->gpuCount; i++)
        if (wcscmp(hw->gpus[i].name, name) == 0) return TRUE;
    return FALSE;
}

// 前向声明：子串匹配与虚拟网卡过滤函数（定义在下方）
static BOOL ContainsCI(const WCHAR* hay, const WCHAR* needle);
static BOOL IsVirtualNic(const WCHAR* desc);
static BOOL IsGpuPlaceholderName(const WCHAR* name);   // v1.2.6：微软基本显示驱动占位名检测

// v1.1.7：PnP / EISA 显示器厂商码 -> 可读厂商名。
// 硬件 ID 第二段前导字母（如 GSM / SAM / DEL）即 PnP 厂商码，映射到中文/品牌名。
// 注：少数显示器固件会把厂商码填错（如 AOC 屏报 LG 的 GSM），这是硬件 EDID 问题，
// 本表按 PnP 标准码翻译，与 Windows 设备管理器行为一致。
static const struct { const WCHAR* code; const WCHAR* name; } kPnpMonitorVendor[] = {
    { L"GSM", L"LG" },          { L"LGD", L"LG Display" }, { L"ACR", L"\u5b8f\u7801" },
    { L"AAC", L"\u5b8f\u7801" },{ L"API", L"\u5b8f\u7801" },{ L"AOC", L"AOC" },
    { L"ACI", L"\u534e\u7853" },{ L"SAM", L"\u4e09\u661f" },{ L"SEC", L"\u4e09\u661f" },
    { L"DEL", L"\u6234\u5c14" },{ L"HWP", L"\u60e0\u666e" },{ L"HPN", L"\u60e0\u666e" },
    { L"SNY", L"\u7d22\u5c3c" },{ L"PHL", L"\u98de\u5229\u6d66" }, { L"BNQ", L"\u660e\u57fa" },
    { L"VSC", L"\u4f18\u6d3e" },{ L"IVM", L"iiyama" },      { L"EIZ", L"\u827a\u5353" },
    { L"ENC", L"\u827a\u5353" },{ L"NEC", L"NEC" },         { L"CMN", L"\u7fa4\u521b" },
    { L"AUO", L"\u53cb\u8fbe" },{ L"BOE", L"\u4eac\u4e1c\u65b9" }, { L"HSD", L"\u701a\u5b87\u5f69\u6676" },
    { L"TCL", L"TCL" },         { L"SPT", L"\u590f\u666e" },{ L"MEI", L"\u677e\u4e0b" },
    { L"FUS", L"\u5bcc\u58eb\u901a" }, { L"MAG", L"\u7f8e\u683c" }, { L"CPT", L"\u4e2d\u534e\u6620\u7ba1" },
    { L"DGC", L"\u5927\u5b87" },{ L"TAT", L"\u5927\u540c" },{ L"ZCM", L"Zenith" },
    { L"WDE", L"\u897f\u5c4b" },{ L"ONN", L"Onn" },         { L"SHP", L"\u590f\u666e" },
};
static void MapPnpVendor(const WCHAR* code3, WCHAR* out, size_t nOut) {
    out[0] = 0;
    if (!code3 || !code3[0] || nOut == 0) return;
    WCHAR up[4] = {0};
    for (int i = 0; i < 3 && code3[i]; i++)
        up[i] = (WCHAR)towupper((wint_t)code3[i]);
    for (int i = 0; i < (int)(sizeof(kPnpMonitorVendor)/sizeof(kPnpMonitorVendor[0])); i++) {
        if (wcscmp(up, kPnpMonitorVendor[i].code) == 0) {
            StringCchCopyW(out, nOut, kPnpMonitorVendor[i].name);
            return;
        }
    }
}

// v1.1.6：从 mdd.DeviceID 解析显示器精确型号（藏在 PnP 硬件 ID 里，形如
// "MONITOR\GSM32G2WG3\...\0000" 或 "DISPLAY\GSM32G2WG3\..."）。
// EDID 没给具体型号时 Windows 把名字 fallback 成 "Generic PnP Monitor"，
// 真实型号（如 32G2WG3）在硬件 ID 第二段。剥掉厂商前缀（前导字母）后拼到名字后；
// 厂商前缀（前导字母）本身即 PnP 厂商码，翻成可读厂商名填到 vendor。
static void AppendMonitorModelFromDeviceId(WCHAR* name, size_t nChars,
                                           const WCHAR* deviceId, WCHAR* vendor, size_t vChars) {
    if (!name || !deviceId || !deviceId[0] || nChars == 0) return;
    const WCHAR* p = wcschr(deviceId, L'\\');   // 跳过 "MONITOR\" / "DISPLAY\"
    if (!p) return;
    p++;
    const WCHAR* q = wcschr(p, L'\\');            // 取到第二段硬件 ID（厂商前缀+型号）
    WCHAR hwId[64] = {0};
    if (q) {
        size_t len = (size_t)(q - p);
        if (len >= _countof(hwId)) len = _countof(hwId) - 1;
        wcsncpy(hwId, p, len);
        hwId[len] = 0;
    } else {
        StringCchCopyW(hwId, _countof(hwId), p);
    }
    if (!hwId[0]) return;
    // Default_Monitor / Generic_xxx 这类占位 ID 没有真实厂商/型号
    if (wcschr(hwId, L'_')) return;
    // 前 3 字符 = PnP / EISA 厂商码（标准固定 3 字母，如 GSM / SAM / DEL），
    // 其后全部为型号。注意：型号本身可能以字母开头（如 C34J / A0B0），
    // 故不能"剥掉所有前导字母"，必须固定取前 3 字符作厂商码、第 4 字符起作型号。
    WCHAR code3[4] = {0};
    wcsncpy(code3, hwId, 3);
    if (vChars > 0) MapPnpVendor(code3, vendor, vChars);
    const WCHAR* model = hwId + 3;
    if (!*model) return;                          // 只有厂商码、无型号

    // v1.2.1：EDID 里没有具体型号时 Windows 会 fallback 成 "Generic PnP Monitor"；
    // 这种情况下直接用 PnP 厂商码 + 型号替换名字，避免 UI 显示 "Generic PnP Monitor (2769)"。
    if (vendor[0]) {
        static const WCHAR* kGeneric[] = {
            L"Generic PnP Monitor",
            L"Generic Non-PnP Monitor",
            L"\u901a\u7528\u5373\u63d2\u5373\u7528\u76d1\u89c6\u5668",   // 中文系统通用名
            L"\u901a\u7528\u975e\u5373\u63d2\u5373\u7528\u76d1\u89c6\u5668",
        };
        BOOL isGeneric = FALSE;
        for (int i = 0; i < (int)(sizeof(kGeneric) / sizeof(kGeneric[0])); i++) {
            if (ContainsCI(name, kGeneric[i])) { isGeneric = TRUE; break; }
        }
        if (isGeneric) {
            WCHAR newName[96];
            StringCchPrintfW(newName, _countof(newName), L"%ls %ls", vendor, model);
            if (wcslen(newName) < nChars) {
                StringCchCopyW(name, nChars, newName);
                return;
            }
        }
    }

    if (ContainsCI(name, model)) return;         // 名字里已含型号，避免重复
    // 拼接 "(MODEL)"，确保不溢出
    size_t need = wcslen(name) + wcslen(model) + 3; // " (" + model + ")"
    if (need >= nChars) return;
    StringCchCatW(name, nChars, L" (");
    StringCchCatW(name, nChars, model);
    StringCchCatW(name, nChars, L")");
}

// v1.1.8：显示器物理尺寸（SetupAPI DEVPKEY_Monitor_PhysicalSize，X/Y 单位 0.1mm）
// MinGW devpkey.h 未导出此 key，按 Windows SDK 定义自行声明
static const DEVPROPKEY PKEY_MonitorPhysicalSize = {
    { 0x233a9ef3, 0x3981, 0x4a16, { 0x97, 0x40, 0xf8, 0x3e, 0xaa, 0x4e, 0xc4, 0x3d } }, 3   // pid=3（DEVPKEY_Monitor_PhysicalSize）
};

// 按显示器硬件 ID（如 MONITOR\GSM32G2WG3\...\0000）匹配 SetupAPI 设备，读物理尺寸（英寸）
static void GetMonitorSizeByDeviceId(const WCHAR* deviceId, WCHAR* outSize, size_t nOut) {
    outSize[0] = 0;
    if (!deviceId || !deviceId[0]) return;
    // 提取 PnP ID：MONITOR\GSM32G2WG3\...\0000 -> GSM32G2WG3
    const WCHAR* pnp = deviceId;
    if (wcsnicmp(pnp, L"MONITOR\\", 8) == 0) pnp += 8;
    WCHAR pnpId[32] = {0};
    int k = 0;
    for (const WCHAR* p = pnp; *p && *p != L'\\' && k < 31; p++) pnpId[k++] = *p;
    if (!pnpId[0]) return;

    static const GUID GUID_MONITOR = {0x4d36e96e,0xe325,0x11ce,{0xbf,0xc1,0x08,0x00,0x2b,0xe1,0x03,0x18}};
    HDEVINFO h = SetupDiGetClassDevsW(&GUID_MONITOR, NULL, NULL, DIGCF_PRESENT);
    if (h == INVALID_HANDLE_VALUE) return;
    SP_DEVINFO_DATA d = {0};
    d.cbSize = sizeof(d);
    for (DWORD i = 0; SetupDiEnumDeviceInfo(h, i, &d); i++) {
        WCHAR hwId[512] = {0};
        if (!SetupDiGetDeviceRegistryPropertyW(h, &d, SPDRP_HARDWAREID, NULL,
                                               (PBYTE)hwId, sizeof(hwId), NULL)) continue;
        BOOL matched = FALSE;
        WCHAR* cur = hwId;
        while (*cur) {
            if (ContainsCI(cur, pnpId)) { matched = TRUE; break; }
            cur += wcslen(cur) + 1;
        }
        if (!matched) continue;
        DEVPROPTYPE ty = 0;
        INT dim[2] = {0, 0};
        DWORD sz = sizeof(dim);
        if (SetupDiGetDevicePropertyW(h, &d, &PKEY_MonitorPhysicalSize, &ty, (PBYTE)dim, sz, NULL, 0)
            && ty == DEVPROP_TYPE_INT32 && dim[0] > 0 && dim[1] > 0) {
            double inch = sqrt((double)dim[0]*dim[0] + (double)dim[1]*dim[1]) / 10.0 / 25.4;
            if (inch >= 5.0 && inch <= 120.0)
                StringCchPrintfW(outSize, nOut, L"%.1f \u82f1\u5bf8", inch);
        }
        break;
    }
    SetupDiDestroyDeviceInfoList(h);
}

static void CollectGpu(HardwareInfo* hw) {
    DISPLAY_DEVICEW dd = {0};
    dd.cb = sizeof(dd);
    for (DWORD i = 0; EnumDisplayDevicesW(NULL, i, &dd, 0); i++) {
        if (!dd.DeviceString[0]) continue;
        // 显卡（适配器）型号
        if (hw->gpuCount < HW_MAX_GPU && !GpuDup(hw, dd.DeviceString)) {
            StringCchCopyW(hw->gpus[hw->gpuCount].name,
                           _countof(hw->gpus[hw->gpuCount].name), dd.DeviceString);
            hw->gpuCount++;
        }
        // 显示器：只对挂在桌面的适配器枚举 monitor（其余 RDP/镜像重复条目不含真实显示器，
        // 否则会枚举出十几条同名重复显示器行）。DISPLAY_DEVICE_ACTIVE 即"挂在桌面"。
        if (!(dd.StateFlags & DISPLAY_DEVICE_ACTIVE)) continue;
        // 用适配器 DeviceName 再枚举一层，mdd.DeviceString 才是 EDID 里的显示器型号
        // （直接用 dd.DeviceString 拿到的是显卡适配器名，会显示成"NVIDIA GeForce GTX 650"）
        DISPLAY_DEVICEW mdd = {0};
        mdd.cb = sizeof(mdd);
        for (DWORD j = 0; EnumDisplayDevicesW(dd.DeviceName, j, &mdd, 0); j++) {
            if (!(mdd.StateFlags & DISPLAY_DEVICE_ACTIVE)) continue;
            if (hw->displayCount >= HW_MAX_DISPLAY) break;
            HwDisplay* d = &hw->displays[hw->displayCount];
            StringCchCopyW(d->name, _countof(d->name),
                           mdd.DeviceString[0] ? mdd.DeviceString : dd.DeviceString);
            // v1.1.6：硬件 ID 里藏着精确型号（如 32G2WG3），补到名字后；
            // v1.1.7：前导厂商码（如 GSM）翻成可读厂商名填到 vendor
            AppendMonitorModelFromDeviceId(d->name, _countof(d->name), mdd.DeviceID,
                                           d->vendor, _countof(d->vendor));
            GetMonitorSizeByDeviceId(mdd.DeviceID, d->size, _countof(d->size));   // v1.1.8：显示器尺寸
            DEVMODEW dm = {0};
            dm.dmSize = sizeof(dm);
            if (EnumDisplaySettingsW(mdd.DeviceName, ENUM_CURRENT_SETTINGS, &dm) ||
                EnumDisplaySettingsW(dd.DeviceName, ENUM_CURRENT_SETTINGS, &dm)) {
                d->width     = dm.dmPelsWidth;
                d->height    = dm.dmPelsHeight;
                d->refreshHz = dm.dmDisplayFrequency;
            } else {
                d->width = d->height = d->refreshHz = 0;
            }
            hw->displayCount++;
        }
    }
}

// ============================================================
// 网卡（GetAdaptersInfo：多网卡逐个取型号/MAC/IPv4）
// ============================================================
static void CollectNic(HardwareInfo* hw) {
    ULONG sz = sizeof(IP_ADAPTER_INFO) * 16;
    IP_ADAPTER_INFO* buf = (IP_ADAPTER_INFO*)malloc(sz);
    if (!buf) return;
    DWORD r = GetAdaptersInfo(buf, &sz);
    if (r == ERROR_BUFFER_OVERFLOW) {
        free(buf);
        buf = (IP_ADAPTER_INFO*)malloc(sz);
        if (!buf) return;
        r = GetAdaptersInfo(buf, &sz);
    }
    if (r == NO_ERROR) {
        for (IP_ADAPTER_INFO* a = buf; a && hw->nicCount < HW_MAX_NIC; a = a->Next) {
            // 先转成宽字符描述，过滤 WAN Miniport 等虚拟适配器
            WCHAR tmpDesc[128] = {0};
            if (MultiByteToWideChar(CP_ACP, 0, a->Description, -1,
                                    tmpDesc, (int)_countof(tmpDesc)) <= 0)
                continue;
            if (IsVirtualNic(tmpDesc)) continue;   // 跳过虚拟网卡，不计入统计
            HwNic* n = &hw->nics[hw->nicCount];
            StringCchCopyW(n->desc, _countof(n->desc), tmpDesc);
            if (a->AddressLength >= 6)
                StringCchPrintfW(n->mac, _countof(n->mac), L"%02X-%02X-%02X-%02X-%02X-%02X",
                                 a->Address[0], a->Address[1], a->Address[2],
                                 a->Address[3], a->Address[4], a->Address[5]);
            else
                PutUnknown(n->mac, _countof(n->mac));
            n->ipv4[0] = 0;
            n->up = FALSE;
            for (IP_ADDR_STRING* ip = &a->IpAddressList; ip; ip = ip->Next) {
                const char* s = ip->IpAddress.String;
                if (s[0] && strcmp(s, "0.0.0.0") != 0) {
                    WCHAR t[32] = {0};
                    MultiByteToWideChar(CP_ACP, 0, s, -1, t, (int)_countof(t));
                    if (n->ipv4[0]) StringCchCatW(n->ipv4, _countof(n->ipv4), L", ");
                    StringCchCatW(n->ipv4, _countof(n->ipv4), t);
                    n->up = TRUE;
                }
            }
            if (!n->ipv4[0]) StringCchCopyW(n->ipv4, _countof(n->ipv4), L"-");
            StringCchCopyW(n->ipv6, _countof(n->ipv6), L"-");
            hw->nicCount++;
        }
    }
    free(buf);
}

// ============================================================
// 音频设备 + USB 外设（SetupAPI 设备枚举树，不要求驱动已加载）
// 只保留有信息价值的外设：U 盘/移动硬盘、键鼠、摄像头；
// 过滤掉 USB 根集线器与主机控制器（实测占 16 项，全是噪声）
// ============================================================
static BOOL GetDevProp(HDEVINFO h, SP_DEVINFO_DATA* d, DWORD prop, WCHAR* out, DWORD nChars) {
    out[0] = 0;
    if (!SetupDiGetDeviceRegistryPropertyW(h, d, prop, NULL,
                                           (PBYTE)out, nChars * sizeof(WCHAR), NULL)) {
        out[0] = 0;
        return FALSE;
    }
    return TRUE;
}

// ============================================================
// v1.2.1：处理器硬件核/线程数
// 优先使用操作系统当前激活的处理器拓扑（与任务管理器一致），解决 VM 环境下
// CPUID.0xB 暴露宿主拓扑而非 Guest 实际可用核数的问题（如 i5-3570 被虚报成 8/16）。
// 当 OS API 不可用时回退到 CPUID.0xB / 0x4 / 0x1。
// ============================================================
static void CpuidGet(int level, int sub, int out[4]) {
    // MinGW 的 cpuid.h 提供 GCC 宏（非 MSVC 数组形式）：__cpuid(level,a,b,c,d) / __cpuid_count(level,sub,a,b,c,d)
    int a, b, c, d;
    if (sub) __cpuid_count(level, sub, a, b, c, d);
    else     __cpuid(level, a, b, c, d);
    out[0] = a; out[1] = b; out[2] = c; out[3] = d;
}

// 用操作系统真实拓扑数物理核 / 逻辑线程。在 VM 或旧平台下这比 CPUID 更可靠。
static void GetProcessorTopologyFromOS(int* outCores, int* outThreads) {
    *outCores = 0; *outThreads = 0;
    DWORD len = 0;
    GetLogicalProcessorInformationEx(RelationProcessorCore, NULL, &len);
    if (len > 0) {
        SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX* buf =
            (SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX*)malloc(len);
        if (buf) {
            if (GetLogicalProcessorInformationEx(RelationProcessorCore, buf, &len)) {
                BYTE* p = (BYTE*)buf;
                BYTE* end = p + len;
                int cores = 0, logical = 0;
                while (p < end) {
                    SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX* cur =
                        (SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX*)p;
                    if (cur->Relationship == RelationProcessorCore) {
                        cores++;
                        for (WORD g = 0; g < cur->Processor.GroupCount; g++) {
                            KAFFINITY mask = cur->Processor.GroupMask[g].Mask;
                            while (mask) {
                                if (mask & 1) logical++;
                                mask >>= 1;
                            }
                        }
                    }
                    p += cur->Size;
                }
                if (cores > 0 && logical >= cores) {
                    *outCores = cores;
                    *outThreads = logical;
                }
            }
            free(buf);
        }
    }
    // 兜底：用 GetSystemInfo / GetNativeSystemInfo 拿逻辑处理器数
    if (*outThreads == 0) {
        SYSTEM_INFO si = {0};
        GetNativeSystemInfo(&si);
        *outThreads = (int)si.dwNumberOfProcessors;
    }
    if (*outCores == 0) {
        *outCores = *outThreads;   // 拿不到物理核时保守假设每核 1 线程
    }
}

static void CollectMaxProcessor(HardwareInfo* hw) {
    int regs[4];
    CpuidGet(0, 0, regs);
    int maxLeaf = regs[0];

    // 优先用操作系统真实拓扑（与 Windows 任务管理器一致）。
    int hwCores = 0, hwThreads = 0;
    int osCores = 0, osThreads = 0;
    GetProcessorTopologyFromOS(&osCores, &osThreads);
    if (osCores > 0 && osThreads >= osCores) {
        hwCores = osCores;
        hwThreads = osThreads;
    }

    // OS 不可用时回退到 CPUID 推导。
    if (hwCores == 0) {
        int threadsPerCore = 0;
        int logicalPerPkg = 0;
        // 方法 1：CPUID.0xB Extended Topology
        if (maxLeaf >= 0x0B) {
            for (int level = 0; level < 4; level++) {
                CpuidGet(0x0B, level, regs);
                int levelType = (regs[2] >> 8) & 0xFF;
                int count = regs[1] & 0xFFFF;
                if (levelType == 1) threadsPerCore = count;
                else if (levelType == 2) logicalPerPkg = count;
                if (regs[1] == 0) break;
            }
        }
        int hwSmt = 0;
        if (logicalPerPkg > 0) {
            if (threadsPerCore > 0) {
                hwCores = logicalPerPkg / threadsPerCore;
                hwSmt = threadsPerCore;
            } else {
                hwCores = logicalPerPkg;
                hwSmt = 1;
            }
        }
        // 方法 2：CPUID.0x4
        if (hwCores == 0 && maxLeaf >= 4) {
            for (int i = 0; ; i++) {
                CpuidGet(4, i, regs);
                int type = regs[0] & 0x1F;
                if (type == 0) break;
                int c = ((regs[0] >> 26) & 0x3F) + 1;
                if (c > hwCores) hwCores = c;
            }
        }
        // 方法 3：CPUID.0x1
        if (hwSmt == 0 && maxLeaf >= 1) {
            CpuidGet(1, 0, regs);
            int logical = (regs[1] >> 16) & 0xFF;
            hwSmt = hwCores ? (logical / hwCores > 0 ? logical / hwCores : 1) : 1;
        }
        if (hwCores == 0) hwCores = (hw->cpuCount > 0) ? hw->cpus[0].cores : 1;
        if (hwCores == 0) hwCores = 1;
        if (hwSmt < 1) hwSmt = 1;
        hwThreads = hwCores * hwSmt;
    }

    if (hwCores == 0) hwCores = 1;
    if (hwThreads < hwCores) hwThreads = hwCores;
    for (int i = 0; i < hw->cpuCount && i < HW_MAX_CPU; i++) {
        hw->cpus[i].maxCores = hwCores;
        hw->cpus[i].maxThreads = hwThreads;
    }
}

// ============================================================
// v1.1.8：主板芯片组（SetupAPI 枚举 PCI 主机桥，读 FriendlyName）
// 注：SMBIOS Type 2 不含芯片组名，最可靠来源是 PCI 北桥设备的 FriendlyName。
// 若只拿到"PCI 标准主机桥"（通用驱动未写具体型号），则需后续建 DEV→代号映射表才能显示"H61"等。
// ============================================================
static void CollectChipset(HardwareInfo* hw) {
    static const GUID GUID_SYSTEM = {0x4d36e97d,0xe325,0x11ce,{0xbf,0xc1,0x08,0x00,0x2b,0xe1,0x03,0x18}};
    HDEVINFO h = SetupDiGetClassDevsW(&GUID_SYSTEM, NULL, NULL, DIGCF_PRESENT);
    if (h == INVALID_HANDLE_VALUE) return;
    SP_DEVINFO_DATA d = {0};
    d.cbSize = sizeof(d);
    for (DWORD i = 0; SetupDiEnumDeviceInfo(h, i, &d); i++) {
        WCHAR name[128];
        if (!GetDevProp(h, &d, SPDRP_FRIENDLYNAME, name, _countof(name)) || !name[0]) continue;
        // 主机桥（北桥/芯片组）设备名：精确匹配，避免误命中 PCIe 下游桥（裸"Bridge" 会命中 PCI Express PCI Bridge 等）
        if (wcsstr(name, L"主机桥") || wcsstr(name, L"Host Bridge") ||
            wcsstr(name, L"芯片组") || wcsstr(name, L"Chipset")) {
            StringCchCopyW(hw->boardChipset, _countof(hw->boardChipset), name);
            break;
        }
    }
    SetupDiDestroyDeviceInfoList(h);
}

// ============================================================
// v1.1.8：显卡独立显存（DXGI IDXGIAdapter1::GetDesc1 → DedicatedVideoMemory）
// 跳过软件适配器（DXGI_ADAPTER_FLAG_SOFTWARE），只统计真实 GPU。
// ============================================================
static void CollectGpuVram(HardwareInfo* hw) {
    if (hw->gpuCount == 0) return;
    IDXGIFactory1* pFactory = NULL;
    if (FAILED(CreateDXGIFactory1(__uuidof(IDXGIFactory1), (void**)&pFactory))) return;
    for (UINT i = 0; i < 64; i++) {
        IDXGIAdapter1* pAdapter = NULL;
        if (FAILED(pFactory->EnumAdapters1(i, &pAdapter))) break;
        DXGI_ADAPTER_DESC1 desc;
        BOOL real = SUCCEEDED(pAdapter->GetDesc1(&desc)) && !(desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE);
        if (real) {
            // DXGI 适配器顺序未必与 CollectGpu（GDI 枚举）一致，按名称匹配再写入显存，避免错位
            int match = -1;
            for (int j = 0; j < hw->gpuCount; j++) {
                if (hw->gpus[j].name[0] &&
                    (wcsstr(hw->gpus[j].name, desc.Description) || wcsstr(desc.Description, hw->gpus[j].name))) {
                    match = j; break;
                }
            }
            DWORD vram = (DWORD)(desc.DedicatedVideoMemory >> 20);
            if (match >= 0) {
                hw->gpus[match].vramMB = vram;
            } else if (hw->gpuCount < HW_MAX_GPU && !IsGpuPlaceholderName(desc.Description)) {
                // DXGI 找到 GDI 未列的真实 GPU（罕见），追加一笔。
                // v1.2.6：微软基本显示驱动占位名（Basic Render Driver）不追加，
                //         否则无驱动机器会多出一条假显卡与 GDI 占位条目重复。
                int n = hw->gpuCount++;
                StringCchCopyW(hw->gpus[n].name, _countof(hw->gpus[n].name), desc.Description);
                hw->gpus[n].vramMB = vram;
            }
        }
        pAdapter->Release();
    }
    pFactory->Release();
}

// ============================================================
// v1.2.6：未装显卡驱动时的真实型号还原（PCI 硬件 ID 查表）
// ------------------------------------------------------------
// 无驱动时 Windows 加载微软基本显示驱动，GDI/DXGI 只报占位名
// ("Microsoft Basic Display Adapter" / "Standard VGA Graphics Adapter")，
// 真实型号（如 GeForce GTX 1060）在 GDI 层拿不到。
// 但 PCI 设备节点与驱动无关：SPDRP_HARDWAREID 恒有 PCI\VEN_XXXX&DEV_XXXX，
// 用内置 gpu_pci_table.h（pci.ids 精简，2223 条三家 GPU）二分查找还原；
// 未命中表时退到 vendor 级（NVIDIA/AMD/Intel + PCI ID）。
// 已装驱动的机器名称本就是真名，此路径零开销跳过。
// ============================================================
static BOOL IsGpuPlaceholderName(const WCHAR* name) {
    if (!name || !name[0]) return TRUE;
    // 宽松匹配（大小写无关），覆盖中英文驱动描述与未来变体
    if (ContainsCI(name, L"Basic Display") || ContainsCI(name, L"Basic Render") ||
        ContainsCI(name, L"Standard VGA"))
        return TRUE;
    return FALSE;
}

static BOOL ParseHex4(const WCHAR* p, unsigned int* out) {
    unsigned int v = 0;
    for (int i = 0; i < 4; i++) {
        WCHAR c = p[i];
        unsigned int d;
        if (c >= L'0' && c <= L'9')      d = (unsigned int)(c - L'0');
        else if (c >= L'a' && c <= L'f') d = (unsigned int)(c - L'a') + 10;
        else if (c >= L'A' && c <= L'F') d = (unsigned int)(c - L'A') + 10;
        else return FALSE;
        v = v * 16 + d;
    }
    *out = v;
    return TRUE;
}

// "PCI\VEN_10DE&DEV_1C82&SUBSYS_..." -> ven=0x10DE dev=0x1C82
static BOOL ParsePciIds(const WCHAR* deviceId, unsigned short* ven, unsigned short* dev) {
    if (!deviceId || !deviceId[0]) return FALSE;
    const WCHAR* v = wcsstr(deviceId, L"VEN_");
    if (!v) return FALSE;
    unsigned int vv = 0, dd = 0;
    if (!ParseHex4(v + 4, &vv)) return FALSE;
    const WCHAR* dp = wcsstr(v + 4, L"DEV_");
    if (!dp) return FALSE;
    if (!ParseHex4(dp + 4, &dd)) return FALSE;
    *ven = (unsigned short)vv;
    *dev = (unsigned short)dd;
    return TRUE;
}

static const WCHAR* GpuPciVendorName(unsigned short ven) {
    switch (ven) {
    case 0x10DE: return L"NVIDIA";
    case 0x1002: return L"AMD";        // ATI 已并入
    case 0x8086: return L"Intel";
    case 0x102B: return L"Matrox";
    case 0x5333: return L"S3";
    case 0x1039: return L"SiS";
    case 0x1106: return L"VIA";
    case 0x121A: return L"3dfx";
    case 0x1023: return L"Trident";
    case 0x1A03: return L"ASPEED";     // 服务器 BMC 板载显卡
    case 0x1414: return L"Microsoft";
    case 0x15AD: return L"VMware";
    case 0x1AF4: return L"Virtio";
    case 0x1B36: return L"QEMU";
    case 0x1234: return L"Bochs";
    case 0x1013: return L"Cirrus";
    default:     return NULL;
    }
}

// kGpuPciTable 按 (ven, dev) 升序，二分查找
static const char* GpuPciLookup(unsigned short ven, unsigned short dev) {
    int lo = 0, hi = (int)GPU_PCI_TABLE_N - 1;
    while (lo <= hi) {
        int mid = lo + (hi - lo) / 2;
        unsigned short mv = kGpuPciTable[mid].ven;
        unsigned short md = kGpuPciTable[mid].dev;
        if (mv < ven || (mv == ven && md < dev)) lo = mid + 1;
        else if (mv == ven && md == dev)         return kGpuPciTable[mid].name;
        else                                     hi = mid - 1;
    }
    return NULL;
}

// 用 PCI ID 生成显卡名称：命中表 -> "GeForce GTX 1060 6GB"；
// 未命中但有 vendor -> "NVIDIA 显卡 (PCI 10DE:1C82)"；vendor 也未知 -> FALSE 保留占位名
static BOOL GpuFixNameFromPci(unsigned short ven, unsigned short dev, WCHAR* out, size_t n) {
    const char* tbl = GpuPciLookup(ven, dev);
    if (tbl && tbl[0]) {
        if (MultiByteToWideChar(CP_UTF8, 0, tbl, -1, out, (int)n) <= 0) return FALSE;
        return TRUE;
    }
    const WCHAR* vn = GpuPciVendorName(ven);
    if (vn) {
        // "显卡 (PCI xxxx:xxxx)"
        StringCchPrintfW(out, n, L"%ls \u663e\u5361 (PCI %04X:%04X)", vn, ven, dev);
        return TRUE;
    }
    return FALSE;
}

// —— v1.2.6：selftest 单测导出（hardware.h 声明）——
const char* HW_GpuPciLookup(unsigned short ven, unsigned short dev) {
    return GpuPciLookup(ven, dev);
}
BOOL HW_IsGpuPlaceholderName(const WCHAR* name) {
    return IsGpuPlaceholderName(name);
}

static void CollectGpuRealNames(HardwareInfo* hw) {    if (hw->gpuCount == 0) return;
    BOOL hasPlaceholder = FALSE;
    for (int i = 0; i < hw->gpuCount; i++)
        if (IsGpuPlaceholderName(hw->gpus[i].name)) { hasPlaceholder = TRUE; break; }
    if (!hasPlaceholder) return;    // 全部真名（驱动已装）→ 零开销返回

    // --- 第一层：GDI 适配器 DeviceID（枚举顺序与 CollectGpu 一致，同名条目共享修正结果）---
    DISPLAY_DEVICEW dd = {0};
    dd.cb = sizeof(dd);
    for (DWORD i = 0; EnumDisplayDevicesW(NULL, i, &dd, 0); i++) {
        if (!dd.DeviceString[0]) continue;
        for (int j = 0; j < hw->gpuCount; j++) {
            HwGpu* g = &hw->gpus[j];
            if (wcscmp(g->name, dd.DeviceString) != 0) continue;
            if (!IsGpuPlaceholderName(g->name)) continue;
            unsigned short ven = 0, dev = 0;
            if (ParsePciIds(dd.DeviceID, &ven, &dev))
                GpuFixNameFromPci(ven, dev, g->name, _countof(g->name));
        }
    }

    // --- 第二层兜底：GDI DeviceID 拿不到 VEN/DEV 时，用 SetupAPI Display 类硬件 ID ---
    // 对齐策略（对抗审查后重写）：先收集全部 Display 设备的 (ven,dev)，
    // nDev >= 占位条目数时按序修正占位条目；多出的设备说明 CollectGpu 的同名占位
    // 去重把第二块卡丢了（无驱动双卡同名），追加补齐（HW_MAX_GPU 内）。
    // 极端"无驱动多卡异型号"下名字可能互换（序号对应不可知），单卡场景精确。
    BOOL still = FALSE;
    for (int i = 0; i < hw->gpuCount; i++)
        if (IsGpuPlaceholderName(hw->gpus[i].name)) { still = TRUE; break; }
    if (!still) return;
    static const GUID GUID_DISPLAY = {0x4d36e968,0xe325,0x11ce,{0xbf,0xc1,0x08,0x00,0x2b,0xe1,0x03,0x18}};
    HDEVINFO h = SetupDiGetClassDevsW(&GUID_DISPLAY, NULL, NULL, DIGCF_PRESENT);
    if (h == INVALID_HANDLE_VALUE) return;
    unsigned short devVen[32], devDev[32];
    int nDev = 0;
    SP_DEVINFO_DATA d = {0};
    d.cbSize = sizeof(d);
    for (DWORD i = 0; nDev < 32 && SetupDiEnumDeviceInfo(h, i, &d); i++) {
        WCHAR hwId[512] = {0};
        if (!SetupDiGetDeviceRegistryPropertyW(h, &d, SPDRP_HARDWAREID, NULL,
                                               (PBYTE)hwId, sizeof(hwId), NULL)) continue;
        for (WCHAR* cur = hwId; *cur; cur += wcslen(cur) + 1) {
            unsigned short ven = 0, dev = 0;
            if (ParsePciIds(cur, &ven, &dev)) {
                devVen[nDev] = ven;
                devDev[nDev] = dev;
                nDev++;
                break;   // 该设备取第一段含 VEN/DEV 的硬件 ID
            }
        }
    }
    SetupDiDestroyDeviceInfoList(h);
    if (nDev == 0) return;
    // 按序修正现有占位条目（每个占位条目消费一个设备，修正失败=vendor 未知，保留原名）
    int idx = 0;
    for (int j = 0; j < hw->gpuCount && idx < nDev; j++) {
        HwGpu* g = &hw->gpus[j];
        if (!IsGpuPlaceholderName(g->name)) continue;
        GpuFixNameFromPci(devVen[idx], devDev[idx], g->name, _countof(g->name));
        idx++;
    }
    // 多出的设备 = 同名去重丢失的卡，追加补齐
    for (; idx < nDev && hw->gpuCount < HW_MAX_GPU; idx++) {
        HwGpu* g = &hw->gpus[hw->gpuCount];
        ZeroMemory(g, sizeof(*g));
        if (GpuFixNameFromPci(devVen[idx], devDev[idx], g->name, _countof(g->name)))
            hw->gpuCount++;
        // vendor 也未知（野卡）→ 跳过该设备，不追加空名条目
    }
}

// ============================================================
// v1.2.3：NVIDIA GPU 遥测（NVML）
// 通过 LoadLibrary("nvml.dll") 动态加载，随 N 卡驱动自带，无需额外驱动。
// 无 N 卡 / 无驱动 / 加载失败 → 各字段保持 0，UI 走"未识别"降级，不影响核心功能。
// 只补 gpus[] 已有的块，不改 gpuCount；不匹配到就不写。
// ============================================================
typedef int (*PFN_nvmlInit_v2)(void);
typedef int (*PFN_nvmlDeviceGetHandleByIndex_v2)(unsigned int, void**);
typedef int (*PFN_nvmlDeviceGetName)(void*, char*, unsigned int);
typedef int (*PFN_nvmlDeviceGetTemperature)(void*, unsigned int*, unsigned int);
typedef int (*PFN_nvmlDeviceGetFanSpeed)(void*, unsigned int*);
typedef int (*PFN_nvmlDeviceGetPowerUsage)(void*, unsigned int*);   // mW
typedef int (*PFN_nvmlDeviceGetUtilizationRates)(void*, void*);      // nvmlUtilization_t
typedef int (*PFN_nvmlDeviceGetMemoryInfo)(void*, void*);            // nvmlMemory_t
typedef int (*PFN_nvmlDeviceGetClock)(void*, unsigned int, unsigned int*);

// nvmlUtilization_t { unsigned int gpu; unsigned int memory; }
struct NvmlUtil { unsigned int gpu; unsigned int memory; };
// nvmlMemory_t { unsigned long long total; unsigned long long free; unsigned long long used; }
struct NvmlMem { unsigned long long total; unsigned long long free; unsigned long long used; };

static void NvmlLoadFns(PFN_nvmlInit_v2* pInit, PFN_nvmlDeviceGetHandleByIndex_v2* pHdl,
                        PFN_nvmlDeviceGetName* pName, PFN_nvmlDeviceGetTemperature* pTemp,
                        PFN_nvmlDeviceGetFanSpeed* pFan, PFN_nvmlDeviceGetPowerUsage* pPwr,
                        PFN_nvmlDeviceGetUtilizationRates* pUtil, PFN_nvmlDeviceGetMemoryInfo* pMem,
                        PFN_nvmlDeviceGetClock* pClk, HMODULE* hOut) {
    *hOut = LoadLibraryW(L"nvml.dll");
    if (!*hOut) return;
    *pInit = (PFN_nvmlInit_v2)         GetProcAddress(*hOut, "nvmlInit_v2");
    *pHdl  = (PFN_nvmlDeviceGetHandleByIndex_v2) GetProcAddress(*hOut, "nvmlDeviceGetHandleByIndex_v2");
    *pName = (PFN_nvmlDeviceGetName)     GetProcAddress(*hOut, "nvmlDeviceGetName");
    *pTemp = (PFN_nvmlDeviceGetTemperature)    GetProcAddress(*hOut, "nvmlDeviceGetTemperature");
    *pFan  = (PFN_nvmlDeviceGetFanSpeed)    GetProcAddress(*hOut, "nvmlDeviceGetFanSpeed");
    *pPwr  = (PFN_nvmlDeviceGetPowerUsage)  GetProcAddress(*hOut, "nvmlDeviceGetPowerUsage");
    *pUtil = (PFN_nvmlDeviceGetUtilizationRates) GetProcAddress(*hOut, "nvmlDeviceGetUtilizationRates");
    *pMem  = (PFN_nvmlDeviceGetMemoryInfo)  GetProcAddress(*hOut, "nvmlDeviceGetMemoryInfo");
    *pClk  = (PFN_nvmlDeviceGetClock)       GetProcAddress(*hOut, "nvmlDeviceGetClock");
    if (!*pInit || !*pHdl) { FreeLibrary(*hOut); *hOut = NULL; }
}

// NVML 设备时钟域常量
enum { NVML_CLOCK_GRAPHICS = 1 };
// NVML 温度传感器
enum { NVML_TEMPERATURE_GPU = 0 };

static void CollectGpuNvml(HardwareInfo* hw) {
    if (hw->gpuCount == 0) return;
    PFN_nvmlInit_v2 pInit; PFN_nvmlDeviceGetHandleByIndex_v2 pHdl;
    PFN_nvmlDeviceGetName pName; PFN_nvmlDeviceGetTemperature pTemp;
    PFN_nvmlDeviceGetFanSpeed pFan; PFN_nvmlDeviceGetPowerUsage pPwr;
    PFN_nvmlDeviceGetUtilizationRates pUtil; PFN_nvmlDeviceGetMemoryInfo pMem;
    PFN_nvmlDeviceGetClock pClk; HMODULE h = NULL;
    NvmlLoadFns(&pInit, &pHdl, &pName, &pTemp, &pFan, &pPwr, &pUtil, &pMem, &pClk, &h);
    if (!h) return;                       // 无 nvml.dll（非 N 卡 / 未装 N 驱动）→ 全字段 0

    if (pInit() != 0) { FreeLibrary(h); return; }

    for (unsigned idx = 0; idx < 16; idx++) {
        void* dev = NULL;
        if (pHdl(idx, &dev) != 0) break;   // 到末尾（INVALID_ARGUMENT/INSUFFICIENT 等）

        // 拿 NVML 侧名字，与 gpus[] 里某条按子串匹配（大小写无关）
        char cbuf[128] = {0};
        WCHAR wname[128];
        int found = -1;
        if (pName && pName(dev, cbuf, sizeof(cbuf)) == 0) {
            MultiByteToWideChar(CP_UTF8, 0, cbuf, -1, wname, 127);
            wname[127] = 0;
            for (int j = 0; j < hw->gpuCount; j++) {
                if (ContainsCI(wname, hw->gpus[j].name) || ContainsCI(hw->gpus[j].name, wname)) { found = j; break; }
            }
        }
        if (found < 0) continue;          // 匹配不到就跳过，不硬塞
        HwGpu* g = &hw->gpus[found];

        // 各遥测指针独立判空（新版驱动可能缺旧符号，如 nvmlDeviceGetClock），缺失则该项保持 0
        unsigned t = 0;    if (pTemp && pTemp(dev, &t, NVML_TEMPERATURE_GPU) == 0) g->tempC = (int)t;
        unsigned f = 0;     if (pFan  && pFan(dev, &f) == 0)                        g->fanPct = (int)f;
        unsigned mw = 0;    if (pPwr  && pPwr(dev, &mw) == 0)                      g->powerW = (int)(mw / 1000);
        unsigned clk = 0;   if (pClk  && pClk(dev, NVML_CLOCK_GRAPHICS, &clk) == 0) g->clockMHz = clk;

        NvmlUtil u = {0, 0};
        if (pUtil && pUtil(dev, &u) == 0) g->utilPct = (int)u.gpu;

        NvmlMem m = {0, 0, 0};
        if (pMem && pMem(dev, &m) == 0) {
            g->memUsedMB = (int)(m.used >> 20);
            g->memFreeMB = (int)(m.free >> 20);
            if (g->vramMB == 0 && m.total > 0) g->vramMB = (DWORD)(m.total >> 20);
        }
    }
    FreeLibrary(h);
}

// ============================================================
// v1.2.4：AMD GPU 遥测（ADL，atiadlxx.dll / 回退 atiadlxy.dll）
// 动态加载，无 A 卡 / 未装驱动 → 全字段保持 0，走降级。
// 复用 HwGpu 的 7 个遥测字段（与 NVML 同名，语义一致）。
// 优先 ADL2_*（带 context）；仅温度/显存/时钟/利用率，风扇/功耗一般不通过 OD 暴露则保持 0。
// ============================================================
typedef int (*PFN_ADL2_Main_Control_Create)(void*, int, void*);
typedef int (*PFN_ADL2_Main_Control_Destroy)(void*);
typedef int (*PFN_ADL2_Adapter_NumberOfAdapters_Get)(void*, int*);
typedef int (*PFN_ADL2_Overdrive6_Temperature_Get)(void*, int, int*);
typedef int (*PFN_ADL2_OverdriveN_Temperature_Get)(void*, int, int, int*);
typedef int (*PFN_ADL2_Adapter_VRAMUsage_Get)(void*, int, int*);
typedef int (*PFN_ADL2_Adapter_Active_Get)(void*, int, int*);
typedef int (*PFN_ADL_Main_Memory_Alloc)(int);

static void CollectGpuAdl(HardwareInfo* hw) {
    if (hw->gpuCount == 0) return;
    HMODULE h = LoadLibraryW(L"atiadlxx.dll");
    if (!h) h = LoadLibraryW(L"atiadlxy.dll");
    if (!h) return;                                   // 无 A 卡 / 未装 AMD 驱动

    PFN_ADL2_Main_Control_Create pCreate   = (PFN_ADL2_Main_Control_Create)GetProcAddress(h, "ADL2_Main_Control_Create");
    PFN_ADL2_Main_Control_Destroy pDestroy = (PFN_ADL2_Main_Control_Destroy)GetProcAddress(h, "ADL2_Main_Control_Destroy");
    PFN_ADL2_Adapter_NumberOfAdapters_Get pNum = (PFN_ADL2_Adapter_NumberOfAdapters_Get)GetProcAddress(h, "ADL2_Adapter_NumberOfAdapters_Get");
    PFN_ADL2_Overdrive6_Temperature_Get pTemp6 = (PFN_ADL2_Overdrive6_Temperature_Get)GetProcAddress(h, "ADL2_Overdrive6_Temperature_Get");
    PFN_ADL2_OverdriveN_Temperature_Get pTempN = (PFN_ADL2_OverdriveN_Temperature_Get)GetProcAddress(h, "ADL2_OverdriveN_Temperature_Get");
    PFN_ADL2_Adapter_VRAMUsage_Get pVram = (PFN_ADL2_Adapter_VRAMUsage_Get)GetProcAddress(h, "ADL2_Adapter_VRAMUsage_Get");
    PFN_ADL2_Adapter_Active_Get pActive = (PFN_ADL2_Adapter_Active_Get)GetProcAddress(h, "ADL2_Adapter_Active_Get");
    PFN_ADL_Main_Memory_Alloc pAlloc = (PFN_ADL_Main_Memory_Alloc)GetProcAddress(h, "ADL_Main_Memory_Alloc");

    if (!pCreate || !pDestroy || !pNum || !pAlloc) { FreeLibrary(h); return; }

    void* ctx = NULL;
    if (pCreate((void*)pAlloc, 1, &ctx) != 0 || !ctx) { FreeLibrary(h); return; }

    int nAdapters = 0;
    if (pNum(ctx, &nAdapters) == 0) {
        for (int idx = 0; idx < nAdapters && idx < 16; idx++) {
            int active = 1;
            if (pActive && pActive(ctx, idx, &active) == 0 && !active) continue;   // 跳过非活跃适配器
            HwGpu* g = (idx < hw->gpuCount) ? &hw->gpus[idx] : NULL;
            if (!g) continue;

            // 温度：OD6 优先，回退 ODN（温度类型 1=核心）
            int temp = 0;
            if (pTemp6 && pTemp6(ctx, idx, &temp) == 0 && temp > 0)
                g->tempC = temp / 1000;
            else if (pTempN && pTempN(ctx, idx, 1, &temp) == 0 && temp > 0)
                g->tempC = temp / 1000;

            // 显存已用（MB）
            int vramUsed = 0;
            if (pVram && pVram(ctx, idx, &vramUsed) == 0) {
                g->memUsedMB = vramUsed;
                if (g->vramMB == 0 && vramUsed > 0) g->vramMB = 0;   // total 未知，留 DXGI 值
            }
        }
    }
    pDestroy(ctx);
    FreeLibrary(h);
}

static BOOL AudioDup(const HardwareInfo* hw, const WCHAR* name) {
    for (int i = 0; i < hw->audioCount; i++)
        if (wcscmp(hw->audios[i].name, name) == 0) return TRUE;
    return FALSE;
}

static BOOL UsbDup(const HardwareInfo* hw, const WCHAR* name) {
    for (int i = 0; i < hw->usbCount; i++)
        if (wcscmp(hw->usbs[i].name, name) == 0) return TRUE;
    return FALSE;
}

// 大小写无关子串匹配（自建，避免依赖 shlwapi 的 StrStrIW）
static BOOL ContainsCI(const WCHAR* hay, const WCHAR* needle) {
    if (!hay || !needle || !needle[0]) return FALSE;
    size_t nl = wcslen(needle), hl = wcslen(hay);
    if (nl > hl) return FALSE;
    for (size_t i = 0; i + nl <= hl; i++) {
        size_t j = 0;
        for (; j < nl; j++)
            if (towlower((wint_t)hay[i + j]) != towlower((wint_t)needle[j])) break;
        if (j == nl) return TRUE;
    }
    return FALSE;
}

// 虚拟 / 软件 / 总线占位设备：不是"真实识别出的硬件"，一律不列入音频与外设清单
static BOOL IsNotRealHw(const WCHAR* s) {
    if (!s || !s[0]) return TRUE;
    static const WCHAR* kBad[] = {
        L"Audio Endpoint",                      // 音频端点（逻辑端点，非硬件）
        L"\u97f3\u9891\u7aef\u70b9",            // 音频端点
        L"\u8f6f\u4ef6\u5408\u6210\u5668",      // 软件合成器
        L"Wavetable", L"Microsoft GS",
        L"Root Hub",                            // USB 根集线器（主板自带，无信息量）
        L"\u6839\u96c6\u7ebf\u5668",            // 根集线器
        L"Generic USB Hub",                     // 通用集线器
        L"\u901a\u7528\u96c6\u7ebf\u5668",      // 通用集线器（中文）
        L"USB Composite Device",                  // USB 复合设备（父设备，型号无意义；精确匹配，避免误伤名字里含 Composite 的声卡）
        L"USB \u590d\u5408\u8bbe\u5907",          // USB 复合设备（中文系统）
        L"\u7cfb\u7edf\u8bbe\u5907",              // 系统设备（中文；英文 System device 易误伤，不列）
    };
    for (int i = 0; i < (int)(sizeof(kBad) / sizeof(kBad[0])); i++)
        if (ContainsCI(s, kBad[i])) return TRUE;
    return FALSE;
}

// 虚拟网卡过滤：WAN Miniport 等不是真实物理网卡，不计入"总计"也不进 UI/报告列表。
static BOOL IsVirtualNic(const WCHAR* s) {
    if (!s || !s[0]) return TRUE;
    static const WCHAR* kVirtual[] = {
        L"WAN Miniport",                // Windows 拨号/VPN 虚拟适配器（WAN Miniport (IKEv2) 等）
        L"Microsoft 6to4",
        L"Microsoft ISATAP",
        L"Microsoft Kernel Debug",
        L"Microsoft Teredo",
        L"Teredo Tunneling",
        L"RAS Async",
        L"Microsoft IP-HTTPS",
        L"Microsoft Wi-Fi Direct",
        L"Bluetooth Device (Personal Area Network)",
        L"Npcap", L"WinPcap",           // 抓包驱动虚拟网卡
        L"TAP-Windows",
        // v1.2.1 审查补充：回环与常见 VM/容器虚拟网卡
        L"Software Loopback Interface", // 软件回环适配器（GetAdaptersInfo 一定会返回）
        L"Microsoft Loopback Adapter",
        L"Hyper-V Virtual Ethernet",    // Hyper-V vEthernet
        L"vEthernet",                   // WSL / Docker / Hyper-V Default Switch
        L"VirtualBox Host-Only",
        L"VMware Virtual Ethernet", L"VMnet",
        L"Microsoft KM-TEST",
    };
    for (int i = 0; i < (int)(sizeof(kVirtual) / sizeof(kVirtual[0])); i++)
        if (ContainsCI(s, kVirtual[i])) return TRUE;
    return FALSE;
}

static void CollectAudioUsb(HardwareInfo* hw) {
    HDEVINFO h = SetupDiGetClassDevsW(NULL, NULL, NULL, DIGCF_ALLCLASSES | DIGCF_PRESENT);
    if (h == INVALID_HANDLE_VALUE) return;
    SP_DEVINFO_DATA did = {0};
    did.cbSize = sizeof(did);
    for (DWORD i = 0; SetupDiEnumDeviceInfo(h, i, &did); i++) {
        WCHAR cls[64] = {0}, desc[256] = {0}, enumer[64] = {0};
        GetDevProp(h, &did, SPDRP_CLASS,            cls,    _countof(cls));
        GetDevProp(h, &did, SPDRP_DEVICEDESC,       desc,   _countof(desc));
        GetDevProp(h, &did, SPDRP_ENUMERATOR_NAME,  enumer, _countof(enumer));
        if (!desc[0]) continue;
        // v1.0.9：只收录真实物理硬件。
        // 先按枚举器排除纯软件设备（SW = 软件枚举设备，如系统自带的音频端点），
        // 再按名字排除虚拟 / 总线占位项。两处都是 continue，顺序不影响结果，
        // 但先判枚举器更贴合"先看它到底是不是硬件"的直觉。
        if (enumer[0] && wcscmp(enumer, L"SW") == 0) continue;
        if (IsNotRealHw(desc)) continue;

        // 音频：仅 MEDIA 类（AudioEndpoint 是逻辑端点，非硬件，不收录）
        if (wcscmp(cls, L"MEDIA") == 0 &&
            hw->audioCount < HW_MAX_AUDIO && !AudioDup(hw, desc)) {
            StringCchCopyW(hw->audios[hw->audioCount].name,
                           _countof(hw->audios[hw->audioCount].name), desc);
            hw->audioCount++;
        }
        // U 盘 / 移动硬盘：枚举器为 USBSTOR
        if (wcscmp(enumer, L"USBSTOR") == 0 &&
            hw->usbCount < HW_MAX_USB && !UsbDup(hw, desc)) {
            StringCchCopyW(hw->usbs[hw->usbCount].name, _countof(hw->usbs[0].name), desc);
            StringCchCopyW(hw->usbs[hw->usbCount].cls,  _countof(hw->usbs[0].cls),  L"USBSTOR");
            hw->usbCount++;
        }
        // 键鼠等 HID 外设（去重后仍有意义）
        else if (wcscmp(cls, L"HIDClass") == 0 &&
                 hw->usbCount < HW_MAX_USB && !UsbDup(hw, desc)) {
            StringCchCopyW(hw->usbs[hw->usbCount].name, _countof(hw->usbs[0].name), desc);
            StringCchCopyW(hw->usbs[hw->usbCount].cls,  _countof(hw->usbs[0].cls),  L"HID");
            hw->usbCount++;
        }
        // 摄像头 / 图像设备
        else if ((wcscmp(cls, L"Image") == 0 || wcscmp(cls, L"Camera") == 0) &&
                 hw->usbCount < HW_MAX_USB && !UsbDup(hw, desc)) {
            StringCchCopyW(hw->usbs[hw->usbCount].name, _countof(hw->usbs[0].name), desc);
            StringCchCopyW(hw->usbs[hw->usbCount].cls,  _countof(hw->usbs[0].cls),  cls);
            hw->usbCount++;
        }
    }
    SetupDiDestroyDeviceInfoList(h);
}

// v1.1.6：把音频设备名归一化成可比较的 key。
// COM 默认端点的 FriendlyName（如 "扬声器 (High Definition Audio Device)"）与
// SetupAPI 采集到的同名设备（如 "High Definition Audio 设备"）存在中英文
// "Device/设备" 差异，直接 contains 匹配会失败。统一：转小写、去空格与括号、
// 剥掉 "device"/"设备" 后再做子串匹配，跨本地化也能对上。
static void NormalizeAudioKey(const WCHAR* in, WCHAR* out, size_t nOut) {
    out[0] = 0;
    if (!in || !in[0] || nOut == 0) return;
    WCHAR tmp[256] = {0};
    size_t k = 0;
    for (size_t i = 0; in[i] && k < _countof(tmp) - 1; i++) {
        WCHAR c = in[i];
        if (c == L' ' || c == L'(' || c == L')' || c == L'-' || c == L'_') continue;
        tmp[k++] = (WCHAR)towlower((wint_t)c);
    }
    tmp[k] = 0;
    // 剥掉中英文 "device/设备"
    WCHAR noDevice[256] = {0};
    size_t m = 0;
    const WCHAR* p = tmp;
    while (*p && m < _countof(noDevice) - 1) {
        if (wcsncmp(p, L"device", 6) == 0) { p += 6; continue; }
        if (wcsncmp(p, L"\u8bbe\u5907", 2) == 0) { p += 2; continue; }
        noDevice[m++] = *p++;
    }
    noDevice[m] = 0;
    StringCchCopyW(out, nOut, noDevice);
}

// v1.0.12.1：按指定 role（eRender / eCapture）拿系统当前默认设备的 friendly name，匹配到 HwAudio.role 字段。
// 走 IMMDeviceEnumerator COM API（Win Vista+ 通用，无需驱动；Server 也可调）。
// 任一步失败都 fallback 不抛异常。roleMask：1=播放 2=录音。
static void CollectDefaultAudioByRole(HardwareInfo* hw, EDataFlow flow, int roleMask) {
    if (!hw || hw->audioCount == 0) return;
    HRESULT hrInit = CoInitializeEx(NULL, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
    BOOL needUninit = SUCCEEDED(hrInit);
    IMMDeviceEnumerator* pEnum = NULL;
    HRESULT hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), NULL, CLSCTX_ALL,
                                  __uuidof(IMMDeviceEnumerator), (LPVOID*)&pEnum);
    if (FAILED(hr) || !pEnum) { if (needUninit) CoUninitialize(); return; }
    IMMDevice* pDev = NULL;
    hr = pEnum->GetDefaultAudioEndpoint(flow, eConsole, &pDev);
    if (FAILED(hr) || !pDev) { pEnum->Release(); if (needUninit) CoUninitialize(); return; }
    IPropertyStore* pProps = NULL;
    hr = pDev->OpenPropertyStore(STGM_READ, &pProps);
    if (FAILED(hr) || !pProps) { pDev->Release(); pEnum->Release(); if (needUninit) CoUninitialize(); return; }
    PROPVARIANT varName;
    PropVariantInit(&varName);
    hr = pProps->GetValue(PKEY_Device_FriendlyName, &varName);
    if (SUCCEEDED(hr) && varName.vt == VT_LPWSTR && varName.pwszVal) {
        // 模糊匹配（双向 contains）：把默认端点 friendly name 与 SetupAPI 采集的 audio name
        // 各自归一化（转小写/去空格括号/剥 device·设备）后再比对，跨中英文本地化也能命中。
        const WCHAR* defName = varName.pwszVal;
        WCHAR defKey[256] = {0};
        NormalizeAudioKey(defName, defKey, _countof(defKey));
        for (int i = 0; i < hw->audioCount; i++) {
            WCHAR nKey[256] = {0};
            NormalizeAudioKey(hw->audios[i].name, nKey, _countof(nKey));
            if (ContainsCI(defKey, nKey) || ContainsCI(nKey, defKey)) {
                hw->audios[i].role |= roleMask;
                break;
            }
        }
    }
    PropVariantClear(&varName);
    pProps->Release();
    pDev->Release();
    pEnum->Release();
    if (needUninit) CoUninitialize();
}

static void CollectDefaultAudio(HardwareInfo* hw) {
    if (!hw || hw->audioCount == 0) return;
    // v1.0.12.1：分别取默认播放和默认录音，一条音频可能既是默认播放又是默认录音
    CollectDefaultAudioByRole(hw, eRender,  1);  // 默认播放
    CollectDefaultAudioByRole(hw, eCapture, 2);  // 默认录音
}

// ============================================================
// 物理硬盘（\\.\PhysicalDriveN + IOCTL）
//
// 不用 WMI（无盘机/未装驱动时经常查不到），也不用任何第三方驱动：
//   IOCTL_STORAGE_QUERY_PROPERTY / StorageDeviceProperty  -> 厂商+型号、总线类型
//   IOCTL_DISK_GET_LENGTH_INFO                            -> 容量
//   IOCTL_STORAGE_QUERY_PROPERTY / SeekPenalty            -> 无寻道惩罚 = SSD
// 均为系统自带 IOCTL，设备存在即可读；打不开（无权限/无盘）则回退 SetupAPI 设备名。
// ============================================================
// 自定义结构：避免不同 SDK / MinGW 版本下 DEVICE_SEEK_PENALTY_DESCRIPTOR 定义差异
struct HwDiskSeekPenalty { DWORD Version; DWORD Size; BOOLEAN IncursSeekPenalty; };

static const WCHAR* BusTypeName(STORAGE_BUS_TYPE t) {
    switch (t) {
    case BusTypeNvme:    return L"NVMe";
    case BusTypeSata:    return L"SATA";
    case BusTypeSas:     return L"SAS";
    case BusTypeUsb:     return L"USB";
    case BusTypeAtapi:   return L"ATA";
    case BusTypeScsi:    return L"SCSI";
    case BusTypeRAID:    return L"RAID";
    case BusTypeiScsi:   return L"iSCSI";
    case BusTypeMmc:     return L"MMC";
    case BusTypeSd:      return L"SD";
    case BusTypeVirtual: return L"虚拟";
    case BusTypeSpaces:  return L"存储空间";
    case BusTypeUfs:     return L"UFS";
    default:             return NULL;
    }
}

// 回退：PhysicalDrive 全部打不开时（极少见），从设备树取 DiskDrive 友好名
static void CollectDiskFallback(HardwareInfo* hw) {
    HDEVINFO h = SetupDiGetClassDevsW(NULL, NULL, NULL, DIGCF_ALLCLASSES | DIGCF_PRESENT);
    if (h == INVALID_HANDLE_VALUE) return;
    SP_DEVINFO_DATA did = {0};
    did.cbSize = sizeof(did);
    for (DWORD i = 0; SetupDiEnumDeviceInfo(h, i, &did) && hw->diskCount < HW_MAX_DISK; i++) {
        WCHAR cls[64] = {0}, desc[256] = {0};
        GetDevProp(h, &did, SPDRP_CLASS, cls, _countof(cls));
        if (wcscmp(cls, L"DiskDrive") != 0) continue;
        if (!GetDevProp(h, &did, SPDRP_FRIENDLYNAME, desc, _countof(desc)))
            GetDevProp(h, &did, SPDRP_DEVICEDESC, desc, _countof(desc));
        if (!desc[0]) continue;
        // 去重
        BOOL dup = FALSE;
        for (int k = 0; k < hw->diskCount; k++)
            if (wcscmp(hw->disks[k].name, desc) == 0) { dup = TRUE; break; }
        if (dup) continue;
        HwDisk* d = &hw->disks[hw->diskCount];
        StringCchCopyW(d->name, _countof(d->name), desc);
        PutUnknown(d->iface, _countof(d->iface));
        PutUnknown(d->media, _countof(d->media));
        d->sizeBytes = 0;
        d->index = hw->diskCount;
        hw->diskCount++;
    }
    SetupDiDestroyDeviceInfoList(h);
}

static void CollectDisk(HardwareInfo* hw) {
    for (int i = 0; i < HW_MAX_DISK; i++) {
        WCHAR path[32];
        StringCchPrintfW(path, _countof(path), L"\\\\.\\PhysicalDrive%d", i);
        // 先按只读打开（IOCTL_DISK_GET_LENGTH_INFO 要求 FILE_READ_ACCESS）；
        // 失败再退回 0 权限（部分只读/被占用设备仍允许查询属性）。
        HANDLE hDev = CreateFileW(path, GENERIC_READ,
                                  FILE_SHARE_READ | FILE_SHARE_WRITE,
                                  NULL, OPEN_EXISTING, 0, NULL);
        if (hDev == INVALID_HANDLE_VALUE)
            hDev = CreateFileW(path, 0, FILE_SHARE_READ | FILE_SHARE_WRITE,
                               NULL, OPEN_EXISTING, 0, NULL);
        if (hDev == INVALID_HANDLE_VALUE) continue;

        BYTE  raw[1024] = {0};
        DWORD ret = 0;
        STORAGE_PROPERTY_QUERY q; ZeroMemory(&q, sizeof(q));
        q.PropertyId = StorageDeviceProperty;
        q.QueryType  = PropertyStandardQuery;

        WCHAR model[128] = {0};
        STORAGE_BUS_TYPE bus = BusTypeUnknown;
        BOOL gotDesc = DeviceIoControl(hDev, IOCTL_STORAGE_QUERY_PROPERTY,
                                       &q, sizeof(q), raw, sizeof(raw), &ret, NULL) && ret;
        if (gotDesc) {
            STORAGE_DEVICE_DESCRIPTOR* d = (STORAGE_DEVICE_DESCRIPTOR*)raw;
            bus = (STORAGE_BUS_TYPE)d->BusType;
            // 厂商/型号/固件版本：偏移为 0 表示字段不存在
            WCHAR vend[64] = {0}, prod[96] = {0};
            // 用显式长度读：raw[1024] 里只有前 ret 字节是设备实际返回的，
            // 传 -1 会把结构体尾部未定义字节也当字符串读（可能触到 buffer 末尾才停）
            if (d->VendorIdOffset && d->VendorIdOffset < ret) {
                int maxb = (int)(ret - d->VendorIdOffset);
                MultiByteToWideChar(CP_UTF8, 0, (char*)(raw + d->VendorIdOffset), maxb,
                                    vend, (int)_countof(vend) - 1);
                vend[_countof(vend) - 1] = 0;         // 截断点可能不补 NUL，手动兜底
            }
            if (d->ProductIdOffset && d->ProductIdOffset < ret) {
                int maxb = (int)(ret - d->ProductIdOffset);
                MultiByteToWideChar(CP_UTF8, 0, (char*)(raw + d->ProductIdOffset), maxb,
                                    prod, (int)_countof(prod) - 1);
                prod[_countof(prod) - 1] = 0;
            }
            // trim
            for (int k = (int)wcslen(vend) - 1; k >= 0 && vend[k] == L' '; k--) vend[k] = 0;
            for (int k = (int)wcslen(prod) - 1; k >= 0 && prod[k] == L' '; k--) prod[k] = 0;
            if (vend[0] && prod[0]) StringCchPrintfW(model, _countof(model), L"%ls %ls", vend, prod);
            else if (prod[0])       StringCchCopyW(model, _countof(model), prod);
            else if (vend[0])       StringCchCopyW(model, _countof(model), vend);
        }

        ULONGLONG size = 0;
        GET_LENGTH_INFORMATION gli = {0};
        if (DeviceIoControl(hDev, IOCTL_DISK_GET_LENGTH_INFO, NULL, 0,
                            &gli, sizeof(gli), &ret, NULL))
            size = (ULONGLONG)gli.Length.QuadPart;

        // 介质类型：无寻道惩罚 = SSD（NVMe 天然是 SSD，打不开查询时据此兜底）
        HwDiskSeekPenalty sp;
        ZeroMemory(&sp, sizeof(sp));
        sp.Size = sizeof(sp);                    // 部分驱动/存储栈会校验 Size 再填充
        STORAGE_PROPERTY_QUERY q2; ZeroMemory(&q2, sizeof(q2));
        q2.PropertyId = StorageDeviceSeekPenaltyProperty;
        q2.QueryType  = PropertyStandardQuery;
        BOOL gotSp = DeviceIoControl(hDev, IOCTL_STORAGE_QUERY_PROPERTY,
                                     &q2, sizeof(q2), &sp, sizeof(sp), &ret, NULL) && ret >= sizeof(sp);
        const WCHAR* media = S_UNKNOWN;
        if (gotSp)          media = sp.IncursSeekPenalty ? L"HDD" : L"SSD";
        else if (bus == BusTypeNvme) media = L"SSD";

        CloseHandle(hDev);

        if (!model[0] && !size) continue;   // 什么都没读到（空读卡器插槽等），跳过

        HwDisk* d = &hw->disks[hw->diskCount];
        if (model[0]) StringCchCopyW(d->name, _countof(d->name), model);
        else          PutUnknown(d->name, _countof(d->name));
        {
            const WCHAR* bt = BusTypeName(bus);
            if (bt) StringCchCopyW(d->iface, _countof(d->iface), bt);
            else    PutUnknown(d->iface, _countof(d->iface));
        }
        StringCchCopyW(d->media, _countof(d->media), media);
        d->sizeBytes = size;
        d->index = i;
        hw->diskCount++;
        hw->diskTotalBytes += size;
    }
    if (hw->diskCount == 0) CollectDiskFallback(hw);
}

// ============================================================
// Server 判定（RtlGetVersion 不受 manifest 兼容性标记影响，比 GetVersionEx 可靠）
// ============================================================
typedef struct _RTL_OSVER {
    ULONG  dwOSVersionInfoSize;
    ULONG  dwMajorVersion, dwMinorVersion, dwBuildNumber, dwPlatformId;
    WCHAR  szCSDVersion[128];
    USHORT wServicePackMajor, wServicePackMinor, wSuiteMask;
    UCHAR  wProductType, wReserved;
} RTL_OSVER;
typedef LONG (WINAPI* PFN_RtlGetVersion)(RTL_OSVER*);

static BOOL DetectIsServer() {
    HMODULE h = GetModuleHandleW(L"ntdll.dll");
    if (!h) return FALSE;
    PFN_RtlGetVersion f = (PFN_RtlGetVersion)GetProcAddress(h, "RtlGetVersion");
    if (!f) return FALSE;
    RTL_OSVER vi = {0};
    vi.dwOSVersionInfoSize = sizeof(vi);
    if (f(&vi) != 0) return FALSE;
    return (vi.wProductType == VER_NT_SERVER);
}

// ============================================================
// 对外接口
// ============================================================
void HW_Collect(HardwareInfo* out) {
    if (!out) return;
    ZeroMemory(out, sizeof(*out));

    out->isServer = DetectIsServer();

    // --- 操作系统 ---
    RegReadStr(HKEY_LOCAL_MACHINE, L"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
               L"ProductName", out->osProduct, _countof(out->osProduct));
    if (!RegReadStr(HKEY_LOCAL_MACHINE, L"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
                    L"DisplayVersion", out->osVersion, _countof(out->osVersion)))
        RegReadStr(HKEY_LOCAL_MACHINE, L"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
                   L"ReleaseId", out->osVersion, _countof(out->osVersion));
    RegReadStr(HKEY_LOCAL_MACHINE, L"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
               L"CurrentBuild", out->osBuild, _countof(out->osBuild));
    if (!out->osProduct[0]) PutUnknown(out->osProduct, _countof(out->osProduct));
    if (!out->osVersion[0]) PutUnknown(out->osVersion, _countof(out->osVersion));
    if (!out->osBuild[0])   PutUnknown(out->osBuild,   _countof(out->osBuild));

    // 安装日期：v1.0.9 起不再采集也展示（用户明确要求"操作系统不显示安装时间"）。
    // out->osInstall 保持全 0（字段保留仅为结构兼容，任何输出路径都不得引用）。

    // 架构
    {
        SYSTEM_INFO si = {0};
        GetNativeSystemInfo(&si);
        switch (si.wProcessorArchitecture) {
        case PROCESSOR_ARCHITECTURE_AMD64: StringCchCopyW(out->osArch, _countof(out->osArch), L"x64"); break;
        case PROCESSOR_ARCHITECTURE_INTEL: StringCchCopyW(out->osArch, _countof(out->osArch), L"x86"); break;
        case PROCESSOR_ARCHITECTURE_ARM64: StringCchCopyW(out->osArch, _countof(out->osArch), L"ARM64"); break;
        default: PutUnknown(out->osArch, _countof(out->osArch)); break;
        }
        out->logicalProcTotal = si.dwNumberOfProcessors;
    }

    // 计算机名 / 当前用户
    {
        DWORD n = _countof(out->computerName);
        if (!GetComputerNameExW(ComputerNameDnsHostname, out->computerName, &n))
            PutUnknown(out->computerName, _countof(out->computerName));
        n = _countof(out->userName);
        if (!GetUserNameW(out->userName, &n))
            PutUnknown(out->userName, _countof(out->userName));
    }

    // --- SMBIOS：BIOS / 主板 / CPU / 内存条 ---
    PutUnknown(out->biosVendor,  _countof(out->biosVendor));
    PutUnknown(out->biosVersion, _countof(out->biosVersion));
    PutUnknown(out->biosDate,    _countof(out->biosDate));
    PutUnknown(out->boardVendor,  _countof(out->boardVendor));
    PutUnknown(out->boardProduct, _countof(out->boardProduct));
    ParseSmbios(out);
    CollectChipset(out);   // v1.1.8：芯片组（PCI 主机桥 FriendlyName）

    // --- CPU：SMBIOS 没拿到时用 CPUID + 系统信息兜底 ---
    if (out->cpuCount == 0) {
        HwCpu* c = &out->cpus[0];
        CpuIdFill(c->name, _countof(c->name), c->vendor, _countof(c->vendor));
        PutUnknown(c->socket, _countof(c->socket));
        c->cores   = out->logicalProcTotal;      // 保守：拿不到物理核就用逻辑核
        c->threads = out->logicalProcTotal;
        c->speedMHz = 0;
        out->cpuCount = 1;
    }
    CollectMaxProcessor(out);   // v1.1.8：硬件最大核/线程（CPUID，不受 BIOS 单核模式影响）

    // v1.2.5：CPU 微架构代号（libcpuid）。代号是型号级属性，采集第一颗即可；
    // 仅填充 codename，不覆盖 SMBIOS/CPUID 已拿到的 name/vendor/cores。
    if (out->cpuCount > 0)
        CollectCpuCodename(out->cpus[0].codename, _countof(out->cpus[0].codename));

    // --- 物理内存总量 ---
    {
        MEMORYSTATUSEX ms = {0};
        ms.dwLength = sizeof(ms);
        out->memTotalMB = GlobalMemoryStatusEx(&ms) ? (ms.ullTotalPhys >> 20) : 0;
    }
    for (int i = 0; i < out->memCount; i++)
        if (out->mem[i].populated) out->memPopulated++;

    // --- 显卡 / 显示器 ---
    CollectGpu(out);
    CollectGpuRealNames(out);   // v1.2.6：未装显卡驱动时用 PCI ID 还原真实型号（占位名修正）
    CollectGpuVram(out);   // v1.1.8：独立显存（DXGI）
    CollectGpuNvml(out);   // v1.2.3：NVIDIA GPU 遥测（NVML，动态加载，无 N 卡保持 0）
    CollectGpuAdl(out);    // v1.2.4：AMD GPU 遥测（ADL，动态加载，无 A 卡保持 0）

    // --- 网卡 ---
    CollectNic(out);

    // --- 物理硬盘（IOCTL，v1.0.9）---
    CollectDisk(out);

    // --- 音频 / USB 外设：Server 系统上不采集（服务器不接声卡与外设，列出来是噪声）---
    if (!out->isServer) CollectAudioUsb(out);
    if (!out->isServer && out->audioCount > 0) CollectDefaultAudio(out);   // v1.0.12：标记默认播放设备
}

// 容量格式化（GB / MB / KB 自适应；0 写"未知"）
static void FmtSizeW(ULONGLONG bytes, WCHAR* out, size_t n) {
    if (bytes >= (1ULL << 30))      StringCchPrintfW(out, n, L"%I64u GB", bytes >> 30);
    else if (bytes >= (1ULL << 20)) StringCchPrintfW(out, n, L"%I64u MB", bytes >> 20);
    else if (bytes)                 StringCchPrintfW(out, n, L"%I64u KB", bytes >> 10);
    else                            StringCchCopyW(out, n, S_UNKNOWN);
}

// ============================================================
// 纯文本摘要（一键复制用）
// ============================================================
void HW_BuildText(const HardwareInfo* hw, WCHAR* out, size_t nChars) {
    if (!hw || !out || !nChars) return;
    out[0] = 0;
    WCHAR line[512];

    // GCC 的 __VA_ARGS__ 不允许零参数；用 ##__VA_ARGS__（GNU 扩展）兜底。
    // 单参数时 ##__VA_ARGS__ 会清掉前面的逗号，规避 "expected primary-expression" 错误。
    #define APPEND(fmt, ...) do { \
        StringCchPrintfW(line, _countof(line), fmt, ##__VA_ARGS__); \
        StringCchCatW(out, nChars, line); } while (0)

    APPEND(L"[%s]\r\n%s: %ls\r\n%s: %ls\r\n%s: %ls\r\n%s: %ls\r\n\r\n",
           S_OS, S_PRODUCT, hw->osProduct, S_VERSION, hw->osVersion,
           S_ARCH, hw->osArch, L"Build", hw->osBuild);

    APPEND(L"[%s]\r\n%s: %ls\r\n%s: %ls\r\n\r\n",
           S_MACHINE, S_MODEL, hw->computerName, S_USER, hw->userName);

    APPEND(L"[%s]\r\n%s: %ls\r\n%s: %ls",
           S_BOARD, S_VENDOR, hw->boardVendor, S_PRODUCT, hw->boardProduct);
    if (hw->boardChipset[0]) APPEND(L"\r\n%s: %ls", L"\u82af\u7247\u7ec4", hw->boardChipset);   // v1.1.8
    APPEND(L"\r\n\r\n");

    APPEND(L"[%s]\r\n%s: %ls\r\n%s: %ls\r\n%s: %ls\r\n\r\n",
           L"BIOS", S_VENDOR, hw->biosVendor, S_VERSION, hw->biosVersion,
           L"Date", hw->biosDate);

    APPEND(L"[%s] x%d (%s %u)\r\n", S_CPU, hw->cpuCount, S_THREAD, hw->logicalProcTotal);
    for (int i = 0; i < hw->cpuCount; i++) {
        const HwCpu* c = &hw->cpus[i];
        // v1.2.1：核/线程（OS 真实拓扑优先），仅当与当前启用不同才追加
        APPEND(L"  %d. %ls\r\n     %s %u", i + 1, c->name, S_CORE, c->cores);
        if (c->maxCores > 0 && c->maxThreads > 0 &&
            (c->maxCores != c->cores || c->maxThreads != c->threads))
            APPEND(L" (%ls %u)", L"\u786c\u4ef6", c->maxCores);
        APPEND(L" / %s %u", S_THREAD, c->threads);
        if (c->maxCores > 0 && c->maxThreads > 0 &&
            (c->maxCores != c->cores || c->maxThreads != c->threads))
            APPEND(L" (%ls %u)", L"\u786c\u4ef6", c->maxThreads);
        if (c->speedMHz) APPEND(L"  %ls %u MHz", S_FREQ, c->speedMHz);
        APPEND(L"\r\n     %s: %ls\r\n", S_SLOT, c->socket);
        if (c->codename[0]) APPEND(L"     %s: %ls\r\n", S_CODENAME, c->codename);   // v1.2.5 libcpuid
    }
    APPEND(L"\r\n");

    { WCHAR mt[32]; FmtSizeW((ULONGLONG)hw->memTotalMB << 20, mt, _countof(mt));
      APPEND(L"[%s] %s %ls (%s %d / %d %s)\r\n", S_MEM, S_TOTAL, mt,
             S_INSERTED, hw->memPopulated, hw->memCount, S_SLOT); }
    for (int i = 0; i < hw->memCount; i++) {
        const HwMemStick* m = &hw->mem[i];
        if (!m->populated)
            APPEND(L"  %d. [%ls] %s\r\n", i + 1, m->locator, S_EMPTY);
        else {
            WCHAR cap[32];
            FmtSizeW((ULONGLONG)m->capacityMB << 20, cap, _countof(cap));
            APPEND(L"  %d. [%ls] %ls", i + 1, m->locator, cap);
            if (m->memType[0]) APPEND(L"  %ls", m->memType);   // v1.1.8：内存类型
            APPEND(L"  %u MHz  %ls %ls\r\n", m->speedMHz, m->vendor, m->partNo);
        }
    }
    APPEND(L"\r\n");

    APPEND(L"[%s]\r\n", S_GPU);
    for (int i = 0; i < hw->gpuCount; i++) {
        const HwGpu* gg = &hw->gpus[i];
        APPEND(L"  %d. %ls", i + 1, gg->name);
        if (gg->vramMB > 0) APPEND(L"  (%u GB)", (gg->vramMB + 512) / 1024);  // v1.1.8 显存
        // v1.2.3：NVML 遥测（N 卡才有；无 N 卡 / 无驱动 / 非 N 卡全 0 则不追加）
        if (gg->tempC > 0 || gg->fanPct > 0 || gg->powerW > 0 || gg->utilPct > 0 || gg->clockMHz > 0) {
            int totalMB = (gg->memUsedMB + gg->memFreeMB > 0) ? (gg->memUsedMB + gg->memFreeMB)
                                                               : (int)gg->vramMB;
            APPEND(L"  [\u6e29\u5ea6%d\u00b0C \u98ce\u6247%d%% %dW \u5229\u7528%d%% \u663e\u5b58%d/%dMB %uMHz]",
                   gg->tempC, gg->fanPct, gg->powerW, gg->utilPct, gg->memUsedMB, totalMB, gg->clockMHz);
        }
        APPEND(L"\r\n");
    }
    if (!hw->gpuCount) APPEND(L"  %ls\r\n", S_UNKNOWN);
    APPEND(L"\r\n");

    APPEND(L"[%s]\r\n", S_DISP);
    for (int i = 0; i < hw->displayCount; i++) {
        const HwDisplay* d = &hw->displays[i];
        APPEND(L"  %d. %u x %u @ %u Hz", i + 1,
               d->width, d->height, d->refreshHz);
        if (d->size[0]) APPEND(L"  %ls", d->size);   // v1.1.8：尺寸
        APPEND(L"  (%ls", d->name);
        if (d->vendor[0]) APPEND(L"  \u5382\u5546:%ls", d->vendor);
        APPEND(L")\r\n");
    }
    if (!hw->displayCount) APPEND(L"  %ls\r\n", S_UNKNOWN);
    APPEND(L"\r\n");

    APPEND(L"[%s]\r\n", S_DISK);
    for (int i = 0; i < hw->diskCount; i++) {
        const HwDisk* d = &hw->disks[i];
        WCHAR sz[32]; FmtSizeW(d->sizeBytes, sz, _countof(sz));
        APPEND(L"  %d. %ls  (%ls / %ls / %ls)\r\n", i + 1,
               d->name, d->iface, d->media, sz);
    }
    if (!hw->diskCount) APPEND(L"  %ls\r\n", S_UNKNOWN);
    APPEND(L"\r\n");

    APPEND(L"[%s] x%d\r\n", S_NIC, hw->nicCount);
    for (int i = 0; i < hw->nicCount; i++) {
        const HwNic* n = &hw->nics[i];
        APPEND(L"  %d. %ls\r\n     MAC: %ls\r\n     IPv4: %ls\r\n",
               i + 1, n->desc, n->mac, n->ipv4);
    }
    if (!hw->nicCount) APPEND(L"  %ls\r\n", S_UNKNOWN);

    // Server 系统不输出音频与 USB（采集阶段就没采，这里也不会有）
    // v1.0.9：只输出真实识别到的设备；一项都没有就整段不输出（不再写"未知"占位）
    if (!hw->isServer) {
        if (hw->audioCount) {
            APPEND(L"\r\n[%s] x%d\r\n", S_AUDIO, hw->audioCount);
            for (int i = 0; i < hw->audioCount; i++)
                APPEND(L"  %d. %ls\r\n", i + 1, hw->audios[i].name);
        }
        if (hw->usbCount) {
            APPEND(L"\r\n[%s] x%d\r\n", S_USB, hw->usbCount);
            for (int i = 0; i < hw->usbCount; i++)
                APPEND(L"  %d. [%ls] %ls\r\n", i + 1, hw->usbs[i].cls, hw->usbs[i].name);
        }
    }
    #undef APPEND
}
