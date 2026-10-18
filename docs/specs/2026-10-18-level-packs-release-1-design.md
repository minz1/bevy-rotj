# Level packs, release 1: the Tiled model, the exporter, the importer, the proofs

Status: approved with changes by the Opus stand-in, 2026-10-18 (applied: `two_page` derived from
the map's size, with tile-less regions and more extra pages; the `mirroring` and
`first_spawn_column` rules, no overrides, area 11 rooms 2 and 3 pending the owner; the residue
sentence; the page-table actors' assertion; `triggers` as a Tiled 1.12 `list`; gid checks;
`no_actor` without a variant; why `pages` lists regions). Release 1 of
`docs/specs/2026-10-18-level-packs-design.md` (the approved format, with the owner's answers of
2026-10-18 at its end), grounded in a survey of the 36 rooms of the 18 areas against the ROM. The
game does not use the pack yet: release 3 moves it onto the `Levels` seam.

## What the survey found

- An area owns the 16-page map (`Terrain::stream`, a whole number of pages in every area), the
  256-cell spawn schedule (`actors::spawn_schedule`, reloaded on every room load) and the metatile
  tables. A room is a 32-entry page table into those pages (`Room::pages`: entries 0-15 along the
  scroll, 16-31 the second band across), plus its own `SpawnTable`.
- All 32 entries matter in every room: the loader copies them to `$0440-$045F`, the probes reach
  entries 16-31 below (or beside) a one-page room, and four stage actors rewrite them
  (`page_table_tail_set` `$60`, `page_table_pair_patch` `$70`, `page_table_tail_clear`,
  `page_column_reset`). Six pages inside the stream's reach are shown only by a one-page room's
  entries 16-31 or by no entry: area 1 page 10, area 2 page 9, area 7 pages 13 and 14, area 14
  page 9, area 15 page 6.
- Rooms share pages in 7 areas. Rooms that share a page share its schedule cells but never their
  spawn tables (0 of 203 pairs), so a cell's record id names a different record in each room.
- 8 areas' rooms show pages past the stream's reach: whatever WRAM held, not part of `Terrain`.
- 27 (area, CHR) variants: areas 5, 13 and 16 have 3, area 11 has 4; 27 rooms carry an
  animation handle (bit 7) in `Room::chr`.
- Each area has at most one non-empty screen record (10 in all); `fixed_rows` and `fixed_quads`
  are a function of it. 22 rooms have the empty record (`$77`), all with arrival `unchanged`.
- Start record: `ring_y == camera_y` (36/36); `mirroring` is 0 in a vertical room, else 1 when
  the camera starts in the second band (`camera_y >= $1000`) and 2 otherwise (36/36);
  `first_spawn_column` is the page column of the page-table entry under the camera (34/36, not
  area 11 rooms 2 and 3). `raised_page_count` is read only in the
  two horizontal two-page rooms, where it is 16 minus the trailing run of one repeated page.
- The orientation byte's low six bits are zero in every room.
- Records: positions are 1/16 px (349 of 613 referenced records have a fraction); 316 records have
  more than one trigger cell; `spike_probe` uses both its kinds; four referenced kinds have no
  family (main `$00` in areas 16-17; second-class `$4A`, `$88`, `$E0` in area 17).
- The banner's digits (bank 2 `$93F0[area]`) name the areas 1-1, 1-2, 1-3, 2-1, 2-2, 3-1, 3-2,
  3-3, 4-1, 4-2, 4-3, 5-1, 5-2, 6-1, 6-2, 6-3, 7-1, 7-2; rooms per area 1,1,1,3,1,4,2,1,1,4,1,4,1,4,1,1,4,1.

## Where the code lives

- `rotj_core::levels` (new module, `levels/model.rs`, `levels/export.rs`, `levels/import.rs`):
  plain Rust, no Bevy, as the approved spec says. `rotj-core` gains `serde` (the workspace's,
  with `derive`; the app already uses it) and `serde_json = "1"` (a new workspace dependency;
  default features, so `serde_json::Map` is a `BTreeMap`). Its only dependency today is
  `thiserror`; the two are small, build-script-free past serde's own, and the format needs them
  anyway. No separate crate: the importer feeds `Terrain`, `Room` and `SpawnTable`, which live here.
- `rotj-tools`: `rotj export-levels --rom <path> --out <dir>` writes the pack's files. The local
  data folder comes with release 4's tileset generator; release 1 takes `--out`.

## The Tiled model (`levels::model`)

