# The actor families' phases as named states (1.1.x step A3)

Status: approved with changes by the Opus stand-in, 2026-10-17 (applied: discriminants
are the bytes the ROM stores, seven machines storing doubled indices; the round-trip test
over all 256 bytes; `dropper` in the pilot; the survey's writers corrected; the comparison
and `if`-chain rules; `blast_walker`'s proof; bit 6 named for its readers; why enums here
and consts in A2). Step A3 of `docs/specs/2026-10-16-modernisation-roadmap-design.md`, on
A1's and A2's pattern (`Motion`, `Weapon`), from a read-only survey of `actor/families/`.

## What the survey found

- Every family's `State` in `Brain` is a unit struct: the machines live in the shared
  `Body`, which the oracle lifts from and projects to WRAM.
- `Body::phase` (`$7DD0`) is each family's state machine. 26 families `match` it on
  literals (`0 => phase0(..)`, ..., `n => panic!("type $0C phase {n} is past the $8A68
  table")`), from two phases (`opening_fall_warp`) to seven (`final_boss`, `spinner`).
  Seven of them store the ROM's doubled table index and dispatch on 0/2/4 or 0/2:
  `diving_bomber`, `dropper`, `falling_enemy`, `patrol_walker`, `hop_thrower`,
  `ledge_walker`, `drift_wave_spawner`. Three more test it as zero or not in `if` chains
  (`spitter`, `proximity_bomb`, `falling_enemy_spawner`). The same value means a different
  thing in each family.
- In about 70 families `phase` is not a machine: a direction bit seeded by the parent, a
  counter (`pickup`, `swinging_hazard`, `volley_thrower`, `stage4_boss_slam`), an animation
  index (`flash_marker`, `two_stage_drifter_continuation`), a flag in bit 7 (13 families), or a
  cache (`boss_status`).
- Every write into a machine's phase from outside its own family is 0: `Body::spawn`, the
  parents' array-zeroing writes (`stage6_boss` → `$AA`, `spinner_spawner` → `$7E`,
  `stage6_lobber_spawner` → `$96`, `falling_enemy_spawner` → `$AE`) and
  `stage4_boss_slam` resetting its parent. `engine::dying`'s zero is the pickup's (kind
  `$10`, not a machine). Non-zero seeds go only to families whose phase is not a machine.
  So nothing reaches a machine's dispatch with a value its own code did not write, and
  `from_byte` adds no panic.
- `Body::state` (`$7D50`) is not a machine: bit 7 "drops a pickup when it dies" (read only
  by `engine::dying`, where the low seven bits are the pickup's weapon), bit 6 "not a
  target" (read only by the targeting code: the homing retarget, `weapons.rs` `$830E`, and
  the player's `$9F05`, all three skipping actors with it set; set on children and
  non-children alike), the low bits per-family data. Bit 7 means other things in
  `crumbling_block` ("not yet blanked"), `cell_painter` and `path_platform`, which keep
  their own names.

## Design

- **Machines become enums.** In each of the 26 family files, a `#[repr(u8)] enum Phase`
  named for what each phase does (from the handler's code and its `$addr` doc comments;
  `Hover`, `Impact`, `Stomp` for the stomper), each variant's discriminant the byte the
  ROM stores (`dropper`'s are 0, 2 and 4), with `byte()` and `from_byte() ->
  Option<Phase>` as `Motion`'s. `phase` stays a `u8` in `Body` (WRAM the oracle compares).
  The dispatch becomes `match Phase::from_byte(body.phase) { Some(Phase::Hover) =>
  hover(..), ..., None => panic!(..) }` with today's message; every write `body.phase =
  Phase::Impact.byte()`; every comparison `body.phase == Phase::X.byte()`. The handlers
  `phase0`..`phaseN` take the phase's name; inline arms (`hover_diver`) stay inline.
  Literals that match no phase stay literals with a one-line note (`dropper`'s
  `body.phase == 1` in its dying branch: it only holds 0, 2 and 4).
- **The one cross-family write** that names a phase: `stage4_boss_slam` resetting its
  parent writes `stage4_boss::Phase::<first>.byte()`. The zero seeds and array-zeroing
  loops stay as they are.
- **`blast_walker`** dispatches on `(phase << 1) & $FF`, the ROM's word index. Its phase is
  written only by `Body::spawn` (0) and by itself (1-3); no family spawns a `$7C` and level
  spawns zero the phase, so no value above `$7F` reaches it: a plain `Phase` match.
- **Why enums here, consts in A2.** A2 kept consts because the projectile kind byte has
  ranges (`$82..=$FF`) and a meaning every module shares (0 is a free slot). Each family's
  phase is a closed set of values, every match already ends in a panic, and the ECS step
  will hold it per family in `Brain`; enums add exhaustive matches without adding or
  losing a panic.
- **Not machines stay bytes**, with named constants where they are flags or thresholds:
  the `if`-chain families (`spitter`, `proximity_bomb`, `falling_enemy_spawner`: their
  `else` accepts any non-zero value, so `from_byte` would add a panic), the bit-7 users,
  the counters.
- **`state`'s shared bits are named** once. `actors.rs` already named them, from an older
  reading (`STATE_ACTIVE`, "still alive", for bit 7; `STATE_DONE` for bit 6); they become
  `STATE_DROPS_PICKUP = $80` (used in `engine::dying`, whose low seven bits are then
  `state & !STATE_DROPS_PICKUP`) and `STATE_UNTARGETED = $40` (the three targeting reads and
  every family that sets it), beside the spawn schedule's `CELL_*`, rather than a second
  definition in `slots.rs` (implementation note, 2026-10-17).
- **Tests.** The oracle phase-coverage helpers keep counting bytes. Each converted family
  gets `phase_bytes_round_trip`: over all 256 bytes, `from_byte` is `Some` exactly at each
  variant's byte and `byte()` returns it. About 500 lines of `byte`/`from_byte`/test
  across 26 files is accepted for readability; a macro only if the copies drift.

## Plan

1. Pilot, in one commit: `stomper` (three phases), `hover_diver` (four, inline arms),
   `dropper` (doubled bytes 0/2/4), `stage3_boss` (six), and `state`'s two bits. Its review
   settles the naming before the rest.
2. The other 22 machines in four batches of five or six, each batch's names checked against
   its handlers' doc comments. The fleet (opencode, at most six agents) may take batches,
   running only `cargo test -p rotj-core <family>`; Claude merges, and runs the release
   `--ignored` suites, a 1-seed sweep and a 300-branch exploration once per merged batch.
   The stale `stage6_lobber` comment (`$7E10,x` for a `$7DD0` write) is fixed in its batch.
3. The `reviewer` agent at the pilot and at the end; clippy throughout.

## Not here

Typing `Body`'s other bytes per family (timer, counter, anim), moving the phase into
`Brain` (the ECS step's component, once `Body` stops being the WRAM mirror), the families'
ROM tables as data (with levels as assets).
