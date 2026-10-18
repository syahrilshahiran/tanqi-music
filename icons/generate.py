#!/usr/bin/env python3
"""Regenerates every file in icons/ from one vector drawing: a bold Q (a platter
ring with an inner groove, a hub dot and a tail), silver on the app's matte black.

    python3 icons/generate.py

Needs pycairo and Pillow. Edit `mark()` below to change the artwork; every size
and format is redrawn from it (sizes up to 128px, and the Android foreground, get a brighter, heavier
groove so it stays readable).

Outputs, by platform:
  desktop     32x32.png 64x64.png 128x128.png 128x128@2x.png icon.png icon.ico icon.icns
  Windows     Square*Logo.png StoreLogo.png (the .ico above is the exe icon)
  Android     android/mipmap-*/ic_launcher{,_round,_foreground}.png, and the
              adaptive icon's background colour in android/values/
  iOS         ios/AppIcon-*.png (opaque squares, as the App Store requires)
"""
import math
import os

import cairo
from PIL import Image

# The app's device palette: matte black body, silver marks.
BASE, EDGE, SILVER, GROOVE, GROOVE_SMALL = "#0d0d0f", "#26262a", "#e4e4e7", "#505056", "#8e8e94"

def rgb(h):
    h = h.lstrip('#'); return tuple(int(h[i:i+2], 16)/255 for i in (0, 2, 4))

def rrect(ctx, x, y, w, h, r, fill, stroke=None, line=0):
    ctx.new_path()
    ctx.arc(x+w-r, y+r, r, -math.pi/2, 0); ctx.arc(x+w-r, y+h-r, r, 0, math.pi/2)
    ctx.arc(x+r, y+h-r, r, math.pi/2, math.pi); ctx.arc(x+r, y+r, r, math.pi, 3*math.pi/2)
    ctx.close_path()
    if fill:
        ctx.set_source_rgb(*rgb(fill))
        ctx.fill_preserve() if stroke else ctx.fill()
    if stroke:
        ctx.set_source_rgb(*rgb(stroke)); ctx.set_line_width(line); ctx.stroke()

def circle(ctx, r, fill=None, stroke=None, line=0):
    ctx.new_path(); ctx.arc(500, 500, r, 0, 2*math.pi)
    if fill:
        ctx.set_source_rgb(*rgb(fill))
        ctx.fill_preserve() if stroke else ctx.fill()
    if stroke:
        ctx.set_source_rgb(*rgb(stroke)); ctx.set_line_width(line); ctx.stroke()

# The mark's visual box in the 1024 canvas: the ring spans 172..828, the tail reaches ~877.
MARK_CENTRE = 524.5

def mark(ctx, small=False):
    """The Q, drawn in a 1024x1024 box. `small` thickens the inner groove for tiny sizes."""
    circle(ctx, 290, stroke=SILVER, line=76)                                   # the platter ring
    circle(ctx, 170, stroke=GROOVE_SMALL if small else GROOVE, line=40 if small else 20)    # inner groove
    k = math.sqrt(0.5)
    ctx.new_path(); ctx.move_to(500+210*k, 500+210*k); ctx.line_to(500+480*k, 500+480*k)  # the tail
    ctx.set_source_rgb(*rgb(SILVER)); ctx.set_line_width(76); ctx.set_line_cap(cairo.LINE_CAP_ROUND); ctx.stroke()
    circle(ctx, 40, fill=SILVER)                                               # the hub

