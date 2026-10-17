# Levels as assets, slice 1: the terrain (1.1.x step B1a)

Status: approved with changes by the Opus stand-in, 2026-10-17 night (applied: the terrain
holds the stream, all 208 classes and the palette classes, and the attributes are recomputed
over the overlaid map; `load_level` imports on each load, with no field on `Game`, no file
format and no build-script import in this slice; the equivalence tested over three images). Step B1 of
`docs/specs/2026-10-16-modernisation-roadmap-design.md` ("levels as assets", ahead of the ECS
steps because it is what the editor, bonus levels and kaizo levels need), first slice, from a
survey of the level pipeline. **Parked for the owner:** the public, authored format (Tiled TMX
or JSON, a binary, or both) and its schema; a level file a player can write is public
interface (`docs/PLAN.md` "Versioning"). This slice needs neither: it builds the internal
asset that any authored format is imported into.

## Goal

Today the game decodes each area's terrain from the ROM when the area loads
(`room::load_level` → `Level::loaded`, `collision.rs`): the LZ map stream in CHR, the
auto-tile corner tables, the attribute stream and the class table. This slice makes that
decode an **importer** that produces one **terrain asset** per area, and the game **loads
the asset** instead of decoding the ROM. Behaviour, RAM, WRAM and the tapes unchanged; the
lockstep proves it, since the terrain is exactly what it compares (WRAM `$6000` map, the
corner tables at `$7000`/`$70D0`/`$71A0`/`$7270`, classes at `$7340`, attributes at
`$7600`).

## Design

- `rotj_core::terrain` (new module):
  `pub struct Terrain { pub stream: Vec<u8>, pub corners: Vec<[u8; 4]>, pub classes:
  [u8; 208], pub palette_classes: Vec<u8> }`:
  - `stream`: the bytes the area's LZ map stream writes over `$7000`, in that order
    (`levelmap::map_reach` of them; a reach need not end on a page);
  - `corners`: the corner tiles of each of the area's `metatile_count` metatiles;
  - `classes`: the class table as the loader writes it, all 208 entries (area 17's run past
    its 102 metatiles, and ids from `$D0` read it, so the lockstep compares every entry);
  - `palette_classes`: each known metatile's 2-bit palette class (`classes_from_chr`),
    from which the attributes are computed: a palette per tile, the shape an editor authors.
- `Terrain::import(prg: &[u8], chr: &[u8], area: u8) -> Option<Terrain>` calls today's
  decoders, which stay public and where they are (the tools and the oracle's tests use
  them).
- `Level::from_terrain(terrain: &Terrain, image: &[u8]) -> Level`, in this order:
  1. `post` = the leftover image with `stream` written over its start;
  2. the map = `transpose(post)`;
  3. the corners from the asset, and those past `metatile_count` from `post` as today;
  4. the classes from the asset;
  5. the attributes recomputed from the overlaid map, the palette classes and `post`
     (`attributes_of`'s rule, which reads the leftover `$7B00` for ids past the known ones);
  6. the unread bytes packed from `post`.
  The `$7570` shadow then comes out as today. The LZ window starts at zero whatever the
  leftover, so the overlay is exact.
- `room::load_level` calls `Terrain::import` then `Level::from_terrain` on each load (the same
  cost as today); `Level::loaded` becomes that composition, its old body kept only in the
  tests as the reference.
- Tests: for every area, `from_terrain(import(..))` equals the old `loaded` over three
  images: the boot payload, the cutscene payload and a synthetic image (each byte a hash of
  its index), which exercises every leftover-dependent byte; the Classic suites and the
  tapes unchanged.

Where the terrain lives once it is not decoded from the ROM, and how it is stored, belong
with the authored format (the owner's decision). The seam then is a content value passed
beside `prg`/`chr` where they reach `load_level` (`enter_area`, `BossIntro`), not state on
`Game`, which must stay liftable from RAM alone.

## Next slices (each its own short spec)

1. Spawns: the `SpawnTable` per room (today decoded on every scroll crossing,
   `scroll.rs` `edge_spawn`) into the asset, with the 12-byte records typed.
2. Rooms: the room record (palette selector, CHR banks, scroll orientation), the start
   record, the raster parameters and the page order.
3. Collision: the class handlers as per-class properties (wall, ground, hazard) instead of
   6502 handler addresses.
4. The authored format and its importer into these assets (the owner's decision).

## Not here

The public format, spawns, rooms, collision properties, the behaviour tables.
