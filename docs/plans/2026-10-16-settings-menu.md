# Settings, Settings Menu and Frame Pacing Implementation Plan

> **For agentic workers:** Implement task-by-task, in order. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A saved `settings.ron`, an NES-styled settings menu, five presentation settings
(volume, overscan, palette, sprite limit, vsync) and one picture per 60 Hz tick.

**Architecture:** Everything lives in the app crate except one parameter on
`rotj_core::render::Frame::render` and three palette tables in `rotj_core::palette`. The
tick moves from `FixedUpdate` to `Update` behind a frame limiter in `First`. The menu is
Bevy UI drawn with the game's own font, assembled from the player's CHR at start-up.

**Tech Stack:** Rust (workspace edition), Bevy 0.20 (`bevy_ui`, `bevy_audio`), serde + ron
0.12 (already dependencies).

**Spec:** `docs/specs/2026-10-16-settings-menu-design.md` (approved 2026-10-16).

## Global Constraints

- Every setting is presentation: nothing here may change `Game`, its RAM/WRAM, or what the
  Classic suites see. `.orchestration/gates.sh` must end `GATES GREEN` on every commit.
- Saved names are public interface: `volume` (0-100, steps of 5), `overscan`
  (`Hidden` | `Shown`), `palette` (`CompositeDirect` | `RgbPpu` | `Ntsc` | `File`),
  `sprite_limit` (bool), `vsync` (bool). Defaults: `100`, `Hidden`, `CompositeDirect`,
  `true`, `true`.