def render(size, variant="rounded", scale=1.0):
    """variant: rounded | square | circle | transparent"""
    surf = cairo.ImageSurface(cairo.FORMAT_ARGB32, size, size)
    ctx = cairo.Context(surf); ctx.set_antialias(cairo.ANTIALIAS_BEST)
    ctx.scale(size/1024, size/1024)
    if variant == "rounded":
        rrect(ctx, 0, 0, 1024, 1024, 230, BASE)
        if size >= 128:  # a hairline rim, like the device body; too fine to help below this
            rrect(ctx, 10, 10, 1004, 1004, 222, None, EDGE, 8)
    elif variant == "square": ctx.rectangle(0, 0, 1024, 1024); ctx.set_source_rgb(*rgb(BASE)); ctx.fill()
    elif variant == "circle":
        ctx.arc(512, 512, 512, 0, 2*math.pi); ctx.set_source_rgb(*rgb(BASE)); ctx.fill()
    ctx.translate(512, 512); ctx.scale(scale, scale); ctx.translate(-MARK_CENTRE, -MARK_CENTRE)
    # The Android adaptive foreground is drawn at ~60% of its canvas, so it needs the small treatment too.
    mark(ctx, small=size <= 128 or variant == "transparent")
    img = Image.frombuffer("RGBA", (size, size), bytes(surf.get_data()), "raw", "BGRa", 0, 1)
    return img.copy()


# ---------------------------------------------------------------- outputs

HERE = os.path.dirname(os.path.abspath(__file__))


def out(*parts):
    path = os.path.join(HERE, *parts)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    return path


def save_png(name_parts, size, variant="rounded", scale=1.0, opaque=False):
    img = render(size, variant, scale)
    if opaque:  # iOS icons must have no alpha channel
        flat = Image.new("RGB", img.size, BASE)
        flat.paste(img, (0, 0), img)
        img = flat
    img.save(out(*name_parts))


def main():
    # desktop PNGs
    for name, size in {"32x32.png": 32, "64x64.png": 64, "128x128.png": 128, "128x128@2x.png": 256, "icon.png": 512}.items():
        save_png([name], size)

    # Windows tiles and store logo
    for n in (30, 44, 71, 89, 107, 142, 150, 284, 310):
        save_png([f"Square{n}x{n}Logo.png"], n)
    save_png(["StoreLogo.png"], 50)

    # Android: legacy square + round launcher icons, and the adaptive foreground
    # (transparent, scaled to sit inside the launcher's circular safe zone)
    for density, (icon, fg) in {"mdpi": (48, 108), "hdpi": (49, 162), "xhdpi": (96, 216), "xxhdpi": (144, 324), "xxxhdpi": (192, 432)}.items():
        d = f"mipmap-{density}"
        save_png(["android", d, "ic_launcher.png"], icon)
        save_png(["android", d, "ic_launcher_round.png"], icon, "circle", 0.78)
        save_png(["android", d, "ic_launcher_foreground.png"], fg, "transparent", 0.62)
    with open(out("android", "values", "ic_launcher_background.xml"), "w") as f:
        f.write(f'''<?xml version="1.0" encoding="utf-8"?>
<resources>
  <color name="ic_launcher_background">{BASE}</color>
</resources>''')

    # iOS: opaque squares (the system rounds the corners itself)
    ios = {
        "20x20@1x": 20, "20x20@2x": 40, "20x20@2x-1": 40, "20x20@3x": 60,
        "29x29@1x": 29, "29x29@2x": 58, "29x29@2x-1": 58, "29x29@3x": 87,
        "40x40@1x": 40, "40x40@2x": 80, "40x40@2x-1": 80, "40x40@3x": 120,
        "60x60@2x": 120, "60x60@3x": 180, "76x76@1x": 76, "76x76@2x": 152,
        "83.5x83.5@2x": 167, "512@2x": 1024,
    }
    for name, size in ios.items():
        save_png(["ios", f"AppIcon-{name}.png"], size, "square", opaque=True)

    # Windows .ico: a true vector render at each size
    sizes = (16, 24, 32, 48, 64, 128, 256)
    frames = [render(s) for s in sizes]
    frames[-1].save(out("icon.ico"), format="ICO", sizes=[(s, s) for s in sizes], append_images=frames[:-1])

    # macOS .icns
    render(1024).save(out("icon.icns"))


if __name__ == "__main__":
    main()
