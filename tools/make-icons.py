#!/usr/bin/env python3
"""Draws openMapper's logos and application icons from Inter (assets/fonts/InterVariable.ttf).

Writes, in the style of openOMSI's but in blue where openOMSI's is orange:
  assets/logos/*.svg                  the wordmark (dark and light backgrounds, one colour) and the mark
  assets/logos/social-preview.png     1280x640 for the repository's social preview
  assets/icons/app/openmapper*.svg    the icon (Windows / Linux, macOS)
  assets/icons/app/openmapper-*.png   the window icons
  assets/icons/app/openmapper.ico     Windows
  assets/icons/app/openmapper.icns    macOS (needs iconutil, so only on a Mac)

Needs Python 3 with Pillow, fontTools and skia-pathops:  pip install pillow fonttools skia-pathops
"""
import math
import os
import shutil
import subprocess
import sys
import tempfile

from fontTools.pens.basePen import BasePen
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer
from PIL import Image, ImageChops, ImageDraw

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
BLUE = "#2E86FF"
INK = "#14161A"
WHITE = "#FFFFFF"
TILE_TOP, TILE_BOTTOM = "#2D3138", "#14161A"
SLANT = math.tan(math.radians(11))  # the heavy italic of the openOMSI mark

font = instancer.instantiateVariableFont(TTFont(os.path.join(ROOT, "assets/fonts/InterVariable.ttf")), {"wght": 900, "opsz": 32}, overlap=instancer.OverlapMode.REMOVE)
gs = font.getGlyphSet()
cmap = font.getBestCmap()
UPM = font["head"].unitsPerEm


def layout(text):
    """(glyph name, x offset) for each letter, and the text's advance width."""
    x, out = 0, []
    for ch in text:
        name = cmap[ord(ch)]
        out.append((name, x))
        x += gs[name].width
    return out, x


class Flatten(BasePen):
    """Contours as point lists (curves cut into short lines)."""

    def __init__(self, gset):
        super().__init__(gset)
        self.contours, self.cur = [], []

    def _moveTo(self, p):
        self.cur = [p]

    def _lineTo(self, p):
        self.cur.append(p)

    def _curveToOne(self, a, b, c):
        p0 = self.cur[-1]
        for i in range(1, 17):
            t = i / 16
            u = 1 - t
            self.cur.append((u**3 * p0[0] + 3 * u * u * t * a[0] + 3 * u * t * t * b[0] + t**3 * c[0],
                             u**3 * p0[1] + 3 * u * u * t * a[1] + 3 * u * t * t * b[1] + t**3 * c[1]))

    def _qCurveToOne(self, a, b):
        p0 = self.cur[-1]
        for i in range(1, 13):
            t = i / 12
            u = 1 - t
            self.cur.append((u * u * p0[0] + 2 * u * t * a[0] + t * t * b[0], u * u * p0[1] + 2 * u * t * a[1] + t * t * b[1]))

    def _closePath(self):
        if self.cur:
            self.contours.append(self.cur)
        self.cur = []

    _endPath = _closePath


def matrix(size, ox, oy):
    """Font units (y up) to the picture (y down), slanted, `size` pixels to the em."""
    s = size / UPM
    return (s, 0, s * SLANT, -s, ox, oy)


def svg_path(text, size, ox, oy):
    pen = SVGPathPen(gs)
    tp = TransformPen(pen, matrix(size, ox, oy))
    for name, x in layout(text)[0]:
        gs[name].draw(TransformPen(tp, (1, 0, 0, 1, x, 0)))
    return pen.getCommands()


def ink_box(text, size):
    """Left, top, right, bottom of the ink of `text` at `size`, its origin on the baseline at 0,0."""
    pts = []
    for name, x in layout(text)[0]:
        fl = Flatten(gs)
        gs[name].draw(TransformPen(fl, (1, 0, 0, 1, x, 0)))
        for c in fl.contours:
            pts += c
    s = size / UPM
    xs = [s * (px + SLANT * py) for px, py in pts]
    ys = [-s * py for px, py in pts]
    return min(xs), min(ys), max(xs), max(ys)


def fmt(v):
    return f"{v:.2f}".rstrip("0").rstrip(".")


# --- layouts: (text, size, x, y) lines in a box -----------------------------------------

def wordmark_lines(h_pad=0):
    """'open' + 'Mapper' side by side on one baseline; returns lines and the picture's width, height."""
    size = 200
    l1, t1, r1, b1 = ink_box("open", size)
    l2, t2, r2, b2 = ink_box("Mapper", size)
    gap = size * 0.06
    x1 = -l1
    x2 = x1 + r1 + gap - l2
    top = min(t1, t2)
    bottom = max(b1, b2)
    base = -top
    return [("open", size, x1, base, "open"), ("Mapper", size, x2, base, "mapper")], x2 + r2, bottom - top


