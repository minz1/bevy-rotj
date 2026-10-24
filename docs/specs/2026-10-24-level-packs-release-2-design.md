# Level packs, release 2: the pack side (the stage knobs)

Status: approved with changes by the Opus stand-in, 2026-10-24 (applied: `spawn_row` among bit 6's
readers and one ROM fact asserted; the payload read via `boot`; name resolution, duplicate passwords
and the last stage's `on_clear` refused; one `first_spawn_column` rule; `format` stays 1;
`BIT_6_CELLS`; `Source` threaded to `import_spawns`). Names approved: `first_spawn_column`,
`extra_passwords`. Implemented (the release adds its version here).

The pack side of release 2 of `docs/specs/2026-10-18-level-packs-design.md`; the engine side
(`actor::stage::Stage`, `Prg::stages`) shipped in 1.1.23
(`docs/specs/2026-10-18-stage-descriptor-design.md`). This release puts the descriptor's fields in
`pack.json`, per the owner's seven decisions of 2026-10-23 (STATUS "Owner decisions made"). The
game still does not load packs (release 3), so nothing it does changes: tapes unchanged by
construction; the change is `rotj_core::levels`, its tests and docs.

## What changes in the format

Per stage in `pack.json` (decision 4's approved names; values are names, never addresses):

| key | values | engine field | absent |
|---|---|---|---|
| `hud_label` (exists) | `"d-d"`, two decimal digits | `banner` | refused |
| `music` | `"song-xx"` (song id in two-digit lower hex, below `$80`), `"keep"` | `music` | refused |
| `cutscene` | `"picture-xx"` (picture id, below `$80`) | `picture` | none |
| `password` | four letters `A`-`Z` | `password`; a stage with one is a checkpoint (Continue, the pause's letters, the levels) | not a checkpoint |
| `on_clear` | `"next-stage"`, `"ending"` | `on_clear` | refused |

The exporter writes `cutscene` and `password` only where the stage has them (serde
`skip_serializing_if = "Option::is_none"`), the others always. The importer refuses a malformed
value by name (file, stage, key), a pack whose first stage is not a checkpoint (the engine's
`checkpoint` needs one), a pack whose last stage's `on_clear` is `next-stage` (the engine would load
a stage past the end), and a password used twice across the checkpoints and `extra_passwords`
(`check_password` takes the last match, so the earlier place could never be reached).

`format` stays 1: no pack has been published, so a release-1 pack that lacks `music` or `on_clear`
is refused by name, as intended.

Pack level (decision 7, name proposed here): `extra_passwords`, a list of
`{ "password": "ABCD", "stage": "7-1" }`, the password screen's passwords the pause never shows,
each starting the named stage, which must be a checkpoint. Written only when non-empty. The
original game's pack has one: the screen's fourteenth (`$6E6C` place 13), starting the 13th
checkpoint (`check_password`'s `place.min(12)`). The importer refuses an `extra_passwords` stage
name that matches no stage, or more than one. Its letters are 0-25 in the payload; the pack
writes them as `A`-`Z` like `password`.

Per room (decision 1, name proposed here): an optional room property `first_spawn_column` (int),
a schedule column 0-511 in the pack's own terms (entry × 16 + column, as `triggers`), read
through the room's page table (`schedule_cell(pages, column)`) to the engine's byte, parsed with the
same `column()` (0-511) check and error as `triggers`. Absent: the derived rule as today, moved out
of `import_room` into one helper in `levels.rs` that the importer and the exporter share. The exporter writes it only where the ROM's value differs from the derived
rule (area 11 rooms 2 and 3, `rooms/5-1-3.tmj` and `5-1-4.tmj`, ROM `$00` and `$70`), as
`(0..512).find(|&c| schedule_cell(pages, c) == value).expect(..)`. The equivalence test drops
`FIRST_SPAWN_COLUMN_PENDING`: all 36 rooms compare every field.

Dropped or kept out (decisions 5, 6): `sprite_palettes` is dropped from the design (rooms carry
their sprite palette); `boss` is an editor-wizard field, not runtime format, so neither the
exporter nor the importer knows it (an unknown `boss` key survives in `extra` as any unknown key
does).

## `no_handler` (decision 3)

`import(pack, catalog, source)` gains `source: Source`, a new `levels::Source { Original, User }`:
with `User` the importer refuses a `no_handler` object by name (file, layer, object id: "the game
has no handler for it"); `Original` keeps it, for the original game's export (the proofs, and
release 3's original-via-pack path). `Source` derives `Clone, Copy, Debug, PartialEq, Eq` and is
threaded through `import_stage` into `import_spawns`, which refuses after `kind_of` resolves the class
to `no_handler`. An `import_original` twin is the same size and less explicit; exporting without
`no_handler` would break "only the original game's export keeps them". `rotj export-levels` is unaffected; tests that import the
original pass `Original`, the hand-made fixture `User`, plus a test that `User` refuses the
original game's pack naming a `no_handler` object and `Original` takes it.

## Bit 6 of the 24 stage 7-2 cells (decision 2)

Not carried: no schema change; the cells keep importing as `$80 | id`. A test shows nothing reads
the bit: the schedule's readers are `spawn_at` and `spawn_row` (bit 7, then the id; `edge_spawn` in
scroll.rs tests bit 7 and calls `spawn_at`), `lock_at`
(`$40` exactly, so `$C0 | id` is not a lock), `despawn` (ORs into a cell already cleared to its id),
and `collision::class_at`, which reads cells `$00-$2F` raw as a metatile's class for ids `$D0` and up
that the level has no class for (the class table holds ids below `$D0`). Five of 7-2's bit 6 cells
lie in `$00-$2F` (`$1A`, `$1D`, `$1F`, `$29`, `$2B`), but its stream uses no id `$D0` and up, so
`class_at` never reads them; the test asserts that from the ROM, and that `spawn_at` and
`spawn_row` on `$C0 | id` and on `$80 | id` give the same slots and leave the same cell, and
`lock_at` the same locks. `PENDING_BIT_6` becomes `BIT_6_CELLS` (nothing is pending), keeping its
count assertion; its doc comment and the release-1 spec's pending note say why it is dropped. Release 3's
lockstep sees these bytes at `$7410`, like the residue; its spec treats both.

## The types

- `levels::Stage { name: String, area: u8, descriptor: actor::stage::Stage }`: `hud_label`
  goes (the descriptor's `banner` carries it; the exporter writes `"{}-{}"` from it).
  `descriptor.area` is `area * 2` (the engine's `$0400` units), set by the importer, never
  authored. `Levels::from_rom` sets `descriptor = Stage::from_rom(prg, area)`.
- `Levels { stages, extra_passwords: Vec<ExtraPassword> }`, `ExtraPassword { letters: [u8; 4]
  (ASCII, as `Stage::password`), stage: usize (index into `stages`) }`. `from_rom` reads place 13
  of the payload's 14 through `boot::payload_byte` and `boot::PASSWORDS` (made `pub(crate)`; not
  `cutscene::payload_byte`, which maps the payload differently), its stage the 13th checkpoint's.
- `model::PackStage` gains `music: String`, `cutscene: Option<String>`, `password: Option<String>`,
  `on_clear: String` after `hud_label`; `PackFile` gains `extra_passwords:
  Vec<model::ExtraPassword { password: String, stage: String, extra }>` after `stages`, with
  `default` and `skip_serializing_if = "Vec::is_empty"`.

## Proof

- Equivalence: for all 18 stages, `import(export(rom)).stages[i].stage.descriptor ==
  Stage::from_rom(prg, i)` (every field: area, banner, music, picture, password, on_clear), and
  `extra_passwords` equal `from_rom`'s; every room field of all 36 rooms, the two
  `first_spawn_column`s included.
- Round trip: `export(import(export(rom)))` byte-equal, as today.
- Unit tests without the ROM: each knob's parse and refusal (`song-80`, `picture-zz`, `abcd`,
  `"10-1"`, unknown `on_clear`, a first stage with no password, a last stage with `next-stage`, a
  password used twice, an `extra_passwords` stage that is unknown, ambiguous or not a checkpoint), `first_spawn_column` through a page table, `Source::User` refusing
  `no_handler`.
- The full set: the Classic suites, a sweep and the exploration unchanged; the chain's tape
  byte-identical (nothing in the game reads `levels` yet).

## Docs

- `docs/specs/2026-10-18-level-packs-design.md`: the `pack.json` example and knob list (no
  `sprite_palettes`, `boss` as an editor field), with "Implemented in 1.1.N".
- The release-1 spec: its "Pending the owner" notes answered.
- STATUS: the row and the Next paragraph; the decisions are already under "Owner decisions made".
- `## Owner` in the run plan: the two proposed names (`first_spawn_column`, `extra_passwords`) for
  the owner's review, as decision 7 asks.

## Not here

The game loading packs and the schedule residue's proof (release 3); validating that `song-xx` and
`picture-xx` name a song or picture the game has (release 5's `classic` validation); the `boss`
wizard (the editor).
