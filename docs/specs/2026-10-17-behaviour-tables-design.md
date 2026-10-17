# Behaviour tables as typed data: the storage, and slice 1 the hitboxes (1.1.8)

Status: approved with changes by the Opus stand-in, 2026-10-17 (applied: a ROM-free test
`Prg`; the missed `Cx` builders and `replay` in the cascade; `exit_*` for the gate pair; no
`Debug`; the leftover-`scratch` note; `Play` builds the `Prg` itself; a cached test `Prg`). While implementing, `touch`'s
table was found and added, and `Hitbox` lost its projectile width to separate arrays so
one type serves all four tables. From `docs/MODERNIZE.md` "Runtime ROM
tables" (the survey of about 390 `rom_byte`/`rom_word` reads) and a read of every hitbox
site and every `work.hitbox` write.

## What this is, and is not

The handlers read their behaviour tables (hitboxes, velocities, offsets, the player's and
the weapons' tables) out of the PRG at run time, by bank and CPU address. This spec makes
the one storage decision for every group, so slices 2 (the family constants) and 3 (the
player and weapons) only add fields, and moves the first group, the hitboxes, to typed
data. It is not the authored level format (owner-reserved): the tables are imported from
the PRG; a later loader for authored content would build the same struct from its assets.
The metasprite pointers, the text and the pointer chases stay ROM reads (the HANDOFF).

## The storage decision

ROM constants can't live in `World` (liftable from RAM alone), and with the mirror decided
the core keeps no Bevy resources. Rejected: a global `OnceLock` (hides the data flow, one
ROM per process); tables generated into the source at build time (ROM-derived data in the
build, and public CI has no ROM).

- **`rotj_core::prg::Prg`**: the PRG image plus the tables imported from it, built once by
  `Prg::new(bytes: Vec<u8>)`, which runs each group's import. It owns the bytes. It
  implements `Deref<Target = [u8]>` (as `bytes::Bytes` does): a `Prg` is the image with its
  tables attached, so every leaf that reads bytes (`rom_byte`, `bank_slice`, the level and
  room importers, the music driver) keeps taking `&[u8]` and is called unchanged. No
  `Debug`: nothing needs it, and both the image and the tables are ROM bytes. A PRG too short for an import panics as `rom_byte` does today (the app
  only builds one from an MD5-checked ROM).
- **Where it replaces `&[u8]`**: `Cx::prg` becomes `&'a Prg`, so `Game::parts`, the handful
  test-built `Cx`s (`engine.rs`, `player.rs` twice, `weapons.rs`, `column_eraser.rs`), and
  every function on the path from an entry point to them take `prg: &Prg` (`Game::frame`,
  the scripts' `step`s, `room::start`, the attract and boot runners, the oracle's
  `Scene::prg()`, `Routine::prg`, `run_game` and `replay`). A function that only reads bytes
  keeps `&[u8]`. Each later slice moves a function to `&Prg` only when it needs a table.
- **Callers build it once**: `Play::running` builds the `Prg` it holds from the `Vec<u8>`
  `power_on`/`start` still take, so no app caller changes; `Scene::prg()` (already an owned
  clone) returns `Prg::new(..)`; tests share `prg::for_test()`, a `OnceLock`-cached `Prg` as
  `rotj_rom::for_test` caches the ROM. The ROM-free test that builds a `Cx` (`engine.rs`)
  uses a `#[cfg(test)]` empty `Prg` with zeroed tables.
- **The tables are plain fields** on `Prg`, each group's type in the module that reads it
  (`prg.hitboxes: engine::Hitboxes`). Slice 2 puts the family tables under one field (e.g.
  `prg.families`, perhaps from `families!`) rather than about 40 top-level ones. Each
  import reads its addresses with `rom_byte`, sized to the logical table; indexes are `u8`
  RAM values turned into `usize`, so a bounded array panics where the ROM would read the
  next table. Each slice checks its indexes' real range first (below for this one).

## Slice 1: the hitboxes

`engine::Hitbox { half_width: u8, height: u8 }`: how close in x the player must be, and how
far above the actor's anchor the player or a projectile may be (below it, the player's own
`reach_y` or the fixed 5 or 3 decides).

