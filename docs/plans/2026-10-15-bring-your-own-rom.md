# Bring your own ROM Implementation Plan

Status: approved 2026-10-15 by a stand-in reviewer under the owner's delegation (approve with changes; all applied).

> **For agentic workers:** Implement task-by-task, in order. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The tracked tree holds nothing ROM-derived; the app, tests, oracle and disassembly read the user's ROM at run time, with every Classic suite still green where the ROM is.

**Architecture:** A std-only crate `rotj-rom` finds, checks (MD5) and parses the ROM; every reader of ROM bytes slices it. fceux captures become oracle runs in the tests, pinned to fceux by committed FNV-1a 64 digests. The tape, the `.s` files and extracted assets become local, untracked outputs.

**Tech Stack:** Rust 2024 workspace (Bevy 0.20 app), the `rotj-oracle` 6502 oracle, Python 3 for `disasm/gen` and `tools/emu` scripts, ca65/ld65.

**Spec:** `docs/specs/2026-10-15-bring-your-own-rom-design.md`

## Global Constraints

- No `unsafe`; workspace lints (clippy pedantic) at zero warnings; no lint suppressions.
- No new crates.io dependencies (MD5 is written out in `rotj-rom`).
- ROM check: MD5 of PRG+CHR without the 16-byte header = `830111df30b8511562d4faca6aeb792d`.
- Dev ROM path: `raw/Batman - Return of the Joker (USA).nes`. Env: `ROTJ_ROM` (path), `ROTJ_REQUIRE_ROM` (any value: ROM tests must run).
- Skip line: `skipped: no ROM (set ROTJ_ROM or put it in raw/)`.
- App error exit status: 2; first line `Batman: Return of the Joker (USA) ROM not found.`
- A converted test compares the same bytes as before (spec section 5); any that cannot is listed as "different bytes" in the fixture table and reported to the owner.
- Code comments: rare, one line; doc comments follow the surrounding style.
- Commits: short subject, no Co-Authored-By.

## Worktree setup (parallel tasks)

Tasks 6-8 run in worktrees made by `.orchestration/launch.sh`/`mkwt.sh`, which symlink `raw/` to the main checkout's, so `for_test()` finds the ROM. Untracked fceux captures are read from `/home/minz1/projects/batmanROTJ/tools/emu/<dir>/` (read-only). Each worktree has its own `target/`.

## Digests (shared by Tasks 6-7)

- `rotj_core::fnv1a(bytes: &[u8]) -> u64` (FNV-1a 64, public, from `actor/tape.rs`).
- One `digests.txt` per test crate: `tools/rotj-oracle/tests/data/digests.txt`, `crates/rotj-core/tests/data/digests.txt`, `crates/app/digests.txt`. Lines `name unit digest` (digest as 16 lowercase hex digits; `unit` is a frame number or `-`); `#` lines name the capture and the `dump.lua` command that regenerates it. Tests read it with `include_str!` and a shared helper per crate.
- `tools/emu/digest.py <capture> <name> <kind> [range]` prints those lines (kinds: `ramseq` per 2048-byte frame, `apu` per frame of the filtered CSV rows exactly as the tests parse them, `file` whole file or a byte range).
- Rule for every converted capture: its digest line is committed, the test asserts the oracle's bytes have it, then the port is compared with the oracle's bytes.

## Review Focus

1. A ROM with a different iNES header (NES 2.0, or a 512-byte trainer flag) but the same PRG+CHR must load: Task 1 test `accepts_another_header`.
2. `ROTJ_ROM` set to a missing file must fail loudly, not fall through to `raw/`: Task 1 test `a_missing_rom_env_is_an_error`; Task 9's no-ROM boot check relies on it.
3. A truncated or wrong ROM in the cache must not brick later launches silently: the error names the cache file (Task 1 test `a_wrong_rom_names_its_file`).
4. A test that used to assert and now silently returns early under `ROTJ_REQUIRE_ROM=1`: Task 10 diffs the baseline's test names and its `skipped` lines against the new run.
5. A digest computed over different bytes than the old comparison (for example the whole RAM where the test compared a range): each conversion's mutation check (Tasks 6-7), with the failing output pasted into the task's report.
6. A bad `rom.nes` in the cache must not hide a good ROM beside the exe: Task 1 test `a_bad_cached_rom_falls_through`.

---

### Task 1: The `rotj-rom` crate

**Files:**
- Create: `crates/rotj-rom/Cargo.toml` (workspace package fields and lints, no dependencies), `crates/rotj-rom/src/lib.rs`, `crates/rotj-rom/src/md5.rs`
- Modify: `Cargo.toml` (members)

**Interfaces:**
- Produces:
  - `pub struct Rom { pub prg: Vec<u8>, pub chr: Vec<u8> }` (`Clone, Debug, PartialEq, Eq`)
  - `impl Rom { pub fn from_ines(bytes: &[u8]) -> Result<Rom, String>; pub fn load(arg: Option<&Path>) -> Result<Rom, String> }`
  - `pub fn for_test() -> Option<&'static Rom>`
  - `pub const PRG_CHR_MD5: &str = "830111df30b8511562d4faca6aeb792d";`
  - `pub fn md5_hex(bytes: &[u8]) -> String` (in `md5.rs`, re-exported)

- [ ] **Step 1: Tests** in `lib.rs`/`md5.rs` `#[cfg(test)]`:
  - `md5_matches_rfc_1321`: the seven RFC 1321 appendix A.5 vectors (`""` -> `d41d8cd98f00b204e9800998ecf8427e`, `"a"`, `"abc"`, `"message digest"`, `"abcdefghijklmnopqrstuvwxyz"`, the 62-char alphanumeric, the 80-digit string).
  - `rejects_other_mappers`, `rejects_a_truncated_image` (moved from `tools/rotj-oracle/src/cart.rs` tests).
  - `rejects_the_wrong_game`: a synthetic mapper-69 image (16 + 128 KiB + 256 KiB zeros) -> `Err` containing the expected MD5.
  - `accepts_another_header` (needs the ROM, via `for_test`): the real image with header bytes 7-15 changed loads and equals the original's `Rom`.
  - The search is a private pure function `search(arg: Option<&Path>, env: Option<&OsStr>, cache: Option<&Path>, exe_dir: Option<&Path>, raw: Option<&Path>) -> Result<(Rom, Source), String>` (`enum Source { Arg, Env, Cache, BesideExe, Raw }`); `load` passes the real values (`std::env::var_os("ROTJ_ROM")`, no `set_var`). Tests against a temp dir holding a good copy of the ROM (when `for_test` has one) and the synthetic bad image:
    - `a_missing_rom_env_is_an_error`: `env` names a missing file -> `Err` naming it, even with a good `raw`.
    - `a_missing_arg_is_an_error`: the same for `arg`.
    - `a_wrong_rom_names_its_file`: `arg` is the bad image -> `Err` naming its path and MD5.
    - `a_bad_cached_rom_falls_through`: bad `cache/rom.nes`, good `.nes` in `exe_dir` -> `Ok(_, BesideExe)`; with nothing else, `Err` naming the cache file.
    - `the_search_order_holds`: good files everywhere -> `Arg`; without arg -> `Env`; and so on down to `Raw`.
