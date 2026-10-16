#!/usr/bin/env python3
"""Turn captured execution evidence into disassembly seeds.

The disassembly's recursive descent starts from the reset/NMI/IRQ vectors and
resolves far calls through the *reset-time* FME-7 window map. Because the game
re-maps its windows per scene, that map is wrong for most of the code, so large
regions stay undecoded. The emulator traces know better: every mapper or PPU
register write records the program counter and frame, and the mapper-write
stream says which PRG bank was in which window at that moment.

fceux reports the program counter *inside* the write callback, so a logged PC is
the address of the instruction *after* the store (the writing store sits just
before it). That is still a valid instruction start, which is all a seed needs.

The FME-7 places cmd $8/$9/$A/$B in `$6000/$8000/$A000/$C000` respectively
(docs/ACTORS.md §0, nesdev), not in the $8000.. order an earlier version of
this script assumed; the corrected map is what makes each PC land in the right
bank. cmd $8 with bit 7 set makes `$6000` WRAM rather than a PRG bank.

This reads those traces and writes `disasm/gen/observed_code.py` (importable by the generator):

    SEEDS   = {bank: [(offset, "reason"), ...]}   code the game actually ran
    WINDOWS = {bank: window}                      observed runtime window map

Run from the repo root (traces are gitignored, this file is committed). The
capture root can be given explicitly, since the traces live outside the port's
own tree:

    python3 tools/emu/observed-code.py                    # tools/emu
    python3 tools/emu/observed-code.py /path/to/captures
"""

import bisect
import csv
import glob
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, "disasm", "gen", "observed_code.py")
DEFAULT_CAPTURES = os.path.join(ROOT, "tools", "emu")

BANK = 8192
PRG_BANKS = 16


def captures(root):
    """Each capture directory under `root` that has a mapper trace, as (name, dir)."""
    for path in sorted(glob.glob(os.path.join(root, "*", "mapper-writes.csv"))):
        yield os.path.basename(os.path.dirname(path)), os.path.dirname(path)


def rows(path):
    """CSV rows of one trace file, as dicts."""
    with open(path) as handle:
        return list(csv.DictReader(handle))


# FME-7 PRG command -> the CPU window base it selects. cmd $8 holds $6000,
# $9 holds $8000, $A holds $A000, $B holds $C000 (docs/ACTORS.md §0, nesdev).
WINDOW_BASE = {8: 0x6000, 9: 0x8000, 10: 0xA000, 11: 0xC000}


def slot_window(bank):
    """The `bank % 3` naming convention the generator uses for switchable banks."""
    if bank == 15:
        return 0xE000
    return (0x8000, 0xA000, 0xC000)[bank % 3]


def record(seeds, windows, kind, pc, state):
    """Seed one observed PC (already the *next* instruction) through `state`."""
    if not pc:
        return 0
    addr = int(pc, 16)
    for cmd, base in WINDOW_BASE.items():
        bank = state.get(cmd)
        if bank is None or not (base <= addr < base + BANK):
            continue
        reason = "observed {} write".format(kind)
        slot = seeds.setdefault(bank, {})
        slot.setdefault(addr - base, reason)
        windows.setdefault(bank, slot_window(bank))
        return 1
    return 0


def read_capture(directory, seeds, windows):
    """Seed one capture's observed PCs, tracking the FME-7 windows in write order.

    The registers change several times per frame and the game writes some
    windows directly, bypassing the RAM shadow (which then lags), so a PC must
    be read against the register state of its own write, not the frame's end.
    Mapper rows give that order; a PPU row carries no window write, so it uses
    the state left by the frame's mapper writes.
    """
    state = {cmd: None for cmd in WINDOW_BASE}
    frames, states = [], []
    hits = 0
    for row in rows(os.path.join(directory, "mapper-writes.csv")):
        if int(row["port"], 16) == 0xA000:
            cmd = int(row["cmd"])
            if cmd in WINDOW_BASE:  # cmd $8 bit 7 selects WRAM, recorded as None
                value = int(row["value"], 16)
                state[cmd] = None if cmd == 8 and value & 0x80 else value & 0x0F
        hits += record(seeds, windows, "mapper", row.get("pc"), state)
        frame = int(row["frame"])
        if frames and frames[-1] == frame:
            states[-1] = dict(state)
        else:
            frames.append(frame)
            states.append(dict(state))
    ppu = os.path.join(directory, "ppu-writes.csv")
    if os.path.exists(ppu):
        for row in rows(ppu):
            i = bisect.bisect_right(frames, int(row["frame"])) - 1
            hits += record(seeds, windows, "ppu", row.get("pc"),
                           states[i] if i >= 0 else {})
    return hits


def main():
    root = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_CAPTURES
    seeds, windows = {}, {}
    total_rows = 0
    for _capture, directory in captures(root):
        total_rows += read_capture(directory, seeds, windows)

    lines = [
        '# Generated by tools/emu/observed-code.py - do not edit by hand.',
        '#',
        '# Code the emulator was observed executing and the FME-7 PRG window map',
        '# (bank -> its `bank % 3` naming window), both from the captured traces.',
        '#',
        '# Each SEED is the program counter of a mapper/PPU register write. fceux',
        '# reports PC inside the write callback, so it is the address of the *next*',
        '# instruction: a valid instruction start whose store is just before it.',
        '# Attributing a PC needs the FME-7 window map in effect at that write',
        '# (cmd $8=$6000, $9=$8000, $A=$A000, $B=$C000; cmd $8 bit 7 is WRAM).',
        '#',
        '# `WINDOWS[bank]` is the CPU window the generator should use for that bank',
        '# when resolving its far calls: the reset-time defaults are wrong for most',
        '# of the code, because the game re-maps per scene.',
        '',
        'SEEDS = {',
    ]
    for bank in sorted(seeds):
        entries = sorted(seeds[bank].items())
        lines.append('    %d: [' % bank)
        for offset, reason in entries:
            lines.append('        (0x%04X, %r),' % (offset, reason))
        lines.append('    ],')
    lines.append('}')
    lines.append('')
    lines.append('WINDOWS = {')
    for bank in sorted(windows):
        lines.append('    0x%02X: 0x%04X,' % (bank, windows[bank]))
    lines.append('}')
    lines.append('')

    with open(OUT, "w") as handle:
        handle.write("\n".join(lines))
    seeded = sum(len(v) for v in seeds.values())
    print("wrote %s: %d seeds across %d banks" % (os.path.relpath(OUT, ROOT), seeded, len(seeds)))
    print("witnessed %d window/PC matches over %d mapper rows" % (
        total_rows, sum(len(rows(os.path.join(d, "mapper-writes.csv"))) for _c, d in captures(root))))


if __name__ == "__main__":
    main()
