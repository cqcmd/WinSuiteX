"""
WinSuiteX 审美沙盒 — 同布局四套不同视觉方向对比。
不编译 C++，纯 PIL 原型，目的是让眼睛挑方向。
"""
import preview as P
from PIL import Image, ImageDraw, ImageFilter

# ---------- 通用：软阴影（真实高斯模糊） ----------
def soft_shadow(img, rect, radius, offset, color, blur):
    x1, y1, x2, y2 = rect
    offx, offy = offset
    sh = Image.new("RGBA", img.size, (0, 0, 0, 0))
    sd = ImageDraw.Draw(sh)
    P.rounded_rect(sd, [x1 + offx, y1 + offy, x2 + offx, y2 + offy], radius, fill=color)
    sh = sh.filter(ImageFilter.GaussianBlur(blur))
    return Image.alpha_composite(img, sh)

def vgradient(w, h, top, bottom):
    img = Image.new("RGBA", (w, h))
    d = ImageDraw.Draw(img)
    r1, g1, b1 = top[:3]; r2, g2, b2 = bottom[:3]
    for y in range(h):
        t = y / max(1, h - 1)
        col = (int(r1 + (r2 - r1) * t), int(g1 + (g2 - g1) * t),
               int(b1 + (b2 - b1) * t), 255)
        d.line([(0, y), (w, y)], fill=col)
    return img

# ---------- 主题定义 ----------
THEMES = {
    "A_ant_soft": {
        "label": "A · Ant 软阴影白底",
        "bg": (245, 245, 245, 255),
        "tb_bg": (250, 250, 250, 255),
        "card_bg": (255, 255, 255, 255),
        "win_border": (217, 217, 217, 255),
        "text_title": (31, 31, 31, 255),
        "text": (38, 38, 38, 255),
        "sub": (89, 89, 89, 255),
        "desc": (89, 89, 89, 255),
        "primary": (22, 119, 255, 255),
        "primary_h": (22, 119, 255, 255),
        "btn_sec_bg": (255, 255, 255, 255),
        "btn_sec_border": (217, 217, 217, 255),
        "btn_sec_border_h": (191, 191, 191, 255),
        "accents": {"run": (22, 163, 74, 255), "reg": (22, 119, 255, 255), "shell": (202, 110, 4, 255)},
        "style": "flat_soft",
        "card_shadow": {"offset": (0, 7), "color": (0, 0, 0, 42), "blur": 9},
        "card_hover_shadow": {"offset": (0, 16), "color": (0, 0, 0, 95), "blur": 9},
        "win_shadow": {"offset": (0, 14), "color": (0, 0, 0, 55), "blur": 22},
    },
    "B_neumorphism": {
        "label": "B · 新拟态 Neumorphism",
        "bg": (224, 229, 236, 255),
        "tb_bg": (224, 229, 236, 255),
        "card_bg": (224, 229, 236, 255),
        "win_border": (224, 229, 236, 255),
        "text_title": (60, 66, 80, 255),
        "text": (74, 80, 94, 255),
        "sub": (120, 126, 140, 255),
        "desc": (130, 136, 150, 255),
        "primary": (78, 115, 223, 255),
        "primary_h": (96, 132, 235, 255),
        "btn_sec_bg": (224, 229, 236, 255),
        "btn_sec_border": (224, 229, 236, 255),
        "btn_sec_border_h": (224, 229, 236, 255),
        "accents": {"run": (46, 160, 67, 255), "reg": (78, 115, 223, 255), "shell": (190, 140, 20, 255)},
        "style": "neu",
        "card_shadow": None,
        "card_hover_shadow": None,
        "win_shadow": None,
    },
    "C_dark_esports": {
        "label": "C · 暗色电竞 Dark",
        "bg": (22, 26, 35, 255),
        "tb_bg": (27, 32, 43, 255),
        "card_bg": (30, 36, 48, 255),
        "win_border": (70, 80, 100, 60),
        "text_title": (236, 240, 248, 255),
        "text": (222, 228, 238, 255),
        "sub": (150, 160, 175, 255),
        "desc": (160, 170, 185, 255),
        "primary": (34, 211, 238, 255),
        "primary_h": (103, 232, 249, 255),
        "btn_sec_bg": (38, 45, 58, 255),
        "btn_sec_border": (70, 82, 100, 255),
        "btn_sec_border_h": (100, 116, 140, 255),
        "accents": {"run": (52, 211, 153, 255), "reg": (34, 211, 238, 255), "shell": (251, 191, 36, 255)},
        "style": "dark",
        "card_shadow": {"offset": (0, 10), "color": (0, 0, 0, 130), "blur": 13},
        "card_hover_shadow": {"offset": (0, 14), "color": (0, 0, 0, 170), "blur": 15},
        "win_shadow": {"offset": (0, 16), "color": (0, 0, 0, 150), "blur": 26},
    },
    "D_glass": {
        "label": "D · 玻璃拟态 Glass",
        "bg_gradient": ((124, 58, 237, 255), (37, 99, 235, 255)),
        "bg": (255, 255, 255, 16),
        "tb_bg": (255, 255, 255, 10),
        "card_bg": (255, 255, 255, 40),
        "win_border": (255, 255, 255, 90),
        "text_title": (255, 255, 255, 255),
        "text": (255, 255, 255, 245),
        "sub": (235, 240, 255, 220),
        "desc": (235, 240, 255, 210),
        "primary": (99, 102, 241, 255),
        "primary_h": (129, 132, 248, 255),
        "btn_sec_bg": (255, 255, 255, 30),
        "btn_sec_border": (255, 255, 255, 110),
        "btn_sec_border_h": (255, 255, 255, 170),
        "accents": {"run": (52, 211, 153, 255), "reg": (129, 140, 248, 255), "shell": (251, 191, 36, 255)},
        "style": "glass",
        "card_shadow": {"offset": (0, 12), "color": (0, 0, 0, 70), "blur": 16},
        "card_hover_shadow": {"offset": (0, 18), "color": (0, 0, 0, 110), "blur": 18},
        "win_shadow": {"offset": (0, 16), "color": (0, 0, 0, 90), "blur": 28},
    },
}

