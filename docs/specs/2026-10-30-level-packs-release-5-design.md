# Level packs release 5: `classic` validation, custom metatiles, mixing areas

Status: approved by the stand-in (owner-proxy, L3) after the reviewer's pass; their changes are
applied: refusals for user packs only, `music` checked against the sound test's songs, one import
path (no separate unmixed one), one id rule, one slot rule, `custom.tsj` validated before it is
read (`tilecount` at most 208), no `rotj-tools tilesets --pack`, the runtime-written ids a stated
limit. Follows `docs/specs/2026-10-18-level-packs-design.md` ("Releases, in order" 5, "Metatiles",
"`classic` and `modern`") and the owner's 2026-10-29 decision: validation refuses a room that
shows pages past its stage's stream, naming the room and the page.

## What it adds

1. **`classic` validation**: every NES limit the format names is checked at import, with errors
   naming the file, the layer and the object.
2. **Mixing areas**: a room's `terrain` may name several area tilesets, and a stage's rooms
   several areas.
3. **Custom metatiles**: a pack's `custom.tsj` defines metatiles from CHR tiles; a room names it
   like any tileset. Its picture `custom.png` is drawn locally.

`format` stays 1 and nothing is renamed: every pack that imports today imports to the same
`Levels` (the original pack exports byte-exact and round-trips). No game behaviour changes; the
tapes, chain and lockstep stay unchanged.

## `classic` validation

What the importer already refuses stays (16 pages a stage, one record or lock a schedule cell,
known actor classes, `no_handler` in a user pack, the knobs' forms). New, for `Source::User`
only (the original pack keeps what its ROM has):

1. **Pages past the stream** (owner, 2026-10-29): a room's map-held entries are the page-table
   entries whose region lies inside its map (16 for a room one screen across). A room with a
   map-held entry on a page the stage's stream does not write is refused, naming the room, the
   entry and the page (`rooms/x-1.tmj: layer \`terrain\`: entry 6 is page 15, past the stream's
   11 pages`). Today such a region is all gid 0 and the game shows whatever `$7000` held. 13 of
   the original's 38 rooms have one (`2-1-1` to `2-1-3`, `2-2`, `3-1-2` to `3-1-4`, `4-3`,
   `5-1-3`, `5-1-4`, `5-2`, `6-3`, `7-2`; mostly one-screen rooms whose camera never reaches
   those entries). A user's one-screen room maps its other entries onto written pages.
2. **CHR banks**: a room loads at most 4 background banks; the mixing rules below refuse a room
   whose metatiles need more, naming the rooms and the bank.
3. **Metatile ids**: a stage's metatile table holds at most 208 metatiles (`CLASS_COUNT`, the
   class table's size; ids from `$D0` read the corner tables one table later, `Level::tiles`),
   naming the stage. When a stage appends metatiles (below), an area tile at or past its area's
   metatile count is refused, naming the room, the cell and the id (no original stream uses
   one).
4. **The knobs' values exist in the game**: `background_palette` and `sprite_palette` are
   selectors the original game's rooms use (the release 4 project file's enums list exactly
   these); `cutscene` (`picture-xx`) is a picture the original game's stages use; `music`
   (`song-xx`) is one of the sound test's 13 songs (`SONG_COUNT`, bank 12 `$9C85`). The catalog
   carries the sets, read from the player's ROM.
5. **Custom metatiles** (below): `custom.tsj` is checked before anything reads or draws it.

The exported original pack does not pass as a user pack (its `no_handler` objects and the 13 rooms
above); release 6's spec says how its `--pack` test pack is derived from it (those rooms' entries
remapped onto written pages, `no_handler` dropped).

## Mixing areas

