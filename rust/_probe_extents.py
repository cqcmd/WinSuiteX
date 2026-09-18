# -*- coding: utf-8 -*-
"""验证 IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS 的结构对齐与 DiskNumber。"""
import ctypes
import struct
from ctypes import wintypes

k32 = ctypes.windll.kernel32
k32.CreateFileW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD,
                            ctypes.c_void_p, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
k32.CreateFileW.restype = wintypes.HANDLE
k32.DeviceIoControl.argtypes = [wintypes.HANDLE, wintypes.DWORD, ctypes.c_void_p, wintypes.DWORD,
                                ctypes.c_void_p, wintypes.DWORD,
                                ctypes.POINTER(wintypes.DWORD), ctypes.c_void_p]
k32.DeviceIoControl.restype = wintypes.BOOL

IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS = 0x00560000
INVALID_HANDLE = ctypes.c_void_p(-1).value

for letter in ("C", "D"):
    path = "\\\\.\\" + letter + ":"
    h = k32.CreateFileW(path, 0, 3, None, 3, 0, None)
    if h == INVALID_HANDLE or h == -1:
        print(f"{letter}: open fail GetLastError={k32.GetLastError()}")
        continue
    buf = ctypes.create_string_buffer(8 + 24 * 16)
    ret = wintypes.DWORD()
    ok = k32.DeviceIoControl(h, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, None, 0,
                             buf, ctypes.sizeof(buf), ctypes.byref(ret), None)
    b = buf.raw[: ret.value]
    cnt = struct.unpack_from("<I", b, 0)[0]
    print(f"{letter}:: ok={ok} ret={ret.value} NumberOfDiskExtents={cnt}")
    if cnt and ret.value >= 12:
        # Rust 现在按 size_of::<u32>() = 4 取第一个 extent → 读到的是 padding
        print(f"   offset 4 (当前 Rust 读法): {struct.unpack_from('<I', b, 4)[0]}")
        # C 布局：DISK_EXTENT 含 LARGE_INTEGER(align 8) → Extents 起始偏移 8
        print(f"   offset 8 (C 对齐正确值)  : DiskNumber={struct.unpack_from('<I', b, 8)[0]}")
        print("   raw head:", " ".join(f"{x:02X}" for x in b[:32]))
    k32.CloseHandle(h)
