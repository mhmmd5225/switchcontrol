#!/usr/bin/env python3
"""Draws the app icon: two screens joined by a link, on a blue rounded square.

Writes assets/icon.png (1024 px, also used for the macOS .icns),
assets/icon-256.png (window icon) and assets/icon.ico (Windows .exe).
Needs Pillow. Run from anywhere: python3 scripts/make-icons.py
"""
from pathlib import Path

from PIL import Image, ImageDraw

SIZE = 1024
SCALE = 4  # draw large, then shrink for smooth edges
ASSETS = Path(__file__).resolve().parent.parent / "assets"


def lerp(a, b, t):
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))


def draw(inset: int) -> Image.Image:
    """`inset`: empty margin around the rounded square, in 1024-px units."""
    n = SIZE * SCALE
    s = lambda v: round(v * SCALE)  # noqa: E731

    # Blue rounded square with a vertical gradient, inset like macOS icons.
    top, bottom = (0x5B, 0x9B, 0xFF), (0x1D, 0x4E, 0xD8)
    gradient = Image.new("RGBA", (n, n))
    g = ImageDraw.Draw(gradient)
    for y in range(n):
        g.line([(0, y), (n, y)], fill=lerp(top, bottom, y / n) + (255,))
    mask = Image.new("L", (n, n), 0)
    m = s(inset)
    radius = s(185 * (SIZE - 2 * inset) / 824)
    ImageDraw.Draw(mask).rounded_rectangle([m, m, n - m, n - m], radius=radius, fill=255)
    icon = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    icon.paste(gradient, (0, 0), mask)

    d = ImageDraw.Draw(icon)
    white = (255, 255, 255, 255)
    soft = (255, 255, 255, 150)

    # Two screens with stands, scaled with the square.
    k = (SIZE - 2 * inset) / 824
    w, h, gap = 280 * k, 196 * k, 96 * k
    y0 = SIZE / 2 - (h + 66 * k) / 2
    left_x = (SIZE - (2 * w + gap)) / 2
    for i, x in enumerate([left_x, left_x + w + gap]):
        box = [s(x), s(y0), s(x + w), s(y0 + h)]
        if i == 0:
            # This computer: outlined.
            d.rounded_rectangle(box, radius=s(26 * k), outline=white, width=s(22 * k))
        else:
            # The other computer: filled, it has the keyboard and mouse.
            d.rounded_rectangle(box, radius=s(26 * k), fill=white)
        cx = x + w / 2
        d.rectangle(
            [s(cx - 16 * k), s(y0 + h + 8 * k), s(cx + 16 * k), s(y0 + h + 52 * k)], fill=soft
        )
        d.rounded_rectangle(
            [s(cx - 70 * k), s(y0 + h + 48 * k), s(cx + 70 * k), s(y0 + h + 66 * k)],
            radius=s(9 * k),
            fill=soft,
        )

    # The link between them.
    ly = y0 + h / 2
    x0, x1 = left_x + w + 14 * k, left_x + w + gap - 14 * k
    d.rounded_rectangle(
        [s(x0), s(ly - 11 * k), s(x1), s(ly + 11 * k)], radius=s(11 * k), fill=white
    )

    return icon.resize((SIZE, SIZE), Image.LANCZOS)


def main():
    ASSETS.mkdir(exist_ok=True)
    # macOS wants the margin of its icon grid; Windows icons fill their square.
    draw(100).save(ASSETS / "icon.png")
    tight = draw(24)
    tight.resize((256, 256), Image.LANCZOS).save(ASSETS / "icon-256.png")
    tight.save(
        ASSETS / "icon.ico",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )


if __name__ == "__main__":
    main()
