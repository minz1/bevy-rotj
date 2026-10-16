# Stage assets (captured from the running game)

How a stage gets into `assets/`, and how far the port is from playing it.

## Method

`tools/emu/dump.lua` dumps whole address spaces at chosen frames, so a stage can
be lifted without decoding the game's own level format — the nametables,
palette RAM and pattern view the game left behind *are* the level:

    mkdir -p tools/emu/stage
    DUMP_DIR="$PWD/tools/emu/stage" INPUT=physics DUMPS="700,900" \
    SHOTS="700,900" RAMSEQ=off UPTIME=1001 \
    timeout -k 5 500 fceux --sound 0 --loadlua "$PWD/tools/emu/dump.lua" \
      "$PWD/raw/Batman - Return of the Joker (USA).nes"

    cargo run -p rotj-tools -- stage --dump tools/emu/stage --frame 700

## What `rotj stage` writes (`assets/levels/stage-1-1/`)

| file | contents |
| --- | --- |
| `palette.bin` | 32 bytes of palette RAM (`$3F00-$3F1F`) — the stage's real colours |
| `nametables.bin` | both physical nametables (`$2000-$27FF`) |
| `pattern.bin` | the live `$0000-$1FFF` pattern view (8 KiB) |
| `collision.bin` | WRAM `$6000-$7FFF`, the level's collision map the loader built |
| `windows.txt` | which CHR bank each 1 KiB PPU window was mapped to |
| `meta.txt` | frame, stage/sublevel, tile counts, collision density |

Stage 1-1 (stage 0, sublevel 8, captured frame 700):

    palette: 0F 07 17 37 | 0F 05 16 27 | 0F 03 12 22 | 0F 0B 00 10
             0F 0F 11 2C | 0F 0B 00 30 | 0F 0F 18 36 | 0F 15 26 30
    nametable tiles used: 82 distinct
    collision: 5,561 non-zero bytes
    CHR windows: $0000 bank 05+4K, $0400 bank 05+5K, $0800 bank 05+6K,
                 $0C00 bank 06+5K, $1000 bank 14, $1400 bank 11+1K,
                 $1800/$1C00 ambiguous (blank sky tiles match several banks)

## Verification — the renderer reproduces gameplay exactly

`rotj-verify screen` renders a dump through `rotj_core::render`;
`tools/emu/compare.py` diffs NES colour indices against the emulator's own
screenshot (RGB tables differ between emulator and port, so only indices are
comparable):

| frame | scene | colour-index mismatch |
| --- | --- | --- |
| 300 | attract/title | 0.00% |
| **700** | **stage 1-1 gameplay** | **0 / 57,344 = 0.00%** |

Frame 700 is at the level start, where the camera scroll is still 0, so a single
render configuration reproduces the frame exactly: tiles, attributes, palette,
sprite layer and nametable arrangement are all correct. That is the visual
foundation for the playable level.

## Collision and the level's shape

Collision comes from the **metatile maps the game itself builds**, captured from
WRAM:

| what | where | contents |
| --- | --- | --- |
| screen 0 | WRAM `$6000` | 16 columns x 14 rows of 16x16 px metatiles |
| screen 1 | WRAM `$6E00` | the same for the next screen |

- Row alignment: map row `r` covers screen y `16 + r*16`, because the map spans
  y 16..239 (the 240 px screen minus its first row). The bytes after the map are
  unrelated data (6502 code at `$60F0`).
- The solid ground band is metatile **`$FF`** (rows 10-13 of both maps = y 176
  to 239). Everything else in these maps is decoration for this stage.
- `rotj_core::level` (see `docs/LEVELDATA.md`) decodes the loader that builds
  this: the loader bit-expands 80 ROM bytes into a 640-byte bitmap, and the
  *visible* nametable is generated from the metatile map by an auto-tiler whose
  compact source is not yet decoded.

Consequences for the port:

- The playable level is **512 x 240 px** (two captured screens), which is
  exactly the extent of the captured nametables — so what the port can walk on
  is what it can also see.
- Solidity is currently `metatile == $FF` only. The stage's decorative walls use
  other ids; their solidity needs the game's tile-attribute table, which is in
  `docs/LEVELDATA.md`'s unknown list. Until then the port's ground is exact and
  its walls are the level's edges.
- The actor's y is the **ground line** he stands on (192 at rest), resting on
  the solid metatile row just above it (`docs/PHYSICS.md`).


## What is still missing for a fully playable stage 1-1

Done and verified: the stage loads from extracted assets, the port's renderer
reproduces a real gameplay frame exactly (`rotj-verify` / the app's
`opening_frame_matches_the_emulator` test: 0 of 57,344 pixels differ), Batman
walks (2 px/frame), jumps (23 px, ~23 frames) and collides with the ground, and
the camera follows him.

Still missing:

1. **The live camera source.** `$00AC` turned out to be the actor's own x, not
   the scroll; the walking camera lives in the `$043D-$047F` working set. The
   port uses its own dead-zone camera until this is identified.
2. **The band/CHR-bank schedule.** Scrolled gameplay frames are composed
   mid-frame (band scroll + 1 KiB CHR bank swaps via the FME-7 IRQ counter,
   `disasm/MAP.md` §3), so they cannot yet be compared against the emulator.
   The renderer already draws per scanline band; it needs the captured schedule.
3. **Solidity of decorative walls.** Only metatile `$FF` is solid today; the
   game's tile-attribute table would make the stage's other structures solid.
4. **Animation.** The actor is drawn in his captured standing pose; the walk and
   jump poses come from the animation tables (being recovered).
5. **Level streaming.** The playable slice is the two captured screens (512 px);
   extending it needs the metatile map's ROM source and the auto-tiler.
6. **Actors, weapons, HUD.** OAM is captured per frame, but enemies/objects come
   from the game's object tables (`$7D00`-area slots), which need their own
   decode.

