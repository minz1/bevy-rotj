#!/usr/bin/env python3
"""Correlate the port's rendered index plane against the emulator's.

Both planes are 256x240 bytes of NES colour indices. The emulator screenshot is
a palette PNG where NES colour i is palette entry i+128 (see compare.py).

Used to answer "is the scroll the only thing wrong?" without re-rendering: if
shifting our plane by (dx, dy) makes it match, then tiles and palette are right
and only the scroll offset differs.

    tools/emu/fit_shift.py ours.indices emulator.png [--band-top N] [--band-bot N]
"""

import argparse
import sys
from PIL import Image


def emulator_plane(png):
    im = Image.open(png)
    pal = im.getpalette()
    idx = im.tobytes()
    # palette entry i + 128 is NES colour i
    return bytes(max(0, b - 128) for b in idx), im.size


def ours_plane(path, w=256, h=240):
    data = open(path, "rb").read()
    assert len(data) == w * h, f"{len(data)} bytes, expected {w*h}"
    return data


# fceux screenshots crop 8 lines top and bottom (compare.py uses the same).
OVERSCAN = 8


def mismatch(a, b, dx, dy, top, bot, w=256, h=240):
    """Pixels of `a` shifted right/down by (dx, dy) that disagree with `b`.

    `a` is our 256x240 plane, `b` the emulator's 256x224 plane, so our row for
    emulator row `y` is `y + OVERSCAN + dy`.
    """
    bad = 0
    total = 0
    for y in range(top, bot):
        sy = y + OVERSCAN + dy
        if not 0 <= sy < h:
            continue
        row_a = a[sy * w:(sy + 1) * w]
        row_b = b[y * w:(y + 1) * w]
        for x in range(w):
            sx = x - dx
            if not 0 <= sx < w:
                continue
            total += 1
            if row_a[sx] != row_b[x]:
                bad += 1
    return bad, total


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("ours")
    ap.add_argument("emulator")
    ap.add_argument("--band-top", type=int, default=0)
    ap.add_argument("--band-bot", type=int, default=224)
    ap.add_argument("--max-dx", type=int, default=64)
    ap.add_argument("--max-dy", type=int, default=8)
    args = ap.parse_args()

    ours = ours_plane(args.ours)
    emu, size = emulator_plane(args.emulator)
    if size != (256, 224):
        print(f"unexpected screenshot size {size}", file=sys.stderr)
        return 1

    results = []
    for dx in range(-args.max_dx, args.max_dx + 1):
        for dy in range(-args.max_dy, args.max_dy + 1):
            bad, total = mismatch(ours, emu, dx, dy, args.band_top, args.band_bot)
            results.append((bad / max(total, 1), dx, dy, bad, total))
    results.sort()
    for pct, dx, dy, bad, total in results[:6]:
        print(f"shift ({dx:+3d},{dy:+3d}): {bad:5d}/{total} = {pct*100:5.2f}%")
    return 0


if __name__ == "__main__":
    sys.exit(main())