`engine::Hitboxes`, each by `work.hitbox` unless noted:
- `actors: [Hitbox; 15]` (`$ABB2`, `$ABC1`) and `actor_projectile_half_widths: [u8; 15]`
  (`$ABD0`); bank 13, back to back, 15 bytes apart.
- `bosses: [Hitbox; 5]` (`$B0F9`, `$B0FE`) and `boss_projectile_half_widths: [u8; 5]`
  (`$B103`); 5 apart.
- `hazards: [Hitbox; 7]` (`$AF2A`, `$AF31`, 7 apart): `touch` (`$AEE3`), the contact of
  actors that only hurt. Found while implementing: the survey's 12 reads missed it.
- `exit: Hitbox` (`$A0A7`, `$A0A8`): `player_proximity`'s reach (two of its three readers
  are room exits, the third the next-area gate).
- `platform_half_widths: [u8; 2]` (bank 14 `$CE7D`, by the second-class `scratch`): how
  close in x an actor must be to ride a second-class platform.

The sites: `overlaps` takes a `Hitbox` (its two table addresses and `prg` go); `contact`
passes `actors[id]`, `boss_contact` `bosses[id]`, `touch` `hazards[id]`; the projectile
loops read their `*_projectile_half_widths[id]` and the same entry's `height`;
`player_proximity` reads `exit`; `collision::ride` reads `platform_half_widths`. The `$CE79`
riding-handler pointer stays a ROM read (a pointer chase).

### The indexes' range (read from every write)

- `work.hitbox` (`$A5`) is written only by the families: 0-6, 9, `$0A`-`$0D`, the
  pickup's 9 and `hover_diver`'s `frame + 2` (at most 9: `frame` is capped at 6, plus 1
  rising). Every one fits the 15 actor entries.
- Every `boss_contact` caller sets 0, 1, 2 or 4 just before the call: in the 5 boss
  entries.
- Every `touch` caller sets 0, 1, 3, 4, 5 or 6 in the same step before the call: in the 7
  hazard entries.
- The three `player_proximity` callers (the two room exits and the next-area gate) each set
  0 just before it, so `$A0A7` is only ever read at entry 0, and `$A0A8` (entry 1) is the
  height. `exit` drops the index; a `debug_assert_eq!(work.hitbox, 0)` records the
  rule, and the callers' `work.hitbox = 0` stays (it is RAM).
- Second-class `scratch` is set to 0 or 1 (`camera_floor` 1); `$CE7F` is code, so the
  table is 2 bytes. Spawning doesn't reset it and the scroll step runs before the actors, so
  `ride` can read a new slot's leftover `scratch`: still 0 or 1, since every writer sets 0
  or 1 and `Slots::default()` starts at 0.
- The oracle tests lift `$A5` at a routine's entry from the ROM's own run, so a lifted
  index is one the game itself wrote.

The full test set exercises these paths; a panic there means a range this audit missed.

## Tests

- The Classic suites (every hitbox site is under a family's oracle test or the chain), the
  tape tests, a 1-seed sweep and a 300-branch exploration; the tapes unchanged.
- No test restates the importer's addresses: the lockstep tests prove them.

## Plan

1. `prg::Prg` (`new`, `Deref`, the test `for_test` and empty `Prg`), `engine::Hitboxes::import`; `Cx::prg` (its doc too) and the
   path to it on `&Prg`; the app, the oracle and the tests build one. No table read moves
   yet; everything green.
2. The hitbox sites onto `prg.hitboxes`; grep that no `$ABB2`/`$ABC1`/`$ABD0`/`$B0F9`/
   `$B0FE`/`$B103`/`$AF2A`/`$AF31`/`$A0A7`/`$A0A8`/`$CE7D` read remains outside the import.
3. Checks, the `reviewer` agent, commit.
