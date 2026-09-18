# -*- coding: utf-8 -*-
# 模拟鼠标悬停在某张卡上，PrintWindow 截一张带 hover 状态的图，用于核验"动感"
import ctypes, os, sys, time, subprocess
from ctypes import wintypes

EXE = r"D:\project\WorkBuddy\内置工具\WinSuiteX\WinSuiteX.exe"
OUT = r"D:\project\WorkBuddy\内置工具\WinSuiteX\win_menu_hover.bmp"
CLS = "WinSuiteXDialog"

user32 = ctypes.windll.user32
gdi32  = ctypes.windll.gdi32

# 卡几何（与 click_capture.py 一致）：TAB 起点 25+1+96*1+48=169，y=69
# 卡片 x0 ≈ 25, y0 ≈ 169, 卡宽 ≈ 552, 卡高 ≈ 100 左右（每张卡之间有间距）
# 第一张卡中心点：约 (300, 220)
HOVER_X, HOVER_Y = 300, 220

hwnd = user32.FindWindowW(CLS, None)
if not hwnd:
    subprocess.Popen([EXE], cwd=os.path.dirname(EXE))
    for _ in range(40):
        hwnd = user32.FindWindowW(CLS, None)
        if hwnd: break
        time.sleep(0.5)
print("hwnd:", hex(hwnd))

# 先发一次点击切到 menu 页（如果还在别的页）
# menu tab 中心：(169, 69)
user32.PostMessageW(hwnd, 0x0200, 0, (HOVER_Y << 16) | 169)   # WM_MOUSEMOVE
user32.PostMessageW(hwnd, 0x0201, 1, (69 << 16) | 169)        # WM_LBUTTONDOWN
user32.PostMessageW(hwnd, 0x0202, 0, (69 << 16) | 169)        # WM_LBUTTONUP
time.sleep(0.5)

# 移动鼠标到第一张卡中心（触发卡 hover）
user32.PostMessageW(hwnd, 0x0200, 0, (HOVER_Y << 16) | HOVER_X)
time.sleep(0.8)  # 给重绘留时间

# 然后捕获窗口
class RECT(ctypes.Structure):
    _fields_ = [("left", ctypes.c_long), ("top", ctypes.c_long),
                ("right", ctypes.c_long), ("bottom", ctypes.c_long)]
class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [("biSize", wintypes.DWORD), ("biWidth", wintypes.LONG), ("biHeight", wintypes.LONG),
                ("biPlanes", wintypes.WORD), ("biBitCount", wintypes.WORD), ("biCompression", wintypes.DWORD),
                ("biSizeImage", wintypes.DWORD), ("biXPelsPerMeter", wintypes.LONG), ("biYPelsPerMeter", wintypes.LONG),
                ("biClrUsed", wintypes.DWORD), ("biBiClrImportant", wintypes.DWORD)]

rc = RECT(); user32.GetWindowRect(hwnd, ctypes.byref(rc))
W = rc.right - rc.left; H = rc.bottom - rc.top
hdcScreen = user32.GetDC(0)
hdcMem = gdi32.CreateCompatibleDC(hdcScreen)
bi = BITMAPINFOHEADER()
bi.biSize = ctypes.sizeof(BITMAPINFOHEADER)
bi.biWidth = W; bi.biHeight = -H
bi.biPlanes = 1; bi.biBitCount = 32; bi.biCompression = 0
ppv = ctypes.c_void_p()
hbmp = gdi32.CreateDIBSection(hdcScreen, ctypes.byref(bi), 0, ctypes.byref(ppv), None, 0)
gdi32.SelectObject(hdcMem, hbmp)

user32.PrintWindow(hwnd, hdcMem, 2)
buf = ctypes.string_at(ppv, W*H*4)

# 保存为 24bit BMP
row = (W*3 + 3) & ~3
data = bytearray()
for y in range(H-1, -1, -1):
    base = y*W*4
    line = bytearray()
    for x in range(W):
        i = base + x*4
        line += bytes((buf[i], buf[i+1], buf[i+2]))
    line += b'\x00' * (row - W*3); data += line
off = 54
hdr = b'BM' + (off+len(data)).to_bytes(4,'little') + b'\x00\x00\x00\x00' + off.to_bytes(4,'little')
dib = (40).to_bytes(4,'little') + W.to_bytes(4,'little') + H.to_bytes(4,'little') \
    + (1).to_bytes(2,'little') + (24).to_bytes(2,'little') + (0).to_bytes(4,'little') \
    + len(data).to_bytes(4,'little') + (2835).to_bytes(4,'little')*2 + (0).to_bytes(4,'little')*2
open(OUT,'wb').write(hdr+dib+bytes(data))
print("saved", OUT)

# 核验：扫描第一张卡区域，看 hover 阴影（更黑）和 accent 边框是否出现
# 第一张卡大致范围：x=25..577, y=170..260 (粗略估算)
# 检查阴影：卡下边缘（y=260-280）应该比平时更暗
# 检查边框：卡上边缘附近应该出现 accent 颜色（绿色：52,211,153）
from collections import Counter
c = Counter()
for i in range(0, len(buf), 4):
    c[(buf[i+2], buf[i+1], buf[i])] += 1
print("TOP:", c.most_common(5))