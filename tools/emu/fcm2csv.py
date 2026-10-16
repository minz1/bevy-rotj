#!/usr/bin/env python3
"""Convert an FCEU 0.98 .fcm movie (power-on start) to a `frame,pad` inputs.csv (row 0 = no input)."""
import struct
import sys

data = open(sys.argv[1], "rb").read()
assert data[:4] == b"FCM\x1a", "not an FCM movie"
assert data[8] & 2, "movie does not start from power-on"
frames, _, length, _, offset = struct.unpack("<5I", data[12:32])
stream = data[offset:offset + length]
pad, frame, pads, i = 0, 0, [], 0
while i < len(stream):
    control = stream[i]
    count = (control >> 5) & 3
    delta = int.from_bytes(stream[i + 1:i + 1 + count], "little")
    i += 1 + count
    pads += [pad] * delta
    frame += delta
    if not control & 0x80 and (control >> 3) & 3 == 0:
        pad ^= 1 << (control & 7)
pads += [pad] * (frames - frame)
with open(sys.argv[2], "w") as out:
    out.write("frame,pad\n0,0\n")
    out.writelines(f"{n},{p}\n" for n, p in enumerate(pads[:frames], start=1))
