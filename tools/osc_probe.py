# -*- coding: utf-8 -*-
"""磁盘页窗高震荡探针（2026-10-22）。

用法: python tools/_osc_probe.py <exe_path> <dump_prefix>
阶段:
  1) 启动 exe（带 WSX_DUMP_LAYERED），等窗口出现
  2) 紧循环采样 GetWindowRect 高度 1.5s（基线 = 硬件页）
  3) SendMessageW(WM_LBUTTONUP) 点「磁盘信息」页签 → 等 0.5s
  4) 紧循环采样高度 3s + 记录 dump 文件重写次数（震荡 = 高度多值 + 重写不停）
"""
import ctypes
import os
import subprocess
import sys
import time
from ctypes import wintypes

user32 = ctypes.windll.user32
WM_LBUTTONUP = 0x0202
CLS = "WinSuiteXGDI"


def win_h(hwnd):
    r = wintypes.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(r))
    return r.bottom - r.top


def tight_sample(hwnd, seconds):
    hist = {}
    t_end = time.perf_counter() + seconds
    n = 0
    while time.perf_counter() < t_end:
        h = win_h(hwnd)
        hist[h] = hist.get(h, 0) + 1
        n += 1
    return hist, n


def main():
    exe, prefix = sys.argv[1], sys.argv[2]
    for ext in (".bgra", ".meta", ".probe", ".err"):
        try:
            os.remove(prefix + ext)
        except OSError:
            pass

    env = os.environ.copy()
    env["WSX_DUMP_LAYERED"] = prefix
    proc = subprocess.Popen([exe], env=env)
    hwnd = 0
    for _ in range(100):
        time.sleep(0.05)
        hwnd = user32.FindWindowW(CLS, None)
        if hwnd:
            break
    if not hwnd:
        proc.kill()
        raise SystemExit("window not found")
    time.sleep(1.5)  # 等首帧与数据采集稳定

    dpi = user32.GetDpiForWindow(hwnd)

    def s(v):
        return max(1, (v * dpi + 48) // 96)

    # 「磁盘信息」= 第 3 个页签（index 2）：x0 = SHADOW_PAD + MARGIN，宽 TAB_W
    x = s(24) + s(96) * 2 + s(96) // 2
    y = s(44) + s(44) // 2
    print(f"hwnd={hwnd:#x} dpi={dpi} click=({x},{y})")

    meta = prefix + ".meta"
    mtime0 = os.path.getmtime(meta) if os.path.exists(meta) else None

    hist1, n1 = tight_sample(hwnd, 1.5)
    print(f"[baseline] heights={dict(sorted(hist1.items()))} samples={n1}")

    lparam = (y << 16) | x
    user32.SendMessageW(hwnd, WM_LBUTTONUP, 0, lparam)
    time.sleep(0.5)

    # 震荡判定：紧采样 3s 高度分布 + 尾部 1s dump 重写次数
    hist2, n2 = tight_sample(hwnd, 3.0)
    print(f"[disk-tab] heights={dict(sorted(hist2.items()))} samples={n2}")

    rewrites = 0
    if os.path.exists(meta):
        last = os.path.getmtime(meta)
        t_end = time.perf_counter() + 1.0
        while time.perf_counter() < t_end:
            m = os.path.getmtime(meta)
            if m != last:
                rewrites += 1
                last = m
    h_values = sorted(hist2.keys())
    verdict = "OSCILLATING" if len(h_values) > 1 or rewrites > 3 else "STABLE"
    print(f"[verdict] {verdict} (distinct_heights={len(h_values)} dump_rewrites_in_1s={rewrites})")

    proc.kill()
    return 0


if __name__ == "__main__":
    sys.exit(main())
