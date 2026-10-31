# Level packs: the authored level format (levels as assets, slice 4)

Status: designed with and approved by the owner, 2026-10-18. The public, on-disk format for
community, kaizo and bonus levels and the future in-game editor; follows
`docs/specs/2026-10-17-levels-as-assets-design.md` (slices 1-3 typed the terrain, rooms,
spawns and surfaces) and `docs/PLAN.md` "Bonus levels".

## What the owner wants

- **New levels from the game's parts**: kaizo, bonus and community levels built from the game's
  metatiles, actors and rooms; the original game is just the first pack.
- **Tiled is the format and an editor**: the files are Tiled's JSON; people use Tiled today, and
  our in-game editor reads and writes the same files (no reinvented wheel, no conversion).
- **Metatiles three ways**: reuse an area's set, mix several areas' sets, or define custom ones.
- **`classic` and `modern` levels**: a `classic` level must work on the NES (the real limits,
  checked); a `modern` level has no limits and is designed for widescreen (the future Modern
  engine). The same `Classic | Modern` naming as the settings.
- **Packs are worlds**: a pack holds several stages, so it can be released as a "world" once the
  overworld menu ships.
- **Stage knobs, not hidden borrowing**: what the engine ties to an area (music, boss, palettes,
  HUD label, what clearing does) is a stage setting a player can turn, by hand or in a
  stage-settings panel (a wizard) in our editor.

## Files and folders

```
my-world/                       the pack, what people share
  pack.json                     identity, engine, format version, the stages and their knobs
  world.world                   Tiled's overview layout of the rooms (layout only)
  rooms/<stage>-<n>.tmj         one Tiled map per room
  custom.tsj                    optional: the pack's own metatiles (definitions, no images)
  rotj.tiled-project            optional: opens the pack in Tiled with typed dropdowns

<local data dir>/tilesets/      generated from the player's ROM, never shared
  area-01.tsj + area-01.png ... area-18.*   one Tiled tile per metatile
  packs/<pack>/custom.png       a pack's custom metatiles, rendered on demand
```

- A pack holds only numbers and names: no ROM art and no ROM-copied tables, so it can be shared.
  Metatile references (an area and a metatile id) and CHR tile references are resolved against
  each player's own ROM.
- Tilesets are generated locally (a tool, and the game on first run); packs name them by stable
  names (`area-03.tsj`), and the project file points Tiled at the local folder.
- Everything is JSON (`.tmj` maps, `.tsj` tilesets, `.world`, `pack.json`).

## `pack.json`

```json
{
  "format": 1,
  "name": "Chemical Revenge",
  "author": "someone",
  "engine": "classic",
  "stages": [
    {
      "name": "The Plant",
      "rooms": ["rooms/plant-1.tmj", "rooms/plant-2.tmj"],
      "hud_label": "1-1",
      "music": "song-01",
      "cutscene": "picture-00",
      "password": "ABCD",
      "on_clear": "ending"
    }
  ],
  "extra_passwords": [{ "password": "WXYZ", "stage": "The Plant" }]
}
```

- `engine`: `classic` or `modern` for the whole pack.
- A stage's rooms run in the listed order; room exits (actors) lead to the next.
- The knobs are the stage settings, everything the engine's stage descriptor reads (release 2, implemented in
  1.1.43, `docs/specs/2026-10-24-level-packs-release-2-design.md`): `hud_label` (`d-d`), `music`
  (`song-xx` or `keep`), `cutscene` (`picture-xx`, optional), `password` (four letters `A`-`Z`,
  optional; a stage with one is a checkpoint, and the first stage must be one) and `on_clear`
  (`next-stage` or `ending`). Values are names, never ROM addresses. `extra_passwords` lists the
  password screen's passwords the pause never shows, each starting a named checkpoint. Rooms
  carry their own sprite palette, so there is no `sprite_palettes`; `boss` is an editor-wizard
  field, not runtime format (an unknown key survives a load and save).

## A room (`.tmj`)

- 16x16 px tiles, one per metatile. In `classic`, a room is a strip of whole pages (256x256 px)
  along its scroll direction.
- Layers:
  - `terrain` (tile layer): metatiles; it may use several tilesets (mixing areas) and
    `custom.tsj`.
  - `spawns` (object layer): actors (below).
  - `markers` (object layer): `player-start`, and an optional `camera-start`.
