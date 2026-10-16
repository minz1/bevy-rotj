#!/usr/bin/env python3
"""Extract Batman's sprite tiles, palettes and animation frames.

Dev-only tooling (never ships with the port). The player object's sprite frames
are *descriptor scripts* in PRG bank 1: a list of 8x16 sprite records the game
copies straight into OAM page $02. This script reads those descriptors out of
the ROM, pairs them with the live CHR window and sprite palette captured from
the running game, and writes:

    assets/sprites/batman/tiles.png       the 8x8 tiles of his $1400 CHR window
    assets/sprites/batman/frames.png      the composed 8x16 frames he is drawn from
    assets/sprites/batman/animations.ron  animation state -> frame -> sprite list

It also *verifies* the decode: it rebuilds the frame for each captured moment
and matches every sprite against the OAM the emulator captured (tile byte,
palette/flip, and screen position). A frame that matches proves the descriptor,
the CHR window and the sprite palette all agree with the hardware.

Run from the repo root:

    python3 tools/emu/sprites.py

Requires: `tools/emu/stage/{ram,ppu}-{700,900}.bin` (captured with
`tools/emu/dump.lua`), `assets/graphics/chr/pattern-space.bin`, and the source
ROM at `raw/Batman - Return of the Joker (USA).nes`.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
STAGE = ROOT / "tools/emu/stage"
OUT = ROOT / "assets/sprites/batman"
ROM_PATH = ROOT / "raw/Batman - Return of the Joker (USA).nes"
PATTERN_PATH = ROOT / "assets/graphics/chr/pattern-space.bin"
PALETTE_PATH = ROOT / "assets/graphics/palette-sources/Composite_Direct_FBX.pal"

# Captured moments: (frame, animation state $0466). The state is the value
# tools/emu/stage/leads.csv shows for that frame.
CAPTURES = [(700, 14), (900, 0)]

# PPU windows (1 KiB each) and the sprite tile window. 8x16 sprite tile byte bit
# 0 selects pattern table $0000/$1000; Batman's tile numbers are $40-$7F, which
# land in the *third* 1 KiB window, $1400.
TILE_WINDOW = 0x1400
WINDOW_BYTES = 0x400
TILES_IN_WINDOW = 64
TILE_BYTES = 16

# OAM tile byte -> 8x16 tile pair; sprite palette RAM is $3F10-$3F1F.
SPRITE_PALETTE_OFFSET = 0x10

# --- PRG bank 1: the player's sprite frame descriptors --------------------
#
# Two 128-entry little-endian pointer tables in bank 4 select the frame
# descriptor; the descriptors themselves live in bank 1 mapped at $A000.
#
#   main    : $9924[$0466*2]                 (states $00-$7F)
#             $9A24[($0466*2)&$FF]           (states $80-$FF)
#   overlay : $9924[$97A4[$0466]*2]          (drawn every frame, no-op if 0)
#   extra   : $9924[$9705[$0466]*2]          (only when $9705[$0466] != 0)
#
# Descriptor layout (read backwards by the OAM builder at bank 0E $C247):
#   byte[0] = length L (entry bytes = L-1, must be 4*sprites)
#   bytes[1..L-1] = 4-byte records, first record at the highest index
#   byte[L] = trailer (an OAM attribute/flag byte, stored to $E0+$98)
# Each record is [dy, attr, dx, tile]; the sprite lands at ($94+dx, $95+dy)
# and OAM gets (y, tile, attr, x). attr bit 6/7 flip X/Y; attr bits 0-1 pick
# the sprite palette; bit 5 is background priority.
BANK4_MAIN_PTR = 0x9924
BANK4_MAIN_PTR_HI = 0x9A24
BANK4_OVERLAY = 0x97A4
BANK4_EXTRA = 0x9705

# The walk cycle: bank 4 $96FD, one byte per state, indexed by $047E (which
# increments each time the 8-frame timer $047D wraps). Proven against the
# captured leads.csv.
WALK_TABLE = 0x96FD
WALK_LEN = 8
WALK_PERIOD_FRAMES = 8  # $047D counts 0..7 per state

# States the player code writes to $0466, with a label only where the code or a
# capture proves the meaning (frame numbers refer to tools/emu/stage/leads.csv).
STATE_NAMES = {
    0: "idle (frame 900)",
    1: "jump take-off (frames 801-802)",
    7: "landing / transition (frames 796, 825)",
    12: "walk", 13: "walk", 14: "walk (frame 700)", 15: "walk",
    16: "walk", 17: "walk", 18: "walk", 19: "walk",
    28: "airborne (frames 803-824)",
    62: "walk start, first 6 frames of a walk (frames 688-693)",
}


def read(path: Path) -> bytes:
    return path.read_bytes()


def rom_bank(bank: int, window: int, addr: int) -> int:
    """One ROM byte at CPU `addr` with PRG `bank` mapped at `window`."""
    return ROM[16 + bank * 8192 + (addr - window)]


def rom_ptr(base: int, index: int) -> int:
    """Little-endian 16-bit pointer from a bank-4 table."""
    lo = rom_bank(4, 0x8000, base + index * 2)
    hi = rom_bank(4, 0x8000, base + index * 2 + 1)
    return lo | (hi << 8)


def descriptor(addr: int) -> tuple[int, int, list[tuple[int, int, int, int]]]:
    """Decode a bank-1 sprite descriptor at CPU `addr` ($A000 window).

    Returns (length, trailer, records) with records in draw order, each
    (dy, attr, dx, tile) as unsigned bytes (dy/dx are signed offsets).
    """
    m = [rom_bank(1, 0xA000, addr + i) for i in range(0x100)]
    length = m[0]
    count = (length - 1) // 4
    trailer = m[length]
    records = []
    y = length - 1
    for _ in range(count):
        records.append((m[y], m[y - 1], m[y - 2], m[y - 3]))
        y -= 4
    return length, trailer, records


def main_pointer(state: int) -> int:
    if state < 0x80:
        return rom_ptr(BANK4_MAIN_PTR, state)
    return rom_ptr(BANK4_MAIN_PTR_HI, (state * 2) & 0xFF)


def frame(state: int) -> list[tuple[int, int, int, int]]:
    """The full sprite frame (main + extra + overlay) for animation `state`.

    Records are (dy, attr, dx, tile), facing right; the engine mirrors them by
    OR-ing $046F bit 6 into every attr byte when Batman faces left.
    """
    out: list[tuple[int, int, int, int]] = []
    ptr = main_pointer(state)
    if ptr:
        out += descriptor(ptr)[2]
    extra = rom_bank(4, 0x8000, BANK4_EXTRA + state)
    if extra:
        out += descriptor(rom_ptr(BANK4_MAIN_PTR, extra))[2]
    overlay = rom_bank(4, 0x8000, BANK4_OVERLAY + state)
    if overlay:
        out += descriptor(rom_ptr(BANK4_MAIN_PTR, overlay))[2]
    return out


def walk_cycle() -> list[int]:
    return [rom_bank(4, 0x8000, WALK_TABLE + i) for i in range(WALK_LEN)]


# The bank-4 animation tables the player code indexes by a counter, with the
# number of entries the code allows (`CPX #n` before the indexed read).
TABLE_LENGTHS = {
    0x96DB: 2, 0x96DD: 3, 0x96E0: 0x11, 0x96F1: 2,
    0x96F3: 8, 0x96FD: 8, 0x979D: 2, 0x979F: 2,
}


def states_written_by_code() -> list[int]:
    """Every immediate/table value the code stores into $0466.

    State 0 is written from a computed value (bank04 `L862B`: when
    `$046C|$046D == 0`, i.e. standing still), so it is added explicitly.
    """
    found: set[int] = {0}
    for bank in ("bank04", "bank0E"):
        lines = (ROOT / "disasm" / f"{bank}.s").read_text().splitlines()
        for i, line in enumerate(lines):
            if not re.search(r"ST[AX] a:\$0466", line):
                continue
            for j in range(i - 1, max(-1, i - 7), -1):
                m = re.search(r"(?:LDA|LDX|LDY) #\$([0-9A-Fa-f]{2})", lines[j])
                if m:
                    found.add(int(m.group(1), 16))
                    break
                m = re.search(r"(?:LDA|LDX|LDY) a:\$([0-9A-Fa-f]{4})", lines[j])
                if m:
                    base = int(m.group(1), 16)
                    found.update(rom_bank(4, 0x8000, base + k)
                                 for k in range(TABLE_LENGTHS.get(base, 0)))
                    break
    return sorted(found)


def nes_rgb() -> list[tuple[int, int, int]]:
    pal = read(PALETTE_PATH)
    return [tuple(pal[i * 3:i * 3 + 3]) for i in range(64)]


def emu_indices(path: Path):
    """The emulator screenshot's per-pixel NES colour index plane.

    FCEUX writes its screenshot as a palette PNG whose entry `i + 128` is NES
    colour `i`; the offset is auto-detected from known colours (compare.py).
    """
    img = Image.open(path)
    pal = img.getpalette()
    known = {0x00: "mid gray", 0x0F: "black", 0x11: "blue"}
    offset = None
    for base in range(0, 256 - 64):
        ok = 0
        for idx in known:
            r, g, b = pal[(base + idx) * 3:(base + idx) * 3 + 3]
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
        sys.exit(f"{path}: could not locate the NES palette offset")
    px = img.load()
    w, h = img.size
    return [px[x, y] - offset for y in range(h) for x in range(w)], w, h


def tile_index(tile: bytes, x: int, y: int) -> int:
    lo = (tile[y] >> (7 - x)) & 1
    hi = (tile[y + 8] >> (7 - x)) & 1
    return (hi << 1) | lo


def window_tiles(pattern: bytes, slot: int) -> bytes:
    return pattern[slot * WINDOW_BYTES:(slot + 1) * WINDOW_BYTES]


def render_tiles(pattern: bytes, slot: int, rgb, palette: bytes) -> Image.Image:
    """8x8 tile sheet for a 1 KiB CHR window, coloured with one sprite palette."""
    win = window_tiles(pattern, slot)
    scale = 3
    cols = 16
    img = Image.new("RGB", (cols * 8 * scale, (TILES_IN_WINDOW // cols) * 8 * scale))
    px = img.load()
    for t in range(TILES_IN_WINDOW):
        tile = win[t * TILE_BYTES:t * TILE_BYTES + TILE_BYTES]
        for y in range(8):
            for x in range(8):
                i = tile_index(tile, x, y)
                r, g, b = rgb[palette[i]]
                for sy in range(scale):
                    for sx in range(scale):
                        px[(t % cols) * 8 * scale + x * scale + sx,
                           (t // cols) * 8 * scale + y * scale + sy] = (r, g, b)
    return img


def s8(b: int) -> int:
    return b - 256 if b >= 128 else b


def compose(records, pattern: bytes, slot: int, palette: bytes):
    """Blit a sprite frame into a palette-RAM index plane.

    Returns (plane, width, height, x0, y0); plane[(y-y0)*width + (x-x0)] holds
    the palette-RAM value, or None where nothing is drawn. Coordinates are the
    frame's own (dx, dy) space; add the object base to place it on screen.
    """
    xs = [s8(dx) for _, _, dx, _ in records] + [s8(dx) + 8 for _, _, dx, _ in records]
    ys = [s8(dy) for dy, _, _, _ in records] + [s8(dy) + 16 for dy, _, _, _ in records]
    x0, y0 = min(xs), min(ys)
    w, h = max(xs) - x0, max(ys) - y0
    plane = [None] * (w * h)
    for dy, attr, dx, tile in records:
        flip_x = attr & 0x40
        flip_y = attr & 0x80
        base = 0x1000 if tile & 1 else 0x0000
        for row in range(16):
            r = 15 - row if flip_y else row
            t = (tile & 0xFE) + r // 8
            off = base + t * TILE_BYTES
            # The frame's tiles all come from the $1400 window; translate the
            # PPU address into an offset inside that 1 KiB window.
            if (off - 0x1000) // 0x400 != (TILE_WINDOW - 0x1000) // 0x400:
                continue
            pat = pattern[slot * WINDOW_BYTES + (off - TILE_WINDOW):][:TILE_BYTES]
            for col in range(8):
                sx = 7 - col if flip_x else col
                i = tile_index(pat, sx, r % 8)
                if i == 0:
                    continue
                plane[(s8(dy) + row - y0) * w + s8(dx) + col - x0] = \
                    palette[SPRITE_PALETTE_OFFSET + (attr & 3) * 4 + i]
    return plane, w, h, x0, y0


def plane_to_image(plane, w: int, h: int, rgb, palette: bytes) -> Image.Image:
    scale = 3
    img = Image.new("RGB", (w * scale, h * scale), rgb[palette[0]])
    px = img.load()
    for y in range(h):
        for x in range(w):
            v = plane[y * w + x]
            if v is None:
                continue
            r, g, b = rgb[v]
            for sy in range(scale):
                for sx in range(scale):
                    px[x * scale + sx, y * scale + sy] = (r, g, b)
    return img


def render_frame(records, rgb, palette: bytes, pattern: bytes, slot: int) -> Image.Image:
    """Compose one sprite frame (records are (dy, attr, dx, tile))."""
    plane, w, h, _, _ = compose(records, pattern, slot, palette)
    return plane_to_image(plane, w, h, rgb, palette)


def oam_sprites(ram: bytes):
    """Visible OAM entries (y < 240) as (slot, x, y, tile, attr)."""
    oam = ram[0x200:0x300]
    out = []
    for i in range(64):
        y, tile, attr, x = oam[i * 4:i * 4 + 4]
        if y >= 240:
            continue
        out.append((i, x, y + 1, tile, attr))
    return out


def main() -> int:
    global ROM, PATTERN
    ROM = read(ROM_PATH)
    PATTERN = read(PATTERN_PATH)
    rgb = nes_rgb()

    OUT.mkdir(parents=True, exist_ok=True)

    # Captured frame -> live $1400 CHR slot and sprite palette RAM.
    caps = []
    for frame_no, state in CAPTURES:
        ram = read(STAGE / f"ram-{frame_no}.bin")
        ppu = read(STAGE / f"ppu-{frame_no}.bin")
        slot = ram[0x8D]  # CHR shadow cmd 5 = the $1400 window
        caps.append((frame_no, state, ram, ppu, slot))
        print(f"frame {frame_no}: state $0466={state:02X}  $1400 window = slot {slot} "
              f"(CHR bank 0x{slot // 8:02X}+{slot % 8} KiB)")

    # --- verify: every decoded sprite must be present in the captured OAM ---
    # OAM slot order is also the PPU's draw order, so the composed preview is
    # built in that order to match the emulator's layering.
    ok = True
    previews = []
    for frame_no, state, ram, ppu, slot in caps:
        records = frame(state)
        oam = oam_sprites(ram)
        ref_dy, _, ref_dx, ref_tile = records[0]
        cand = [o for o in oam if o[3] == ref_tile]
        if not cand:
            print(f"  frame {frame_no}: FAIL no OAM sprite with tile ${ref_tile:02X}")
            ok = False
            continue
        base_x, base_y = cand[0][1] - s8(ref_dx), cand[0][2] - s8(ref_dy)
        matched = 0
        ordered = []
        for dy, attr, dx, tile in records:
            want = (base_x + s8(dx), base_y + s8(dy), tile, attr & 0x03)
            hit = [o for o in oam if o[1] == want[0] and o[2] == want[1]
                   and o[3] == want[2] and (o[4] & 0x03) == want[3]]
            if hit:
                matched += 1
                ordered.append((hit[0][0], (dy, attr, dx, tile)))
        ordered.sort(key=lambda p: p[0])
        previews.append((slot, ppu, [r for _, r in ordered]))
        print(f"  frame {frame_no}: {matched}/{len(records)} decoded sprites "
              f"matched captured OAM at base ({base_x},{base_y})")
        ok = ok and matched == len(records)

        # Strongest proof: compose the frame and compare every drawn pixel to
        # the emulator screenshot by NES colour index.
        palette = ppu[0x3F00:0x3F20]
        plane, pw, ph, px0, py0 = compose(records, PATTERN, slot, palette)
        emu, ew, eh = emu_indices(STAGE / f"shot-{frame_no}.png")
        bad = tot = 0
        for y in range(ph):
            cy = base_y + py0 + y - 8  # fceux crops 8 lines top and bottom
            if not 0 <= cy < eh:
                continue
            for x in range(pw):
                v = plane[y * pw + x]
                if v is None:
                    continue
                tot += 1
                if emu[cy * ew + (base_x + px0 + x) % ew] != v:
                    bad += 1
        print(f"  frame {frame_no}: composed frame vs emulator screenshot: "
              f"{tot - bad}/{tot} pixels match ({100 * (tot - bad) / tot:.1f}%)")
        ok = ok and bad == 0

    # --- tiles.png: the $1400 window for each captured moment, both sprite
    # palettes (0 = body, 3 = face), left to right: walk/0, walk/3, idle/0, idle/3.
    panels = []
    for _, _, _, ppu, slot in caps:
        palette = ppu[0x3F00:0x3F20]
        for pal in (0, 3):
            panels.append(render_tiles(PATTERN, slot, rgb,
                                       palette[SPRITE_PALETTE_OFFSET + pal * 4:][:4]))
    tiles = Image.new("RGB", (sum(p.width for p in panels) + 12 * (len(panels) - 1),
                              max(p.height for p in panels)))
    x = 0
    for p in panels:
        tiles.paste(p, (x, 0))
        x += p.width + 12
    tiles.save(OUT / "tiles.png")

    # --- frames.png: the composed frame for each captured moment -----------
    frame_imgs = []
    for slot, ppu, records in previews:
        frame_imgs.append(render_frame(records, rgb, ppu[0x3F00:0x3F20], PATTERN, slot))
    frames = Image.new("RGB", (sum(p.width for p in frame_imgs) + 24 * (len(frame_imgs) - 1),
                               max(p.height for p in frame_imgs)))
    x = 0
    for p in frame_imgs:
        frames.paste(p, (x, 0))
        x += p.width + 24
    frames.save(OUT / "frames.png")

    # --- animations.ron -----------------------------------------------------
    written = states_written_by_code()
    lines = [
        "// Batman sprite animation table -- generated by tools/emu/sprites.py.",
        "// Do not edit by hand.",
        "//",
        "// Each sprite is drawn at (base_x + dx, base_y + dy); `tile` is the OAM",
        "// tile byte (bit0: 1 = pattern table $1000). attr bit6/7 flip X/Y, bits",
        "// 0-1 pick the sprite palette. Batman's tiles live in the $1400 CHR",
        "// window; the bank mapped there changes per animation set.",
        "(",
        "    sprite_window: \"$1400\",",
        f"    walk_cycle: {walk_cycle()},",
        f"    walk_period_frames: {WALK_PERIOD_FRAMES},",
        "    // $047D is a per-state sub-counter: 0..7 while walking (one walk",
        "    // state per 8 frames), and the jump phase while airborne.",
        "    jump_phases: ( ground: 0, takeoff: 1, airborne: 2 ),",
        "    frames: {",
    ]
    for state in written:
        records = frame(state)
        name = STATE_NAMES.get(state)
        lines.append(f"        {state}: (  // ${state:02X}" +
                     (f" {name}" if name else ""))
        lines.append("            sprites: [")
        for dy, attr, dx, tile in records:
            lines.append(
                f"                (tile: {tile:#04x}, attr: {attr:#04x}, "
                f"palette: {attr & 3}, "
                f"flip_x: {'true' if attr & 0x40 else 'false'}, "
                f"flip_y: {'true' if attr & 0x80 else 'false'}, "
                f"dx: {dx - 256 if dx >= 128 else dx}, "
                f"dy: {dy - 256 if dy >= 128 else dy}),")
        lines.append("            ],")
        lines.append("        ),")
    lines += ["    },", ")", ""]
    (OUT / "animations.ron").write_text("\n".join(lines))

    print(f"wrote {OUT.relative_to(ROOT)}/tiles.png, frames.png, animations.ron")
    print(f"animation states written by code: "
          f"{', '.join(f'${s:02X}' for s in written)}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
