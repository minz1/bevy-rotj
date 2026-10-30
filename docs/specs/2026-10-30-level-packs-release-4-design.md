# Level packs release 4: the tileset generator and the Tiled project file

Status: implemented on autopilot/2026-10-29-6. One forced deviation: the original pack does have
`no_handler` objects (only `Source::Original` keeps them), so the coverage test pins its undeclared
set to exactly `class no_handler` rather than empty; any other undeclared class or value fails it.
Approved by the Opus stand-in (L3, 2026-10-30) after the reviewer's pass. Changes from the
draft: the startup writer rewrites stale files too (stand-in 1a); the tilesets have 256 tiles
(release 1's rule); no per-tile `surface` (stand-in 6: the approved format gives that name to
custom metatiles' `Surface`); a missing `variant` imports as the family's first kind (stand-in A);
each tileset carries its banks as `chr` (stand-in B, release 1's promise); the project file is
built in `write` from the levels; `no_handler` gets no class. Follows
`docs/specs/2026-10-18-level-packs-design.md` ("Releases, in order" 4) and the owner's 2026-10-29
decisions (HANDOFF.md): generated tilesets go in the same local data folder as the settings file.

## What it adds

Opening an exported pack in Tiled today shows broken tile images (the `.tsj` files the maps name
don't exist) and untyped properties. After this release:

1. **Tilesets**: one Tiled tileset per (area, CHR variant), named as the maps already name them
   (`area-01.tsj`, `area-06-b.tsj`, ...; 27 for the original game), each with its PNG, in
   `<data>/tilesets/`. `<data>` is the folder `settings.ron` lives in (the exe's folder); packs
   live in `<data>/packs/<pack>/`, which is what the maps' `../../../tilesets/` assumes.
2. **The project file** `rotj.tiled-project` in every exported pack: Tiled's typed enums for the
   properties the exporter tags with a `propertytype`, one object class per actor family, and the
   tileset folder.

No game behaviour changes; `format` stays 1. The importer changes in one way: a multi-kind
family's object without `variant` imports as the family's first kind (it accepts strictly more;
every pack it accepted before imports the same). The original pack still exports byte-exact and
round-trips.

## Tilesets

- `rotj_core::levels::tilesets(prg, chr) -> Option<Vec<Tileset>>`: for each catalog area and CHR
  variant, in catalog order: its name, its `.tsj` text and its picture as RGBA, 256x256 px, 16
  tiles a row, one 16x16 tile per metatile id 0-255 (`TILESET_TILES`; so a map's
  `gid - firstgid` is the metatile id the importer reads). Ids past the area's metatiles are
  transparent. No PNG in core; the callers encode (`image`).
- Picture: each metatile's four corners (top-left, top-right, bottom-left, bottom-right) are
  nametable tiles drawn from the variant's four background banks (`Room::chr`; a bit-7 animation
  handle resolves to the bank the room loads with, through a `loaded_bank(prg, bank)` shared with
  `animate_banks`), in the colours of the metatile's palette class (`palette[4c..4c+4]`) from the
  palette `build_palette` makes for the first room of that variant, colour 0 `$0F`
  (`set_first_colours`), through the default palette (`palette::rgba`). The picture is a preview
  for the editor and never feeds the game.
- `.tsj` (Tiled JSON, keys in Tiled's order, `version` and `tiledversion` as the maps write them):
  `columns: 16`, `image: "<name>.png"`, `imageheight: 256`, `imagewidth: 256`, `margin: 0`,
  `name`, `properties` (one: `chr`, an int list of the variant's four `Room::chr` bytes as
  stored, animation handles included), `spacing: 0`, `tilecount: 256`, `tiledversion`,
  `tileheight: 16`, `tilewidth: 16`, `type: "tileset"`, `version`.
- Writers:
  - `rotj-tools tilesets -r <rom> -o <out>` writes every tileset's `.tsj` and `.png` into `<out>`.
  - The game, at startup, writes each file into `<data>/tilesets/` when it is missing or its bytes
    differ from what the generator makes now (measure first: generation plus compare stays
    synchronous if it costs a few ms). A failure is logged and ignored; the game never reads them.

## The project file

- `rotj_core::levels::write` adds `rotj.tiled-project`, built from the levels it writes, so the
  round trip and equivalence proofs cover it; the importer never reads it.
- Tiled 1.12 project JSON: `folders: [".", "../../tilesets"]` (relative to the pack root in
  `<data>/packs/<pack>/`), and `propertyTypes`:
  - String enums (`type: "enum"`, `storageType: "string"`, `valuesAsFlags: false`), one per
    `propertytype` the exporter writes: `arrival` (`unchanged`, `held`, `flying`), `hud_split`
    (`standard`, `none`), `scroll` (`horizontal`, `vertical`), `background_palette` and
    `sprite_palette` (the `bg-xx` and `sprites-xx` selectors the levels' rooms use, ascending), and
    one `<class>_variant` per family with more than one kind (its variant names, as `class_of`
    writes them).
  - One class per actor family (`type: "class"`, `useAs: ["object"]`) as `classes()` names them,
    `no_handler` left out (the importer refuses it in a pack). Members: the spawn record's int
    fields (`behaviour`, `chr_slot`, `contact`, `flags`, `health`, `state`, `subtype`, default 0,
    the importer's missing-int rule), `triggers` (an int list, default empty), and `variant` (its
    enum, default the family's first kind, the importer's missing-variant rule) where the family
    has one. Classes for the markers: `player-start` (`facing`, `scroll_mode`, `scroll_lock`),
    `camera-start` (none) and `scroll-lock` (`locks`). Members use Tiled's `propertyType` key.
- `rotj-tools export-levels`' help says the pack belongs in `<data>/packs/<pack>/` for the
  tileset paths to resolve.

## Proof

- `cargo test -p rotj-core` (ROM-gated like the other ROM tests):
  - the original pack exports byte-exact and round-trips, now with the project file (41 files;
    the save-back loop checks it as JSON);
  - every exported room's `propertytype`s and object classes are declared in the project file
    with the value used (so a `no_handler` object would fail it);
  - a multi-kind object without `variant` imports as the family's first kind;
  - the tilesets: 27 names equal the catalog's, every picture 256x256, an FNV-1a digest of the 27
    pictures pinned, and one tile of a room with an animated bank equals the game's own state
    after `room::load` (`oam.chr`, `effects.palette`, the area's corners and palette classes).
- `rotj-tools`: `tilesets` writes 54 files into a temp folder.
- App: the startup writer writes missing files, rewrites a stale one, leaves an equal one
  untouched, and survives a folder it cannot create (a regular file as the parent path).
- Tapes, chain and lockstep unchanged (full set).
- The owner opens an exported pack in Tiled 1.12 once after the release (no Tiled here): tiles
  render, dropdowns appear, a new multi-kind actor saves and imports. Does not block.

## Not here

Custom metatiles' `custom.png` (release 5, with `custom.tsj`); `classic` validation and the
selectors' ranges (release 5); `--pack` (release 6); a per-tile collision property (release 5's
`Surface` enum if at all); per-family typed properties (a later format version); a "Load pack"
menu entry (with the editor and the overworld).
