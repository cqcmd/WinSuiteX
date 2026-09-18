"""
右键增强 v4.0 layout preview.
Renders the same geometry/colors as main.cpp will use, for self-validation
before compiling C++.
"""
from PIL import Image, ImageDraw, ImageFont

# ---- Layout constants (must stay in sync with main.cpp) ----
WIN_W = 600
TITLE_H = 46
HEADER_H = 96
CARD_H = 108
CARD_GAP = 16
FOOTER_H = 64
MARGIN = 24
TB_BTN_W = 46
BTN_W, BTN_H = 84, 32
BADGE_W, BADGE_H = 76, 22
ICON_SZ = 46
BAR_W = 3
RADIUS = 8
SHADOW_PAD = 1
WIN_RADIUS = 8           # 窗口/卡片圆角（Ant 8px）

# ---- Colors (mirror main.cpp v3.5 tokens · Ant Design 语言移植) ----
C_BG = (245, 245, 245)          # #F5F5F5 colorBgLayout（去蓝调，改中性灰）
C_TB_BG = (250, 250, 250)       # #FAFAFA
C_CARD_BG = (255, 255, 255)     # #FFFFFF colorBgContainer
C_BORDER = (217, 217, 217)      # #D9D9D9 colorBorder
C_DIVIDER = (240, 240, 240)     # #F0F0F0 colorBorderSecondary
C_TITLE = (31, 31, 31)          # #1F1F1F colorText（rgba(0,0,0,.88) 织白）
C_TEXT = (38, 38, 38)           # #262626 Ant gray-9
C_SUBTITLE = (89, 89, 89)       # #595959 colorTextSecondary（rgba .65 织白）
C_DESC = (89, 89, 89)           # #595959 卡片描述（守 WCAG AA）
C_PRIMARY = (9, 88, 217)        # #0958D9 Ant blue-7（守 WCAG AA）
C_PRIMARY_H = (22, 119, 255)    # #1677FF colorPrimaryHover（Ant 悬停变亮）
C_PRIMARY_A = (0, 62, 179)      # #003EB3 激活更深
C_SUCCESS = (82, 196, 26)       # #52C41A Ant 成功绿（语义色·备用）
C_WARNING = (250, 173, 20)      # #FAAD14 Ant 警告琥珀（语义色·备用）
C_ERROR = (255, 77, 79)         # #FF4D4F Ant 错误红（语义色·备用）
C_GREEN = (22, 163, 74)
C_GREEN_DARK = (21, 128, 61)    # #15803D 已启用徽章字色（WCAG AA）
C_GREEN_BG = (223, 242, 230)    # #DFF2E6
C_AMBER = (202, 110, 4)         # #CA6E04
C_AMBER_BG = (254, 244, 225)
C_BTN_SECONDARY = (255, 255, 255)
C_BTN_SECONDARY_H = (250, 250, 250)
C_BTN_BORDER = (217, 217, 217)  # #D9D9D9
C_BTN_BORDER_H = (191, 191, 191)  # #BFBFBF
C_BADGE_NO_BG = (240, 240, 240)   # #F0F0F0
C_BADGE_NO_FG = (89, 89, 89)      # #595959
C_SHADOW = (240, 240, 240)      # #F0F0F0 常态单层极淡阴影
C_SH_HOVER_3 = (232, 232, 232)  # #E8E8E8 hover 远层 偏移8
C_SH_HOVER_2 = (220, 220, 220)  # #DCDCDC hover 中层 偏移4
C_SH_HOVER_1 = (208, 208, 208)  # #D0D0D0 hover 近层 偏移1
C_TB_HOVER = (232, 232, 232)
C_TB_CLOSE_HOVER = (232, 17, 35)
# 窗口层阴影（纯自绘，多层实色模拟柔和向下投影；Ant 语言：中性灰、整体更浅）
C_WIN_SHADOW_BASE = (240, 240, 240)   # #F0F0F0
C_WIN_SHADOW_3 = (232, 232, 232)      # #E8E8E8
C_WIN_SHADOW_2 = (222, 222, 222)      # #DEDEDE
C_WIN_SHADOW_1 = (212, 212, 212)      # #D4D4D4
C_WIN_BORDER = (217, 217, 217)        # #D9D9D9 同 colorBorder

WIN_H = TITLE_H + HEADER_H + CARD_GAP + 4 * (CARD_H + CARD_GAP) + FOOTER_H
MOCK = 30   # 预览图外层留白（用于呈现窗口阴影）

