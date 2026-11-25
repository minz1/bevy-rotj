//! Level maps, terrain, collision surfaces, and room layouts.
//!
//! Unifies the metatile map (`$6000-$6FFF`), its auto-tile and attribute tables
//! (`$7000-$79FF`), the collision-class table (`$7340`), per-area [`Terrain`] assets,
//! and per-room [`Room`] layouts. ROM decoders live in [`rom`].

pub mod rom;
pub mod room;

pub use rom::{
    AREA_COUNT, BOOT_FIRST_BANK, LEVEL_AREA_TABLE, PAYLOAD_FIRST_BANK, PAYLOAD_TAIL_BANK,
    PAYLOAD_TAIL_BYTES, PAYLOAD_TAIL_OFFSET, area_bank, area_for_level, area_ppu,
    attributes_from_classes, attributes_from_rom, attributes_of, auto_tile_tables, boot_image,
    class_table, classes_from_chr, map_from_rom, map_over, metatile_count, payload_image,
    streamed_attributes, streamed_map, transpose,
};
pub(crate) use room::listed_rooms;
pub use room::{
    Arrival, Chain, ChrBanks, Facing, Orientation, Raster, Room, ScrollLock, ScrollMode, Start,
};

/// Metatile rows in the map (`$6000-$6EFF`), one 256-byte page each.
pub const MAP_ROWS: usize = 15;
/// Metatile columns in the map, one byte per page.
pub const MAP_COLUMNS: usize = 256;
/// One physical nametable (`$2000-$23FF`).
pub const NAMETABLE_BYTES: usize = 1024;
/// Tiles per nametable row (and the ring length in tiles).
pub const NAMETABLE_COLUMNS: usize = 32;
/// Metatile edge in NES pixels.
pub const METATILE_PIXELS: usize = 16;
/// The sublevel's full width: every metatile column, i.e. 4096 px.
pub const LEVEL_WIDTH_PIXELS: usize = MAP_COLUMNS * METATILE_PIXELS;
/// CPU/WRAM address of the map's first byte.
pub const MAP_ADDR: u16 = 0x6000;
/// Addresses of the four auto-tile tables: top-left, top-right, bottom-left,
/// bottom-right (bank 0D `sub_B351`).
pub const TILE_TABLE_ADDRS: [u16; 4] = [0x7000, 0x70D0, 0x71A0, 0x7270];
/// Bytes per auto-tile table (one per metatile id).
pub const TILE_TABLE_BYTES: usize = 256;
/// Metatile rows the ROM map decoder produces (`$6000-$6FFF`): 16 pages. The
/// renderer only reads rows 0-14 ([`MAP_ROWS`]).
pub const ROM_MAP_ROWS: usize = 16;
/// Bytes in one nametable's attribute table (`$23C0-$23FF`): 8x8 quads.
pub const ATTRIBUTE_BYTES: usize = 64;
/// 256 px pages in the map's width, i.e. attribute tables in WRAM `$7600-$79FF`.
pub const ATTRIBUTE_PAGES: usize = MAP_COLUMNS / 16;
/// The loader fills `$D0` = 208 entries; ids `>= 208` are outside the level.
pub const CLASS_COUNT: usize = 208;
/// The `$7000`-relative ranges of [`Level::unread`].
pub const UNREAD: [std::ops::Range<usize>; 3] = [0x510..0x530, 0x5B0..0x600, 0xA00..0xB00];
const UNREAD_BYTES: usize = 0x20 + 0x50 + 0x100;

/// A cell of the live map: `row` 0-15, `column` 0-255 (a page's first column from the page
/// table plus the column in the page).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// 0-15.
    pub row: u8,
    /// 0-255.
    pub column: u8,
}

impl Cell {
    /// The cell the game's probes read for a point's 16 px column and row (`$C99D`'s
    /// `$2A/$2B`): the page from [`page_index`], its first column from `page_columns`.
    #[must_use]
    pub fn at(column: u8, row: u8, page_columns: &[u8; 32], vertical: bool) -> Cell {
        let page = page_index(column, row, vertical);
        Cell {
            row: row & 0x0F,
            column: (column & 0x0F) | page_columns[usize::from(page)],
        }
    }

    /// The cell `column` (0-15) past the page's first column `page_column`, carrying into
    /// the next row as the flat buffer does.
    #[must_use]
    pub fn in_page(row: u8, page_column: u8, column: u8) -> Cell {
        let offset = u16::from(row) << 8 | u16::from(page_column);
        let [row, column] = (offset + u16::from(column)).to_be_bytes();
        Cell { row, column }
    }

