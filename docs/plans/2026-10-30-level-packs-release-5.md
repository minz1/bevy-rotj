# Level packs release 5 Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans to implement this plan task-by-task, in order. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `classic` validation for user packs, rooms and stages mixing area tilesets, and a pack's `custom.tsj` metatiles, with the original pack importing exactly as today.

**Architecture:** The importer keeps one path. A room's map lists several tilesets; each cell becomes a `Key` (stage-area id, another area's variant and id, or a custom id). The stream is built from keys, the stage area's ids stay, the other keys are appended after the stage area's metatiles, and one greedy pass gives each appended bank a stage-wide CHR slot. `custom.tsj` is parsed and checked in its own module, which also draws `custom.png`.

**Tech Stack:** Rust, serde_json, thiserror; the app's `image` crate for the PNG.

**Spec:** `docs/specs/2026-10-30-level-packs-release-5-design.md`

## Global Constraints

- `format` stays 1, nothing in the format is renamed; the original pack exports byte-exact, round-trips and imports to the assets the game reads (the ROM tests stay as they are, apart from `write(..)` now returning `Option`; Task 1 changed the ROM-less fixture, Task 3 rewrites the ROM-less cases `exactly one tileset` (now "gids overlap"), `gid 257 is not in the tileset` (now "not in a tileset") and `its tileset's area is not the stage's` (now mixing), Task 5 the project-file test).
- New refusals apply to `Source::User` only, except the stage-level ones (208 metatiles, CHR slots, area tile past its count when appending, `custom.tsj`), which no original stage can trip.
- Errors name the file, then the layer and object/cell/page: `rooms/x-1.tmj: layer \`terrain\`: entry 6 is page 15, past the stream's 11 pages`.
- `CLASS_COUNT` (208) is the metatile limit; `TILESET_TILES` (256) an area tileset's size.
- No game behaviour change: tapes, chain and lockstep unchanged (full set).
- No comments beyond one line where the code cannot say why; no lint suppressions.
- Checks each task: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test -p rotj-core` (and `-p batman-rotj` in Task 7). Check `$?`, never `| tail`.

## Review Focus

1. Gid ranges that overlap or a gid between two tilesets' ranges: refused "gid N is not in a tileset", never a panic (Task 3 test `overlapping_tilesets_are_refused`).
2. A `custom.tsj` with a duplicate tile `id` or an `id` at or past `tilecount`: refused naming the id (Task 5 test `bad_custom_tilesets_are_refused`).
3. A room naming `custom` with no `custom.tsj` in the pack: `ImportError::Missing { file: "custom.tsj" }` (Task 5).
4. A room whose only tilesets are custom or another area's, while it is not the first room: imports with the stage area's first variant as its base `chr` (Task 4 test `a_room_with_no_stage_area_tileset_takes_the_first_variant`).
5. A stage-area tile id at or past the area's count in a stage that does not append: still imports as today (no new refusal; Task 3 test covers only the appending case, plus the existing equivalence).

---

### Task 1: Rooms showing pages past the stream are refused (user packs)

**Files:**
- Modify: `crates/rotj-core/src/levels/import.rs` (`import_stage`)
- Test: `crates/rotj-core/src/levels/tests.rs` (`hand_made`, new test)

**Interfaces:**
- Produces: `fn held_entries(shape: &Shape) -> impl Iterator<Item = usize>` in import.rs: the page-table entries (0-31) whose `region_place` lies inside the map (`along < shape.along && band < shape.across`). Task 4 reuses it.

- [x] **Step 1: Fix the fixture.** In `hand_made`, the stream grows to 4 pages, so room A's held entries 2-15 show a written page 3; page 4 (room C's entries 16-31, outside its map) stays past the stream. (Remapping A onto page 1 multiplied its triggers; D onto page 2 removed A's extra page.) The "tiles after" case now names page 3. The ROM half of the `no_handler` test accepts either refusal: the original pack as a user pack now first fails on 2-1-1's past-stream entry.
- [x] **Step 2: Write the failing test** `a_room_showing_a_page_past_the_stream_is_refused_as_a_user_pack`: A's `pages[6] = 4` → `rooms/9-9-1.tmj: layer \`terrain\`: entry 6 is page 4, past the stream's 4 pages`; imports as `Source::Original`.
- [x] **Step 3: Run it, expect FAIL** (imports Ok). `cargo test -p rotj-core past_the_stream`
- [x] **Step 4: Implement.** In `import_stage`, after `stream(..)`, `let written = stream.len() / PAGE_CELLS;` and for `Source::User`, for each room and each `held_entries(shape)` entry whose page `>= written`, return `invalid(file, format!("layer `terrain`: entry {entry} is page {page}, past the stream's {written} pages"))`. Thread `source` as today.
- [x] **Step 5: Run checks, expect PASS.**
- [x] **Step 6: Commit** `Level packs: a user pack's room showing a page past the stream is refused`

