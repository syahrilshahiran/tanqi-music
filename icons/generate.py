#!/usr/bin/env python3
"""Regenerates every file in icons/ from one vector drawing: a cat wearing
headphones, in the app's Rose Pine colours.

    python3 icons/generate.py

Needs pycairo and Pillow. Edit `cat()` below to change the artwork; every size
and format is redrawn from it (sizes up to 64px get slightly heavier detail so
the face stays readable).

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

# Rose Pine (the app's own palette)
BASE, SURFACE, OVERLAY = "#191724", "#1f1d2e", "#26233a"
IRIS, ROSE, GOLD, LOVE, TEXT = "#c4a7e7", "#ebbcba", "#f6c177", "#eb6f92", "#e0def4"

def rgb(h):
    h = h.lstrip('#'); return tuple(int(h[i:i+2], 16)/255 for i in (0, 2, 4))

def poly(ctx, pts, fill, join_stroke=0, stroke_color=None):
    ctx.new_path(); ctx.move_to(*pts[0])
    for p in pts[1:]: ctx.line_to(*p)
    ctx.close_path()
    ctx.set_source_rgb(*rgb(fill)); 
    if join_stroke:
        ctx.fill_preserve()
        ctx.set_source_rgb(*rgb(stroke_color or fill)); ctx.set_line_width(join_stroke); ctx.set_line_join(cairo.LINE_JOIN_ROUND); ctx.stroke()
    else: ctx.fill()

def rrect(ctx, x, y, w, h, r, fill):
    ctx.new_path()
    ctx.arc(x+w-r, y+r, r, -math.pi/2, 0); ctx.arc(x+w-r, y+h-r, r, 0, math.pi/2)
    ctx.arc(x+r, y+h-r, r, math.pi/2, math.pi); ctx.arc(x+r, y+r, r, math.pi, 3*math.pi/2)
    ctx.close_path(); ctx.set_source_rgb(*rgb(fill)); ctx.fill()

def mirror(pts): return [(1024-x, y) for x, y in pts]

def cat(ctx, small=False):
    """The cat + headphones, drawn in a 1024x1024 box. `small` thickens detail for tiny sizes."""
    k = 1.35 if small else 1.0
    # ears (round-joined triangles) + inner ears
    ear = [(222, 470), (240, 205), (445, 352)]
    inner = [(274, 402), (284, 282), (382, 340)]
    for flip in (False, True):
        e = mirror(ear) if flip else ear; i = mirror(inner) if flip else inner
        poly(ctx, e, IRIS, 38)
        poly(ctx, i, ROSE, 14)
    # head
    ctx.save(); ctx.translate(512, 605); ctx.scale(312, 252); ctx.arc(0, 0, 1, 0, 2*math.pi); ctx.restore()
    ctx.set_source_rgb(*rgb(IRIS)); ctx.fill()
    # cheeks
    for cx in (372, 652):
        ctx.save(); ctx.translate(cx, 700); ctx.scale(52, 30); ctx.arc(0, 0, 1, 0, 2*math.pi); ctx.restore()
        ctx.set_source_rgba(*rgb(ROSE), 0.55); ctx.fill()
    # happy closed eyes
    ctx.set_source_rgb(*rgb(BASE)); ctx.set_line_width(20*k); ctx.set_line_cap(cairo.LINE_CAP_ROUND)
    for cx in (405, 619):
        ctx.new_path(); ctx.move_to(cx-42, 628); ctx.curve_to(cx-30, 548, cx+30, 548, cx+42, 628); ctx.stroke()
    # nose + mouth
    poly(ctx, [(482, 668), (542, 668), (512, 705)], LOVE, 16)
    ctx.set_source_rgb(*rgb(BASE)); ctx.set_line_width(10*k); ctx.set_line_cap(cairo.LINE_CAP_ROUND)
    for s in (-1, 1):
        ctx.new_path(); ctx.move_to(512, 708); ctx.curve_to(512, 742, 512+s*18, 756, 512+s*50, 744); ctx.stroke()
    # headphone band (over the head, between the ears)
    ctx.new_path(); ctx.move_to(214, 590); ctx.curve_to(214, 252, 810, 252, 810, 590)
    ctx.set_source_rgb(*rgb(GOLD)); ctx.set_line_width(54); ctx.set_line_cap(cairo.LINE_CAP_ROUND); ctx.stroke()
    # ear cups
    for cx in (214, 810):
        rrect(ctx, cx-62, 478, 124, 244, 58, GOLD)
        rrect(ctx, cx-62+(50 if cx < 512 else 0), 478, 74, 244, 37, "#e8a553")   # head-facing cushion, slightly deeper gold
        rrect(ctx, cx-30, 540, 60, 120, 30, OVERLAY)                                # speaker grille

def render(size, variant="rounded", cat_scale=1.0):
    """variant: rounded | square | circle | transparent"""
    surf = cairo.ImageSurface(cairo.FORMAT_ARGB32, size, size)
    ctx = cairo.Context(surf); ctx.set_antialias(cairo.ANTIALIAS_BEST)
    ctx.scale(size/1024, size/1024)
    if variant == "rounded": rrect(ctx, 0, 0, 1024, 1024, 232, BASE)
    elif variant == "square": ctx.rectangle(0, 0, 1024, 1024); ctx.set_source_rgb(*rgb(BASE)); ctx.fill()
    elif variant == "circle":
        ctx.arc(512, 512, 512, 0, 2*math.pi); ctx.set_source_rgb(*rgb(BASE)); ctx.fill()
    # centre the cat's visual bounding box (x 152..872, y ~188..857) on the canvas
    ctx.translate(512, 512); ctx.scale(cat_scale, cat_scale); ctx.translate(-512, -(188+857)/2)
    cat(ctx, small=size <= 64)
    img = Image.frombuffer("RGBA", (size, size), bytes(surf.get_data()), "raw", "BGRa", 0, 1)
    return img.copy()


# ---------------------------------------------------------------- outputs

HERE = os.path.dirname(os.path.abspath(__file__))


def out(*parts):
    path = os.path.join(HERE, *parts)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    return path


def save_png(name_parts, size, variant="rounded", cat_scale=1.0, opaque=False):
    img = render(size, variant, cat_scale)
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
        save_png(["android", d, "ic_launcher_round.png"], icon, "circle", 0.86)
        save_png(["android", d, "ic_launcher_foreground.png"], fg, "transparent", 0.68)
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