    /// The cell at a map address (`$6000-$6FFF`), the form ROM tables and spawn records hold.
    ///
    /// # Panics
    /// On an address outside the map.
    #[must_use]
    pub fn from_address(address: u16) -> Cell {
        let offset = address
            .checked_sub(MAP_ADDR)
            .filter(|&offset| offset < 0x1000)
            .unwrap_or_else(|| panic!("${address:04X} is outside the map"));
        let [row, column] = offset.to_be_bytes();
        Cell { row, column }
    }

    /// The cell to the right; past column `$FF` the next row's first, as the flat buffer runs.
    #[must_use]
    pub fn next(self) -> Cell {
        let [row, column] = (u16::from_be_bytes([self.row, self.column]) + 1).to_be_bytes();
        Cell { row, column }
    }

    /// The cell's offset from `$6000`.
    #[cfg(test)]
    pub(crate) fn offset(self) -> usize {
        usize::from(self.row) << 8 | usize::from(self.column)
    }
}

/// The page-table index of a point's 16 px column and row: `(inner & $10) | (outer >> 4)`,
/// the column outer on horizontal levels and the row on vertical ones.
#[must_use]
pub fn page_index(column: u8, row: u8, vertical: bool) -> u8 {
    let (outer, inner) = if vertical {
        (row, column)
    } else {
        (column, row)
    };
    (inner & 0x10) | (outer >> 4)
}

/// The level's live map (`$6000-$6FFF`): 16 rows of 256 metatile ids, each row every page's
/// 16 columns, a level page's place given by the page table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Map {
    rows: [[u8; MAP_COLUMNS]; ROM_MAP_ROWS],
}

impl Default for Map {
    fn default() -> Self {
        Map::from_rows([[0; MAP_COLUMNS]; ROM_MAP_ROWS])
    }
}

impl Map {
    /// The map from its 16 rows.
    #[must_use]
    pub fn from_rows(rows: [[u8; MAP_COLUMNS]; ROM_MAP_ROWS]) -> Map {
        Map { rows }
    }

    /// The map as WRAM holds it.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; 0x1000]) -> Map {
        let (rows, _) = bytes.as_chunks::<MAP_COLUMNS>();
        Map::from_rows(rows.try_into().expect("16 rows"))
    }

    /// The 16 rows.
    #[must_use]
    pub fn rows(&self) -> &[[u8; MAP_COLUMNS]; ROM_MAP_ROWS] {
        &self.rows
    }

    /// The 16 rows.
    pub fn rows_mut(&mut self) -> &mut [[u8; MAP_COLUMNS]; ROM_MAP_ROWS] {
        &mut self.rows
    }

    /// The bytes as WRAM holds them.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.rows.as_flattened()
    }

    /// The bytes as WRAM holds them, to write.
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        self.rows.as_flattened_mut()
    }

    /// Render one physical nametable (`$2000-$23FF`) with the camera at `scroll_x`, the
    /// auto-tile tables read from `tables`, the `$7000-$7FFF` image.
    ///
    /// Only the 30 tile rows the map covers are written; rows 30-31 stay `$00`,
    /// matching the captures.
    #[must_use]
    pub fn render_nametable(&self, tables: &[u8; 0x1000], scroll_x: u16) -> [u8; NAMETABLE_BYTES] {
        let tile = |table: usize, metatile: u8| {
            tables[usize::from(TILE_TABLE_ADDRS[table] - 0x7000) + usize::from(metatile)]
        };
        let tiles = MAP_COLUMNS * 2;
        let left = (usize::from(scroll_x) / 8) % tiles;
        let mut nametable = [0u8; NAMETABLE_BYTES];
        for p in 0..NAMETABLE_COLUMNS {
            let delta = (p + NAMETABLE_COLUMNS - left % NAMETABLE_COLUMNS) % NAMETABLE_COLUMNS;
            let column = (left + delta) % tiles;
            let half = column % 2;
            let metatile_col = column / 2;
            for row in 0..MAP_ROWS {
                let metatile = self.rows[row][metatile_col];
                nametable[2 * row * NAMETABLE_COLUMNS + p] = tile(half, metatile);
                nametable[(2 * row + 1) * NAMETABLE_COLUMNS + p] = tile(2 + half, metatile);
            }
        }
        nametable
    }
}

impl std::ops::Index<Cell> for Map {
    type Output = u8;

    fn index(&self, cell: Cell) -> &u8 {
        &self.rows[usize::from(cell.row)][usize::from(cell.column)]
    }
}

