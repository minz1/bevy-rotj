# Faithful UI Implementation Plan

> **For agentic workers:** Implement task-by-task, in order. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `ui: Faithful` shows the game's own HUD, pause, title-cursor and password-entry
sprites as Bevy UI (the HUD label steady), with `Classic` untouched; then tag 1.1.0.

**Architecture:** The core's `OamBuilder` marks which drawer placed each OAM entry
(metadata beside the page, not RAM). The app copies the marks with the OAM, parks the
non-`Scene` entries in its display copy, and shows them as Bevy UI image nodes on tile
atlases built from the player's CHR.

**Tech Stack:** Rust, Bevy 0.20 `bevy_ui`, serde/ron (existing).

**Spec:** `docs/specs/2026-10-16-faithful-ui-design.md` (approved with changes by the Opus
stand-in, changes applied). This plan: approved with changes by the Opus stand-in (applied
below: the clone test clears the OAM, Task 2 checks the OAM against the state before each
step, the label carry-over lives in `Play::step`, sprites-off lines and the x range, the
8x16 table rule, the menu row index, fewer files).

## Global Constraints

- `Game`'s behaviour, RAM and WRAM are unchanged; `OamBuilder::lift`/`project` stay as
  they are (marks are not RAM). `.orchestration/gates.sh` ends `GATES GREEN` on every
  commit that is pushed.
