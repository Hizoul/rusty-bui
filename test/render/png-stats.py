#!/usr/bin/env python3
"""Report basic facts about a PNG written by the render harness.

Stdlib only (the build container has no Pillow). Proves an image is not blank without a
human looking at it: a real render has many distinct colours and a non-trivial spread of
luminance, where a failed one is one flat colour.

    python test/render/png-stats.py test/render/out/*.png [--bands 12]

Columns: file, width, height, distinct colours, share of pixels that differ from the most
common colour, luminance spread (max - min). `--bands N` also prints ink per horizontal band,
which is how to tell where the content landed in a layout nobody can look at.
"""

import struct
import sys
import zlib
from collections import Counter


def read_png(path):
    data = path and open(path, "rb").read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"{path}: not a PNG")
    pos, idat, header = 8, bytearray(), None
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        kind = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + length]
        if kind == b"IHDR":
            header = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            idat += body
        elif kind == b"IEND":
            break
        pos += 12 + length

    width, height, depth, color_type = header[0], header[1], header[2], header[3]
    if depth != 8:
        raise ValueError(f"{path}: {depth}-bit not supported")
    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[color_type]
    raw = zlib.decompress(bytes(idat))

    stride = width * channels
    previous = bytearray(stride)
    pixels = []
    for row in range(height):
        start = row * (stride + 1)
        filter_type = raw[start]
        line = bytearray(raw[start + 1 : start + 1 + stride])
        if filter_type == 1:
            for i in range(channels, stride):
                line[i] = (line[i] + line[i - channels]) & 0xFF
        elif filter_type == 2:
            for i in range(stride):
                line[i] = (line[i] + previous[i]) & 0xFF
        elif filter_type == 3:
            for i in range(stride):
                left = line[i - channels] if i >= channels else 0
                line[i] = (line[i] + ((left + previous[i]) >> 1)) & 0xFF
        elif filter_type == 4:
            for i in range(stride):
                left = line[i - channels] if i >= channels else 0
                up = previous[i]
                upleft = previous[i - channels] if i >= channels else 0
                estimate = left + up - upleft
                pa, pb, pc = abs(estimate - left), abs(estimate - up), abs(estimate - upleft)
                predictor = left if (pa <= pb and pa <= pc) else (up if pb <= pc else upleft)
                line[i] = (line[i] + predictor) & 0xFF
        pixels.append(bytes(line))
        previous = line

    return width, height, channels, pixels


def report(path, bands=0):
    width, height, channels, pixels = read_png(path)
    counter = Counter()
    for line in pixels:
        for i in range(0, len(line), channels):
            counter[line[i : i + 3]] += 1
    total = width * height
    common, common_count = counter.most_common(1)[0]
    luma = [0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2] for c in counter]
    print(
        f"{path.split('/')[-1]:28s} {width:4d}x{height:<4d} "
        f"colours={len(counter):5d} "
        f"nonbackground={100.0 * (total - common_count) / total:5.1f}% "
        f"luma={min(luma):5.1f}..{max(luma):5.1f}"
    )
    # Ink per horizontal band, to show where the content actually landed in a layout that
    # nobody can look at.
    if bands > 0:
        step = max(1, height // bands)
        for band in range(0, height, step):
            rows = pixels[band : band + step]
            if not rows:
                break
            seen = Counter()
            for row in rows:
                for i in range(0, len(row), channels):
                    seen[row[i : i + 3]] += 1
            pixels_in_band = len(rows) * width
            ink = pixels_in_band - seen.most_common(1)[0][1]
            filled = "#" * int(round(40 * ink / pixels_in_band))
            print(f"    rows {band:4d}-{min(band + step, height):<4d} {100.0 * ink / pixels_in_band:5.1f}% {filled}")
    return len(counter), (total - common_count) / total


if __name__ == "__main__":
    args = sys.argv[1:]
    bands = 0
    if "--bands" in args:
        index = args.index("--bands")
        bands = int(args[index + 1])
        del args[index : index + 2]
    if not args:
        print(__doc__)
        sys.exit(2)
    failures = 0
    for arg in args:
        colors, ink = report(arg, bands)
        if colors < 8 or ink < 0.02:
            print(f"  !! looks blank: {arg}")
            failures += 1
    sys.exit(1 if failures else 0)
