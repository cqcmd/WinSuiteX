#!/usr/bin/env python3
# -*- coding: utf-8 -*-
# Click a tab in WinSuiteXDialog, then PrintWindow-capture the window.
import ctypes, os, sys, time, subprocess
from ctypes import wintypes

EXE = r"D:\project\WorkBuddy\内置工具\WinSuiteX\WinSuiteX.exe"
BASE = r"D:\project\WorkBuddy\内置工具\WinSuiteX"
CLS = "WinSuiteXDialog"

user32 = ctypes.windll.user32
gdi32  = ctypes.windll.gdi32
shell32= ctypes.windll.shell32

user32.FindWindowW.restype = wintypes.HWND
user32.FindWindowW.argtypes = [wintypes.LPCWSTR, wintypes.LPCWSTR]

# tab geometry (client coords): x0 = MARGIN(24)+SHADOW_PAD(1)+t*TAB_W(96); y = TITLE_H(46)+SHADOW_PAD(1)..+TAB_H(44)
TABS = {"hw": (25 + 96*0 + 48, 69), "menu": (25 + 96*1 + 48, 69), "disk": (25 + 96*2 + 48, 69)}

def find_or_launch():
    hwnd = user32.FindWindowW(CLS, None)
    if hwnd:
        print("existing hwnd", hex(hwnd)); return hwnd
    print("launching...")
    try:
        subprocess.Popen([EXE], cwd=os.path.dirname(EXE))
    except OSError as e:
        print("Popen fail", e); sys.exit(2)
    for _ in range(40):
        hwnd = user32.FindWindowW(CLS, None)
        if hwnd: return hwnd
        time.sleep(0.5)
    print("NOT FOUND"); sys.exit(3)

class RECT(ctypes.Structure):
    _fields_ = [("left", ctypes.c_long), ("top", ctypes.c_long),
                ("right", ctypes.c_long), ("bottom", ctypes.c_long)]
class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [("biSize", wintypes.DWORD), ("biWidth", wintypes.LONG), ("biHeight", wintypes.LONG),
                ("biPlanes", wintypes.WORD), ("biBitCount", wintypes.WORD), ("biCompression", wintypes.DWORD),
                ("biSizeImage", wintypes.DWORD), ("biXPelsPerMeter", wintypes.LONG), ("biYPelsPerMeter", wintypes.LONG),
                ("biClrUsed", wintypes.DWORD), ("biClrImportant", wintypes.DWORD)]
class BITMAPINFO(ctypes.Structure):
    _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", wintypes.DWORD*3)]

W = H = 0
hdcScreen = hdcMem = hbmp = old = None
ppv = ctypes.c_void_p()

def setup(hwnd):
    global W, H, hdcScreen, hdcMem, hbmp, old, ppv
    rc = RECT(); user32.GetWindowRect(hwnd, ctypes.byref(rc))
    W = rc.right - rc.left; H = rc.bottom - rc.top
    hdcScreen = user32.GetDC(0)
    hdcMem = gdi32.CreateCompatibleDC(hdcScreen)
    bi = BITMAPINFO()
    bi.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    bi.bmiHeader.biWidth = W; bi.bmiHeader.biHeight = -H
    bi.bmiHeader.biPlanes = 1; bi.bmiHeader.biBitCount = 32; bi.bmiHeader.biCompression = 0
    hbmp = gdi32.CreateDIBSection(hdcScreen, ctypes.byref(bi), 0, ctypes.byref(ppv), None, 0)
    old = gdi32.SelectObject(hdcMem, hbmp)
    print("size", W, H)

def save_bmp24(path, bgra):
    row = (W*3 + 3) & ~3
    data = bytearray()
    for y in range(H-1, -1, -1):
        base = y*W*4
        line = bytearray()
        for x in range(W):
            i = base + x*4
            line += bytes((bgra[i], bgra[i+1], bgra[i+2]))
        line += b'\x00' * (row - W*3); data += line
    off = 54
    hdr = b'BM' + (off+len(data)).to_bytes(4,'little') + b'\x00\x00\x00\x00' + off.to_bytes(4,'little')
    dib = (40).to_bytes(4,'little') + W.to_bytes(4,'little') + H.to_bytes(4,'little') \
        + (1).to_bytes(2,'little') + (24).to_bytes(2,'little') + (0).to_bytes(4,'little') \
        + len(data).to_bytes(4,'little') + (2835).to_bytes(4,'little')*2 + (0).to_bytes(4,'little')*2
    open(path,'wb').write(hdr+dib+bytes(data))
    print("saved", path, os.path.getsize(path), "bytes")

def capture(hwnd, out):
    user32.PrintWindow(hwnd, hdcMem, 2)
    buf = ctypes.string_at(ppv, W*H*4)
    save_bmp24(out, buf)
    from collections import Counter
    c = Counter()
    for i in range(0, len(buf), 4):
        c[(buf[i+2], buf[i+1], buf[i])] += 1
    print("TOP:", c.most_common(5))

def click(hwnd, name):
    x, y = TABS[name]
    lp = (y << 16) | (x & 0xFFFF)
    user32.PostMessageW(hwnd, 0x0201, 1, lp)   # WM_LBUTTONDOWN, MK_LBUTTON
    time.sleep(0.08)
    user32.PostMessageW(hwnd, 0x0202, 0, lp)   # WM_LBUTTONUP
    time.sleep(1.0)
    print("clicked", name, "at", x, y)

if __name__ == '__main__':
    hwnd = find_or_launch()
    setup(hwnd)
    target = sys.argv[1] if len(sys.argv) > 1 else "menu"
    if target != "none":
        click(hwnd, target)
    capture(hwnd, os.path.join(BASE, f"win_{target}.bmp"))
