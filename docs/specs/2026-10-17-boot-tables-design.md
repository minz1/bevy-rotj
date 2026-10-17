# Scripts as data: the boot's linear parts (1.1.x step A4, slice 2)

Status: approved with changes by the Opus stand-in, 2026-10-17 (applied: the attract
re-enters at the payload copy, found by lookup; the skip check's rule stated exactly; the
`skip_intro` jump at a table end; `TitlePalette`; `copy_payload` shared; the frame table;
the ignored gates named). The follow-up `docs/specs/2026-10-17-scripts-as-data-design.md`
left ("`Boot`'s linear parts as a second table"), from a read-only survey of `boot.rs`.

## The frames, as the code has them

W takes a frame, 0 runs none; ✓ the frame is followed by the skip check (`check`).

- Power: Reset W, Palette W, Payload 0 (music `$FF`, level 0), Copy 0-2 W W W; the
  attract loop re-enters at Copy 0 (`$98CB`), skipping the music write. Then Intro.
- Intro: Enter W (ppu_ctrl `$A8`, mask 0, the clear reset); Clear's first call writes the
  palette, then, if `skip_intro`, goes to TitleSetup's start in the same frame; else Clear
  36 W (✓ from the clear's third frame: `clear.frame() > 1`) and a 0 exit; Banks W ✓;
  BanksHold W; Show W (mask `$1E`, `queue_palette`). Then Slide.
- TitleSetup: Start 0 (music `$FF`, the clear reset), Clear 36 W and a 0 exit, Banks W,
  BanksHold W, Palette W (ppu_ctrl `$A8`, `$3F00` pushed), Blocks 0-11 W (cursors from
  `TITLE_BLOCKS`), Blocks 12 0 (mask `$1E`), Show W (an empty frame). Then Title's start.
  No check.

## Design (this slice: Power, Intro, TitleSetup)

- Three tables on `actor::script`'s `Runner`, each a `const &[Row]` of its own row enum
  named for what the row does, with the cutscene's convention (write on frame 0 and
  `Waited`, `Done` on frame 1, the next table starting in the same frame):
  `POWER: [Reset, DarkPalette, StopMusic, CopyPayload, Enter, IntroPalette]`, then at its
  end a handover to `TitleSetup` if `skip_intro`, else to `INTRO: [ClearScreen,
  ShowBanks, Wait(1), ShowPicture]`, then to Slide; `TITLE_SETUP: [StopMusic, ClearScreen,
  ShowBanks, Wait(1), TitlePalette, TitleBlocks, ShowScreen]`, then to Title's start
  (`TitleBlocks` draws the twelve blocks, a frame each, and sets the mask in the zero-frame
  thirteenth). The palette write before the skip test is `IntroPalette`, a zero-frame row,
  and the branch is at the table's end, where handovers already happen.
- The attract loop re-enters Power at `CopyPayload`, by lookup
  (`POWER.iter().position(|&row| row == PowerRow::CopyPayload)`), so it cannot drift.
- `CopyPayload` uses `cutscene::copy_payload` with the first bank as a parameter (the
  boot's and the cutscene's payloads start at different banks).
- `check`, the rule exactly: a row sets it only on a call that returns `Waited`, never on
  the frame-1 `Done` call (after `step` has taken it) or in a zero-frame row. `INTRO`'s
  `ClearScreen` sets `clear.frame() > 1` after its step; `ShowBanks` sets it on its write
  frame; every other row of these tables leaves it false.
- Slice 3: `Slide` with the title's chosen-flash tail (both use `count`; `toggle` is the
  flicker's), so the hidden counters go together; the flicker runs exactly 79 frames (31 of
  countdown, 48 increments from `$C0` to `$F0`).

## Tests