# ---------- 复用 preview 的控件画法 ----------
def draw_icon_tile(draw, rect, fid, accent):
    P.rounded_rect(draw, rect, 11, fill=accent, outline=accent)
    P.draw_icon(draw, rect, fid, accent)

def draw_button(draw, rect, text, primary, hover, t):
    if primary:
        fill = t["primary_h"] if hover else t["primary"]
        fg = (15, 23, 42, 255) if t["style"] == "dark" else (255, 255, 255, 255)
    else:
        fill = t["btn_sec_bg"]
        border = t["btn_sec_border_h"] if hover else t["btn_sec_border"]
        P.rounded_rect(draw, rect, 6, fill=fill, outline=border)
        cx = (rect[0] + rect[2]) // 2; cy = (rect[1] + rect[3]) // 2
        P.draw_text(draw, (cx, cy), text, t["text"], 13, anchor="mm")
        return
    P.rounded_rect(draw, rect, 6, fill=fill, outline=fill)
    cx = (rect[0] + rect[2]) // 2; cy = (rect[1] + rect[3]) // 2
    P.draw_text(draw, (cx, cy), text, fg, 13, anchor="mm")

def draw_badge(draw, rect, text, installed, t):
    if t["style"] in ("dark", "glass"):
        fill = (52, 211, 153, 60) if installed else (255, 255, 255, 22)
        fg = (150, 240, 190, 255) if installed else (210, 216, 230, 235)
    else:
        fill = (223, 242, 230, 255) if installed else (240, 240, 240, 255)
        fg = (21, 128, 61, 255) if installed else (89, 89, 89, 255)
    P.rounded_rect(draw, rect, 11, fill=fill, outline=fill)
    cx = (rect[0] + rect[2]) // 2; cy = (rect[1] + rect[3]) // 2
    dr = 3
    drect = [rect[0] + 10, cy - dr, rect[0] + 10 + 2 * dr, cy + dr]
    if installed:
        draw.ellipse(drect, fill=fg)
    else:
        draw.ellipse(drect, fill=fill, outline=fg, width=1)
    P.draw_text(draw, (rect[0] + 22, cy), text, fg, 11, anchor="lm")

