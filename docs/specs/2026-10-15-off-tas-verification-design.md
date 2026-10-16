# Off-TAS verification: random play everywhere, playtest replays, code coverage

Status: approved 2026-10-15 by a stand-in reviewer under the owner's delegation, all
required changes applied (the RAM-overwrite start replaced by `Scene::level`, the pad
feed and RNG pin moved into one oracle hook at every NMI wait, the replay checks the app's `Game::nmi`,
coverage deferred and narrowed to port-owned code).

## Why

The whole-game chain proves the port equals the game on one input: the published
TAS. The TAS never dies, never continues, rarely uses most weapons after stage 1 and
takes one route through each room. 1.0.0 means full-game parity, and the owner will
playtest before it is tagged. This work is for evidence that the port matches the game
on input the TAS never gives, and for turning anything odd the owner sees in a
playtest into a reproducible, byte-named bug.

Success:
1. Seeded random play matches the oracle byte for byte from a start in every area
   and every boss fight, with every weapon, and through deaths; the passing runs are
   pinned and run at every merge and tag.
2. A playtest session recorded by the app replays in the oracle as the same session
   (same start, same input, same randomness), checking the app's own NMI code too, and
   the lockstep names the first byte the port gets wrong, or passes.
3. (Deferred until 1 and 2 land.) How much of the port-owned game code the verified
   runs execute, and which routines none reaches.

Out of scope: lag-frame emulation in the app (see "Known gaps"), the BYOR pass, fixing
the divergences found (each becomes its own task, root cause first).

## 1. Random play everywhere

The harness exists (`actor::tests::random_play`: a TAS lift, `$0476` poked to a
weapon, 2000 frames of seeded `random_pads`, the whole-loop lockstep; 13 pinned stage 1
runs). Extend it:

- **Start frames.** The chain test gains a way to list the TAS frame of each next-area
  load, room change and boss intro it runs (e.g. `GameRun` records `(kind, oracle
  frame)` pairs; `Lockstep.frame` is private today, so this is a small harness
  addition). The starts table is generated from that list once and committed as
  `const STARTS: [u64; N]`, the sweep's output line naming each by its area and room
  (`$0400`/`$0402` at the start).
- **Weapons.** Each start runs with weapons 0-4, and 5 once its overflow flag is
  ported.
- **Deaths.** A variant pokes the player's energy (`$0478`) to 1 at the start so the
  first hit dies (the existing poke test at `actor.rs` uses `$0478`/`$047A`/`$0479`).
  Lives (`$0482`) poked to 0 to reach GAME OVER is to be verified first (no test pokes
  it yet); the run ends at the game's reset (`GameRun::reset`).
- **Sweep.** An `#[ignore]` test `random_play_sweep` (release) runs K seeds per
  (start, weapon) and prints one line per run: start, area/room, seed, weapon, loop
  frames, and for a divergence the lockstep's panic message. Each run is wrapped in
  `std::panic::catch_unwind(AssertUnwindSafe(..))` with a quiet panic hook installed
  for the sweep (it runs alone as an ignored test, so the process-global hook is
  safe). K and the seed base come from `ROTJ_SWEEP_SEEDS`/`ROTJ_SWEEP_BASE` (default 3,
  1); seed 0 is skipped (xorshift would give all-zero pads).
- **Pinning.** The first passing seed per (area/boss start, weapon) is pinned like the
  stage 1 runs, `(start, seed, weapon, loop frames)`, in one `#[ignore]` test per stage
  (release; the gates run `--ignored` in release already). Room-change starts run in
  the sweep only. Cap: the pinned set stays under ~130 runs of 2000 frames (about 145 s
  at ~1800 loop frames/s). A divergence found is fixed at its root cause in its own
  task, then its run pinned.

## 2. Playtest replays

### Recording (app)