impl std::ops::IndexMut<Cell> for Map {
    fn index_mut(&mut self, cell: Cell) -> &mut u8 {
        &mut self.rows[usize::from(cell.row)][usize::from(cell.column)]
    }
}

/// What a metatile's collision class does to the game's three probes: the wall probe
/// (`$C99D`, handlers at `$CADC`), the floor probe (`$CAFE`, `$CD2E`) and the point test
/// (`$C93B`, the pairs at `$C97D`). One variant per distinct set of handlers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// Blocks everything (class 0).
    Solid,
    /// Solid; landing on it carries the actor right (class 1).
    BeltRight,
    /// Solid; landing on it carries the actor left (class 2).
    BeltLeft,
    /// Passable; touching it hurts and draws the actor behind the scenery (class 3).
    HazardBehind,
    /// Solid; touching it hurts (class 4).
    SolidHazard,
    /// Solid; the floor keeps little of the actor's sideways speed (class 5).
    Slippery,
    /// Passable; the actor draws behind the scenery (class 6).
    Behind,
    /// Passable (classes 7-10).
    Open,
    /// Blocks the wall and floor probes; the point test answers clear (classes 11, 13).
    SolidToBodies,
    /// Clear to the wall probe; a block that starts to fall when landed on from above
    /// (class 12).
    Crumbling,
    /// A platform landed on from above only (class 14).
    OneWay,
    /// A platform landed on from above only; the wall probe draws the actor behind the
    /// scenery (class 15).
    OneWayBehind,
    /// Spikes: solid, and touching them hurts badly (class 16).
    Spikes,
}

impl Surface {
    /// The surface of a class byte as `$7340` stores it (`2 * class`), or `None` for an
    /// odd byte or one past class 16 (a schedule cell read for an id from `$D0`).
    #[must_use]
    pub const fn from_stored(stored: u8) -> Option<Surface> {
        if stored & 1 != 0 {
            return None;
        }
        Some(match stored >> 1 {
            0 => Surface::Solid,
            1 => Surface::BeltRight,
            2 => Surface::BeltLeft,
            3 => Surface::HazardBehind,
            4 => Surface::SolidHazard,
            5 => Surface::Slippery,
            6 => Surface::Behind,
            7..=10 => Surface::Open,
            11 | 13 => Surface::SolidToBodies,
            12 => Surface::Crumbling,
            14 => Surface::OneWay,
            15 => Surface::OneWayBehind,
            16 => Surface::Spikes,
            _ => return None,
        })
    }
}

/// One area's terrain as data: what the loader `$89FE` would decode from the ROM, imported
/// once, so the game builds its [`Level`] from data rather than from the ROM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Terrain {
    /// The bytes the map stream writes over `$7000`, in that order.
    pub stream: Vec<u8>,
    /// Each metatile's nametable tiles: top-left, top-right, bottom-left, bottom-right.
    pub corners: Vec<[u8; 4]>,
    /// The collision class of every id the class table covers, past the metatiles too
    /// (ids from `$D0` read it).
    pub classes: [u8; CLASS_COUNT],
    /// Each metatile's 2-bit palette class, by id.
    pub palette_classes: Vec<u8>,
}

impl Terrain {
    /// Area `area`'s terrain decoded from the ROM, or `None` past the areas.
    #[must_use]
    pub fn import(prg: &[u8], chr: &[u8], area: usize) -> Option<Terrain> {
        let tables = auto_tile_tables(prg, chr, area)?;
        let corners = (0..metatile_count(prg, chr, area)?)
            .map(|id| std::array::from_fn(|table| tables[table][id]))
            .collect();
        let (palette_classes, known) = classes_from_chr(prg, chr, area)?;
        Some(Terrain {
            stream: streamed_map(prg, chr, area)?,
            corners,
            classes: class_table(prg, area)?,
            palette_classes: palette_classes[..known].to_vec(),
        })
    }
}

/// The level's terrain as WRAM holds it: the map (`$6000-$6FFF`), each metatile's
/// tiles (`$7000`), the class table (`$7340`, stored as `2 * class`) and the attribute
/// pages (`$7600`). The one owner: the probes read it and the actors that rebuild the
/// level write it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Level {
    /// The live map.
    pub map: Map,
    /// Each metatile id's nametable tiles: top-left, top-right, bottom-left, bottom-right.
    pub corners: [[u8; 4]; CLASS_COUNT],
    /// `2 * class` by metatile id; ids from `$D0` read past it (the schedule).
    pub classes: [u8; CLASS_COUNT],
    /// Attribute bytes, 64 per 16-column page (`$7600-$79FF`).
    pub attributes: [u8; 1024],
    /// The bytes of `$7000-$7FFF` the loader leaves and nothing else writes, in the
    /// order of [`UNREAD`]: the stream's, or what WRAM held before it.
    pub unread: [u8; UNREAD_BYTES],
}

