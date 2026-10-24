#!/usr/bin/env python3
"""Generates the LeoView lioness avatar as SVG and PNG from one set of shapes.

Usage: python3 assets/make_icon.py   (needs Pillow; writes into assets/)
"""
from pathlib import Path

from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parent
SIZE = 512

BG = "#1E2A38"
BG_RING = "#2C3D52"
FUR = "#D9A441"
FUR_DARK = "#B07A2C"
FUR_LIGHT = "#EDC779"
MUZZLE = "#F6E3BC"
EAR_INNER = "#7A4A24"
NOSE = "#6E3B2A"
LINE = "#3A2416"
EYE = "#F3D36B"

# Shapes are (color, path) where a path is a start point followed by cubic segments
# (c1, c2, end). Shapes listed with mirror=True are reflected around x = 256.


def mirror_x(p):
    return (SIZE - p[0], p[1])


def symmetric(right_half):
    """Closes a right-half outline (top center -> bottom center) into a full shape."""
    start, segs = right_half[0], right_half[1:]
    # Walk the right half, then back up the mirrored left half.
    pts = [start] + [p for seg in segs for p in seg]
    rev = list(reversed(pts))
    left = []
    for i in range(0, len(rev) - 1, 3):
        c1, c2, end = rev[i + 1], rev[i + 2], rev[i + 3]
        left.append((mirror_x(c1), mirror_x(c2), mirror_x(end)))
    return [start] + segs + left


def mirrored(path):
    return [mirror_x(path[0])] + [tuple(mirror_x(p) for p in seg) for seg in path[1:]]


EAR = [(150, 196), ((128, 130), (150, 92), (196, 96)), ((226, 100), (240, 128), (238, 150)),
       ((210, 160), (176, 178), (150, 196))]
EAR_IN = [(170, 176), ((160, 136), (176, 116), (198, 118)), ((216, 122), (222, 138), (220, 150)),
          ((200, 156), (184, 166), (170, 176))]

HEAD = symmetric([
    (256, 112),
    ((318, 112), (366, 140), (378, 196)),
    ((392, 256), (378, 306), (342, 350)),
    ((312, 388), (290, 420), (256, 422)),
])
BRIDGE = symmetric([
    (256, 140),
    ((270, 180), (280, 240), (284, 288)),
    ((276, 296), (266, 298), (256, 298)),
])
MUZZLE_SHAPE = symmetric([
    (256, 284),
    ((300, 280), (330, 312), (328, 344)),
    ((326, 380), (292, 404), (256, 404)),
])
NOSE_SHAPE = symmetric([
    (256, 286),
    ((270, 286), (286, 288), (288, 294)),
    ((286, 308), (268, 322), (256, 326)),
])
EYE_SHAPE = [(278, 238), ((290, 212), (326, 206), (344, 220)),
             ((334, 244), (300, 254), (278, 238))]
EYE_PATCH = [(276, 246), ((296, 264), (330, 262), (348, 232))]
CHEEK_MARK = [(300, 262), ((320, 262), (348, 270), (362, 288))]
TEAR = [(286, 244), ((290, 258), (292, 270), (290, 284))]
MOUTH = [(256, 326), ((256, 340), (256, 346), (256, 350))]
MOUTH_CURVE = [(256, 350), ((264, 362), (280, 362), (288, 352))]


def flatten(path, steps=24):
    pts = [path[0]]
    p0 = path[0]
    for c1, c2, p3 in path[1:]:
        for i in range(1, steps + 1):
            t = i / steps
            mt = 1 - t
            x = mt**3 * p0[0] + 3 * mt**2 * t * c1[0] + 3 * mt * t**2 * c2[0] + t**3 * p3[0]
            y = mt**3 * p0[1] + 3 * mt**2 * t * c1[1] + 3 * mt * t**2 * c2[1] + t**3 * p3[1]
            pts.append((x, y))
        p0 = p3
    return pts


def svg_path(path, closed=True):
    d = "M{:g},{:g}".format(*path[0])
    for c1, c2, p in path[1:]:
        d += " C{:g},{:g} {:g},{:g} {:g},{:g}".format(*c1, *c2, *p)
    return d + (" Z" if closed else "")


