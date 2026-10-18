#!/usr/bin/env python3
"""Regenerate the app icon, UI logo and README banner from docs/assets/brand.

Needs Pillow. Run from the repository root: python3 scripts/make-icons.py
"""
import io
import struct
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
BRAND = ROOT / "docs/assets/brand"
STACK = Image.open(BRAND / "nouwall-icon-source.png").convert("RGB")
BANNER = Image.open(BRAND / "nouwall-banner-source.png").convert("RGB")

# Full card stack for large sizes; just the front wave card below 48 px,
# where the stacked cards would blur into noise.
FULL = STACK.crop((115, 95, 1135, 1115))
FRONT = STACK.crop((95, 175, 785, 865))


def tile(src: Image.Image, size: int) -> Image.Image:
    """Square, rounded tile with transparent corners, drawn at 4x then reduced."""
    big = size * 4
    img = src.resize((big, big), Image.LANCZOS).convert("RGBA")
    mask = Image.new("L", (big, big), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, big - 1, big - 1), radius=int(big * 0.22), fill=255)
    img.putalpha(mask)
    return img.resize((size, size), Image.LANCZOS)


def png_bytes(img: Image.Image) -> bytes:
    buf = io.BytesIO()
    img.save(buf, "PNG", optimize=True)
    return buf.getvalue()


def write_ico(path: Path, sizes):
    images = [(s, png_bytes(tile(FRONT if s < 48 else FULL, s))) for s in sizes]
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries, blobs = b"", b""
    for s, data in images:
        entries += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(data), offset + len(blobs))
        blobs += data
    path.write_bytes(header + entries + blobs)


write_ico(ROOT / "crates/nouwall/assets/icon/nouwall.ico", [16, 20, 24, 32, 40, 48, 64, 128, 256])
tile(FULL, 128).save(ROOT / "crates/nouwall/assets/icon/logo.png", optimize=True)
tile(FULL, 256).save(ROOT / "docs/assets/icon.png", optimize=True)
BANNER.resize((1000, 1000), Image.LANCZOS).save(ROOT / "docs/assets/banner.jpg", quality=90, optimize=True, progressive=True)
print("icons written")
