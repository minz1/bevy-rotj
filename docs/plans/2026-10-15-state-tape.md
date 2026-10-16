# State Tapes Implementation Plan

> Approved 2026-10-15 by a stand-in reviewer (all changes applied).

> **For agentic workers:** Implement task-by-task, in order. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replay the whole-TAS parity chain through the port alone from a committed tape, checked by a per-frame state hash, so CI covers parity without the ROM.

**Architecture:** `assert_game_from`'s loop becomes `run_game<E: Environment>`; `Lockstep` (the oracle) and `TapePlayer` (the tape) implement `Environment`. The lockstep records every injection and hash into a `Tape`; the player feeds them back through the same injection code.

**Tech Stack:** Rust (edition 2024, workspace lints), `rotj-core` test code only, no new dependencies.

**Spec:** `docs/specs/2026-10-15-state-tape-design.md`

## Global Constraints

- No new dependencies. No `unsafe`. No lint suppressions.
- Hash: FNV-1a 64 (offset basis `0xcbf29ce484222325`, prime `0x100000001b3`).
- Tape file: `u32` magic/version first, little-endian, committed raw at `crates/rotj-core/tapes/<test>.tape`.
- All new code is `#[cfg(test)]` (it lives beside `actor/oracle.rs`, which is test-only).
- Readable names, comments rare (one line, only where the code cannot say it).
- Gates: `ROTJ_REQUIRE_ROM=1 cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`; also `cargo test --workspace` **without** the variable must run the replay.
- Starts after the `stage2` and `firecheck` branches merge into main.

## Review Focus

