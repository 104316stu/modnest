"""Draws the ModNest icons for the ChainedForge mods (Chained Beasts).

usage: python scripts/make-cf-icons.py   -> registry/icons/<id>.png (512x512)

Style: the game's dark crimson arena palette, a rough painted frame, one bold emblem per mod
and a short label, so they read well as small thumbnails.
"""
import math
import os
import random
from PIL import Image, ImageDraw, ImageFilter, ImageFont

S = 512
OUT = os.path.join(os.path.dirname(__file__), "..", "registry", "icons")
FONT = "C:/Windows/Fonts/impact.ttf"

INK = (28, 14, 22, 255)
CREAM = (246, 232, 204, 255)
CRIMSON = (176, 28, 54, 255)
GOLD = (238, 178, 52, 255)
RARITY = [(150, 140, 132, 255), (47, 134, 201, 255), (162, 63, 196, 255), (201, 71, 143, 255), (201, 138, 18, 255)]


def background(top, bottom, seed):
    rnd = random.Random(seed)
    img = Image.new("RGBA", (S, S))
    d = ImageDraw.Draw(img)
    for y in range(S):
        t = y / (S - 1)
        c = tuple(int(top[i] + (bottom[i] - top[i]) * t) for i in range(3)) + (255,)
        d.line([(0, y), (S, y)], fill=c)
    # brush-stroke texture
    tex = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    td = ImageDraw.Draw(tex)
    for _ in range(140):
        x, y = rnd.randrange(-50, S), rnd.randrange(0, S)
        w = rnd.randrange(60, 220)
        a = rnd.randrange(8, 26)
        td.line([(x, y), (x + w, y + rnd.randrange(-14, 14))], fill=(255, 255, 255, a) if rnd.random() < 0.5 else (0, 0, 0, a + 10), width=rnd.randrange(2, 9))
    img.alpha_composite(tex.filter(ImageFilter.GaussianBlur(1.2)))
    # vignette
    vig = Image.new("L", (S, S), 0)
    vd = ImageDraw.Draw(vig)
    for r in range(0, 300, 6):
        vd.ellipse([S / 2 - 380 + r, S / 2 - 380 + r, S / 2 + 380 - r, S / 2 + 380 - r], fill=int(r * 0.85))
    shade = Image.new("RGBA", (S, S), (10, 4, 8, 255))
    img = Image.composite(img, shade, vig.filter(ImageFilter.GaussianBlur(40)))
    return img


def frame(img, color, seed):
    rnd = random.Random(seed)
    d = ImageDraw.Draw(img)
    m = 18
    pts = []
    for side in range(4):
        for i in range(12):
            t = i / 12
            jitter = rnd.uniform(-5, 5)
            if side == 0: pts.append((m + t * (S - 2 * m), m + jitter))
            elif side == 1: pts.append((S - m + jitter, m + t * (S - 2 * m)))
            elif side == 2: pts.append((S - m - t * (S - 2 * m), S - m + jitter))
            else: pts.append((m + jitter, S - m - t * (S - 2 * m)))
    d.line(pts + [pts[0]], fill=color, width=12, joint="curve")
    d.line(pts + [pts[0]], fill=INK, width=3, joint="curve")


def label(img, text, color=CREAM):
    d = ImageDraw.Draw(img)
    size = 74
    while True:
        f = ImageFont.truetype(FONT, size)
        w = d.textlength(text, font=f)
        if w <= S - 80 or size <= 30: break
        size -= 4
    # banner behind the text
    band = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    bd = ImageDraw.Draw(band)
    bd.polygon([(26, 398), (S - 30, 388), (S - 22, 474), (34, 482)], fill=(16, 8, 12, 225))
    img.alpha_composite(band)
    d.text(((S - w) / 2, 432), text, font=f, fill=color, anchor="lm", stroke_width=4, stroke_fill=INK)


