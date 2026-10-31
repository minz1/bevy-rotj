# Level packs release 6: playing a pack

Status: implemented in 1.1.53. Approved by the stand-in (owner-proxy, L3) and revised after the reviewer's pass: plain
`std::fs` instead of `bevy_asset_loader` (MODERNIZE #14 moves to the in-game "Load pack" entry), a
folder without `pack.json` refused first, the `mirroring` enum and its values inside the owner's
approval, Modern's final-room check reading the stage's area through `&Levels`, at most 128 stages
and 128 rooms a stage, the test asserting the pack's levels are the ones played; and from the
reviewer: `Catalog::from_rom`, every argument parsed once, the enum appended last, a
`next_area_gate`'s boss checked, the original's six trailing exits listed. Follows
`docs/specs/2026-10-18-level-packs-design.md` ("Releases, in order" 6), the release 3 spec's open
question 4, and the owner's 2026-10-29 decisions (HANDOFF.md): `--pack <dir>` and dropping a pack
folder or its `pack.json` onto the exe, like the ROM; `--pack` refuses by name a pack with too few
stages or rooms or a bad extra password; the optional room `mirroring` property (absent = derived,
`format` stays 1), written by the exporter where the derived rule is wrong, with
`Levels::original` dropping that part of its overlay; which area-keyed ROM tables follow
`stage.area` is the stand-in's call.

## What it adds

1. **Playing a pack**: `batman-rotj --pack <dir>`, or a pack folder or its `pack.json` dropped
   onto the exe. The game plays the pack's levels with the player's ROM for everything else.
2. **Refusals at load**: a pack the game would run past (too few rooms or stages, too many for a
   byte, a boss the ROM has no tables for) is refused by name before the window opens, like a
   missing ROM.
3. **The room `mirroring` property**: 1-1-2's start mirroring travels in the pack;
   `Levels::original` overlays only the schedule residue.
4. **Area-keyed tables**: the one left keyed by stage number follows `stage.area`.

No game behaviour changes for the original game: the tapes, chain and lockstep stay unchanged.

## Launching a pack (app)

`main` parses the arguments once, before the app is built, into an optional ROM path and an
optional pack folder:

- `--pack <dir>` names the pack folder.
- Any other argument that is a folder, or a file named `pack.json` (any case, for Windows), is the
  pack: its folder. Dropping a pack and a ROM together works in either order.
- The first remaining argument is the ROM path, as today: `Rom::load` gets it in a build without
  an embedded ROM; a build with one ignores it.
- Two packs, or `--pack` without a folder, is an error.

The pack:

- **Read** by `rotj_core::levels::read(dir) -> Result<PackFiles, String>`: a folder without a
  `pack.json` is refused at once, naming it; otherwise every `.json`, `.tmj` and `.tsj` file under
  the folder, keyed by its path relative to the folder with `/` separators. Plain `std::fs`, no
  Bevy. `rotj-tools`' `write_pack` moves beside it as `levels::save(dir, files)`, the pair.
- **Imported** with `import(&files, &Catalog::from_rom(prg, chr)?, Source::User)`
  (`Catalog::from_rom` carries the sound test's songs release 5 allows).
- **Any failure** (no `pack.json`, unreadable, an `ImportError`, a refusal below) prints
  `batman-rotj: <dir>: <error>` and exits 2, with Windows' "Press Enter to close", as the ROM
  error does: one shared helper for both.
- The levels go into the app as a resource (`GameLevels(Levels)`, the pack's or
  `Levels::original`), built once in `main`; `power_on` clones it into `Prg::new` instead of
  calling `Levels::original` itself.
- `ROTJ_LEVEL` is checked against the levels' checkpoint count instead of the fixed 13, so a pack
  with fewer checkpoints warns instead of panicking at `level_area(..).expect` (boot.rs).
- The tilesets are still written from the ROM at startup. The startup writer draws `custom.png`
  only for packs under `<settings dir>/packs/` (release 5); a pack played from elsewhere plays the
  same, but Tiled shows its custom metatiles only once the pack is copied there.

### `bevy_asset_loader`: not used (stand-in, RULING [review])

The release 3 spec moved MODERNIZE #14 here. Plain `std::fs` is used instead:

- The refusals must name the pack's problem before the window opens, like the ROM error (the
  owner's "like the ROM"). In a `LoadingState` they arrive after the window is up and need an
  in-game error screen.
- The file list is dynamic (`pack.json` names the rooms), so the collection is either two-phase
  (dynamic assets) or a folder load with a typed text loader that also picks up a pack's PNGs;
  both are more code than `read`, and the importer wants `PackFiles` anyway.
- A pack is loaded once, at launch: no hot reload or streaming to gain. The original game has no
  loading state, so its boot timing is untouched.

MODERNIZE #14 is not closed: its row moves to "with the Load pack menu entry and the editor
(loading in game, hot reload)", and the release 3 spec's pointer says so.

## Refusals

The import already refuses an extra password naming an unknown stage or a stage that is not a
checkpoint, a first stage without a password, and a last stage whose `on_clear` is `next-stage`
(releases 1-5). New, for `Source::User` only, in `import`, each naming the file, layer and object
like release 5:

1. **Too few rooms**: a stage's last room holding an object whose family asks for the next room
   (`room_exit`, `top_screen_room_exit`: `$BA` = 1, room + 2, `room.rs` `enter` and `load_room`)
   (`rooms/2-1-3.tmj: layer spawns object 12: a room_exit in the stage's last room`).
2. **Too few stages**: the last stage holding an object whose family asks for the next stage
   regardless of `on_clear` (`next_area_gate`, `next_area_cue`: `$BA` = 3; `stage_end_trigger`:
   2, then 3 unconditionally, `stage.rs` `stage_end`). `boss_defeated_script` reads `on_clear` and
   `final_boss_defeat` goes to the ending, so both are covered.
3. **Too many**: more than 128 stages, or more than 128 rooms in a stage (`world.area` and
   `world.room` are bytes stepped by 2, and `level_area` doubles a `u8`).
4. **An unknown boss**: a `next_area_gate` whose `health` (the boss index, `stage_args[1]`, which
   `BossIntro` uses to index the ROM's `$BCA6` and `$BE4E` tables) is not one an original gate
   uses. The catalog carries the set, read from the ROM, like the palettes and songs.

These are static: an object counts whether or not its trigger cell is reachable. A stage that
never reaches an exit just never ends; that is level design, not a refusal. The original pack
(imported as `Source::Original`) is not checked: six of its stages' last rooms hold exits the game
never reaches (`3-1-4` room exits 3 and 11, `3-2-2` 12, `3-3-1` 3 and 11, `4-3-1` 9, `5-1-4`
top-screen exit 1 and room exit 28, `6-3-1` 3 and 15). A user pack remixed from the export is
told about each by name and drops it.

## The room `mirroring` property

- An optional map property `mirroring` on a room, typed by a new project enum `mirroring` with
  the values `side-by-side`, `stacked`, `single-low`, `single-high` (`Mirroring`'s `SideBySide`,
  `Stacked`, `SingleLow`, `SingleHigh`), stand-in RULING [review]. Absent: the derived rule as
  today (vertical room: `side-by-side`; horizontal with camera y >= `$1000`: `stacked`; else
  `single-low`), now one `derived_mirroring` helper beside `derived_first_spawn_column`, shared by
  the importer and the exporter.
- The exporter writes it only where `start.mirroring` differs from the derived rule: 1-1-2
  (`single-low` in a vertical room) in the original game.
- `Levels::original` drops its mirroring overlay loop; the schedule residue overlay stays.
- `format` stays 1. A pack without the property imports as today.
- The enum is appended last in the project file, so no other type's id moves. The exported
  original pack changes in two places: 1-1-2's map gains the property and the project file gains
  the enum. No digest pins the export's bytes. Tests that change: `RESTORED_MIRRORING` and its
  overlay checks (levels/tests.rs, the equivalence test and
  `the_original_levels_are_the_imported_export_with_the_roms_schedules`) now expect no mirroring
  overlay; the round trip stays byte for byte with 41 files.

## Area-keyed tables (stand-in RULING)

The `world.area` reads that are not `Levels` lookups:

| Read | Keyed by | Ruling |
| --- | --- | --- |
| `hurtful_burst`'s metasprite entry (`area == 0x0A`) | `stage::at(..).area` (the stage's `area * 2`) | stays: already follows `stage.area` |
| Modern's `view`: the final boss room is all runs (`world.area == FINAL_AREA`) | the stage number | follows `stage.area`: `stage::at(levels, world.area).area == FINAL_AREA`, the caller passes `&Levels` |
| The stage-end boss tables (`$040E-$040F`, `stage_args`) | the `next_area_gate` actor's `subtype` and `health` | stay: they follow the actor a pack places (refusal 4 bounds the boss) |
| `Descriptor::from_rom`'s `on_clear` (`FINAL_AREA`) | the ROM's area | stays: ROM-side, the pack's `on_clear` replaces it |
| `ram[0x400] = area` (the WRAM projection) | the stage number | stays: what proves Classic |

Everything else the engine reads by area (music, banner, picture, password, terrain, CHR, the
screen record, spawns, room loads) goes through `Levels` since releases 2 and 3.

## Tests

- `levels::read` and `levels::save`: a saved export reads back to the same `PackFiles`; a folder
  without `pack.json` is refused by name.
- **A pack through `--pack`** (app): the test pack is the exported original made a valid user
  pack: `no_handler` objects dropped, the 13 rooms' map-held entries on unwritten pages remapped
  onto written ones (release 5), and the six trailing exits above dropped. The remap is lossy: a
  remapped entry's objects share the target page's schedule cells, so the objects and locks that
  collide are dropped too, each asserted to lie in a stage the remap touched (stand-in RULING
  [review]). The refusals other than those collisions are exactly nine of the trailing exits;
  `4-3-1`'s exit 9 collides on its remapped page first. It is saved to a temp
  folder; the argument parser, `read` and the import take it as `--pack <dir>` and as a dropped
  `pack.json`; the app's levels are asserted to be that import and to differ from
  `Levels::original` (a dropped object is absent); the game powers on with them and plays from
  each checkpoint's start (`forced`) to its first gameplay frame, and stage 1-1 for a few hundred
  frames, without a panic.
- **One test per refusal**: too few rooms, too few stages, too many stages and rooms, an unknown
  boss (new); the extra password's unknown stage and non-checkpoint (existing). Each error names
  the file and object.
- The argument parser, table-driven: `--pack <dir>` first or last, `--pack` naming a
  `pack.json`, a folder, a `pack.json`, a ROM and a pack in either order, a ROM alone, nothing,
  two packs.
- Mirroring: the equivalence and round-trip tests cover the property without the overlay; one
  small test that a room without it derives.
- Modern's `view`: the final boss room test passes the levels (the test at the end of modern.rs).

## Not here

- The "Load pack" menu entry (owner, 2026-10-29: later, with the editor and the overworld), and
  with it `bevy_asset_loader` (MODERNIZE #14) for loading in game and hot reload; the
  stage-settings panel, the menu and the overworld; packs for the Modern engine.
- The aliased-byte write-back MODERNIZE ties to level packs (a running slot 16+c's writes to
  slot c's aliased bytes undone by `store_aliases`): it needs an oracle test before any change,
  so it is its own item after this release; MODERNIZE's lead and the release 3 spec point there.
- Recording input with a pack: `ROTJ_RECORD_INPUT`'s tapes do not name the pack, so a tape
  recorded on a pack replays only against that pack; the recorder is unchanged.

## Open questions

None for the owner. The stand-in's [review] rulings (no `bevy_asset_loader`; the `mirroring`
enum's names) are listed in the run's `## Owner` for a later look.
