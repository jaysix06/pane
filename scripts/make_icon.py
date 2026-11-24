"""Draws Pane's icon: an amber rounded tile with a dot-matrix "P", the same
mark as the title bar, at every size Windows asks for. Writes
assets/pane.ico (and assets/pane-256.png for README/release use).

    python scripts/make_icon.py
"""
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

AMBER = (255, 176, 32, 255)
AMBER_DEEP = (232, 140, 10, 255)
INK = (26, 19, 7, 255)

# The "P" as a 3x4 dot grid on a 12-unit canvas (same as tabs.luau's mark).
DOTS = [(3, 2), (6, 2), (9, 3.2), (3, 5), (9, 5.8), (3, 8), (6, 7), (3, 11)]


def tile(size: int) -> Image.Image:
    s = size * 4  # supersample, then shrink for smooth edges
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    pad = s * 0.06
    radius = s * 0.24
    # Vertical gradient tile.
    grad = Image.new("RGBA", (s, s))
    gd = ImageDraw.Draw(grad)
    for y in range(s):
        t = y / s
        c = tuple(int(AMBER[i] + (AMBER_DEEP[i] - AMBER[i]) * t) for i in range(4))
        gd.line([(0, y), (s, y)], fill=c)
    mask = Image.new("L", (s, s), 0)
    ImageDraw.Draw(mask).rounded_rectangle([pad, pad, s - pad, s - pad], radius=radius, fill=255)
    img.paste(grad, (0, 0), mask)
    # A soft top highlight, like the app's machined card edges.
    hl = Image.new("L", (s, s), 0)
    ImageDraw.Draw(hl).rounded_rectangle([pad, pad, s - pad, s * 0.5], radius=radius, fill=38)
    white = Image.new("RGBA", (s, s), (255, 255, 255, 255))
    img.paste(white, (0, 0), Image.composite(hl, Image.new("L", (s, s), 0), mask))
    # The dots.
    inner = s - 2 * pad
    unit = inner * 0.62 / 12
    ox = pad + (inner - 12 * unit) / 2 + unit * 0.4
    oy = pad + (inner - 13 * unit) / 2
    r = unit * (1.15 if size >= 32 else 1.35)
    for x, y in DOTS:
        cx, cy = ox + x * unit, oy + y * unit
        d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=INK)
    return img.resize((size, size), Image.LANCZOS)


def main() -> None:
    root = Path(__file__).resolve().parent.parent
    assets = root / "assets"
    assets.mkdir(exist_ok=True)
    sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
    images = [tile(n) for n in sizes]
    images[-1].save(assets / "pane.ico", sizes=[(n, n) for n in sizes], append_images=images[:-1])
    images[-1].save(assets / "pane-256.png")
    print("wrote", assets / "pane.ico")


if __name__ == "__main__":
    main()