/// The map cell the loader's transpose moves `$7000 + at` to.
pub(crate) fn streamed_cell(at: usize) -> Cell {
    let [row, column] = [(at >> 4) & 0x0F, (at >> 8) << 4 | (at & 0x0F)]
        .map(|part| u8::try_from(part).expect("within $7000-$7FFF"));
    Cell { row, column }
}

impl Default for Level {
    fn default() -> Self {
        Level {
            map: Map::default(),
            corners: [[0; 4]; CLASS_COUNT],
            classes: [0; CLASS_COUNT],
            attributes: [0; 1024],
            unread: [0; UNREAD_BYTES],
        }
    }
}

impl Level {
    /// Area `area`'s terrain as the loader leaves it over the payload's bytes (the
    /// first load after boot), from the ROM's PRG tables and CHR ([`rom`]).
    #[must_use]
    pub fn from_rom(prg: &[u8], chr: &[u8], area: usize) -> Option<Level> {
        let image = payload_image(chr)?;
        Some(Level::from_terrain(
            &Terrain::import(prg, chr, area)?,
            image,
        ))
    }

    /// A terrain built over `image`, the `$7000-$7FFF` the loader finds: the stream written
    /// over it, the whole of it transposed into the map, the tile tables past the area's
    /// metatiles, the attributes of ids past the known palette classes and
    /// [`Level::unread`] from what the stream does not reach.
    ///
    /// # Panics
    /// If the stream is longer than the image or there are more corners than ids.
    #[must_use]
    pub fn from_terrain(terrain: &Terrain, mut image: [u8; 0x1000]) -> Level {
        assert!(
            terrain.stream.len() <= image.len(),
            "the stream overruns $7000-$7FFF"
        );
        assert!(
            terrain.corners.len() <= CLASS_COUNT,
            "more corners than metatile ids"
        );
        image[..terrain.stream.len()].copy_from_slice(&terrain.stream);
        let map = transpose(&image);
        let classes = terrain.classes.map(|class| class << 1);
        let corners = std::array::from_fn(|id| {
            terrain.corners.get(id).copied().unwrap_or_else(|| {
                std::array::from_fn(|table| map[streamed_cell(table * CLASS_COUNT + id)])
            })
        });
        let pages = attributes_from_classes(&terrain.palette_classes, &map, &image);
        let mut attributes = [0; 1024];
        for (bytes, page) in attributes.as_chunks_mut::<64>().0.iter_mut().zip(&pages) {
            *bytes = *page;
        }
        Level {
            map,
            corners,
            classes,
            attributes,
            unread: Level::pack_unread(&image),
        }
    }

    /// Gathers the bytes [`UNREAD`] covers from a `$7000-$7FFF` image.
    #[must_use]
    pub fn pack_unread(image: &[u8]) -> [u8; UNREAD_BYTES] {
        let mut unread = [0u8; UNREAD_BYTES];
        for (at, byte) in UNREAD.iter().flat_map(Clone::clone).zip(&mut unread) {
            *byte = image[at];
        }
        unread
    }

    /// The byte at `$7000 + at` if one of the [`UNREAD`] ranges holds it.
    #[must_use]
    pub fn unread_at(&self, at: usize) -> Option<u8> {
        let mut base = 0;
        for range in UNREAD {
            if range.contains(&at) {
                return self.unread.get(base + (at - range.start)).copied();
            }
            base += range.len();
        }
        None
    }

    /// Scatters [`Level::unread`] into a `$7000-$7FFF` image.
    pub fn unpack_unread(&self, image: &mut [u8]) {
        for (at, &byte) in UNREAD.iter().flat_map(Clone::clone).zip(&self.unread) {
            image[at] = byte;
        }
    }

    /// The map stream's byte at `$7000 + at` (`$89FE` writes it there, then transposes
    /// it into the map), for `at` within the stream ([`streamed_map`]).
    #[must_use]
    pub fn streamed(&self, at: usize) -> u8 {
        self.map[streamed_cell(at)]
    }

    /// The metatile id at a 16 px column and row.
    #[must_use]
    pub fn metatile(&self, column: u8, row: u8, page_columns: &[u8; 32], vertical: bool) -> u8 {
        self.map[Cell::at(column, row, page_columns, vertical)]
    }

