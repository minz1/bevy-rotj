#!/usr/bin/env python3
"""Check the observed program counters against the disassembly and the ROM.

An observed PC is the address of an instruction (fceux reports it just after the
register store), so two independent checks must hold:

1. wherever the generator has decoded the surrounding bytes, the PC must be an
   instruction *start*. A PC strictly inside a decoded instruction means the
   bank attribution -- or the decode there -- is wrong. PCs in still-undecoded
   `.byte` regions prove nothing either way and are counted separately (they are
   exactly the code the seed is meant to add).
2. the ROM bytes at `pc-3` must be an absolute store to the write's own register
   window (`8D/8E/8C 00 80`, `... 00 A0`, or `... 0x 20`). This one needs no
   decode, so a clean result is independent evidence the PC was put in the right
   bank.

    python3 disasm/gen/check_observed.py
"""
import bisect
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generate

try:  # generated; absent on a clean checkout
    from observed_code import SEEDS
except ImportError:  # pragma: no cover
    SEEDS = {}

STORE_OPCODES = (0x8D, 0x8E, 0x8C)  # STA/STX/STY absolute


def linear_starts(data):
    """The historical fallback listing's instruction starts."""
    starts, pc = {}, 0
    while pc < len(data):
        ent = generate.decode(data, pc)
        if ent is None:
            pc += 1
            continue
        starts[pc] = ent
        pc += 1 if ent[1] in ('imp', 'acc') else ent[2]
    return starts


def decoded_intervals(prg, seeds, bank):
    """The `(start, length)` of every decoded instruction in `bank`, sorted."""
    data = prg[bank * generate.BANK_SIZE:(bank + 1) * generate.BANK_SIZE]
    if seeds[bank]:
        descent = generate.Descent(data, generate.style_windows()[bank])
        for off, name in seeds[bank]:
            descent.seed(off, name)
        starts = descent.starts
    else:
        starts = linear_starts(data)
    return sorted((off, generate.MODE_SIZE[e[1]]) for off, e in starts.items())


def check_starts(prg, seeds):
    """Return (checked, hits, outside) for the decoded-region check."""
    intervals = {b: decoded_intervals(prg, seeds, b) for b in range(generate.PRG_BANKS)}
    starts = {b: [s for s, _ in v] for b, v in intervals.items()}
    total = checked = hits = outside = 0
    for bank in sorted(SEEDS):
        for off, reason in SEEDS[bank]:
            total += 1
            i = bisect.bisect_right(starts[bank], off) - 1
            if i < 0:
                outside += 1
                continue
            start, length = intervals[bank][i]
            if not start <= off < start + length:
                outside += 1
                continue
            checked += 1
            if off != start:
                hits += 1
                print('MID-INSTRUCTION bank %02X offset $%04X (%s): inside the '
                      '%d-byte instruction at $%04X' % (bank, off, reason, length, start))
    print('%d observed PCs: %d inside the decoded code (all instruction starts), '
          '%d mid-instruction hits, %d outside it' % (total, checked, hits, outside))
    return hits


def check_store_shape(prg):
    """Every PC's preceding instruction must be a store to a mapper/PPU register."""
    hits = 0
    for bank in sorted(SEEDS):
        data = prg[bank * generate.BANK_SIZE:(bank + 1) * generate.BANK_SIZE]
        for off, reason in SEEDS[bank]:
            op, lo, hi = data[off - 3], data[off - 2], data[off - 1]
            if op not in STORE_OPCODES or not (
                    (lo, hi) in ((0x00, 0x80), (0x00, 0xA0))
                    or (hi == 0x20 and lo < 0x08)):
                hits += 1
                print('BAD STORE bank %02X offset $%04X (%s): $%02X $%02X $%02X'
                      % (bank, off, reason, op, lo, hi))
    print('%d observed PCs: %d not preceded by a store to its register window'
          % (sum(len(v) for v in SEEDS.values()), hits))
    return hits


def main():
    rom = open(generate.ROM, 'rb').read()
    prg = rom[generate.HEADER_SIZE:generate.HEADER_SIZE + generate.PRG_SIZE]
    # The unobserved baseline: a seed that is itself a decoded start would prove
    # nothing, so check the observed PCs against the decode without them.
    seeds = generate.collect_entries(prg, observed={})
    return 1 if check_starts(prg, seeds) or check_store_shape(prg) else 0


if __name__ == '__main__':
    sys.exit(main())
