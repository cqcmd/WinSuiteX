#!/usr/bin/env python
"""GDI 面板冒烟截图：抓 WinSuiteXGDI 窗口的客户区并存PNG。

用途：本机 VM 无 D3D12，但 GDI 是软件渲染，能直接验证"界面真的画出来了"。
只用 ctypes 调 GDI（BitBlt 抓屏），不依赖 PIL 的ImageGrab（那走的是另一套 API）。
"""

import ctypes
import sys
from ctypes import wintypes

from PIL import Image

user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32

SRCCOPY = 0x00CC0020
DIB_RGB_COLORS = 0


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", wintypes.DWORD),
        ("biWidth", ctypes.c_long),
        ("biHeight", ctypes.c_long),
        ("biPlanes", wintypes.WORD),
        ("biBitCount", wintypes.WORD),
        ("biCompression", wintypes.DWORD),
        ("biSizeImage", wintypes.DWORD),
        ("biXPelsPerMeter", ctypes.c_long),
        ("biYPelsPerMeter", ctypes.c_long),
        ("biClrUsed", wintypes.DWORD),
        ("biClrImportant", wintypes.DWORD),
    ]


class BITMAPINFO(ctypes.Structure):
    _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", wintypes.DWORD * 3)]


def find_window(class_name: str, title: str | None = None):
    return user32.FindWindowW(class_name, title)


def capture(hwnd: int, out_path: str) -> tuple[int, int]:
    rect = wintypes.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    w = rect.right - rect.left
    h = rect.bottom - rect.top
    if w <= 0 or h <= 0:
        raise SystemExit(f"窗口尺寸异常: {w}x{h}")

    win_dc = user32.GetWindowDC(hwnd)
    mem_dc = gdi32.CreateCompatibleDC(win_dc)

    bmi = BITMAPINFO()
    bmi.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    bmi.bmiHeader.biWidth = w
    bmi.bmiHeader.biHeight = -h  # 负高度 = 自上而下
    bmi.bmiHeader.biPlanes = 1
    bmi.bmiHeader.biBitCount = 32
    bmi.bmiHeader.biCompression = 0  # BI_RGB

    bits = ctypes.c_void_p()
    bmp = gdi32.CreateDIBSection(
        mem_dc, ctypes.byref(bmi), DIB_RGB_COLORS, ctypes.byref(bits), None, 0
    )
    if not bmp:
        raise SystemExit("CreateDIBSection 失败")
    old = gdi32.SelectObject(mem_dc, bmp)
    gdi32.BitBlt(mem_dc, 0, 0, w, h, win_dc, 0, 0, SRCCOPY)

    buf = ctypes.string_at(bits, w * h * 4)
    img = Image.frombuffer("RGBA", (w, h), buf, "raw", "BGRA", 0, 1).convert("RGB")
    img.save(out_path)

    gdi32.SelectObject(mem_dc, old)
    gdi32.DeleteObject(bmp)
    gdi32.DeleteDC(mem_dc)
    user32.ReleaseDC(hwnd, win_dc)
    return w, h


def pixel_stats(path: str) -> dict:
    """统计非背景像素占比——用来证明"界面真的画了东西"而不是一片空白。"""
    img = Image.open(path).convert("RGB")
    colors = img.getcolors(maxcolors=1 << 20)
    total = img.width * img.height
    uniq = len(colors)
    top = sorted(colors, reverse=True)[:5]
    return {
        "size": img.size,
        "unique_colors": uniq,
        "top_colors": [(c, rgb) for c, rgb in top],
        "total_px": total,
    }


if __name__ == "__main__":
    out = sys.argv[1] if len(sys.argv) > 1 else "gdi_shot.png"
    hwnd = find_window("WinSuiteXGDI")
    if not hwnd:
        raise SystemExit("找不到 WinSuiteXGDI 窗口")
    user32.SetForegroundWindow(hwnd)
    w, h = capture(hwnd, out)
    st = pixel_stats(out)
    print(f"saved={out} size={w}x{h} unique_colors={st['unique_colors']}")
    for cnt, rgb in st["top_colors"]:
        print(f"  {cnt:>8} px  rgb{rgb}")