Only the subset we read and write, each struct with `#[serde(flatten)] extra: Map<String, Value>`
so a file's unknown fields survive a load and save (written after the known ones, sorted).

- `Map`: `type` `"map"`, `version` and `tiledversion` as Tiled 1.12 writes them (`"1.12"`-style
  values; the implementer checks a real Tiled 1.12 file if one is at hand, else Tiled's documented
  values), `orientation` `"orthogonal"`,
  `renderorder` `"right-down"`, `infinite` false, `width`, `height` (tiles), `tilewidth` and
  `tileheight` 16, `nextlayerid`, `nextobjectid`, `properties`, `tilesets`, `layers`.
- `TilesetRef { firstgid: u32, source: String }`: the importer reads the source's file stem
  only (`area-06-b`).
- `Layer`: one struct for every layer (`id`, `name`, `type`, `x`, `y`, `opacity`, `visible`, and
  `width`, `height`, `data: Option<Vec<u32>>` for `tilelayer`, `draworder`,
  `objects: Option<Vec<Object>>` for `objectgroup`). Layers other than `terrain`, `spawns` and
  `markers` are kept as they are and ignored.
- `Object`: `id`, `name` (written `""`), `type` (the class), `point` true, `x`, `y`, `width` and
  `height` 0, `rotation` 0, `visible`, `properties`.
- `Property { name, type, propertytype: Option<String>, value: serde_json::Value }`: `type`
  `int`, `bool`, `string` or `list` (Tiled 1.12's list property, its value an array of
  `{"type": "int", "value": 37}` items); enums are `string` with a `propertytype` naming the
  project file's enum (release 4 writes those enums). Properties are written sorted by name, as
  Tiled does.
- `Position(u16)`, 1/16 px: serialised as an integer when whole and as the exact decimal
  otherwise (`29.9375`); read from any JSON number that is a whole number of sixteenths in range,
  an error otherwise.
- `World { maps: Vec<WorldMap { fileName, x, y, width, height }>, onlyShowAdjacentMaps: false,
  type: "world" }`; `PackFile` for `pack.json` (below).
- Files are written with `serde_json::to_string_pretty` and a final newline: struct field order
  fixes the key order, integers print as integers, so output is deterministic.

## The pack

```
the-original-game/
  pack.json
  world.world
  rooms/1-1-1.tmj ... rooms/7-2-1.tmj      36 rooms, <stage>-<room>, rooms counted from 1
```

- One stage per area, named by its banner, in area order. A stage's rooms run in the game's room
  order (room `n` is `$0402 = 2(n-1)`).
- `pack.json`:
  ```json
  {
    "format": 1,
    "name": "The Original Game",
    "author": "Sunsoft",
    "engine": "classic",
    "stages": [
      {
        "name": "3-2",
        "rooms": ["rooms/3-2-1.tmj", "rooms/3-2-2.tmj"],
        "pages": [
          [{ "room": "rooms/3-2-1.tmj", "entry": 0 }, { "room": "rooms/3-2-2.tmj", "entry": 4 }],
          [],
          ...
        ],
        "hud_label": "3-2"
      }
    ]
  }
  ```
- `pages` is the owner's physical page map: 16 entries, one per physical page of the area in
  order, each the list of room regions that show it (`entry` a page-table index 0-31, or 32 and
  up for an extra page, below), sorted by room then entry; `[]` for a page no room shows and the
  stream does not reach. Every region of every room appears exactly once. A page holds a list
  because rooms share pages and repeat them: one region per page couldn't rebuild the other
  rooms' tables.
- Knobs: release 1 writes `hud_label` only; release 2's survey fixes the rest (`music`,
  `sprite_palettes`, `boss`, `on_clear`, ...). Readers ignore knobs they don't know.
- `world.world`: layout only, each stage a row (`y` = stage index × 4352 px), its rooms left to
  right 256 px apart. The importer doesn't read it.

## A room (`.tmj`)

- The map is a strip of whole pages along the scroll: 16 pages along and 1 across in a one-page
  room (horizontal 4096×256 px, 256×16 tiles; vertical 256×4096 px), 2 across in a two-page room.
  Entry `e` sits at along page `e & 15`, band `e >> 4`. A one-page room's entries 16-31 are still
  regions in the stage's `pages`, with no tiles.
