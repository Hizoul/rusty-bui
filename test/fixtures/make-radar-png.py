#!/usr/bin/env python3
"""Write a small synthetic radar frame, so the map can be rendered without the network.

The render harness serves the plugins a captured payload and a captured *image*, because a
map that draws nothing is a map that proves nothing: this draws a band of rain crossing the
box with a heavier core inside it, in the colours the composite uses, over transparency.

    python test/fixtures/make-radar-png.py     # writes test/fixtures/radar-sample.png
"""

import pathlib
import struct
import zlib

HERE = pathlib.Path(__file__).resolve().parent
TARGET = HERE / "radar-sample.png"

WIDTH = 400
HEIGHT = 250


def pixel(x: int, y: int) -> tuple:
    """A front running south-west to north-east, transparent everywhere else."""
    along = (x / WIDTH) * 0.8 + (1 - y / HEIGHT) * 0.6
    if not 0.5 < along < 0.95:
        return (0, 0, 0, 0)
    strength = 1 - abs(along - 0.72) / 0.23
    if strength > 0.65:
        return (150, 60, 190, 235)  # heavy: violet
    if strength > 0.35:
        return (60, 140, 235, 210)  # moderate: blue
    return (110, 205, 240, 170)  # light: cyan


def chunk(kind: bytes, data: bytes) -> bytes:
    body = kind + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)


def main() -> None:
    rows = bytearray()
    for y in range(HEIGHT):
        rows.append(0)  # filter: none
        for x in range(WIDTH):
            rows.extend(pixel(x, y))

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", WIDTH, HEIGHT, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(rows), 9))
    png += chunk(b"IEND", b"")
    TARGET.write_bytes(png)
    print(f"wrote {TARGET} ({TARGET.stat().st_size // 1024} KB, {WIDTH}x{HEIGHT})")


if __name__ == "__main__":
    main()