def icon_lines(width):
    """'open' above 'Mapper', left aligned, 'Mapper' `width` wide; origin at the block's top left."""
    probe = 200
    l2, t2, r2, b2 = ink_box("Mapper", probe)
    size = probe * width / (r2 - l2)
    l1, t1, r1, b1 = ink_box("open", size)
    l2, t2, r2, b2 = ink_box("Mapper", size)
    pitch = size * 1.04
    base1 = -t1
    base2 = base1 + pitch
    lines = [("open", size, -l1, base1, "open"), ("Mapper", size, -l2, base2, "mapper")]
    return lines


def svg_text_group(lines, ox, oy, colors):
    out = []
    for text, size, x, y, role in lines:
        out.append(f'<path d="{svg_path(text, size, ox + x, oy + y)}" fill="{colors[role]}"/>')
    return "\n".join(out)


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(text)


def wordmark_svg(colors):
    lines, w, h = wordmark_lines()
    pad = 24
    W, H = math.ceil(w + 2 * pad), math.ceil(h + 2 * pad)
    body = svg_text_group(lines, pad, pad, colors)
    return f'<svg width="{W}" height="{H}" viewBox="0 0 {W} {H}" fill="none" xmlns="http://www.w3.org/2000/svg">\n{body}\n</svg>\n'


def tile_geometry(kind):
    """Tile rectangle (x, y, w, rx) of the icon in its 1024 square: full bleed for Windows and Linux, inset for macOS."""
    return (24, 976, 200) if kind == "win" else (100, 824, 185)


def icon_layout(kind):
    t, side, rx = tile_geometry(kind)
    bw = side * 0.84
    lines = icon_lines(bw)
    # centre the block (ink top of 'open' to the bottom of 'Mapper') in the tile
    l1, t1, r1, b1 = ink_box("open", lines[0][1])
    l2, t2, r2, b2 = ink_box("Mapper", lines[1][1])
    top = lines[0][3] + t1
    bottom = lines[1][3] + b2
    ox = t + (side - bw) / 2
    oy = t + (side - (bottom - top)) / 2 - top
    return lines, ox, oy


def icon_svg(kind):
    t, side, rx = tile_geometry(kind)
    lines, ox, oy = icon_layout(kind)
    body = svg_text_group(lines, ox, oy, {"open": WHITE, "mapper": BLUE})
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">\n'
            f'<defs><linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{TILE_TOP}"/>'
            f'<stop offset="1" stop-color="{TILE_BOTTOM}"/></linearGradient></defs>\n'
            f'<rect x="{t}" y="{t}" width="{side}" height="{side}" rx="{rx}" fill="url(#bg)"/>\n{body}\n</svg>\n')


# --- raster (Pillow, the same outlines) ---------------------------------------------------

def fill_text(img, lines, ox, oy, scale, color):
    """Paints the lines of one role (all in `color`) onto an RGBA picture, `scale` times the layout's units."""
    size = img.size
    outer = Image.new("L", size, 0)
    holes = Image.new("L", size, 0)
    do, dh = ImageDraw.Draw(outer), ImageDraw.Draw(holes)
    for text, fs, x, y, role in lines:
        s = fs / UPM
        for name, gx in layout(text)[0]:
            fl = Flatten(gs)
            gs[name].draw(TransformPen(fl, (1, 0, 0, 1, gx, 0)))
            for c in fl.contours:
                poly = [(scale * (ox + x + s * (px + SLANT * py)), scale * (oy + y - s * py)) for px, py in c]
                area = sum(a[0] * b[1] - b[0] * a[1] for a, b in zip(poly, poly[1:] + poly[:1]))
                # (y points down here: a positive area is a contour drawn the font's outer way)
                (do if area > 0 else dh).polygon(poly, fill=255)
    mask = ImageChops.multiply(outer, ImageChops.invert(holes))
    layer = Image.new("RGBA", size, color)
    img.alpha_composite(Image.composite(layer, Image.new("RGBA", size, (0, 0, 0, 0)), mask))


def hexrgb(h):
    return tuple(int(h[i:i + 2], 16) for i in (1, 3, 5)) + (255,)


def icon_png(kind, px=1024):
    ss = 4
    S = px * ss
    k = S / 1024
    t, side, rx = tile_geometry(kind)
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    grad = Image.new("RGBA", (1, 256))
    a, b = hexrgb(TILE_TOP), hexrgb(TILE_BOTTOM)
    for i in range(256):
        grad.putpixel((0, i), tuple(round(a[j] + (b[j] - a[j]) * i / 255) for j in range(4)))
    grad = grad.resize((round(side * k), round(side * k)), Image.BILINEAR)
    m = Image.new("L", grad.size, 0)
    ImageDraw.Draw(m).rounded_rectangle((0, 0, grad.size[0] - 1, grad.size[1] - 1), radius=rx * k, fill=255)
    img.paste(grad, (round(t * k), round(t * k)), m)
    lines, ox, oy = icon_layout(kind)
    fill_text(img, [l for l in lines if l[4] == "open"], ox, oy, k, hexrgb(WHITE))
    fill_text(img, [l for l in lines if l[4] == "mapper"], ox, oy, k, hexrgb(BLUE))
    return img.resize((px, px), Image.LANCZOS)


