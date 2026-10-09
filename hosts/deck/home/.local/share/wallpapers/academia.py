#!/usr/bin/env python3
"""Dark academia wallpaper: mottled walnut leather, a vignette, and a faint brass astrolabe.

Deterministic (fixed seeds), needs only Pillow:  python3 academia.py [out.png] [WIDTHxHEIGHT]
Palette matches ~/.config/quickshell/Theme.qml (espresso #1c1612 .. leather #2f2620, brass #c9a46a).
"""
import math
import random
import sys

from PIL import Image, ImageChops, ImageDraw, ImageFilter

OUT = sys.argv[1] if len(sys.argv) > 1 else "academia.png"
W, H = map(int, (sys.argv[2] if len(sys.argv) > 2 else "1920x1200").split("x"))

ESPRESSO = (0x1C, 0x16, 0x12)
WALNUT = (0x24, 0x1D, 0x18)
LEATHER = (0x2F, 0x26, 0x20)
BRASS = (0xC9, 0xA4, 0x6A)


def noise(w, h, sigma, seed):
    """Grey Gaussian noise centred on 128, reproducible."""
    random.seed(seed)
    img = Image.new("L", (w, h))
    img.putdata([max(0, min(255, int(random.gauss(128, sigma)))) for _ in range(w * h)])
    return img


def mottle(scale, blur, seed):
    """Low-frequency blotches: small noise, upscaled and blurred."""
    n = noise(max(2, W // scale), max(2, H // scale), 60, seed).filter(ImageFilter.GaussianBlur(1))
    return n.resize((W, H), Image.BICUBIC).filter(ImageFilter.GaussianBlur(blur))


# Leather: walnut base, lightened and darkened by two octaves of blotches.
mask = ImageChops.add(mottle(64, 40, 1), mottle(16, 10, 2), scale=2.0)
img = Image.composite(Image.new("RGB", (W, H), LEATHER), Image.new("RGB", (W, H), ESPRESSO),
                      mask.point(lambda v: max(0, min(255, (v - 104) * 5))))
img = Image.blend(Image.new("RGB", (W, H), WALNUT), img, 0.75)

# Fine grain, like the pores of the hide.
grain = noise(W, H, 7, 3).filter(ImageFilter.GaussianBlur(0.7))
img = ImageChops.add(img, Image.merge("RGB", [grain] * 3), scale=1.0, offset=-128)

# Brass astrolabe, drawn at 2x and scaled down for anti-aliasing.
S = 2
R = int(min(W, H) * 0.42) * S
cx, cy = int(W * 0.62) * S, int(H * 0.5) * S
art = Image.new("L", (W * S, H * S), 0)
d = ImageDraw.Draw(art)


def circle(r, width, a=255):
    d.ellipse((cx - r, cy - r, cx + r, cy + r), outline=a, width=width)


def polar(r, deg):
    t = math.radians(deg)
    return cx + r * math.cos(t), cy + r * math.sin(t)


circle(R, 3 * S)
circle(R - 10 * S, 1 * S)
circle(int(R * 0.80), 1 * S, 200)
circle(int(R * 0.52), 1 * S, 170)
circle(int(R * 0.14), 2 * S, 200)
for deg in range(0, 360, 2):  # degree scale between the outer rules
    long = deg % 30 == 0
    mid = deg % 10 == 0
    r0 = R - (34 if long else 24 if mid else 17) * S
    d.line([polar(r0, deg), polar(R - 10 * S, deg)], fill=255 if long else 190, width=S)
for deg in range(0, 360, 30):  # hour lines across the plate
    d.line([polar(int(R * 0.14), deg), polar(int(R * 0.80), deg)], fill=110, width=S)
# Tilted ecliptic ring and a rete pointer.
ex, ey, er = cx + int(R * 0.18), cy - int(R * 0.12), int(R * 0.58)
d.ellipse((ex - er, ey - er, ex + er, ey + er), outline=150, width=S)
d.line([polar(int(R * 0.14), -58), polar(int(R * 0.88), -58)], fill=210, width=S)
d.line([polar(int(R * 0.14), 122), polar(int(R * 0.40), 122)], fill=210, width=S)
d.polygon([polar(int(R * 0.88), -58), polar(int(R * 0.83), -56.6), polar(int(R * 0.83), -59.4)], fill=210)
# A few stars, joined like a constellation.
random.seed(7)
stars = [polar(random.uniform(R * 0.25, R * 0.75), random.uniform(0, 360)) for _ in range(7)]
d.line(stars[:5], fill=120, width=S)
for x, y in stars:
    s = random.choice((3, 4, 6)) * S
    d.ellipse((x - s, y - s, x + s, y + s), fill=255)

art = art.resize((W, H), Image.LANCZOS)
art = Image.blend(art, art.filter(ImageFilter.GaussianBlur(3)), 0.35)  # soften, like worn engraving
art = art.point(lambda v: v * 0.22)  # faint
img = Image.composite(Image.new("RGB", (W, H), BRASS), img, art)

# Thin double rule framing the screen.
frame = Image.new("L", (W, H), 0)
f = ImageDraw.Draw(frame)
for inset, a in ((28, 70), (34, 40)):
    f.rectangle((inset, inset, W - 1 - inset, H - 1 - inset), outline=a, width=1)
img = Image.composite(Image.new("RGB", (W, H), BRASS), img, frame)

# Vignette: darken towards the corners.
v = Image.new("L", (W, H), 0)
ImageDraw.Draw(v).ellipse((-W * 0.15, -H * 0.2, W * 1.15, H * 1.2), fill=255)
v = v.filter(ImageFilter.GaussianBlur(min(W, H) * 0.18))
img = Image.composite(img, Image.blend(img, Image.new("RGB", (W, H), (8, 6, 5)), 0.6), v)

img.save(OUT, optimize=True)