1. A tape recorded before a routine was ported, replayed after: must fail "stale", never pass. (Task 3 test `a_stale_tape_is_rejected`.)
2. A port change that alters one byte mid-chain: replay must fail naming that loop frame and TAS frame. (Task 3 test `a_changed_hash_names_its_frame`.)
3. `assets/prg.bin` or the committed CHR drifting from the cart: recording must refuse. (Task 3, the record path's assertion.)
4. A truncated or wrong-version tape file: decode must fail with a clear message, not panic on an index. (Task 1 test `decode_rejects_a_wrong_magic_and_a_truncated_file`.)
5. Nondeterminism in the frame path: two replays must agree. (Task 3 test `the_replay_is_deterministic`.)

---

### Task 1: Tape format and hashes

**Files:**
- Create: `crates/rotj-core/src/actor/tape.rs`
- Modify: `crates/rotj-core/src/actor.rs` (add `#[cfg(test)] mod tape;` next to `mod oracle;`)

**Interfaces:**
- Produces:
  - `pub(crate) enum Event { Start(Box<[u8; 10240]>), Relift(Box<[u8; 10240]>), Irq(Vec<(u16, u8)>), Nmi(Vec<(u16, u8)>), Outside { kind: u8, waited: bool, writes: Vec<(u16, u8)> }, Unreached(Box<[u8; 8192]>), ScriptCheck(u64), Check(u64), Reset, End }` (addresses index the 10 KiB RAM+WRAM space: RAM `0..0x800`, WRAM `0x800..0x2800`; `kind` is `outside as u8` on the fieldless `actor::Outside`, checked on replay; `waited` is the lockstep's `waited` flag, which only `outside` sets). `Irq` is recorded at **every** `inject_irqs` call, empty or not, so no event is optional and the player never looks ahead.
  - `pub(crate) struct Tape { pub events: Vec<(u64, Event)> }` (the `u64` is the oracle frame)
  - `impl Tape { pub(crate) fn encode(&self) -> Vec<u8>; pub(crate) fn decode(bytes: &[u8]) -> Result<Tape, String> }`
  - `pub(crate) fn loop_hash(ram: &[u8; 2048], wram: &[u8; 8192]) -> u64` (SCRATCH zeroed, RAM then WRAM)
  - `pub(crate) fn script_hash(ram: &[u8; 2048]) -> u64` (SCRATCH and `PPU_MASK` zeroed)
  - Make `oracle::SCRATCH` and `oracle::PPU_MASK` `pub(crate)` for these.

- [ ] **Step 1: Write the failing tests** in `tape.rs`:
  - `fnv_matches_the_reference_vectors`: `fnv1a(b"") == 0xcbf29ce484222325`, `fnv1a(b"a") == 0xaf63dc4c8601ec8c`.
  - `scratch_bytes_do_not_change_the_hash`: two zeroed memories, one with `ram[0x10] = 1` and `ram[0x4A0] = 7`: equal `loop_hash`; with `ram[0x40] = 1` instead: different.
  - `script_hash_ignores_the_mask_shadow`: `ram[0xFE]` differs, `script_hash` equal.
  - `every_event_round_trips`: a `Tape` holding one of each `Event` variant at distinct frames: `Tape::decode(&tape.encode()) == Ok(tape)` (derive `PartialEq, Debug`).
  - `decode_rejects_a_wrong_magic_and_a_truncated_file`: wrong first `u32` -> `Err` containing `"magic"`; the encoding cut one byte short -> `Err` containing `"truncated"`; a tape whose last event is neither `End` nor `Reset` (cut at an event boundary) -> `Err` containing `"truncated"`.
- [ ] **Step 2: Run** `CARGO_BUILD_JOBS=2 cargo test -p rotj-core tape::` — expected: FAIL (does not compile).
- [ ] **Step 3: Implement** `tape.rs`: a private `fn fnv1a(bytes: &[u8]) -> u64`, the two hashes, `Event`/`Tape` and the codec (one tag byte per event, then its frame as `u64`, then a `u32` count for write lists; snapshots fixed-size). Magic `u32` = `0x5441_5045` ("TAPE"), then a `u32` version `1`; decode reads through a small cursor that returns `Err("truncated ...")` instead of indexing past the end.
- [ ] **Step 4: Run** the same command — expected: 5 passed.
- [ ] **Step 5:** leave uncommitted for the orchestrator.

### Task 2: One loop, the lockstep behind a trait, recording

**Files:**
- Modify: `crates/rotj-core/src/actor/oracle.rs` (`Lockstep`, `assert_game`, `assert_game_from`, `inject_unreached`)

**Interfaces:**
- Consumes: Task 1's `Event`, `Tape`, `loop_hash`, `script_hash`.
- Produces:
  - `pub(crate) trait Environment` with the methods the loop needs (names are yours; they must cover: the first lift, the IRQ injection, the NMI wait→woke injection, the per-frame script compare, `outside`, `relift`, the `waited` flag, the GAME OVER reset, the `Unreached` injection, the loop-frame compare, and the end-of-run test that replaces `scene.machine.frame < end`).
  - `pub(crate) fn run_game(env: &mut impl Environment, prg: &[u8], chr: &[u8]) -> GameRun` — today's `assert_game_from` loop, the only copy. The environment's start method returns `(State, bool)`: the first state and whether the first frame's NMI is already consumed (today's `started`); `Lockstep` handles `start: Some` (the `WOKE` skip and the peek buffers) inside it, the player always returns `false`.
  - `pub(crate) fn assert_game_recording(scene: Scene, frames: u64) -> (GameRun, Tape)`; `assert_game(scene, frames)` keeps its signature and is `assert_game_recording(scene, frames).0`.
  - Failures panic, as the lockstep's compares do today (no `Result` across the trait).
  - `inject_unreached(game, chr, oracle_wram: &[u8; 8192], wram: &mut [u8; 8192])` (takes a WRAM slice, not a `PeekEvent`).
  - `assert_game_from` with `start: Some` keeps working through the same `run_game` (tapes do not cover it).
- [ ] **Step 1: Pin today's behaviour.** The existing chain test (`actor::tests::the_loop_matches_the_tas_from_stage_1_through_the_cutscene_into_stage_2`, its counts as on main after `stage2` merges) is the regression test. Run it on main first and note its counts and time.
- [ ] **Step 2: Write the failing test** in `oracle.rs` tests: `recording_counts_match_the_run`: `assert_game_recording(Scene::tas(350)?, 600)`; assert the first event is `Start`, the last is `End`, the number of `Event::Check` equals `run.frames`, and there is at least one `Event::Irq` and one `Event::Nmi` per `Check`.
- [ ] **Step 3: Run** `ROTJ_REQUIRE_ROM=1 CARGO_BUILD_JOBS=2 cargo test -p rotj-core recording_counts` — expected: FAIL (does not compile).
- [ ] **Step 4: Implement**: move the loop into `run_game`; `Lockstep` implements `Environment` with exactly today's comparisons; with `recorder: Option<Tape>` set it pushes each injection's bytes as it applies them (the raw `irq_writes` before filtering; the wait→woke and outside deltas as `(addr, value)`; the oracle WRAM at `Unreached`; full snapshots at start/relift) and each compare's hash computed from the oracle's bytes (`loop_hash(&oracle.ram, &oracle.wram)`; `script_hash(&wait.ram)`).
- [ ] **Step 5: Run** the chain test and `recording_counts_match_the_run` — expected: both pass, chain counts unchanged from Step 1.

### Task 3: Replay, the committed tape, and the freshness check

**Files:**
- Modify: `crates/rotj-core/src/actor/oracle.rs` (add `TapePlayer`)
- Modify: `crates/rotj-core/src/actor.rs` (the chain test records and compares; the replay tests)
- Create: `crates/rotj-core/tapes/the_loop_matches_the_tas_from_stage_1.tape` (name after the chain test; recorded, not hand-written)

**Interfaces:**
- Consumes: Tasks 1-2.
- Produces: `pub(crate) struct TapePlayer` (owns the tape cursor and the persistent 10 KiB buffer, seeded by `Start`/`Relift`), implementing `Environment`; `pub(crate) fn replay(tape: &Tape, prg: &[u8], chr: &[u8]) -> GameRun` (panics on a stale tape or a hash mismatch). The IRQ handler's entry in `GameRun::injected` counts IRQs, which the tape does not hold; the replay leaves that entry out, and comparisons with the lockstep's `GameRun` exclude it.

- [ ] **Step 1: Write the failing tests** (in `actor.rs` tests):
  - `the_tape_replays_the_chain_without_the_rom`: no `Scene`; reads `assets/prg.bin` and `assets/graphics/chr/pattern-space.bin` (via `env!("CARGO_MANIFEST_DIR")`) and the committed tape; `replay(...)`'s `frames`, `room_changes`, `stage_ends`, `next_area`, `script_frames`, `relifted` equal the chain test's pinned counts. Runs without `ROTJ_REQUIRE_ROM`.
  - `the_replay_is_deterministic`: two replays return equal `GameRun` debug strings.
  - `a_changed_hash_names_its_frame`: `#[should_panic(expected = "loop frame 99")]`, the 100th `Check`'s hash flipped (the message also carries that event's TAS frame).
  - `a_stale_tape_is_rejected`: `#[should_panic(expected = "stale")]`, an extra `Outside { kind: 0, waited: false, writes: vec![] }` inserted before the 10th `Check`.
  - The chain test, extended: it uses `assert_game_recording`; with `ROTJ_RECORD_TAPE=1` it writes the tape file; otherwise it asserts the recorded events equal the committed file's, reporting the first differing event index and frame.
