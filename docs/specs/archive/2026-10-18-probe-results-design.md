# The collision probes' results, typed

Status: approved with changes by the Opus stand-in, 2026-10-18 (applied: the oracle tests'
mapping; no "by construction" test; `weapons.rs`'s computed flags; the `$04` name; the polarity in
each doc). Implemented in 1.1.19. The survey loop's #2 (MODERNIZE "The survey loop, 2026-10-18"). No
behaviour change: the probes' oracle tests and the lockstep prove it.

## What is there

- The hit probes (`actor::collision`'s `wall`, `floor`, `feet`, `head`, `ahead`, `column`,
  `corners`) return a byte that is only ever `0` (blocked: pushed out of a wall, or landed) or
  `$80` (nothing there), the 6502's N flag; `push`, `stop`, `settle` and `crumble` return those
  two values only. Callers test `& 0x80 == 0` for "hit", and one (`opening_fall_warp`) tests
  `== 0`.
- `solid` (`$C93B`) returns `$80`, `0`, or for a class past the ported surfaces a byte of the
  ROM's `$C97E` table; every caller reads only its bit 7 (`& 0x80 != 0` = solid), `below` with
  them. So the same bit 7 means "clear" from the hit probes and "solid" from `solid`/`below`: a
  trap at 25 sites in 19 files.
- The probe's flags (`Probe::flags`, `$24`) are passed raw (`0x80`, `0x84`, `0xC0`, `0x88`,
  `0x08`, `0x10`, directions 1 and 2). The port's handlers read three bits: `$01` the probe looks
  rightward (a wall pushes back left), `$02` leftward, `$04` one-way, crumbling and spike tiles
  count as solid. The others are the game's and reach no ported handler.

## The change

- The hit probes return `bool`, true when they hit (a wall blocked, or a floor landed); `solid` and
  `below` return `bool`, true when solid (the ROM-table arm keeps its bit 7). Each probe's doc
  states its polarity. Callers lose every `& 0x80` on these results.
- `collision::probe` names the flag bits the handlers read: `RIGHTWARD = 0x01` (a wall pushes back
  left), `LEFTWARD = 0x02`, `FLOORS_SOLID = 0x04` (one-way, one-way-behind, crumbling and spike
  tiles count as solid); callers write their flags from them where those bits are meant, the rest
  of each byte a literal noted as the game's own bits. `weapons.rs`'s `phase << 1` stays computed
  (the phase's bit 1 becomes `FLOORS_SOLID`), with that said in a line.
- Out of scope: `probe_flags` (`$9F`, the damage and grounded bits, the survey's #9), the
  family-local probe-result accumulators in `patrol_walker` and `crate_soldier` (they become
  named bools with this change only where they hold a probe's result directly).

## Proof

The oracle tests keep their exact comparisons: the hit probes' results map back to the ROM's A
(`if hit { 0 } else { 0x80 }`) against the oracle's, the real check that they are only blocked or
clear; `solid` and `below` compare the N flag (their ROM-table arm can leave any byte in A). The
Classic suites, a sweep and a 300-branch exploration unchanged; tapes byte-identical.
