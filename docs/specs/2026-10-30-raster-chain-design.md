# The raster chain typed: `$D3` as `Chain`, the bands named where they are used

Status: approved by the Opus stand-in (L3), 2026-10-30, after the reviewer's pass: the band views
of the first draft became a destructuring per handler, and `phases!` stays where it is. Variant
names chosen by the implementation (stand-in [review]): `TwoBands`, `ThreeBands`, `FixedRow`,
`BossFloor`, `BossBar`, `FourBands`, `SwingingRow`, `FourBandsTall`, `FixedTop`, `Intro`,
`Cutscene`, `Closing`, `EndingTitle`, `Credits`. Survey #7 (`.orchestration/survey2.md`),
HANDOFF roadmap 2, before Modern's `$D3` 3 boss floor and band rooms (roadmap 3). No behaviour
change: the lockstep proves it, tapes unchanged. Implemented in 1.1.48.

## What is there

- `Raster.handler: u8` (`$D3`) picks one of 14 IRQ handlers (0-13) in `raster::handler`'s
  `match`, behind a `debug_assert!` and a `_ => None` arm, and `chain()` indexes `FIRST` with a
  fallback. Other sites set or compare it with literals: `boot.rs`'s `arm_slide` (9),
  `room.rs`'s `INTRO_SPLIT` (9), `cutscene.rs` (`$0A`), `ending.rs` (`$0C`, `$0D`),
  `raster.rs`'s `four_bands` (`== 5`), `modern.rs` (`== 4`, tests set 3 and 4), `app/play.rs`
  (`== 9`), `oracle.rs`'s divergence message, `levels/tests.rs`' fixture, the room parser
  (`rooms.rs`, screen record byte 3) and the lift/projection. The ROM's rooms use 0-8; the boot,
  cutscene and ending scripts set 9, `$0A`, `$0C`, `$0D`; nothing sets `$0B`.
- `Scroll.bands: [u8; 8]` (`$D5-$DC`) is a union: each handler gives the bytes its own meaning (a
  band's x, a bounce flag, nametable bits, 8.8 positions), by index, 58 times in `raster.rs` and
  a few in `boot.rs`, `cutscene.rs`, `ending.rs`, `room.rs`. The bytes outlive their chain: a room
  without a split sets only `$D0`, so the next chain starts on the last one's bytes, and the
  lockstep compares all eight.

## The design

1. **`Chain`**: `Raster.handler: u8` becomes `Raster.chain: Chain`, a `phases!` enum at `"$D3"`
   in `rooms.rs` beside `Raster`, one variant per handler 0-13, `Default` = `TwoBands` (byte 0,
   cleared RAM's; `Raster` and `Scroll` derive `Default`). `handler`'s `match` is exhaustive (no
   `debug_assert!`, no `_` arm); `chain()` indexes `FIRST` by `chain.byte()`. Every site above
   names its variant; `oracle.rs` prints `chain.byte()`.
   - The lift: `Chain::from_byte(ram[0xD3]).expect(..)`, beside `$D0`'s assert (fceux's power-on
     RAM and the game's reset both leave `$D3` 0). The projection writes `chain.byte()`.
   - The room parser maps byte 3 through `Chain::from_byte(..)?` inside the non-empty branch;
     `every_listed_room_imports` (38 rooms, 24 empty) shows no room is lost.
   - The level-pack schema does not change: `hud_split` stays `standard`/`none`.
2. **The bands named**: `bands` stays `[u8; 8]`, the RAM union. Each handler that keeps state
   names its bytes once, at the top, with a pattern on `&mut scroll.bands`
   (`let [playfield, top, far, falling, ..] = &mut scroll.bands;`); `step` and `bounce` take the
   bytes they change and the gate. Nothing is copied, so nothing is stored back.
   - `cutscene.rs` names the `$0A` bytes the same way; boot's "all five intro bands slid" `$1F`
     is one const that the intro handler's bits use too.
   - `bands[..6].fill(0)` (boot, ending), `INTRO_BANDS` and the room's `bands = [0; 8]` stay.
3. **What stays Classic's rule**: the cycle tables (`FIRST`, each IRQ's counter and overhead,
   each write's offset) and the `Reg` writes, unchanged and in place.

## Proof

- `phases!`'s round-trip test for `Chain`; `every_listed_room_imports`.
- `the_chain_is_the_games_over_the_tas` (ignored, in the full set), the Classic suites, a sweep
  and an exploration unchanged; the tapes byte-identical.

## Not here

Modern's use of `Chain` and the bands (roadmap 3, its own spec; a band read outside `raster.rs`
gets a read-only accessor the handler reads through too); the cycle tables as data.
