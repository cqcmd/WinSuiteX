import ctypes, ctypes.wintypes, struct, zlib, sys

user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32

def find_window():
    res = []
    @ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_void_p)
    def enum(hwnd, lp):
        if user32.IsWindowVisible(hwnd):
            buf = ctypes.create_unicode_buffer(256)
            user32.GetClassNameW(hwnd, buf, 256)
            if buf.value == "WinSuiteXDialog":
                res.append(hwnd)
        return True
    user32.EnumWindows(enum, 0)
    return res[0] if res else None

hwnd = find_window()
r = ctypes.wintypes.RECT()
user32.GetClientRect(hwnd, ctypes.byref(r))
W, H = r.right - r.left, r.bottom - r.top

hdc = user32.GetDC(hwnd)
hdcMem = gdi32.CreateCompatibleDC(hdc)
bmi = ctypes.create_string_buffer(40)
struct.pack_into('I', bmi, 0, 40)
struct.pack_into('i', bmi, 4, W)
struct.pack_into('i', bmi, 8, -H)
struct.pack_into('H', bmi, 12, 1)
struct.pack_into('H', bmi, 14, 32)
pv = ctypes.c_void_p()
hBmp = gdi32.CreateDIBSection(hdc, bmi, 0, ctypes.byref(pv), None, 0)
gdi32.SelectObject(hdcMem, hBmp)
user32.PrintWindow(hwnd, hdcMem, 2)
buf = ctypes.string_at(pv, W * H * 4)

# build RGBA scanlines
raw = bytearray()
for y in range(H):
    raw.append(0)  # filter type 0
    for x in range(W):
        o = (y * W + x) * 4
        raw += bytes((buf[o+2], buf[o+1], buf[o]))

def chunk(typ, data):
    c = struct.pack('>I', len(data)) + typ + data
    crc = zlib.crc32(typ + data) & 0xffffffff
    return c + struct.pack('>I', crc)

png = b''
png += b'\x89PNG\r\n\x1a\n'
png += chunk(b'IHDR', struct.pack('>IIBBBBB', W, H, 8, 2, 0, 0, 0))  # colortype 2 = RGB
png += chunk(b'IDAT', zlib.compress(bytes(raw), 9))
png += chunk(b'IEND', b'')

out = sys.argv[1] if len(sys.argv) > 1 else "WinSuiteX_current.png"
with open(out, "wb") as f:
    f.write(png)
print("wrote", out, W, "x", H)