### Task 2: Knob values must exist in the game (user packs)

**Files:**
- Modify: `crates/rotj-core/src/levels.rs` (`Catalog`, `from_rom`, `from_levels`)
- Modify: `crates/rotj-core/src/actor/sound_test.rs` (expose the song list)
- Modify: `crates/rotj-core/src/levels/import.rs` (`descriptor`, `import_room`)
- Test: `crates/rotj-core/src/levels/tests.rs`

**Interfaces:**
- Produces: `Catalog` fields `background_palettes`, `sprite_palettes`, `pictures`, `songs: BTreeSet<u8>`. `export::project` reads the palette sets from the catalog instead of recomputing them from the levels.
- Produces: `pub(crate) fn songs(prg: &[u8]) -> [u8; SONG_COUNT as usize]` in sound_test.rs (`rom_byte(prg, 12, SONGS + i)`).

- [ ] **Step 1: Write the failing tests** (ROM-less, on `hand_made`, whose catalog has `bg`/`sprites` selectors of `room()`, picture `0x05`, song `0x03`):
  - `a_user_packs_knobs_must_be_values_the_game_uses`: rewrite `pack.json`'s stage `music` to `song-04` → `pack.json: stage \`9-9\`: \`music\`: \`song-04\` is not one of the sound test's songs`; `cutscene` `picture-06` → `... \`cutscene\`: \`picture-06\` is not a picture the game's stages show`; a room's `background_palette` to an unused `bg-xx` → `rooms/9-9-1.tmj: property \`background_palette\`: \`bg-xx\` is not a preset the game's rooms use` (same for `sprite_palette`). Each also imports as `Source::Original`.
  - ROM: `the_original_stages_songs_are_in_the_sound_test`: the set of `Music::Song` values over `Levels::from_rom` has 6 members, all in `catalog.songs` from `Catalog::from_rom`, which has 13.
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement.** `from_levels` fills the sets from the levels (songs: the stages' `Music::Song` values); `from_rom` replaces `songs` with `sound_test::songs(prg)`. Pass `catalog` and `source` into `descriptor` and `import_room`; check only for `Source::User`. `Music::Keep` is always fine.
- [ ] **Step 4: Run checks, expect PASS.**
- [ ] **Step 5: Commit** `Level packs: a user pack's palettes, pictures and songs must be the game's`

### Task 3: Rooms list several tilesets; cells become keys

**Files:**
- Modify: `crates/rotj-core/src/levels/import.rs` (`Shape`, `shape`, `region_tiles`, `Tiles`, `stream`, `import_stage`, `import_room`)
- Test: `crates/rotj-core/src/levels/tests.rs`

**Interfaces:**
- Produces, in import.rs:

```rust
/// A tileset a room's map names.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TilesetKind { Area { area: u8, chr: [u8; 4] }, Custom }
struct RoomTileset { firstgid: u32, count: u32, kind: TilesetKind, name: String }
/// A cell's metatile: the stage area's id (any variant), another area variant's id, or a custom id.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Key { Stage(u8), Other { area: u8, chr: [u8; 4], id: u8 }, Custom(u8) }
```

  `Shape` drops `firstgid`/`area`/`chr` for `tilesets: Vec<RoomTileset>` sorted by `firstgid`. `Tiles::Metatiles(Vec<Key>)`. `fn stage_area(shape: &Shape) -> Option<(u8, [u8; 4])>` = its lowest-`firstgid` area tileset. `fn own_variant(shape: &Shape, area: u8) -> Option<[u8; 4]>`. `stream(..)` returns the keyed pages `Vec<Vec<Key>>` (written pages only) plus each page's first region `(room, entry)`; Task 4 turns keys into bytes.
- In this task, any key other than `Key::Stage` is refused with `layer \`terrain\`: tile ({x}, {y}) is from another area's tileset` (temporary; Task 4 removes it), so the stream bytes are the `Stage` ids.

