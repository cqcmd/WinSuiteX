"""
Soft-shadow prototype for WinSuiteX.
Reuses preview.py's geometry/helpers but renders REAL blurred shadows
(what GDI+ Helper.Blur + alpha-composite produces) instead of solid layers.
This is the visual the C++ side would get if we adopt GDI+ for the
shadow/elevation pass. Pure PIL preview — no C++ change yet.
"""
from PIL import Image, ImageDraw, ImageFilter
import preview as P

R = P.RADIUS
WIN_R = P.WIN_RADIUS
SHADOW_PAD = P.SHADOW_PAD
MOCK = P.MOCK
WIN_W = P.WIN_W
WIN_H = P.WIN_H
TITLE_H, HEADER_H, CARD_H, CARD_GAP, FOOTER_H, MARGIN = (
    P.TITLE_H, P.HEADER_H, P.CARD_H, P.CARD_GAP, P.FOOTER_H, P.MARGIN)

FEATURES = [
    {"id": "ComputerManagement", "kind": "run", "name": "计算机管理",
     "desc": "在“此电脑”右键菜单添加“计算机管理”，一键打开 compmgmt.msc", "installed": True},
    {"id": "DeviceManager", "kind": "run", "name": "设备管理器",
     "desc": "演示模块：在“此电脑”右键菜单添加“设备管理器”(devmgmt.msc)", "installed": False},
    {"id": "SuperHidden", "kind": "shell", "name": "显示/隐藏 系统文件",
     "desc": "在文件夹空白处右键一键切换“显示/隐藏 系统文件与扩展名”", "installed": True},
    {"id": "CmdHere", "kind": "run", "name": "命令提示符",
     "desc": "在文件夹上右键一键打开命令提示符，并自动定位到当前文件夹", "installed": False},
]


def card_shadow(sd, x1, y1, x2, y2, hover):
    # Real soft shadow: dark rounded rect, offset down, alpha; blurred later.
    if hover:
        P.rounded_rect(sd, [x1, y1 + 16, x2, y2 + 16], R, fill=(0, 0, 0, 95))
    else:
        P.rounded_rect(sd, [x1, y1 + 7, x2, y2 + 7], R, fill=(0, 0, 0, 42))


