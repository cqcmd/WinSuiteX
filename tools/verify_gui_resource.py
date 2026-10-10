#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""验证 winsuitex-gui-gdi.exe 真的嵌进了 图标 + 版本信息 + 清单（权威，非猜）。

- 版本信息：GetFileVersionInfoSizeW 返回 >0 且 GetLastError() != 1813
  （1813 = 资源类型找不到，即 VS_VERSION_INFO 被当成字符串名、Explorer 读不出版本）。
- 图标 / 清单：LoadLibraryExW(LOAD_LIBRARY_AS_DATAFILE) + FindResourceW 确认存在。
- 版本字符串：VerQueryValueW 取出 FileVersion / CompanyName 验证中文无乱码。
"""
import ctypes
import ctypes.wintypes as wt
import struct
import sys

path = sys.argv[1] if len(sys.argv) > 1 else r"rust\target\release\winsuitex-gui-gdi.exe"

ver = ctypes.WinDLL("version", use_last_error=True)
ver.GetFileVersionInfoSizeW.argtypes = [wt.LPCWSTR, ctypes.POINTER(wt.DWORD)]
ver.GetFileVersionInfoSizeW.restype = wt.DWORD
ver.GetFileVersionInfoW.argtypes = [wt.LPCWSTR, wt.DWORD, wt.DWORD, ctypes.c_void_p]
ver.GetFileVersionInfoW.restype = wt.BOOL
ver.VerQueryValueW.argtypes = [ctypes.c_void_p, wt.LPCWSTR, ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(wt.UINT)]
ver.VerQueryValueW.restype = wt.BOOL

print(f"[*] 检查: {path}")
h = wt.DWORD()
sz = ver.GetFileVersionInfoSizeW(ctypes.c_wchar_p(path), ctypes.byref(h))
err = ctypes.get_last_error()
print(f"[版本] GetFileVersionInfoSizeW sz={sz} err={err}")
vs_ok = (sz > 0 and err != 1813)
if vs_ok:
    buf = (ctypes.c_byte * sz)()
    if ver.GetFileVersionInfoW(ctypes.c_wchar_p(path), 0, sz, buf):
        ptr = ctypes.c_void_p()
        plen = wt.UINT()
        lang = ""
        if ver.VerQueryValueW(buf, ctypes.c_wchar_p(r"\VarFileInfo\Translation"), ctypes.byref(ptr), ctypes.byref(plen)):
            raw = ctypes.string_at(ptr, plen.value)
            lp = struct.unpack("<HH", raw[:4])
            lang = f"{lp[0]:04X}{lp[1]:04X}"
            print(f"[版本] Translation={lang}")
        for key in ("FileVersion", "CompanyName", "LegalCopyright", "ProductName", "OriginalFilename"):
            p2 = ctypes.c_void_p()
            l2 = wt.UINT()
            sub = ctypes.c_wchar_p(rf"\StringFileInfo\{lang}\{key}")
            if ver.VerQueryValueW(buf, sub, ctypes.byref(p2), ctypes.byref(l2)):
                # VerQueryValueW 返回的 l2.value 是【字符数(含结尾 NUL)】，不是字节数。
                n = int(l2.value) * 2
                s = ctypes.string_at(p2, n).decode("utf-16-le", errors="replace").rstrip("\x00")
                print(f"[版本] {key} = {s}")
else:
    print("[版本] FAIL: 版本资源未正确嵌入")

k = ctypes.WinDLL("kernel32", use_last_error=True)
k.LoadLibraryExW.argtypes = [wt.LPCWSTR, wt.HANDLE, wt.DWORD]
k.LoadLibraryExW.restype = wt.HMODULE
k.FindResourceW.argtypes = [wt.HMODULE, wt.LPCWSTR, wt.LPCWSTR]
k.FindResourceW.restype = wt.HANDLE
k.LoadResource.argtypes = [wt.HMODULE, wt.HANDLE]
k.LoadResource.restype = wt.HGLOBAL
k.SizeofResource.argtypes = [wt.HMODULE, wt.HANDLE]
k.SizeofResource.restype = wt.DWORD
k.FreeLibrary.argtypes = [wt.HMODULE]
k.FreeLibrary.restype = wt.BOOL

hmod = k.LoadLibraryExW(ctypes.c_wchar_p(path), None, 0x00000002)  # LOAD_LIBRARY_AS_DATAFILE
print(f"[资源] LoadLibraryEx hmod={hmod}")
ok = vs_ok and bool(hmod)
if hmod:
    RT_ICON = 3
    RT_GROUP_ICON = 14
    RT_MANIFEST = 24
    # `name ICON "x.ico"` 编译后是【图标组】(RT_GROUP_ICON=14)，单帧在 RT_ICON(3)。
    hico = k.FindResourceW(hmod, ctypes.c_wchar_p("#101"), ctypes.c_wchar_p(f"#{RT_GROUP_ICON}"))
    ico_sz = k.SizeofResource(hmod, hico) if hico else 0
    print(f"[图标] #101 (IDI_APP_ICON / RT_GROUP_ICON) res={hico} size={ico_sz}")
    hman = k.FindResourceW(hmod, ctypes.c_wchar_p("#1"), ctypes.c_wchar_p(f"#{RT_MANIFEST}"))
    man_sz = k.SizeofResource(hmod, hman) if hman else 0
    print(f"[清单] #1 (RT_MANIFEST) res={hman} size={man_sz}")
    ok = ok and bool(hico) and bool(hman)
    k.FreeLibrary(hmod)

print("RESULT:", "PASS" if ok else "FAIL")
sys.exit(0 if ok else 1)