- A page takes its cells from its first region that has tiles; only regions with tiles must
  agree. A page inside the stream's reach that only tile-less regions (or none) show is an extra
  page: appended to the stage's first room along the scroll after page 15, first band (entries
  32 and up, in physical page order); the map grows by them. Release 1 has six (the survey's).
- Layers, ids 1-3: `terrain` (`tilelayer`), `spawns` and `markers` (`objectgroup`,
  `draworder` `"index"`); `nextlayerid` 4.
- `terrain`: gid = the variant tileset's `firstgid` + metatile id. Every tileset ref has
  `tilecount` 256 (every byte value), so a room using one variant has `firstgid` 1; release 4's
  generated tilesets must have exactly 256 tiles. Gid 0 marks a cell the stream does not write (a
  page past the reach); a page is all gid 0 or none. The importer rejects a gid with flip bits
  set or at or past `firstgid + 256`.
- Tilesets: one per (area, CHR) variant, `area-NN` (area counted from 1) for the variant of the
  area's first room, then `area-NN-b`, `-c`, `-d` in order of first appearance across the area's
  rooms. The exporter writes `"source": "../../../tilesets/area-06-b.tsj"` (the pack in
  `<data>/packs/<pack>/`). Release 1 needs no tileset file: the importer resolves the name against
  the ROM (`levels::Catalog::from_rom(prg, chr)`: name → area and the variant's four `chr`
  bytes, animation handles included). Release 4 writes the files, with the banks as properties.
- Room properties:

  | name | type | values |
  |---|---|---|
  | `scroll` | enum `scroll` | `horizontal`, `vertical` (orientation bit 6) |
  | `arrival` | enum `arrival` | `unchanged`, `held`, `flying` |
  | `background_palette` | enum `background_palette` | `bg-XX`, the game's selector in hex (19 used) |
  | `sprite_palette` | enum `sprite_palette` | `sprites-XX`, likewise (14 used) |
  | `hud_split` | enum `hud_split` | `standard` (the area's screen record), `none` (`$77`) |

- `markers`:
  - `player-start` (id 65): position = `player_x`, `player_y`; properties `facing` (int),
    `scroll_mode` (int), `scroll_lock` (int).
  - `camera-start` (id 66): position = `camera_x`, `camera_y`; always written.
  - `scroll-lock` (ids from 67, by column): one per lock cell (flags `$40`) on a page the room's
    table shows; position = the cell's schedule column (x = column × 16 px along, band × 256 px
    across, the other coordinate 0); property `locks` (int, the cell's low six bits).
- `spawns`: one point object per record id that a pending cell on a page of the room's table
  names; object id = record id + 1 (1-64), objects in id order; position = the record's `x`,
  `y`; `type` = the family; properties `flags`, `chr_slot`, `state`, `behaviour`, `subtype`,
  `health`, `contact` (int), `variant` (enum `<family>_variant`, families with several kinds only)
  and `triggers` (`list` of `int`: the schedule columns of the record's pending cells on the
  room's pages, ascending; a schedule column is entry × 16 + the cell's column, 0-511), as
  `{"name": "triggers", "type": "list", "value": [{"type": "int", "value": 37}, ...]}`. The
  exporter writes every property; the importer reads a
  missing one as 0 until the per-family defaults arrive with typed per-family properties.
- Classes: the families' module names (`room_exit`, `stomper`, `sideways_platform`, ...; the
  main and second-class name spaces don't collide), so `kind` and `second_class` come from the
  class and `variant`. Variants, valued by the game's type byte:
  - `breakable_block`: `type-04`, `type-06`
  - `ace_ranger_shot`: `type-3a`, `type-3c`
  - `spike_probe`: `type-58`, `type-5a`
  - `no_handler` (second-class `$4A`, `$88`, `$E0`: record ids past area 17 room 0's table, which
    `$CEC3` has no entry for and play never reaches; renamed from `inert_platform` by
    `docs/specs/2026-10-18-empty-slot-design.md`): `type-4a`, `type-88`, `type-e0`

  `no_actor` (main `$00`: the cell clears and nothing spawns) has one kind and no `variant`.

## The importer (`levels::import`)

`import(pack: &PackFiles, catalog: &Catalog) -> Result<Levels, ImportError>`, errors naming the
file, layer and object. `Levels { stages: Vec<StageAssets> }`:

- `StageAssets { stage: Stage, terrain: Terrain, schedule: [u8; 256], rooms: Vec<RoomAssets> }`,
  `RoomAssets { room: Room, spawns: SpawnTable }`, `Stage { name, area, hud_label }` (the stub
  release 2 grows). `area` is the one area of the stage's tilesets; release 1 refuses mixed areas
  (mixing comes with release 5).
- `Terrain`: `corners`, `classes`, `palette_classes` from the catalog's area; `stream` from the
  page map: physical page `p`'s 256 cells from its first region with tiles, laid out as `$7000`
  holds them (`p × 256 + row × 16 + column`), for the leading pages that are not gid 0 (a page
  with no region with tiles counts as gid 0); every other region of `p` with tiles must hold the
  same cells.
- `Room`: `pages[e]` = the physical page whose regions include (room, `e`); `chr` from the
  variant; `orientation` from `scroll` (bit 6) and the map's size across (two pages: bit 7);
  palettes from the presets; `raised_page_count` = 16 minus the trailing run of `pages[0..16]`'s
  last page; `start` from the markers, `ring_y = camera_y`, `mirroring` 0 in a vertical room,
  else 1 when `camera_y >= $1000` and 2 otherwise; `first_spawn_column` =
  `pages[band × 16 + along page] << 4` under `camera-start` (band 1 when the across coordinate is
  at least `$1000`, along page the along coordinate `>> 12`); `raster`, `fixed_rows`,
  `fixed_quads` from the catalog's area when
  `hud_split` is `standard`, else none and `arrival` must be `unchanged`.
- `schedule`: each trigger sets `$80 | id` and each `scroll-lock` `$40 | locks` at the cell its
  schedule column names through the room's `pages`; rooms that share a cell must agree; every
  other cell 0 (the residue the owner dropped).
- `SpawnTable`: 64 entries, `Some` for each object's id, `None` elsewhere.

## The exporter (`levels::export`)

- `Levels::from_rom(prg, chr)`: today's assets in the importer's types, from today's importers
  (`Terrain::import`, `Room::import`, `SpawnTable::import`, `actors::spawn_schedule`) and the
  banner digits. It is what the equivalence compares against.
- `write(levels: &Levels, catalog: &Catalog) -> PackFiles` (`BTreeMap<String, String>`: path to
  file text), so `export(prg, chr)` is `write(&Levels::from_rom(..), &catalog)` and
  `export(import(export(rom)))` is `write(&import(export(rom)), &catalog)`. `write` reads only
  what the format carries (unread fields and residue never reach a file).
- `rotj export-levels` writes `PackFiles` to disk. Nothing ROM-derived is committed: the tests
  export in memory.

## The proofs

- **Equivalence**, `import(export(rom))` against today's assets, every area and room, field by
  field over what the game reads:
  - `Terrain`: all four fields, equal.
  - `Room`: every field, except `raised_page_count` outside the two horizontal two-page rooms
    (not read there) and the two fields pending the owner, skipped by name.
  - `SpawnTable`: the records the room's read cells name (a pending cell on a page of its table),
    every field; the other ids, which no cell names and the game never spawns, are skipped.
  - The schedule: every pending or lock cell on a page some room's table shows, equal; every
    other ROM cell is residue (non-pending ids, cells on pages no table shows) and must import as 0.
  - In each room whose spawn table holds `page_table_tail_set`, `page_table_pair_patch`,
    `page_table_tail_clear` or `page_column_reset`, the pages they write at runtime are already in
    its static table (else the read cells' scope could miss records); true today.
  - The test also checks the counts above (36 rooms, 27 variants, 613 records, 6 extra pages) so
    a silent skip fails.
- **Round trip**: `export(import(export(rom)))` equals `export(rom)` byte for byte, every file;
  and each exported file parsed into the model and saved again is byte-equal (with an unknown
  field and an unknown layer added by the test, which must survive).
- **Unit tests** without the ROM: `Position`'s number forms, the property sort, a small
  hand-made pack (two rooms sharing a page, a lock, a two-trigger record, an extra page, a gid-0
  page) through the importer, and the errors (a region listed twice, disagreeing shared cells,
  an unknown class or variant).
- Residue is dropped (owner's answer 2) and must import as 0. Release 3's spec decides how the
  lockstep treats those `$7410-$750F` cells and puts any proof change to the owner.
- Pending the owner: `first_spawn_column` in area 11 rooms 2 and 3 (`rooms/5-1-3.tmj`,
  `5-1-4.tmj`; ROM `$00` and `$70`), which no approved field carries. The importer writes the
  derived value; the equivalence test skips these two fields by name; release 3 waits for the
  answer.

## Not here

Using the pack in the game (release 3); the knob list and stage descriptor (release 2); the
tileset files and the Tiled project file (release 4); `classic` validation, custom metatiles,
mixing areas (release 5); per-family defaults and typed per-family properties.