`ROTJ_RECORD_INPUT=<dir>`: each `Play` session (from `enter` to `leave`, one stage
start to GAME OVER or exit) writes `<dir>/<unix milliseconds>-level<N>.pads`: one byte
per `Play::step`, the pad it was given (the raw `$C1DB` byte, the game's bit order). The
writer lives in a Bevy resource next to `Play`, written from `tick`, not in
`Play::step` (the core under test does no I/O). Each byte is written with
`write_all` as it happens (no buffering: one byte at 60 Hz, and a crash loses nothing).
The file is created with `OpenOptions::create_new(true)` (no clobbering, no following
a planted symlink). An empty variable means off; a missing directory or a failed
create logs a warning and the game runs without recording. No header: the level is in
the name, the frame count is the length.

### Replay (rotj-core test harness)

`ROTJ_PLAYTEST=<file>` with an `#[ignore]` test `the_playtest_matches_the_oracle`
(release, ROM required). Files over 216,000 bytes (an hour) are rejected.

1. **Start state: a real fresh stage start on both sides.** The oracle powers on with
   the level forced (`Scene::level(n, frame)`: `$D3D8` poked, Start pressed), and the
   port starts from `room::start(level)`, what the app ran, compared at the oracle's own
   `$D3F9` through `assert_game_from` (which takes a port start state). The `$D3D8` poke
   is removed once the start is reached (it would re-force the level at every next-area
   load). The start's random bytes and frame counters (`$F9/$FA`, `$F1`, `$F7` bit 7)
   are set on the oracle to the port's start values by the hook below (bit 7 of `$F7`
   is a read-modify-write, which a static poke cannot do). The per-level start frame is
   found once by running to `$D3F9` and committed as a table. Proof: a test that every
   level 0-12 runs 300 loop frames of no input clean from this start; levels that fail
   are listed in STATUS as not replayable until fixed.
2. **One oracle hook for input and randomness.** A clonable `Playtest` value on the
   oracle's trace (`tools/rotj-oracle`), applied in `Machine::instruction` before the
   peeks at the same PC (the order pokes already use): at the NMI wait's
   entry (`WAIT`, `$C232`: every NMI wait, which is one `Play::step` in the app, loop and
   script frames alike) it sets `bus.pad` to the next recorded pad (latched on the
   `$4016` strobe, so a lag frame's second NMI reads the same pad) and saves `$F9/$FA`, and at its return (`WOKE`, `$C23F`) it overwrites them
   with the saved value stepped `RNG_SPINS` times (`Rng::next_frame`, which mirrors
   `$C21C`). The lockstep's NMI injection reads at `WOKE`, so the port receives the
   pinned bytes. The static `trace.pokes` cannot do this (fixed values only).
3. **The app's NMI is checked.** The lockstep injects the oracle's NMI bytes instead
   of calling `Game::nmi`. So at each injected NMI the replay also runs `Game::nmi(pad)`
   on a clone of the port and asserts its `rng`, `pad`, `pressed`, `frame` and
   `frame_odd` equal the injected ones: the replay verifies the app's NMI code, not
   only the loop.
4. **Pads.** `Play::step(pad_i)` runs loop frame `i`, then `nmi(pad_i)`; pad 0 is read
   at the end of the first loop frame (the run starts at a loop frame, skipping its
   NMI). The hook hands out pad `i` at the `i`th NMI wait, which is the same, and stays
   aligned through room changes, pauses and cutscenes (one wait per app tick there too).
5. **Report.** Pass: `playtest <file>: N loop frames match`. Fail: the lockstep's own
   message (`frame F (N loop frames in): $addr port XX oracle YY`) plus the playtest
   time (`N / 60` s) and the lag NMIs counted so far (`GameRun.injected`).

### Known gaps

- **Lag frames.** The app has no lag model: every tick runs one loop frame. When the
  game lags, its second NMI reads the pad again, so a press on that frame counts as
  held, and the game ignores a press the app took. A divergence right after a lag NMI
  is reported with the lag count, so it is not mistaken for a port bug. Lag is rare (21
  lag NMIs in the whole TAS); modelling it is a post-1.0 decision.
- **Waits inside routines.** Cutscene waits and the waits inside an injected `Outside`
  bracket hit `WAIT`/`WOKE` too; the app does not run those the same way, so a replay
  through one may diverge there for that reason. Reported like lag.

## 3. Code coverage (deferred)

After sections 1 and 2: an `#[ignore]` test runs the chain and the pinned random runs
with `trace.exec` covering `$6000-$FFFF`, drains the `ExecEvent`s (which carry the
bank) into a `HashSet<(bank, pc)>` each frame, and drops instructions inside interrupt
handlers and inside the injected `Outside` brackets (tracked like `survey()` does, by
vector and `RTI`), since those are injected rather than compared. It prints the count
of `JSR` targets in the disassembly's code (how `disasm/MAP.md` §11 derived its code
set is stated alongside) that no run executes. "Executed" means executed; the compare
is RAM/WRAM at loop frames. A Python report onto the listing only if that list is
long enough to need one.

