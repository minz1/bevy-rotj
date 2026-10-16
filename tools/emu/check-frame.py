#!/usr/bin/env python3
"""Verify one captured frame against the emulator, with the right pairing.

Everything written to the PPU during vblank — CHR banks, OAM, palettes, the
nametables — belongs to the *next* frame, so a dump and its screen only line up
when each component comes from the frame it describes. Doing that by hand is
several commands and easy to get wrong (it cost me two false conclusions), so
this wraps it:

    tools/emu/check-frame.py tools/emu/lag 800
    tools/emu/check-frame.py tools/emu/lag 800 --no-pair   # all from one frame

Prints the index mismatch, the per-band table, and each band's best shift (a
band fitting at a non-zero shift is a real difference in that band, not noise).
"""

import argparse
import csv
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))


def run(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, check=False, **kw)


def have(path):
    return os.path.exists(path)


def render(dump, frame, pair, preview_dir):
    """Renders `frame`, pairing vblank-updated components from the next frame."""
    ppu = os.path.join(dump, f"ppu-{frame}.bin")
    ram = os.path.join(dump, f"ram-{frame}.bin")
    args = [
        "cargo", "run", "-q", "-p", "rotj-verify", "--", "screen", ppu, ram,
        "--out", os.path.join(preview_dir, f"frame-{frame}.png"),
        "--indices", os.path.join(preview_dir, f"frame-{frame}.indices"),
    ]
    if pair and have(os.path.join(dump, f"ppu-{frame + 1}.bin")):
        newer_ppu = os.path.join(dump, f"ppu-{frame + 1}.bin")
        # Measured best pairing (docs/PHYSICS.md): the CHR banks a dump shows
        # belong to the next frame (the NMI rewrites them), while OAM, palette
        # and nametables describe their own frame.
        args += ["--pattern-from", newer_ppu]
    result = run(args, cwd=ROOT)
    if result.returncode != 0:
        sys.exit(f"render failed:\n{result.stderr or result.stdout}")
    return (
        os.path.join(preview_dir, f"frame-{frame}.png"),
        os.path.join(preview_dir, f"frame-{frame}.indices"),
    )


def compare(ours_png, ours_indices, reference, preview_dir):
    """Runs the index comparison, returning its raw report."""
    result = run(
        [sys.executable, os.path.join(HERE, "compare.py"), ours_png, ours_indices, reference],
        cwd=ROOT,
    )
    # compare.py exits with the mismatch *count*, so a non-zero code is normal;
    # only an empty report means it failed.
    if not result.stdout:
        sys.exit(f"compare failed:\n{result.stderr}")
    return result.stdout


def best_shifts(ours_indices, reference):
    """Per-band best shift, so a band that needs one is visible."""
    bands = [(0, 32), (32, 96), (96, 160), (160, 224)]
    out = []
    for top, bottom in bands:
        result = run(
            [sys.executable, os.path.join(HERE, "fit_shift.py"), ours_indices, reference,
             "--band-top", str(top), "--band-bot", str(bottom)],
            cwd=ROOT,
        )
        first = (result.stdout or "").splitlines()[:1]
        out.append((f"y{top}-{bottom}", first[0].strip() if first else "(no fit)"))
    return out


def frame_report(dump, frame):
    """What the frame's trace says about whether it can be reproduced at all.

    A frame in which the game rewrites the nametables shows a mixture of old and
    new data that no dump contains, so comparing it is meaningless. `mapper.lua`
    writes the traces next to the dumps; when they are absent, say so.
    """
    path = os.path.join(dump, "ppu-writes.csv")
    if not have(path):
        return ["no trace next to these dumps (capture with tools/emu/mapper.lua "
                "to get one), so the frame's representability is unknown"]
    nametable = scroll = other = 0
    for row in csv.DictReader(open(path)):
        if int(row["frame"]) != frame:
            continue
        addr = row["addr"]
        if addr in ("2006", "2007"):
            nametable += 1
        elif addr == "2005":
            scroll += 1
        else:
            other += 1
    lines = [
        f"frame {frame} register writes: {nametable} nametable, {scroll} scroll, {other} other"
    ]
    if nametable:
        lines.append("UNREPRESENTABLE: the game rewrote the nametables during this frame, "
                     "so the screen shows a mixture of old and new data that no dump holds")
    else:
        lines.append("representable: no mid-frame nametable writes")
    if scroll:
        lines.append(f"note: {scroll} mid-frame scroll writes (a split may be in effect)")
    return lines


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("dump", help="capture directory (ppu-<frame>.bin, ram-<frame>.bin, shot-<frame>.png)")
    ap.add_argument("frame", type=int)
    ap.add_argument("--no-pair", action="store_true",
                    help="use only this frame's components (expect ~28%% on scrolled frames)")
    ap.add_argument("--preview", default=os.path.join(ROOT, "assets", "preview"))
    args = ap.parse_args()

    dump = os.path.abspath(args.dump)
    reference = os.path.join(dump, f"shot-{args.frame}.png")
    if not have(reference):
        sys.exit(f"no reference screenshot {reference}")
    os.makedirs(args.preview, exist_ok=True)

    for line in frame_report(dump, args.frame):
        print(line)
    ours_png, ours_indices = render(dump, args.frame, not args.no_pair, args.preview)
    report = compare(ours_png, ours_indices, reference, args.preview)
    print(report, end="")
    print("per-band best shift (non-zero means that band really differs):")
    for band, shift in best_shifts(ours_indices, reference):
        print(f"  {band}: {shift}")

    match = re.search(r"mismatch: (\d+) / (\d+) = ([\d.]+)%", report)
    if match and args.no_pair:
        print(f"\n(note: --no-pair compares dump {args.frame} with screen {args.frame}; "
              "vblank-updated components describe the next frame)")


if __name__ == "__main__":
    main()
