#!/usr/bin/env python
"""圆角抗锯齿 A/B 度量：比较基线（无 AA）与超采样版截图的边缘过渡。

指标：
1. 唯一色数（AA 会在圆角弧上产生中间混合色 → 颜色数上升）
2. 局部最大色差直方图：硬边 = 少数大跳变；AA = 跳变被摊成多个小台阶
   → 统计 >60 的"硬跳变像素"占比下降、1~20 的"微过渡像素"占比上升
3. 抽取同一处圆角外缘 24x24 区域，放大 6 倍输出对比 PNG

用法: python tools/aa_edge_compare.py <baseline_prefix> <aa_prefix> <page> <crop_x> <crop_y>
"""
import sys

from PIL import Image

PAGES = ("hw", "menu", "disk")


def load(prefix: str, page: str) -> Image.Image:
    return Image.open(f"{prefix}_{page}.png").convert("RGB")


def metrics(im: Image.Image) -> dict:
    w, h = im.size
    px = im.load()
    hard = soft = 0
    total = 0
    for y in range(1, h - 1):
        for x in range(1, w - 1):
            c = px[x, y]
            right = px[x + 1, y]
            down = px[x, y + 1]
            d = max(
                abs(c[0] - right[0]) + abs(c[1] - right[1]) + abs(c[2] - right[2]),
                abs(c[0] - down[0]) + abs(c[1] - down[1]) + abs(c[2] - down[2]),
            )
            total += 1
            if d > 60:
                hard += 1
            elif 1 <= d <= 20:
                soft += 1
    colors = len(im.getcolors(maxcolors=1 << 22) or [])
    return {
        "colors": colors,
        "hard_pct": 100.0 * hard / total,
        "soft_pct": 100.0 * soft / total,
    }


def zoom(im: Image.Image, x: int, y: int, s: int = 6, n: int = 24) -> Image.Image:
    return im.crop((x, y, x + n, y + n)).resize((n * s, n * s), Image.NEAREST)


def main() -> None:
    base_prefix, aa_prefix, page = sys.argv[1], sys.argv[2], sys.argv[3]
    cx, cy = (int(sys.argv[4]), int(sys.argv[5])) if len(sys.argv) > 5 else (24, 150)
    b, a = load(base_prefix, page), load(aa_prefix, page)
    mb, ma = metrics(b), metrics(a)
    print(f"[{page}] 尺寸 {b.size} / {a.size}")
    print(
        f"  唯一色数   基线 {mb['colors']:>6}  ->  AA {ma['colors']:>6} "
        f"(x{ma['colors'] / max(mb['colors'], 1):.2f})"
    )
    print(
        f"  硬跳变占比 基线 {mb['hard_pct']:>5.2f}%  ->  AA {ma['hard_pct']:>5.2f}% "
        f"({ma['hard_pct'] - mb['hard_pct']:+.2f}pt)"
    )
    print(
        f"  微过渡占比 基线 {mb['soft_pct']:>5.2f}%  ->  AA {ma['soft_pct']:>5.2f}% "
        f"({ma['soft_pct'] - mb['soft_pct']:+.2f}pt)"
    )
    out = f"{aa_prefix}_{page}_corner_ab.png"
    zb, za = zoom(b, cx, cy), zoom(a, cx, cy)
    cmp_img = Image.new("RGB", (zb.width + za.width + 12, zb.height), (30, 30, 30))
    cmp_img.paste(zb, (0, 0))
    cmp_img.paste(za, (zb.width + 12, 0))
    cmp_img.save(out)
    print(f"  圆角放大对比 -> {out} (左=基线 右=AA)")


if __name__ == "__main__":
    main()
