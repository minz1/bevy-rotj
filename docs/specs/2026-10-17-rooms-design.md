# Levels as assets, slice 3: the rooms (1.1.x step B1c)

Status: approved with changes by the Opus stand-in, 2026-10-17 (applied: what reaches RAM
corrected; the test retargeted; `rows_from_rom` folded in; `pages` masked; named start
fields; `background_palette`; the whole `palettes.rs` deleted with its one unique check
moved; stale docs listed; the orientation readers completed, with a predicate, as its own
commit; kept palette bytes 17-19). Third slice of step B1
(`docs/specs/2026-10-17-levels-as-assets-design.md`, "Next slices" 2), from a read-only
survey of `actor/room.rs`, `collision.rs`, `palettes.rs` and `actors.rs`. The raster
parameters (`room::parameters`, the `$DA18` screen record, `actors.rs::screen_record`'s
duplicate) are slice 4: their bytes are partly understood, they panic on unknown handlers
and they load on a different path (`start()` and the transitions).

## Goal

A room's layout data is decoded inline in `room::load` (`$B40A`) on every room entry: the
room record (bank 0 via bank 14 `$C048[area]`: background palette selector, four CHR banks,
orientation, a page count), the sprite palette set (bank 13 `$B5F5[area]`, by `room >> 1`),
the page order (`collision::rows_from_rom`, bank 14 `$C024[area]`: 32 page numbers, held
in RAM `<< 4`) and the start record (bank 13 `$B653[area]`, 15 bytes). This slice makes
them one `Room` asset that `load` builds the game's state from. Behaviour, RAM, WRAM and
the tapes unchanged. Most fields land in RAM as they are (`$DD`, `$0440-$045F` shifted, the
start record's targets, fixed CHR banks in `$E0-$E3`); the palette selectors, animated CHR
handles and the page count do not, and the lockstep checks them through their effects
(the colours `build_palette` derives, the banks `animate_banks` writes, the attributes
`raise_attributes` edits).

## Design

- `rotj_core::rooms` (new module, beside `terrain`; `actor::room` is the loader that uses
  it):
  ```
  pub struct Room {
      pub background_palette: u8, // record byte 0
      pub sprite_palette: u8,     // $B5F5[area][room >> 1]
      pub chr: [u8; 4],           // record bytes 1-4 ($E0-$E3); bit 7 an animation handle
      pub orientation: u8,        // record byte 5 ($DD)
      pub raised_page_count: u8,  // record byte 6, read only in two-page rooms
      pub pages: [u8; 32],        // page numbers 0-15, in order
      pub start: Start,
  }
  pub struct Start {
      pub player_x: u16, pub player_y: u16, pub camera_x: u16, pub camera_y: u16,
      pub ring_y: u16, pub scroll_lock: u8, pub scroll_mode: u8, pub mirroring: u8,
      pub first_spawn_column: u8, pub facing: u8,
  }
  ```
  `Room::import(prg, area, room) -> Option<Room>`, `area`/`room` the raw `$0400`/`$0402`
  values as `SpawnTable::import` takes them (`room` already doubled). The palette selectors
  stay indices (the game indexes its tables with them; colours are the palette feature's);
  `chr` keeps the stored bytes (an enum would need all seven low bits to round-trip a
  zero-frame group's raw handle, for one consumer). `pages` holds `byte & 0x0F` (the high
  nibble is lost in `page << 4` today; masking is behaviour-neutral and an editor asset
  should not carry it).
- `load` imports the room and writes from it in today's order (CHR before
  `animate_banks`; rows and orientation before `raise_attributes`); the palette's kept bytes
  17-19 and `set_first_colours` stay in `load` (state carried across loads). The `Start`
  values are initial values: the game mutates them in RAM afterwards. `rows_from_rom` is
  folded into `Room::import` and deleted from `collision.rs`; `spawn_first_row`'s
  `rows[page << 4]` panic is unaffected.
- `palettes.rs` is deleted whole (`Record`, `palette_for_area`/`_level`,
  `chr_banks_for_area`/`_level`, `base_palette`, `resolve_chr`, `BackgroundAnimation`,
  `x_selector`, its constants, `pub mod palettes`): nothing outside its own tests uses it
  (the app's and `rotj-verify`'s `PALETTE_BYTES` is `render::PALETTE_BYTES`). Its one unique
  check, the oracle's frame-624 `$E0-$E7` and frame-744 palette for all 13 levels pinned to
  the `stagearea-probe` digests, moves to `oracle.rs` as a small test; the decoder half is
  redundant, the whole-game chain loading every level's rooms. The docs that name it point
  at `rooms::Room`, `room::build_palette` and `room::animate_banks` instead:
  `docs/STATUS.md:38` (which also names the gone `Stage::advance_background`),
  `docs/LEVELDATA.md` §14/§16 (lines 910, 967, 1015, 1137, 1142), `docs/SUBLEVELS.md:278`.
- **Orientation bits, its own commit in the slice:** `pub mod orientation` in `world.rs`
  beside `pose` (`World` owns the field): `TWO_PAGES = $80`, `VERTICAL = $40`, and one
  predicate for the five `& $C0 == $80` tests (`room.rs` 330, 469, 695; `engine.rs` 217,
  397), named for what `engine.rs` calls it (two pages across, then vertical). Every
  reader: `scroll.rs`, `engine.rs` (including the `& $C0 == $C0` tests at 102 and 344),
  `collision.rs`, `room.rs`, `player.rs:485`, `families/next_area_gate.rs:49`. The app's
  `play.rs` reads of the raw byte stay.
- `Room` is not stored on `Game` (liftable from RAM); importing on each load is today's cost.

## Tests

- `rows_from_rom_matches_every_room_load` (oracle.rs) is retargeted to
  `Room::import(..).pages` (shifted) and asserts every page is below 16. The `Start` fields,
  `orientation` and the fixed `chr` bytes join it only if its capture point is after the
  loader's last start-record write (the `$B40A` exit); otherwise the lockstep, which checks
  every load byte-exact, covers them.
- The moved `captured_level` digest test.
- The Classic suites, a 1-seed sweep and a 300-branch exploration; the tapes unchanged.

## Plan

1. Orientation bits and predicate at every reader; checks; commit.
2. `rooms::Room`, `Room::import`; `load` builds from it; `rows_from_rom` folded in; the
   oracle test retargeted.
3. Delete `palettes.rs`, move its digest check, fix the docs.
4. Checks, the `reviewer` agent, commit `Levels as assets: the rooms`.

## Not here

The raster parameters and screen record (slice 4), the CHR animation groups, the palettes'
colours, collision properties.
