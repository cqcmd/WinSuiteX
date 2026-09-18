// ============================================================
// hardware.h - 硬件信息采集（WinSuiteX）
//
// 设计原则：**不依赖任何已安装的驱动程序**。
// 网维场景（无盘启动、刚装完系统还没打驱动）下，常规工具看到的是
// "Standard VGA"、"PCI Device" 这类占位名，本模块改从以下硬件/固件层读取：
//   - SMBIOS：BIOS 写入主板的硬件清单（GetSystemFirmwareTable 'RSMB'），
//             含 BIOS / 主板 / 处理器 / 内存条插槽，无需驱动即可准确识别
//   - CPUID ：直接读 CPU 寄存器拿品牌串与厂商，硬件级
//   - SetupAPI：设备枚举树（设备节点存在即可读描述，不要求驱动已加载）
//   - 注册表：OS 版本、网卡型号等系统记录
//
// 仅「一键复制」与 UI 绘制需要本模块之外的代码，本文件只产出数据。
// ============================================================
#pragma once
#include <windows.h>

// 采集上限（超出部分截断，避免极端机器撑爆栈）
#define HW_MAX_CPU      8
#define HW_MAX_MEM      16
#define HW_MAX_GPU      4
#define HW_MAX_DISPLAY  8
#define HW_MAX_NIC      16
#define HW_MAX_AUDIO    8
#define HW_MAX_USB      32
#define HW_MAX_DISK     8

// v1.2.6：GPU PCI 查表（供 selftest 单测；采集内部用同名 static 逻辑）
const char* HW_GpuPciLookup(unsigned short ven, unsigned short dev);
// v1.2.6：微软基本显示驱动占位名检测（供 selftest 单测）
BOOL HW_IsGpuPlaceholderName(const WCHAR* name);

// 单块物理硬盘（\\.\PhysicalDriveN，IOCTL 读取，不依赖任何驱动/WMI）
struct HwDisk {
    WCHAR     name[128];   // 型号（Vendor + Product；IOCTL 拿不到时为 SetupAPI 友好名）
    WCHAR     iface[24];   // 接口：NVMe / SATA / SAS / USB / SCSI / RAID ...
    WCHAR     media[16];   // 介质：SSD / HDD / 未知
    ULONGLONG sizeBytes;   // 容量（字节）
    int       index;       // PhysicalDrive 序号
};

// 单根内存条（SMBIOS Type 17 Memory Device）
struct HwMemStick {
    WCHAR  locator[40];   // 插槽位置（Device Locator，如 "DIMM_A1" / "Controller0-ChannelA"）
    WCHAR  bank[24];      // 通道/插槽组（Bank Locator）
    WCHAR  partNo[40];    // 型号 / 料号（Part Number）
    WCHAR  vendor[32];    // 制造商
    WCHAR  memType[12];   // 内存类型：DDR3 / DDR4 / LPDDR4 ...
    DWORD  capacityMB;    // 容量（MB）；0 = 空槽
    DWORD  speedMHz;      // 频率（Configured Speed 优先，回退 Speed）
    BOOL   populated;     // 是否实际插了内存
};

// 单颗物理 CPU（SMBIOS Type 4 Processor Information）
struct HwCpu {
    WCHAR  socket[40];    // 插槽标识（Socket Designation，如 "CPU1" / "SOCKET 0"）
    WCHAR  name[64];      // 型号名
    WCHAR  vendor[32];    // 制造商
    WCHAR  codename[48];  // 微架构代号（如 "Ivy Bridge" / "Zen 3"，v1.2.5 libcpuid 识别）
    DWORD  cores;         // 物理核心数（当前 BIOS 启用）
    DWORD  threads;       // 逻辑处理器（线程）数（当前 BIOS 启用）
    DWORD  maxCores;      // 物理核数（v1.2.1 优先 OS 真实拓扑，回退 CPUID）
    DWORD  maxThreads;    // 逻辑线程数（v1.2.1 优先 OS 真实拓扑，回退 CPUID）
    DWORD  speedMHz;      // 标称/当前频率
};

struct HwNic {
    WCHAR desc[128];      // 型号描述
    WCHAR mac[24];        // MAC 地址（XX-XX-XX-XX-XX-XX）
    WCHAR ipv4[64];       // IPv4（多个用 ", " 分隔）
    WCHAR ipv6[64];       // IPv6（取第一个）
    BOOL  up;             // 是否已连接（有 IP）
};

