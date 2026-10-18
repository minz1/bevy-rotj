# The empty slot, named

Status: approved with changes by the Opus stand-in, 2026-10-18 (applied: no `Inert` family, the
ROM has no handler for those kinds; the 1.1.15 assert kept, reworded; `engine::unported` deleted,
the `Empty` arm unreachable; the level packs' `inert_platform` renamed `no_handler`; whether user
packs may name it deferred to the owner). MODERNIZE #3 (`Brain::Unported`), approved for the survey
loop by the owner's review (2026-10-18). No behaviour change in the game.

## What is there

- `Brain::Unported` and `SecondBrain::Unported` are each enum's `#[default]`: the brain of an empty
  slot (and of the slot the actor loop has taken out to run, which `shell_spawner::own_slot` finds by
  it), and the fallback `Brain::new`/`SecondBrain::new` give a kind no family claims.
- Main slots: every kind `$02`-`$C8` has a family since 1.0.0; a kind past them would index past
  the ROM's `$CF72` dispatch, and neither the game's data nor a pack can produce one, so
  `engine::unported` (a far-off despawn standing in for a missing handler) is dead code that invents
  behaviour.
- Second-class slots: `$CE7F` dispatches through the `$CEC3` table, which has entries for kinds
  `$00`-`$10` only; kinds `$4A`, `$88` and `$E0` would jump through the bytes after it. They occur
  only as record ids past the end of area 17 room 0's record table (pending cells naming non-record
  bytes), which play never reaches; the 1.1.15 test-only assert that no live second-class slot is
  unported is right.

## The change

- `Unported` becomes `Empty` in both enums, documented as the empty slot's brain (and the taken-out
  slot's, for `own_slot`), and the fallback for a kind with no handler.
- `engine::unported` and its test go; `step`'s `Empty` arm is `unreachable!` naming `$CF72` (the
  project's rule for unreached paths).
- The 1.1.15 assert reads "a live second-class slot's kind has no `$CEC3` entry"; the oracle
  tests' `!= Unported` checks become `!= Empty`.
- The level packs' `inert_platform` class becomes `no_handler` (beside `no_actor`), documented as
  ids past area 17 room 0's record table with no `$CEC3` entry, which play never reaches.
- Deferred to the owner: whether the importer should refuse `no_handler` in packs other than the
  original game (placing one would reach the `unreachable!` once the game loads packs).

## Proof

The Classic suites, a sweep and a 300-branch exploration unchanged; tapes byte-identical.
