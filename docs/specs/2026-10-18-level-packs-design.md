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
      "music": "stage-1",
      "sprite_palettes": "stage-1",
      "hud_label": "1-1",
      "boss": { "family": "stage3_boss", "room": "rooms/plant-2.tmj" },
      "on_clear": "next-stage"
    }
  ]
}
```

- `engine`: `classic` or `modern` for the whole pack.
- A stage's rooms run in the listed order; room exits (actors) lead to the next.
- The knobs (`music`, `sprite_palettes`, `hud_label`, `boss`, `on_clear`) are the stage settings;
  release 2 fixes their full list as everything the engine reads by area number today. Values
  are names (songs, palette sets, boss families), never ROM addresses.

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
   unchanged.
4. The tileset generator (PNGs from the player's ROM) and the Tiled project file.
5. `classic` validation and custom metatiles.
6. Playing a pack: a `--pack <dir>` launch flag first; the stage-settings panel, the menu and the
   overworld come with the editor and the overworld work.

## Not here

The Modern engine; the in-game editor itself; the overworld menu; art beyond the game's CHR;
typed per-family actor properties (a later format version).
