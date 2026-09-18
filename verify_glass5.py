# -*- coding: utf-8 -*-
"""启动 WinSuiteX_glass5.exe 并采集硬证据：窗口样式位、窗口/客户区尺寸、截图。
用途：验证 WS_OVERLAPPEDWINDOW + WM_NCCALCSIZE(return 0 不改 rgrc[0]) 无边框是否真的生效
（客户区 == 窗口 且无 WS_CAPTION）。"""
import ctypes
import ctypes.wintypes as wt
import os
import subprocess
import sys
import time

from PIL import ImageGrab

BASE = r"D:\project\WorkBuddy\内置工具\WinSuiteX"
EXE = os.path.join(BASE, "WinSuiteX_glass5.exe")
WIN_W, WIN_H = 600, 718
GWL_STYLE = -16
WS_CAPTION = 0x00C00000
WS_THICKFRAME = 0x00040000
WS_SYSMENU = 0x00080000
WS_MINIMIZEBOX = 0x00020000

u = ctypes.windll.user32

proc = subprocess.Popen([EXE])
print("launched pid=%d" % proc.pid)
time.sleep(5)

hwnd = u.FindWindowW("GlassShowcaseOvl", None)
print("hwnd = %s" % hwnd)
if not hwnd:
    print("!! 未找到 GlassShowcaseOvl 窗口")
    proc.terminate()
    sys.exit(2)

wr = wt.RECT()
u.GetWindowRect(hwnd, ctypes.byref(wr))
cr = wt.RECT()
u.GetClientRect(hwnd, ctypes.byref(cr))
st = u.GetWindowLongW(hwnd, GWL_STYLE)

ww, wh = wr.right - wr.left, wr.bottom - wr.top
cw, ch = cr.right - cr.left, cr.bottom - cr.top
print("window rect  = (%d,%d)-(%d,%d)  size=%dx%d" % (wr.left, wr.top, wr.right, wr.bottom, ww, wh))
print("client size  = %dx%d" % (cw, ch))
print("style        = 0x%08X" % st)
print("  WS_CAPTION    = %s" % bool(st & WS_CAPTION))
print("  WS_THICKFRAME = %s" % bool(st & WS_THICKFRAME))
print("  WS_SYSMENU    = %s" % bool(st & WS_SYSMENU))
print("  WS_MINIMIZEBOX= %s" % bool(st & WS_MINIMIZEBOX))
print("--- 判定 ---")
print("无边框生效 = 客户区(%dx%d) == 目标(%dx%d) 且无 CAPTION : %s"
      % (cw, ch, WIN_W, WIN_H, (cw == WIN_W and ch == WIN_H and not (st & WS_CAPTION))))

shot = os.path.join(BASE, "shot_glass5.png")
ImageGrab.grab().save(shot)
print("saved %s" % shot)

# hover 第一张卡片，验证 hover 态绘制（也便于人工看阴影/边框变化）
u.SetCursorPos(wr.left + 300, wr.top + 46 + 96 + 16 + 54)
time.sleep(1)
shot2 = os.path.join(BASE, "shot_glass5_hover.png")
ImageGrab.grab().save(shot2)
print("saved %s" % shot2)

time.sleep(1)
proc.terminate()
print("terminated")