- The boot's oracle tests, including the ignored ones the gates run in release:
  `start_anywhere_in_the_intro_plays_like_the_oracle` (a skip at each of 15 phases: the
  guard for `check`) and `the_attract_mode_plays_like_the_oracle` (the re-entry); the
  password and sound-test tests, `the_intro_runs_its_own_raster_split`; the Classic suites,
  the tapes, a 1-seed sweep and a 300-branch exploration. "Tests green" at each step below
  includes the ignored boot tests in release.

## Plan

1. `POWER` (with the intro's start) and the attract re-entry; tests green.
2. `INTRO`; tests green.
3. `TITLE_SETUP`; tests green.
4. Checks, the `reviewer` agent, commit `Scripts as data: the boot's power-on, intro and
   title set-up`.

## Slice 3: the frames, as the code has them

W takes a frame, 0 runs none. `Slide` sets `check` at the top of every call, so every W
frame of it is followed by the skip check; `TitleBlocks(5)` clears it before the title's
first frame. The chosen flash has no check.

| State | Frames | Writes |
|---|---|---|
| Slide `Setup` | 0 | the split armed (`room[3] = 9`, `room[1..=2] = $80, $26`, y `$C0`), cursor 0, `count` 6 |
| `Block` | W | `room[4]`, `room[5..=10]` = 0, `toggle` off, a block |
| `Sliding` | W until `room[10] & $1F = $1F`, then 0 | `room[0] = $80` when `toggle`, `toggle` flipped, the palette queued; the exit call's `room[0]` and flip are dead (`End` rewrites `room[0]`, `Block`/`HoldBlocks(2)` reset `toggle`) |
| `End` | W | `room[0] = 0`, ppu_ctrl `$A8`, the palette queued |
| `Blocks(0-1)` | W W | a block each |
| `Blocks(2)` | 0 | `count - 1`: `Block` again, `Settle` at 0 (six rounds) |
| `Settle` | W | `room[0] = 0` |
| `SettleBlock` | W | a block, `count` `$10` |
| `Hold` | 16 W, then 0 | the palette queued, `count - 1` |
| `HoldBlocks(0-1)` | W W | a block each |
| `HoldBlocks(2)` | 0 | `count` `$20`, `toggle` off, `flicker` `$C0` |
| `Flicker` | 79 W, then 0 | on even frames y `flicker`, mirroring 2, on odd y 0, mirroring 3; frames 0, 8, 16, 24 push the sprite palette of pointer 3, 2, 1, 0; `flicker` rises from frame 31 (`$C0` to `$EF` used); the palette queued; the exit call zeroes x, y and mirroring |
| `TitleBlocks(0-4)` | 5 W | a block each |
| `TitleBlocks(5)` | 0 | `check` cleared, then `Title::Start` in the frame |
| Title `Chosen` | W | music `$FF`, `count` 10 |
| `Flash(0-11)` | 12 W | phase 0 and 6 draw the line (cursor 8 or `$10` by the choice, 0 at 6), sound `$3B` at the first phase 0; the others queue the palette |
| `Flash(12)` | 0 | `count - 1`: `Flash(0)` again, at 0 the falling fade and `FadeOut` in the frame |

The new tables, every counter the runner's frame (`count`, `toggle` and `flicker` go):

- `SLIDE: [ArmSplit, 6 × [DrawSlider, SlideUntilSplit, EndSplit, Blocks(2)], SplitOff,
  Blocks(1), Hold(16), Blocks(2), Flicker, HomeScroll, Blocks(5)]`, then `Title::Start` in
  the frame. `ArmSplit` and `HomeScroll` take no frame; `SlideUntilSplit` checks
  `room[10]` first (Done at once on the split's end, as the cutscene's `SlideIn`) and sets
  `room[0]` by its frame's parity; `Flicker` is Done at frame 79 and `HomeScroll` zeroes
  the scroll. `check = flow == Waited` after every row: the same frames as today, with no
  clear at the end.
- `CHOSEN: [StopMusic, Flash(10)]`, then the falling fade and `Title::FadeOut` in the
  frame. `Flash(10)` runs 120 frames, the phase its frame mod 12, Done at frame 120.