- [ ] **Step 2: Run** them — expected: FAIL (no `TapePlayer`, no tape file).
- [ ] **Step 3: Implement** `TapePlayer`: each `Environment` method pops the next event and checks its variant (the event's frame is for the messages only; an empty tape is stale too; else `"tape is stale at event N (frame F): expected X, found Y; re-record with ROTJ_RECORD_TAPE=1"`), and applies it through the same code `Lockstep` uses (`inject`'s write-then-lift over the buffer, `inject_irqs`'s `$0181` filter, `inject_unreached` with the recorded WRAM); compares hash the port's projected memory with `loop_hash`/`script_hash` and panic with `"tape <name>: loop frame N (TAS frame F): state hash differs; byte diff: ROTJ_REQUIRE_ROM=1 cargo test -p rotj-core <chain test>"`. The record path asserts `assets/prg.bin == scene.prg()` and the CHR asset equals `scene.machine.bus.cart.chr`.
- [ ] **Step 4: Record**: `ROTJ_REQUIRE_ROM=1 ROTJ_RECORD_TAPE=1 cargo test -p rotj-core the_loop_matches_the_tas` — the tape file appears; note its size (expected 0.5-1 MiB; if over 2 MiB, stop and report).
- [ ] **Step 5: Run** all five tests with `ROTJ_REQUIRE_ROM=1`, then `cargo test -p rotj-core the_tape_replays` **without** it — expected: all pass; the replay alone finishes in well under a minute (report its time).

### Task 4: Docs and gates

**Files:**
- Modify: `docs/ACTORS.md` §9 (the harness: tapes, recording, the stale/hash failures), `docs/STATUS.md` (Gates row: the replay runs in plain `cargo test` and in CI; "Owner decisions pending" ROM-in-CI: whole-chain parity now runs in CI through the tape), `.gitattributes` (`*.tape binary`).

- [ ] **Step 1:** Write the docs changes above (STATUS is the orchestrator's to edit if this runs as an agent task: report the wording instead).
- [ ] **Step 2: Run** the gates in Global Constraints, plus `cargo test --workspace` without `ROTJ_REQUIRE_ROM` — expected: green, and the replay test listed as run (not skipped).
