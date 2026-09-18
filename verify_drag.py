# -*- coding: utf-8 -*-
"""确定性验证无边框窗口的"可拖动性"（lParam 驱动，不依赖真实光标）。

为什么这样设计：RDP 下 SetCursorPos/GetCursorPos 坐标对不上，依赖真实光标的
mouse_event 方案非确定性失灵。本脚本直接对 hwnd 发消息：
  WM_NCLBUTTONDOWN(lParam=屏幕坐标) -> 记录起始点屏幕坐标 + 窗口左上角
  WM_MOUSEMOVE(lParam=客户坐标)     -> 换算屏幕坐标后按位移重定位窗口
  WM_LBUTTONUP                      -> 结束拖拽
handler 全程只读取消息 lParam，真实鼠标与测试消息走同一逻辑。
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

WM_NCLBUTTONDOWN = 0x00A1
WM_MOUSEMOVE = 0x0200
WM_LBUTTONUP = 0x0202
HTCAPTION = 2

u = ctypes.windll.user32

def mk(x, y):
    return ctypes.c_ulong(((y & 0xffff) << 16) | (x & 0xffff)).value

proc = None
try:
    proc = subprocess.Popen([EXE])
    time.sleep(4)

    hwnd = u.FindWindowW("GlassShowcase", None)
    if not hwnd:
        print("!! 未找到 GlassShowcase 窗口")
        sys.exit(2)

    def rect():
        r = wt.RECT()
        u.GetWindowRect(hwnd, ctypes.byref(r))
        return (r.left, r.top, r.right, r.bottom)

    before = rect()
    print("拖动前 window rect = %s" % (before,))

    # 标题栏内、避开最小化/关闭按钮的空白处：窗口内相对坐标 (300, 23)
    start = (before[0] + 300, before[1] + 23)
    DX, DY = 60, 40

    # 1) 在标题空白处按下左键（lParam = 屏幕坐标）
    u.SendMessageW(hwnd, WM_NCLBUTTONDOWN, HTCAPTION, mk(start[0], start[1]))
    time.sleep(0.1)

    # 2) 分 6 步移动；每步把"期望屏幕坐标"换算成相对当前窗口的客户坐标再发送
    for k in range(1, 7):
        desX = start[0] + DX * k // 6
        desY = start[1] + DY * k // 6
        rl, rt, _, _ = rect()
        cx = desX - rl
        cy = desY - rt
        u.SendMessageW(hwnd, WM_MOUSEMOVE, 0, mk(cx, cy))

    time.sleep(0.1)
    # 3) 抬起左键结束拖拽
    u.SendMessageW(hwnd, WM_LBUTTONUP, 0, 0)
    time.sleep(0.3)

    after = rect()
    print("拖动后 window rect = %s" % (after,))

    dx, dy = after[0] - before[0], after[1] - before[1]
    print("实际位移 = (%d, %d)   期望 ≈ (%d, %d)" % (dx, dy, DX, DY))

    # 判别：读取窗口属性 GlassDragMoves（handler 每次拖拽移动自增并通过 SetProp 暴露）。
    # >0 才证明真的是"自绘拖拽分支"在动，而非系统默认 caption 拖拽兜底。
    u.GetPropW.argtypes = [wt.HWND, wt.LPCWSTR]
    u.GetPropW.restype = ctypes.c_void_p
    moves = u.GetPropW(hwnd, "GlassDragMoves")
    moves = int(moves) if moves else 0
    print("拖拽分支执行计数 g_dragMoves = %d (期望 >0)" % moves)

    ok = abs(dx - DX) <= 12 and abs(dy - DY) <= 12 and moves > 0
    print("--- 判定 ---")
    print("标题栏可拖动 = %s" % ok)
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
