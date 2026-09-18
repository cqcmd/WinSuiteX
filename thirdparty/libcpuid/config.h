/*
 * config.h — WinSuiteX 专用极简配置头（libcpuid 0.4.1 快照）
 *
 * 上游 libcpuid 的 config.h 由 autotools/cmake 生成，包含大量 HAVE_* 探测宏。
 * WinSuiteX 只消费 libcpuid 的 CPU 代号识别能力（cpu_identify → cpu_id_t::cpu_codename），
 * 无需那些探测宏；这里仅保留 cpuid_main.c 实际引用的 VERSION，其余全部省略。
 *
 * 引入方式：cpuid_main.c 以 `#ifdef HAVE_CONFIG_H` 包裹 include "config.h"，
 * 故编译时需同时定义 HAVE_CONFIG_H 才会显式包含本文件。若未来不定义 HAVE_CONFIG_H，
 * 也务必保证 VERSION/CURRENT/... 宏在别处可见（否则 cpuid_lib_version 等编译报错）。
 */
#ifndef WINSUITEX_LIBCPUID_CONFIG_H
#define WINSUITEX_LIBCPUID_CONFIG_H

#define VERSION "0.4.1"

#endif /* WINSUITEX_LIBCPUID_CONFIG_H */