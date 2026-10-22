#!/usr/bin/env python
"""UI 方案横向对比度量（颜值视角的客观项）。

对若干张"同一页面、不同方案"的截图，输出四个维度：
1. 色板：卡片/渐变/底栏的主色、平均饱和度、色相集中度（越集中越"克制"）
2. 层级：卡片与所在渐变的 CIE L* 差（大 = 卡片"浮"得清楚；深色／亮色两套色板只有 L* 可比）
   3. 可读性：白字 vs 卡片底色的 WCAG 对比度
4. 边缘：软过渡像素占比（圆角 AA 的客观指标）+ 唯一色数

用法: python tools/ui_scheme_compare.py <page> <name1>=<prefix1> <name2>=<prefix2> ...
输出: 表格 + 三方并排拼图 <page>_cmp.png
"""
import math
import sys
from collections import Counter

from PIL import Image


def lum(c) -> float:
    def f(v: int) -> float:
        v /= 255.0
        return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4

    return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2])


def contrast(a, b) -> float:
    la, lb = lum(a), lum(b)
    hi, lo = max(la, lb), min(la, lb)
    return (hi + 0.05) / (lo + 0.05)


def lstar(c) -> float:
    """CIE L*（感知均匀明度）。深色卡片/背景必须用这个——相对亮度 Y 在暗区
    会被压平，跨"深中性/亮饱和"两种色板比较 ΔY 会失真，ΔL* 才可比。"""
    y = lum(c)
    return 116.0 * (y ** (1.0 / 3.0)) - 16.0 if y > 0.008856 else 903.3 * y


def sat(c) -> float:
    mx, mn = max(c), min(c)
    if mx == 0:
        return 0.0
    return (mx - mn) / mx


def hue(c) -> float:
    r, g, b = [v / 255.0 for v in c]
    mx, mn = max(r, g, b), min(r, g, b)
    d = mx - mn
    if d == 0:
        return 0.0
    if mx == r:
        h = ((g - b) / d) % 6
    elif mx == g:
        h = (b - r) / d + 2
    else:
        h = (r - g) / d + 4
    return h * 60.0


def analyze(path: str) -> dict:
    im = Image.open(path).convert("RGB")
    px = im.load()
    w, h = im.size
    cnt = Counter(im.getdata())
    common = [c for c, _ in cnt.most_common(200)]

    card = common[1]  # 次常见色 = 卡片面（最常见是底栏/大面积背景）
    # 渐变取样：右侧留白列（卡片右缘外），避开卡片
    gcol = min(w - 3, 578)
    grad = [px[gcol, y] for y in range(int(h * 0.35), int(h * 0.75), 4)]
    grad_avg = tuple(sum(c[i] for c in grad) // len(grad) for i in range(3))

    # 平均饱和度（按像素频加权的前 200 色，忽略噪声）
    tot = sum(n for _, n in cnt.most_common(200))
    s_avg = sum(sat(c) * n for c, n in cnt.most_common(200)) / tot
    # 色相集中度：把色相分 12 桶，取前三桶占比（越高越统一）
    buckets = Counter(int(hue(c) // 30) for c, n in cnt.most_common(200) for _ in [0])
    hconc = sum(n for _, n in buckets.most_common(3)) / sum(buckets.values())

    # 边缘软过渡占比（AA 指标）+ 局部色差统计
    hard = soft = total = 0
    for y in range(1, h - 1, 2):
        for x in range(1, w - 1, 2):
            c = px[x, y]
            right, down = px[x + 1, y], px[x, y + 1]
            d = max(
                abs(c[0] - right[0]) + abs(c[1] - right[1]) + abs(c[2] - right[2]),
                abs(c[0] - down[0]) + abs(c[1] - down[1]) + abs(c[2] - down[2]),
            )
            total += 1
            if d > 60:
                hard += 1
            elif 1 <= d <= 20:
                soft += 1

    return {
        "path": path,
        "size": (w, h),
        "card": card,
        "grad": grad_avg,
        "card_sat": sat(card),
        "avg_sat": s_avg,
        "hue_conc": hconc,
        "card_l": lstar(card),
        "grad_l": lstar(grad_avg),
        "d_lum": abs(lstar(card) - lstar(grad_avg)),
        "contrast": contrast((255, 255, 255), card),
        "soft_pct": 100.0 * soft / total,
        "hard_pct": 100.0 * hard / total,
        "colors": len(cnt),
        "img": im,
    }


def main() -> None:
    page = sys.argv[1]
    pairs = [a.split("=", 1) for a in sys.argv[2:]]
    rows = []
    for name, prefix in pairs:
        rows.append((name, analyze(f"{prefix}_{page}.png")))

    print(f"=== 页面 {page} ===")
    hdr = f"{'方案':<12}{'卡片色':<18}{'卡片饱和度':>10}{'整体饱和':>10}{'色相集中':>10}"
    print(hdr + f"{chr(0x394) + 'L*层级':>9}{'白字对比':>10}{'软过渡%':>10}{'色数':>9}")
    for name, a in rows:
        print(
            f"{name:<12}{str(a['card']):<18}{a['card_sat']:>10.2f}{a['avg_sat']:>10.2f}"
            f"{a['hue_conc']:>10.2f}{a['d_lum']:>9.1f}{a['contrast']:>10.2f}"
            f"{a['soft_pct']:>10.2f}{a['colors']:>9}"
        )

    # 并排拼图
    ims = [a["img"] for _, a in rows]
    hh = max(i.height for i in ims)
    scale = min(1.0, 300 / max(i.width for i in ims))
    ims = [i.resize((int(i.width * scale), int(i.height * scale)), Image.LANCZOS) for i in ims]
    hh = max(i.height for i in ims)
    total_w = sum(i.width for i in ims) + 10 * (len(ims) - 1)
    mont = Image.new("RGB", (total_w, hh), (18, 18, 22))
    x = 0
    for i in ims:
        mont.paste(i, (x, 0))
        x += i.width + 10
    out = f"{rows[0][1]['path'].rsplit('_', 1)[0]}_cmp_{page}.png"
    mont.save(out)
    print(f"拼图 -> {out}  顺序: {', '.join(n for n, _ in rows)}")


if __name__ == "__main__":
    main()
