#!/usr/bin/env python3
"""Generate the NSIS installer bitmaps (header 150x57, sidebar 164x314).

Concrete surfaces from the Spiral tokens, the mark in one color, hairline
rules — same material language as the app and the DMG background.

Run from spiral-wallpaper/:  python3 scripts/make-nsis-images.py
"""

from PIL import Image, ImageDraw, ImageFont

# Spiral tokens (tokens.css)
CONC_01 = "#EBE9E4"
CONC_03 = "#CFCCC4"
STL_02 = "#666863"
HLX_01 = "#D52E2B"

MONO_500 = "src/fonts/ibm-plex-mono-latin-500-normal.woff2"
MARK = "../assets/spiral-mark-256.png"


def mark_at(size: int) -> Image.Image:
    return Image.open(MARK).convert("RGBA").resize((size, size), Image.LANCZOS)


# Header: 150x57, shown top-right of installer pages. Mark + hairline only.
header = Image.new("RGB", (150, 57), CONC_01)
d = ImageDraw.Draw(header)
d.rectangle([0, 56, 150, 57], fill=CONC_03)
m = mark_at(32)
header.paste(m, (150 - 32 - 16, (57 - 32) // 2), m)
header.save("src-tauri/windows/header.bmp")

# Sidebar: 164x314, shown on the Welcome and Finish pages.
side = Image.new("RGB", (164, 314), CONC_01)
d = ImageDraw.Draw(side)
d.rectangle([163, 0, 164, 314], fill=CONC_03)  # hairline against the page
m = mark_at(56)
side.paste(m, ((164 - 56) // 2, 72), m)
# Eyebrow, centered: red dot + uppercase mono.
label = "S P I R A L"
font = ImageFont.truetype(MONO_500, 11)
tw = d.textlength(label, font=font)
x = (164 - (tw + 12)) // 2
d.ellipse([x, 164, x + 5, 169], fill=HLX_01)
d.text((x + 12, 160), label, font=font, fill=STL_02)
side.save("src-tauri/windows/sidebar.bmp")

print("wrote src-tauri/windows/header.bmp (150x57), sidebar.bmp (164x314)")
