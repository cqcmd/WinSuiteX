#!/usr/bin/env python
"""测量截图里白字/字段名 vs 卡片底的 WCAG 对比度，验证可读性整改有效。

口径 v2：背景取内容区**众数色**（卡片占大面积，众数即卡面），
而不是"白字最少的行"（那会采到卡片下方的裸渐变，数字失真）。
"""
import sys
from collections import Counter

from PIL import Image


def lum(r, g, b):
    def f(c):
        c = c / 255.0
        return c / 12.92 if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4

    return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)


def contrast(c1, c2):
    l1, l2 = lum(*c1), lum(*c2)
    hi, lo = max(l1, l2), min(l1, l2)
    return (hi + 0.05) / (lo + 0.05)


for path in sys.argv[1:]:
    im = Image.open(path).convert("RGB")
    W, H = im.size
    px = im.load()
    top, bot = 150, H - 100
    left, right = 30, W - 30

    # 背景 = 内容区众数色（卡面）。
    # 磁盘页自适应窗口后页面变矮，薄荷绿选中行 (216,240,228) 可能占成众数，
    # 把"白字 vs 薄荷底"算成 1.2:1 的假失真——统计背景时剔除薄荷绿。
    SEL_MINT = (216, 240, 228)
    cnt = Counter()
    for y in range(top, bot, 3):
        for x in range(left, right, 3):
            c = px[x, y]
            if all(abs(c[i] - SEL_MINT[i]) <= 12 for i in range(3)):
                continue
            cnt[c] += 1
    bg = cnt.most_common(1)[0][0]

    # 白字 = 近纯白像素（>235，避开薄荷绿选中行底色的污染）
    whites = [
        px[x, y]
        for y in range(top, bot, 2)
        for x in range(left, right, 2)
        if px[x, y][0] > 235 and px[x, y][1] > 235 and px[x, y][2] > 240
    ]
    whites.sort(key=lambda p: p[0] + p[1] + p[2])
    txt = whites[len(whites) // 2] if whites else (255, 255, 255)

    # 字段名 = 左标签列（x 40-62）里的中亮淡紫像素
    labels = [
        px[x, y]
        for y in range(top, bot, 1)
        for x in range(40, 62)
        if 170 < px[x, y][0] < 245 and 170 < px[x, y][1] < 245 and 190 < px[x, y][2]
    ]
    labels.sort(key=lambda p: p[0] + p[1] + p[2])
    lab = labels[len(labels) // 2] if labels else txt

    print(path)
    print(f"  bg={bg}  white_text={txt}  对比度={contrast(bg, txt):.2f}:1")
    print(f"  label={lab}  字段名对比度={contrast(bg, lab):.2f}:1")