- Public names: settings key `ui`, values `Classic` (default) and `Faithful`; menu row `UI`
  with `CLASSIC` / `FAITHFUL` (the game's font: A-Z, 0-9, space, `! ? - .`).
- No new dependencies, no lint suppressions beyond the existing
  `#[expect(clippy::needless_pass_by_value, reason = "Bevy system params, rust-clippy#8940")]`
  on Bevy systems; comments at most one line; nothing ROM-derived committed.
- One short commit subject, no trailer. Code review and test runs: the `reviewer` agent.

## Review Focus

1. A frame that places exactly 63 entries (`left` 0, so `finish` parks nothing) and then
   drops a piece at the edge: that entry's mark must be its last writer's (Task 1 test).
2. Faithful with the HUD hidden (`effects.hud & 0x80`, a boss room): no stale label carried
   over beyond one frame (Task 3 test).
3. Switching `ui` while paused or mid-password: the UI appears or the game's sprites return
   on the next frame, nothing left behind (Task 3 test).
4. Overscan shown vs hidden: a piece at OAM x 250 or y 4 crops exactly as the picture does
   (Task 3 test of positions plus the container's clip).
5. A palette change every frame (the pause's cycling colour): atlases do not accumulate
   (Task 3 test of the cache size).

---

### Task 1: Marks in the core

**Files:**
- Modify: `crates/rotj-core/src/actor/oam.rs` (Mark, `marks`, `marking`, `draw`, `finish`,
  `new`, `lift`)
- Modify: `crates/rotj-core/src/actor.rs:187` (Hud around `draw_hud`), `frame.rs:33-36`
  (Label around the label), `pause.rs:88` (Pause around `draw_screen`),
  `boot.rs` `draw_title` (Title around the cursor) and `draw_password` (Password around the
  entry/rejected draw only)

**Interfaces:**
- Produces: `#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)] pub enum Mark { #[default] Scene, Hud, Label, Pause, Title, Password }`;
  `OamBuilder { pub marks: [Mark; 64], pub marking: Mark, .. }`.

- [ ] **Step 1: Failing tests** in `oam.rs`:
  - `draw_marks_the_entries_it_writes`: a two-piece metasprite, the second piece at x past
    `width`, `marking = Hud`: the first entry is `Hud`; the dropped piece's entry (written
    but not advanced past) is `Hud` before `finish` and `Scene` after (parked).
  - `finish_keeps_drawn_marks_and_clears_parked_ones`: draw one piece as `Pause`, `finish`:
    that entry still `Pause`, every parked entry `Scene`.
  - `clear_makes_every_mark_scene`.
  - `a_full_page_keeps_its_last_writers_marks`: 63 pieces as `Pause`, then one dropped at
    the edge as `Hud` (`left` 0, `finish` parks nothing): that entry is `Hud`.
  With the ROM, in `actor.rs` tests (a level start, `room::start(..., 0)`), 4 loop frames:
  - `the_hud_marks_exactly_its_pieces`: the HUD is the loop frame's first draw. Each
    frame: `game.nmi(0); let mut c = game.clone(); c.oam.clear(); c.oam.marking =
    Mark::Hud; frame::draw_hud(&mut c, prg);` then the real `game.frame(..)`. The real
    frame's `Hud` entries equal the clone's `Hud` entries, and its `Label` entries the
    clone's `Label` entries, each as a multiset of `(y, tile, attr, x)`; `Label` entries
    exist on odd frames only.
  The pause card, title cursor and password entry are checked in Task 2's app sessions.
- [ ] **Step 2: Run** `cargo test -p rotj-core oam::` → compile errors.
- [ ] **Step 3: Implement.** `draw` sets `self.marks[entry >> 2] = self.marking` where it
  writes the entry's bytes (oam.rs:173). `finish`'s parking loop sets `Scene` on each entry it parks.
  `new` and `lift` start all `Scene`, `marking: Scene`. The callers set `marking` and reset
  it to `Scene` after (including `draw_hud`'s early returns: set it at the call site in
  `actor.rs`, and inside `draw_hud` only switch to `Label` and back to `Hud` around the
  label).
- [ ] **Step 4: Run** `cargo test -p rotj-core` and the release `--ignored` suite of
  rotj-core (the chain must be unchanged). Expected: PASS.
- [ ] **Step 5: Commit** `The OAM builder marks who drew each sprite`.

### Task 2: The chain's invariants for marked sprites

**Files:**
- Modify: `crates/rotj-core/src/actor/oracle.rs` (`Lockstep::compare_lines`, oracle.rs:894:
  the hook every lockstep frame passes through, with `before.oam` and
  `raster::lines(registers, game)`), `crates/app/src/play.rs` (`Play` gains
  `marks: [Mark; 64]`, copied with `oam` in `step` and set in `running`) and its tests

**Interfaces:**
- Produces: `fn assert_marked_drawable(oam: &OamBuilder, lines: &[Line])` (test helper in
  rotj-core, `pub(crate)` or `#[cfg(test)] pub`): for every entry with a non-`Scene` mark,
  `attr & 0x20 == 0`, `lines[y + 1].ctrl & 0x20 != 0`, and `lines[y + 1..y + 17]` share one
  `sprite_chr` (rows past 239 ignored), and `8 <= x <= 240` (clear of the black sides and
  of `$2001`'s left-8 clip, so Faithful matches Classic there).

- [ ] **Step 1:** Call it every lockstep frame (the chain counts the `Hud` and `Label`
  entries it saw and asserts both are above 0, so the invariants cannot pass on nothing),
  and every frame of three app sessions, checking `play.oam`/`play.marks`/`play.lines`
  against the screen state read *before* each `step` (the OAM `step` copies is the frame
  before it): power-on to the title and
  10 frames of cursor moves; title → PASSWORD → enter four letters and submit a wrong one;
  stage 1-1 start → Start (pause) → 30 frames → Start. In those sessions also: while
  paused every visible (y < `$EF`) entry is marked `Pause`; on the title every visible entry
  is `Title`; on the password screen the `Password` entries number 0, 6 or 9 on every frame
  and 6 and 9 each occur (a different count is a constant to correct from the ROM, not a
  redesign), every other visible entry `Scene`.
- [ ] **Step 2: Run** the release `--ignored` suites of rotj-core and the app. Expected:
  PASS (a broken drawability invariant means the spec's assumption broke for that element:
  stop and redesign it).
- [ ] **Step 3: Commit** `The marked sprites are drawable as UI on every frame`.

### Task 3: The UI in the app

**Files:**
- Create: `crates/app/src/ui.rs` (the overlay and its tile atlases)
- Modify: `settings.rs` (`ui` key, `UiStyle { Classic, Faithful }`, `Fields`), `settings-1.1.0.ron`
  (add `ui: Classic`), `menu.rs` (row `UI` at index 5, before `BACK`), `play.rs` (park in
  `compose` when Faithful; `label: Vec<[u8; 4]>`, the previous step's `Label` entries, kept
  in `step`; `oam`, `marks`, `label`, `lines` and the PPU palette `pub(crate)`), `main.rs`
  (plugin)

**Interfaces:**
- Consumes: Task 1's `Mark`, `OamBuilder::marks`.
- Produces: `ui::tiles(chr: &[u8], banks: [u8; 4], colours: [[u8; 3]; 4]) -> Image`
  (128 8x16 tiles of one 4 KiB pattern table, 16 x 8 grid, colour 0 transparent);
  `pub enum UiStyle { Classic, Faithful }` (`#[default] Classic`); `Play::compose(&self,
  frame, sprites_per_line, park: bool)`; `ui::Shown { y, tile, attr, x, banks: [u8; 4],
  colours: [[u8; 3]; 4] }` and `fn shown(oam: &[u8; 256], marks: &[Mark; 64], label:
  &[[u8; 4]], lines: &[Line], palette: &[u8; 32], rgb: &Rgb) -> Vec<Shown>`: the non-Scene
  visible entries plus the carried label when this step drew none, skipping entries whose
  line `lines[y + 1].mask & 0x10 == 0` (sprites off). In 8x16 mode the table is `tile & 1`,
  the banks `sprite_chr[4 * (tile & 1)..][..4]`, the cell `tile >> 1`, the colours
  `palette[0x10 + 4 * (attr & 3) + v]` (`& 0x30` when `mask & 1`).

- [ ] **Step 1: Failing tests:**
  - settings: `(ui: Faithful)` loads; the fixture has `ui: Classic`; an unknown `ui: Modern`
    keeps the rest.
  - menu: the `UI` row cycles; every value string has glyphs.
  - play: `classic_keeps_the_games_oam` (park false: the display copy equals the game's);
    `faithful_parks_exactly_the_marked_entries` (a stage frame: non-Scene entries' y is
    `$F4` in the copy, the rest equal).
  - play: `the_label_carries_over_one_step` (`Play::step` keeps the last step's `Label`
    entries for one step, then drops them).
  - ui: `shown_skips_lines_with_sprites_off`; `positions_follow_the_visible_area` (x 250
    stays 250 in both modes; OAM y 4 → -3 with Hidden, clipped above);
    `the_atlas_cache_keeps_only_this_frames_keys`.
  - atlas: a known tile pair's pixels from a synthetic CHR (no ROM): value 0 transparent,
    1-3 the three colours, the bottom half from tile `top | 1`.
- [ ] **Step 2: Run** `cargo test -p batman-rotj` → FAIL.
- [ ] **Step 3: Implement.** `step` copies `marks` with `oam`. `compose` parks non-Scene
  entries in a copy when `park`. `ui.rs`: a system after `draw` (`Step::Picture`) that,
  when `Settings::ui == Faithful` and the shown entries/lines/colours changed, despawns the
  overlay and spawns, inside a full-window centring root and a `visible`-sized NES-pixel
  container with `Overflow::clip()` and a `GlobalZIndex` below the menu's, one absolutely
  positioned 8x16 `ImageNode` per entry on its atlas (flips from attr bits 6-7). The label
  carry-over is `Play::label`, so extra updates without a tick change nothing. The UI root
  gets `GlobalZIndex(-1)`, below the menu (which has none).
- [ ] **Step 4: Run** `cargo test -p batman-rotj`; clippy. Expected: PASS.
- [ ] **Step 5: Commit** `Faithful UI: the game's own UI sprites as Bevy UI`.

### Task 4: Real output, review, docs, 1.1.0

- [ ] **Step 1: Captures** on the owner's machine (Windows exe, `capture.ps1`, sprite limit
  off via `settings.ron`): stage 1-1 odd and even ticks, the pause card, the title, the
  password entry, each in `Classic` and `Faithful`; compare in Python: identical except the
  even tick's label, which `Faithful` shows.
- [ ] **Step 2: Gates**, then the `reviewer` agent on the branch since `43fbd20`; fix
  findings; gates again.
- [ ] **Step 3: Docs:** STATUS (a 1.1.0 row replacing "1.1.0, spec 1": both specs and their
  evidence), PLAN (1.1.0 tagged), README (the `UI` setting).
- [ ] **Step 4: Version 1.1.0:** workspace `version`, every internal dependency's
  `version = "1.0.5"` to `"1.1.0"` (19 lines, `tools/*` included), `Cargo.lock` (as the
  1.0.5 release commit did: `git show 8e9fd31 --stat`). Commit `Release 1.1.0`.
- [ ] **Step 5: Release checks:** gates on the release commit; a fresh clone without a ROM
  in the scratchpad (clippy, `cargo test --workspace`, `cargo build -p batman-rotj`, the exe
  exits 2 with "no usable ROM found", boots from `ROTJ_ROM`, the first argument and a ROM
  beside it); then `git tag 1.1.0`, `git push origin main`, `git push origin 1.1.0`; watch
  the release workflow until both ROM-less exes are attached.
- [ ] **Step 6:** The Windows exe in `Documents\batman-rotj` rebuilt from the tag.