struct HwGpu {
    WCHAR name[128];
    DWORD vramMB;        // v1.1.8：独立显存（MB），DXGI 拿
    // v1.2.3：NVML 遥测（LoadLibrary("nvml.dll") 动态加载；无 N 卡 / 无驱动 / 非 N 卡全保持 0）
    int  tempC;          // GPU 温度 °C（0=未识别，NVML_ERROR_NOT_SUPPORTED 常见于 iGPU）
    int  fanPct;        // 风扇转速 %（0=不支持或未识别）
    int  powerW;        // 功耗 W（NVML 返回 mW，已转 W；0=不支持）
    int  utilPct;       // GPU 利用率 %（0=空闲或未识别）
    int  memUsedMB;     // 显存已用（total-free）
    int  memFreeMB;     // 显存可用
    DWORD clockMHz;    // GPU 核心时钟 MHz
};
struct HwDisplay { WCHAR name[96]; WCHAR vendor[48]; WCHAR size[16]; DWORD width, height, refreshHz; };
// v1.0.12：isDefault 标记当前系统默认播放设备（IMMDeviceEnumerator::GetDefaultAudioEndpoint(eRender, eConsole)）
// v1.0.12.1：改用 role 字段同时标记默认播放/默认录音（bit 0=播放 / bit 1=录音 / 0=未识别为默认）
struct HwAudio   { WCHAR name[128]; int role; };
struct HwUsb     { WCHAR name[128]; WCHAR cls[24]; };

struct HardwareInfo {
    // 操作系统
    WCHAR osProduct[128];     // 如 "Windows 10 Pro"
    WCHAR osVersion[48];      // DisplayVersion，如 "22H2"
    WCHAR osBuild[24];        // 如 "19045"
    WCHAR osArch[24];         // x64 / x86 / ARM64
    WCHAR osInstall[32];      // 已废弃：v1.0.9 起界面与报告均不再显示安装日期，字段保留置空
    // 计算机
    WCHAR computerName[64];
    WCHAR userName[64];
    // 主板 + BIOS
    WCHAR boardVendor[64];
    WCHAR boardProduct[64];
    WCHAR boardChipset[64];   // v1.1.8：芯片组（SetupAPI 枚举 PCI 主机桥，如 "英特尔 H61 芯片组"）
    WCHAR biosVendor[64];
    WCHAR biosVersion[64];
    WCHAR biosDate[32];
    // 处理器
    HwCpu cpus[HW_MAX_CPU];
    int   cpuCount;           // 物理 CPU 颗数（多路服务器 >1）
    DWORD logicalProcTotal;   // 系统逻辑处理器总数（GetSystemInfo）
    // 内存
    HwMemStick mem[HW_MAX_MEM];
    int   memCount;           // 插槽总数
    int   memPopulated;       // 已插条数
    ULONGLONG memTotalMB;     // 物理内存总量（GlobalMemoryStatusEx）
    // 显卡 / 显示器
    HwGpu gpus[HW_MAX_GPU];
    int   gpuCount;
    HwDisplay displays[HW_MAX_DISPLAY];
    int   displayCount;
    // 网卡
    HwNic nics[HW_MAX_NIC];
    int   nicCount;
    // 物理硬盘（v1.0.9 新增：主页面"硬盘"段与 HTML 报告）
    HwDisk disks[HW_MAX_DISK];
    int    diskCount;
    ULONGLONG diskTotalBytes;
    // 音频 / USB 外设（Server 系统隐藏 —— 服务器不上声卡与外设，列出来是噪声）
    HwAudio audios[HW_MAX_AUDIO];
    int   audioCount;
    HwUsb  usbs[HW_MAX_USB];
    int   usbCount;
    BOOL  isServer;           // RtlGetVersion wProductType == VER_NT_SERVER
};

// 采集全部硬件信息。内部已做边界与容错，任何一项拿不到都退回"未知"而非崩溃。
// 阻塞式（SMBIOS/SetupAPI 调用），建议在启动时调用一次并缓存结果。
void HW_Collect(HardwareInfo* out);

// 生成纯文本摘要（供「一键复制」），按分类排版，out 需足够大（建议 >=4096 字符）
void HW_BuildText(const HardwareInfo* hw, WCHAR* out, size_t nChars);
