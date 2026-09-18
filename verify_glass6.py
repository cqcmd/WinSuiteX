# -*- coding: utf-8 -*-
# 量化核验 glass4 对齐：卡/面板是否从"牛奶白板"变为"紫蓝渐变透出"的玻璃质感。
# 指标（仅统计内部带，排除可能的黑色边距伪影）：
#   - near_white: R,G,B 都 >238 的像素占比（旧版整片白板会很高 ~0.5+；玻璃风应很低）
#   - purple_bleed: B>190 且 R>90 且 G<150 且 B>=R（紫/品红调，渐变透出的直接证据）
#   - interior_avg: 内部带平均 RGB（应为蓝/紫调，而非近白 250,250,250）
import struct, sys

def load_bmp(path):
    with open(path, "rb") as f:
        d = f.read()
    assert d[:2] == b"BM"
    off = struct.unpack("<I", d[10:14])[0]
    w = struct.unpack("<i", d[18:22])[0]
    h = struct.unpack("<i", d[22:26])[0]
    bpp = struct.unpack("<H", d[28:30])[0]
    assert bpp == 24, bpp
    stride = (w * 3 + 3) & ~3
    px = d[off:]
    return w, h, stride, px

def analyze(path):
    w, h, stride, px = load_bmp(path)
    # 内部带：y 12%~88%，x 5%~95%
    y0, y1 = int(h*0.12), int(h*0.88)
    x0, x1 = int(w*0.05), int(w*0.95)
    n = 0
    black = 0
    near_white = 0
    purple = 0
    white_text = 0
    sr = sg = sb = 0
    for y in range(y0, y1):
        base = y * stride
        for x in range(x0, x1):
            i = base + x*3
            b, g, r = px[i], px[i+1], px[i+2]   # BMP 文件按 BGR 存储
            n += 1
            if r == 0 and g == 0 and b == 0:
                black += 1
                continue
            sr += r; sg += g; sb += b
            if r > 238 and g > 238 and b > 238:
                near_white += 1
            if b > 190 and r > 90 and g < 150 and b >= r:
                purple += 1
            if r > 225 and g > 225 and b > 225:
                white_text += 1
    painted = n - black
    def pct(a): return (100.0 * a / painted) if painted else 0.0
    print("== %s ==" % path)
    print("  size=%dx%d  interior_px=%d  black_frac=%.1f%%" % (w, h, n, 100.0*black/n))
    if painted:
        print("  interior_avg_rgb = (%d,%d,%d)" % (sr//painted, sg//painted, sb//painted))
    print("  near_white (old milk-board signature) = %.2f%%" % pct(near_white))
    print("  purple_bleed (gradient showing through) = %.2f%%" % pct(purple))
    print("  white_text (light text pixels) = %.2f%%" % pct(white_text))
    print()

for p in sys.argv[1:]:
    analyze(p)