- Room properties (typed in the project file): `scroll` (`horizontal` | `vertical`), `arrival`
  (`unchanged` | `held` | `flying`), `background_palette` and `sprite_palette` (named presets),
  `hud_split` (`standard` | `none`).
- Derived by the importer, never authored: the CHR banks (from the metatiles used), the page
  order, the fixed rows, the start record's scroll values, the spawn schedule columns.

## Metatiles

- An area tileset's tile is that area's metatile (its image, collision and palette come from the
  ROM).
- A custom metatile is a tile in `custom.tsj` with properties `tiles` (four CHR references,
  bank and tile), `palette` (0-3) and `surface` (the game's `Surface`: solid, ground, hazard, ...);
  its image is rendered locally.
- Reusing a set, mixing sets and custom metatiles are the same mechanism: the importer builds the
  stage's metatile table from the references the rooms use.

## Actors

- A point object in `spawns`; its class is the actor's family name (`stomper`, `patrol_walker`,
  `room_exit`, `pickup`, ...), a dropdown from the project file; its position is the actor's.
- Properties, format 1: the spawn record's fields by name (`flags`, `state`, `behaviour`,
  `subtype`, `health`, `contact`, `chr_slot`), each defaulting per family. Typed per-family
  properties (`side`, `path`, `drop`, ...) come in a later format version, with the per-family
  state work; the importer reads both.

## `classic` and `modern`

- `classic` is checked against the NES limits, with errors naming the file, layer and object:
  at most 4 background CHR banks per room; a stage's rooms fit one area's 16 pages; the metatile
  and class counts; spawn records per schedule cell; only ported actor families; the knobs'
  values exist in the game.
- `modern` skips those checks; the Classic engine refuses it; it waits for the Modern engine
  (1.2.0 onward, a separate, parity-free mode with its own tests).

## The importer, the exporter, the proof

- **Importer** (`rotj_core::levels`, plain Rust with serde, no Bevy): a pack into the game's typed
  assets per stage (the map and metatiles as `Terrain`, the `Room`s, the `SpawnTable`s, the
  stage descriptor); `classic` validation.
- **One seam**: a `Levels` value beside `Prg`; by default the original game, imported from the
  ROM; a pack replaces it. The simulation doesn't care which.
- **Exporter** (`rotj-tools export-levels`): the original 18 areas as one world pack, "The
  Original Game", into the local data folder (ROM-derived: never committed or shared); a starting
  point for remixes.
- **Proof**:
  1. Equivalence: `import(export(rom))` equals the assets the game builds from the ROM today,
     every area and room, field by field.
  2. The game on the imported pack passes the Classic suites, the chain and the tapes unchanged.
  3. Round trip: `export(import(export(rom)))` equals `export(rom)` byte for byte: what our
     editor's saves rely on.
- **Versioning**: `format` is an integer; the importer reads every known version; files are
  written in the latest; our editor keeps unknown layers, properties and fields.

## Releases, in order