- `settings.ron` and `palette.pal` live beside the exe (`std::env::current_exe()`'s parent).
- No new dependencies. No lint suppressions (only the existing
  `#[expect(clippy::needless_pass_by_value, reason = "Bevy system params, rust-clippy#8940")]`
  pattern on Bevy systems). Code comments: default none, at most one line.
- Nothing ROM-derived committed (`tools/check-no-rom-data.py` guards it). The font atlas is
  built at runtime from `RomData`.
- Menu text uses only A-Z, 0-9, space and `! ? - .` (the game's font).
- `desired_maximum_frame_latency` stays `NonZeroU32::new(1)`.
- Commits: one short subject line, no trailer. Code review and test runs go to the
  `reviewer` agent.

## Review Focus

1. A hand-edited `settings.ron` with `volume: 250` or `volume: 7`: clamped into 0-100 on
   load; Left/Right then steps by 5 and stops at 0 and 100 (Task 2 test).
2. Esc or Guide held down: the menu toggles once per press, never every frame (Task 7 test).
3. The smallest window (whole multiple 1): the menu box fits inside 256x224 NES pixels
   (Task 7 test on the box's size).
4. `palette.pal` added, replaced or deleted while the game runs: choosing `File` re-reads it
   (Task 3 test).
5. The window dragged or the machine stalled for seconds: the limiter resets its deadline
   and never runs a burst of catch-up ticks (Task 4 test).

---

### Task 1: Core: the per-line sprite limit and the palettes

**Files:**
- Modify: `crates/rotj-core/src/render.rs` (`Frame::render`, `sprite_line`, tests' `Scene::render`)
- Modify: `crates/rotj-core/src/palette.rs`
- Modify: `crates/app/src/play.rs:146` and `:1021` (callers pass `MAX_SPRITES_PER_LINE` for now)

**Interfaces:**
- Produces: `Frame::render(&mut self, picture: &Picture<'_>, sprites_per_line: usize)`;
  `pub type Rgb = [[u8; 3]; 64]`; `palette::RGB_PPU: Rgb`; `palette::ntsc() -> Rgb`;
  `palette::from_pal(bytes: &[u8]) -> Option<Rgb>`; `NES_PALETTE: Rgb` (unchanged values).

- [ ] **Step 1: Write the failing tests** in `render.rs` tests:
  `nine_sprites_on_a_line_draw_eight_with_the_limit_and_nine_without`: nine 8x16 sprites
  side by side at x = 0, 8, …, 64 on y = 10, opaque pixels; with
  `MAX_SPRITES_PER_LINE` the pixel at x = 64 on line 12 is the background's, with 64 it is
  the ninth sprite's colour.
  In `palette.rs` tests:
  - `rgb_ppu_is_the_2c03_levels`: every channel of every entry is one of the eight 3-bit
    levels `[0x00, 0x24, 0x49, 0x6D, 0x92, 0xB6, 0xDB, 0xFF]`; `RGB_PPU[0x30] == [0xFF; 3]`;
    `RGB_PPU[0x0F] == [0; 3]`.
  - `ntsc_greys_are_neutral_and_ordered`: for `0x00, 0x10, 0x20, 0x30` R == G == B (±1),
    strictly increasing; `ntsc()[0x0F] == [0, 0, 0]`; no entry brighter than `0x30`'s sum.
  - `ntsc_is_pinned`: `fnv1a(ntsc().as_flattened())` equals the value printed on the first
    run (pin it; a later change must be deliberate).
  - `pal_files_are_192_or_1536_bytes`: `from_pal(&[0; 192])` and `from_pal(&[0; 1536])` are
    `Some`; 191, 193, 0 and 1535 bytes are `None`; the 1536-byte case returns its first 192.
- [ ] **Step 2: Run** `cargo test -p rotj-core render:: palette::`. Expected: compile errors / FAIL.
- [ ] **Step 3: Implement.** `sprites_per_line` replaces the `.take(MAX_SPRITES_PER_LINE)`.
  `RGB_PPU`: the 2C03 table from the NESdev wiki's "PPU palettes" 2C03/2C05 entry, each
  3-bit level `n` written as `n * 255 / 7` rounded (the eight values above); cite the page
  in a one-line doc comment. `ntsc()`: for each colour `(hue = i & 15, level = i >> 4)`,
  sample the 2C02's square wave at 12 phases with the terminated signal levels and the
  phase convention of the NESdev wiki "NTSC video" page (fetch it in this step; do not
  use levels from memory), demodulate Y, U and V with the page's carrier phase and
  convert with its YUV matrix, clamp to 0-1 and round to bytes, with no gamma step
  (the signal is already gamma-encoded for a TV, as sRGB is). Cite the page in a one-line
  doc comment. `from_pal`: length
  check, chunk the first 192 bytes.
- [ ] **Step 4: Run** `cargo test -p rotj-core` and `cargo test -p batman-rotj`. Expected: PASS
  (all existing picture tests unchanged, so the parameter preserves Classic).
- [ ] **Step 5: Commit** `Frame::render takes the sprite limit; RGB PPU and NTSC palettes`.

### Task 2: The settings file

**Files:**
- Create: `crates/app/src/settings.rs`, `crates/app/settings-1.1.0.ron`
- Modify: `crates/app/src/main.rs` (`mod settings;`, insert the resource at start-up)

**Interfaces:**
- Produces: `#[derive(Resource, Serialize, Deserialize, Clone, Debug, PartialEq)] pub struct Settings { pub volume: u8, pub overscan: Overscan, pub palette: PaletteChoice, pub sprite_limit: bool, pub vsync: bool }` with `#[serde(default)]` on the struct and `Default` giving the spec's defaults;
  `pub enum Overscan { Hidden, Shown }`; `pub enum PaletteChoice { CompositeDirect, RgbPpu, Ntsc, File }`;
  `pub fn load(dir: &Path) -> Settings`; `pub fn save(dir: &Path, settings: &Settings) -> std::io::Result<()>`;
  `#[derive(Resource)] pub struct SettingsDir(pub Option<PathBuf>)` (`None` when the exe's
  directory is unknown); `pub fn exe_dir() -> Option<PathBuf>`.

- [ ] **Step 1: Write the failing tests** (in `settings.rs`, each in its own temp directory
  under `std::env::temp_dir()` named by the test):
  - `no_file_is_the_defaults`: `load(empty dir) == Settings::default()`, and nothing is created.
  - `settings_round_trip`: a non-default `Settings` saved then loaded is equal; no
    `settings.ron.tmp` is left.
  - `a_missing_key_is_its_default`: `"(volume: 40)"` loads as defaults with `volume: 40`.
  - `an_unknown_key_is_ignored`: `"(volume: 40, frame_rate: Display)"` loads with `volume: 40`.
  - `a_bad_file_is_kept_as_bak`: `"(volume: "` loads defaults, and `settings.ron.bak` holds
    the bad text.
  - `volume_is_clamped`: `"(volume: 250)"` loads `volume: 100`.
  - `the_1_1_0_file_parses`: `ron::from_str::<Settings>(include_str!("../settings-1.1.0.ron"))`
    is `Ok` and equals the fixture's values.
  - `an_unwritable_dir_is_an_error_not_a_panic`: `save` to a path under a regular file
    returns `Err`.
- [ ] **Step 2: Run** `cargo test -p batman-rotj settings::`. Expected: FAIL (module missing).
- [ ] **Step 3: Implement.** `load`: read `settings.ron`; `NotFound` gives defaults; a parse
  error warns, renames the file to `settings.ron.bak` and gives defaults; clamp `volume` to
  100. `save`: `ron::ser::to_string_pretty` to `settings.ron.tmp`, then `fs::rename`. The
  fixture holds the spec's default block with every key written out. At start-up `main`
  inserts `SettingsDir(exe_dir())` and `load`'s result (defaults when the dir is `None`).
- [ ] **Step 4: Run** `cargo test -p batman-rotj settings::`. Expected: PASS.
- [ ] **Step 5: Commit** `Settings file beside the exe`.

### Task 3: The picture follows the settings (palette, overscan, sprite limit, vsync)

**Files:**
- Modify: `crates/app/src/screen.rs` (upload through a colour table, crop)
- Modify: `crates/app/src/main.rs` (`integer_view` takes the visible line count; `fit_camera`
  reruns when `Settings` changes; vsync)
- Modify: `crates/app/src/play.rs` (`draw` passes the limit)

**Interfaces:**
- Consumes: Task 1's `Rgb`, `RGB_PPU`, `ntsc()`, `from_pal`, `Frame::render(…, usize)`;
  Task 2's `Settings`, `SettingsDir`.
- Produces: `#[derive(Resource)] pub struct Colours { pub rgb: Rgb, pub file_missing: bool }`
  in `screen.rs`; `pub fn colours(choice: PaletteChoice, dir: Option<&Path>) -> Colours`
  (`File` reads `<dir>/palette.pal` each call, Composite Direct and `file_missing: true`
  when absent or the wrong size); `pub fn rgba_into(indices: &[u8], rgb: &Rgb, out: &mut [u8])`;
  `fn integer_view(width: f32, height: f32, lines: f32) -> (f32, Vec2)`;
  `Play::compose(&self, frame: &mut Frame, sprites_per_line: usize)`;
  `pub const fn visible_lines(overscan: Overscan) -> (usize, usize)` returning
  `(first, count)`: `(8, 224)` hidden, `(0, 240)` shown.

- [ ] **Step 1: Write the failing tests:**
  - `screen::tests::composite_direct_upload_is_the_classic_frame` (with the ROM): power on,
    step 400 frames, compose with 8; `rgba_into(frame.indices(), &NES_PALETTE, …)` equals
    `frame.rgba()` byte for byte.
  - `screen::tests::the_file_palette_is_reread_and_falls_back`: in a temp dir, `colours(File)`
    with no file is `NES_PALETTE` and `file_missing`; after writing a 192-byte file of `0x11`
    it is that table and not missing; after deleting it, missing again.
  - `main::tests::the_picture_scales_by_whole_multiples_on_whole_pixels` gains 224-line
    cases: `integer_view(1280.0, 1000.0, 224.0) == (0.8, Vec2::ZERO)` (multiple 5, where
    240 lines give 4) and the existing four cases with `240.0`.
- [ ] **Step 2: Run** `cargo test -p batman-rotj screen:: main::`. Expected: FAIL.
- [ ] **Step 3: Implement.** `upload_frame` maps indices through `Colours` (reuse the
  image's buffer, no allocation). The quad's `Sprite::rect` shows rows
  `first..first + count` and its `custom_size` height is `count * 4`. A system on
  `Settings` change (`resource_changed::<Settings>`) recomputes `Colours`, the quad's rect
  and size, the camera fit (`fit_camera` also runs when `Settings` changed), and sets the
  primary window's `present_mode` to `AutoVsync` / `AutoNoVsync`; `draw` passes `8` or
  `64` from `sprite_limit`.
- [ ] **Step 4: Run** `cargo test -p batman-rotj`, then `cargo run --release -p batman-rotj`
  on WSL long enough to see the title with 224 lines (llvmpipe is fine for this). Expected:
  PASS; the title's top strip is gone.
- [ ] **Step 5: Commit** `The picture follows the settings`.

### Task 4: One picture per tick

**Files:**
- Create: `crates/app/src/pacing.rs`
- Modify: `crates/app/src/main.rs` (drop `Time::<Fixed>`, add the plugin), `play.rs`
  (`tick` to `Update`), `audio.rs` (`advance_music` to `Update`, after `tick`)

**Interfaces:**
- Produces: `pub const PERIOD: Duration` (1/60 s); `pub fn next_deadline(deadline: Instant, now: Instant, period: Duration) -> Instant`;
  `#[derive(SystemSet)] pub struct Tick` (the set `tick` and `advance_music` run in,
  ordered `tick` before `advance_music`); `pub struct PacingPlugin` (limiter in `First`,
  the frame log).

- [ ] **Step 1: Write the failing tests** in `pacing.rs`:
  - `on_time_advances_by_a_period`: deadline `t`, now `t - 3 ms` gives `t + PERIOD`.
  - `a_little_late_keeps_the_rhythm`: now `t + 5 ms` gives `t + PERIOD`.
  - `a_stall_resets_instead_of_bursting`: now `t + 3 s` gives `now + PERIOD` (one tick,
    not 180).
  - `#[ignore] real_time_frames_average_a_sixtieth` : run the limiter's wait 600 times;
    mean interval within 1% of 16.667 ms; print the 99th-percentile wake error.
- [ ] **Step 2: Run** `cargo test -p batman-rotj pacing::`. Expected: FAIL.
- [ ] **Step 3: Implement.** `next_deadline`: `deadline + period` unless `now` is past it by
  more than one period, then `now + period`. The limiter waits for the current deadline
  (`thread::sleep` to 1 ms short, then `std::hint::spin_loop` until `Instant::now() >=
  deadline`), then stores `next_deadline`. Runs in `First` before input sampling.
  `ROTJ_FRAME_LOG=1`: every 300 frames `info!` the interval mean, standard deviation and
  maximum. Move `tick` and `advance_music` to `Update` in `Tick`.
- [ ] **Step 4: Run** `cargo test -p batman-rotj`, `cargo test --release -p batman-rotj pacing -- --ignored --nocapture`,
  and the release `--ignored` suite of `batman-rotj` (the boot and attract picture tests
  drive `Play` directly and must not change). Expected: PASS.
- [ ] **Step 5: Commit** `One picture per 60 Hz tick`.

### Task 5: Volume and the silent menu

**Files:**
- Modify: `crates/app/src/audio.rs`

**Interfaces:**
- Consumes: Task 2's `Settings::volume`.
- Produces: `Music::advance_frame(&mut self, run_driver: bool, gain: f32)`: renders the
  frame's samples, ticking the driver only when `run_driver`, ramping the applied gain
  toward `gain` by at most `1.0 / 441.0` per sample; `pub fn gain(volume: u8) -> f32`
  (`(volume / 100)²`); `#[derive(Resource, Default)] pub struct MenuOpen(pub bool)` lives
  here or in `menu.rs` (Task 7 decides; `advance_music` reads it: `run_driver = !open`,
  target `0.0` while open).

- [ ] **Step 1: Write the failing tests** (with the ROM, a `Music` fed the title song's
  request, run 60 frames at full gain first):
  - `the_menu_fades_out_and_back_in_without_a_step`: then 10 frames with
    `run_driver: false, gain: 0.0`, then 10 with `true, 1.0`; across the whole pushed
    sequence no consecutive samples differ by more than `max(|music's largest step in the
    first 60 frames|, last level / 441)`; the samples from 441 after the open to the close
    are exactly 0.0.
  - `gain_is_squared`: `gain(100) == 1.0`, `gain(50) == 0.25`, `gain(0) == 0.0`.
  - `the_driver_does_not_run_while_open`: a clone of the driver (`rotj_music::Music` is
    `Clone`) taken at the open and the driver after the 10 open frames return the same
    writes from their next `tick()`.
- [ ] **Step 2: Run** `cargo test -p batman-rotj audio::`. Expected: FAIL.
- [ ] **Step 3: Implement** the ramp per sample after `stream.fill`; `advance_music` passes
  `!MenuOpen` and `gain(settings.volume)` (0.0 while open).
- [ ] **Step 4: Run** `cargo test -p batman-rotj`. Expected: PASS.
- [ ] **Step 5: Commit** `Master volume, and the menu fades the sound`.

### Task 6: Held buttons hidden from the game

**Files:**
- Modify: `crates/app/src/input.rs`, `crates/app/src/play.rs` (`tick` uses the masked byte;
  `Play::paused`)

**Interfaces:**
- Produces: `NesInput::hide_held(&mut self)` (hides every button held now until each is
  released); `NesInput::game_byte(&self) -> u8` (held minus hidden, in the game's order;
  `tick` uses it instead of `buttons.game_byte()`); `Play::paused(&self) -> bool`
  (`matches!(self.game.script, Some(Script::Pause(_)))`).

- [ ] **Step 1: Write the failing tests** in `input.rs`:
  - `hidden_buttons_stay_hidden_until_released`: hold A+Right, `hide_held`, update with A+Right
    still held: `game_byte() == 0`; release A (Right held): still 0; press A again: `0x80`.
  - In `play.rs`: `paused_follows_the_games_pause` (with the ROM): `Play::start(…, 0)`,
    step with Start (`0x10`) once and a few frames of 0: `paused()`; Start again: not.
- [ ] **Step 2: Run** `cargo test -p batman-rotj input:: play::tests::paused`. Expected: FAIL.
- [ ] **Step 3: Implement.** `hidden: u8` on `NesInput`; `update` clears hidden bits that
  are released.
- [ ] **Step 4: Run** `cargo test -p batman-rotj`. Expected: PASS.
- [ ] **Step 5: Commit** `Buttons held when the menu closes stay hidden from the game`.

### Task 7: The menu

**Files:**
- Create: `crates/app/src/menu.rs`
- Modify: `crates/app/src/main.rs` (plugin, `UiScale` from the view's multiple),
  `play.rs` (`tick` does not run while open)

**Interfaces:**
- Consumes: Tasks 2, 3 (`Colours`, `visible_lines`), 5 (`MenuOpen`), 6 (`hide_held`,
  `paused`), `RomData`.
- Produces: `pub struct MenuPlugin`; `pub fn glyph(c: char) -> Option<u8>` (the top tile of
  the pair in bank `$CC`; `' '` is `0x00`); `pub fn font_atlas(chr: &[u8], rgb: &Rgb) -> Image`
  (each glyph 8x16, pixel value 0 transparent, 1 `$0F`, 2 and 3 `$30`, as the sound test's
  palette `0F 0F 30 30` draws it); `#[derive(Resource)] struct Menu { open: bool, row: usize }`;
  `const ROWS: [&str; 6] = ["VOLUME", "OVERSCAN", "PALETTE", "SPRITE LIMIT", "VSYNC", "BACK"]`;
  `fn value(settings: &Settings, colours: &Colours, row: usize) -> String` giving `100`,
  `HIDE`/`SHOW`, `COMPOSITE`/`RGB PPU`/`NTSC`/`FILE`/`NO FILE`, `ON`/`OFF`.

- [ ] **Step 1: Write the failing tests:**
  - `sound_test_text_maps_to_its_tiles`: `"SOUND TEST".chars().map(glyph)` equals
    `0C 0A 2C 38 14 00 1C 24 0C 1C`.
  - `every_menu_character_has_a_glyph`: every char of `ROWS`, `"SETTINGS"`, every `value`
    string and `"SELECT  SETTINGS"` has `Some`.
  - `the_box_fits_the_smallest_view`: the menu's box size in NES pixels is at most
    256 x 224.
  - Headless app tests (`MinimalPlugins`, the app's input, play, menu and audio plugins,
    `RomData` from `for_test`, systems driven by `app.update()` with `ButtonInput<KeyCode>`
    pressed and released by hand):
    - `esc_opens_and_closes_once_per_press`: press Esc and hold for 5 updates: open after
      the first, still open after the fifth; release, press again: closed.
    - `the_game_does_not_step_while_open`: the NMI frame counter
      (`Play::game().world.frame`, `$F1`) is unchanged over 30 open updates and moves
      after closing.
    - `right_changes_the_setting_and_closing_saves`: open, Down (to OVERSCAN), Right:
      `Settings::overscan == Shown`; close: the temp dir's `settings.ron` has `Shown`.
    - `select_opens_it_only_in_the_games_pause`: Select in gameplay does not open; after
      Start pauses the game, Select opens it.
    - `the_button_that_closed_it_is_hidden`: close with B held: the next tick's pad has no B.
- [ ] **Step 2: Run** `cargo test -p batman-rotj menu::`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - Toggle on `just_pressed` of Esc, F1, any pad's `Mode` or `RightThumb`, Select (only
    when `Play::paused()`), and B or A/Enter on BACK when open. On close: `hide_held`, save
    through `SettingsDir` (warn once if it fails), `MenuOpen(false)`.
  - Navigation on `NesInput::pressed`: Up/Down wrap over `ROWS`, Left/Right change the row's
    setting (volume ±5 clamped; the others cycle).
  - UI: a full-window root node centring a box with a 1-glyph border, 8x16 cells,
    `SETTINGS` on the first line; the font has no arrow, so the cursor row is drawn white
    (`$30`) and the other rows grey (`$10`, an `ImageNode::color` tint on the same atlas),
    values right-aligned at a fixed column. Each line is a row of `ImageNode`s on the font
    atlas; rebuild the box's children when `Menu` or `Settings` change. Background `$0F`
    from `Colours`. `UiScale` = the multiple from `integer_view` (set in `fit_camera`).
  - Pause hint: while `Play::paused()` and the menu is closed, `SELECT  SETTINGS` centred on
    NES row 200 (the pause draws its area name at `$BF`; check against a capture in Task 8
    and move it if it overlaps).
  - The atlas is rebuilt when `Colours` changes.
- [ ] **Step 4: Run** `cargo test -p batman-rotj`, then the app on WSL: Esc opens the menu,
  each row changes live, Esc closes. Expected: PASS.
- [ ] **Step 5: Commit** `The settings menu`.

### Task 8: Real output, docs, release of the branch

**Files:**
- Modify: `docs/STATUS.md` (a 1.1 row: what landed, the measurements, the sprite-limit
  review), `docs/PLAN.md` (1.1.0 = this spec + spec 2; the ladder renumbered: interpolation
  and the compositor, widescreen, CRT filters as later 1.x minors), `README.md` (the
  settings, Esc/F1/Guide, `settings.ron` and `palette.pal` beside the exe)

- [ ] **Step 1: The sprite-limit review.** An `--ignored` throwaway (not committed) over the
  oracle's TAS frames counts frames with any line of more than 8 sprites; render 10 of them
  spread over the stages with 8 and with 64, save both PNGs to the scratchpad, look at each
  difference for deliberate masking. Record the count and the finding in STATUS.
- [ ] **Step 2: Gates.** `bash .orchestration/gates.sh`. Expected: last line `GATES GREEN`;
  read `.orchestration/logs/` for counts (687 + this plan's new tests).
- [ ] **Step 3: Reviewer.** The `reviewer` agent on `8e9fd31..HEAD` (correctness, the Classic
  constraints, the public names, ponytail pass). Fix its findings; gates again.
- [ ] **Step 4: Windows exe.** Build and strip into `C:\Users\emery\Documents\batman-rotj\`
  (HANDOFF commands); `df /mnt/c` first.
- [ ] **Step 5: Real output on the owner's machine** (the exe launched from WSL through
  interop):
  - `ROTJ_FRAME_LOG=1`: the logged interval is 16.67 ms ± 0.5 (mean), report the standard
    deviation and worst case.
  - A PowerShell capture of the window
    (`[System.Windows.Forms.Screen]` + `Graphics.CopyFromScreen` to a PNG in the
    scratchpad), measured in Python: picture height = 224 × the multiple (and 240 × after
    switching to SHOW), NES pixel runs all the multiple wide, sampled RGB equals the chosen
    table, the menu's glyph pixels on the same grid; the pause hint clear of the pause's
    sprites.
- [ ] **Step 6: Commit and push main** once the gates are green on that exact commit
  (`git push origin main`; no tag: 1.1.0 is tagged after spec 2).
- [ ] **Step 7: Owner's playtest** on the 170 Hz G-Sync display: judder, the menu by keyboard
  and pad, volume steps. Their findings start the next round.
