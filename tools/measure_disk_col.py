# -*- coding: utf-8 -*-
"""量 Z.W. 真机截图：磁盘页「盘符」表头 vs 内容行盘符的精确 x 坐标。

背景：二十五轮修过"表头与内容错位"（首列 +20、数值列右对齐），
真机又圈红框反馈盘符列。不再肉眼猜，直接量像素：
  1. 找薄荷选中行（C_SEL_ROW ≈ (216,240,228)）→ 行带 y 范围；
  2. 行带内找深色字素（图标 + "A:"）→ 各簇 x 范围；
  3. 表头带找亮色字素（C_ON_GRAD_DIM 亮灰）→ 「盘符」左缘 x；
  4. 对比：表头左缘 vs 盘符字左缘 vs 图标左缘；
  5. 顺带量详情卡底边与最后一行灰字的相对位置（查裁字）。
"""
from PIL import Image
import sys

img = Image.open(sys.argv[1]).convert("RGB")
W, H = img.size
px = img.load()
print(f"image {W}x{H}")


def lum(p):
    return 0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2]


def close(p, c, tol=18):
    return all(abs(p[i] - c[i]) <= tol for i in range(3))


# ---- 1. 找薄荷选中行 ----
MINT = (216, 240, 228)
rows = []
for y in range(H):
    cnt = sum(1 for x in range(0, W, 2) if close(px[x, y], MINT))
    if cnt > 30:
        rows.append(y)
if not rows:
    print("!! 没找到薄荷选中行")
    sys.exit(1)
ry0, ry1 = min(rows), max(rows)
print(f"selected mint row band: y {ry0}..{ry1} (h={ry1-ry0+1})")

# ---- 2. 选中行内深色字素（C_SEL_ROW_TEXT 深色）----
cols_dark = []
for x in range(0, W):
    cnt = sum(1 for y in range(ry0, ry1 + 1) if lum(px[x, y]) < 120)
    if cnt >= 2:
        cols_dark.append(x)
# 聚簇（间隔>=3 断开）
clusters = []
for x in cols_dark:
    if clusters and x - clusters[-1][1] <= 3:
        clusters[-1][1] = x
    else:
        clusters.append([x, x])
print("dark glyph clusters in selected row (x0..x1):")
for c in clusters[:6]:
    print(f"  {c[0]}..{c[1]} (w={c[1]-c[0]+1})")

# ---- 3. 表头带「盘符」左缘：选中行上方 20~26px 的带内找亮字 ----
hy0, hy1 = ry0 - 26, ry0 - 6
best = None
for x in range(20, 140):
    cnt = sum(1 for y in range(max(hy0, 0), hy1) if lum(px[x, y]) > 150)
    if cnt >= 2:
        best = x
        break
print(f"header '盘符' leftmost bright x: {best}  (band y {hy0}..{hy1})")

# 参考：表头第二列「磁盘」左缘（在 x 90..170 找）
best2 = None
for x in range(95, 200):
    cnt = sum(1 for y in range(max(hy0, 0), hy1) if lum(px[x, y]) > 150)
    if cnt >= 2:
        best2 = x
        break
print(f"header '磁盘' leftmost bright x: {best2}")

# 内容行第二列「—/PD0」左缘（取 C: 行，即选中行下面一行）
row_c_y = ry1 + 34  # 粗略：下一行
if 0 < row_c_y < H:
    for x in range(95, 200):
        ok = any(lum(px[x, yy]) > 170 for yy in range(row_c_y - 8, row_c_y + 8))
        if ok:
            print(f"content col2 glyph near x={x} (row y~{row_c_y})")
            break

# ---- 4. 详情卡区域：找卡底边与最后一行文字 ----
# 详情卡是深 navy 玻璃卡，底部之后是更亮的渐变底/底栏。在 x=300 竖着扫，
# 找 y>470 区间亮度突变的位置，并打印每行的亮度概览（粗略）。
print("\n-- detail card bottom probe (x=300, y 470..520) --")
for y in range(470, min(525, H)):
    row = [px[x, y] for x in range(290, 310, 4)]
    avg = tuple(sum(c[i] for c in row) // len(row) for i in range(3))
    print(f"  y={y}: avg={avg} lum={lum(avg):.0f}")
