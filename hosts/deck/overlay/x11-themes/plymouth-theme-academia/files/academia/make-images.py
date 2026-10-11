#!/usr/bin/env python3
"""Images for the academia Plymouth theme: the wallpaper's brass astrolabe, small, in two layers.

Needs Pillow, pango-view and Iosevka Etoile:  python3 make-images.py [outdir]
  dial.png    the fixed rings and degree scale
  rete.png    the pointer and stars, which turn slowly while booting
  label.png   "deck", shown with the password field
  field.png   the rule under the password dots
  bullet.png  one password dot
  LOGO.GIF    the same astrolabe on black for the ThinkPad firmware's custom startup logo (not
              installed; see the deck README): at most 768x480 and 60 KB
Palette as in ~/.config/quickshell/Theme.qml.
"""
import math
import os
import random
import subprocess
import sys

from PIL import Image, ImageDraw, ImageFilter

OUT = sys.argv[1] if len(sys.argv) > 1 else "."
BRASS = (0xC9, 0xA4, 0x6A)
LINEN = (0xC9, 0xB8, 0x9F)
BARK = (0x54, 0x45, 0x3A)

SIZE = 300  # px; the rete is drawn in the same square so both share a centre
S = 4  # supersampling


def layer(draw_fn, alpha=1.0, blur=0.0):
    """Draw in white on a supersampled mask, then colour it brass."""
    big = Image.new("L", (SIZE * S, SIZE * S), 0)
    draw_fn(ImageDraw.Draw(big), SIZE * S / 2)
    mask = big.resize((SIZE, SIZE), Image.LANCZOS)
    if blur:
        mask = Image.blend(mask, mask.filter(ImageFilter.GaussianBlur(blur)), 0.3)
    mask = mask.point(lambda v: int(v * alpha))
    img = Image.new("RGBA", (SIZE, SIZE), BRASS + (0,))
    img.putalpha(mask)
    return img


def polar(c, r, deg):
    t = math.radians(deg)
    return c + r * math.cos(t), c + r * math.sin(t)


def dial(d, c):
    R = c - 4 * S

    def circle(r, w, a=255):
        d.ellipse((c - r, c - r, c + r, c + r), outline=a, width=w)

    circle(R, 2 * S)
    circle(R - 6 * S, S)
    circle(int(R * 0.78), S, 170)
    circle(int(R * 0.16), S, 200)
    for deg in range(0, 360, 5):
        long = deg % 30 == 0
        r0 = R - (16 if long else 10) * S
        d.line([polar(c, r0, deg), polar(c, R - 6 * S, deg)], fill=255 if long else 170, width=S)
    for deg in range(0, 360, 30):
        d.line([polar(c, R * 0.16, deg), polar(c, R * 0.78, deg)], fill=90, width=S)


def rete(d, c):
    R = c - 4 * S
    er = int(R * 0.56)
    ex, ey = c + R * 0.16, c - R * 0.1
    d.ellipse((ex - er, ey - er, ex + er, ey + er), outline=150, width=S)
    d.line([polar(c, R * 0.16, -58), polar(c, R * 0.86, -58)], fill=230, width=S)
    d.line([polar(c, R * 0.16, 122), polar(c, R * 0.42, 122)], fill=230, width=S)
    d.polygon([polar(c, R * 0.86, -58), polar(c, R * 0.79, -55.8), polar(c, R * 0.79, -60.2)], fill=230)
    random.seed(7)
    stars = [polar(c, random.uniform(R * 0.25, R * 0.72), random.uniform(0, 360)) for _ in range(7)]
    d.line(stars[:5], fill=110, width=S)
    for x, y in stars:
        s = random.choice((2, 3, 4)) * S
        d.ellipse((x - s, y - s, x + s, y + s), fill=255)


os.makedirs(OUT, exist_ok=True)
layer(dial, 0.85, 1.5).save(os.path.join(OUT, "dial.png"), optimize=True)
layer(rete, 0.95, 1.5).save(os.path.join(OUT, "rete.png"), optimize=True)

# Text through pango-view: Gentoo's Pillow may be built without FreeType.
subprocess.run(["pango-view", "--no-display", "--output", os.path.join(OUT, "label.png"),
                "--font", "Iosevka Etoile Italic 21", "--foreground", "#c9b89f",
                "--background", "transparent", "--margin", "4", "--text", "deck"], check=True)

field = Image.new("RGBA", (260, 1), BARK + (255,))
field.save(os.path.join(OUT, "field.png"), optimize=True)

b = Image.new("L", (12 * S, 12 * S), 0)
ImageDraw.Draw(b).ellipse((2 * S, 2 * S, 10 * S, 10 * S), fill=255)
bullet = Image.new("RGBA", (12, 12), BRASS + (0,))
bullet.putalpha(b.resize((12, 12), Image.LANCZOS))
bullet.save(os.path.join(OUT, "bullet.png"), optimize=True)

# Firmware startup logo: dial and rete together on black (the firmware fills the screen with
# black), 440 px, 48 colours to stay under the 60 KB limit.
logo = Image.new("RGBA", (440, 440), (0, 0, 0, 255))
logo.alpha_composite(Image.alpha_composite(Image.open(os.path.join(OUT, "dial.png")),
                                           Image.open(os.path.join(OUT, "rete.png"))).resize((440, 440), Image.LANCZOS))
logo.convert("RGB").quantize(colors=48, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE) \
    .save(os.path.join(OUT, "LOGO.GIF"), optimize=True)
