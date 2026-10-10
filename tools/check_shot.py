#!/usr/bin/env python
"""校验截图按钮产出的 PNG 是不是"真有内容"（不靠肉眼看）。

真机曾报"截图没有内容"——根因是 GDI 的 COLORREF 只写 24 位色、alpha 字节恒 0，
GDI+ 按 32bppARGB 编码得到**全透明图**：RGB 一分不少，看图器打开却是空白。
本脚本把这条钉死，改编码链路后跑一遍即可确认没退化。

用法：
  python tools/check_shot.py                 # 校验桌面最新一张 WinSuiteX_硬件截图*.png
  python tools/check_shot.py <file.png>      # 指定文件
"""
import collections
import glob
import os
import sys

from PIL import Image


def check(path: str) -> bool:
    im = Image.open(path).convert("RGBA")
    px = list(im.getdata())
    alpha = collections.Counter(p[3] for p in px)
    rgb = collections.Counter((p[0], p[1], p[2]) for p in px)

    ok = True
    print(f"file : {path}")
    print(f"size : {im.size}")

    # 1) alpha 必须全不透明
    opaque_ratio = alpha.get(255, 0) / len(px)
    print(f"alpha: 255 占比 {opaque_ratio:.4f}  分布前3 {alpha.most_common(3)}")
    if opaque_ratio < 0.999:
        print("  ✗ 存在透明像素——看图器会显示空白（GDI+ ARGB 编码问题）")
        ok = False
    else:
        print("  ✓ 全部不透明")

    # 2) 颜色多样性：一张有内容的界面图不该只有一两种颜色
    print(f"颜色 : {len(rgb)} 种，最常见 {rgb.most_common(2)}")
    if len(rgb) < 20:
        print("  ✗ 颜色种类过少，疑似空白/纯色图")
        ok = False
    else:
        print("  ✓ 颜色丰富")

    # 3) 文字墨水：白字（255,255,255）应占一定比例（界面有大量白色文字）
    white = rgb.get((255, 255, 255), 0) / len(px)
    print(f"白字 : 占比 {white:.4f}")
    if white < 0.005:
        print("  ✗ 几乎没有白色文字像素，内容可能没画上去")
        ok = False
    else:
        print("  ✓ 有文字墨水")

    return ok


if __name__ == "__main__":
    if len(sys.argv) > 1:
        target = sys.argv[1]
    else:
        pat = os.path.join(
            os.path.expanduser("~"), "Desktop", "WinSuiteX_硬件截图*.png"
        )
        files = sorted(glob.glob(pat), key=os.path.getmtime)
        if not files:
            raise SystemExit("桌面上没有 WinSuiteX_硬件截图*.png，先点一次截图按钮")
        target = files[-1]
    sys.exit(0 if check(target) else 1)
