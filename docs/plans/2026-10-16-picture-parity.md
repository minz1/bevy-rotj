# Picture parity Implementation Plan

Status: draft for the stand-in review.

**Goal:** the app draws every frame like the game, the raster splits included, and every
lockstep proves it against the oracle, whose pictures are proven against fceux's.

**Spec:** `docs/specs/2026-10-16-picture-parity-design.md`

## Global constraints

- No `unsafe`, clippy pedantic clean, no lint suppressions, rare one-line comments.
- No new dependencies. Commits: short subject, no Co-Authored-By. Push to `origin main`
  after the release (owner, 2026-10-16); `.git/hooks/pre-push` must stay in place.
- Every Classic suite stays green; the 1.0.1 gates plus the new picture test.

## Task 1: the oracle's lines and the fceux test (done in part)

- `tools/rotj-oracle/src/{ppu,machine}.rs`: `Line`, `FrameLog::{lines, splits}`, the
  render address, `LINE_END_DOT = 256`, sprites' banks from the previous line (written
  2026-10-16, 0/48100 frames off fceux).
- `ExecEvent::cycle` (frame-relative CPU cycle), for the measurements in task 3.
- Tripwires: a `$2004`/`$2007`/`$4014` write or a palette write during a visible line, or
  `$2001` bits 5-7 set, panics (`ppu.rs`, with a unit test each).
- Unit test `render_address_follows_the_ppu`: synthetic writes (`$2006` mid-frame, `$2005`
  in hblank, a CHR write mid-line) give the expected lines and splits.
- `tools/emu/digest.py` kind `shots <dir> <first> <last>`: one digest line per frame of
  lines 8-231 of fceux's indexed screenshot. `tools/rotj-oracle/tests/data/tas-picture.txt`.
- `tools/rotj-oracle/tests/picture.rs::the_oracle_draws_every_tas_frame_like_fceux`
  (`#[ignore]`, release): power-on, the TAS to 48100, render each frame through
  `rotj_core::render::Frame::render`, compare digests; with `tools/emu/tas-shots/all.idx`
  present, a mismatch is reported by row with the split-line crossover rule. A mutation check
  (one digest digit) fails it.

## Task 2: the renderer

- `crates/rotj-core/src/render.rs`: `Line`, `Picture`, `Frame::render` (written). Unit tests:
  `the_lowest_oam_index_is_in_front`, `sprites_clip_at_the_right_edge`,
  `sprite_size_and_table_follow_ctrl`, `left_eight_pixels_clip`, `greyscale_masks_the_hue`.
- The old `Background`/`Sprites`/`background`/`sprites` API is deleted once the app moves
  (task 5) and its tests are rewritten on `render`.

## Task 3: the port's raster model

`crates/rotj-core/src/raster.rs` (new):

- `pub fn lines(game: &Game) -> Lines` (240 `Line`s plus the frame's split lines): the NMI's
  base state (`$FD`, `$FC`, `$FF` into `t`, CHR banks `$E0-$E7`, mirroring `$EC`, `$2001`
  `$1E`) stepped down the frame by the PPU's rules, with each raster IRQ's register writes
  (`$2005`, `$2006`, CHR) applied at its write's scanline and dot.
- The write positions: the first IRQ's entry at `IRQ_ARM + counter($D1:$D2)` cycles, each
  next at a constant offset from the one before, and each write at a constant offset from its
  IRQ's entry, per `$D3` and IRQ index, in a table measured from the oracle over the TAS
  (means, rounded), with a test (`raster::tests::the_timing_table_is_the_oracles`) that
  re-measures and checks the table.
- `pub fn run_irqs(game: &mut Game)`: the handlers' RAM effects for `$D3` 0-`$0D` (the nine
  new ones from `$DDF4-$E1CF`, the four from `cutscene::raster_irqs`), in IRQ order.
- `pub fn picture(game: &Game, ppu: &Ppu, chr: &[u8], frame: &mut Frame)`.
- `cutscene::raster_irqs` and the app's `split`/`split_line` are deleted.

## Task 4: the lockstep checks

`crates/rotj-core/src/actor/oracle.rs`:

- At every NMI the lockstep already wakes on, compare the port's `raster::lines` for the
  frame with the oracle's `FrameLog` lines of the frame that NMI began (kept by the scene);
  an oracle split line accepts either of its two states. Failure: frame, line, field.
  Applies to `assert_game` and so to the chain, the sweep, the explorer and playtests.
- Before `inject_irqs`, run `raster::run_irqs` on a clone and assert the modelled bytes
  (`$D1`, `$D2`, `$D4-$DA`, `$FC`, `$FD`, `$FF`) equal what the oracle's IRQs left; an
  IRQ with a `$D3` the port does not model fails.
- Record in the tape whatever replay needs (the line hash is not added: the lockstep checks).

## Task 5: the app

- `Play::compose` calls `raster::picture`; `Play::step` calls `raster::run_irqs` where it
  called `cutscene::raster_irqs`. The app's raster tests (`the_intro_animates`, the cutscene
  and ending tests) move to `raster`'s rules; the title digest test still passes.
- `Play` replays the owner's recording `1792151201551-level0.pads` headless (local only) and
  its boss frames show the status bar (a check by eye, recorded in the report, not a test).

## Task 6: release 1.0.2

STATUS (the bug, the checks that missed it, the new gates), PLAN, `gates.sh` (the picture
test in the release `--ignored` run), Windows build, the owner's recording replayed through
the new exe by eye, tag `1.0.2`, push `main` and the tag.

## Then 1.0.3

The build-time ROM embedding per `HANDOFF-1.0.2.md` (written 2026-10-16), tagged and pushed
the same way.
