#!/usr/bin/env python
"""交互验证：给 WinSuiteXGDI 窗口发点击/滚轮消息并抓图。

用法:
  python gdi_interact.py click <out.png> <x> <y>   # 发一次左键点击后抓图
  python gdi_interact.py wheel <out.png> <notches>  # 发滚轮后抓图（负=向下）
  python gdi_interact.py seq   <out_prefix>          # 依次切三页签并各抓一张

坐标是**客户区坐标**。

注意：`SendMessageW` 是**直送`wnd_proc`**，绕过系统，所以 lparam 必须已经是客户区
坐标 —— 真实鼠标消息的系统转换我们拿不到，得自己算准（我一开始传屏幕坐标，
结果每次点击都偏了窗口原点，三个页签全点成了最后一个，排查半天才发现）。
"""

import ctypes
import sys
import time


user32 = ctypes.windll.user32

WM_LBUTTONDOWN = 0x0201
WM_LBUTTONUP = 0x0202
WM_MOUSEWHEEL = 0x020A
WND_CLASS = "WinSuiteXGDI"


def find() -> int:
    hwnd = user32.FindWindowW(WND_CLASS, None)
    if not hwnd:
        raise SystemExit(f"找不到 {WND_CLASS} 窗口")
    return hwnd


def click(hwnd: int, x: int, y: int) -> None:
    # lparam 直接放**客户区坐标**：SendMessage 直送 wnd_proc，系统不做转换。
    lp = (y << 16) | (x & 0xFFFF)
    user32.SendMessageW(hwnd, WM_LBUTTONDOWN, 1, lp)
    user32.SendMessageW(hwnd, WM_LBUTTONUP, 0, lp)


def wheel(hwnd: int, notches: int) -> None:
    # wparam 高 16 位是**有符号格数**（应用按 `as i16` 解读），低 16 位按键状态。
    # 旧写法 `((notches * DELTA) & 0xFFFF) << 16` 把 delta(±120/格) 塞进高位，
    # 应用把 ±120 当成 ±120 格 → 一滚就钳到 0 或 max，中程态永远测不到
    # （第十九轮发现的工具债，导致十八轮残影排查绕了远路）。
    wp = (ctypes.c_int16(notches).value & 0xFFFF) << 16
    lp = (300 << 16) | 300
    user32.SendMessageW(hwnd, WM_MOUSEWHEEL, wp, lp)


def shot(hwnd: int, out: str) -> None:
    import importlib.util

    spec = importlib.util.spec_from_file_location(
        "gdi_shot", __file__.replace("gdi_interact", "gdi_shot")
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    user32.SetForegroundWindow(hwnd)
    w, h = mod.capture(hwnd, out)
    st = mod.pixel_stats(out)
    print(f"saved={out} size={w}x{h} unique={st['unique_colors']}")


if __name__ == "__main__":
    mode = sys.argv[1]
    hwnd = find()
    user32.SetForegroundWindow(hwnd)
    if mode == "click":
        click(hwnd, int(sys.argv[3]), int(sys.argv[4]))
        time.sleep(0.5)
        shot(hwnd, sys.argv[2])
    elif mode == "wheel":
        wheel(hwnd, int(sys.argv[3]))
        time.sleep(0.5)
        shot(hwnd, sys.argv[2])
    elif mode == "seq":
        # 页签条 y = body_pad + TITLE_H + 26 ≈ 16+44+26
        for i, name in enumerate(["hw", "menu", "disk"]):
            click(hwnd, 16 + 24 + 96 * i + 48, 16 + 44 + 22)
            time.sleep(0.7)
            out = f"{sys.argv[2]}_{name}.png"
            shot(hwnd, out)
    else:
        raise SystemExit("mode 必须是 click / wheel / seq")