def draw_card(img, x1, y1, x2, y2, feature, installed, hover, t):
    style = t["style"]
    sh = t["card_hover_shadow"] if hover else t["card_shadow"]
    if sh:
        img = soft_shadow(img, [x1, y1, x2, y2], P.RADIUS, sh["offset"], sh["color"], sh["blur"])
    draw = ImageDraw.Draw(img)

    if style == "neu":
        # 双柔光阴影（暗下右 + 亮上左），无边框
        img = soft_shadow(img, [x1, y1, x2, y2], P.RADIUS, (7, 7), (160, 168, 182, 110), 12)
        draw = ImageDraw.Draw(img)
        img = soft_shadow(img, [x1, y1, x2, y2], P.RADIUS, (-7, -7), (255, 255, 255, 200), 12)
        draw = ImageDraw.Draw(img)
        P.rounded_rect(draw, [x1, y1, x2, y2], P.RADIUS, fill=t["card_bg"])
        if hover:
            P.rounded_rect(draw, [x1, y1, x2, y2], P.RADIUS, outline=t["primary"], width=2)
    elif style == "dark":
        P.rounded_rect(draw, [x1, y1, x2, y2], P.RADIUS, fill=t["card_bg"],
                       outline=t["primary"] if hover else t["win_border"], width=1 if not hover else 2)
    elif style == "glass":
        P.rounded_rect(draw, [x1, y1, x2, y2], P.RADIUS, fill=t["card_bg"], outline=t["win_border"], width=1)
    else:  # flat_soft
        if hover:
            P.rounded_rect(draw, [x1, y1, x2, y2], P.RADIUS, fill=t["card_bg"], outline=t["primary"], width=2)
        else:
            P.rounded_rect(draw, [x1, y1, x2, y2], P.RADIUS, fill=t["card_bg"], outline=t["win_border"])

    accent = t["accents"]["reg"]
    if feature["kind"] == "run":
        accent = t["accents"]["run"]
    elif feature["kind"] == "shell":
        accent = t["accents"]["shell"]
    # accent bar
    P.rounded_rect(draw, [x1, y1 + 14, x1 + P.BAR_W, y2 - 14], 2, fill=accent, outline=accent)
    # icon tile
    icon_rect = [x1 + 18, y1 + (P.CARD_H - P.ICON_SZ) // 2, x1 + 18 + P.ICON_SZ, y1 + (P.CARD_H - P.ICON_SZ) // 2 + P.ICON_SZ]
    draw_icon_tile(draw, icon_rect, feature["id"], accent)
    # button
    btn_rect = [x2 - 18 - P.BTN_W, y1 + (P.CARD_H - P.BTN_H) // 2, x2 - 18, y1 + (P.CARD_H - P.BTN_H) // 2 + P.BTN_H]
    draw_button(draw, btn_rect, "启用" if not installed else "停用", primary=not installed, hover=hover, t=t)
    # text
    tx = icon_rect[2] + 16
    P.draw_text(draw, (tx, y1 + 18), feature["name"], t["text"], 15, bold=True, anchor="lt")
    max_desc_w = btn_rect[0] - 14 - tx
    P.draw_text_wrapped(draw, (tx, y1 + 42), feature["desc"], t["desc"], 12, max_desc_w, max_lines=2)
    badge_rect = [tx, y1 + 82, tx + P.BADGE_W, y1 + 82 + P.BADGE_H]
    draw_badge(draw, badge_rect, "已启用" if installed else "未启用", installed, t)
    return img

def render_window(t):
    W = P.WIN_W + 2 * P.SHADOW_PAD
    H = P.WIN_H + 2 * P.SHADOW_PAD
    MOCK = 44
    if t.get("bg_gradient"):
        bg = vgradient(W + 2 * MOCK, H + 2 * MOCK, t["bg_gradient"][0], t["bg_gradient"][1])
    else:
        bg = Image.new("RGBA", (W + 2 * MOCK, H + 2 * MOCK), (0, 0, 0, 0))
    img = bg
    if t.get("win_shadow"):
        img = soft_shadow(img, [MOCK + P.SHADOW_PAD, MOCK + P.SHADOW_PAD,
                                MOCK + P.SHADOW_PAD + P.WIN_W, MOCK + P.SHADOW_PAD + P.WIN_H],
                          P.WIN_RADIUS, t["win_shadow"]["offset"], t["win_shadow"]["color"], t["win_shadow"]["blur"])
    draw = ImageDraw.Draw(img)
    ox, oy = MOCK + P.SHADOW_PAD, MOCK + P.SHADOW_PAD

    P.rounded_rect(draw, [ox, oy, ox + P.WIN_W, oy + P.WIN_H], P.WIN_RADIUS,
                   fill=t["bg"], outline=t["win_border"], width=1)
    # title bar
    P.rounded_rect(draw, [ox, oy, ox + P.WIN_W, oy + P.TITLE_H], P.WIN_RADIUS, fill=t["tb_bg"], outline=t["tb_bg"])
    draw.rectangle([ox + 1, oy + P.TITLE_H - 6, ox + P.WIN_W - 1, oy + P.TITLE_H], fill=t["tb_bg"][:3] + (255,) if len(t["tb_bg"]) == 3 else t["tb_bg"])
    draw.line([(ox, oy + P.TITLE_H), (ox + P.WIN_W, oy + P.TITLE_H)], fill=t["win_border"], width=1)
    # app icon
    P.rounded_rect(draw, [ox + 16, oy + 12, ox + 38, oy + 34], 5, fill=t["primary"], outline=t["primary"])
    cx, cy = ox + 27, oy + 23
    P.rounded_rect(draw, [cx - 6, cy - 4, cx + 6, cy + 4], 2, outline=(255, 255, 255, 255), width=1)
    draw.rectangle([cx - 6, oy + 16, cx + 6, oy + 18], fill=(255, 255, 255, 255))
    P.draw_text(draw, (ox + 48, oy + P.TITLE_H // 2), "WinSuiteX", t["text_title"], 14, bold=True, anchor="lm")
    # min / close
    mc = ox + P.WIN_W - P.TB_BTN_W * 3 // 2
    ccx = ox + P.WIN_W - P.TB_BTN_W // 2
    mcy = ccy = oy + P.TITLE_H // 2
    sc = t["sub"]
    draw.line([(mc - 5, mcy), (mc + 5, mcy)], fill=sc, width=1)
    draw.line([(ccx - 5, ccy - 5), (ccx + 5, ccy + 5)], fill=sc, width=1)
    draw.line([(ccx - 5, ccy + 5), (ccx + 5, ccy - 5)], fill=sc, width=1)
    # hero
    P.draw_text(draw, (ox + P.MARGIN, oy + P.TITLE_H + 33), "Windows \u5957\u4ef6", t["text_title"], 16, bold=True, anchor="lm")
    draw.rectangle([ox + P.MARGIN, oy + P.TITLE_H + 46, ox + P.MARGIN + 28, oy + P.TITLE_H + 49], fill=t["primary"][:3] + (255,) if len(t["primary"]) == 3 else t["primary"])
    P.draw_text(draw, (ox + P.MARGIN, oy + P.TITLE_H + 61), "Windows 11 (Build 22631)", t["sub"], 13, anchor="lm")
    draw.line([(ox + P.MARGIN, oy + P.TITLE_H + P.HEADER_H - 16), (ox + P.WIN_W - P.MARGIN, oy + P.TITLE_H + P.HEADER_H - 16)], fill=t["win_border"], width=1)

    features = [
        {"id": "ComputerManagement", "kind": "run", "name": "\u8ba1\u7b97\u673a\u7ba1\u7406", "desc": "\u5728\u201c\u6b64\u7535\u8111\u201d\u53f3\u952e\u83dc\u5355\u6dfb\u52a0\u201c\u8ba1\u7b97\u673a\u7ba1\u7406\u201d\uff0c\u4e00\u952e\u6253\u5f00 compmgmt.msc", "installed": True},
        {"id": "DeviceManager", "kind": "run", "name": "\u8bbe\u5907\u7ba1\u7406\u5668", "desc": "\u6f14\u793a\u6a21\u5757\uff1a\u5728\u201c\u6b64\u7535\u8111\u201d\u53f3\u952e\u83dc\u5355\u6dfb\u52a0\u201c\u8bbe\u5907\u7ba1\u7406\u5668\u201d(devmgmt.msc)", "installed": False},
        {"id": "SuperHidden", "kind": "shell", "name": "\u663e\u793a/\u9690\u85cf \u7cfb\u7edf\u6587\u4ef6", "desc": "\u5728\u6587\u4ef6\u5939\u7a7a\u767d\u5904\u53f3\u952e\u4e00\u952e\u5207\u6362\u201c\u663e\u793a/\u9690\u85cf \u7cfb\u7edf\u6587\u4ef6\u4e0e\u6269\u5c55\u540d\u201d", "installed": True},
        {"id": "CmdHere", "kind": "run", "name": "\u547d\u4ee4\u63d0\u793a\u7b26", "desc": "\u5728\u6587\u4ef6\u5939\u4e0a\u53f3\u952e\u4e00\u952e\u6253\u5f00\u547d\u4ee4\u63d0\u793a\u7b26\uff0c\u5e76\u81ea\u52a8\u5b9a\u4f4d\u5230\u5f53\u524d\u6587\u4ef6\u5939", "installed": False},
    ]
    cy = P.TITLE_H + P.HEADER_H + P.CARD_GAP
    for i, f in enumerate(features):
        y1 = oy + cy
        img = draw_card(img, ox + P.MARGIN, y1, ox + P.WIN_W - P.MARGIN, y1 + P.CARD_H, f, f["installed"], hover=(i == 0), t=t)
        draw = ImageDraw.Draw(img)
        cy += P.CARD_H + P.CARD_GAP
    # footer
    zone_top = oy + P.WIN_H - P.FOOTER_H
    mid = zone_top + P.FOOTER_H // 2
    P.draw_text(draw, (ox + P.WIN_W // 2, mid), '\u53f3\u952e\u83dc\u5355\u9879\u542f\u7528\u540e\u5373\u65f6\u751f\u6548\uff0c\u65e0\u9700\u91cd\u542f', t["sub"], 11, anchor="mm")
    P.rounded_rect(draw, [ox, oy, ox + P.WIN_W, oy + P.WIN_H], P.WIN_RADIUS, outline=t["win_border"], width=1)
    bbox = img.getbbox()
    return img.crop(bbox)

def main():
    keys = list(THEMES.keys())
    cells = {}
    for k in keys:
        cells[k] = render_window(THEMES[k])
        cells[k].save(f"theme_{k}.png")
        print("saved", k, cells[k].size)
    # 2x2 grid
    pad = 28
    label_h = 34
    cw = max(c.size[0] for c in cells.values())
    ch = max(c.size[1] for c in cells.values())
    grid = Image.new("RGBA", (cw * 2 + pad * 3, ch * 2 + pad * 3 + label_h * 2), (245, 246, 248, 255))
    g = ImageDraw.Draw(grid)
    positions = [(0, 0), (1, 0), (0, 1), (1, 1)]
    for k, (col, row) in zip(keys, positions):
        x = pad + col * (cw + pad)
        y = pad + row * (ch + pad + label_h)
        grid.paste(cells[k], (x, y + label_h))
        g.text((x, y), THEMES[k]["label"], fill=(40, 40, 40, 255), font=P.font(16, True))
    grid.save("themes_gallery.png")
    print("themes_gallery.png saved", grid.size)

if __name__ == "__main__":
    main()
