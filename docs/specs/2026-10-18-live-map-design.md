# The level's live map, typed

Status: approved with changes by the Opus stand-in, 2026-10-18 (applied: the types live in
`levelmap.rs` with its constants; the rows stored as `[[u8; 256]; 16]`; no `clear_columns`; the
carry past column `$FF` kept and tested; the `$6000` rule narrowed to map sites; the missed sites
added; `crumbling_block`'s `+` noted; edge-case tests; what C3 adds). Implemented in 1.1.13;
the reviewer's change: `Cell::in_page` adds the page column with the flat buffer's carry, as the
code did, rather than ORing it. Item 3 of HANDOFF's order
(Phase C, before C3): the owner's answer to `docs/specs/2026-10-18-phase-c-mirror-design.md` makes
C3's background a tilemap over the level's live map, so the map is typed first. No behaviour
change: the lockstep proves it, tapes unchanged.

## What the survey found

- `collision::Level::map: [u8; 4096]` is `$6000-$6FFF`: 16 rows of 256 metatile ids. Each row
  holds 16 slots of 16 columns; a slot is one 256x256 px page of the level. Which slot a level
  page sits in comes from the page table `Schedule::rows` (`$0440-$045F`, 32 entries: index
  `(inner & $10) | (outer >> 4)`, x and y swapped on vertical levels; value the slot's first
  column, `slot << 4`, which a few stage actors overwrite). The spawn schedule's 256 cells use
  the same table. `rows` is a misnomer: it holds base columns.
- Every reader and writer builds a flat `usize` itself: `row * 256 + base + column` (the
  streaming in `scroll.rs` three times, `room.rs` twice, `crumbling_block`), `collision::map_offset`
  (the probes, `Level::metatile`), a WRAM address minus `$6000` (`engine::copy_metatiles`,
  `breakable_block`'s source and target from its spawn record, `collapsing_floor_controller`), a
  fixed offset (`cell_painter`'s `$740`/`$7AF`), a 256-wide chunk walk (`column_eraser`).
- Two uses that are not the map: the stage-end countdown draws its glyphs into the first 640 bytes
  (`stage.rs`, `$6000-$627F`, as the game does) and reads them back; the WRAM projection copies
  the 4096 bytes.
- Four stage actors that edit the page table are named for "rows": `row_table_pair_patch`,
  `row_table_tail_set`, `row_table_tail_clear`, `row_base_reset`.
- `levelmap.rs` already owns the map's layout (`MAP_ADDR`, `MAP_COLUMNS`, `ROM_MAP_ROWS`),
  `transpose` (which returns `[[u8; 256]; 16]`) and `LevelMap`, the capture reader `rotj-tools`
  uses.
- The flat buffer carries: one column past `$FF` is the next row's column 0
  (`crumbling_block`'s `at..at + 2`, `collapsing_floor_controller`'s `at + 1`, the address sums in
  `copy_metatiles` and `breakable_block`).
- `crumbling_block` adds the page column (`base + column`) where `map_offset` ORs it; they agree
  because every page-table value has a zero low nibble (`page << 4`, and the actors' `$60`, `$70`,
  `0`).

## The design

- **`levelmap::Cell { row: u8, column: u8 }`**: a map cell, `row` 0-15, `column` 0-255 (a slot's
  base plus the column in its page). Built by:
  - `Cell::at(column, row, page_columns, vertical)`: the probes' rule, today's `map_offset`;
  - `Cell::from_address(addr)`: a `$6000-$6FFF` address, the form ROM tables and spawn records
    hold, summed in `u16` first as the game does (one outside it panics, as today's indexing does);
  - `Cell::next`: the cell to the right, carrying past column `$FF` into the next row's column 0,
    as the flat buffer does;
  - `Cell { row, column }` directly where the code has both, and `Cell::offset` for the raw-WRAM
    oracle test in `player.rs`.
- **`levelmap::Map`** owns the rows (`[[u8; 256]; 16]`, private, what `transpose` returns, so
  `Level::from_terrain` stops flattening), indexed by `Cell` (`Index`/`IndexMut`), with `rows_mut`
  for `column_eraser`'s loop and `as_bytes`/`as_bytes_mut` (`as_flattened`) for the WRAM
  projection and the countdown's glyphs (named there: "the countdown's glyphs over the map's
  first 640 bytes"). `Level::map` becomes a `Map`; `Level::metatile` and `streamed` read through it.
- **The page table**: `Schedule::rows` is renamed `Schedule::page_columns` (still `[u8; 32]`: the
  actors fill slices of it and `spawn.rs` reads it with `get`), with `levelmap::page_index(column,
  row, vertical)` the one rule that `Cell::at` and `engine::map_page` share. The four actors become
  `page_table_pair_patch`, `page_table_tail_set`, `page_table_tail_clear`, `page_column_reset`
  (now, before the level format publishes family names as actor classes), in `families.rs` and
  `docs/ACTORS.md` too.
- **Every map site** above moves to `Cell` and `Map`; no `usize` map offset and no `- 0x6000` for
  the map stays outside `levelmap.rs` (other WRAM keeps its own `0x6000` arithmetic).
- **For C3** (not built here): a level page's metatiles are `Map` cells from
  `Cell { row, column: page_columns[page] + c }`; its tilemap also needs `Level::tiles` and the
  attribute pages (indexed by `page_columns[page] >> 4`); C3 adds the accessors it needs.

## Proof

- The lockstep (the chain, the release suite, a sweep and a 300-branch exploration) unchanged; the
  tapes byte-identical.
- Edge-case tests in `levelmap.rs`: `Cell::next` of `{ r, $FF }` is `{ r + 1, 0 }`;
  `Cell::from_address` at `$6000`, `$60FF`, `$6101` and `$6FFF`, and a sum past a row
  (`$60FF + 1`); `page_index` on a horizontal and a vertical level, top and bottom half.

## Not here

C3's tilemap and the accessors it adds; typing the spawn schedule's cells and the attribute pages
(`Level::attributes`); folding `LevelMap` (the capture reader) into `Map` (logged in MODERNIZE).
