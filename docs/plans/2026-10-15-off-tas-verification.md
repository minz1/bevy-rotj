# Off-TAS Verification Implementation Plan

> **Reviewed:** approved with changes by a stand-in reviewer (2026-10-15); changes applied (the pad hook moved to every NMI wait, the LFSR passed in as a function, `Start` carries its kind, the sweep wrapper takes a closure, the panic hook only in the sweep, the start state's differences reported not patched).

> **For agentic workers:** Implement task-by-task, in order. Steps use checkbox (`- [ ]`) syntax for tracking. Agents in this repo do not commit: leave each task's changes in the working tree and record it in `REPORT.md`.

**Goal:** Evidence that the port matches the game off the TAS route (seeded random play from every area start with every weapon, deaths included), and byte-named replays of the owner's playtest sessions.

**Architecture:** Part A extends the existing random-play lockstep (`crates/rotj-core/src/actor.rs` tests) with a start table taken from the chain, a panic-catching sweep and pinned runs per stage. Part B adds one clonable input/RNG hook to the oracle (`tools/rotj-oracle`), a fresh-stage-start replay harness in `actor/oracle.rs` that also checks the app's `Game::nmi`, and an input recorder in the app.

**Tech Stack:** Rust 1.99 (edition 2024 workspace), Bevy 0.20 (app only), the in-repo 6502 oracle; std only, no new dependencies.

**Spec:** `docs/specs/2026-10-15-off-tas-verification-design.md`

## Global Constraints

- No `unsafe`; no lint suppressions (only `#[expect(clippy::needless_pass_by_value, reason = "Bevy system params, rust-clippy#8940")]` on Bevy systems).
- Readable names (no `r_b8`-style address names in new code; the address goes in a doc comment). Code comments rare, one line.
- New tests that need the ROM return early through `Scene::tas`/`Scene::level` returning `None` (which panics under `ROTJ_REQUIRE_ROM`), like the existing ones.
- Slow tests are `#[ignore = "<why>; the gates run it in release with --ignored"]` and must pass under `ROTJ_REQUIRE_ROM=1 cargo test --release -p rotj-core -p rotj-oracle -- --ignored`.
- The chain's state tape must not change (no task changes port behaviour); if `the_loop_matches_the_tas_from_stage_1_to_the_ending` reports the tape stale, a task changed behaviour by mistake.
- Pinned random runs: under ~130 runs of 2000 frames in total.
- Cargo prefix in agent worktrees: `CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=line-tables-only`, one cargo at a time.

## Review Focus

- A sweep run whose lockstep hangs rather than panics (the `run_game` hang guard ends it as `hung`): the sweep must print it as hung, not stall. Pinned in Task 2's test via the known freezing case `random_play(1000, 48514, 0)`.
- A recording shorter than the game needs to reach the first loop wait (0 or 1 bytes): the replay must report "0 loop frames" cleanly, not panic in the hook. Task 6 test.
- A recording that outlives the oracle's game (GAME OVER reset mid-recording): the replay ends at the reset like `GameRun::reset` and reports the frames compared. Task 6 test with energy poked to 1 and lives 0 if `$0482` is verified, else documented.
- The recorder's directory missing or not writable: the game keeps running with a warning. Task 7 test.
- Two sessions started in the same millisecond: `create_new` fails for the second; it retries once with a `-2` suffix, then gives up with a warning. Task 7 test.

---

## Part A: random play everywhere (one DeepSeek agent, task `randomplay`)

### Task 1: The chain lists its starts

**Files:**
- Modify: `crates/rotj-core/src/actor/oracle.rs` (`GameRun`, `Environment`, `run_game`, `Lockstep`, `TapePlayer`)
- Modify: `crates/rotj-core/src/actor.rs` (tests)

**Interfaces:**
- Produces: `GameRun::starts: Vec<Start>` with `pub(crate) struct Start { pub kind: StartKind, pub frame: u64, pub area: u8, pub room: u8 }` and `pub(crate) enum StartKind { RoomChange, NextArea }`: the oracle frame of the first loop frame after each room change and next-area load, with the port's `$0400`/`$0402` there (boss fights are next-area loads: the boss areas `$04/$0E/$14/$1E/$22` are areas). `Environment::frame(&self) -> u64`, delegating to `TapePlayer`'s existing `frame()` (`oracle.rs`, the last consumed event's frame) and `Lockstep`'s `frame` field.

- [ ] **Step 1:** test `the_chain_lists_its_starts` (in `actor.rs` tests, a plain test: the tape needs no ROM): replays the committed tape with `replay_chain` and asserts 12 `RoomChange` and 17 `NextArea` starts, frames strictly increasing; prints every start as `kind frame area room`.
- [ ] **Step 2:** run it; expect a compile failure (`starts` missing).
- [ ] **Step 3:** implement: in `run_game`, a `pending: Option<StartKind>` set where a `RoomChange`/`NextArea` script starts (the `if let Some(script)` branch); after the next `env.check(..)` (not after `loop_wait`: `TapePlayer::loop_wait` is a no-op, so its frame is only current after the check) push `Start { kind, frame: env.frame(), area: game.world.area, room: game.world.room }` and clear it.
- [ ] **Step 4:** run the test and the tape tests (`cargo test -p rotj-core tape`); all pass, tape unchanged.

### Task 2: The sweep

**Files:**
- Modify: `crates/rotj-core/src/actor.rs` (tests: `random_play`, new `sweep_run`, `random_play_sweep`, `STARTS`)

**Interfaces:**
- Consumes: Task 1's printed starts.
- Produces: `const STARTS: [(u64, StartKind, u8, u8); N]` (TAS frame 350 as a `NextArea` start of area 0 room 0, plus Task 1's 29 starts, committed from its output); `fn random_play(start: u64, seed: u32, weapon: u8, poke: &[(u16, u8)]) -> Option<GameRun>` (the existing one gains `poke`: RAM bytes set in the scene at the start; existing callers pass `&[]`); `fn caught(run: impl FnOnce() -> Option<GameRun>) -> Result<Option<GameRun>, String>` (`catch_unwind(AssertUnwindSafe(run))`, the panic payload as the `Err` string, both `&str` and `String` payloads).

- [ ] **Step 1:** tests (ignored, release): `caught_reports_a_divergence`: `caught(|| ..)` around `assert_game_from` with a port start whose player energy differs from the oracle's (the pattern of `the_port_starts_stage_1_1_and_plays_it_like_the_tas`) returns `Err` containing `$0478`, then `caught(|| random_play(350, 8269, 1, &[]))` returns `Ok(Some(run))` with 1967 frames. `lives_at_zero_reach_game_over`: `random_play(start, seed, 1, &[(0x478, 1), (0x482, 0)])` from a stage 1 start ends with `run.reset` (find a seed that dies; if none in 20 seeds, report the lives poke unverified and drop the `0x482` poke from the sweep).
- [ ] **Step 2:** run; fails to compile.
- [ ] **Step 3:** implement `caught` (no panic-hook changes: the ignored tests run in parallel and the hook is process-global) and `random_play_sweep` (`#[ignore]`; the only place that installs a quiet hook, once, at its start): for every start in `STARTS`, weapons 0-4, seeds `base..base+k` (`ROTJ_SWEEP_SEEDS` default 3, `ROTJ_SWEEP_BASE` default 1, 0 skipped), plus one death run per start (`&[(0x478, 1)]`, and `(0x482, 0)` if verified); prints `kind start area room seed weapon pokes -> frames | hung | reset | DIVERGES <message>`; never fails itself.
- [ ] **Step 4:** run both tests; pass. Run the sweep once with defaults and save its output to `REPORT.md`.

### Task 3: Pinned runs

**Files:**
- Modify: `crates/rotj-core/src/actor.rs` (tests)

- [ ] **Step 1:** from the sweep output, for each `NextArea` start in `STARTS` (18 with frame 350) and weapon 0-4 take the first passing seed (room-change starts stay sweep-only, per the spec; ~90 pinned runs plus the death runs, under the cap with stage 1's 13); group by stage (area `$0400 >> 1` -> stage via `docs/SUBLEVELS.md`'s table) into `#[ignore]` tests `random_play_in_stage_N_matches_the_oracle` (N = 2..7; stage 1's existing tests stay as they are) using `plays_randomly`; include each start's `energy = Some(1)` run if it passed.
- [ ] **Step 2:** `ROTJ_REQUIRE_ROM=1 cargo test --release -p rotj-core random_play -- --ignored`; all pass; total time printed in REPORT.md (must be under ~150 s).
- [ ] **Step 3:** REPORT.md lists every divergence (start, area/room, seed, weapon, message) and every (start, weapon) with no passing seed. Do not fix divergences.
- [ ] **Step 4:** gates: `ROTJ_REQUIRE_ROM=1 cargo test --workspace`, the ignored release run above plus `-p rotj-oracle`, clippy `-D warnings`, `cargo fmt --check`.

## Part B: playtest replays (one agent, task `playtest`, after the `ending` merge)

### Task 4: The oracle's playtest hook

**Files:**
- Modify: `tools/rotj-oracle/src/bus.rs` (`Trace`), `tools/rotj-oracle/src/machine.rs` (`instruction`)
- Test: `tools/rotj-oracle/src/machine.rs` tests (or the crate's existing test module for `Machine`)

**Interfaces:**
- Produces: `#[derive(Clone, Debug)] pub struct Playtest { pub pads: Vec<u8>, pub next: usize, pub last_pad: u8, pub wait: u16, pub woke: u16, pub spins: usize, pub step: fn([u8; 2]) -> [u8; 2], pub start: Option<StartBytes>, saved: [u8; 2] }` and `pub struct StartBytes { pub rng: [u8; 2], pub frame: u8, pub frame_odd: bool }` (applied once, at the first `woke`: `$F9/$FA = rng`, `$F1 = frame`, `$F7` bit 7 = `frame_odd`, other bits kept); `Trace::playtest: Option<Playtest>`. Behaviour in `instruction`, before pokes and peeks: at `wait` (every NMI wait, which is one `Play::step` in the app, loop frame or script frame alike) set `bus.pad = pads[next]` (0 past the end), `last_pad` to it, `next += 1`, and save `$F9/$FA`; at `woke` write the saved bytes stepped `spins` times with `step`. A lag NMI reads the latched `bus.pad` unchanged. `step` is supplied by the core harness (`|rng| { let mut r = Rng(rng); r.next_frame(); r.0 }`): rotj-oracle has rotj-core only as a dev-dependency, so its library cannot call `Rng`.
- [ ] **Step 1:** tests on a hand-assembled program in a test machine (the crate's existing test style): the pad advances once per `wait`; the RNG bytes at `woke` equal the bytes at `wait` stepped `spins` times with a test `step`; `start` bytes land once; `$F7`'s low bits survive.
- [ ] **Step 2:** run, fail; implement; run, pass. `cargo test -p rotj-oracle`.

### Task 5: Fresh stage starts in lockstep

**Files:**
- Modify: `crates/rotj-core/src/actor/oracle.rs` (`Scene`, `Lockstep::new`/`start`)

**Interfaces:**
- Produces: `Scene::fresh(level: u8) -> Option<Scene>` (power-on, `$D3D8` forced, Start pressed: `Scene::level` with the level's start frame from `const FRESH_START: [u64; 13]`, generated once by a throwaway loop that steps `Scene::level(l, f)` a frame at a time until a `$D3F9` peek appears and prints `f`; the `$D3D8` poke removed from `trace.pokes` once there); `fn assert_fresh(level: u8, pads: &[u8]) -> GameRun` (the oracle from `Scene::fresh(level)` with `trace.playtest = Some(Playtest { pads, spins: RNG_SPINS, start: Some(port start bytes), .. })`, the port from `room::start(&prg, &chr, level)`, run through `assert_game_from`).

- [ ] **Step 1:** test `every_fresh_level_start_matches` (ignored, release): for levels 0-12, `assert_fresh(level, &[0; 300])` (300 oracle frames) compares at least 250 loop frames, and the `$D3F9` peek's `$F9/$FA`/`$F1` equal the port's start bytes (the hook's start bytes must already be applied at that peek: `Lockstep::new` takes it on a scene clone). The existing stage 1 start test patches `probe`, `oam`, `work`, `pad`, `pressed` from the oracle; `assert_fresh` patches nothing, because the app runs `room::start` unpatched: every difference is a real `Play::start` bug. Levels that fail are listed in REPORT.md with their first diverging byte (not patched in the harness) and excluded from the test with that note; STATUS gets the list.
- [ ] **Step 2:** run, fail (missing); implement; run.
- [ ] **Step 3:** the existing `the_port_starts_stage_1_1_and_plays_it_like_the_tas` still passes.

### Task 6: The replay and the app-NMI check

**Files:**
- Modify: `crates/rotj-core/src/actor/oracle.rs` (`Lockstep::nmi` gains the check when a playtest runs), `crates/rotj-core/src/actor.rs` (tests)

**Interfaces:**
- Consumes: Tasks 4-5.
- Produces: `#[ignore]` test `the_playtest_matches_the_oracle` reading `ROTJ_PLAYTEST` (absent: returns; over 216,000 bytes: panics `playtest too long`); the level parsed from the file name's `-level<N>.pads`; prints `playtest <file>: N loop frames match` or panics with the lockstep message plus `at {N/60}s of play, {lag} lag NMIs so far`.
- The compare's panic message (in `Lockstep`, which holds `injected` and the loop count) adds `at {N/60}s of play, {lag} lag NMIs so far` when a playtest runs.
- In `Lockstep::nmi` with a playtest: clone the port state, run `Game::nmi(pad)` (the hook's `last_pad`, read from the oracle's trace) on the clone, inject as today, then assert `world.rng`, `world.pad`, `world.pressed`, `world.frame`, `world.frame_odd` equal between the clone and the injected state (message `the app's NMI differs: <field>`).

- [ ] **Step 1:** tests: `a_recording_replays` (ignored): 2000 seeded `random_pads` from level 0 and from a later level that passed Task 5 replay clean through `assert_fresh` (frames = `pads.len()` oracle frames), including at least one room change; `the_app_nmi_is_checked` (should_panic `the app's NMI differs`): the same run with the hook's `spins` set to `RNG_SPINS + 1`; `an_empty_recording_replays` (0 pads: ends cleanly). Divergence naming is already covered by the existing lockstep tests.
- [ ] **Step 2:** run, fail; implement; run, pass.

### Task 7: The app's recorder

**Files:**
- Modify: `crates/app/src/play.rs` (a `Recorder` resource inserted in `enter` when `ROTJ_RECORD_INPUT` is set and non-empty, written in `tick`, removed in `leave`)

**Interfaces:**
- Produces: `struct Recorder(std::fs::File)` with `fn open(dir: &Path, level: usize) -> Option<Recorder>` (reads the clock, calls `open_at`) and `fn open_at(dir: &Path, millis: u128, level: usize) -> Option<Recorder>` (`<millis>-level<N>.pads` via `OpenOptions::new().write(true).create_new(true)`, one retry as `<millis>-2-level<N>.pads`, else `warn!` and `None`; Task 6's parser reads the level from the `-level<N>.pads` suffix, so both names parse) and `fn record(&mut self, pad: u8)` (`write_all(&[pad])`; on error `warn!` once and stop recording).

- [ ] **Step 1:** tests: `the_recorder_writes_each_pad` (temp dir under `std::env::temp_dir()`, record 3 pads, read back the same 3 bytes, the name ends `-level0.pads`); `a_second_recorder_does_not_clobber` (two `open_at` calls with the same `millis` both succeed, distinct files); `a_missing_directory_records_nothing` (`open` returns `None`, no panic).
- [ ] **Step 2:** run, fail; implement; run, pass.
- [ ] **Step 3:** gates as in Task 3 Step 4, plus `bash tools/boot-check.sh`. REPORT.md gives the owner's two commands: play with `ROTJ_RECORD_INPUT=<dir> cargo run --release -p batman-rotj`, check with `ROTJ_REQUIRE_ROM=1 ROTJ_PLAYTEST=<file> cargo test --release -p rotj-core the_playtest_matches_the_oracle -- --ignored --nocapture`.

## Part C: picture parity (the Part B agent, after Task 7)

### Task 8: The port's VRAM against the oracle's

**Files:**
- Modify: `crates/app/src/play.rs` (`Ppu` moves out), `crates/rotj-core/src/actor/world.rs` (`Ppu` moves in, `poke` unchanged), `crates/rotj-core/src/actor/oracle.rs` (`Lockstep`: the port-side `Ppu`, the clone before inject, the compare at `WOKE`), `tools/rotj-oracle` (a `pub fn ppu(&self) -> &Ppu` on `Machine` if `bus.ppu` is not already reachable)

**Interfaces:**
- Consumes: Task 6's clone in `Lockstep::nmi` (run before the inject; one clone for both checks).
- Produces: `rotj_core::actor::world::Ppu` (`new`, `apply(&mut self, writes: &[VramWrite], mirroring: u8)`, `nametables: [[u8; 1024]; 2]`, `palette: [u8; 32]`), used by the app.

- [ ] **Step 1:** tests: `a_changed_direct_write_is_caught` (ignored, release): a lockstep through the stage 1 -> 2 banner cutscene with one `vram.direct` byte changed in the clone after `nmi` panics naming `VRAM table` and the offset; `the_palette_mirror_agrees` (plain): `Ppu::poke` at `$3F10`/`$3F14` and the oracle's `palette_index` land on the same entry, compared `& 0x3F`.
- [ ] **Step 2:** run, fail; implement (spec section 4); run, pass.
- [ ] **Step 3:** the whole-game chain in release passes with the check on (no re-record), and its time before/after goes in the report; any VRAM divergence it finds is reported (frame, table, offset, bytes, the script running), not patched, unless it is a clear root-cause fix in the port's VRAM path with a test. Same for the pinned random runs if Part A has merged.

## Part D: novelty-guided exploration (spec section 5; one agent, task `explore`)

### Task 9: The lockstep hands back its final scene

**Files:** `crates/rotj-core/src/actor/oracle.rs`
**Produces:** `pub(crate) fn assert_game_scene(scene: Scene, frames: u64) -> (GameRun, Scene)` (the existing `assert_game` becomes a wrapper that drops the scene; the scene is the lockstep's own after its last frame).
- [ ] Test `the_final_scene_is_the_oracles`: `assert_game_scene(Scene::tas(350), 120)` returns a scene whose `machine.frame` is 470 and whose RAM equals a plain `Scene::tas(350)` run 120 frames with the same pads.

### Task 10: Features and the explorer

**Files:** `crates/rotj-core/src/actor.rs` (tests module, next to `random_play`)
**Consumes:** `STARTS`, `random_pads`, `caught`, Task 9.
**Produces:** `struct Path { start: u64, pokes: Vec<(u16, u8)>, pads: Vec<u8> }`; `fn features(ram: &[u8; 2048], wram: &[u8; 8192]) -> (Cell, BTreeSet<(u8, u8)>)` with `type Cell = (u8, u8, u16, u8)` = `(ram[$400], ram[$402], camera_x >> 12, ram[$46E])` and the pairs `(wram[$7CC0 - $6000 + slot] & $7F, wram[$7DD0 - $6000 + slot] >> 4)` over occupied slots (camera x is `$CC:$CD`, 1/16 px; slots as `Scene::isolate` iterates them); `fn explore_branch(path: &Path, seed: u32) -> Result<(GameRun, Scene, Path), String>` (replays `path.pads` then 300 new `random_pads(seed, 300)` frames in one `assert_game_scene` from `Scene::tas(path.start)` with the pokes, inside `caught`); `#[ignore] fn explore()` (archive seeded with `STARTS` x weapons 0-5; `ROTJ_EXPLORE_BRANCHES` default 2000, `ROTJ_EXPLORE_SEED` default 1; pick uniformly among the 16 newest entries with a xorshift from the seed; depth cap 8 segments; a branch with `run.frames == 0` past its parent is not novel; prints cells and pairs reached, the TAS's own counts from a pass over the chain's tape replay or a 2000-frame TAS run per start with the same key, and each divergence with its pads written to `target/explore/<n>-start<frame>.pads`).
- [ ] Tests: `features_of_a_known_frame` (a TAS frame's cell and pairs, values read from the oracle once and pinned); `the_same_seed_explores_the_same_way` (two 20-branch runs give the same archive); `a_branch_into_a_cutscene_is_skipped` (no panic escapes `explore_branch`).
- [ ] Gates as Task 3, plus one `explore` run with defaults whose summary goes in REPORT.md (do not fix divergences; list them).
