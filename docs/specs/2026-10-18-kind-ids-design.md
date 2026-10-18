# The spawned kinds named

Status: approved by the Opus stand-in, 2026-10-18, as the second half of the survey loop's #7
(the first, `docs/specs/2026-10-18-sound-ids-design.md`). No behaviour change: constant
substitution, proven by the lockstep.

## What is there

17 spawned kinds are written as bare literals: 15 actor kinds (`kind = 0x..` and `Brain::new(0x..)`
in the families, `engine.rs` and `collision.rs`) and two projectile kinds (`0x06`, the splitter's
part in `weapons.rs`; `0x0A`, the three-way's parts in `player.rs`). About 40 families already name
their child kinds with a local `CHILD`/`*_KIND` const.

## The change

- Local private consts, named by the child's family, minus the spawner's own prefix, used in both
  the `kind =` and the `Brain::new(..)` writes:
  - `engine.rs`: `DEFEAT_SCRIPT`, `BURST`, `PICKUP`; `collision.rs`: `CRUMBLING_BLOCK`;
  - `final_boss.rs`: `DEFEAT`, `FALLING_SHOT`, `FALLING_PART`, `BOUNCER`, `EXPLOSION`;
  - `stage6_boss.rs`: `PART`, `DEATH_PART`; `ace_ranger.rs`: `PART = 0x3A`, `SHELL = 0x3C`
    (`ace_ranger_shot`'s own words);
  - the rest by the same rule.
- The projectile kinds go beside `weapons.rs`'s `homing_stage`/`super_stage` modules
  (`splitter_stage::PART`, `three_way_stage::PART` or the existing modules' names where one fits).
- `bomber_bomb.rs`'s test literal stays (test data).
- Out of scope: renaming the `spawn_3a`/`spawn_b8` functions after the consts (a later naming pass).

## Proof

The lockstep compares every slot's kind each frame; the diff swaps literals for consts of the same
value. The Classic suites, a sweep and a 300-branch exploration unchanged; tapes byte-identical.
