# Modern's band rooms and the boss floor: every split frame as line runs

Status: approved with changes by the owner-proxy (L3) and the reviewer, 2026-10-30. Implemented
in 1.1.49. HANDOFF
roadmap 3 (item 21), owner review #5 (`docs/MODERNIZE.md`, "the Modern picture's reach"): the rest
of the split rooms, `$D3` 3 (the boss floor) and the band rooms (`$D3` 0, 1, 2, 5-8), proven by
the background gate and the headless pixel comparison with Classic. Builds on
`.orchestration/briefs/spec-boss-bar.md` (1.1.41) and `spec-final-boss-bar.md` (1.1.42), and on
`Chain` (1.1.48). No behaviour change: the game, its RAM and the tapes are untouched; only which
frames the Modern picture draws grows. Classic's picture stays the software `Frame`.
Applied: the owner-proxy's rulings (runs from the nametables, the reason in "Not here"; one camera
per piece; `strip`/`nametable_playfield` folded into `runs` + `map_end`; the names; two commits;
the chain test counts what stays Classic) and the review (the app draws a split frame from the
lines after the loop; a run's first row is `fine_y` above its line, no underflow; each piece's
projection and camera order; the map is run 0; `Run`'s banks from its top; no pattern half; the
increment shared with the raster; what the gate proves; the headless steps; a negative gate test).
The implementation review's note that each piece rebuilds its own tilesets goes to roadmap 4 (the
sprite and atlas churn measurement).
Left Classic: 486 of the TAS's 503 boss floor frames (area `$04`), where Batman stands across the
floor's bank change and `sprites_steady` refuses (Modern draws a sprite in one set of banks).

## What is there

- `modern::view` draws a split frame Modern only for `Chain::BossBar`: the playfield from the map
  (or, in area `$22`, one nametable run from line 0), then the bar as a `Strip` from the tile row
  of its first changed line, which must meet the playfield on a tile edge. Every other armed split
  is Classic.
- The app draws two layers (`playfield`, `strip`) of four `TilemapChunk`s each, whole 8 px tiles,
  through one camera; sprites are in screen space on the same render layer, `BEHIND` ones under the
  tiles by depth.