def card_surface(draw, x1, y1, x2, y2, f, installed, hover):
    P.rounded_rect(draw, [x1, y1, x2, y2], R, fill=P.C_CARD_BG,
                   outline=(P.C_PRIMARY_H if hover else P.C_BORDER), width=(2 if hover else 1))
    accent = P.C_GREEN if f["kind"] == "run" else (P.C_PRIMARY if f["kind"] == "reg" else P.C_AMBER)
    P.rounded_rect(draw, [x1, y1 + 14, x1 + P.BAR_W, y2 - 14], 2, fill=accent, outline=accent)
    icon_rect = [x1 + 18, y1 + (CARD_H - P.ICON_SZ) // 2,
                 x1 + 18 + P.ICON_SZ, y1 + (CARD_H - P.ICON_SZ) // 2 + P.ICON_SZ]
    P.rounded_rect(draw, icon_rect, 11, fill=accent, outline=accent)
    P.draw_icon(draw, icon_rect, f["id"], accent)
    tx = icon_rect[2] + 16
    btn_rect = [x2 - 18 - P.BTN_W, y1 + (CARD_H - P.BTN_H) // 2,
                x2 - 18, y1 + (CARD_H - P.BTN_H) // 2 + P.BTN_H]
    P.draw_button(draw, btn_rect, "启用" if not installed else "停用", primary=not installed)
    P.draw_text(draw, (tx, y1 + 18), f["name"], P.C_TEXT, 15, bold=True, anchor="lt")
    max_desc_w = btn_rect[0] - 14 - tx
    P.draw_text_wrapped(draw, (tx, y1 + 42), f["desc"], P.C_DESC, 12, max_desc_w, max_lines=2)
    badge_rect = [tx, y1 + 82, tx + P.BADGE_W, y1 + 82 + P.BADGE_H]
    P.draw_badge(draw, badge_rect, "已启用" if installed else "未启用", installed)


def main():
    W = WIN_W + 2 * SHADOW_PAD
    H = WIN_H + 2 * SHADOW_PAD
    img = Image.new("RGBA", (W + MOCK * 2, H + MOCK * 2), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    ox, oy = MOCK + SHADOW_PAD, MOCK + SHADOW_PAD

    # 1) window outer soft shadow (under everything)
    win_sh = Image.new("RGBA", img.size, (0, 0, 0, 0))
    wsd = ImageDraw.Draw(win_sh)
    P.rounded_rect(wsd, [ox - 12, oy - 12 + 16, ox + WIN_W + 12, oy + WIN_H + 12 + 16],
                   WIN_R + 12, fill=(0, 0, 0, 60))
    win_sh = win_sh.filter(ImageFilter.GaussianBlur(22))
    img.alpha_composite(win_sh)

    # 2) window chrome (opaque, hides inner part of window shadow)
    P.rounded_rect(draw, [ox, oy, ox + WIN_W, oy + WIN_H], WIN_R, fill=P.C_BG, outline=P.C_WIN_BORDER, width=1)
    title_bar = [ox, oy, ox + WIN_W, oy + TITLE_H]
    P.rounded_rect(draw, title_bar, WIN_R, fill=P.C_TB_BG, outline=P.C_TB_BG)
    draw.rectangle([ox + 1, oy + TITLE_H - 6, ox + WIN_W - 1, oy + TITLE_H], fill=P.C_TB_BG)
    draw.line([(ox, oy + TITLE_H), (ox + WIN_W, oy + TITLE_H)], fill=P.C_DIVIDER, width=1)
    P.draw_window_icon(draw, [ox + 16, oy + 12, ox + 38, oy + 34])
    P.draw_text(draw, (ox + 48, oy + TITLE_H // 2), "WinSuiteX", P.C_TITLE, 14, bold=True, anchor="lm")
    ccx = ox + WIN_W - P.TB_BTN_W // 2
    ccy = oy + TITLE_H // 2
    draw.line([(ccx - 5, ccy - 5), (ccx + 5, ccy + 5)], fill=P.C_SUBTITLE, width=1)
    draw.line([(ccx - 5, ccy + 5), (ccx + 5, ccy - 5)], fill=P.C_SUBTITLE, width=1)
    hero_y = oy + TITLE_H + 20
    P.draw_text(draw, (ox + MARGIN, hero_y + 13), "Windows 套件", P.C_TITLE, 16, bold=True, anchor="lm")
    draw.rectangle([ox + MARGIN, oy + TITLE_H + 46, ox + MARGIN + 28, oy + TITLE_H + 49], fill=P.C_PRIMARY)
    P.draw_text(draw, (ox + MARGIN, oy + TITLE_H + 61), "Windows 11 (Build 22631)", P.C_SUBTITLE, 13, anchor="lm")
    draw.line([(ox + MARGIN, oy + TITLE_H + HEADER_H - 16),
               (ox + WIN_W - MARGIN, oy + TITLE_H + HEADER_H - 16)], fill=P.C_DIVIDER, width=1)

    # 3) card soft shadows (composited over window bg, under card surfaces)
    card_sh = Image.new("RGBA", img.size, (0, 0, 0, 0))
    csd = ImageDraw.Draw(card_sh)
    cy = TITLE_H + HEADER_H + CARD_GAP
    rects = []
    for i, f in enumerate(FEATURES):
        y1 = oy + cy
        rects.append((x1 := ox + MARGIN, y1, x2 := ox + WIN_W - MARGIN, y1 + CARD_H, i == 0))
        card_shadow(csd, x1, y1, x2, y1 + CARD_H, i == 0)
        cy += CARD_H + CARD_GAP
    card_sh = card_sh.filter(ImageFilter.GaussianBlur(9))
    img.alpha_composite(card_sh)

    # 4) card surfaces on top
    for (x1, y1, x2, y2, hv), f in zip(rects, FEATURES):
        card_surface(draw, x1, y1, x2, y2, f, f["installed"], hv)

    zone_top = oy + WIN_H - FOOTER_H
    mid = zone_top + FOOTER_H // 2
    P.draw_text(draw, (ox + WIN_W // 2, mid), '右键菜单项启用后即时生效，无需重启', P.C_SUBTITLE, 11, anchor="mm")
    P.rounded_rect(draw, [ox, oy, ox + WIN_W, oy + WIN_H], WIN_R, outline=P.C_WIN_BORDER, width=1)

    bbox = img.getbbox()
    img = img.crop(bbox)
    img.save("preview_softshadow.png")
    print("preview_softshadow.png saved", img.size)


if __name__ == "__main__":
    main()