- [ ] **Step 2:** `cargo test -p rotj-rom` fails (no crate).
- [ ] **Step 3: Implement.** `from_ines` = today's `Cart::from_ines` plus the MD5 check of `prg ++ chr`. `load`: search order exactly as spec section 3 ("The app's search"): `arg`, `ROTJ_ROM`, `cache_dir()/rom.nes`, `*.nes` beside `current_exe()` (first that passes), `raw/` only under `cfg(debug_assertions)` (path from `env!("CARGO_MANIFEST_DIR")`). A missing or failing `arg`/`ROTJ_ROM` file is an `Err`; a failing cache or beside-exe candidate is remembered and the search goes on (its error joins the final message). When the source was `Arg` or `BesideExe`, `load` copies the file to the cache via `rom.nes.tmp` + `rename` (overwriting a bad one), ignoring cache errors. `cache_dir()` private: `LOCALAPPDATA` on Windows, else `XDG_CACHE_HOME` or `HOME/.cache`, joined with `batman-rotj`. The `NotFound` error lists every path tried. `for_test`: `ROTJ_ROM`, then `raw/`; `OnceLock`; prints the skip line or panics under `ROTJ_REQUIRE_ROM`; a failing ROM panics.
- [ ] **Step 4:** `cargo test -p rotj-rom` and `ROTJ_REQUIRE_ROM=1 cargo test -p rotj-rom` pass; clippy clean.
- [ ] **Step 5: Commit** `rom: the rotj-rom loader`.

### Task 2: The oracle and `rotj-core` read the ROM through `rotj-rom`

**Files:**
- Modify: `tools/rotj-oracle/Cargo.toml` (`rotj-rom` dependency), `tools/rotj-oracle/src/{cart.rs,lib.rs,replay.rs,machine.rs,bus.rs}`, `tools/rotj-oracle/examples/*.rs`, `tools/rotj-oracle/tests/{solids,inventory,inventory_tas,route_stage1,bossroom,tas}.rs` and every other test that calls `load_rom`/`rom_path`/`power_on`; `crates/rotj-core/Cargo.toml`, `crates/rotj-core/src/actor/{oracle.rs,oam.rs}`, `crates/rotj-core/src/actor.rs` (`asset("prg.bin")`, `asset("graphics/chr/pattern-space.bin")`).
- Delete: `tools/rotj-oracle/src/cart.rs` (its `Cart` becomes `rotj_rom::Rom`; keep `pub use rotj_rom::Rom as Cart` only if that keeps the diff smaller, otherwise rename at the call sites).
- Also: `actor.rs`'s `#[cfg(test)] mod oracle;` becomes `pub(crate) mod oracle;` (still `cfg(test)`) so `levelmap.rs`/`collision.rs`/`palettes.rs`/`actors.rs` tests can use `crate::actor::oracle::Scene`. `physics_input` (`tools/rotj-oracle/tests/music.rs:97`) and run1's `plan_input` (`tests/run1.rs`) move to `rotj_oracle::replay` as `pub fn physics_input(frame: u64) -> u8` and `pub fn plan_input(frame: u64) -> u8`. The digest helpers (section "Digests": public `fnv1a`, `digest.py`, the three empty `digests.txt` with their readers) land here too.

**Interfaces:**
- Consumes: Task 1.
- Produces: `rotj_oracle::replay::power_on() -> Option<Machine>` unchanged in signature, gated by `rotj_rom::for_test()`; `Machine::power_on(rom: Rom)`.

- [ ] **Step 1:** Replace every ROM read listed above with `rotj_rom::for_test()` (tests) or a `Rom` passed in (library code). The tests that returned early without the `ROTJ_REQUIRE_ROM` assert now go through `for_test`.
- [ ] **Step 2:** `cargo test --workspace` in a scratch worktree with no `raw/` and `ROTJ_ROM` unset: passes, ROM tests print the skip line.
- [ ] **Step 3:** `ROTJ_REQUIRE_ROM=1 cargo test -p rotj-oracle -p rotj-core` passes (same failures as the baseline, if any, are findings to report, not fixes in this task).
- [ ] **Step 4: Commit** `rom: the oracle and core tests load the ROM through rotj-rom`.

### Task 3: The app plays the user's ROM

**Files:**
- Modify: `crates/app/Cargo.toml` (`rotj-rom`; dev: `rotj-oracle`), `crates/app/src/{main.rs,play.rs,audio.rs}` (incl. the `.set(AssetPlugin { file_path: assets_dir() })` at `main.rs:70-72`, deleted; `read_asset` at `play.rs:23` and every test call `read_asset("graphics/chr/pattern-space.bin")`/`prg()`), `crates/rotj-music/src/{lib.rs,driver.rs}`, `crates/rotj-music/tests/*.rs`, `tools/rotj-oracle/tests/music.rs:37-40,130` (`load_from_assets` -> `Music::from_prg(&rom.prg)`), `crates/rotj-apu/tests/driver_trace.rs:78` (sample bank -> `prg[0x1E000..]` via `for_test`)

**Interfaces:**
- Consumes: Task 1.
- Produces: `rotj_music::Music::from_prg(prg: &[u8]) -> Music` (driver = PRG bank 10 `prg[0x14000..0x16000]`, songs = banks 6-7 `prg[0xC000..0x10000]`), replacing `load_from_assets`. `rotj_rom::Rom` derives Bevy's `Resource` in the app through a newtype `RomData(Rom)` inserted in `main`, read by `play::power_on` and the audio setup. `audio::apu_with_samples(prg: &[u8])` takes bank 15 `prg[0x1E000..0x20000]`.

- [ ] **Step 1: Test** `rotj-music` `from_prg_takes_the_old_asset_bytes` (needs the ROM): `rotj_core`-free FNV-1a 64 of the two slices `from_prg` takes equal the FNV-1a 64 of today's `assets/audio/music/driver.bin` and `song-banks.bin`, computed before deleting them and written into the test as constants. (The behavioural proof is `rotj-oracle/tests/music.rs`.)
- [ ] **Step 2:** Implement `from_prg`; delete `load_from_assets`; the driver tests use `rotj_rom::for_test()`.
- [ ] **Step 3:** `main`: `Rom::load(std::env::args_os().nth(1).as_deref().map(Path::new))`; on `Err(message)` print the spec section 4 message (first line from Global Constraints, then `Drop the .nes file onto batman-rotj.exe, put it next to the exe, or set ROTJ_ROM.`, then `message`), on Windows wait for a line on stdin, `std::process::exit(2)`. Delete `assets_dir` and `read_asset`; `Play::power_on`/`Play::start` take the slices from the resource; app tests take them from `rotj_rom::for_test()` and return early on `None`.
- [ ] **Step 4:** `cargo test -p batman-rotj` with and without the ROM; `ROTJ_ROM=/nonexistent cargo run -p batman-rotj` prints the message and exits 2; `tools/boot-check.sh` prints `OK`.
- [ ] **Step 5: Commit** `app: play the user's ROM`.

### Task 4: The tape is a local output

