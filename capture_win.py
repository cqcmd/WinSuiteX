#!/usr/bin/env python3
# -*- coding: utf-8 -*-
# Launch WinSuiteX.exe and capture ITS WINDOW via PrintWindow (works on headless/black sessions).
import ctypes, os, sys, time, subprocess
from ctypes import wintypes

EXE = r"D:\project\WorkBuddy\内置工具\WinSuiteX\WinSuiteX.exe"
OUT = r"D:\project\WorkBuddy\内置工具\WinSuiteX\win_shot.bmp"
CLS = "WinSuiteXDialog"

user32 = ctypes.windll.user32
gdi32  = ctypes.windll.gdi32
shell32= ctypes.windll.shell32
kernel32 = ctypes.windll.kernel32

user32.FindWindowW.restype = wintypes.HWND
user32.FindWindowW.argtypes = [wintypes.LPCWSTR, wintypes.LPCWSTR]

def log(*a): print(*a, flush=True)

def is_admin():
    try:
        return bool(shell32.IsUserAnAdmin())
    except Exception:
        return False

log("admin:", is_admin())

# 1) launch
launched = False
try:
    p = subprocess.Popen([EXE], cwd=os.path.dirname(EXE))
    log("Popen ok pid", p.pid)
    launched = True
except OSError as e:
    log("Popen failed:", e)

if not launched:
    verb = "open" if is_admin() else "runas"
    r = shell32.ShellExecuteW(None, verb, EXE, None, os.path.dirname(EXE), 1)
    log("ShellExecuteW", verb, "->", r)
    if int(r) <= 32:
        log("LAUNCH FAILED rc=", r); sys.exit(2)

# 2) find window
hwnd = 0
for i in range(40):
    hwnd = user32.FindWindowW(CLS, None)
    if hwnd:
        break
    time.sleep(0.5)

if not hwnd:
    log("WINDOW NOT FOUND after 20s (class=%s)" % CLS)
    sys.exit(3)

log("hwnd:", hex(hwnd), "visible:", user32.IsWindowVisible(hwnd))

class RECT(ctypes.Structure):
    _fields_ = [("left", ctypes.c_long), ("top", ctypes.c_long),
                ("right", ctypes.c_long), ("bottom", ctypes.c_long)]
rc = RECT()
user32.GetWindowRect(hwnd, ctypes.byref(rc))
w = rc.right - rc.left
h = rc.bottom - rc.top
log("rect:", rc.left, rc.top, w, h)

user32.SetForegroundWindow(hwnd)
time.sleep(1.2)

class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [("biSize", wintypes.DWORD), ("biWidth", wintypes.LONG), ("biHeight", wintypes.LONG),
                ("biPlanes", wintypes.WORD), ("biBitCount", wintypes.WORD), ("biCompression", wintypes.DWORD),
                ("biSizeImage", wintypes.DWORD), ("biXPelsPerMeter", wintypes.LONG), ("biYPelsPerMeter", wintypes.LONG),
                ("biClrUsed", wintypes.DWORD), ("biClrImportant", wintypes.DWORD)]
class BITMAPINFO(ctypes.Structure):
    _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", wintypes.DWORD*3)]

hdcScreen = user32.GetDC(0)
hdcMem = gdi32.CreateCompatibleDC(hdcScreen)
bi = BITMAPINFO()
bi.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
bi.bmiHeader.biWidth = w
bi.bmiHeader.biHeight = -h   # top-down
bi.bmiHeader.biPlanes = 1
bi.bmiHeader.biBitCount = 32
bi.bmiHeader.biCompression = 0
ppv = ctypes.c_void_p()
hbmp = gdi32.CreateDIBSection(hdcScreen, ctypes.byref(bi), 0, ctypes.byref(ppv), None, 0)
old = gdi32.SelectObject(hdcMem, hbmp)

def nonblack_bytes():
    buf = ctypes.string_at(ppv, w*h*4)
    nz = sum(1 for i in range(0, len(buf), 4) if buf[i] or buf[i+1] or buf[i+2])
    return buf, nz

# Try PW_RENDERFULLCONTENT(2) then fallback 0
best = None
for flag in (2, 0, 1):
    user32.PrintWindow(hwnd, hdcMem, flag)
    buf, nz = nonblack_bytes()
    log("PrintWindow flag=%d nonblack_px=%d / %d" % (flag, nz, w*h))
    if nz > (w*h)//100:
        best = (flag, buf)
        break

if best is None:
    log("ALL PRINTWINDOW FLAGS RETURNED ~BLACK")
    flag, buf = 2, ctypes.string_at(ppv, w*h*4)
else:
    flag, buf = best

# save 24-bit BMP (bottom-up, BGR)
def save_bmp24(path, w, h, bgra):
    row = (w*3 + 3) & ~3
    data = bytearray()
    for y in range(h-1, -1, -1):
        base = y*w*4
        line = bytearray()
        for x in range(w):
            i = base + x*4
            line += bytes((bgra[i], bgra[i+1], bgra[i+2]))  # B,G,R
        line += b'\x00' * (row - w*3)
        data += line
    off = 54
    hdr = b'BM' + (off+len(data)).to_bytes(4,'little') + b'\x00\x00\x00\x00' + off.to_bytes(4,'little')
    dib = (40).to_bytes(4,'little') + w.to_bytes(4,'little') + h.to_bytes(4,'little') \
        + (1).to_bytes(2,'little') + (24).to_bytes(2,'little') + (0).to_bytes(4,'little') \
        + len(data).to_bytes(4,'little') + (2835).to_bytes(4,'little')*2 + (0).to_bytes(4,'little')*2
    open(path,'wb').write(hdr+dib+bytes(data))
    log("saved", path, os.path.getsize(path), "bytes  flag=", flag)

save_bmp24(OUT, w, h, buf)

# report top colors to sanity check
from collections import Counter
c = Counter()
for i in range(0, len(buf), 4):
    c[(buf[i+2], buf[i+1], buf[i])] += 1   # RGB
log("TOP COLORS(RGB):", c.most_common(6))