# (kind, color, path, mirror, width) — kind: "fill" or "stroke"
LAYERS = [
    ("fill", FUR_DARK, EAR, True, 0),
    ("fill", EAR_INNER, EAR_IN, True, 0),
    ("fill", FUR, HEAD, False, 0),
    ("fill", FUR_LIGHT, BRIDGE, False, 0),
    ("stroke", FUR_DARK, CHEEK_MARK, True, 6),
    ("fill", MUZZLE, MUZZLE_SHAPE, False, 0),
    ("stroke", MUZZLE, EYE_PATCH, True, 10),
    ("fill", EYE, EYE_SHAPE, True, 0),
    ("stroke", LINE, EYE_SHAPE, True, 6),
    ("stroke", FUR_DARK, TEAR, True, 6),
    ("fill", NOSE, NOSE_SHAPE, False, 0),
    ("stroke", LINE, MOUTH, False, 6),
    ("stroke", LINE, MOUTH_CURVE, True, 6),
]
PUPIL = ((312, 229), 9, 9)  # center, rx, ry


FACE_SCALE = 1.12


def place(p):
    """Scales the face up a bit and centers it in the frame."""
    return (256 + (p[0] - 256) * FACE_SCALE, 256 + (p[1] - 259) * FACE_SCALE)


def transform(path):
    return [place(path[0])] + [tuple(place(p) for p in seg) for seg in path[1:]]


def expand(layers):
    for kind, color, path, mirror, width in layers:
        yield kind, color, transform(path), width * FACE_SCALE
        if mirror:
            yield kind, color, transform(mirrored(path)), width * FACE_SCALE


def pupils():
    (cx, cy), rx, ry = PUPIL
    for x in (cx, SIZE - cx):
        yield place((x, cy)), rx * FACE_SCALE, ry * FACE_SCALE


def write_svg():
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {SIZE} {SIZE}">',
        f'<rect x="8" y="8" width="496" height="496" rx="116" fill="{BG}" stroke="{BG_RING}" stroke-width="10"/>',
    ]
    for kind, color, path, width in expand(LAYERS):
        if kind == "fill":
            parts.append(f'<path d="{svg_path(path)}" fill="{color}"/>')
        else:
            closed = path[0] == path[-1][2]
            parts.append(
                f'<path d="{svg_path(path, closed)}" fill="none" stroke="{color}" '
                f'stroke-width="{width:g}" stroke-linecap="round" stroke-linejoin="round"/>'
            )
    for (x, y), rx, ry in pupils():
        parts.append(f'<ellipse cx="{x:g}" cy="{y:g}" rx="{rx:g}" ry="{ry:g}" fill="{LINE}"/>')
    parts.append("</svg>")
    (OUT / "icon.svg").write_text("\n".join(parts) + "\n")


def render_png(size):
    ss = 4  # supersampling factor for anti-aliasing
    scale = size * ss / SIZE
    img = Image.new("RGBA", (size * ss, size * ss), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    def s(pts):
        return [(x * scale, y * scale) for x, y in pts]

    ring = round(10 * scale)
    d.rounded_rectangle(
        [8 * scale, 8 * scale, 504 * scale, 504 * scale],
        radius=116 * scale, fill=BG, outline=BG_RING, width=ring,
    )
    for kind, color, path, width in expand(LAYERS):
        pts = s(flatten(path))
        if kind == "fill":
            d.polygon(pts, fill=color)
        else:
            w = max(1, round(width * scale))
            d.line(pts, fill=color, width=w, joint="curve")
            for x, y in (pts[0], pts[-1]):  # round caps
                d.ellipse([x - w / 2, y - w / 2, x + w / 2, y + w / 2], fill=color)
    for (x, y), rx, ry in pupils():
        d.ellipse([(x - rx) * scale, (y - ry) * scale, (x + rx) * scale, (y + ry) * scale], fill=LINE)
    return img.resize((size, size), Image.LANCZOS)


if __name__ == "__main__":
    write_svg()
    for size in (256, 512):
        render_png(size).save(OUT / f"icon-{size}.png")
    print("wrote", OUT)