    /// Metatile `id`'s four tiles as the game reads them: ids from `$D0` on read the
    /// overlapping tables one table later (the last from the class table).
    #[must_use]
    pub fn tiles(&self, id: u8) -> [u8; 4] {
        let id = usize::from(id);
        if let Some(&corners) = self.corners.get(id) {
            return corners;
        }
        let at = id - CLASS_COUNT;
        let [_, top_right, bottom_left, bottom_right] = self.corners[at];
        [top_right, bottom_left, bottom_right, self.classes[at]]
    }

    /// The stored class (`2 * class`) of metatile `id`, or `None` past the table.
    #[must_use]
    pub fn class(&self, id: u8) -> Option<u8> {
        self.classes.get(usize::from(id)).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_covers_the_whole_map() {
        assert_eq!(LEVEL_WIDTH_PIXELS, 4096);
    }

    #[test]
    fn a_cell_in_a_page_carries_past_the_last_column() {
        assert_eq!(
            Cell::in_page(2, 0x70, 5),
            Cell {
                row: 2,
                column: 0x75
            }
        );
        assert_eq!(Cell::in_page(2, 0xF8, 9), Cell { row: 3, column: 1 });
    }

    #[test]
    fn the_map_round_trips_its_bytes() {
        let bytes: [u8; 0x1000] =
            std::array::from_fn(|at| u8::try_from(at * 7 % 251).expect("a byte"));
        let map = Map::from_bytes(&bytes);
        assert_eq!(map.as_bytes(), &bytes[..]);
        assert_eq!(map[Cell { row: 2, column: 3 }], bytes[0x203]);
    }

    #[test]
    fn the_cell_right_of_the_last_column_is_the_next_rows_first() {
        assert_eq!(
            Cell {
                row: 3,
                column: 0xFF
            }
            .next(),
            Cell { row: 4, column: 0 }
        );
        assert_eq!(
            Cell {
                row: 3,
                column: 0x41
            }
            .next(),
            Cell {
                row: 3,
                column: 0x42
            }
        );
    }

    #[test]
    fn map_addresses_are_cells() {
        for (address, row, column) in [
            (0x6000, 0, 0),
            (0x60FF, 0, 0xFF),
            (0x6101, 1, 1),
            (0x6FFF, 15, 0xFF),
            (0x60FF + 1, 1, 0),
        ] {
            let cell = Cell::from_address(address);
            assert_eq!(cell, Cell { row, column }, "${address:04X}");
            assert_eq!(cell.offset(), usize::from(address - 0x6000));
        }
    }

    #[test]
    #[should_panic(expected = "outside the map")]
    fn an_address_past_the_map_is_refused() {
        let _ = Cell::from_address(0x7000);
    }

    #[test]
    fn the_page_index_swaps_its_axes_on_vertical_levels() {
        assert_eq!(page_index(0x35, 0x04, false), 0x03);
        assert_eq!(page_index(0x35, 0x14, false), 0x13);
        assert_eq!(page_index(0x04, 0x35, true), 0x03);
        assert_eq!(page_index(0x14, 0x35, true), 0x13);
        let mut page_columns = [0; 32];
        page_columns[0x13] = 0x70;
        assert_eq!(
            Cell::at(0x35, 0x14, &page_columns, false),
            Cell {
                row: 4,
                column: 0x75
            }
        );
        assert_eq!(
            Cell::at(0x14, 0x35, &page_columns, true),
            Cell {
                row: 5,
                column: 0x74
            }
        );
    }

    /// `Cell::next` is the byte pair it replaced, over every cell but the last.
    #[test]
    fn next_is_word_maths() {
        for word in 0..u16::MAX {
            let [row, column] = word.to_be_bytes();
            let (next_column, carry) = column.overflowing_add(1);
            let bytes = Cell {
                row: row + u8::from(carry),
                column: next_column,
            };
            assert_eq!(Cell { row, column }.next(), bytes, "{word:#06X}");
        }
    }

    #[test]
    fn every_area_imports_within_its_bounds() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let areas: Vec<Terrain> = (0..32)
            .map_while(|area| Terrain::import(&rom.prg, &rom.chr, area))
            .collect();
        assert_eq!(areas.len(), 18);
        for (area, terrain) in areas.iter().enumerate() {
            assert!(terrain.stream.len() <= 0x1000, "area {area}");
            assert_eq!(
                Some(terrain.corners.len()),
                metatile_count(&rom.prg, &rom.chr, area),
                "area {area}"
            );
        }
    }
}