1. The Tiled JSON model, the exporter, the importer, the equivalence and round-trip proofs (the
   game doesn't use it yet).
2. The stage descriptor: a survey of everything the engine reads by area number (music, boss
   intro and HP bar, the defeat-to-stage-end flow, sprite palettes, HUD label, ...) moved into
   one descriptor the engine reads; the original game's pack sets the original values; the
   tapes unchanged. The knobs' final list comes from this survey.
3. The game loads levels through the `Levels` seam (the original game via the pack path); tapes
   unchanged. Implemented in 1.1.44 (`docs/specs/2026-10-24-level-packs-release-3-design.md`):
   the pack plus a ROM overlay of the 225 schedule cells and the one room mirroring the format
   drops; `bevy_asset_loader` moves to release 6.
4. The tileset generator (PNGs from the player's ROM) and the Tiled project file. Implemented in
   1.1.51 (`docs/specs/2026-10-30-level-packs-release-4-design.md`): the tilesets go in the folder
   the settings file lives in (owner, 2026-10-29).
5. `classic` validation and custom metatiles. Owner, 2026-10-29: validation refuses a room that
   shows pages past its stage's stream, naming the room and the page. Implemented in 1.1.52
   (`docs/specs/2026-10-30-level-packs-release-5-design.md`), with mixing areas in a room and a
   stage.
6. Playing a pack: `--pack <dir>` and dropping a pack folder or its `pack.json` onto the exe, like
   the ROM (owner, 2026-10-29); `--pack` refuses by name a pack with too few stages or rooms or a
   bad extra password. The optional room `mirroring` property (absent = derived, `format` stays
   1) lets the pack carry 1-1-2's start, and `Levels::original` drops that part of its overlay.
   Which area-keyed ROM tables follow `stage.area` is the stand-in's call. The stage-settings
   panel, the menu and the overworld come with the editor and the overworld work.

## Not here

The Modern engine; the in-game editor itself; the overworld menu; art beyond the game's CHR;
typed per-family actor properties (a later format version); a "Load pack" menu entry (owner,
2026-10-29: later, with the editor and the overworld).

## Release 1: what the approved schema can't carry (survey, 2026-10-18)

Before writing release 1, every field of the game's level assets was mapped onto the approved
format and each "derived by the importer" rule checked against the ROM's 36 rooms in 18 areas.
`import(export(rom))` can't equal the assets field by field without owner decisions; release 1
waits for them (the schema is the owner's).

- **What maps cleanly or derives exactly**: the terrain, palettes as named presets, arrival and
  orientation, the player and camera starts (the exporter always writes `camera-start`: camera
  from the player fits 3 of 36 rooms), `ring_y` (36/36), mirroring (35/36), the first spawn
  column (32/36), actors' positions and properties with Tiled's object ids as record ids, the
  raster and fixed rows through `hud_split` resolved per area (each area has at most one split).
- **CHR banks**: "the banks the area's tileset was rendered with" fits 25/36; the others are the
  same metatiles in other banks. Exact with one generated tileset per (area, CHR) variant (27,
  e.g. `area-06-b.tsj`), which changes the local tileset list, not the pack.
- **Not derivable, needs the schema**:
  1. *The physical page order*: an area's 16 pages in an arbitrary order that actors hard-code
     (`page_table_tail_set`, `page_table_pair_patch`, `cell_painter`, `column_eraser`), pages
     no room shows, and pages past the stream's reach (whatever WRAM held). Derived by first
     appearance it fits 11 of 36 rooms.
  2. *The spawn schedule*: trigger cells sit apart from where the actor appears (175 of 803
     pending cells at the actor's column), 316 records have several triggers, and 398 cells are
     scroll locks; plus residue the game never reads.
  3. *The start's `scroll_mode`, `scroll_lock` and `facing`*: no rule fits (20/36 and 19/36 for
     the best).
- **Proof scope**: the game reads a record only through a pending cell and `raised_page_count`
  only in the two two-page horizontal rooms; comparing only what the game reads would drop the
  unreferenced spawn records and that byte elsewhere.

### Questions for the owner

1. May the schema carry an area's physical page order (a room property, or a stage-level map of
   the area's pages)?
2. May actors carry their trigger columns (e.g. a `triggers` list property) and rooms a
   `scroll-lock` marker; and should schedule residue the game never reads be dropped?
3. May `player-start` carry `facing`, `scroll_mode` and `scroll_lock`?
4. May the equivalence proof cover only what the game reads (unreferenced spawn records,
   `raised_page_count` outside the two-page rooms)?
5. Are per-CHR-variant tilesets (27 instead of 18) fine; is `<stage>-<n>` with stages named by
   their banner ("3-2", so `rooms/3-2-1.tmj`) the naming; and which class names should
   `spike_probe`'s two kinds and the four unported kinds get?

### The owner's answers (2026-10-18)

1. A stage carries its area's physical page map: an ordered list of the area's pages, each a
   room region or empty, so the importer rebuilds the layout exactly.
2. Actors carry a `triggers` list (schedule columns); rooms get a `scroll-lock` marker; the
   schedule's residue the game never reads is dropped.
3. `player-start` carries `facing`, `scroll_mode` and `scroll_lock`.
4. The equivalence proof covers what the game reads.
5. One generated tileset per (area, CHR) variant; rooms named by the stage's banner,
   `rooms/<stage>-<room>.tmj`. One class per family; a family with more than one kind
   (`spike_probe`, `ace_ranger_shot`, `breakable_block`, and the four number-only kinds under
   family names) gets a typed `variant` enum property, a dropdown per family, beside the typed
   per-family properties to come.
