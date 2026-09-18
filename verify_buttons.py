# -*- coding: utf-8 -*-
"""确定性验证自绘标题栏按钮的功能正确性（不依赖真实光标，避开 RDP 前台门控）：
  1) 发送 WM_LBUTTONDOWN(最小化按钮) -> IsIconic 应为真，随后还原
  2) 发送 WM_LBUTTONDOWN(关闭按钮)   -> 窗口应被真正销毁（IsWindow == 0）

坐标与源码 GetTitleButtons() 完全一致（窗口内相对坐标）：
  btnSz=36, top=10, right=WIN_W-6=594
  close = [558,594] -> 中心 x=576
  min   = [518,554] -> 中心 x=536
  y 中心 = 10 + 36/2 = 28
无论成败都会终止子进程。
"""
import ctypes
import ctypes.wintypes as wt
import os
import subprocess
import sys
import time

BASE = r"D:\project\WorkBuddy\内置工具\WinSuiteX"
EXE = os.path.join(BASE, "WinSuiteX_glass4.exe")

WM_LBUTTONDOWN = 0x0201

u = ctypes.windll.user32

def mk(x, y):
    return ctypes.c_ulong(((y & 0xffff) << 16) | (x & 0xffff)).value

CLOSE_XY = (576, 28)
MIN_XY = (536, 28)

proc = None
try:
    proc = subprocess.Popen([EXE])
    time.sleep(4)
    hwnd = u.FindWindowW("GlassShowcase", None)
    if not hwnd:
        print("!! 未找到窗口")
        sys.exit(2)
    print("hwnd = %s" % hwnd)

    fails = []

    # --- 1) 最小化按钮 ---
    u.SendMessageW(hwnd, WM_LBUTTONDOWN, 1, mk(*MIN_XY))
    time.sleep(0.6)
    iconic = bool(u.IsIconic(hwnd))
    print("点击最小化后 IsIconic = %s" % iconic)
    if not iconic:
        fails.append("最小化按钮无效")

    # 还原，便于继续测试关闭
    if iconic:
        u.ShowWindow(hwnd, 9)  # SW_RESTORE
        time.sleep(0.8)
        print("还原后 IsIconic = %s" % bool(u.IsIconic(hwnd)))

    # --- 2) 关闭按钮 ---
    u.SendMessageW(hwnd, WM_LBUTTONDOWN, 1, mk(*CLOSE_XY))
    time.sleep(1.0)
    alive = bool(u.IsWindow(hwnd))
    print("点击关闭后 IsWindow = %s (期望 False)" % alive)
    if alive:
        fails.append("关闭按钮无效")

    print("--- 判定 ---")
    if fails:
        print("标题栏按钮功能异常: %s" % "; ".join(fails))
        sys.exit(1)
    print("标题栏按钮功能正常 = True")
    sys.exit(0)
finally:
    if proc is not None:
        try:
            if proc.poll() is None:
                proc.terminate()
                time.sleep(0.6)
            if proc.poll() is None:
                proc.kill()
        except Exception:
            pass
