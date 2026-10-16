#!/usr/bin/env python3
"""Diff the port's rendered frame against the emulator's own screen output.

Why not compare RGB: the emulator and the port use different RGB tables for the
same NES palette, so pixels that are the *same* NES colour differ slightly in
RGB. Colour indices are the comparable thing.

Usage:
    tools/emu/compare.py <ours.png> <ours.indices> <emulator-screen.png> [--out compare.png]

`ours.indices` is the raw 256x240 index plane written by
`rotj-verify screen --indices`. The emulator screenshot is an indexed PNG: FCEUX
writes NES colour index `i` as palette entry `i + 128` (checked against the
known entries for $00/$0F/$11 in fceux 2.6.5), so this maps back by
subtracting that offset, auto-detecting it from the entries actually present.

Exit status is 0 when the frames match within `--tolerance` percent, else 1.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from PIL import Image

SCREEN_W, SCREEN_H = 256, 240
OVERSCAN = 8  # fceux screenshots crop 8 lines top and bottom


def emulator_indices(path: Path) -> tuple[list[int], int, int]:
    """Return the emulator's per-pixel NES colour index plane."""
    img = Image.open(path)
    if img.mode != "P":
        sys.exit(f"{path} is mode {img.mode}; expected an indexed (P) PNG")
    palette = img.getpalette()
    if palette is None:
        sys.exit(f"{path} has no palette")
    # Find the offset by matching known NES colours: $00 mid gray, $0F black,
    # $11 blue. FCEUX's own RGB values are approximate, so match loosely.
    known = {
        0x00: "mid gray",
        0x0F: "black",
        0x11: "blue",
    }
    offset = None
    for base in range(0, 256 - 64):
        ok = 0
        for idx in known:
            r, g, b = palette[(base + idx) * 3 : (base + idx) * 3 + 3]
            if idx == 0x0F and (r, g, b) == (0, 0, 0):
                ok += 1
            elif idx == 0x00 and abs(r - g) < 8 and abs(g - b) < 8 and r > 64:
                ok += 1
            elif idx == 0x11 and b > 150 and b > r + 100:
                ok += 1
        if ok == len(known):
            offset = base
            break
    if offset is None:
        sys.exit("could not locate the NES palette offset in the emulator PNG")
    px = img.load()
    w, h = img.size
    return [px[x, y] - offset for y in range(h) for x in range(w)], w, h


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("ours_png", type=Path)
    ap.add_argument("ours_indices", type=Path)
    ap.add_argument("emulator_png", type=Path)
    ap.add_argument("--out", type=Path, default=Path("assets/preview/compare.png"))
    ap.add_argument("--tolerance", type=float, default=0.5,
                    help="acceptable mismatch percentage")
    args = ap.parse_args()

    ours = args.ours_indices.read_bytes()
    if len(ours) != SCREEN_W * SCREEN_H:
        sys.exit(f"{args.ours_indices} is {len(ours)} bytes; expected {SCREEN_W*SCREEN_H}")

    emu, w, h = emulator_indices(args.emulator_png)
    if (w, h) != (SCREEN_W, SCREEN_H - 2 * OVERSCAN):
        sys.exit(f"emulator image is {w}x{h}; expected {SCREEN_W}x{SCREEN_H - 2*OVERSCAN}")

    ours_img = Image.open(args.ours_png).convert("RGB")
    emu_img = Image.open(args.emulator_png).convert("RGB")

    mismatch = 0
    bands: list[list[int]] = [[0, 0] for _ in range((h + 15) // 16)]
    panel = Image.new("RGB", (w * 3, h))
    for y in range(h):
        for x in range(w):
            i = y * w + x
            o = ours[(y + OVERSCAN) * SCREEN_W + x]
            if o != emu[i]:
                mismatch += 1
                bands[y // 16][0] += 1
            bands[y // 16][1] += 1
            panel.putpixel((x, y), ours_img.getpixel((x, y + OVERSCAN)))
            panel.putpixel((w + x, y), emu_img.getpixel((x, y)))
            panel.putpixel((2 * w + x, y), (255, 128, 0) if o != emu[i] else (0, 0, 0))

    total = w * h
    pct = 100.0 * mismatch / total
    print(f"colour-index mismatch: {mismatch} / {total} = {pct:.2f}%")
    print("band  mismatch%  (non-black in emulator)")
    for band, (bad, all_px) in enumerate(bands):
        y0 = band * 16
        nonblack = sum(
            1 for y in range(y0, min(y0 + 16, h)) for x in range(w)
            if emu[y * w + x] != 0x0F
        )
        print(f"  y{y0:3d}: {100.0*bad/all_px:6.1f}   {nonblack}")

    args.out.parent.mkdir(parents=True, exist_ok=True)
    panel.save(args.out)
    print(f"wrote {args.out} (port | emulator | disagreement)")
    return 0 if pct <= args.tolerance else 1


if __name__ == "__main__":
    sys.exit(main())
