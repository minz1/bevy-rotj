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
stand-in, changes applied).

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

1. An OAM page that overflows on an even frame (`finish` parks nothing): the marks of the
   entries left from the previous frame must still match their bytes (Task 1 test).
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
  - `draw_marks_what_it_places_and_not_what_it_drops`: a two-piece metasprite with one
    piece at x past `width`, `marking = Hud`: the placed entry (`next / 4` before the
    draw) is `Hud`, the entry after it `Scene`.
  - `finish_keeps_drawn_marks_and_clears_parked_ones`: draw one piece as `Pause`, `finish`:
    that entry still `Pause`, every parked entry `Scene`.
  - `clear_makes_every_mark_scene`.
  - `an_overflowed_page_keeps_its_marks`: fill the page past 63 entries on an even frame
    as `Hud`; after `finish` (which parks nothing) every entry is still `Hud`.
  With the ROM, in `actor.rs` tests (a level start, `room::start(..., 0)`), 4 loop frames:
  - `the_hud_marks_exactly_its_pieces`: the HUD is the loop frame's first draw, so a clone
    of the game taken before `Game::frame`, given `marking = Hud` and run through
    `frame::draw_hud` alone, places the same entries; the real frame's `Hud`/`Label`
    entries equal the clone's non-`Scene` entries as a multiset of `(y, tile, attr, x)`,
    with `Label` present on odd frames only.
  The pause card, title cursor and password entry are checked in Task 2's app sessions.
- [ ] **Step 2: Run** `cargo test -p rotj-core oam::` → compile errors.
- [ ] **Step 3: Implement.** `draw` sets `self.marks[usize::from(next >> 2)] = self.marking`
  next to the `next` advance. `finish`'s parking loop sets `Scene` on each entry it parks.
  `new` and `lift` start all `Scene`, `marking: Scene`. The callers set `marking` and reset
  it to `Scene` after (including `draw_hud`'s early returns: set it at the call site in
  `actor.rs`, and inside `draw_hud` only switch to `Label` and back to `Hud` around the
  label).
- [ ] **Step 4: Run** `cargo test -p rotj-core` and the release `--ignored` suite of
  rotj-core (the chain must be unchanged). Expected: PASS.
- [ ] **Step 5: Commit** `The OAM builder marks who drew each sprite`.

### Task 2: The chain's invariants for marked sprites

**Files:**
- Modify: the release chain test in `crates/rotj-core/src/actor.rs` tests (the TAS chain
  loop) and the app's scripted sessions in `crates/app/src/play.rs` tests

**Interfaces:**
- Produces: `fn assert_marked_drawable(oam: &OamBuilder, lines: &[Line])` (test helper in
  rotj-core, `pub(crate)` or `#[cfg(test)] pub`): for every entry with a non-`Scene` mark,
  `attr & 0x20 == 0`, `lines[y + 1].ctrl & 0x20 != 0`, and `lines[y + 1..y + 17]` share one
  `sprite_chr` (rows past 239 ignored).

- [ ] **Step 1:** Call it every frame of the TAS chain with the frame's raster lines
  (`raster::lines`), and every frame of three app sessions: power-on to the title and
  10 frames of cursor moves; title → PASSWORD → enter four letters and submit a wrong one;
  stage 1-1 start → Start (pause) → 30 frames → Start. In those sessions also: while
  paused every visible (y < `$EF`) entry is marked `Pause`; on the title every visible entry
  is `Title` (the cursor's 2 pieces); on the password screen the `Password` entries number
  9 while the letters show, 6 for the rejection, 0 otherwise, and every other visible entry
  is `Scene`.
- [ ] **Step 2: Run** the release `--ignored` suites of rotj-core and the app. Expected:
  PASS (if not, the spec's assumption broke: stop and redesign that element).
- [ ] **Step 3: Commit** `The marked sprites are drawable as UI on every frame`.

### Task 3: The UI in the app

**Files:**
- Create: `crates/app/src/ui.rs` (the overlay), `crates/app/src/atlas.rs` (CHR to tile
  atlas, shared with the GPU renderer later)
- Modify: `settings.rs` (`ui` key, `UiStyle { Classic, Faithful }`, `Fields`), `settings-1.1.0.ron`
  (add `ui: Classic`), `menu.rs` (row `UI`), `play.rs` (copy marks in `step`; park in
  `compose` when Faithful; expose `shown_marks()`, `shown_oam()`, `lines()`, `palette()`),
  `main.rs` (plugin)

**Interfaces:**
- Consumes: Task 1's `Mark`, `OamBuilder::marks`.
- Produces: `atlas::tiles(chr: &[u8], banks: [u8; 4], colours: [[u8; 3]; 4]) -> Image`
  (128 8x16 tiles of one 4 KiB pattern table, 16 x 8 grid, colour 0 transparent);
  `pub enum UiStyle { Classic, Faithful }` (`#[default] Classic`); `Play::compose(&self,
  frame, sprites_per_line, park: bool)`; `ui::Shown` (the entries to show: `(entry, y,
  tile, attr, x, mark)` plus their line's banks and colours) and
  `fn shown(oam, marks, lines, palette, rgb, label_last_frame: bool) -> Vec<Piece>`.

- [ ] **Step 1: Failing tests:**
  - settings: `(ui: Faithful)` loads; the fixture has `ui: Classic`; an unknown `ui: Modern`
    keeps the rest.
  - menu: the `UI` row cycles; every value string has glyphs.
  - play: `classic_keeps_the_games_oam` (park false: the display copy equals the game's);
    `faithful_parks_exactly_the_marked_entries` (a stage frame: non-Scene entries' y is
    `$F4` in the copy, the rest equal).
  - ui: `the_label_carries_over_one_frame` (shown on the frame after it was drawn, not two
    frames after); `positions_follow_the_visible_area` (x 250 → 242 with Hidden, OAM y 4 →
    -3 with Hidden, i.e. clipped above); `the_atlas_cache_keeps_only_this_frames_keys`.
  - atlas: a known tile pair's pixels from a synthetic CHR (no ROM): value 0 transparent,
    1-3 the three colours, the bottom half from tile `top | 1`.
- [ ] **Step 2: Run** `cargo test -p batman-rotj` → FAIL.
- [ ] **Step 3: Implement.** `step` copies `marks` with `oam`. `compose` parks non-Scene
  entries in a copy when `park`. `ui.rs`: a system after `draw` (`Step::Picture`) that,
  when `Settings::ui == Faithful` and the shown entries/lines/colours changed, despawns the
  overlay and spawns, inside a full-window centring root and a `visible`-sized NES-pixel
  container with `Overflow::clip()` and a `GlobalZIndex` below the menu's, one absolutely
  positioned 8x16 `ImageNode` per entry on its atlas (flips from attr bits 6-7). The label
  carry-over keeps the previous shown frame's `Label` entries for one frame.
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
  `version = "1.0.5"` to `"1.1.0"`, `Cargo.lock`; the in-game version string if any (as the
  1.0.5 release commit did: `git show 8e9fd31 --stat`). Commit `Release 1.1.0`.
- [ ] **Step 5: Release checks:** gates on the release commit; a fresh clone without a ROM
  in the scratchpad (clippy, `cargo test --workspace`, `cargo build -p batman-rotj`, the exe
  exits 2 with "no usable ROM found", boots from `ROTJ_ROM`, the first argument and a ROM
  beside it); then `git tag 1.1.0`, `git push origin main`, `git push origin 1.1.0`; watch
  the release workflow until both ROM-less exes are attached.
- [ ] **Step 6:** The Windows exe in `Documents\batman-rotj` rebuilt from the tag.
