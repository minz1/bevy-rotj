#!/usr/bin/env python3
"""Packs fceux's indexed screenshots `shot-<n>.png` (n = first..last) into one file of
256x224 NES colour indices per frame (fceux writes colour i at palette entry i + 128), the
capture `tools/rotj-oracle/tests/picture.rs` reads for its row-level report.

    pack-shots.py <dir> <first> <last> <out>
"""
import sys

from PIL import Image

INDEX = bytes((p - 128) if p >= 128 else 255 for p in range(256))


def main():
    folder, first, last, out = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
    with open(out, "wb") as f:
        for frame in range(first, last + 1):
            f.write(Image.open(f"{folder}/shot-{frame}.png").tobytes().translate(INDEX))


if __name__ == "__main__":
    main()