**Files:**
- Modify: `crates/rotj-core/src/actor.rs` (`tape_path`, `committed`, `record_or_compare`, `the_tape_replays_the_chain_without_the_rom` and the two rejection tests), `.gitignore` is unchanged (`/target/` covers it)
- Delete: `crates/rotj-core/tapes/` (git rm)

- [ ] **Step 1:** `tape_path` -> `<workspace>/target/tapes/{name}.tape`. The lockstep writes the tape on every run (drop the compare and `ROTJ_RECORD_TAPE`); the replay test is renamed `the_tape_replays_the_chain` and, when the file is absent, prints `skipped: no tape (run the release lockstep)` and returns, or panics with that command under `ROTJ_REQUIRE_ROM`. The stale/cut-short tests use the local file the same way. `gates.sh` runs the release `--ignored` suite before the debug workspace tests (Task 9).
- [ ] **Step 2:** `ROTJ_REQUIRE_ROM=1 cargo test --release -p rotj-core the_loop_matches_the_tas -- --ignored` writes `target/tapes/the_loop_matches_the_tas_from_stage_1_to_the_ending.tape`; then `ROTJ_REQUIRE_ROM=1 cargo test -p rotj-core tape` passes and runs (no skip line).
- [ ] **Step 3:** Update `docs/ACTORS.md` §9k and the tape spec's status line to say where the tape lives. Commit `tape: record locally under target/`.

### Task 5: The disassembly is generated locally

**Files:**
- Modify: `disasm/gen/generate.py` (ROM path from `sys.argv[1]` if given), `disasm/assemble.sh`, `disasm/README.md`, `.gitignore` (`disasm/*.s`, `disasm/rotj.cfg`)
- Delete: `disasm/*.s`, `disasm/rotj.cfg` from the index (`git rm --cached`)

- [ ] **Step 1:** `assemble.sh`: `ROM="${ROTJ_ROM:-../raw/Batman - Return of the Joker (USA).nes}"`, run `python3 gen/generate.py "$ROM"` (never `-I`), then assemble and `cmp` against `$ROM`.
- [ ] **Step 2:** From a clean state (`rm disasm/*.s disasm/rotj.cfg`), `bash disasm/assemble.sh` prints `OK byte-identical (668a395c…)`, and `git status` shows no tracked change in `disasm/` other than the intended ones.
- [ ] **Step 3:** README: the `.s` files are generated from your ROM by `assemble.sh`. Commit `disasm: generate the listing from the user's ROM`.

### Task 6: fceux anchors become digests

**Files:**
- Modify: `tools/rotj-oracle/tests/{run1,route,music}.rs`, `tools/rotj-oracle/tests/data/digests.txt`, `crates/app/src/play.rs` (`the_title_draws_the_emulators_frame` only), `crates/app/digests.txt`
- Captures (absolute, read-only): `tools/emu/run1/{ramseq-0.bin,apu-writes.csv}`, `tools/rotj-oracle/tests/data/route-900-{ram,oam}.bin`, `route-apu.csv`, `assets/audio/reference/apu-writes-600-1200.csv`, `assets/screens/attract/reference-emulator-300.indices`.

**Interfaces:** Consumes the "Digests" section (Task 2) and `rotj_oracle::replay::{physics_input, plan_input}`.

- [ ] **Step 1:** `music.rs`: the physics run's APU writes compare by per-frame digest; the `driver-calls` text compare is deleted with `rotj-music/tests/acceptance.rs` (Task 7c).
- [ ] **Step 2:** For each anchor: generate the digest from the capture, change the test to digest the oracle's (or `Frame`'s) bytes and compare per unit, failure message naming the unit and the regenerate command. Mutation check: flip one bit of the oracle's output in the test once, see it fail, restore.
- [ ] **Step 3:** `ROTJ_REQUIRE_ROM=1 cargo test -p rotj-oracle --test run1 --test route --test music` and the app's title test pass; `rg -n 'assets/|tests/data/.*\.(bin|csv)|tools/emu/' tools/rotj-oracle/tests/{run1,route,music}.rs` finds only `digests.txt`/input CSVs. Report pastes each mutation check's failing output. Commit `tests: pin the oracle to fceux by digest`.

### Task 7: Captured state comes from the oracle

Three parallel sub-tasks, one per file group, each following spec section 5's rule and the fixture table below (filled from the research pass before execution). Each sub-task: for every row, (1) commit the capture's digest file, (2) assert the oracle's bytes match it, (3) compare the port with the oracle's bytes, (4) mutation check, (5) stop reading the capture. Rows the oracle cannot reproduce get "different bytes" and a note.

