#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""真机窗口行为回归：任务栏图标 + 任务栏单击最小化/恢复。

## 为什么需要这个脚本
frameless 窗口（`WS_POPUP`）把系统边框/标题栏全去掉了，代价是**窗口与任务栏的
两套契约都得自己补**。这两类 bug 不会崩、不会有日志，只能靠行为断言兜住：

1. **任务栏 / Alt+Tab 没图标**：`WNDCLASSEXW` 用 `..Default::default()` 时
   `hIcon` / `hIconSm` 都是 0，窗口类没有图标 → 任务栏按钮空白。
2. **任务栏单击不最小化/恢复**：`DefWindowProc` 处理 `WM_SYSCOMMAND` 前会
   **先查窗口样式位**——`SC_MINIMIZE` 要求 `WS_MINIMIZEBOX`、`SC_MAXIMIZE` 要求
   `WS_MAXIMIZEBOX`。frameless 窗口原本没带这两个样式位，于是系统命令被静默忽略。
   任务栏单击发的正是 `WM_SYSCOMMAND`，所以"点了没反应"。

## 断言方式
不是看代码，而是**真的给窗口发 `WM_SYSCOMMAND`** 并观察 `IsIconic` 变化——
与用户点任务栏按钮走的是同一条系统路径。

用法：`python tools/check_window_behavior.py [exe路径]`
退出码：0=全部通过；1=有断言失败；2=环境/启动问题。
"""

import ctypes
import os
import subprocess
import sys
import time
from ctypes import wintypes

u = ctypes.windll.user32

# ---- 常量 ----
WM_SYSCOMMAND = 0x0112
SC_MINIMIZE = 0xF020
SC_MAXIMIZE = 0xF030
SC_RESTORE = 0xF120
WM_GETICON = 0x007F
WM_CLOSE = 0x0010
ICON_SMALL = 0
ICON_BIG = 1
GWL_STYLE = -16
WS_MINIMIZEBOX = 0x00020000
WS_MAXIMIZEBOX = 0x00010000
SPI_GETWORKAREA = 0x0030
WND_CLASS = "WinSuiteXGDI"

u.FindWindowW.restype = wintypes.HWND
u.FindWindowW.argtypes = [wintypes.LPCWSTR, wintypes.LPCWSTR]
u.SendMessageW.restype = ctypes.c_longlong
u.SendMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
u.GetWindowLongPtrW.restype = ctypes.c_longlong
u.GetWindowLongPtrW.argtypes = [wintypes.HWND, ctypes.c_int]
u.IsIconic.restype = wintypes.BOOL
u.IsWindowVisible.restype = wintypes.BOOL
u.IsZoomed.restype = wintypes.BOOL


def wait_window(timeout=15.0):
    """轮询等待窗口出现（进程启动 + 硬件采集需要时间）。"""
    end = time.time() + timeout
    while time.time() < end:
        hwnd = u.FindWindowW(WND_CLASS, None)
        if hwnd:
            return hwnd
        time.sleep(0.2)
    return None


def main() -> int:
    exe = sys.argv[1] if len(sys.argv) > 1 else os.path.join("dist", "winsuitex-gui-gdi.exe")
    if not os.path.exists(exe):
        print(f"[FATAL] 找不到 exe: {exe}")
        return 2

    # 单实例：先清掉残留进程，否则 FindWindowW 会找到**旧二进制**的窗口，
    # 测出来的还是修复前的行为（最容易被忽略的假通过）。
    subprocess.run(
        ["taskkill", "/IM", "winsuitex-gui-gdi.exe", "/F"],
        capture_output=True,
        text=True,
    )
    time.sleep(0.5)

    print(f"[启动] {exe}")
    proc = subprocess.Popen([exe], cwd=os.path.dirname(os.path.abspath(exe)) or None)
    hwnd = None
    try:
        hwnd = wait_window()
        if not hwnd:
            print("[FATAL] 窗口未出现（启动失败？）")
            return 2
        print(f"[OK] 窗口已创建 hwnd={hwnd:#x}")

        failures = []

        # ---- 1. 窗口样式位：DefWindowProc 处理系统命令的前置条件 ----
        style = u.GetWindowLongPtrW(hwnd, GWL_STYLE)
        has_min = bool(style & WS_MINIMIZEBOX)
        has_max = bool(style & WS_MAXIMIZEBOX)
        print(f"[样式] WS_MINIMIZEBOX={has_min} WS_MAXIMIZEBOX={has_max}")
        if not has_min:
            failures.append("缺 WS_MINIMIZEBOX → 任务栏单击最小化会被 DefWindowProc 忽略")
        if not has_max:
            failures.append("缺 WS_MAXIMIZEBOX → 双击标题栏/最大化命令会被忽略")

        # ---- 2. 窗口图标（任务栏 / Alt+Tab）----
        ico_small = u.SendMessageW(hwnd, WM_GETICON, ICON_SMALL, 0)
        ico_big = u.SendMessageW(hwnd, WM_GETICON, ICON_BIG, 0)
        print(f"[图标] ICON_SMALL={ico_small:#x} ICON_BIG={ico_big:#x}")
        if not ico_small:
            failures.append("窗口小图标(ICON_SMALL)为空 → 任务栏按钮无图标")
        if not ico_big:
            failures.append("窗口大图标(ICON_BIG)为空 → Alt+Tab 无图标")

        # ---- 3. 任务栏单击：最小化 ----
        u.SendMessageW(hwnd, WM_SYSCOMMAND, SC_MINIMIZE, 0)
        time.sleep(0.8)
        iconic = bool(u.IsIconic(hwnd))
        print(f"[最小化] IsIconic={iconic}")
        if not iconic:
            failures.append("发 SC_MINIMIZE 后窗口未进入最小化态 → 任务栏单击无效")

        # ---- 4. 任务栏单击：恢复 ----
        u.SendMessageW(hwnd, WM_SYSCOMMAND, SC_RESTORE, 0)
        time.sleep(0.8)
        iconic2 = bool(u.IsIconic(hwnd))
        vis = bool(u.IsWindowVisible(hwnd))
        print(f"[恢复] IsIconic={iconic2} Visible={vis}")
        if iconic2:
            failures.append("发 SC_RESTORE 后仍处于最小化态")
        if not vis:
            failures.append("恢复后窗口不可见")

        # ---- 5. 最大化应落在工作区（不盖任务栏）----
        u.SendMessageW(hwnd, WM_SYSCOMMAND, SC_MAXIMIZE, 0)
        time.sleep(0.8)
        zoomed = bool(u.IsZoomed(hwnd))
        print(f"[最大化] IsZoomed={zoomed}")
        if not zoomed:
            failures.append("发 SC_MAXIMIZE 后未进入最大化态")
        if zoomed:
            r = wintypes.RECT()
            u.GetWindowRect(hwnd, ctypes.byref(r))
            wa = wintypes.RECT()
            u.SystemParametersInfoW(SPI_GETWORKAREA, 0, ctypes.byref(wa), 0)
            win_h = r.bottom - r.top
            work_h = wa.bottom - wa.top
            print(f"[几何] 窗口高={win_h} 工作区高={work_h}")
            if abs(win_h - work_h) > 2:
                failures.append(
                    f"最大化高度({win_h}) != 工作区高({work_h}) → 盖住任务栏或没铺满"
                )
        u.SendMessageW(hwnd, WM_SYSCOMMAND, SC_RESTORE, 0)
        time.sleep(0.3)

        if failures:
            print("\nRESULT: FAIL")
            for f in failures:
                print(f"  - {f}")
            return 1
        print("\nRESULT: PASS")
        return 0
    finally:
        if hwnd:
            try:
                u.SendMessageW(hwnd, WM_CLOSE, 0, 0)
            except Exception:
                pass
        time.sleep(0.3)
        if proc.poll() is None:
            proc.kill()


if __name__ == "__main__":
    sys.exit(main())
