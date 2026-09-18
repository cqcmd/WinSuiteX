# -*- coding: utf-8 -*-
"""启动 WinSuiteX_glass4.exe 并采集硬证据：窗口样式位、窗口/客户区尺寸。
截图尽力而为（RDP 下 ImageGrab 偶发失败，不影响硬指标判定）。
无论成功失败都会终止子进程，避免遗留孤儿窗口干扰后续测试。"""
import ctypes
import ctypes.wintypes as wt
import os
import subprocess
import sys
import time

try:
    from PIL import ImageGrab
    HAVE_PIL = True
except Exception:
    HAVE_PIL = False

BASE = r"D:\project\WorkBuddy\内置工具\WinSuiteX"
EXE = os.path.join(BASE, "WinSuiteX_glass4.exe")
WIN_W, WIN_H = 600, 718
GWL_STYLE = -16
WS_CAPTION = 0x00C00000
WS_THICKFRAME = 0x00040000
WS_SYSMENU = 0x00080000
WS_MINIMIZEBOX = 0x00020000

u = ctypes.windll.user32
proc = None
try:
    proc = subprocess.Popen([EXE])
    print("launched pid=%d" % proc.pid)
    time.sleep(5)

    hwnd = u.FindWindowW("GlassShowcase", None)
    print("hwnd = %s" % hwnd)
    if not hwnd:
        print("!! 未找到 GlassShowcase 窗口")
        # 子进程在 finally 中终止
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
    ok = (cw == WIN_W and ch == WIN_H and not (st & WS_CAPTION))
    print("无边框生效 = 客户区(%dx%d) == 目标(%dx%d) 且无 CAPTION : %s" % (cw, ch, WIN_W, WIN_H, ok))

    if HAVE_PIL:
        try:
            ImageGrab.grab().save(os.path.join(BASE, "shot_glass4.png"))
            print("saved shot_glass4.png")
            u.SetCursorPos(wr.left + 300, wr.top + 46 + 96 + 16 + 54)
            time.sleep(1)
            ImageGrab.grab().save(os.path.join(BASE, "shot_glass4_hover.png"))
            print("saved shot_glass4_hover.png")
        except Exception as e:
            print("!! 截图失败（不影响指标判定）: %s" % e)
    else:
        print("!! 无 PIL，跳过截图")

    sys.exit(0 if ok else 1)
finally:
    if proc is not None:
        try:
            proc.terminate()
            time.sleep(0.6)
            if proc.poll() is None:
                proc.kill()
        except Exception:
            pass
        print("terminated")
