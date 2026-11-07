#!/usr/bin/env python3
"""Turn the game's raw store renders into the files the stores take.

    cargo run --release -p mofact -- --store RAW --scenario tools/store/showcase.ron
    python3 tools/store/compose.py RAW docs/store

Screenshots become JPEGs (the App Store refuses PNGs with an alpha channel,
and JPEGs keep the repository small). The feature graphic and Steam's
capsules get the game's name, set in the game's own font, over a shade on
one side; Steam's library hero stays without text, as Steam asks, and its
logo is drawn on its own with a transparent background. Needs Pillow.
"""

import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[2]
FONT = ROOT / "assets" / "fonts" / "SairaSemiCondensed-SemiBold.ttf"
NAME = "MOFACT"
TAGLINE = "Build a factory that runs itself"
INK = (255, 255, 255)
ACCENT = (244, 180, 64)  # the hub's hazard yellow
SHADE = (14, 22, 18)


def font(px):
    return ImageFont.truetype(str(FONT), int(px))


def shade(im, side, reach=0.62, strength=0.88):
    """Darken one side of `im` (left, bottom or top), fading out by `reach`."""
    w, h = im.size
    mask = Image.new("L", (w, h), 0)
    d = ImageDraw.Draw(mask)
    span = int((w if side == "left" else h) * reach)
    for i in range(span):
        a = int(255 * strength * (1 - i / span) ** 1.6)
        if side == "left":
            d.line([(i, 0), (i, h)], fill=a)
        elif side == "bottom":
            d.line([(0, h - 1 - i), (w, h - 1 - i)], fill=a)
        else:
            d.line([(0, i), (w, i)], fill=a)
    return Image.composite(Image.new("RGB", (w, h), SHADE), im, mask)


def title(im, at, size, tagline=True, anchor="ls"):
    """The name, with an accent bar under it and the tagline below."""
    d = ImageDraw.Draw(im)
    x, y = at
    f = font(size)
    # A soft drop shadow, so the name reads on any part of the map.
    for dx, dy in ((0, size * 0.04),):
        d.text((x + dx, y + dy), NAME, font=f, fill=(0, 0, 0), anchor=anchor)
    d.text((x, y), NAME, font=f, fill=INK, anchor=anchor)
    left, _, right, bottom = d.textbbox((x, y), NAME, font=f, anchor=anchor)
    bar = max(2, int(size * 0.07))
    gap = int(size * 0.12)
    d.rectangle([left, bottom + gap, left + (right - left) * 0.42, bottom + gap + bar], fill=ACCENT)
    if tagline:
        t = font(size * 0.3)
        d.text((left, bottom + gap * 2 + bar), TAGLINE, font=t, fill=INK, anchor="lt")


def art(raw, out):
    a = raw / "art"
    out.mkdir(parents=True, exist_ok=True)

    def load(name):
        return Image.open(a / f"{name}.png").convert("RGB")

    # Google Play feature graphic, 1024 x 500. Play may crop the edges and
    # lays a play button in the middle for a video, so the name sits left.
    im = shade(load("play-feature"), "left")
    title(im, (64, 250), 120)
    im.save(out / "play-feature-graphic-1024x500.png")

    # Steam header capsule 920 x 430, main capsule 1232 x 706, small 462 x 174.
    for name, size, at, px, tag in (
        ("steam-header", (920, 430), (56, 220), 110, True),
        ("steam-main", (1232, 706), (72, 380), 170, True),
        ("steam-small", (462, 174), (28, 112), 76, False),
    ):
        im = shade(load(name), "left")
        title(im, at, px, tagline=tag)
        im.save(out / f"{name}-{size[0]}x{size[1]}.png")

    # Steam library capsule, 600 x 900: the name at the top.
    im = shade(load("steam-library"), "top", reach=0.5)
    d = ImageDraw.Draw(im)
    title(im, (300, 190), 150, tagline=False, anchor="ms")
    t = font(40)
    d.text((300, 270), TAGLINE, font=t, fill=INK, anchor="mt")
    im.save(out / "steam-library-600x900.png")

    # Steam library hero, 3840 x 1240: art only, no text.
    load("steam-hero").save(out / "steam-library-hero-3840x1240.png")

    # Steam library logo: the name alone on transparent, 1280 x 720.
    logo = Image.new("RGBA", (1280, 720), (0, 0, 0, 0))
    d = ImageDraw.Draw(logo)
    f = font(300)
    d.text((640, 330), NAME, font=f, fill=INK + (255,), anchor="ms",
           stroke_width=10, stroke_fill=SHADE + (255,))
    l, _, r, b = d.textbbox((640, 330), NAME, font=f, anchor="ms")
    d.rectangle([l, b + 34, l + (r - l) * 0.42, b + 54], fill=ACCENT + (255,))
    logo.save(out / "steam-library-logo-1280x720.png")


def screenshots(raw, out):
    for folder in sorted(p for p in raw.iterdir() if p.is_dir() and p.name != "art"):
        dest = out / folder.name
        dest.mkdir(parents=True, exist_ok=True)
        for png in sorted(folder.glob("*.png")):
            Image.open(png).convert("RGB").save(dest / f"{png.stem}.jpg", quality=90, optimize=True)


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    raw, out = Path(sys.argv[1]), Path(sys.argv[2])
    screenshots(raw, out / "screenshots")
    art(raw, out / "art")


if __name__ == "__main__":
    main()
