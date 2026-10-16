#!/usr/bin/env python3
"""Turn swept emulator dumps into level "pages" the engine can stream.

The game streams the level by rewriting nametable columns as the camera moves
(`docs/STATUS.md`), so the level's appearance is only ever visible as a 512 px
window at a known camera position. A sweep capture (`INPUT=sweep` in
`tools/emu/dump.lua`) dumps that window every few pixels, and this stitches them
into one page per dump:

    assets/levels/stage-1-1/pages/page-NNNN.bin

Each page is self-contained, because the CHR banks change as the level streams:
what a page needs to be rendered is exactly what the emulator had at that moment.

    u16 level_x   little-endian: the level x of the page's left edge
    u8  [8192]    live PPU pattern view ($0000-$1FFF)
    u8  [2048]    both nametables ($2000-$27FF)
    u8  [32]      palette RAM ($3F00-$3F1F)

`pages.txt` lists `level_x path` per page.

    tools/emu/pages.py tools/emu/pages assets/levels/stage-1-1/pages
"""

import argparse
import os
import struct
import sys

PATTERN_BYTES = 8192
NAMETABLE_BYTES = 2048
PALETTE_BYTES = 32
PAGE_BYTES = 2 + PATTERN_BYTES + NAMETABLE_BYTES + PALETTE_BYTES


def read_dump(dump_dir, frame):
    ppu_path = os.path.join(dump_dir, f"ppu-{frame}.bin")
    ram_path = os.path.join(dump_dir, f"ram-{frame}.bin")
    if not (os.path.exists(ppu_path) and os.path.exists(ram_path)):
        return None
    with open(ppu_path, "rb") as f:
        ppu = f.read()
    with open(ram_path, "rb") as f:
        ram = f.read()
    if len(ppu) < 0x4000 or len(ram) < 0x800:
        return None
    # The horizontal scroll lives in RAM $00AC; its high bit (which nametable)
    # is PPUCTRL's bit 0, read from the $00AB shadow.
    level_x = ram[0x00AC] | ((ram[0x00AB] & 0x01) << 8)
    page = struct.pack("<H", level_x)
    page += ppu[0:PATTERN_BYTES]
    page += ppu[0x2000:0x2000 + NAMETABLE_BYTES]
    page += ppu[0x3F00:0x3F00 + PALETTE_BYTES]
    assert len(page) == PAGE_BYTES, len(page)
    return level_x, page, ppu


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("dump_dir", help="directory written by tools/emu/dump.lua")
    ap.add_argument("out_dir", help="directory to write pages into")
    ap.add_argument("--min-gap", type=int, default=7,
                    help="skip pages closer than this many level pixels (default 7)")
    args = ap.parse_args()

    frames = sorted(
        int(name.split("-")[1][:-4])
        for name in os.listdir(args.dump_dir)
        if name.startswith("ppu-") and name.endswith(".bin")
    )
    if not frames:
        sys.exit(f"no ppu-<frame>.bin dumps in {args.dump_dir}")

    os.makedirs(args.out_dir, exist_ok=True)
    written = []
    last_x = None
    for frame in frames:
        got = read_dump(args.dump_dir, frame)
        if got is None:
            print(f"  frame {frame}: incomplete, skipped")
            continue
        level_x, page, _ = got
        if last_x is not None and abs(level_x - last_x) < args.min_gap:
            continue
        name = f"page-{frame:04d}.bin"
        with open(os.path.join(args.out_dir, name), "wb") as f:
            f.write(page)
        written.append((level_x, name))
        print(f"  frame {frame}: level x {level_x:4d} -> {name}")
        last_x = level_x

    with open(os.path.join(args.out_dir, "pages.txt"), "w") as f:
        f.write("# level_x page\n")
        for level_x, name in written:
            f.write(f"{level_x} {name}\n")

    spans = [x for x, _ in written]
    print(f"\nwrote {len(written)} pages covering level x {min(spans)}..{max(spans)}")
    gaps = [b - a for a, b in zip(spans, spans[1:])]
    if gaps:
        print(f"page spacing: min {min(gaps)} max {max(gaps)} (each page shows 512 px)")
        if max(gaps) > 512:
            print("WARNING: a gap exceeds the window width; some level is missing")


if __name__ == "__main__":
    main()