def hex_to_rgb(h):
    h = h.lstrip('#')
    return tuple(int(h[i:i+2], 16) for i in (0, 2, 4))

def rounded_rect(draw, xy, radius, fill=None, outline=None, width=1):
    x1, y1, x2, y2 = xy
    r = min(radius, (x2 - x1) // 2, (y2 - y1) // 2)
    if r <= 0:
        draw.rectangle(xy, fill=fill, outline=outline)
        return
    draw.rectangle([x1 + r, y1, x2 - r, y2], fill=fill, outline=None)
    draw.rectangle([x1, y1 + r, x2, y2 - r], fill=fill, outline=None)
    draw.pieslice([x1, y1, x1 + r * 2, y1 + r * 2], 180, 270, fill=fill)
    draw.pieslice([x2 - r * 2, y1, x2, y1 + r * 2], 270, 360, fill=fill)
    draw.pieslice([x1, y2 - r * 2, x1 + r * 2, y2], 90, 180, fill=fill)
    draw.pieslice([x2 - r * 2, y2 - r * 2, x2, y2], 0, 90, fill=fill)
    if outline and width > 0:
        draw.arc([x1, y1, x1 + r * 2, y1 + r * 2], 180, 270, fill=outline, width=width)
        draw.arc([x2 - r * 2, y1, x2, y1 + r * 2], 270, 360, fill=outline, width=width)
        draw.arc([x1, y2 - r * 2, x1 + r * 2, y2], 90, 180, fill=outline, width=width)
        draw.arc([x2 - r * 2, y2 - r * 2, x2, y2], 0, 90, fill=outline, width=width)
        draw.line([x1 + r, y1, x2 - r, y1], fill=outline, width=width)
        draw.line([x1 + r, y2, x2 - r, y2], fill=outline, width=width)
        draw.line([x1, y1 + r, x1, y2 - r], fill=outline, width=width)
        draw.line([x2, y1 + r, x2, y2 - r], fill=outline, width=width)

# Use a fallback Chinese-capable font path if available, else default.
import sys
font_paths = [
    r"C:\Windows\Fonts\msyh.ttc",
    r"C:\Windows\Fonts\msyh.ttf",
    r"C:\Windows\Fonts\segoeui.ttf",
]
font_path = None
for p in font_paths:
    try:
        ImageFont.truetype(p, 12)
        font_path = p
        break
    except Exception:
        pass
if font_path is None:
    font_path = ImageFont.load_default()

def font(size, bold=False):
    try:
        return ImageFont.truetype(font_path, size)
    except Exception:
        return ImageFont.load_default()

def draw_text(draw, pos, text, color, size, bold=False, anchor="lt"):
    f = font(size, bold)
    draw.text(pos, text, fill=color, font=f, anchor=anchor)

def draw_text_clipped(draw, pos, text, color, size, max_w, bold=False):
    """Draw text left-top, truncating with ... if it exceeds max_w pixels."""
    f = font(size, bold)
    bbox = f.getbbox(text)
    text_w = (bbox[2] - bbox[0]) if bbox else 0
    if text_w <= max_w or not text:
        draw.text(pos, text, fill=color, font=f, anchor="lt")
        return
    ellipsis = "..."
    ebox = f.getbbox(ellipsis)
    ew = (ebox[2] - ebox[0]) if ebox else 0
    lo, hi = 0, len(text)
    while lo < hi:
        mid = (lo + hi + 1) // 2
        sub = text[:mid]
        bb = f.getbbox(sub)
        w = (bb[2] - bb[0]) if bb else 0
        if w + ew <= max_w:
            lo = mid
        else:
            hi = mid - 1
    draw.text(pos, text[:lo] + ellipsis, fill=color, font=f, anchor="lt")

def draw_text_wrapped(draw, pos, text, color, size, max_w, max_lines=2):
    """Left-top wrapped text approximating DT_WORDBREAK.
    Uses textwrap for word/CJK boundaries; truncates last line with ... if needed."""
    import textwrap
    f = font(size, False)
    def w(s):
        bb = f.getbbox(s)
        return (bb[2] - bb[0]) if bb else 0
    # Estimate chars-per-line from a typical CJK glyph width.
    # Multiply by 1.15 so the preview roughly matches C++ DrawTextW's pixel-level wrapping.
    cw = max(1, w("中"))
    width = max(1, int(max_w / cw * 1.15))
    lines = textwrap.wrap(text, width=width)
    lines = lines[:max_lines]
    if len(lines) == max_lines:
        last = lines[-1]
        if w(last) > max_w:
            while last and w(last + "...") > max_w:
                last = last[:-1]
            last = last + "..."
        lines[-1] = last
    x, y = pos
    lh = int(size * 1.45)
    for i, ln in enumerate(lines):
        draw.text((x, y + i * lh), ln, fill=color, font=f, anchor="lt")

def draw_button(draw, rect, text, primary=False, hover=False):
    fill = C_PRIMARY_H if (primary and hover) else (C_PRIMARY if primary else (C_BTN_SECONDARY_H if hover else C_BTN_SECONDARY))
    border = fill if primary else (C_BTN_BORDER_H if hover else C_BTN_BORDER)
    rounded_rect(draw, rect, 6, fill=fill, outline=border)
    cx = (rect[0] + rect[2]) // 2
    cy = (rect[1] + rect[3]) // 2
    draw_text(draw, (cx, cy), text, (255, 255, 255) if primary else C_TEXT, 13, anchor="mm")

def draw_badge(draw, rect, text, enabled):
    fill = C_GREEN_BG if enabled else C_BADGE_NO_BG
    fg = C_GREEN_DARK if enabled else C_BADGE_NO_FG
    bg = fill
    rounded_rect(draw, rect, 11, fill=fill, outline=fill)
    cx = (rect[0] + rect[2]) // 2
    cy = (rect[1] + rect[3]) // 2
    # indicator dot
    dr = 3
    drect = [rect[0] + 10, cy - dr, rect[0] + 10 + 2 * dr, cy + dr]
    if enabled:
        draw.ellipse(drect, fill=fg)
    else:
        draw.ellipse(drect, fill=bg, outline=fg, width=1)
    # label (left of dot)
    draw_text(draw, (rect[0] + 22, cy), text, fg, 11, anchor="lm")

def draw_icon(draw, rect, fid, accent):
    x1, y1, x2, y2 = rect
    cx, cy = (x1 + x2) // 2, (y1 + y2) // 2
    white = (255, 255, 255)
    if fid == "ComputerManagement":
        # monitor
        mw, mh = 26, 12
        mx1, my1 = cx - mw // 2, cy - 10
        mx2, my2 = mx1 + mw, my1 + mh
        rounded_rect(draw, [mx1, my1, mx2, my2], 3, outline=white, width=2)
        draw.line([(cx, my2), (cx, my2 + 5)], fill=white, width=2)
        draw.line([(cx - 7, my2 + 5), (cx + 7, my2 + 5)], fill=white, width=2)
    elif fid == "DeviceManager":
        # chip
        s = 18
        sx1, sy1 = cx - s // 2, cy - s // 2
        sx2, sy2 = sx1 + s, sy1 + s
        rounded_rect(draw, [sx1, sy1, sx2, sy2], 2, outline=white, width=2)
        draw.rectangle([cx - 3, cy - 3, cx + 3, cy + 3], fill=white)
        for dx in (-6, 0, 6):
            draw.line([(cx + dx, sy1), (cx + dx, sy1 - 3)], fill=white, width=2)
            draw.line([(cx + dx, sy2), (cx + dx, sy2 + 3)], fill=white, width=2)
        for dy in (-6, 0, 6):
            draw.line([(sx1, cy + dy), (sx1 - 3, cy + dy)], fill=white, width=2)
            draw.line([(sx2, cy + dy), (sx2 + 3, cy + dy)], fill=white, width=2)
    elif fid.startswith("CmdHere"):
        # 命令提示符：白色圆角终端窗口 + ">" 提示符 + 光标短横
        tw, th = 34, 22
        tx1, ty1 = cx - tw // 2, cy - th // 2 - 2
        tx2, ty2 = tx1 + tw, ty1 + th
        rounded_rect(draw, [tx1, ty1, tx2, ty2], 3, fill=white, outline=white, width=2)
        # ">" 提示符（深色，终端内）
        pgx, pgy = tx1 + 8, ty1 + 14
        draw.line([(pgx, pgy - 4), (pgx + 5, pgy)], fill=accent, width=2)
        draw.line([(pgx + 5, pgy), (pgx, pgy + 4)], fill=accent, width=2)
        # 光标短横
        draw.rectangle([pgx + 9, pgy - 1, pgx + 15, pgy + 1], fill=accent)
    else:
        # folder + eye
        fw, fh = 28, 16
        fx1, fy1 = cx - fw // 2, cy - 5
        fx2, fy2 = fx1 + fw, fy1 + fh
        rounded_rect(draw, [fx1, fy1, fx2, fy2], 3, outline=white, width=2)
        draw.rectangle([fx1, fy1 - 5, fx1 + 12, fy1], fill=white)
        ex, ey, er = cx, cy + 3, 4
        draw.ellipse([ex - er, ey - er, ex + er, ey + er], outline=white, width=2)
        draw.ellipse([ex - 1, ey - 1, ex + 1, ey + 1], fill=white)

def draw_card(draw, x1, y1, x2, y2, feature, installed, hover=False):
    if hover:
        # hover：三层实色阴影模拟 elevation 上浮（远层偏移大且浅 → 近层偏移小且深）
        rounded_rect(draw, [x1 + 2, y1 + 8, x2 + 2, y2 + 8], RADIUS, fill=C_SH_HOVER_3)
        rounded_rect(draw, [x1 + 1, y1 + 4, x2 + 1, y2 + 4], RADIUS, fill=C_SH_HOVER_2)
        rounded_rect(draw, [x1,     y1 + 1, x2,     y2 + 1], RADIUS, fill=C_SH_HOVER_1)
        rounded_rect(draw, [x1, y1, x2, y2], RADIUS, fill=C_CARD_BG, outline=C_PRIMARY_H, width=2)
    else:
        # 常态：极淡单层阴影 + 中性边框（Ant 卡片"轻"观感，靠边框立起）
        rounded_rect(draw, [x1, y1 + 2, x2, y2 + 2], RADIUS, fill=C_SHADOW)
        rounded_rect(draw, [x1, y1, x2, y2], RADIUS, fill=C_CARD_BG, outline=C_BORDER)
    # accent bar
    accent = C_GREEN if feature["kind"] == "run" else (C_PRIMARY if feature["kind"] == "reg" else C_AMBER)
    rounded_rect(draw, [x1, y1 + 14, x1 + BAR_W, y2 - 14], 2, fill=accent, outline=accent)
    # icon
    icon_rect = [x1 + 18, y1 + (CARD_H - ICON_SZ) // 2, x1 + 18 + ICON_SZ, y1 + (CARD_H - ICON_SZ) // 2 + ICON_SZ]
    rounded_rect(draw, icon_rect, 11, fill=accent, outline=accent)
    draw_icon(draw, icon_rect, feature["id"], accent)
    # text baseline
    tx = icon_rect[2] + 16
    # button: centered-right
    btn_rect = [x2 - 18 - BTN_W, y1 + (CARD_H - BTN_H) // 2, x2 - 18, y1 + (CARD_H - BTN_H) // 2 + BTN_H]
    draw_button(draw, btn_rect, "启用" if not installed else "停用", primary=not installed)
    # name
    draw_text(draw, (tx, y1 + 18), feature["name"], C_TEXT, 15, bold=True, anchor="lt")
    # desc (two lines, DT_WORDBREAK, only ellipsis if still overflowing)
    max_desc_w = btn_rect[0] - 14 - tx
    draw_text_wrapped(draw, (tx, y1 + 42), feature["desc"], C_DESC, 12, max_desc_w, max_lines=2)
    # badge
    badge_rect = [tx, y1 + 82, tx + BADGE_W, y1 + 82 + BADGE_H]
    draw_badge(draw, badge_rect, "已启用" if installed else "未启用", installed)

def draw_window_icon(draw, rect):
    # Simulate the real app icon (blue rounded tile + white window mark)
    rounded_rect(draw, rect, 5, fill=C_PRIMARY, outline=C_PRIMARY)
    ix1, iy1, ix2, iy2 = rect
    cx, cy = (ix1 + ix2) // 2, (iy1 + iy2) // 2
    w = 12
    rounded_rect(draw, [cx - w // 2, cy - 4, cx + w // 2, cy + 4], 2, outline=(255, 255, 255), width=1)
    draw.rectangle([cx - w // 2, iy1 + 4, cx + w // 2, iy1 + 6], fill=(255, 255, 255))

def main():
    W = WIN_W + 2 * SHADOW_PAD
    H = WIN_H + 2 * SHADOW_PAD
    img = Image.new("RGBA", (W + MOCK * 2, H + MOCK * 2), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    wx1, wy1 = MOCK, MOCK
    wx2, wy2 = wx1 + W, wy1 + H
    ox, oy = wx1 + SHADOW_PAD, wy1 + SHADOW_PAD   # 内容原点

    # 窗口层：干净圆角窗口 + 1px 边框（不再绘制灰色外阴影，v4.1 由系统原生阴影替代）
    rounded_rect(draw, [ox, oy, ox + WIN_W, oy + WIN_H], WIN_RADIUS,
                 fill=C_BG, outline=C_WIN_BORDER, width=1)

    # 标题栏
    title_bar = [ox, oy, ox + WIN_W, oy + TITLE_H]
    rounded_rect(draw, title_bar, WIN_RADIUS, fill=C_TB_BG, outline=C_TB_BG)
    draw.rectangle([ox + 1, oy + TITLE_H - 6, ox + WIN_W - 1, oy + TITLE_H], fill=C_TB_BG, outline=None)
    draw.line([(ox, oy + TITLE_H), (ox + WIN_W, oy + TITLE_H)], fill=C_DIVIDER, width=1)

    # 应用图标 + 标题
    icon_rect = [ox + 16, oy + 12, ox + 38, oy + 34]
    draw_window_icon(draw, icon_rect)
    draw_text(draw, (ox + 48, oy + TITLE_H // 2), "WinSuiteX", C_TITLE, 14, bold=True, anchor="lm")

    # 窗口控件（细线字形）
    close_r = [ox + WIN_W - TB_BTN_W, oy, ox + WIN_W, oy + TITLE_H]
    min_r = [ox + WIN_W - TB_BTN_W * 2, oy, ox + WIN_W - TB_BTN_W, oy + TITLE_H]
    mc = (min_r[0] + min_r[2]) // 2
    ccx = (close_r[0] + close_r[2]) // 2
    mcy = ccy = (min_r[1] + min_r[3]) // 2
    draw.line([(mc - 5, mcy), (mc + 5, mcy)], fill=C_SUBTITLE, width=1)
    draw.line([(ccx - 5, ccy - 5), (ccx + 5, ccy + 5)], fill=C_SUBTITLE, width=1)
    draw.line([(ccx - 5, ccy + 5), (ccx + 5, ccy - 5)], fill=C_SUBTITLE, width=1)

    # 导语区
    hero_y = oy + TITLE_H + 20
    draw_text(draw, (ox + MARGIN, hero_y + 13), "Windows 套件", C_TITLE, 16, bold=True, anchor="lm")
    draw.rectangle([ox + MARGIN, oy + TITLE_H + 46, ox + MARGIN + 28, oy + TITLE_H + 49], fill=C_PRIMARY)
    draw_text(draw, (ox + MARGIN, oy + TITLE_H + 61), "Windows 11 (Build 22631)", C_SUBTITLE, 13, anchor="lm")
    draw.line([(ox + MARGIN, oy + TITLE_H + HEADER_H - 16),
               (ox + WIN_W - MARGIN, oy + TITLE_H + HEADER_H - 16)], fill=C_DIVIDER, width=1)

    # 卡片
    features = [
        {"id": "ComputerManagement", "kind": "run", "name": "计算机管理", "desc": "在“此电脑”右键菜单添加“计算机管理”，一键打开 compmgmt.msc", "installed": True},
        {"id": "DeviceManager", "kind": "run", "name": "设备管理器", "desc": "演示模块：在“此电脑”右键菜单添加“设备管理器”(devmgmt.msc)", "installed": False},
        {"id": "SuperHidden", "kind": "shell", "name": "显示/隐藏 系统文件", "desc": "在文件夹空白处右键一键切换“显示/隐藏 系统文件与扩展名”", "installed": True},
        {"id": "CmdHere", "kind": "run", "name": "命令提示符", "desc": "在文件夹上右键一键打开命令提示符，并自动定位到当前文件夹", "installed": False},
    ]
    cy = TITLE_H + HEADER_H + CARD_GAP
    for i, f in enumerate(features):
        y1 = oy + cy
        draw_card(draw, ox + MARGIN, y1, ox + WIN_W - MARGIN, y1 + CARD_H, f, f["installed"], hover=(i == 0))
        cy += CARD_H + CARD_GAP

    # 页脚（居中单行）
    zone_top = oy + WIN_H - FOOTER_H
    mid = zone_top + FOOTER_H // 2
    draw_text(draw, (ox + WIN_W // 2, mid),
              '右键菜单项启用后即时生效，无需重启',
              C_SUBTITLE, 11, anchor="mm")

    # 窗口 1px 清晰圆角外边框（最后描边，压在内容之上）
    rounded_rect(draw, [ox, oy, ox + WIN_W, oy + WIN_H], WIN_RADIUS,
                 outline=C_WIN_BORDER, width=1)

    bbox = img.getbbox()
    img = img.crop(bbox)
    img.save("preview.png")
    print("preview.png saved", img.size)

if __name__ == "__main__":
    main()