- The app's snapshot takes a frame's lines at the NMI; Classic draws the lines after the loop. In
  `$D3` 6 and 8 they differ: the IRQ reads the scroll the loop just moved (`raster::lines`' doc).
- A probe over the TAS (2026-10-30, throwaway): in play, the split rooms other than `$D3` 4 are
  areas `$02` (`$D3` 0, 3927 frames), `$10` (1, 4336), `$12` (2, 5299), `$04` (3, 503), `$08` (5,
  2903), `$0C` (6, 395), `$18` (7, 2800), `$1C` (8, 3009), 2-7 runs a frame. Runs start mid-tile
  (`$18`'s at 31 with fine y 7, 60 with 4). Many runs are parallax art the live map does not hold
  at any x, and the arenas repeat, so a map match does not say which run is the playfield.

## The design

Every armed split frame in play is a list of line runs, each drawn from the nametables in screen
space with its own scroll and banks, clipped to its own lines; the one exception is the boss bar
rooms' playfield (outside area `$22`), still drawn from the map as in 1.1.41.

### The model (`rotj_core::actor::modern`)

- `Run` (was `Strip`): `line`, `end` (the line after its last) and `top`, `line`'s state.
  `fine_y()` is `top`'s fine y: its first tile row starts that many px above `line`. `banks()` are
  `top`'s background banks.
- `View`: `strip` and `nametable_playfield` give way to `runs: Runs` (`[Option<Run>; MAX_RUNS]`,
  `MAX_RUNS = 8`, so `View` stays `Copy`) and `map_end`, the line the map playfield stops at (240
  without a split, 0 when every line is a run).
- `runs(lines, from) -> Option<Runs>`: lines `from..240`, a new run where fine x, the mirroring or
  the background banks change or `v` is not the line above's moved down by the PPU
  (`raster::down`, the vertical half of the raster's `end_line`). Moving down past row 29 wraps
  into the other nametable, so it is a new run too (row 31 to row 0 is not). Past `MAX_RUNS`:
  `None` (Classic).
- `view` with a split armed: `Chain::BossBar` outside area `$22`: `map_end` is the end of run 0
  (240 when no bank changes), the runs from there. Any other chain, and `BossBar` in area `$22`:
  `map_end = 0`, every line a run. The other conditions (in play, the background on every line,
  `sprites_steady`) are unchanged; a frame without a split is as now (`map_end = 240`, no runs).
- `run_tile(ppu, run, column, row)` (was `strip_tile`): unchanged.

### The gate (`check_background`, every lockstep)

- The map half over lines `8..map_end.min(232)`, as now.
- `check_run` over each run's TV-visible lines (`line.max(8)..end.min(232)`): each line has the
  run's fine x and banks, fine y `(y - line + fine_y) % 8`, and each cell's nametable tile and
  palette equal `run_tile` at row `(y - line + fine_y) / 8`.
- What it proves: both sides read the lockstep's nametables, so it proves the port's lines are
  those runs and `run_tile`'s row and nametable arithmetic, not where the app puts the pieces; the
  link to the hardware stays `compare_lines` and `compare_vram`, the app's placement rests on the
  headless pixel test.
- `Lockstep::check_modern` checks `view(game at the NMI, Classic's lines)`, the pair the app draws.
- `Pictures`: `strip` and `nametable_playfield` become `runs` (split frames drawn Modern) and
  `from_nametables` (`map_end = 0`), plus `classic_splits` (split frames in play left Classic)
  and `crowded` (those past `MAX_RUNS`, which the chain test asserts is 0).

### The app (`crates/app/src/modern.rs`, `play.rs`)

- `Play::run_frame`: a frame with a split armed at its NMI redoes its `ModernFrame` from the game
  at the NMI (a clone) and the lines after the loop, so the app draws what the gate checked.
- One `Layer` per piece: the map playfield and up to `MAX_RUNS` runs. Each piece's camera renders
  into the Modern image with a viewport of its lines (full width), the default 2D projection (one
  world unit a pixel at any viewport size) centred on those lines, the render layers
  `{its own, SPRITES}` and `ClearColorConfig::None`. A backdrop camera with no layers clears the
  image first. Orders: the backdrop `-PIECES - 1`, the pieces `-PIECES..0` top down, all below the
  screen camera. Sprites on `SPRITES` draw clipped in every piece, with their depth against that
  piece's tiles. Unused pieces' cameras are off.
- A run's tiles are `run_tile` over `play.ppu` from `top` down, `(end - line + fine_y).div_ceil(8)`
  rows, its chunks' top-left corner at screen `(-top.fine_x, line - fine_y)`. The map piece is
  filled from `frame.tiles` as now, rows down to `map_end`.

### The proof

- The gate over every lockstep: the chain passes through all eight areas above with the counts
  `modern 34815, runs 25985, from_nametables 23807, classic_splits 486, crowded 0`; the sweep and
  the explorer.
- Core unit tests: `runs` splits a boss bar frame into three, on a fine x change, the wrap past
  row 29, a `$2006` move of `v`, not on row 31 to 0, and gives `None` past `MAX_RUNS`; `view` on
  the boss bar keeps `map_end = 192`, all-map without a bank change, every line a run for another
  chain and in area `$22`; `check_run` refuses a run a row off or a fine y off.
- An app test: 60 frames walking in area `$1C`, each drawn Modern from the runs of the lines after
  its loop (fails without `run_frame`'s redo).
- The headless test gains two renders, each compared with Classic over lines 8-231, x 8-247:
  `Play::start(.., 9)` (area `$18`, 61 frames walking, six runs from the nametables, starting
  mid-tile) and `Play::start(.., 11)` (area `$1C`, 31 frames walking: the IRQ's band from the
  moved scroll). The `$1C` render stops before frame 40, where the allocator's rotation puts two
  of an enemy's overlapping sprites out of draw order (Modern keeps draw order by design, the
  1.1.14 Layers). The existing three stay.

### Commits

One release in two commits: 1, the model, the gate, `run_frame`'s redo and the cameras with only
`BossBar` opened; 2, every chain, its tests and the headless renders.

## Not here

The band rooms' playfield from the map, a 1.2.0 prerequisite (`HANDOFF.md`, `docs/MODERNIZE.md`):
a second gate path for the same pixels now buys nothing, and widescreen needs a model of the
parallax art anyway. Which run is the playfield, per chain: `$D3` 0, IRQ 2's run (line 126 on);
1, IRQ 3's (143 on); 2 `FixedRow`, the top run (0-161; the row below is fixed at `$2680`); 3, line
0's run (the floor from IRQ 2 is fixed at x 0); 5 and 7, none (all parallax); 6, the run from line
141 (the `$2620` row); 8, lines 0-110 and IRQ 2's run from 194 (111-193 is fixed art). Also: a
sprite drawn in two sets of banks (the boss floor's remainder); non-split frames whose banks change
down the screen (none in play today); widescreen and interpolation (1.2.0).
