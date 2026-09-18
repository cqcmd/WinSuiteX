#ifndef EXPLORERCOMMAND_H
#define EXPLORERCOMMAND_H

#include <windows.h>
#include <shlobj.h>
#include <shlwapi.h>
#include <strsafe.h>

// ============================================================
// explorercommand.h - Win11 新菜单直显用的通用 IExplorerCommand COM 服务器
//
// 现代 MinGW-w64（含本机 16.1.0）的 shobjidl.h 已自带 IExplorerCommand 定义，
// 因此这里不再手动声明接口，直接使用系统版本，保证 vtable 与方法顺序完全正确。
//
// 系统头定义的权威 IID 为：
//   IID_IExplorerCommand = a08ce4d0-FA25-44AB-B57C-C7B1C323E0B9
// （社区里偶尔出现的 A2CFF95B-... 为过时/错误值，本实现不再采用。）
//
// 为最大程度兼容，QueryInterface 额外接受一份"历史遗留 IID"（见下方
// IID_IExplorerCommand_legacy），以免某些旧 Shell 文档以错误 IID 查询时静默失败。
// ============================================================

// 历史遗留（社区误传）IID，仅作为 QI 兜底，不用于注册
static const IID IID_IExplorerCommand_legacy = {
    0xa2cff95b, 0xa84b, 0x4581, {0xba, 0x1c, 0xa5, 0x33, 0x90, 0xf0, 0x4c, 0xb9}};

#endif // EXPLORERCOMMAND_H
