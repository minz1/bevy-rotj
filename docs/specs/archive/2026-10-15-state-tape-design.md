# State tapes: whole-TAS parity without the oracle

Status: approved 2026-10-15 by a stand-in reviewer under the owner's delegation (approve with changes; all seven applied).

## Why

The parity gate is `assert_game` (`crates/rotj-core/src/actor/oracle.rs`): the 6502
oracle and the port run in lockstep and every RAM/WRAM byte is compared at every
loop frame. It is exact and names the first differing byte, but:

1. **It needs the ROM**, so CI never runs it (STATUS "Owner decisions pending":
   ROM data in CI).
2. **It is slow** and gets slower with every stage: the chain
   (`the_loop_matches_the_tas_from_stage_1_through_the_cutscene_into_stage_2`)
   covers 7,220 of the TAS's ~41,000 frames and already takes minutes, because
   the oracle emulates every CPU cycle.

Emulators test against recorded movies with a per-frame state hash. The port
cannot replay from input alone: the RNG is spun inside the NMI wait, a count that
depends on CPU cycle timing the port does not model, and the raster IRQ, unported
routines and area-load leftovers are injected from the oracle. So the movie has to
carry those injections too: a **tape**.

## Success criteria

- A default-gate test (`cargo test`, no ROM, runs in CI) replays the whole chain
  through the port alone from a committed tape and checks every loop frame's
  state hash, in well under a minute.
- On a mismatch it names the TAS frame and the loop frame, and the command that
  gives the byte diff (the lockstep test from that frame).
- A stale tape cannot pass silently: if the port's injection points change (a
  routine gets ported, an injection is removed), replay fails with "re-record".
- The lockstep test still exists and stays the ground truth; it checks that the
  committed tape equals what it would record.

## Design

### One loop, two environments

`assert_game_from`'s loop body stays the single source of control flow (for
`start: None`, the only form the chain uses; tapes do not cover a `start: Some`
run). What it asks of the oracle becomes a trait with two implementations:
`Lockstep` (today's code, which also records when given a `Tape`) and
`TapePlayer`. Both own the persistent 10 KiB RAM/WRAM buffer the loop projects
into today (`lock.ram`/`lock.wram`): it carries bytes no system projects (the
leftover ranges `inject_unreached` writes), so the player seeds it from the
`Start` and `Relift` snapshots and keeps it across frames exactly as the
lockstep does.

Every injection is replayed through the same code the lockstep runs, fed from
the tape instead of a peek:

| event | recorded | replayed with |
| --- | --- | --- |
| `Start`, `Relift` | the oracle's 10 KiB RAM+WRAM | `State::lift_memory`, buffer reset to it |
| `Irq` | the raw `irq_writes` list | `inject_irqs`, which filters by the port's `$0181` at replay time |
| `Nmi`, `Outside(kind, waited)` | the bytes the oracle changed between the bracket's ends (and, for `Outside`, whether the routine waited for the NMI itself, after which a `Relift` follows) | `inject`'s write-then-lift |
| `Unreached` | the oracle's 8 KiB WRAM at that loop frame | `inject_unreached`, now taking a WRAM slice instead of a `PeekEvent` |
| `ScriptCheck` | RAM-only hash | per-frame script compare |
| `Check` | RAM+WRAM hash | loop-frame compare |
| `Reset` | GAME OVER reached the oracle's reset | ends the run like `lock.expect(RESET)` |
| `End` | the frame budget ran out | ends the loop instead of `scene.machine.frame < end` |

Every event also carries the oracle's frame number, for the failure messages. Events are
consumed strictly in order: a replay that asks for an `Outside` the tape does not
hold next (or skips one it does) fails with "tape is stale: re-record". This
catches a routine that stopped being injected. It cannot be stricter than the
lockstep about what an injection masks: both overwrite the same bytes.

### Hashes

FNV-1a 64 (a few lines, no dependency; `std`'s `DefaultHasher` is not stable
across Rust releases), over exactly what each compare checks today:

- **Loop frame** (`oracle.rs` `diff(&oracle, port, &oracle)`, which reduces to
  equality of every non-`SCRATCH` byte): the port's projected RAM then WRAM with
  the `SCRATCH` ranges zeroed.
- **Script frame** (the cutscene/pause compare, RAM only, `PPU_MASK` patched from
  the oracle): RAM only, `SCRATCH` and `PPU_MASK` zeroed.

### Tape file

- Binary, little-endian: a `u32` magic/version, then the events. Byte writes are
  `(u16 address in the 10 KiB RAM+WRAM space, u8)` runs.
- One tape per chain test, committed raw at `crates/rotj-core/tapes/<test>.tape`
  (estimated 0.5-1 MiB for today's chain). If a tape passes ~2 MiB, revisit then.
- Stored gzip-compressed (flate2, already in the lock), since the whole-game tape is ~7 MB raw.
- The record path asserts that `assets/prg.bin` and the committed CHR equal the
  cart's PRG and CHR, so the replay cannot silently run on different data.
- It holds ROM-derived bytes (snapshots, the leftovers' WRAM), like the committed
  `assets/prg.bin`; the 1.0.1 "bring your own ROM" pass handles it with the other
  assets.

### Determinism

The replay runs the tape twice from the same start and asserts both runs agree,
to catch any nondeterminism in the frame path (iteration order, uninitialised
state).

### Gates

| run | what |
| --- | --- |
| `cargo test` (CI) | the tape replay; the port reads `assets/prg.bin` and the committed CHR |
| `ROTJ_REQUIRE_ROM=1 cargo test` | also the lockstep chain, which records in memory and asserts its event stream equals the committed tape's |
| `ROTJ_RECORD_TAPE=1` on the lockstep chain | overwrites the committed tape instead of comparing |

The lockstep's "equals the committed tape" check compares the recorded event
stream with the file's, event by event, and names the first difference.

### Mismatch report

`tape <name>: loop frame 4123 (TAS frame 5021): state hash differs. Byte diff:
ROTJ_REQUIRE_ROM=1 cargo test -p rotj-core <chain test>`; the tape stores the TAS
frame of every check, so the report needs no oracle.

## Out of scope

- Fuzzing with random input (firecheck) needs the live oracle and stays lockstep.
- Segmented tapes per stage (smaller git churn on re-record): only if re-records
  turn out frequent and large.
- Removing injections themselves (the stage2 task's leftovers work) is separate;
  the tape records whatever the port still injects.

## Order

After the `stage2` and `firecheck` branches merge (both touch the chain and
`stage2` changes the leftover injection). One core task.
