#!/usr/bin/env python
"""单实例验证：第二次启动必须激活已有实例并静默退出，进程数恒为 1。

场景：
  1. 首启 → 进程数 1，窗口存在
  2. 再启 → 进程数仍 1（新进程自己退出），窗口仍在
  3. 最小化已有窗口后重启 → 窗口被还原（IsIconic=False）
"""
import ctypes
import subprocess
import sys
import time

user32 = ctypes.windll.user32
WND_CLASS = "WinSuiteXGDI"
WM_SYSCOMMAND = 0x0112
SC_MINIMIZE = 0xF020
EXE = r"dist\winsuitex-gui-gdi.exe"


def procs():
    # tasklist 在中文 Windows 输出 GBK，按 utf-8 解码会抛 UnicodeDecodeError（stdout 变 None）。
    out = subprocess.run(
        ["tasklist", "/FI", "IMAGENAME eq winsuitex-gui-gdi.exe"],
        capture_output=True, text=True, encoding="gbk", errors="ignore",
    ).stdout or ""
    return [ln for ln in out.splitlines() if ln.lower().startswith("winsuitex-gui-gdi")]


def hwnd():
    return user32.FindWindowW(WND_CLASS, None)


def launch(wait=True):
    p = subprocess.Popen([EXE], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    if wait:
        try:
            p.wait(timeout=15)  # 二次启动的实例应自己退出
        except subprocess.TimeoutExpired:
            pass
    return p


def kill_all():
    subprocess.run("taskkill /IM winsuitex-gui-gdi.exe /F", capture_output=True)
    time.sleep(0.6)


if __name__ == "__main__":
    kill_all()
    ok = True

    # 1) 首启
    launch(wait=False)
    time.sleep(2.5)
    n1 = len(procs())
    h1 = hwnd()
    print(f"[1] 首启: 进程数={n1} 窗口={'有' if h1 else '无'}")
    ok &= n1 == 1 and h1 != 0

    # 2) 二次启动：应激活已有实例并退出，进程数保持 1
    launch(wait=True)
    time.sleep(1.5)
    n2 = len(procs())
    h2 = hwnd()
    print(f"[2] 再启: 进程数={n2} 窗口={'有' if h2 else '无'}（期望 1 / 有）")
    ok &= n2 == 1 and h2 != 0

    # 3) 最小化后重启：窗口应被还原
    if h2:
        user32.SendMessageW(h2, WM_SYSCOMMAND, SC_MINIMIZE, 0)
        time.sleep(0.8)
        iconic_before = bool(user32.IsIconic(h2))
        launch(wait=True)
        time.sleep(1.2)
        iconic_after = bool(user32.IsIconic(hwnd()))
        n3 = len(procs())
        print(f"[3] 最小化后重启: 重启前最小化={iconic_before} 重启后最小化={iconic_after} 进程数={n3}")
        ok &= iconic_before and not iconic_after and n3 == 1

    print("PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)
