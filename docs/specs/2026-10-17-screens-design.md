# Levels as assets, slice 4: the screen records (1.1.x step B1d)

Status: approved with changes by the Opus stand-in, 2026-10-17 (applied: the record folds
into `rooms::Room`, no `Screen` type; the empty record has no arrival; the port already
matches the game's `$77` path; five callers; the arrival applied in the loader; names;
an asserting test; the full deletion list). Fourth slice of step B1
(`docs/specs/2026-10-17-levels-as-assets-design.md`), split off from the rooms
(`docs/specs/2026-10-17-rooms-design.md`) for the loader's reasons, from a read-only survey
of `room::parameters`, `actor/raster.rs`, `scroll.rs` and `actors.rs`.

## Goal

Each room has a screen record (bank 14 `$DA18[area]`, then `+ room`, both the raw even
`$0400`/`$0402` offsets), decoded inline by `room::parameters` on area entry (`stage.rs`
`Enter`), room start (`stage.rs` `enter`, `$BDE2`), both room changes (fade and slide,
`room.rs`) and the new-game state (`room::start`). It holds the room's raster split setup,
which metatile rows and attribute quads the column stream leaves alone, and how the player
arrives. Every caller runs after `load` of the same room. A second, unused decoder
(`actors.rs` `ScreenRecord` and friends) names the same bytes `header`/`block_a`/`block_b`;
only a dump tool and its own tests use it. This slice adds the record to the `Room` asset,
`parameters` writes from it, and the duplicate goes. Behaviour, RAM, WRAM and the tapes
unchanged.

## Design

- `rooms::Room` gains, decoded in `Room::import` with its existing closures
  (`pointer(14, 0xDA18, 14)`):
  ```
  pub raster: Option<Raster>,  // None: the one-byte `$77` record
  pub fixed_rows: [u8; 16],    // bytes 5-20 ($70-$7F): bit 7 keeps the metatile row
  pub fixed_quads: [u8; 8],    // bytes 21-28 ($60-$67): bit 7 keeps the attribute half-row
  pub arrival: Arrival,        // bytes 29-30, a handler pointer
  pub struct Raster {
      pub armed: bool,             // byte 0 ($D0) bit 7: the NMI arms the first IRQ
      pub first_irq_cycles: u16,   // bytes 1-2 ($D1:$D2), little-endian
      pub handler: u8,             // byte 3 ($D3): raster.rs's handler 0-13
      pub next_irq: u8,            // byte 4 ($D4): which IRQ of the chain runs next
  }
  pub enum Arrival { Unchanged, Held, Flying }  // $DA0E, $DA0F, $D9FF
  ```
  `armed` is a `bool` only if every record's byte 0 is 0 or `$80` (as every other `$D0`
  writer's is), else `control: u8` with only bit 7 read; `next_irq` goes if every record's
  byte 4 is 0; `fixed_rows`/`fixed_quads` become `[bool; N]` only if every record's bytes
  are 0 or `$80`. These are decided once, from a survey of every record during
  implementation, and the test asserts the invariant the chosen form depends on. The `$77`
  record is one byte: `import` stops there, with the fixed arrays zero and `arrival:
  Unchanged`, as the game's `RTS` does. An arrival pointer none of the three makes `import`
  return `None` (today's "not ported" panic becomes the import's `expect`).
- `parameters` takes the imported `Room` and makes today's writes in today's order: for a
  raster, `scroll.room` zeroed then `room[0..5]` from the fields; for none, `room[0] = 0`
  only (the game's `$D9B5` sets only `$D0`; the first area after boot keeps the intro's
  `$D1-$DC`, and the lockstep compares them); then `fixed_rows`, `fixed_quads`, and the
  arrival as a three-arm match in `room.rs` (`Held`: motion `Held`, `stage_script = $E0`;
  `Flying`: `anim = $85`, motion `Flying`, `stage_script = $A0`). `rooms.rs` imports
  nothing from `actor`: the asset holds data, the loader writes.
- `handler` stays a `u8`: the live `$D3` is also set by scripts (the intro, cutscenes,
  credits: 9, `$0A`, `$0C`, `$0D`), indexes `raster.rs`'s `FIRST` table and is read raw
  elsewhere, so an enum named by effect belongs with the raster/GPU step. `Scroll::room`'s
  name (it is the raster state) waits for that step too.
- Deleted from `actors.rs`: `ScreenRecord`, `screen_record`, `screen_record_at`,
  `block_offset`, `SCREEN_*`, `ScriptKind`, `script_kind`, the bank-14 screen bytes of the
  tests' `synthetic_prg`, the four synthetic screen tests (the real-ROM test covers them),
  the module title's "for a level screen", the intro's "two adjacent systems" paragraph,
  and the "5/16/8-byte screen blocks" bullet. The dump tool (`rotj-tools` `actors`, and its
  module doc) prints the imported `Room`'s new fields as one `{:?}` line.
- Docs: `docs/ACTORS.md` §7 intro (186-190) and the §7a table (its "(empty) zero `$D0-$DC`"
  row is wrong: the empty record sets only `$D0`), the "not exercised" note (357); the §7a
  heading stays (cited by `world.rs` and `docs/SUBLEVELS.md`).

## Tests

- `rooms::tests::every_listed_room_imports`: every room the screen tables list (each area's
  count runs to the next per-area table, the last to the first record) imports, which
  asserts the invariants behind the field types (`import` refuses a flag byte other than 0
  or `$80` and an unknown arrival).
- The Classic suites, a 1-seed sweep and a 300-branch exploration; the tapes unchanged.

## Plan

1. Survey every record for the type decisions. Result (2026-10-17): 21 records over 18
   areas, 10 empty; byte 0 is only ever 0 or `$80` (`armed: bool`); byte 4 is not always 0
   (`next_irq` stays); every fixed byte is 0 or `$80` (`[bool; N]`); every arrival is one
   of the three (8 unchanged, 1 held, 2 flying).
2. Tests first, then the `Room` fields and `import`; `parameters` writes from the room.
3. Delete the `actors.rs` duplicate, rewire the tool, the docs.
4. Checks, the `reviewer` agent, commit `Levels as assets: the screen records`.

## Not here

The raster handler as an enum and `Scroll::room`'s name (the raster/GPU step), collision
properties.