- **7a** `crates/rotj-core/src/levelmap.rs` and `collision.rs` tests (stage 1-1 level files, `nametable-drive-*` (port dump.lua's `drive` mode into the test: Start 300-306, Right from 500, A when `f % 90 < 3`, B when `f % 150 < 3`), maps, attribute streams, `classes-7b00.bin`, stage 1-2 references, `route2-*`), plus `tools/rotj-oracle/tests/levels.rs` (the three single-level map tests become its loop over levels 0-12; a level other than 1, 4, 12 failing is a finding to report, not to fix) and `tools/rotj-oracle/tests/route_stage1.rs` (row 27's asserts; `room_change_advances_0402_without_reselecting_the_area` is deleted). Row 25's assertion: each of the 65 fixture `(camera, table)` pairs is matched to a frame of the `stage1-inputs.csv` oracle run with that camera and `$23C0` table, and the port's `streamed_attributes` equals the oracle's on every frame of the run; the 65 pairs' digest is in `digests.txt`. If the run takes over 60 s in a debug build, mark it `#[ignore]` with the gates' release reason.
- **7b** `crates/rotj-core/src/{palettes.rs,actors.rs,actor/families/k12.rs}` tests (`level-palettes.bin`, `level-chr.bin`, `prg-bank*-tables.bin` -> ROM slices, `chr-level.bin`, `actors-*-wram.bin`).
- **7c** `crates/app/src/play.rs` `the_first_page_is_the_captured_nametable`, `crates/rotj-apu/tests/driver_trace.rs`, `crates/rotj-music/tests/acceptance.rs` (convert to the oracle's writes for the physics run, or delete where `rotj-oracle/tests/music.rs` already compares the same writes; say which).

Fixture table: see "Fixture table" at the end of this plan.

- [ ] Each sub-task's gate: `ROTJ_REQUIRE_ROM=1 cargo test -p <crate>` passes with the same test count as the baseline minus the listed deletions (paste both counts); `rg -n 'assets/|tests/data/.*\.bin|tools/emu/' <touched files>` finds no capture reads; the report pastes each mutation check's failing output; `cargo test --workspace` passes. Commit per sub-task.

### Task 8: Pasted tables read from the ROM

Runs after 7a is merged (same files).

**Files:**
- Modify: `crates/rotj-core/src/collision.rs`, `crates/rotj-core/src/levelmap.rs`, `crates/rotj-core/src/actor/room.rs:654`, `tools/rotj-oracle/tests/levels.rs:5,30` (`map_from_rom`), `tools/rotj-oracle/tests/solids.rs:9,71,105,179,232` (`class_table_for_level`, `collision_of`), `tools/rotj-tools/src/levelrom.rs:14,29,35,61` (uses `AREA_BANK_TABLE`/`AREA_PPU_TABLE`; its own iNES parser at :29 becomes `rotj_rom::Rom::from_ines`), and any other caller `cargo check --workspace --all-targets` names.

**Interfaces:**
- Produces: `collision::class_table(prg: &[u8], area: usize) -> Option<[u8; CLASS_COUNT]>`, `class_table_for_level(prg, level)`, `collision_of(prg: &[u8], class: u8) -> Collision`, `Level::from_rom(prg, chr, area)`, `Level::loaded(prg, chr, area, image)`, `levelmap::map_from_rom(prg, chr, area)`, `levelmap::attributes_from_rom(prg, chr, area)` (and the class-table reader near `levelmap.rs:421`).

- [ ] **Step 1:** Before deleting each constant, add a test that the ROM read equals the constant (`class_rle_reads_the_rom`, etc.), run it with the ROM: passes. Then replace the constant by the read at its documented address (bank 0 `$02A5`+, bank 14 `$C000`/`$CADC`/`$C97D`, bank 02 `$8B63`/`$8B75`/`$8ECF`/`$8EE1`), delete the constant and that test's constant side (it becomes the existing decode tests).
- [ ] **Step 2:** `ROTJ_REQUIRE_ROM=1 cargo test --workspace` and the release `--ignored` chain pass (the chain proves the loader still builds every level identically). Commit `core: read collision and level tables from the ROM`.

### Task 9: Untrack, guard, document

**Files:**
- Create: `tools/check-no-rom-data.py`
- Modify: `.gitignore`, `tools/boot-check.sh`, `.github/workflows/ci.yml` (boot job: build, then the no-ROM check instead of a 60 s run), `.orchestration/gates.sh` (local, git-ignored: add the check script, `cargo deny check`, `bash disasm/assemble.sh`), `docs/STATUS.md`, `docs/PLAN.md`, `README.md`, `HANDOFF.md`
- Delete from the index: every file in spec section 2's "ROM slices", "captures nothing reads" and the converted captures, `assets/` except `graphics/palette-sources/`.

- [ ] **Step 1:** `check-no-rom-data.py`: loads the ROM (`ROTJ_ROM` or `raw/`), builds a set of every 32-byte window of PRG and CHR that has at least 8 distinct byte values (padding and fills are not evidence), scans every `git ls-files` blob for any window; also flags hex-text runs (decode runs of `0x..`/`$..`/bare hex pairs to bytes and scan them). Prints offending file:offset and exits 1. Run it now on the tree: it lists the files still to untrack (sanity check that it finds `assets/prg.bin` and `collision.rs` before Task 8).
- [ ] **Step 2:** `git rm --cached` the files; `.gitignore`: `/assets/*` with `!/assets/graphics/` + `/assets/graphics/*` + `!/assets/graphics/palette-sources/`. Re-run the check: exit 0.
- [ ] **Step 3:** `gates.sh` order: fmt, clippy, the release `--ignored` suite (records the tape), the debug workspace tests, `check-no-rom-data.py`, `cargo deny check`, `bash disasm/assemble.sh`, boot. `rotj-tools`' `prg`/`chr`/`level`/`stage` commands write to the now-ignored `assets/`; `level --dump`'s comparison with a capture is documented as local-only in `docs/LEVELDATA.md`.
- [ ] **Step 3b:** `boot-check.sh` also runs `ROTJ_ROM=/nonexistent` and expects exit 2 and the message.
- [ ] **Step 4:** Docs: STATUS (milestone row 1.0.1, Gates row: tape local, ROM loader, "Owner decisions pending" updated with section 9's three questions), PLAN (BYOR done), README (how to supply the ROM), HANDOFF. Commit `repo: no ROM-derived files tracked`.

### Task 10: Verify and release

- [ ] **Step 1:** `bash .orchestration/gates.sh` green with `ROTJ_REQUIRE_ROM=1`; `cargo deny check`; `bash disasm/assemble.sh`; `bash tools/boot-check.sh`.
- [ ] **Step 2:** Test-name diff: list `test ... ok` names from `.orchestration/logs/baseline-test.log` + `baseline-ignored.log` and the new run; every baseline name is present or listed as merged/renamed in the fixture table; no new `skipped` line in the REQUIRE run.
- [ ] **Step 3:** Without the ROM (`raw/` renamed, `ROTJ_ROM` unset): `cargo test --workspace` green, clippy, fmt.
- [ ] **Step 4:** Windows build (HANDOFF command), strip into `C:\Users\emery\Documents\batman-rotj\`, remove that folder's `assets/`; launch from WSL with the ROM passed as the argument: it plays (log shows the title running for 30 s) and the cache holds `rom.nes`; launch with `ROTJ_ROM` pointing at a missing file: the message.
- [ ] **Step 5:** Version 1.0.1 in `Cargo.toml` workspace and crate `version` pins, STATUS milestone row; release commit `Release 1.0.1`; tag `1.0.1` after the owner's answers to section 9.1 if they change CI, otherwise at once.

## Fixture table

Decisions on the research below (verified against the oracle 2026-10-15, 0 bytes differing unless noted):

- Same bytes from the oracle: rows 1-3, 5, 9-12, 14-21, 24. Pure ROM slices: rows 22-23 (read the loaded ROM).
- Digest-only anchors (no oracle source): row 8 (title pixels), rows 11-12, 14-15 (`route.rs`, `run1.rs`), row 9 in `music.rs`.
- Row 10 (`driver-calls`) is the oracle's own output and its only other reader (`acceptance.rs`) is deleted: the compare is dropped.
- Row 25: the test runs over every frame of the `stage1-inputs.csv` run (port vs oracle) and asserts the fixture's 65 `(camera, table)` digests all occur in it: more frames than before, same bytes.
- Row 26: 8 of 12 records come from the oracle (drive, `route-inputs.csv`); pages 720, route-chain 900/1200 and seek2 700 (the "14 differences" record) have no recorded input: dropped, reported to the owner.
- Row 27: `levelmap.rs`'s `room_change_advances_0402_without_reselecting_the_area` is deleted; its asserts (`$0400` unchanged and the map unchanged across the `$0402` change) move into `route_stage1.rs::stage1_route_reaches_the_red_brick_room_change`; the bytes differ from route2's, the relationship is the same. Reported.
- Redundant, deleted: `rotj-music/tests/acceptance.rs` (subsumed by `rotj-oracle/tests/music.rs`); the three single-level map tests become one loop over levels 0-12 (`Scene::level(n, 623)`).
- `driver_trace.rs` takes the oracle's APU writes for the physics run.
- Orphans (rows 4, 6, 7): untrack.
- Conventions for implementers are in the research's first section (dump at frame F = oracle after `run_frame(1..=F-1)`; OAM = `bus.ram[0x200..0x300]`; palette mirror fix).

### Research (verbatim)

# Fixture audit for spec section 5 (bring-your-own-ROM)

Conventions, all checked by me this session with a scratch crate (`/tmp/.../scratchpad/chk`, `chk2`, path-dep on `tools/rotj-oracle`, release build, nothing in the repo touched):

- An fceux dump "at frame F" equals the oracle after `run_frame(1..=F-1)` (`machine.frame == F-1`), with input(f) fed to `run_frame(f)`. (Same LAG=1 as `run1.rs`.) `Scene::level(n, F-1)` is therefore the "frame F" dump.
- Oracle field for each capture kind: WRAM dump = `bus.wram` (`$6000-$7FFF`), RAM = `bus.ram`, OAM "page of the last $4014" = `bus.ram[0x200..0x300]` (NOT `ppu.oam`, which differs in 42 bytes at 700), nametables = `bus.ppu.vram` (2 KiB physical; mirroring mode is 2 = single-screen A in stage 1-1, so fceux's 2048-byte `$2000-$27FF` = `vram[0..1024]` twice, and a 1024-byte NT = `vram[0..1024]`), palette = `bus.ppu.palette` with entries `$10,$14,$18,$1C` copied from `$00,$04,$08,$0C` (fceux dumps show the mirrored values).
- `tools/emu/digest.py` does not exist yet (only compare.py, check-frame.py, ...); the FNV-1a pin is new work.
- Local `tools/emu/*/config.txt` only lists `dumps/ramseq/uptime/leads`, plus `input=` and env vars for runs made after commit a3b47c7 (only `oracle-route` among those relevant). Older INPUT modes are therefore inferred from `dump.lua`, docs, and (where I could) confirmed by reproducing the capture bytes.
- "Verified" below means: I ran the oracle and compared bytes, 0 differ.

## A. Table

| # | fixture | reader(s) (file:line, test) | what the test does / bytes compared | pure ROM slice? | origin (capture dir, INPUT mode, env, pokes) | oracle source | same / different bytes |
|---|---|---|---|---|---|---|---|
| 1 | `assets/levels/stage-1-1/collision.bin` (8192 B, WRAM `$6000-$7FFF`) | `crates/rotj-core/src/levelmap.rs:645` `captured_map()` used by `renders_every_captured_camera_position` (:662); `:711` `rom_decoder_reproduces_the_captured_map`; `:731` `rom_map_columns_beyond_160_are_payload_code`; `:773` `rom_attributes_reproduce_the_captured_attribute_map` (bytes `0x1600..0x1A00`); `:901` `auto_tiler_reproduces_the_captured_tile_tables` (area 0, bytes at `TILE_TABLE_ADDRS - $6000`, `..ids(0)` each); `crates/rotj-core/src/collision.rs:427` `area_zero_table_matches_the_capture` (`[0x1340..+CLASS_COUNT]`). Non-test: `tools/rotj-tools/src/level.rs:75` `verify` (CLI acceptance, `rotj level --dump`) | expected value. Port decodes from ROM/CHR, compares to these bytes: whole map `0..0x1000` (all 4096), attribute pages `0x1600..`, tile tables, class table. Also `from_wram` input for `captured_map()` (input to the port) | No. Chunk-only ROM hits (a 256 B piece at 0x3eba is a coincidence of repeated code). Not a ROM slice: it is game WRAM after the level loader ran | `tools/emu/stage` (identical bytes in `phys/ route-dense/ screens/ prog/ ...` `wram-700.bin`), frame 700. `docs/LEVELS.md`: `INPUT=physics DUMPS="700,900" RAMSEQ=off UPTIME=1001`; commit 4ead47f. `physics` mode = Start 300-306, Right 660-780, A at 800, Right >=900, A at 905. No pokes | `physics_input` in `tools/rotj-oracle/tests/music.rs:97` (identical to dump.lua mode), run to `machine.frame == 699`, take `bus.wram`. **Verified 0/8192 differ.** Confidence high. (Level load only, so `Scene::level(0, 699)` or any other path to the level start would also give it: wram at 700 is identical across all runs that start the level at ~600.) | same bytes |
| 2 | `assets/levels/stage-1-1/nametables.bin` (2048 B, `$2000-$27FF` at 700) | `levelmap.rs:665` (`renders_every_captured_camera_position`, scroll 0, compares 30 visible rows = first 960 B of each 1 KiB page via `visible_mismatches`); `levelmap.rs:803` `rom_attributes_reproduce_the_captured_ppu_attribute_table` (bytes `[960..1024]` vs `pages[0]`); `crates/app/src/play.rs:643` `the_first_page_is_the_captured_nametable` (`captured[..960]` vs `play.ppu.nametables[0][..960]`). Non-test: `rotj-tools level.rs:74` | expected value (rendered nametable, attribute table, port's first page) | No (game VRAM) | same capture as row 1 (`stage` frame 700, `ppu-700.bin` offset `0x2000`) | physics run, `vram[0..1024]` twice at `machine.frame==699`. **Verified 0/2048 differ.** Confidence high | same bytes |
| 3 | `nametable-drive-600.bin`, `-700.bin`, `-800.bin` (1024 B each) | `levelmap.rs:666-668` `renders_every_captured_camera_position`, scrolls 0 / 50 / 54, 960 visible bytes each | expected value | No | `tools/emu/drive` frames 600/700/800 (`DUMPS=200,...,1000`, `ramseq=100,1000`), `INPUT=drive`: Start 300-306; from 500 Right; A when `f%90<3`; B when `f%150<3`. Commit 22daa5b. `$AC` = 0 / 50 / 54 (I read it back) | port `dump.lua drive` mode to Rust (6 lines, I wrote it in the scratch crate), `vram[0..1024]` at `machine.frame==F-1`. **Verified 0/1024 differ for all three; also `ram` equals `drive/ram-F.bin` byte for byte.** Confidence high | same bytes |
| 4 | `assets/levels/stage-1-1/level.bin` (640 B) | none (grep: only written by `rotj-tools level.rs:25`, `stage.rs`) | not read | No | Not a capture: `rotj level` output decoded from the ROM (commit e9282e9). A 640 B run of it exists in `route2-left2/wram-5500.bin` | n/a. Untrack with the other generated outputs | n/a |
| 5 | `assets/levels/stage-1-2/reference/tile-tables-744.bin` (880 B) | `levelmap.rs:912` `auto_tiler_reproduces_the_captured_tile_tables` (area 1, `captured[at..at+ids(1)]` per table, `at = addr-$6000-0x1000`) | expected value, only the ids `< N` of each of the 4 tables | No (WRAM `$7000-$736F`) | `tools/emu/stagearea-probe/lvl-1` `wram-744.bin` offset 4096. probe.lua: `LEVEL=1` writes `$20 = 1` at exec of `$D3D8`; Start 300-306; `AFTER` frames from the `$D3D8` hit (frame 474) | `Scene::level(1, 743)` (`oracle.rs:249`, poke `(0xD3D8, 0x20, 1)`, Start 300-306), take `bus.wram[0x1000..0x1370]`. **Verified 0 differ.** Confidence high | same bytes |
| 6 | `stage-1-2/reference/background-744.indices`, `nametable-744.bin`, `palette-744.bin` | none (no reader anywhere in `*.rs/*.py/*.sh`) | orphans | No | `stagearea-probe/lvl-1` frame 744 | If wanted: `vram[0..1024]` and mirrored palette at `Scene::level(1,743)`: both **verified 0 differ**; indices need a renderer | untrack, nothing reads them |
| 7 | `stage-1-1/reference/*` : `background-{pages-816,route-dense-916/922/926/934,walk-940}.indices`, `emulator-700.indices`, `scrolled-camera-{50,54}.indices`, `oam-{700,camera-50,camera-54,pages-816,route-dense-916/922/926/934,walk-940}.bin`, `physics-700-830.csv`, `jump-hold-lengths.csv` | none: the legacy-path tests that read them were deleted in ef6cb89 ("delete the ROTJ_LEGACY path"; `LEVELDATA.md` §16 still names tests that no longer exist) | orphans | No (oam files are RAM page `$0200` of the dump; none occurs in the ROM) | `stage` 700; `route-dense` 916-934; `walk` 940; `pages` 816; `drive`/`phys` camera 50/54; `jump-hold*` | OAM fixtures are `bus.ram[0x200..0x300]`: oam-700 (physics@699) and oam-route-dense-916/922/926/934 (`route-inputs.csv`) **verified 0 differ**. Pixels: no renderer in the oracle | untrack; nothing to convert |
| 8 | `assets/screens/attract/reference-emulator-300.indices` (57344 B = 256x224) | `crates/app/src/play.rs:446` `the_title_draws_the_emulators_frame` | expected value: all 57344 pixels of the port's title picture after 400 attract frames + 10 steps, `differ == 0` | No | `tools/emu/attract` frame 300 (`DUMPS=80..400`); no input before 400 (default `script` mode starts at 600), so pad 0 throughout. Pixels came from the screenshot via compare.py | **None found for the pixels.** The oracle has no pixel output. Nearest: oracle `ppu.vram/palette/oam`, CHR bank regs and scroll at `machine.frame==299` with pad 0 fed to `rotj_core::render` (the `rotj-verify screen` route, which `docs/LEVELS.md` says reproduces emulator indices 0/57344), but the title has a raster IRQ split so scroll/ctrl per scanline are not in the oracle either. This is spec 5's "pure fceux anchor, digest only" case. Confidence that no existing helper does it: high | n/a. If kept: same bytes, digest pin only, and then the test cannot compare port to oracle, only to the pinned capture, which is not available without fceux. Needs the owner |
| 9 | `assets/audio/reference/apu-writes-600-1200.csv` (5109 rows `frame,addr,value`, incl. `$4014/$4016` rows) | (a) `tools/rotj-oracle/tests/music.rs:155` `physics_run_is_the_acceptance_capture` (oracle vs capture, `$4014/$4016` filtered, frames >= first row). (b) `crates/rotj-music/tests/acceptance.rs:42` `ported_driver_matches_the_capture` and `ported_driver_matches_the_song_window` (frames 600-700, 769 writes). (c) `crates/rotj-apu/tests/driver_trace.rs:33` `replaying_the_driver_produces_sound_on_every_channel` | (a) both (oracle vs capture); (b) expected value; (c) input to `rotj_apu::Apu` (all rows, unfiltered, frame ranges `first..=last`, asserts only >4000 writes, channel coverage, peak/rms thresholds, not a byte compare) | No | `tools/emu/stage/apu-writes.csv` rows with frame 600..1200: **exactly equal** (5108 rows + header). Commit 54a4cd7. `INPUT=physics` | `physics_input` (`music.rs:97`) frames 1..=1005, `FrameLog.apu` rows. `music.rs:155` already proves it (test passes: I ran `cargo test --release -p rotj-oracle --test music`). Confidence high | same bytes (the CSV equals the oracle's filtered stream; for (c) the `$4014/$4016` rows are in the CSV and not in the filtered stream, harmless to the APU model: GUESS, not checked) |
| 10 | `assets/audio/reference/driver-calls-600-1200.csv` (979 lines) | `music.rs:151` (equality with oracle-generated text); `acceptance.rs:60` `replay()` | (a) the oracle derives it and asserts equality; (b) input to the port | Not a capture: generated by the oracle's `SideBySide.calls` (commit b42e6ce) | generated by `music.rs` | text built in `music.rs:143-149`; **already oracle output** (test passes) | same bytes |
| 11 | `tools/rotj-oracle/tests/data/route-900-ram.bin` (2048), `route-900-oam.bin` (256) | `tools/rotj-oracle/tests/route.rs:18-19` `route_replay_matches_frame_900` (RAM all 2048 B; `ppu.oam` all 256 B at `machine.frame==899`) | expected value | No | `tools/emu/oracle-route` frame 900 (`ram-900.bin`, `oam-900.bin`; byte-equal). config: `input=route`, `ROUTE_JUMPS=735..760,792..817`, dumps 600,900, ramseq 600,1000, uptime 1001. `route` mode: Start 300-306, Right from 610 (`ROUTE_FROM` default), A on listed frames | the same test (route-inputs.csv, frames 1..=899). Pure fceux anchor (spec 5), digest only. Note `ppu.oam` here (the test passes: the route frame lines up) vs `ram[0x200..]` | same bytes |
| 12 | `route-apu.csv` (3443 rows) | `route.rs:40` same test, APU writes frames 600..=1000 | expected value | No | `oracle-route/apu-writes.csv` rows with frame 600..=1000 and `$4014/$4016` removed: **exactly equal** | same test | same bytes |
| 13 | `route-inputs.csv`, `stage1-inputs.csv` (1003 / 1028 rows) | route.rs, bossroom.rs, solids.rs, route_stage1.rs, examples bg_chr/bg_split/routefind, `Scene::route`, ... | the oracle's input; not a capture. `route-inputs.csv` is byte-equal to `tools/emu/oracle-route/inputs.csv` (fceux's own log); `stage1-inputs.csv` is `routefind` output | n/a | see left | these ARE the oracle sources named by spec 5; keep tracked | n/a |
| 14 | `tools/emu/run1/ramseq-0.bin` (6,146,048 B = 3001 x 2048) | `tools/rotj-oracle/tests/run1.rs:16` via `ramseq()`: `matches_fceux_ram_for_3000_frames` (all 2048 B each frame 0..2999, entries 0 and 1 vs power-on RAM); `dmc_sample_keeps_rng_in_step` (`$F9-$FA`, frames 801-840) | expected value | No | `tools/emu/run1`, `ramseq=0,3000 uptime=3001`. INPUT is not in config.txt; `plan_input` in `run1.rs:21` states `INPUT=plan` (dump.lua:737: Start when `100<=f<=900 && f%20<3`; after 900 Right, A `f%60<3`, B `f%137<3`). Confirmed: the oracle matches all 3000 frames (test passes, I ran it) | the test's own `plan_input`, power-on, `LAG=1`. Pure anchor: digests per frame (3001 x FNV-1a of 2048 B) so a mismatch names the frame | same bytes |
| 15 | `tools/emu/run1/apu-writes.csv` | `run1.rs:123` `apu_writes_match_the_capture` (frames <= 3000, `$4014/$4016` filtered, `frame,addr,value`) | expected value | No | same run | same test; digest of the filtered tuple stream (28065 rows before filtering) | same bytes |
| 16 | `crates/rotj-core/tests/data/actors-seek-700-wram.bin` (304 B = WRAM `$7CC0-$7DEF`) | `crates/rotj-core/src/actors.rs:733` `captured_slots` -> `live_slots_decode_the_captured_opening_screen` (:747, whole 304 B: only slot 2 occupied, schedule/state/behaviour/kind/phase), `occupied_byte_matches_the_record_and_schedule` (:861) | input to the port's `live_slots` + expected (asserted constants) | No | Doc says `tools/emu/seek` 700 (SEEK pokes `$0400/$00AC/$94/...` from frame 620). **The same 304 bytes are in `stage/phys/seek/seek2` `wram-700.bin`**, i.e. no poke is needed | physics run at `machine.frame==699`, `bus.wram[0x1CC0..0x1CC0+304]`. **Verified 0 differ.** The seek pokes are not needed | same bytes |
| 17 | `actors-route-900-wram.bin` (304 B) | `actors.rs:766` `route_slots` -> `live_slots_decode_the_route_capture`, `route_slots_match_their_spawn_records` | same as 16 | No | `tools/emu/route-dense` frame 900 (`INPUT=route` + `ROUTE_JUMPS`; same input as `oracle-route`) | `route-inputs.csv`, `machine.frame==899`, `bus.wram[0x1CC0..+304]`. **Verified 0 differ** | same bytes |
| 18 | `area5-map.bin` (4096) | `levelmap.rs:964` `rom_decoder_matches_a_captured_second_area` | expected: all 4096 B vs `map_from_rom(5)` | No | `sublevels-force/wram-a5.bin` (writes `$0400=10` at the loader; force.lua), identical to `stagearea-probe/lvl-4/wram-624.bin[0..0x1000]` | `Scene::level(4, 623)` (level 4 -> area 5). **Verified 0 differ.** Already analogous to `tools/rotj-oracle/tests/levels.rs` (only levels 2, 3, 9) | same bytes (the forced-`$0400` capture and the `$20=4` capture have identical map bytes) |
| 19 | `area16-map.bin` (4096), `area16-attrs.bin` (1024) | `levelmap.rs:982` `rom_decoder_matches_the_extended_area_16`; `:929` `rom_attributes_reproduce_a_second_area` (all 1024 B but only 2x2 blocks with ids `< count`, 1005 checked) | expected | No | `sublevels-force/wram-a16.bin` = `stagearea-probe/lvl-12/wram-624.bin` (map `[0..0x1000]`, attrs `[0x1600..0x1A00]`) | `Scene::level(12, 623)`. **Verified 0 differ both** | same bytes |
| 20 | `level1-map.bin` (4096) | `levelmap.rs:1022` `map_for_level_matches_a_captured_second_level` | expected | No | `stagearea-probe/lvl-1/wram-624.bin[..0x1000]` (LEVEL=1) | `Scene::level(1, 623)`. **Verified 0 differ** | same bytes |
| 21 | `level-palettes.bin` (416 = 13 x 32), `level-chr.bin` (104 = 13 x 8) | `crates/rotj-core/src/palettes.rs:298` `captured_palette`, `:307` `captured_chr`, `decoded_palette_and_chr_match_every_captured_level` (:323): all 13 levels | expected (palette bytes, `$E0-$E7` CHR set) | No | `stagearea-probe/lvl-<n>/ppu-744.bin[0x3F00..0x3F20]` (palette) and `ram-624.bin[0xE0..0xE8]` (CHR set), n = 0..12 | `Scene::level(n, 743)` palette (mirror `$10,$14,$18,$1C`), `Scene::level(n, 623)` `bus.ram[0xE0..0xE8]`, n = 0..12. **Verified 0 differ for all 13** (palette only with the mirror fix; raw `ppu.palette` differs in 4 bytes/level) | same bytes |
| 22 | `prg-bank00-tables.bin` (416), `prg-bank0D-tables.bin` (128), `prg-bank0E-tables.bin` (1472) | `palettes.rs:285-287` `prg_fixture()` (used by `rejects_a_short_rom`, `decoded_palette_and_chr_match_every_captured_level`, `resolves_every_levels_chr_handles`, `decodes_the_background_chr_animations`) | input to the port (a 128 KiB PRG built with these slices at bank 0 `$8020`, bank 13 `$B5E0`, bank 14 `$C040`) | **Yes.** ROM file offsets `0x30`, `0x1b5f0`, `0x1c050` (PRG `0x20`, `0x1b5e0`, `0x1c040`) | n/a (ROM bytes, also in any `wram-0.bin` dump of bank 0) | read the real PRG (`assets/prg.bin` is itself a ROM copy and goes away under BYOR; the oracle's `cart.prg` or the app's loaded ROM) instead of the three slices; no digest needed | same bytes |
| 23 | `chr-level.bin` (40960) | `levelmap.rs:698` `chr_fixture()` -> 13 tests (`rom_decoder_reproduces_the_captured_map` :708, `rom_map_columns_beyond_160...` :728, `rom_decoder_decodes_every_area` :748, `rom_attributes_*` :770/:800/:810/:929, `streamed_attributes_*` :833/:855, `rom_classes_*` :872, `rom_decoder_matches_*` :962/:980, `map_for_level_*` :1007/:1020) | input to the port (CHR banks `$D8-$FF`) | **Yes.** File offset `0x56010` = CHR `0x36000` = `0xD8 * 1024`; same as `machine.bus.cart.chr[0x36000..]` | n/a | the loaded ROM's CHR | same bytes |
| 24 | `classes-7b00.bin` (201) | `levelmap.rs:875` `rom_classes_reproduce_the_emulator_class_table` | expected: all 201 B == `classes_from_chr(chr, 0).0[..201]` | No | `tools/emu/attributes-trace/at-8E57.bin` first 201 B (attributes-trace/trace.lua, snapshot at exec of `$8E57`, frame 606, `$7B00` before the auto-tiler overwrites the stale tail). Differs from wram at 700 in 119 B | `machine.bus.trace.peeks = vec![0x8E57]`, power-on, physics input (or Start 300-306 only) to ~607, take `peeked[0].wram[0x1B00..0x1B00+201]`. **Verified: all 14 peeks I looked at match, 0 differ.** | same bytes |
| 25 | `attrstream-scroll.bin` (4290 = 65 x (u16 camera + 64 B `$23C0` table)) | `levelmap.rs:835` `streamed_attributes_reproduce_every_scrolled_frame` (all 65 records, `streamed_attributes(pages, camera)` vs table) | expected | No | Three scripted runs `tools/emu/attrstream-{ra,rb,rc}` (trace.lua `ROUTE=1`, `JFROM=735`, jump hold `JP/JH` = 50/22, 40/30, 60/25 per LEVELDATA.md; `CYCLE`, `BACK`, `INV` values are **not recorded anywhere** (no config.txt), only `cam.csv`/`attr-frames.bin`/`ppu-nt.csv` are kept; 1401 rows each). Fixture = per-tile-step dedupe (cameras 0, 8, ..., 512) | **Exact input lost.** I tried the base hypothesis (no CYCLE/BACK) vs `cam.csv`: not matching (84 of 300 frames), so the run was not reproduced (GUESS: needs the lost `CYCLE/BACK/INV` env; brute force is possible but I did not). Nearest oracle run: `stage1-inputs.csv`, record (previous-frame `($CC:$CD)>>4 & !7`, `vram[0x3C0..0x400]`) per frame: **all 65 of 65 fixture records reproduced exactly** (route-inputs 31/65, TAS 41/65 in the first 3000 frames) | **different bytes** in the sense of source run (the oracle route replaces three lost scripted runs) but every fixture pair is present, so the test can assert the port against oracle pairs for all 65 steps. Report to owner |
| 26 | `attrstream-dumps.bin` (792 = 12 x 66) | `levelmap.rs:858` `streamed_attributes_against_earlier_captures` (expected diff counts `[0,0,0,0,0,2,0,0,0,0,0,14]`) | expected | No | order: drive 700 (cam 50), drive 800 (54), pages 720 (22), route-dense 760/820/852/900 (73/109/152/231), route-step 1000 (246), route-oam 850 (148), route-chain 900/1200 (265/512), seek2 700 (188; forged `$00AC` pokes) | drive 700, 800: `drive` input, **verified table equal**. route-dense 760/820/852/900, route-oam 850, route-step 1000: `route-inputs.csv` at `machine.frame==F-1`, **verified `$23C0` tables equal (0 differ), and RAM equals the local `route-oam/ram-850.bin`, `route-step/ram-1000.bin`** (so route-step/oam share the route-dense input). **Not reproduced**: pages 720, route-chain 900/1200 (no recorded input; camera 265 vs 231 shows a different input), seek2 700 (SEEK_* env missing from config; 14-byte diff arises from the poke) | **same bytes for 8 of 12**, **different/none for 4** (pages 720, route-chain x2, seek2 700). The seek2 row's "14 bytes differ because nothing streamed" has no oracle equivalent unless the seek pokes are ported (`dump.lua` seek mode: `$0400=2*screen`, `$AC`, `$94`, `$92/$93=0`, `$B1=192` from frame 620; default SEEK_SCREEN=2, SEEK_X=700: GUESS the env) |
| 27 | `route2-ram-1148.bin`, `route2-ram-1152.bin` (2048), `route2-map-1148.bin`, `route2-map-1152.bin` (4096) | `levelmap.rs:1037` `room_change_advances_0402_without_reselecting_the_area` | bytes compared: RAM `[0x0400]` (both), `[0x0402]` (0x00 before, 0x02 after) and `map1148 == map1152`. Does not call the port at all | No | `tools/emu/route2-route` (also `route2-final`), `INPUT=route2` with unrecorded env (`ROUTE_*` bunny-hop etc.; config only has dumps list; no inputs.csv), `$0402` written 0->1->2 at frame 1150 | **none found for those exact bytes.** Nearest: `stage1-inputs.csv`: `$0402` 0 -> 2 at frame 1025; I checked `$0400` stays 0 and `wram[0..0x1000]` is identical across the change and equals the fixture map (**map bytes equal fixture, `$0400`/`$0402` equal**); remaining RAM differs from the capture. `route_stage1.rs:39` `stage1_route_reaches_the_red_brick_room_change` pins `0->2` but not `$0400` and the map | **different bytes** (only the 4 values/relationship compared, which the oracle run reproduces); trivially made redundant by adding the two asserts to `route_stage1.rs` |

## B. Per-file notes the table cannot hold

- Pure ROM slices (no digest, read the loaded ROM): `chr-level.bin` (CHR 0x36000..0x40000, 40960), `prg-bank00-tables.bin` (PRG 0x20), `prg-bank0D-tables.bin` (PRG 0x1b5e0), `prg-bank0E-tables.bin` (PRG 0x1c040). Everything else in the list is not a ROM slice (only incidental 16-128 B chunk matches in code/tables).
- Directly computable from the ROM without fceux (the port's own decoder is the thing under test, so they must stay oracle bytes): none beyond the above.
- `assets/levels/stage-1-1/{level.txt,meta.txt,windows.txt}` and the 15 other files in `assets/audio/dmc`, `music`, `graphics` were not asked and are not capture-read by tests I found.
- Non-test readers of captures, to change with the same plan: `tools/rotj-tools/src/level.rs:73-75` (`rotj level --dump`: `nametables.bin`, `collision.bin`), `stage.rs`/`sublevels.rs` write them. `tools/rotj-oracle/examples/*` only read `route-inputs.csv`/`stage1-inputs.csv`/`tas/inputs.csv`.

## C. Tests that would be fully redundant with an existing oracle test

1. `crates/rotj-music/tests/acceptance.rs` `ported_driver_matches_the_capture` and `ported_driver_matches_the_song_window` (replay recorded calls into the port, compare writes with `apu-writes-600-1200.csv`; the song-window one is a 600-700 subset) are subsumed by `tools/rotj-oracle/tests/music.rs` `physics_run_is_the_acceptance_capture` (:137: same run, port driven side by side with the game every frame, game writes asserted equal to the capture, calls CSV asserted equal). Strictly weaker; delete, plus the `driver-calls-600-1200.csv` replay dependency. (The `769` count in the window test is the only unique assertion: a constant of the capture.)
2. `levelmap.rs` `rom_decoder_matches_a_captured_second_area` (:962), `rom_decoder_matches_the_extended_area_16` (:980), `map_for_level_matches_a_captured_second_level` (:1020) are the same check as `tools/rotj-oracle/tests/levels.rs` (`stages_2_2_and_5_2_are_one_screen_maps...`, `a_full_stage_keeps_its_map`: oracle `wram[..0x1000] == map_from_rom(area_for_level(level))`) but for levels 1, 4, 12 instead of 2, 3, 9. One loop over levels 0..12 in `levels.rs` replaces all three (I verified 1, 4, 12 equal; I did not run the other ten).
3. `levelmap.rs` `room_change_advances_0402_without_reselecting_the_area` (:1037) overlaps `route_stage1.rs` `stage1_route_reaches_the_red_brick_room_change` (adding two asserts there covers it).
4. `crates/rotj-apu/tests/driver_trace.rs`: NOT redundant (only test feeding the apu core a real write stream). Take the oracle's writes (rotj-oracle dev-dependency) and drop the CSV.
5. `tools/rotj-oracle/tests/route.rs` and `run1.rs` are pure fceux anchors with distinct inputs; not redundant with each other.
6. Probably redundant (GUESS, not checked): `crates/app/src/play.rs:642` `the_first_page_is_the_captured_nametable` against the lockstep tests in `crates/rotj-core/src/actor/oracle.rs`, which diff PPU/queue state against the oracle each frame; check whether the lockstep covers `ppu.nametables[0][..960]` at room start before keeping it.

## D. Rows needing the owner (spec 5, rule 3)

- Row 8 (title pixels): no oracle renderer; either digest-only anchor or build a render-from-oracle-state helper.
- Row 25 (attrstream-scroll): three lost scripted runs replaced by `stage1-inputs.csv`, all 65 records still reproduced.
- Row 26: four of twelve dump records (pages 720, route-chain 900/1200, seek2 700) have no recoverable input; nearest is the same routine on the oracle route.
- Row 27 (route2): not the same bytes, only the compared relationship.
