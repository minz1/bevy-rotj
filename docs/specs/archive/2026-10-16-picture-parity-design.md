# Picture parity: the raster splits, and pixels checked against the emulator (1.0.2)

Status: done 2026-10-16 (tag 1.0.2). Approved by a stand-in reviewer under the owner's
delegation (changes requested; all applied). As built, three things differ from the text
below: a frame's lines are built at its loop's end (the IRQs read RAM that loop has
written), not at the NMI; a line where the real game itself varies (an IRQ write within
its wander of the line's end, a byte the loop writes in the frame an IRQ reads it) may
show either outcome, counted and pinned (`docs/STATUS.md` "Decided autonomously"); and the
app's own pixels are compared with the oracle's from power-on through the attract
(`play::tests::the_app_draws_like_the_oracle_from_power_on`). Owner, 2026-10-16: "work autonomously until the bug is fixed
... update the tests to check the images too ... find a way to test for this so this doesn't
happen again."

## Why

The owner's playtest of 1.0.1 showed the stage 1 boss's status bar as garbage, and warping
at the top of the screen while jumping in stage 1. Investigation (2026-10-16):

- The game runs an FME-7 raster IRQ chain (`$E2A9`, handler by `$D3` through `$E2CA`) in 12
  areas, not only the area STATUS named: `$D3` 0 (`$0400 = $02`), 1 (`$10`), 2 (`$12`), 3
  (`$04`, the stage 1 boss), 4 (`$0E`, `$14`, `$1E`, `$22`: the other bosses), 5 (`$08`), 6
  (`$0C`), 7 (`$18`), 8 (`$1C`). The handlers set per-band horizontal scroll, point the PPU at
  another nametable row (`$2006`), and switch background CHR banks for the boss status bar.
- The app models only `$D3` 9, `$0A`, `$0C`, `$0D` (`play::split`, `cutscene::raster_irqs`:
  "shortcut: area 2's split ... is not run"); for 0-8 it draws one band with the top's banks.
- No check saw it: the core is byte-exact (the lockstep injects the IRQs' RAM bytes), picture
  parity compares nametables and palette only, and the one pixel test is the title.

## Goal

The app's picture equals the real game's on every frame the lockstep reaches (the whole TAS,
random play, the explorer, playtest replays), except on a scanline a mid-frame write splits,
where it shows one of the line's two states; and the reference those frames are compared with
is proven pixel-exact against fceux on every frame of the TAS.

## Design

### 1. Line states (oracle)

The oracle's machine runs the 240 visible lines one at a time and records, per line, what the
PPU draws it with: the render address `v` (kept apart from the CPU-visible `v` by the PPU's
rules: copied from `t` on the pre-render line and by a second `$2006` write, stepped down a
line and given `t`'s horizontal bits at dot 256), fine X, `$2000`, `$2001`, the eight CHR banks,
the sprites' CHR banks (as at dot 256 of the line before, when the PPU fetched them), and
mirroring. A write that changes a line's state while it is drawn (before dot 256), or in the
hblank before it, marks the line split, with its two states.

Measured against fceux (2026-10-16): with these three rules (the line ends at dot 256; a
split line shows one state up to some pixel and the other after, in either order; sprites use
the previous hblank's banks) the oracle's rendered frames equal fceux's screenshots on all
48,100 frames of the TAS (power-on to the ending's hold): 0 differing frames, split lines
included. Moving the line end to dot 341 broke the `$2006` split (area `$12`, 5281 frames);
line-start sprite banks broke the stage 1 boss's bar (486 frames).

### 2. One renderer, one render function

`render::Frame::render(&Picture)` draws a frame from line states with the PPU's rules: the
background per line from `v`, fine X and the line's banks, `$2001` enables, left-8 clipping
and greyscale; up to eight sprites per line, the lowest OAM index in front, behind the
background by its priority bit, clipped at the right edge, 8x8 or 8x16 and the 8x8 table
from `$2000`. `rotj_core::raster::lines(&Game) -> [Line; 240]` builds the port's lines from
its state (the NMI's scroll, `$2000`, `$2001`, banks, mirroring, and the frame's raster
IRQs), and `rotj_core::raster::picture(&Game, &Ppu, chr)` renders it. The app's `compose`
is a call to that; `play::split` and `cutscene::raster_irqs` go.

### 3. The port's raster IRQs

All of `$D3` 0-8 (`$DDF4-$E1CF`) are ported next to 9, `$0A`, `$0C`, `$0D`: each IRQ's
effect on the lines (from which scanline, what changes) and on RAM (`$D4-$DA`, `$D1:$D2`,
`$FD`...). An IRQ's scanline is fixed per handler and IRQ, from the oracle's measurements
over the TAS: the IRQ entry varies by 20-30 CPU cycles, under a third of a line, and the
split points sit mid-line, so the line is stable; where a write lands mid-line the line is a
split line on the real game too, and the port shows one of its two states.

### 4. Tests

- **Oracle vs fceux** (`tools/rotj-oracle/tests/picture.rs`, `#[ignore]`, release in the
  gates): power-on and the TAS to frame 48100; every frame's render digested (FNV-1a 64 of
  lines 8-231) and compared with the committed digest of fceux's screenshot
  (`tests/data/tas-picture.txt`, one line per frame). A split line is checked by the
  crossover rule only where the local capture is present (the digest of a split frame is
  that of fceux's own pixels, so a digest match is exact); without it a mismatch names the
  frame and the regenerate command (`dump.lua INPUT=replay SHOTS=1,48100`).
- **Port vs oracle** in every lockstep (`assert_game` and the chain, the sweep, the explorer,
  playtest replays): at every NMI, the port's 240 lines for the frame equal the oracle's;
  on an oracle split line the port's line must equal one of its two states. Failures name
  the frame, the line and the field. With the shared renderer and the already compared
  OAM, nametables and palette this is picture equality.
- **The IRQs' RAM:** the lockstep keeps injecting the IRQs' bytes (`inject_irqs`), but first
  asserts that the port's own step produced the same values for the bytes it models; an IRQ
  whose `$D3` the port does not model fails the lockstep. That tripwire alone would have
  caught 1.0.1's bug.
- **Tripwires** in the oracle: a `$2007`, `$2004`, `$4014` or palette write while a visible
  line is drawn fails, so the line model's assumption (no VRAM, OAM or palette change
  mid-frame) is checked.
- **Unit tests**: the renderer's sprite rules (priority, edge clip, size and table, left-8),
  and the oracle's `v` tracking with synthetic mid-frame `$2005`/`$2006` and CHR writes.

## Coverage, said plainly

The TAS reaches power-on, the intro, the title, every stage, room, boss and cutscene and the
ending, but not the password screen, GAME OVER and continue, or deaths: the existing boot
tests (`boot::tests`, passwords and the attract mode from power-on), the sweep's death and
GAME OVER runs and the playtest replays reach those under the port-vs-oracle check. Lines
0-7 and 232-239 are outside fceux's screenshot; the port-vs-oracle check covers them. Not
modelled: emphasis bits (the game does not set them: checked by the tripwire on `$2001`
bits 5-7 during the TAS), and sprite-0 hit and overflow (the game splits with the FME-7 IRQ).

## Out of scope

The 1.0.3 ROM embedding follows this.
