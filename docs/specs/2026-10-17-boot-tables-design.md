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
