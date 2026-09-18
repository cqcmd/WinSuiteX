#!/usr/bin/env python3
# Pure-python 24-bit BMP -> PNG (no PIL). Uses only stdlib zlib/struct.
import sys, zlib, struct

def bmp_to_png(src, dst):
    with open(src, 'rb') as f:
        data = f.read()
    assert data[:2] == b'BM', "not a BMP"
    bf_off = struct.unpack_from('<I', data, 10)[0]
    # DIB header
    bi_size = struct.unpack_from('<I', data, 14)[0]
    bi_w = struct.unpack_from('<i', data, 18)[0]
    bi_h = struct.unpack_from('<i', data, 22)[0]
    bi_bpp = struct.unpack_from('<H', data, 28)[0]
    bi_comp = struct.unpack_from('<I', data, 30)[0]
    assert bi_bpp == 24 and bi_comp == 0, f"unsupported bmp bpp={bi_bpp} comp={bi_comp}"
    top_down = bi_h < 0
    h = abs(bi_h)
    w = bi_w
    row_bytes = (w * 3 + 3) & ~3  # padded to 4
    px = data[bf_off:]
    # build raw scanlines (filter byte 0 + RGB)
    raw = bytearray()
    for y in range(h):
        src_y = (h - 1 - y) if not top_down else y
        row_start = src_y * row_bytes
        row = px[row_start:row_start + w * 3]
        raw.append(0)  # filter type 0
        for x in range(w):
            i = x * 3
            raw += bytes((row[i+2], row[i+1], row[i]))  # BGR -> RGB
    comp = zlib.compress(bytes(raw), 9)

    def chunk(typ, body):
        c = typ + body
        return struct.pack('>I', len(body)) + c + struct.pack('>I', zlib.crc32(c) & 0xffffffff)

    png = b'\x89PNG\r\n\x1a\n'
    png += chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0))
    png += chunk(b'IDAT', comp)
    png += chunk(b'IEND', b'')
    with open(dst, 'wb') as f:
        f.write(png)
    print(f"OK {src} -> {dst}  ({w}x{h}, {len(png)} bytes)")

if __name__ == '__main__':
    bmp_to_png(sys.argv[1], sys.argv[2])
