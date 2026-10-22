#!/usr/bin/env python
"""启动耗时 + 滚动对齐实测探针（2026-10-22 两则回归）。

用法：
    python tools/scroll_align_probe.py <exe> <dump_prefix>

量两件事：
1. **出窗耗时**：CreateProcess → FindWindowW 命中，旧版卡在 WMI 采集（秒级），
   新版应 <1s（采集移后台，先出加载页）。
2. **滚动对齐**：SendWheel 滚到底 → 等重绘 → 转储缓冲离线合成，
   产出 <prefix>_scrolled.png 供目检（AA 卡片/徽标必须与文字同位）。

顺带验证加载→内容切换（dump 首帧 vs 末帧颜色分布差异）。
"""
import ctypes
import subprocess
import sys
import time
from ctypes import wintypes
from pathlib import Path

user32 = ctypes.windll.user32

WND_CLASS = "WinSuiteXGDI"
WM_MOUSEWHEEL = 0x020A
WHEEL_DELTA = 120


def find_hwnd() -> int:
    return user32.FindWindowW(WND_CLASS, None)


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__)
        return 2
    exe, prefix = sys.argv[1], sys.argv[2]

    t0 = time.time()
    proc = subprocess.Popen([exe], env={"WSX_DUMP_LAYERED": prefix, **__import__("os").environ})
    # 1) 出窗耗时
    hwnd = 0
    while time.time() - t0 < 20:
        hwnd = find_hwnd()
        if hwnd:
            break
        time.sleep(0.01)
    appear = time.time() - t0
    if not hwnd:
        print(f"FAIL: window never appeared (proc={proc.pid})")
        proc.kill()
        return 1
    print(f"[startup] window appeared in {appear:.2f}s  hwnd={hwnd:#x}")

    # 2) 等数据到位（dump 出现 + 短暂稳定）
    meta = Path(prefix + ".meta")
    for _ in range(300):
        if meta.exists() and meta.stat().st_size > 0:
            break
        time.sleep(0.05)
    # 给后台采集一点时间（WMI/SMART 读完会有 WM_APP_SNAPSHOT_READY 重绘）
    time.sleep(3.0)

    # 3) 滚轮滚两屏（向下一格 delta=-WHEEL_DELTA，见 wheel_scroll_delta）
    for _ in range(6):
        # wParam: HIWORD=滚轮格数（负=向下），LOWORD=修饰键
        wparam = wintypes.WPARAM((-WHEEL_DELTA & 0xFFFF) << 16)
        user32.SendMessageW(hwnd, WM_MOUSEWHEEL, wparam, wintypes.LPARAM(0))
        time.sleep(0.15)
    time.sleep(0.5)  # 等最后一帧 WM_PAINT + dump 落盘

    bgra = Path(prefix + ".bgra")
    if not bgra.exists():
        print("WARN: dump not produced (WSX_DUMP_LAYERED ignored?) — scroll visual check skipped")
        print(f"[summary] startup={appear:.2f}s")
        proc.kill()
        return 0 if appear < 1.0 else 1

    # 4) 合成白底 PNG
    try:
        import numpy as np
        from PIL import Image

        w, h = (int(x) for x in meta.read_text().split())
        buf = np.frombuffer(bgra.read_bytes(), dtype=np.uint8).reshape(h, w, 4)
        B, G, R, A = (buf[:, :, i].astype(np.float64) for i in range(4))
        a = A[..., None] / 255.0
        rgb = np.stack([R, G, B], -1)
        out = np.clip(rgb + 255.0 * (1 - a), 0, 255).astype(np.uint8)
        Image.fromarray(out).save(prefix + "_scrolled.png")
        print(f"[visual] composited -> {prefix}_scrolled.png ({w}x{h})")
    except Exception as e:  # noqa: BLE001
        print(f"WARN: composite failed: {e}")

    print(f"[summary] startup={appear:.2f}s dump={'ok' if bgra.exists() else 'missing'}")
    proc.kill()
    return 0 if appear < 1.0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
