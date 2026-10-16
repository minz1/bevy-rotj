# Batman's sprites

How Batman is drawn, where his tiles live, and how the port can play his
animations. Everything here comes from the captured stage-1-1 frames
(`tools/emu/stage/`) and the ROM; the generator is `tools/emu/sprites.py`.

## Files

| file | contents |
| --- | --- |
| `assets/sprites/batman/tiles.png` | the 8x8 tiles of his CHR window — four panels, left to right: walk/palette 0, walk/palette 3, idle/palette 0, idle/palette 3 |
| `assets/sprites/batman/frames.png` | his composed 8x16 frames, drawn in OAM order |
| `assets/sprites/batman/animations.ron` | animation state -> sprite list (tile, attr, flip, dx, dy) |

Regenerate with `python3 tools/emu/sprites.py` (reads the dumps, the pattern
space and the ROM; prints its verification numbers).

## How his graphics are addressed

PPUCTRL is `$A8` (`docs/PHYSICS.md`): **8x16 sprites**, so each OAM sprite is a
tile *pair* and the tile byte's bit 0 picks the pattern table (`1` = `$1000`).
All of Batman's tile bytes are odd (`$41 $45 $51 ...`), so every sprite reads
from `$1000`: the tile pair starts at `$1000 + (tile & $FE) * 16`, so his tile
numbers `$40-$7F` occupy **PPU `$1400-$17FF`** — the third 1 KiB CHR window.

That window is FME-7 CHR bank register 5; the game's shadow of it is CPU RAM
`$8D` (`disasm/MAP.md` §1: `$88-$8F` is the CHR 1 KiB bank shadow). The value
written *is* the slot index into `assets/graphics/chr/pattern-space.bin` (the
`windows.txt` provenance method: `slot = bank*8 + KiB`, and the shadow byte
equals that slot). So:

| frame | state `$0466` | `$8D` (slot) | CHR bank |
| --- | --- | --- | --- |
| 700 | `$0E` = 14 (walk) | 137 | `$11 + 1 KiB` |
| 900 | `$00` = 0 (idle) | 128 | `$10 + 0 KiB` |

(these are the first two panels of `tiles.png`; panels 3-4 repeat them with
palette 3).

**The bank changes with the animation set** (slots 137 and 128 are different
data — 653 of 1024 bytes differ), so the same tile number means different art in
different states. The port must switch the `$1400` window with the animation,
not just the tile bytes.

Sprite palettes are palette RAM `$3F10-$3F1F`; Batman uses **palette 0**
(`$0F $0F $11 $2C`: black/blue/cyan) for the body and **palette 3**
(`$0F $15 $26 $30`: skin) for the face — the face sprite is the one with
`attr & 3 == 3` (e.g. tile `$41`).

## How the animation table is indexed

`$0466` is Batman's animation state. `$047D` is a sub-counter: while walking it
counts `0..7` (one state per 8 frames); while airborne it is the jump phase
(observed `0` ground, `1` take-off, `2` airborne at frames 801-824).

The **walk cycle** is a bank-4 table at `$96FD`, indexed by `$047E` (which
increments when `$047D` wraps at 8):

```
$96FD: 0E 0F 10 11 12 13 0C 0D     ->  states 14,15,16,17,18,19,12,13
```

Before the cycle starts, `$047F` bit 4 forces state `$3E` (62, "walk start") for
the first 6 frames (`$8A86`, bank04.s). This matches the captured leads exactly:
`62` at frames 688-693, then `14,15,...,13` in 8-frame blocks.

The sprite frame for a state is up to three descriptor scripts (bank 1), all
selected through bank-4 pointer tables:

| part | pointer | when |
| --- | --- | --- |
| main | `$9924[$0466*2]` (states `$00-$7F`), `$9A24[($0466*2)&$FF]` (`$80-$FF`) | always |
| extra | `$9924[$9705[$0466]*2]` | when `$9705[$0466] != 0` |
| overlay | `$9924[$97A4[$0466]*2]` | when `$97A4[$0466] != 0` |

A descriptor (copied straight into OAM by the bank-0E routine `$C247`) is
`[length L][ L-1 bytes of 4-byte records, read backwards ][ trailer ]`; each
record is `[dy, attr, dx, tile]` and the sprite lands at
`($94 + dx, $95 + dy)`. The tables are "facing right"; the engine mirrors a
whole frame by OR-ing `$046F` bit 6 into every `attr` (and negating `dx`).
`animations.ron` lists the frames for every state the code writes to `$0466`
(the immediates plus the `$96DB/$96DD/$96E0/$96F1/$96F3/$96FD/$979D/$979F`
tables).

## Evidence

Proof (b) from the task — reconstruct and compare by NES colour index. The
generator prints these numbers on every run:

- **Sprite pixels**: composing the decoded frame for state 14 and comparing
  every drawn pixel to `shot-700.png` gives **665/665 (100.0%)**; for state 0
  against `shot-900.png`, **672/672 (100.0%)**.
- **Whole sprite list**: the decoded frame matches **10/10** captured OAM
  entries at frame 700 (base `(115,193)`) and **11/11** at frame 900 (base
  `(128,193)`), by tile byte, palette/flip and screen position.
- **Whole frame** (measured separately with a Python reimplementation of
  `rotj_core::render`): frame 700 reproduces `shot-700.png` at **0 / 57,344 =
  0.00%** colour-index mismatch, which also pins the background, scroll and
  nametables.

Together these pin the tiles, the `$1400` window and the sprite palette at the
same time.

## Proven vs assumed

- **Proven (measured)**: frames 700 (state 14) and 900 (state 0) — tiles,
  palette, sprite list, and the `$1400` bank each used.
- **Proven (ROM)**: the descriptor format, the pointer tables, the walk table
  `$96FD`, the walk-start rule, and the state list.
- **Assumed**: that all walk states 12-19 use the same `$1400` bank (only state
  14 was captured); the facing-left `extra` remap through `$978D/$9795`; and
  that the port should draw the frame in descriptor order.
- **Not covered**: the graphics for every state other than 0 and 14. Only the
  idle bank (slot 128) and the walk bank (slot 137) were captured; the other
  poses (`$1C`, `$24-$27`, `$38-$48`, `$85`, ...) reference tiles whose bank we
  have not seen, and the `$047D` jump phases were observed only in `leads.csv`,
  not with OAM.

## What is needed for complete coverage

1. **RAM+PPU dumps at more frames**, one per animation state, so each state's
   `$1400` bank is captured: standing idle (have), each walk frame 12-19 (have
   state 14), jump take-off/apex/fall, attack/punch, hurt, crouch, and the
   `$85` pose. The existing `tools/emu/dump.lua` with `INPUT=physics` and
   `DUMPS="796,801,803,810,820,825"` would cover the jump.
2. **The `$1400` CHR bank schedule** (which slot at which frame), so the port
   can swap windows the way the game does. Not currently dumped; the frame-700
   and frame-900 shadows are the only two samples.
