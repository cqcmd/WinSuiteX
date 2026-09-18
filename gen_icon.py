import struct
import os

# 生成多尺寸 ICO（32x32 + 64x64），"滑块"图标象征个性化/设置


def render(size):
    W = H = size
    pixels = bytearray(W * H * 4)          # BGRA, 初始全透明
    mask = bytearray(((W + 31) // 32) * 4 * H)

    def set_px(x, y, b, g, r, a=255):
        if x < 0 or x >= W or y < 0 or y >= H:
            return
        yy = H - 1 - y                      # BMP 自底向上
        idx = (yy * W + x) * 4
        pixels[idx:idx + 4] = bytes([b, g, r, a])

    def fill_rounded_rect(x0, y0, x1, y1, rad, color):
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                cx = min(max(x, x0 + rad), x1 - rad)
                cy = min(max(y, y0 + rad), y1 - rad)
                if (x - cx) ** 2 + (y - cy) ** 2 > rad * rad:
                    continue
                set_px(x, y, *color)

    # 背景：蓝色圆角方块 (R=0x00 G=0x78 B=0xD7 -> BGRA 0xD7,0x78,0x00)
    blue = (0xD7, 0x78, 0x00, 0xFF)
    fill_rounded_rect(1, 1, W - 2, H - 2, size // 4, blue)

    # 三条白色"滑块"
    white = (255, 255, 255, 255)
    lineW = int(size * 0.56)
    cx = W // 2
    x0 = cx - lineW // 2
    x1 = cx + lineW // 2
    top = int(size * 0.25)
    step = max(1, (H - 2 * top) // 2)
    for i in range(3):
        y = top + i * step
        for xx in range(x0, x1 + 1):
            for yy in range(y - max(1, size // 16), y + max(1, size // 16) + 1):
                set_px(xx, yy, *white)
        kx = x0 + (i + 1) * (lineW // 4)
        kr = max(2, size // 16)
        for dy in range(-kr, kr + 1):
            for dx in range(-kr, kr + 1):
                if dx * dx + dy * dy <= kr * kr:
                    set_px(kx + dx, y + dy, *white)

    return pixels, mask


def make_icon(path, sizes=(32, 64)):
    images = []
    for s in sizes:
        px, mk = render(s)
        bmp_header = struct.pack('<IiiHHIIiiII', 40, s, s * 2, 1, 32,
                                 0, len(px) + len(mk), 0, 0, 0, 0)
        images.append(bmp_header + px + mk)

    header = struct.pack('<HHH', 0, 1, len(images))
    entries = b''
    offset = 6 + 16 * len(images)
    for s, img in zip(sizes, images):
        entries += struct.pack('<BBBBHHII', s, s, 0, 0, 1, 32,
                                len(img), offset)
        offset += len(img)

    with open(path, 'wb') as f:
        f.write(header)
        f.write(entries)
        for img in images:
            f.write(img)


if __name__ == '__main__':
    d = os.path.dirname(os.path.abspath(__file__))
    out = os.path.join(d, 'app.ico')
    make_icon(out)
    print('Icon created:', out)
