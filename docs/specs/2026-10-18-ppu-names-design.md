# The PPU registers named

Status: approved with changes by the Opus stand-in, 2026-10-18 (applied, and `SHOWN` from the implementation's review: the bits beside
`render::Line`, not a new module; `Vram::push_palette` instead of a palette const; the nametable
bits split; every odd mask named; the counts and the missed sites; the tests' policy). The survey
loop's #4 (MODERNIZE "The survey loop, 2026-10-18"). No behaviour change: constant substitution,
proven by the lockstep (which compares every frame's 240 lines and the IRQs' register writes).

## What is there

- `Scroll::ppu_ctrl` (`$FF`, the NMI's `$2000` shadow) and `Scroll::mask` (`$FE`, `$2001`) are
  written raw at about 95 sites in 10 files: `= 0xA8` (8), `&= 0xFC` (6), `|= 0x80` (6, plus two
  `(.. & 0xFC) | 0x80`), `|= 1`, `^= 1`, `|= 2`, `^= 2`, `&= 0x7C`/`0x7B`/`0x78`/`0xFB`/`0xFD`/
  `0xFE`/`0x7F`, and for the mask `= 0`, `= 0x18`, `= 0x1E`, `|= 0x18`, `&= 0xE7`; the IRQ
  handlers' `Reg::Ctrl(.. | 1)` and `Reg::Mask(0x0A)`; the ending's `Row::Mask(0x1E)` (5).
- 22 readers decode them raw: `render.rs` (`mask` bits 0-4, `ctrl & 0x08`/`0x10`/`0x20`),
  `raster.rs` (`Regs::apply`'s `value & 3`, `mask & 0x18`, `ppu_ctrl & 3`), `oam.rs`
  (`mask & 0x10`, `ctrl & 0x20`), `actor::modern` (`mask & 0x08`, `ctrl & 0x10`), the oracle
  (`0x10`, `0x28`), `room.rs` (`& 1`, `& 3`), `attract.rs`, and the app's `ui.rs`, `modern.rs` and
  `play.rs` (`mask & 0x18 == 0`, rendering off).
- The palette's VRAM address `0x3F00` is a literal at 20 `vram.push(0x3F00, 0, ..)` sites (all
  flags 0), two test sites and `Ppu::poke`'s `addr >= 0x3F00`.

## The change

- Beside `render::Line`, whose `ctrl`/`mask` docs already describe the bits, two modules of named
  masks like 1.1.18's `oam::attr`, named by what the PPU does with each bit:
  - `render::ctrl`: `NAMETABLE_X = 0x01`, `NAMETABLE_Y = 0x02`,
    `NAMETABLE = NAMETABLE_X | NAMETABLE_Y`, `VERTICAL_INCREMENT = 0x04`,
    `SPRITE_PATTERNS_HIGH = 0x08`, `BACKGROUND_PATTERNS_HIGH = 0x10`, `TALL_SPRITES = 0x20`,
    `NMI = 0x80`;
  - `render::mask`: `GREYSCALE = 0x01`, `LEFT_BACKGROUND = 0x02`, `LEFT_SPRITES = 0x04`,
    `BACKGROUND = 0x08`, `SPRITES = 0x10`, `RENDERING = BACKGROUND | SPRITES`, and
    `SHOWN = RENDERING | LEFT_BACKGROUND | LEFT_SPRITES` (the full picture, written 11 times).
  Bit 6 and the emphasis bits are left out: nothing uses them. Not newtypes: the bytes are RAM the
  lockstep compares and the game does arithmetic on them. `Line`'s field docs link to them.
- Every combination is spelled out, no literal left: `0xA8` = `NMI | TALL_SPRITES |
  SPRITE_PATTERNS_HIGH`; `0x1E` = `SHOWN`; `0x0A` =
  `BACKGROUND | LEFT_BACKGROUND`; `&= 0xFC` = `&= !NAMETABLE`; `&= 0xE7` = `&= !RENDERING`;
  `0x7C` = `!(NMI | NAMETABLE)`; `0x7B` = `!(NMI | VERTICAL_INCREMENT)`; `0x78` =
  `!(NMI | VERTICAL_INCREMENT | NAMETABLE)`; `0xFB` = `!VERTICAL_INCREMENT`; `0xFD` =
  `!NAMETABLE_Y`; `0xFE` = `!NAMETABLE_X`; `0x7F` = `!NMI`; the one-bit writers (`|= 1`, `^= 2`,
  `Reg::Ctrl(.. | 1)`) take `NAMETABLE_X`/`NAMETABLE_Y`; `room.rs`'s mirrored flip is
  `(flipped & !NAMETABLE) | ((flipped & NAMETABLE_Y) >> 1)`.
- Every reader above uses the same names, so the Classic and Modern pictures decode the registers
  one way.
- `Vram::push_palette(&mut self, bytes: &[u8])` replaces the 22 `push(0x3F00, 0, ..)` calls; the
  address is one private const in `world.rs`, shared with `Ppu::poke`'s palette range check. `stage::PALETTE_OFFSET`
  (an offset into the dump, beside `NAMETABLE_OFFSET`) stays.
- Tests that build a `Line` or registers use the names (`render.rs`'s, `actor::modern`'s, the
  app's `ui.rs`: `0x1F` = `SHOWN | GREYSCALE`, `0x0E`); a test building a raw `VramWrite` or
  checking a parsed address keeps `0x3F00`;
  `tape.rs`'s hash of RAM byte `$FE` stays a literal.
- Out of scope: the nametable and attribute addresses (the ROM's write lists carry them as data;
  the banner's and the HUD's fixed cells are one-offs, each documented by address); a VRAM queue
  entry's own flag byte (`write.flags & 4`, world.rs, not `$2000`'s bit though it means the same);
  `raster.rs`'s band nametable bit built arithmetically (`(bands[5] >> band) & 1 | ppu_ctrl`);
  the IRQ handlers' write timing; the mapper's registers.

## Proof

The Classic suites (the chain lockstep compares the lines' `ctrl`/`mask` and every IRQ register
write), the picture tests (`render.rs`, the app's pixels against the oracle's, the Modern render),
a sweep and a 300-branch exploration unchanged; tapes byte-identical.
