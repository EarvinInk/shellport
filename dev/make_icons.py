#!/usr/bin/env python3
"""Render Shellport's app icons (purple gradient tile with a `>_` prompt).

    python3 dev/make_icons.py

Writes src-tauri/icons/{32x32,128x128,128x128@2x,icon}.png and public/logo.svg.
"""
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
TOP = (167, 139, 250)  # violet-400
BOTTOM = (109, 40, 217)  # violet-700
SS = 4  # supersampling factor for smooth edges


def tile(size: int) -> Image.Image:
    n = size * SS
    grad = Image.new("RGBA", (n, n))
    px = grad.load()
    assert px is not None
    for y in range(n):
        for x in range(n):
            t = (x * 0.35 + y * 0.65) / n
            px[x, y] = tuple(int(TOP[i] + (BOTTOM[i] - TOP[i]) * t) for i in range(3)) + (255,)
    mask = Image.new("L", (n, n), 0)
    pad = int(n * 0.06)
    ImageDraw.Draw(mask).rounded_rectangle([pad, pad, n - pad, n - pad], radius=int(n * 0.22), fill=255)
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    img.paste(grad, (0, 0), mask)

    d = ImageDraw.Draw(img)
    w = max(int(n * 0.075), SS)
    white = (255, 255, 255, 255)
    # Chevron ">"
    x0, x1 = n * 0.27, n * 0.47
    yc, h = n * 0.5, n * 0.17
    d.line([(x0, yc - h), (x1, yc), (x0, yc + h)], fill=white, width=w, joint="curve")
    for p in [(x0, yc - h), (x0, yc + h), (x1, yc)]:
        d.ellipse([p[0] - w / 2, p[1] - w / 2, p[0] + w / 2, p[1] + w / 2], fill=white)
    # Underscore "_"
    ux0, ux1, uy = n * 0.52, n * 0.74, yc + h
    d.rounded_rectangle([ux0, uy - w / 2, ux1, uy + w / 2], radius=w / 2, fill=white)
    return img.resize((size, size), Image.Resampling.LANCZOS)


SVG = """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">
  <defs>
    <linearGradient id="g" x1="0" y1="0" x2="0.35" y2="1">
      <stop offset="0" stop-color="#a78bfa"/>
      <stop offset="1" stop-color="#6d28d9"/>
    </linearGradient>
  </defs>
  <rect x="4" y="4" width="56" height="56" rx="14" fill="url(#g)"/>
  <path d="M17.3 21.1 30.1 32 17.3 42.9" fill="none" stroke="#fff" stroke-width="4.8" stroke-linecap="round" stroke-linejoin="round"/>
  <path d="M33.3 42.9h14" stroke="#fff" stroke-width="4.8" stroke-linecap="round"/>
</svg>
"""

if __name__ == "__main__":
    icons = ROOT / "src-tauri" / "icons"
    for size, name in [(32, "32x32.png"), (128, "128x128.png"), (256, "128x128@2x.png"), (512, "icon.png")]:
        tile(size).save(icons / name)
        print("wrote", icons / name)
    pub = ROOT / "public"
    pub.mkdir(exist_ok=True)
    (pub / "logo.svg").write_text(SVG)
    print("wrote", pub / "logo.svg")
