# -*- coding: utf-8 -*-
# 纯 ctypes 截取主屏到 24-bit BMP（不依赖 PIL），供亲自目视校验。
import ctypes, sys

user32 = ctypes.windll.user32
gdi32  = ctypes.windll.gdi32

SRCCOPY = 0x00CC0020
BI_RGB  = 0
out = sys.argv[1] if len(sys.argv) > 1 else r"D:/project/WorkBuddy/内置工具/WinSuiteX/verify_shot.bmp"

cx = user32.GetSystemMetrics(0)
cy = user32.GetSystemMetrics(1)
print("screen logical size: %dx%d" % (cx, cy))
if cx == 0 or cy == 0:
    print("NO_DISPLAY")
    sys.exit(2)

hdc = user32.GetDC(0)
if not hdc:
    print("NO_HDC")
    sys.exit(3)

mem = gdi32.CreateCompatibleDC(hdc)
bmp = gdi32.CreateCompatibleBitmap(hdc, cx, cy)
gdi32.SelectObject(mem, bmp)
gdi32.BitBlt(mem, 0, 0, cx, cy, hdc, 0, 0, SRCCOPY)

# 取位图信息
class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", ctypes.c_uint32),
        ("biWidth", ctypes.c_int32),
        ("biHeight", ctypes.c_int32),
        ("biPlanes", ctypes.c_uint16),
        ("biBitCount", ctypes.c_uint16),
        ("biCompression", ctypes.c_uint32),
        ("biSizeImage", ctypes.c_uint32),
        ("biXPelsPerMeter", ctypes.c_int32),
        ("biYPelsPerMeter", ctypes.c_int32),
        ("biClrUsed", ctypes.c_uint32),
        ("biClrImportant", ctypes.c_uint32),
    ]

bih = BITMAPINFOHEADER()
bih.biSize = ctypes.sizeof(BITMAPINFOHEADER)
bih.biWidth = cx
bih.biHeight = -cy   # top-down
bih.biPlanes = 1
bih.biBitCount = 24
bih.biCompression = BI_RGB

pixels = ctypes.create_string_buffer(cx * cy * 3)
gdi32.GetDIBits(mem, bmp, 0, cy, pixels, ctypes.byref(bih), 0)

# 写 BMP 文件
def u16(v): return ctypes.c_uint16(v).value.to_bytes(2, "little")
def u32(v): return ctypes.c_uint32(v).value.to_bytes(4, "little")

with open(out, "wb") as f:
    f.write(b"BM")
    filesize = 14 + ctypes.sizeof(BITMAPINFOHEADER) + len(pixels)
    f.write(u32(filesize))
    f.write(u32(0))
    f.write(u32(14 + ctypes.sizeof(BITMAPINFOHEADER)))
    f.write(int(bih.biSize).to_bytes(4, "little"))
    f.write(int(bih.biWidth).to_bytes(4, "little"))
    f.write(int(bih.biHeight).to_bytes(4, "little", signed=True))   # negative = top-down
    f.write(u16(1))
    f.write(u16(24))
    f.write(u32(0))
    f.write(u32(len(pixels)))
    f.write(u32(2835))
    f.write(u32(2835))
    f.write(u32(0))
    f.write(u32(0))
    f.write(pixels.raw)

gdi32.DeleteObject(bmp)
gdi32.DeleteDC(mem)
user32.ReleaseDC(0, hdc)
print("SAVED", out, len(pixels), "bytes")