A room's map lists one or more tilesets: area tilesets (`area-06-b.tsj`, by file stem, as today)
and at most one `custom.tsj` (stem `custom`, the pack's file). Their gid ranges are Tiled's:
ascending `firstgid`, an area tileset 256 tiles, `custom.tsj` its `tilecount`. One import path
covers every stage; a stage whose rooms all name one tileset of one area imports exactly as
today (needed slots take the room's own tileset's bytes, free slots keep them, nothing is
appended), so the equivalence and round-trip tests prove it on all 38 rooms.

- The **stage's area** is the area of the first area tileset (lowest `firstgid`) of the stage's
  first room; a first room with none is refused. It is `Stage::area` and keys everything an area
  keys today (the split, the descriptor's `area`, the catalog tables release 6 decides). A
  room's **own variant** is its lowest-`firstgid` tileset of the stage's area, if any.
- **Keys and pages**: a cell's metatile is keyed by its tileset and id; every variant of the
  stage's area is one key space (the stage-area id). Rooms sharing a page must agree on its
  cells by key, not by gid.
- **Ids**: the stage area's metatiles keep their ids, corners, palette classes and collision
  classes (every variant shares them). Each other key the stage's rooms use (another area's
  metatile, or a custom one) takes the next id from the stage area's corner count, in the order
  the stream first writes it. An appended metatile is written at its id, extending `corners`,
  `classes` and `palette_classes` (which are padded to a multiple of 4; an id in the padding
  overwrites it): its corners are its source's re-slotted (bank `chr[c >> 6]` of its tileset,
  tile `c & 63`; a custom one's banks and tiles as given), its classes its source area's, or a
  custom one's `palette` and `surface`.
- **CHR slots**: a corner is a bank and a tile 0-63; on the NES the corner byte is
  `slot * 64 + tile` and the room's `chr[slot]` names the bank. A room's needs are the corners of
  every metatile on the pages its map-held entries name (gid-0 cells on those pages show other
  rooms' metatiles), plus every slot any stage-area corner uses (actors copy and write stage-area
  ids at runtime): a stage-area slot needs the room's own variant's bank (else the stage area's
  first variant's). Two variants disagreeing on a slot one room needs is refused.
  Every bank the appended metatiles use gets one slot for the whole stage (their corner bytes are
  stage-wide), in first-use order, by one rule: try the slots some room already gives that bank
  (ascending), then the rest; take the first that, in every room needing the bank, is free or
  already that bank. None: refused, naming the bank and the rooms (`bank $50 finds no CHR slot
  free in every room that uses it (rooms 3-1-2, 3-1-3)`). The order is greedy: a stage another
  order would fit can be refused.
- A room's `chr` is the bank each slot needs; a slot nothing needs keeps the room's own variant's
  byte, else the stage area's first variant's. Animation handles (bit 7) are banks like any
  other: two corners agree when their bytes are equal.
- **Stated limit**: actors that write fixed metatile ids at runtime write stage-area ids:
  `cell_painter`'s `$60` and the stomper's rubble table (bank 8 `$8AE9`). In a stage that
  appends metatiles, such an id at or past the stage area's count draws the appended metatile
  there. Crumbling and collapsing write 0 and breakable blocks copy cells, so they are right.
- The exporter writes the original game only: `write` refuses (returns `None`, its panic doc
  updated) a stage whose terrain is not its catalog area's (the editor saves the files it loaded).

## Custom metatiles (`custom.tsj`)

A Tiled tileset at the pack's root, 16x16 tiles, `columns: 16`, `tilecount` at most 208; its
`image` points at the locally drawn picture, `../../tilesets/packs/<pack>/custom.png` from
`<data>/packs/<pack>/` (the importer ignores it). Each tile `id` in its `tiles` array carries:

- `tiles`: a `list` of four `string` items, top-left, top-right, bottom-left, bottom-right, each
  `"<bank>:<tile>"` in two lowercase hex digits (`"2c:05"`, as selectors are written). The bank
  is a byte as `Room::chr` stores it (the area tilesets' `chr` property lists the same bytes in
  decimal): a plain bank below `$80`, or an animation handle (bit 7) one of the original rooms
  loads; the tile is `00`-`3f`.
- `palette`: `int` 0-3, the metatile's palette class.
- `surface`: a `string` of the project file's `surface` enum: `solid`, `belt-right`, `belt-left`,
  `hazard-behind`, `solid-hazard`, `slippery`, `behind`, `open`, `solid-to-bodies`, `crumbling`,
  `one-way`, `one-way-behind`, `spikes` (the game's `Surface`; a name covering several classes is
  its lowest: `open` 7, `solid-to-bodies` 11).

The importer, `custom_picture` and the startup writer check `columns`, `tilecount` and every
tile's properties before reading or drawing, with errors naming `custom.tsj` and the tile id; a
tile with no definition is refused where a room uses it. The project file adds the `surface`
enum (named after its property, as `arrival` is). Nothing else in the format changes.

**`custom.png`**: `rotj_core::levels::custom_picture(prg, chr, text) -> Result<Vec<u8>, String>`
draws a `custom.tsj`'s tiles (16 a row, `tilecount` rows rounded up; banks resolved with
`loaded_bank` as the area tilesets do) in four greys (`$0F $00 $10 $30`) for every palette class:
a preview only, since a metatile's colours are the room's preset. The game's startup tileset
writer, for every `<data>/packs/<pack>/custom.tsj`, writes `<data>/tilesets/packs/<pack>/custom.png`
when missing or stale; a failure is logged and ignored, as release 4's.

## Proof

- `cargo test -p rotj-core`:
  - the original pack still exports byte-exact, round-trips and imports to the assets the game
    reads (unchanged tests, now through the one import path);
  - ROM-less (CI has no ROM): the hand-made fixture's rooms map their entries onto written pages
    (the user-pack tests keep passing), and a "past the stream" case is refused as a user pack
    and kept as `Source::Original`; one test per validation rule above, each asserting the error
    names the file, layer and object or page;
  - ROM-less mixed stage: hand-made `Levels` with two areas through `Catalog::from_levels` and a
    `custom.tsj` import to the expected ids, corners, classes and `chr`, including an appended id
    in `palette_classes`' padding, a shared page whose rooms list the tilesets in different
    orders, a slot conflict that falls through to the next slot, a fifth bank refused, an area
    tile past its count refused, and a mixed stage `write` refuses;
  - every original room's needed slots hold its own `chr` bytes (ROM);
  - bad `custom.tsj` refused before reading: `tilecount` 209 and a huge one, wrong `columns`, a
    bad bank, tile, `palette` or `surface`;
  - `custom_picture` of a small `custom.tsj` equals the corners drawn by hand from `chr::tile`;
  - the 6 songs the original stages use are in the sound test's list (ROM).
- App: the startup writer draws a pack's `custom.png` and survives a bad `custom.tsj`.
- Tapes, chain and lockstep unchanged (full set).

## Not here

`--pack` and which area-keyed tables follow `stage.area` (release 6); exporting a mixed stage;
custom metatiles with colours of their own; per-tile collision on area tilesets; a `modern`
engine; a "Load pack" menu entry (with the editor and the overworld); refusing the runtime-written
ids above (about 10 lines if wanted).