- [ ] **Step 1: Write the failing tests**
  - `a_room_may_list_several_variants_of_its_area`: hand-made levels with two variants of area 0 (rooms with `chr` `[1,2,3,4]` and `[1,2,3,5]`, which agree on slot 0, the only slot the fixture's corners use, so Task 4 keeps them importing; `Catalog::from_levels` makes `area-01` and `area-01-b`); in room B's map add a second tileset ref `area-01` at `firstgid` 257 and repaint a page with gids `257 + id` instead of `1 + id`; it imports to the same `Levels` as before the repaint.
  - `rooms_sharing_a_page_agree_by_key_whatever_their_tileset_order` (same two variants): room A lists `area-01` then `area-01-b`, room B the reverse (firstgids swapped, gids rewritten); the shared page imports.
  - `overlapping_tilesets_are_refused`: two refs at firstgid 1 and 100 → `rooms/..: two tilesets' gids overlap`; a gid past the last range → `layer \`terrain\`: gid N is not in a tileset`; a ref at firstgid 4294967295 → refused, not a panic.
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement.** `shape`: every ref resolves by file stem: `custom` → `Custom` (in this task refused as an unknown tileset; Task 5 reads `custom.tsj` once in `import` when any room names it and passes its parsed tiles down, `tilecount` giving the count), else `catalog.tileset(name)` → `Area`; at most one `custom`; `firstgid >= 1`; sort by `firstgid`; refuse overlaps, computing range ends in `u64`; a first room with no area tileset → `rooms/x.tmj: the stage's first room names no area tileset`. A gid's tileset is the last with `firstgid <= gid`, local id `< count`. The stage area is the first room's `stage_area`; a `Area` tileset of that area keys `Stage(id)`, another area's `Other`, `custom` `Custom`. Rooms disagree by key (the existing "disagrees with page N's first region" error). `import_room`'s `chr` comes from Task 4; in this task, the room's own variant (else the stage area's first variant).
- [ ] **Step 4: Run checks, expect PASS** (the ROM equivalence and round-trip tests too).
- [ ] **Step 5: Commit** `Level packs: a room lists several tilesets of its area, cells keyed by metatile`

### Task 4: Appended metatiles, CHR slots and each room's `chr`

**Files:**
- Modify: `crates/rotj-core/src/levels/import.rs`
- Test: `crates/rotj-core/src/levels/tests.rs`

**Interfaces:**
- Consumes: Task 3's `Key`, `Shape.tilesets`, `held_entries` (Task 1), the catalog's per-area `Terrain` and variants.
- Produces: `pub(crate) struct Metatile { corners: [(u8, u8); 4], palette: u8, class: u8 }` (corner = bank byte as `Room::chr` stores it, tile 0-63; Task 5's `custom.tsj` tiles are this type too) and `fn appended(key: Key, catalog: &Catalog, custom: &[Option<Metatile>]) -> Metatile` (Task 5 adds the `Custom` arm; here `Other` only: corner `c` of its area's `corners[id]` → `(chr[c >> 6], c & 63)`, class and palette class of its area's terrain). `fn assign_slots(..) -> Result<(Vec<[u8; 4]> per room, BTreeMap<u8, u8> bank → slot), ImportError>` (errors built with `invalid`/`at_stage`).

- [ ] **Step 1: Write the failing tests** with a ROM-less two-stage fixture `two_areas()`: stage `1-1` area 0 with 6 metatiles whose corners use slots 0 and 1 only and 8 palette classes, rooms `1-1-1` `chr` `[1,2,3,4]` (variant a), `1-1-2` `[1,2,0x13,9]` (variant b, agrees on slots 0-1), `1-1-3` `[7,2,3,4]` (variant c, differs on slot 0); stage `2-1` area 1 with 210 metatiles, `chr` `[0x10,0x11,0x12,0x13]`, metatile k's corners all in slot 2 (k even) or slot 3 (k odd), except metatile 5's in slots 0 and 1. A helper paints another area's cells into a stage-1 room via a second tileset ref (`area-02`, firstgid 257).
  - `mixing_appends_another_areas_metatiles_after_the_stage_areas`: area-02 ids 3 then 1 painted in stream order → stage-1 `terrain.corners.len() == 8`, id 6 is area 1's metatile 3 with each corner byte `slot * 64 + (c & 63)`, `classes[6]` and `palette_classes[6]` its area-1 values; ids 0-5 unchanged; the room's `chr` holds the appended banks at their slots.
  - `an_appended_id_in_the_padding_overwrites_it`: id 6 and 7 land in `palette_classes`' padding; its length stays 8; a ninth metatile pads it to 12 with 0.
  - `a_bank_whose_slot_is_taken_falls_through_to_the_next_slot`: 1-1-1 paints area-02 ids 2 then 3 (banks `0x12`, `0x13`), 1-1-2 id 3; `0x12` takes slot 2; `0x13`'s candidate slot 2 (1-1-2 gives it) holds `0x12` in 1-1-1 → it takes slot 3; 1-1-1 `chr` `[1,2,0x12,0x13]`, 1-1-2 `[1,2,0x13,0x13]`.
  - `a_fifth_bank_is_refused`: 1-1-1 paints area-02 ids 2, 3, 5 → `pack.json: stage \`1-1\`: bank $10 finds no CHR slot free in every room that uses it (rooms 1-1-1)`.
  - `an_area_tile_past_its_count_is_refused_when_the_stage_appends`: stage-area gid for id 6 (count 6) plus one appended cell → `rooms/1-1-1.tmj: layer \`terrain\`: tile (x, y) is metatile 6, past area-01's 6`; an `area-02` gid for id 210 → `... is metatile 210, past area-02's 210` (never a panic).
  - `more_than_208_metatiles_are_refused`: 203 distinct area-02 ids painted into stage 1 → `pack.json: stage \`1-1\`: 209 metatiles, past the 208 the class table holds`.
  - `two_variants_disagreeing_on_a_needed_slot_are_refused`: 1-1-1 listing `area-01` and `area-01-c` (differ on slot 0) → `rooms/1-1-1.tmj: tilesets \`area-01\` and \`area-01-c\` disagree on CHR slot 0, which the room needs`.
  - `a_room_with_no_stage_area_tileset_takes_the_first_variant`: 1-1-2 listing only `area-02` gets variant a's bytes `[1,2,3,4]` in its unneeded slots, not its own former `[1,2,0x13,9]`.
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement** in `import_stage`, after the keyed stream:
  1. Ids: `Stage(id)` → `id`; other keys numbered from `terrain.corners.len()` in first stream appearance. An `Other` id at or past its area's count, or a `Custom` id `custom.tsj` does not define (Task 5), is refused here; with any appended key, so is a `Stage(id)` with `id >= corners.len()` (tile coordinates from the page's first region via `tile_index`). Only when the stage appends: a total past `CLASS_COUNT` → refused (`hand_made` keeps its 251 metatiles).
  2. Slots: `S` = the slots (`c >> 6`) of every stage-area corner. Each room's base = its own variant, else the stage area's first variant; each room needs `base[s]` for every `s` in `S`; a room's stage-area tilesets must agree on those slots. Rooms needing an appended bank: rooms whose held entries' (and extra pages') written pages hold an appended key with that bank. Banks in first-use order (appended ids ascending, corners TL, TR, BL, BR); for each, candidates = slots where some room needing it already gives that bank (its needed or base byte; ascending), then the rest (ascending); take the first slot that is free or that bank in every room needing it; assign it in those rooms. No slot → refused, rooms named by file stem.
  3. Terrain: push each appended metatile's corners (`slot_of[bank] * 64 + tile`), set `classes[id]`, `palette_classes` (resize to the next multiple of 4 past `id` with 0 when `id` is past its length). Stream bytes from ids.
  4. Each room's `chr[s]` = what it needs, else its base byte. Remove Task 3's temporary refusal.
- [ ] **Step 4: Run checks, expect PASS**, including the ROM equivalence (every original room's `chr` is its own: the equivalence compares `room`, `chr` included).
- [ ] **Step 5: Commit** `Level packs: a stage mixes areas, appended metatiles get stage-wide CHR slots`

### Task 5: `custom.tsj`

**Files:**
- Create: `crates/rotj-core/src/levels/custom.rs` (model, validation, `SURFACES`, `custom_picture`)
- Modify: `crates/rotj-core/src/levels.rs` (`pub mod custom; pub use custom::custom_picture;`)
- Modify: `crates/rotj-core/src/levels/import.rs` (read `custom.tsj` when a room names it; `Custom` arm of `appended`)
- Modify: `crates/rotj-core/src/levels/export.rs` (`project`: `enumeration("surface", SURFACES names)`)
- Test: `crates/rotj-core/src/levels/custom.rs` (`#[cfg(test)] mod tests`), `tests.rs` for the import

**Interfaces:**
- Produces:

```rust
/// The `surface` enum's names and the collision class each writes (a name covering several classes is its lowest).
pub(crate) const SURFACES: [(&str, u8); 13] = [("solid", 0), ("belt-right", 1), ("belt-left", 2),
    ("hazard-behind", 3), ("solid-hazard", 4), ("slippery", 5), ("behind", 6), ("open", 7),
    ("solid-to-bodies", 11), ("crumbling", 12), ("one-way", 14), ("one-way-behind", 15), ("spikes", 16)];
/// `custom.tsj`'s tiles by id (`None` where a tile has no definition), checked before anything is read.
pub(crate) fn parse(text: &str, handles: &BTreeSet<u8>) -> Result<Vec<Option<Metatile>>, String>;
pub fn custom_picture(prg: &[u8], chr: &[u8], text: &str) -> Result<Vec<u8>, String>;
```

  `Custom` is Task 4's `Metatile` (no separate type). `Catalog::handles(&self) -> BTreeSet<u8>`: every variant byte with bit 7. `parse` reads a serde struct with `columns: u64`, `tilecount: u64`, `tiles: Vec<{ id: u64, properties: Vec<Property> }>` (default empty); errors are `tile {id}: ...` or about the whole file; the importer wraps them as `invalid("custom.tsj", ..)`.
- [ ] **Step 1: Write the failing tests** (custom.rs, ROM-less): `bad_custom_tilesets_are_refused` with one case each and its message: `tilecount 209, past the 208 a stage's metatiles hold`; `tilecount` `18446744073709551615` (same message form); `columns 8, not 16`; `tile 0: corner \`2C:05\` is not \`bb:tt\` in lowercase hex`; `tile 0: tile $40 is past $3f`; `tile 0: bank $85 is not an animation handle the game's rooms load`; `tile 0: \`palette\` 4 is not 0-3`; `tile 0: unknown \`surface\` \`lava\``; `tile 3 is defined twice`; `tile 9 is past tilecount 4`. `a_good_custom_tileset_parses`: `["2c:05","2c:06","02:3f","81:00"]` with handle `0x81`, palette 2, surface `open` → `Metatile { corners: [(0x2c,5),(0x2c,6),(2,0x3f),(0x81,0)], palette: 2, class: 7 }`, undefined ids `None`.
  In tests.rs: `custom_metatiles_import_after_the_stage_areas` (Task 4's fixture plus a `custom.tsj` with two tiles painted from a `custom` ref): ids, corners, `classes` from `surface`, `palette_classes` from `palette`; `a_used_custom_tile_with_no_definition_is_refused` → `rooms/1-1-1.tmj: layer \`terrain\`: tile (x, y) is custom tile 1, which \`custom.tsj\` does not define`; a room naming `custom` with no file → `custom.tsj: missing`; `a_first_room_with_no_area_tileset_is_refused`: first room lists only `custom` (with a `custom.tsj`) → `rooms/1-1-1.tmj: the stage's first room names no area tileset`. Update `the_project_file_declares_every_class_and_value_the_rooms_use` to expect the `surface` enum.
  ROM test `custom_picture_draws_the_corners_from_chr`: a one-tile `custom.tsj` drawn equals, pixel by pixel, the four `chr::tile` patterns with `banks = [loaded_bank(prg, bank); 4]` in the greys `rgba` of `$0F $00 $10 $30`; picture size `256 * 16 * 4` bytes.
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement.** `parse` checks `columns == 16` and `tilecount <= CLASS_COUNT` before touching `tiles`. `import` reads and parses `custom.tsj` once when any room names `custom`. `custom_picture` builds `Catalog::from_rom` for the handles (`Err("the ROM's levels do not import")` if it cannot), then draws 16 tiles a row, `tilecount.div_ceil(16)` rows, undefined tiles clear.
- [ ] **Step 4: Run checks, expect PASS.**
- [ ] **Step 5: Commit** `Level packs: custom metatiles from a pack's custom.tsj`

### Task 6: `write` refuses a mixed stage

**Files:**
- Modify: `crates/rotj-core/src/levels/export.rs` (`write`, `export`, the panic doc)
- Modify: every `write(..)` caller (`rg -n 'write\(&' crates tools`; tests take `.expect("the original game")`)
- Test: `crates/rotj-core/src/levels/tests.rs`

**Interfaces:**
- Produces: `pub fn write(levels: &Levels, catalog: &Catalog) -> Option<PackFiles>`: `None` when a stage's terrain (stream aside) is not its catalog area's, or a room's `chr` is not one of the area's variants (no longer a panic).
- [ ] **Step 1: Failing test** `write_refuses_a_mixed_stage`: import Task 4's mixed pack, `assert_eq!(write(&imported, &catalog), None)`; the unmixed `hand_made` still writes `Some`.
- [ ] **Step 2: Run, expect FAIL** (compile error is fine as the red step).
- [ ] **Step 3: Implement; update callers.** May land before Task 4's commit if Task 4's tests need it.
- [ ] **Step 4: Run checks, expect PASS.**
- [ ] **Step 5: Commit** `Level packs: write refuses a stage whose terrain is not its area's`

### Task 7: The startup writer draws each pack's `custom.png`

**Files:**
- Modify: `crates/app/src/tilesets.rs` (`write`)

**Interfaces:**
- Consumes: `rotj_core::levels::custom_picture`.
- [ ] **Step 1: Failing test** `draws_a_packs_custom_png_and_survives_a_bad_one` (ROM): `dir/packs/a/custom.tsj` good and `dir/packs/b/custom.tsj` with `columns: 8`; after `write`, `dir/tilesets/packs/a/custom.png` decodes to `256 x 16 * rows`, `packs/b/custom.png` is absent, and `write` returned `Ok`.
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement.** After the area tilesets, for each directory in `dir/packs` with a `custom.tsj`: `custom_picture`, encode PNG, `update(..)` into `dir/tilesets/packs/<pack>/custom.png` (create the folder); a failure for one pack is `warn!`-logged with its path and skipped. A missing `dir/packs` is fine.
- [ ] **Step 4: Run** `cargo test -p batman-rotj tilesets` and the workspace clippy, expect PASS.
- [ ] **Step 5: Commit** `Level packs: the startup writer draws each pack's custom.png`

### After the tasks

`reviewer` over the branch; full set in `rotj-map`; release 1.1.52 from `rotj-rel` per `.autopilot/rules.md` (STATUS, PLAN, the level-packs spec's "Releases, in order" 5 "Implemented in 1.1.52", the release-5 spec's Status).
