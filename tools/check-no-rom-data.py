#!/usr/bin/env python3
"""Fails if a tracked file carries a 32-byte run of the ROM's PRG or CHR, raw or written
out as hex bytes or little-endian words (`0x12, 0x34`, `$12 $34`, `12 34`, `0x3412`,
nested `[..]` rows). Windows with fewer than 8 distinct byte
values (padding, fills) are not evidence and are skipped. Without the ROM it skips.

    tools/check-no-rom-data.py        # ROM from $ROTJ_ROM, else raw/
"""
import os
import re
import subprocess
import sys

WINDOW = 32
DISTINCT = 8
SEP = rb"[\s,;\[\](){}]+"
WORD = rb"(?:0x|\$)?[0-9A-Fa-f]{2}(?:[0-9A-Fa-f]{2})?(?:_?u(?:8|16))?"
HEX = re.compile(rb"(?:%s%s){%d,}" % (WORD, SEP, WINDOW // 2 - 1) + WORD)
TOKEN = re.compile(rb"(?:0x|\$)?([0-9A-Fa-f]{2})([0-9A-Fa-f]{2})?(?:_?u(?:8|16))?")


def windows(data):
    found = set()
    for i in range(len(data) - WINDOW + 1):
        chunk = data[i:i + WINDOW]
        if len(set(chunk)) >= DISTINCT:
            found.add(chunk)
    return found


def first_hit(data, rom):
    for i in range(len(data) - WINDOW + 1):
        if data[i:i + WINDOW] in rom:
            return i
    return None


def main():
    root = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True,
                          text=True, check=True).stdout.strip()
    path = os.environ.get("ROTJ_ROM") or os.path.join(
        root, "raw", "Batman - Return of the Joker (USA).nes")
    if not os.path.exists(path):
        print(f"skipped: no ROM at {path}")
        return 0
    image = open(path, "rb").read()
    rom = windows(image[16:])
    files = subprocess.run(["git", "ls-files", "-z"], cwd=root, capture_output=True,
                           check=True).stdout.split(b"\0")
    bad = 0
    for name in filter(None, files):
        full = os.path.join(root, name.decode())
        if not os.path.isfile(full):
            continue
        data = open(full, "rb").read()
        at = first_hit(data, rom)
        if at is not None:
            print(f"{name.decode()}: ROM bytes at offset {at}")
            bad += 1
            continue
        for run in HEX.finditer(data):
            tokens = list(TOKEN.finditer(run.group(0)))
            pairs = bytes(int(t.group(1), 16) for t in tokens if not t.group(2))
            words = b"".join(
                int(t.group(1) + (t.group(2) or b""), 16).to_bytes(2 if t.group(2) else 1, "little")
                for t in tokens)
            if first_hit(pairs, rom) is not None or first_hit(words, rom) is not None:
                line = data.count(b"\n", 0, run.start()) + 1
                print(f"{name.decode()}:{line}: ROM bytes written as hex")
                bad += 1
                break
    if bad:
        print(f"FAIL: {bad} tracked files carry ROM data")
        return 1
    print("OK: no tracked file carries ROM data")
    return 0


if __name__ == "__main__":
    sys.exit(main())