def social_png():
    W, H, ss = 1280, 640, 2
    img = Image.new("RGBA", (W * ss, H * ss))
    a, b = hexrgb(TILE_TOP), hexrgb(TILE_BOTTOM)
    ImageDraw.Draw(img).rectangle((0, 0, W * ss, H * ss), fill=b)
    grad = Image.new("RGBA", (1, 256))
    for i in range(256):
        grad.putpixel((0, i), tuple(round(a[j] + (b[j] - a[j]) * i / 255) for j in range(4)))
    img.paste(grad.resize((W * ss, H * ss), Image.BILINEAR), (0, 0))
    lines, w, h = wordmark_lines()
    k = 0.78 * W / w
    # the wordmark scaled: lay it out at the scale by scaling every size and offset
    scaled = [(t, s * k, x * k, y * k, r) for t, s, x, y, r in lines]
    ox, oy = (W - w * k) / 2, (H - h * k) / 2 - 20
    fill_text(img, [l for l in scaled if l[4] == "open"], ox, oy, ss, hexrgb(WHITE))
    fill_text(img, [l for l in scaled if l[4] == "mapper"], ox, oy, ss, hexrgb(BLUE))
    # a tagline in the same family, plain
    from PIL import ImageFont
    f = ImageFont.truetype(os.path.join(ROOT, "assets/fonts/InterVariable.ttf"), 38 * ss)
    try:
        f.set_variation_by_axes([32, 500])
    except Exception:
        pass
    text = "A simple map editor for openOMSI"
    d = ImageDraw.Draw(img)
    tw = d.textlength(text, font=f)
    d.text(((W * ss - tw) / 2, ((H + h * k) / 2 + 24) * ss), text, font=f, fill=(190, 196, 206, 255))
    return img.resize((W, H), Image.LANCZOS).convert("RGB")


def main():
    logos = os.path.join(ROOT, "assets/logos")
    write(os.path.join(logos, "openmapper-wordmark-dark.svg"), wordmark_svg({"open": INK, "mapper": BLUE}))
    write(os.path.join(logos, "openmapper-wordmark-light.svg"), wordmark_svg({"open": WHITE, "mapper": BLUE}))
    write(os.path.join(logos, "openmapper-wordmark-mono-dark.svg"), wordmark_svg({"open": INK, "mapper": INK}))
    write(os.path.join(logos, "openmapper-wordmark-mono-light.svg"), wordmark_svg({"open": WHITE, "mapper": WHITE}))
    social_png().save(os.path.join(logos, "social-preview.png"), optimize=True)

    app = os.path.join(ROOT, "assets/icons/app")
    write(os.path.join(app, "openmapper.svg"), icon_svg("win"))
    write(os.path.join(app, "openmapper-macos.svg"), icon_svg("mac"))
    big = icon_png("win", 1024)
    big.resize((256, 256), Image.LANCZOS).save(os.path.join(app, "openmapper-256.png"), optimize=True)
    big.resize((512, 512), Image.LANCZOS).save(os.path.join(app, "openmapper-512.png"), optimize=True)
    ims = [big.resize((n, n), Image.LANCZOS) for n in (256, 128, 64, 48, 32, 24, 16)]
    ims[0].save(os.path.join(app, "openmapper.ico"), sizes=[i.size for i in ims], append_images=ims[1:])
    mac = icon_png("mac", 1024)
    mac.save(os.path.join(app, "openmapper-macos-1024.png"), optimize=True)
    if shutil.which("iconutil"):
        tmp = tempfile.mkdtemp()
        iconset = os.path.join(tmp, "icon.iconset")
        os.mkdir(iconset)
        for n in (16, 32, 128, 256, 512):
            mac.resize((n, n), Image.LANCZOS).save(os.path.join(iconset, f"icon_{n}x{n}.png"))
            mac.resize((2 * n, 2 * n), Image.LANCZOS).save(os.path.join(iconset, f"icon_{n}x{n}@2x.png"))
        subprocess.check_call(["iconutil", "-c", "icns", iconset, "-o", os.path.join(app, "openmapper.icns")])
        shutil.rmtree(tmp)
    else:
        print("no iconutil: openmapper.icns not rewritten", file=sys.stderr)
    print("logos in assets/logos, icons in assets/icons/app")


if __name__ == "__main__":
    main()
