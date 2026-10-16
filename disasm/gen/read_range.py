#!/usr/bin/env python3
"""Disassemble a byte range of one PRG bank with the generator's own decoder.

The listing's addresses are *nominal* window conventions (`disasm/gen/
generate.py`), and bank regions the recursive descent never reached stay `.byte`
— including, for example, the code that decides a blocked move. The runtime
window map from the captures says which bank really runs where, so this reads a
bank's bytes directly:

    python3 disasm/gen/read_range.py 13 0x0A29 60     # bank 13, offset, bytes
    python3 disasm/gen/read_range.py 13 0x0A29 60 --window 0xC000

CAVEAT: instruction alignment is a *guess* unless the offset is a known entry
point, so a byte that is really the middle of an instruction decodes as a
plausible-looking instruction and everything after it is wrong. Reads from this
tool are leads to verify, not facts. The descent in `generate.py` now resolves
far calls through the observed runtime window map, so a bank's reachable code is
listed there; this tool still reads bytes the descent never reached.
"""

import argparse
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generate  # noqa: E402  (the decoder and the ROM loading live here)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("bank", type=lambda t: int(t, 0))
    ap.add_argument("offset", type=lambda t: int(t, 0))
    ap.add_argument("count", nargs="?", type=lambda t: int(t, 0), default=48,
                    help="bytes to decode (default 48)")
    ap.add_argument("--window", type=lambda t: int(t, 0), default=None,
                    help="CPU base for the address shown (default: naming convention)")
    args = ap.parse_args()

    prg = generate.load_prg() if hasattr(generate, "load_prg") else None
    if prg is None:
        rom = open(generate.ROM, "rb").read()
        prg = rom[generate.HEADER_SIZE:generate.HEADER_SIZE + generate.PRG_SIZE]
    data = prg[args.bank * generate.BANK_SIZE:(args.bank + 1) * generate.BANK_SIZE]
    base = args.window if args.window is not None else generate.REAL_WINDOWS.get(args.bank, 0x8000)

    end = min(args.offset + args.count, len(data))
    pc = args.offset
    while pc < end:
        entry = generate.decode(data, pc)
        if entry is None:
            print(f"${base + pc:04X}  .byte ${data[pc]:02X}")
            pc += 1
            continue
        mnemonic, mode, size = entry
        window = (base, base + generate.BANK_SIZE - 1)
        operand = generate.fmt_operand(data, pc, base, window, {}, mode)
        print(f"${base + pc:04X}  {mnemonic} {operand}".rstrip())
        pc += size


if __name__ == "__main__":
    main()
