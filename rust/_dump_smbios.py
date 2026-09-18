# -*- coding: utf-8 -*-
"""Dump 本机 raw SMBIOS 表到文件，并用 dmidecode 风格遍历做对照分析。

用法: python _dump_smbios.py [out.bin]
"""
import ctypes
import ctypes.wintypes as wt
import struct
import sys

k32 = ctypes.windll.kernel32
GetSystemFirmwareTable = k32.GetSystemFirmwareTable
GetSystemFirmwareTable.argtypes = [wt.DWORD, wt.DWORD, ctypes.c_void_p, wt.DWORD]
GetSystemFirmwareTable.restype = ctypes.c_uint


def fetch(sig: int) -> bytes | None:
    n = GetSystemFirmwareTable(sig, 0, None, 0)
    if n == 0:
        return None
    buf = ctypes.create_string_buffer(n)
    r = GetSystemFirmwareTable(sig, 0, ctypes.cast(buf, ctypes.c_void_p), n)
    if r == 0 or r > n:
        return None
    return buf.raw[:r]


def walk_dmidecode(tbl: bytes):
    """dmidecode 风格：找到双 00 后 +2"""
    out = []
    pos = 0
    while pos + 4 <= len(tbl):
        typ = tbl[pos]
        length = tbl[pos + 1]
        handle = struct.unpack_from("<H", tbl, pos + 2)[0]
        if length < 4:
            out.append(("BAD_LEN", pos, typ, length))
            break
        end = pos + length
        if end > len(tbl):
            out.append(("TRUNC", pos, typ, length))
            break
        # 字符串区
        nxt = end
        while nxt + 1 < len(tbl) and not (tbl[nxt] == 0 and tbl[nxt + 1] == 0):
            nxt += 1
        nxt += 2
        strs = []
        i = end
        while i < nxt - 2:
            if tbl[i] == 0:
                break
            j = i
            while tbl[j] != 0:
                j += 1
            strs.append(tbl[i:j].decode("ascii", "replace"))
            i = j + 1
        out.append(("OK", pos, typ, length, handle, strs))
        if typ == 127:
            break
        pos = nxt
    return out


def walk_current_rust(tbl: bytes):
    """复刻当前 Rust split_strings 的行为（0 字符串时 consumed=1）"""

    def split_strings(area: bytes):
        out = []
        i = 0
        while i < len(area):
            if area[i] == 0:
                i += 1
                break
            start = i
            while i < len(area) and area[i] != 0:
                i += 1
            out.append(area[start:i])
            i += 1
        return out, i

    out = []
    pos = 0
    steps = 0
    while pos + 4 <= len(tbl) and steps < 500:
        steps += 1
        typ = tbl[pos]
        length = tbl[pos + 1]
        if length < 4:
            out.append(("BAD_LEN", pos, typ, length))
            break
        end = pos + length
        if end > len(tbl):
            out.append(("TRUNC", pos, typ, length))
            break
        _, consumed = split_strings(tbl[end:])
        out.append(("OK", pos, typ, length, consumed))
        nxt = end + consumed
        if nxt <= pos:
            break
        pos = nxt
    return out


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "_smbios.bin"
    raw = None
    for sig in (0x52534D42, 0x424D5352):
        raw = fetch(sig)
        if raw:
            print(f"[ok] signature=0x{sig:08X} size={len(raw)}")
            break
    if not raw:
        print("[fail] GetSystemFirmwareTable 返回 0")
        return 1

    with open(out_path, "wb") as f:
        f.write(raw)
    print(f"[ok] 已写入 {out_path}")

    declared = struct.unpack_from("<I", raw, 4)[0]
    print(f"[hdr] method={raw[0]} major={raw[1]} minor={raw[2]} dmi_rev={raw[3]} len={declared} actual={len(raw) - 8}")
    tbl = raw[8:]

    print("\n=== dmidecode 风格遍历（正确基准）===")
    for e in walk_dmidecode(tbl):
        if e[0] != "OK":
            print("  !!", e)
            continue
        _, pos, typ, length, handle, strs = e
        print(f"  @{pos:04X} type={typ:3d} len={length:3d} h={handle:#06x} strs={strs}")

    print("\n=== 当前 Rust 遍历（对照）===")
    for e in walk_current_rust(tbl):
        if e[0] != "OK":
            print("  !!", e)
            continue
        _, pos, typ, length, consumed = e
        print(f"  @{pos:04X} type={typ:3d} len={length:3d} consumed={consumed}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
