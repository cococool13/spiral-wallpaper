#!/usr/bin/env python3
"""Generate the macOS DMG background (concrete, mark, red drag cue).

Renders at 2x (1320x800 @ 144dpi) for a 660x400pt Finder window — matches
bundle.macOS.dmg in tauri.conf.json (app at 180,170; Applications at 480,170).
Colors and type come from the Spiral tokens; fonts are the app's own woff2s.

Run from spiral-wallpaper/:  python3 scripts/make-dmg-background.py
"""

from PIL import Image, ImageDraw, ImageFont

# Spiral tokens (tokens.css)
CONC_01 = "#EBE9E4"
CONC_03 = "#CFCCC4"
INK_01 = "#10181B"
STL_02 = "#666863"
HLX_01 = "#D52E2B"

S = 2  # retina scale
W, H = 660 * S, 400 * S

MONO_500 = "src/fonts/ibm-plex-mono-latin-500-normal.woff2"
MONO_400 = "src/fonts/ibm-plex-mono-latin-400-normal.woff2"
MARK = "../assets/spiral-mark-256.png"
OUT = "src-tauri/dmg/background.png"

img = Image.new("RGB", (W, H), CONC_01)
draw = ImageDraw.Draw(img)


def pt(v: float) -> int:
    return round(v * S)


# Hairline rules top and bottom — the concrete surface is framed, not decorated.
draw.rectangle([pt(32), pt(44), W - pt(32), pt(44) + S], fill=CONC_03)
draw.rectangle([pt(32), pt(356), W - pt(32), pt(356) + S], fill=CONC_03)

# Eyebrow: red dot + uppercase mono label, top-left inside the frame.
eyebrow = ImageFont.truetype(MONO_500, pt(11))
draw.ellipse([pt(32), pt(25), pt(32 + 6), pt(25 + 6)], fill=HLX_01)
draw.text((pt(46), pt(22)), "S P I R A L   W A L L P A P E R", font=eyebrow, fill=STL_02)

# The mark, one color (red), centered above the icon row.
mark = Image.open(MARK).convert("RGBA").resize((pt(44), pt(44)), Image.LANCZOS)
img.paste(mark, (W // 2 - pt(22), pt(72)), mark)

# Red drag cue: a straight line with an arrowhead, icon-center to icon-center.
# Icons sit at y=170; leave clearance around the 80pt icons themselves.
y = pt(170)
x0, x1 = pt(248), pt(404)
draw.line([x0, y, x1 - pt(10), y], fill=HLX_01, width=pt(3))
draw.polygon(
    [(x1, y), (x1 - pt(14), y - pt(8)), (x1 - pt(14), y + pt(8))], fill=HLX_01
)

# Stated, not sold.
body = ImageFont.truetype(MONO_400, pt(12))
line = "Drag Spiral into Applications. That's the whole install."
tw = draw.textlength(line, font=body)
draw.text(((W - tw) // 2, pt(300)), line, font=body, fill=INK_01)

sub = ImageFont.truetype(MONO_400, pt(11))
line2 = "4.6 MB. No account. Nothing runs until you open it."
tw2 = draw.textlength(line2, font=sub)
draw.text(((W - tw2) // 2, pt(322)), line2, font=sub, fill=STL_02)

import os

os.makedirs(os.path.dirname(OUT), exist_ok=True)
img.save(OUT, dpi=(144, 144))
print(f"wrote {OUT} ({W}x{H} @144dpi = 660x400pt)")