## 4. Picture parity: the port's VRAM against the oracle's

Every lockstep compares RAM and WRAM. The picture the app draws comes from what the
port's own NMI (`Game::nmi`) queues for VRAM, applied by `play::Ppu` (the nametable
and palette writes, the `$C084` address step, the FME-7 mirroring). Loop-frame queue
bytes (`$0100-$0180`) are already RAM-compared; what nothing compares is the port's
`vram.direct` writes (first pages, banners, the intro card, the ending's screens),
script frames' writes, and `Ppu::apply`'s addressing and mirroring. Outside stage 1-1's
first page (`play::tests::the_first_page_is_the_captured_nametable`) a wrong tile,
attribute or palette entry would pass every test.

- **One `Ppu`.** `play::Ppu` (nametables, palette, `apply`) moves to rotj-core's
  `actor/world.rs`, next to `VramWrite`; the app uses it from there. No behaviour change.
- **The check, always on in the lockstep** (no flag; it is not recorded, so the state
  tape is unchanged). The lockstep keeps a port-side `Ppu`, lifted from the oracle's
  `ppu.vram`/`ppu.palette` at the start (the oracle already holds any first page the
  port's start queued; re-applying it is idempotent). At every NMI, *before* the
  oracle's NMI bytes are injected (the inject re-lifts the game and drops
  `vram.direct`), the port's state is cloned and runs `Game::nmi(0)` (one clone shared
  with section 2's app-NMI check); its `Vblank::writes` are applied with the port's
  `scroll.mirroring` at that NMI, as `Play::step` does. At the NMI's return (`WOKE`) the
  port-side tables (2 KiB, physical layout equal to the oracle's flat `vram`) and the
  palette are compared with the oracle's, palette bytes masked `& 0x3F` on both sides
  (the oracle stores them masked). A difference panics as `frame F (N loop frames in):
  VRAM table T offset $OOO port XX oracle YY` (physical table A/B) or `palette $NN`.
  This needs a `Scene`/`Machine` accessor for the oracle's `ppu` (`tools/rotj-oracle`).
- **Known limits.** Reported with the context named, not mistaken for port bugs:
  writes by injected routines (`Outside` brackets); lag NMIs, which drain the queue
  mid-frame in the game; mirroring, which the game applies at each `$2007` write and
  the port at the NMI (a script changing `$EC` between queueing and its NMI differs);
  whether the raster IRQ handler writes `$2007` is checked once by the implementer and
  recorded. The raster split's timing is not VRAM and stays a `shortcut:`
  (`play::split_line`).
- **Tests.** The check fails when a `vram.direct` byte of a script frame (e.g. a
  banner's) is changed in the clone after `nmi`, naming its table and offset; the
  palette mask and the `$3F10`/`$3F00` mirror agree between `Ppu::poke` and the oracle.
- **Cost.** One `Game` clone and `nmi` per NMI; measured on the chain in release and
  recorded (the chain is 19 s today).

## 5. Novelty-guided exploration (owner's idea: nesfuzz plus DFS, 2026-10-15)

Random play starts from 30 fixed points and mashes for 2000 frames, so it rarely gets
deep: a boss's late phases, a rare enemy state, a second death in a row. This section
steers it toward states no run has seen, the way nesfuzz keeps inputs that reach new
RAM states, plus the "dig from the newest interesting state" it lacks (depth first,
the owner's DFS). Approved by a stand-in reviewer with its changes applied (no
mid-run snapshots, a coarser key, a branch budget).

- **Paths, not snapshots.** An archive entry is `(start, pokes, pads)`: a `STARTS`
  frame, the RAM pokes made there (weapon `$0476`, and energy for death runs), and the
  pad bytes from there on. A branch replays its parent's whole path plus one new
  segment of `random_pads` (300 frames, fresh seed) in a single lockstep from the TAS
  lift, so the port is lifted once and every divergence reproduces exactly from its
  entry (re-lifting mid-run would compare lift artifacts, not port behaviour).
  Depth is capped at 8 segments (2400 frames past the start).
- **The end state.** The lockstep hands back the oracle's final `Scene` (a small change
  to `assert_game`/`Lockstep`, which consume it today), and the features are read from
  its RAM. A branch whose new segment compared no loop frame (inside a cutscene, say)
  is not novel.
- **The novelty key.** A cell is `(area $0400, room $0402, camera_x >> 12 (256 px),
  player motion $046E)`, read at the end of a branch. Also novel: a `(type $7CC0 & $7F,
  phase $7DD0 >> 4)` pair, over occupied slots only, never seen before (many types use
  `$7DD0` as a counter, so its low nibble is timing noise). A branch that adds a new cell
  or pair joins the archive.
- **Selection.** Uniformly among the 16 newest archive entries (depth first in effect);
  the archive starts as the 30 `STARTS` with weapons 0-5.
- **Output.** An `#[ignore]` test `explore` (release; `ROTJ_EXPLORE_BRANCHES`, default
  2000, and `ROTJ_EXPLORE_SEED`), deterministic for a seed and branch count, prints the
  cells and pairs reached and every divergence as `start pokes: <lockstep message>`,
  with the path's pads written to `target/explore/<n>.pads` (with its start and pokes in
  the file name) for replay. A TAS pass with the same key prints the TAS's own counts,
  so "how far off-route" is a number.
- **Pins.** A divergence fixed at its root cause gets a test replaying its path, like the
  random-play pins.
- **Tests.** Feature extraction on a known TAS frame gives the expected cell and pairs;
  the same seed gives the same archive; a branch ending in a cutscene is skipped, never a
  panic out of the explorer.
- **Not a gate.** Exploration runs on demand and before a release tag; its pins are gates.

## Gates and cost

- The pinned random runs and the chain run in release with `--ignored` (gates.sh
  already does): about 145 s plus the chain's 19 s.
- The sweep, the playtest replay and coverage are on demand, not gates.

## Testing

- Sweep: a run with a poked divergence is reported and the next run still plays.
- Recording: an app test steps `Play` with a known pad sequence under a recorder
  pointed at a temp dir and reads the same bytes back; a second recorder in the same
  millisecond does not clobber the first.
- Replay: a recording made by the test itself (random pads through `Play::step` from
  `Play::start`) replays clean from level 0 and from one later stage; the same
  recording with one pad flipped still replays clean (it is the new input) while a
  port state poked mid-run fails naming its byte; the `Game::nmi` assertion fails when
  `RNG_SPINS` is off by one.
- Oracle hook: unit tests in `tools/rotj-oracle` that the pad is set at `LOOP_WAIT` and
  the RNG overwritten at `WOKE`.

## Order

1. Section 1 (DeepSeek: the start-frame listing, the sweep and pinning; divergences it
   finds go to core tasks).
2. Section 2 as one task for one agent (the oracle hook, the harness and the app
   recorder together), after the ending work merges, which also edits
   `actor/oracle.rs`.
3. Section 4 by the Part B agent, after its Tasks 4-7 (it shares `Lockstep::nmi`'s clone).
4. Section 5 after the triage of section 1's divergences (it reuses `caught`, `random_pads` and `STARTS`).
5. Section 3 after the rest.