def rot_rect(cx, cy, w, h, ang):
    a = math.radians(ang)
    pts = []
    for dx, dy in [(-w / 2, -h / 2), (w / 2, -h / 2), (w / 2, h / 2), (-w / 2, h / 2)]:
        pts.append((cx + dx * math.cos(a) - dy * math.sin(a), cy + dx * math.sin(a) + dy * math.cos(a)))
    return pts


def card(d, cx, cy, ang, edge, glyph=None, font=None):
    d.polygon(rot_rect(cx + 6, cy + 8, 150, 210, ang), fill=(0, 0, 0, 140))
    d.polygon(rot_rect(cx, cy, 150, 210, ang), fill=CREAM, outline=INK, width=6)
    d.polygon(rot_rect(cx, cy, 128, 188, ang), outline=edge, width=8)
    if glyph:
        d.text((cx, cy), glyph, font=font, fill=edge, anchor="mm", stroke_width=3, stroke_fill=INK)


def chain_link(d, cx, cy, ang, color, w=150, h=84, t=22):
    # ring drawn as thick outline of a rotated rounded box
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    ld = ImageDraw.Draw(img)
    ld.rounded_rectangle([cx - w / 2, cy - h / 2, cx + w / 2, cy + h / 2], radius=h / 2, outline=INK, width=t + 8)
    ld.rounded_rectangle([cx - w / 2, cy - h / 2, cx + w / 2, cy + h / 2], radius=h / 2, outline=color, width=t)
    ld.rounded_rectangle([cx - w / 2 + 8, cy - h / 2 + 6, cx + w / 2 - 8, cy - h / 2 + 12], radius=4, fill=(255, 255, 255, 90))
    return img.rotate(ang, center=(cx, cy), resample=Image.BICUBIC)


def chainedforge():
    img = background((122, 26, 44), (36, 10, 20), 1)
    d = ImageDraw.Draw(img)
    # anvil
    anvil = [(110, 250), (400, 250), (430, 222), (330, 222), (330, 200), (150, 200), (150, 222), (82, 222)]
    d.polygon([(x + 6, y + 10) for x, y in anvil], fill=(0, 0, 0, 150))
    d.polygon(anvil, fill=(70, 66, 78, 255), outline=INK, width=6)
    d.polygon([(190, 250), (320, 250), (300, 300), (350, 330), (160, 330), (210, 300)], fill=(56, 52, 64, 255), outline=INK, width=6)
    # chain over it
    img.alpha_composite(chain_link(d, 190, 150, 20, GOLD))
    img.alpha_composite(chain_link(d, 320, 150, -20, GOLD))
    # sparks
    d = ImageDraw.Draw(img)
    rnd = random.Random(3)
    for _ in range(26):
        x, y = rnd.randrange(120, 400), rnd.randrange(60, 200)
        r = rnd.randrange(2, 6)
        d.ellipse([x - r, y - r, x + r, y + r], fill=(255, rnd.randrange(150, 220), 60, 255))
    frame(img, CRIMSON, 1)
    label(img, "CHAINEDFORGE")
    return img


def arena_deck():
    img = background((70, 26, 92), (22, 8, 30), 2)
    d = ImageDraw.Draw(img)
    f = ImageFont.truetype(FONT, 96)
    for i, (ang, edge, g) in enumerate([(-24, RARITY[0], "C"), (-8, RARITY[1], "U"), (8, RARITY[2], "R"), (24, RARITY[4], "L")]):
        card(d, 256 + (i - 1.5) * 52, 222 + abs(i - 1.5) * 10, ang, edge, g, f)
    frame(img, RARITY[2], 2)
    label(img, "ARENA DECK")
    return img


