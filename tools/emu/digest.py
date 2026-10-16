#!/usr/bin/env python3
"""Prints `name unit digest` lines (FNV-1a 64) for a fceux capture, the format of the
`digests.txt` tables that pin the oracle to fceux without committing the capture
(rotj_core::digest).

    digest.py <capture> <name> ramseq           # one line per 2048-byte frame (unit = index)
    digest.py <capture> <name> apu              # one line per frame of an apu-writes.csv,
                                                #   without $4014/$4016 (addr LE, value)
    digest.py <capture> <name> file [start end] # one line for the file or a byte range (unit -)
"""
import sys


def fnv1a(data):
    h = 0xCBF29CE484222325
    for b in data:
        h = ((h ^ b) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def main():
    path, name, kind = sys.argv[1:4]
    if kind == "ramseq":
        data = open(path, "rb").read()
        for i in range(len(data) // 2048):
            print(f"{name} {i} {fnv1a(data[i * 2048:(i + 1) * 2048]):016x}")
    elif kind == "apu":
        frames = {}
        for line in open(path).read().splitlines()[1:]:
            frame, addr, value = line.split(",")[:3]
            addr, value = int(addr, 16), int(value, 16)
            if addr in (0x4014, 0x4016):
                continue
            frames.setdefault(int(frame), bytearray()).extend([addr & 0xFF, addr >> 8, value])
        for frame in sorted(frames):
            print(f"{name} {frame} {fnv1a(frames[frame]):016x}")
    elif kind == "file":
        data = open(path, "rb").read()
        if len(sys.argv) > 4:
            data = data[int(sys.argv[4], 0):int(sys.argv[5], 0)]
        print(f"{name} - {fnv1a(data):016x}")
    else:
        sys.exit(f"unknown kind {kind}")


if __name__ == "__main__":
    assert fnv1a(b"a") == 0xAF63DC4C8601EC8C and fnv1a(b"foobar") == 0x85944171F73967E8
    main()