def rounds_and_rain():
    img = background((18, 58, 92), (8, 16, 34), 3)
    d = ImageDraw.Draw(img)
    rnd = random.Random(9)
    for _ in range(60):  # rain
        x, y = rnd.randrange(0, S), rnd.randrange(0, 380)
        d.line([(x, y), (x - 10, y + 34)], fill=(160, 210, 255, 110), width=3)
    card(d, 210, 222, -10, (47, 134, 201, 255))
    # big drop on the card
    drop = [(210, 120)]
    for a in range(0, 181, 10):
        r = math.radians(a)
        drop.append((210 + 52 * math.cos(r), 232 + 52 * math.sin(r)))
    d.polygon(drop, fill=(64, 156, 230, 255), outline=INK, width=6)
    d.ellipse([186, 206, 206, 226], fill=(220, 240, 255, 255))
    # a bullet streak (ROUNDS)
    d.line([(300, 300), (450, 150)], fill=(255, 210, 90, 160), width=16)
    d.ellipse([420, 128, 470, 178], fill=GOLD, outline=INK, width=6)
    frame(img, (64, 156, 230, 255), 3)
    label(img, "ROUNDS & RAIN")
    return img


def starter_pack():
    img = background((112, 66, 20), (36, 18, 8), 4)
    d = ImageDraw.Draw(img)
    # crossed swords behind a shield
    for ang in (35, -35):
        blade = Image.new("RGBA", (S, S), (0, 0, 0, 0))
        bd = ImageDraw.Draw(blade)
        bd.polygon([(244, 70), (268, 70), (268, 300), (256, 330), (244, 300)], fill=(214, 214, 222, 255), outline=INK, width=5)
        bd.rectangle([214, 300, 298, 318], fill=GOLD, outline=INK, width=5)
        bd.rectangle([246, 318, 266, 368], fill=(90, 50, 30, 255), outline=INK, width=5)
        img.alpha_composite(blade.rotate(ang, center=(256, 220), resample=Image.BICUBIC))
    d = ImageDraw.Draw(img)
    shield = [(256, 110), (350, 140), (346, 250), (256, 330), (166, 250), (162, 140)]
    d.polygon([(x + 6, y + 8) for x, y in shield], fill=(0, 0, 0, 150))
    d.polygon(shield, fill=CRIMSON, outline=INK, width=8)
    d.polygon([(256, 132), (330, 156), (326, 244), (256, 306)], fill=(150, 20, 44, 255))
    f = ImageFont.truetype(FONT, 110)
    d.text((256, 218), "6", font=f, fill=CREAM, anchor="mm", stroke_width=5, stroke_fill=INK)
    frame(img, GOLD, 4)
    label(img, "STARTER PACK")
    return img


def hard_mode():
    img = background((96, 10, 14), (20, 4, 6), 5)
    d = ImageDraw.Draw(img)
    # skull
    d.ellipse([146 + 6, 92 + 8, 366 + 6, 300 + 8], fill=(0, 0, 0, 160))
    d.ellipse([146, 92, 366, 300], fill=CREAM, outline=INK, width=8)
    d.rounded_rectangle([196, 250, 316, 336], radius=18, fill=CREAM, outline=INK, width=8)
    d.ellipse([186, 170, 244, 232], fill=INK)
    d.ellipse([268, 170, 326, 232], fill=INK)
    d.ellipse([200, 188, 222, 210], fill=(230, 40, 40, 255))
    d.ellipse([282, 188, 304, 210], fill=(230, 40, 40, 255))
    d.polygon([(256, 236), (240, 270), (272, 270)], fill=INK)
    for x in range(214, 304, 22):
        d.line([(x, 290), (x, 334)], fill=INK, width=6)
    # flames / horns
    d.polygon([(150, 150), (96, 60), (176, 118)], fill=(230, 60, 30, 255), outline=INK, width=6)
    d.polygon([(362, 150), (416, 60), (336, 118)], fill=(230, 60, 30, 255), outline=INK, width=6)
    frame(img, (230, 60, 30, 255), 5)
    label(img, "HARD MODE", (255, 120, 90, 255))
    return img


if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    for name, fn in [("chainedforge", chainedforge), ("cf-arena-deck", arena_deck), ("cf-rounds-and-rain", rounds_and_rain),
                     ("cf-starter-pack", starter_pack), ("cf-hard-mode", hard_mode)]:
        p = os.path.join(OUT, name + ".png")
        fn().convert("RGB").save(p, optimize=True)
        print("wrote", os.path.normpath(p))
