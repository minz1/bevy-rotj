//! The game's own per-metatile collision classes — its "solid tile" rule.
//!
//! Every solidity test the player's movement makes is a far call into bank 14.
//! `$C99D` (body/wall: bank 4 `$80F8`, `$812B`, `$8184`, `$8198`, `$81B7`,
//! `$8226`, `$823A`) and `$C93B` (head and ground: bank 4 `$847F`, `$8521`,
//! `$8533`, `$8547` and state handlers) both compute the map address `$6000 +
//! row*256 + col` from the probe position, read the metatile id, look it up in a
//! 208-byte class table at WRAM `$7340`, and dispatch through a per-class
//! handler table: `$CADC` for the body probe, `$C97D` for the head/ground probe.
//! `$CAFE` (`$CD2E`) is the third, the step probe.
//!
//! The class table is not in the map: bank 02 `$89C1` fills `$7340-$740F` at
//! sublevel load by run-length decoding a per-area blob. Its source pointer is a
//! 16-bit value at bank 14 `$C000 + area*2`; the loader maps bank 0 to `$A000`
//! and reads `$A0xx` as bank 0 offset `xx`, so the pointers are the 16-bit words
//! at bank 14 `$C000` and the blobs are the bank 0 bytes after `$02A5`:
//!
//! ```text
//! $89D2 LDX #$0A : LDA #$00 : JSR $C229    ; map bank 0 to $A000 ($C229)
//! $89D9 LDX $0400                          ; the area index (raw value)
//! $89DC LDA $C000,x : STA $30              ; pointer low, bank 14 $C000
//!       LDA $C001,x : CLC : ADC #$20 : STA $31
//! $89E2 LDA ($30),y : STA $32 : INY        ; class (stored after ASL as 2*class)
//!       LDA ($30),y : STA $33 : INY        ; run length
//! $89E9 LDA $32 : ASL : STA $7340,x : INX  ; repeat $33 times, stop at $D0
//! ```
//!
//! Is this the rule? `docs/LEVELDATA.md` §11 recorded a derived flood-fill
//! because the map's metatile ids near the actor repeat, but the game's answer
//! is per metatile *id* through this table: area 0 marks the ground body (ids
//! `$92-$95`) as "blocks the floor probe", the sky/backdrop band (ids `$00-$63`,
//! `$C8-$CF`) as clear, and the structural tiles (ids `$64-$91`) as walls.

/// The loader fills `$D0` = 208 entries; ids `>= 208` are outside the level.
pub const CLASS_COUNT: usize = 208;

/// Bank 14 `$C000`: each area's class-table pointer, read as 16-bit little-endian
/// values indexed by the raw `$0400` value / 2.
const CLASS_PTR_TABLE: usize = 0xC000;

/// Bank 0 offset `$02A5`: where the run-length class blobs begin. Area 0's is
/// `0A 64 00 2E 0E 04 06 32 …`: 100 x class 10, 46 x class 0, 4 x class 14,
/// 50 x class 6, then 8 more class 10 (the blob overlaps the next area's, as the
/// loader just stops at 208). Area 17's (the last boss) covers its 102 ids and
/// then runs into the code after it (`A9 02 85 11 A5 F1`), which the loader takes
/// as classes too.
const CLASS_RLE_BASE: usize = 0x02A5;

/// Bytes of the class blobs (`$02A5-$0322`).
const CLASS_RLE_BYTES: usize = 126;

/// Bank 14 `$CADC`: the body-probe handler per class, 16-bit little-endian.
/// The classes whose pointer reaches the position revert (`$C9E2`/`$CA1D`) are
/// the ones that push the actor out of the tile.
const BODY_HANDLER_ADDR: usize = 0xCADC;

/// Bank 14 `$C97D`: the head/ground-probe `(sign, result)` pair per class. A
/// negative sign (`$C974`) answers the probe with `$80` when `$24` bit 2 is set,
/// `$00` otherwise; otherwise the result byte is returned. Class 16's pair runs
/// past the table into `$C99D` code (`$85` = `STA $24`).
const HEAD_TABLE_ADDR: usize = 0xC97D;

/// The highest class the two handler tables define; ids past it are clamped.
const CLASS_MAX: usize = 16;

/// One PRG ROM byte, addressed inside an 8 KiB window mapped from `bank`.
fn byte(prg: &[u8], bank: usize, addr: usize) -> Option<u8> {
    prg.get(bank.checked_mul(0x2000)? + (addr & 0x1FFF))
        .copied()
}

/// A little-endian 16-bit word, addressed as in [`byte`].
fn word(prg: &[u8], bank: usize, addr: usize) -> Option<u16> {
    Some(u16::from_le_bytes([
        byte(prg, bank, addr)?,
        byte(prg, bank, addr + 1)?,
    ]))
}

/// How a metatile answers the two probes the player's movement makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Collision {
    /// The body probe (`$C99D`, table `$CADC`) pushes the actor back out.
    pub wall: bool,
    /// The head/ground probe (`$C93B`, table `$C97D`) answers negative.
    pub block: bool,
    /// The body handler writes the damage flags (`$9F` bit 7, `$99` bit 5,
    /// `$A7`), which the port does not model.
    pub hazard: bool,
}

/// The three kinds the class table expresses, from [`Collision`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Blocks both probes: the structural tiles.
    Solid,
    /// Blocks only one probe: the ground body blocks the floor probe, classes
    /// 11/13 block only the body probe, classes 12/14/15/16 are conditional.
    OneWay,
    /// Blocks neither.
    Empty,
}

impl Collision {
    /// The kind the task's categories name.
    #[must_use]
    pub const fn kind(self) -> Kind {
        match (self.wall, self.block) {
            (true, true) => Kind::Solid,
            (false, false) => Kind::Empty,
            _ => Kind::OneWay,
        }
    }
}

/// A class' answer, from the two handler tables above.
///
/// `wall` is true when the class' `$CADC` handler is one that reverts the
/// position; `block` is true when its `$C97D` pair takes the `$C974` arm (sign
/// bit 7) — the conditional one-way; `hazard` when the `$CADC` handler writes
/// the damage flags. Both tables are read from the ROM at bank 14.
///
/// # Panics
/// Never: `class` is clamped to the table, and a ROM too short reads as zero.
#[must_use]
pub fn collision_of(prg: &[u8], class: u8) -> Collision {
    let class = usize::from(class).min(CLASS_MAX);
    let handler = word(prg, 14, BODY_HANDLER_ADDR + class * 2).unwrap_or(0);
    let wall = matches!(handler, 0xC9DA | 0xCA2C | 0xCA3D | 0xCA66 | 0xCA86 | 0xCAA9);
    let hazard = matches!(handler, 0xCA4E | 0xCA66 | 0xCA97 | 0xCAA9);
    // `$C97D,Y` (sign) selects the `$C974` conditional arm; `$C97E,Y` (result)
    // is returned otherwise. Either is negative for a blocked answer.
    let block = byte(prg, 14, HEAD_TABLE_ADDR + class * 2).is_some_and(|sign| sign & 0x80 != 0)
        || byte(prg, 14, HEAD_TABLE_ADDR + class * 2 + 1).is_some_and(|result| result & 0x80 != 0);
    Collision {
        wall,
        block,
        hazard,
    }
}

/// Decode one area's 208-byte class table (`$7340`) from the ROM bytes.
///
/// `area` is the raw `$0400` value / 2, the index the bank 14 `$C000` pointer
/// table uses. The result is the *class* per metatile id (`$7340` holds `2 *
/// class`).
///
/// Returns `None` for an area outside the 18-entry table or a malformed blob.
#[must_use]
pub fn class_table(prg: &[u8], area: usize) -> Option<[u8; CLASS_COUNT]> {
    if area >= crate::levelmap::AREA_COUNT {
        return None;
    }
    // The stored pointer is $8000 + offset; the loader adds $20 to its high byte
    // to reach the $A000 window bank 0 is mapped into, so the offset is the low 13 bits.
    let ptr = word(prg, 14, CLASS_PTR_TABLE + area * 2)? & 0x1FFF;
    let blobs = prg.get(CLASS_RLE_BASE..CLASS_RLE_BASE + CLASS_RLE_BYTES)?;
    let mut at = usize::from(ptr).checked_sub(CLASS_RLE_BASE)?;
    let mut table = [0u8; CLASS_COUNT];
    let mut id = 0;
    while id < CLASS_COUNT {
        let value = *blobs.get(at)?;
        let run = *blobs.get(at + 1)?;
        at += 2;
        if run == 0 {
            return None;
        }
        for _ in 0..run {
            if id >= CLASS_COUNT {
                break;
            }
            table[id] = value;
            id += 1;
        }
    }
    Some(table)
}

/// The class table for a level index (`$20`), through [`LEVEL_AREA_TABLE`].
///
/// [`LEVEL_AREA_TABLE`]: crate::levelmap::LEVEL_AREA_TABLE
#[must_use]
pub fn class_table_for_level(prg: &[u8], level: usize) -> Option<[u8; CLASS_COUNT]> {
    let raw = *crate::levelmap::LEVEL_AREA_TABLE.get(level)?;
    class_table(prg, usize::from(raw) / 2)
}

/// The level's terrain as WRAM holds it: the map (`$6000-$6FFF`), each metatile's
/// tiles (`$7000`), the class table (`$7340`, stored as `2 * class`) and the attribute
/// pages (`$7600`). The one owner: the probes read it and the actors that rebuild the
/// level write it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Level {
    /// Metatile ids, 16 rows of 256 (a row holds every page's 16 columns).
    pub map: [u8; 4096],
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

/// The `$7000`-relative ranges of [`Level::unread`].
pub const UNREAD: [std::ops::Range<usize>; 3] = [0x510..0x530, 0x5B0..0x600, 0xA00..0xB00];
const UNREAD_BYTES: usize = 0x20 + 0x50 + 0x100;

/// The map cell the loader's transpose moves `$7000 + at` to.
fn streamed_cell(at: usize) -> usize {
    ((at >> 4) & 0x0F) * 256 + (at >> 8) * 16 + (at & 0x0F)
}

/// The page -> map column table (`$0440`) when pages are laid out in order.
pub const IN_ORDER: [u8; 32] = [
    0x00, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 0x90, 0xA0, 0xB0, 0xC0, 0xD0, 0xE0, 0xF0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// An area's room's page -> map column table (`$0440-$045F`, bank 13 `$B4CB`): bank 14
/// `$C024[area]` points at per-room pointers in bank 0, each to 32 page numbers.
///
/// `area` and `room` are the raw `$0400`/`$0402` values.
#[must_use]
pub fn rows_from_rom(prg: &[u8], area: u8, room: u8) -> Option<[u8; 32]> {
    let word = |bank: usize, addr: u16| -> Option<u16> {
        let at = bank * 0x2000 + usize::from(addr & 0x1FFF);
        Some(u16::from_le_bytes([*prg.get(at)?, *prg.get(at + 1)?]))
    };
    let rooms = word(14, 0xC024 + u16::from(area))?;
    let pages = word(0, rooms.checked_add(u16::from(room))?)?;
    let mut rows = [0u8; 32];
    for (i, row) in rows.iter_mut().enumerate() {
        *row = prg.get(usize::from(pages & 0x1FFF) + i)? << 4;
    }
    Some(rows)
}

/// The map offset (`$6000 +`) the game's probes build from a point's 16 px column
/// and row (`$C99D`'s `$2A/$2B`): page `(row & $10) | (column >> 4)` (swapped on
/// vertical levels), `rows[page]` its base column.
#[must_use]
pub fn map_offset(column: u8, row: u8, rows: &[u8; 32], vertical: bool) -> usize {
    let (outer, inner) = if vertical {
        (row, column)
    } else {
        (column, row)
    };
    let page = usize::from((inner & 0x10) | (outer >> 4));
    usize::from(row & 0x0F) << 8 | usize::from((column & 0x0F) | rows[page])
}

impl Default for Level {
    fn default() -> Self {
        Level {
            map: [0; 4096],
            corners: [[0; 4]; CLASS_COUNT],
            classes: [0; CLASS_COUNT],
            attributes: [0; 1024],
            unread: [0; UNREAD_BYTES],
        }
    }
}

impl Level {
    /// Area `area`'s terrain as the loader leaves it over the payload's bytes (the
    /// first load after boot), from the ROM's PRG tables and CHR ([`crate::levelmap`]).
    #[must_use]
    pub fn from_rom(prg: &[u8], chr: &[u8], area: usize) -> Option<Level> {
        Level::loaded(prg, chr, area, crate::levelmap::payload_image(chr)?)
    }

    /// Area `area`'s terrain as the loader `$89FE` leaves it over `image`, the
    /// `$7000-$7FFF` it finds: imported from the ROM, then built over the image.
    #[must_use]
    pub fn loaded(prg: &[u8], chr: &[u8], area: usize, image: [u8; 0x1000]) -> Option<Level> {
        Some(Level::from_terrain(
            &crate::terrain::Terrain::import(prg, chr, area)?,
            image,
        ))
    }

    /// A terrain built over `image`, the `$7000-$7FFF` the loader finds: the stream written
    /// over it, the whole of it transposed into the map, the tile tables past the area's
    /// metatiles, the attributes of ids past the known palette classes and
    /// [`Level::unread`] from what the stream does not reach.
    #[must_use]
    pub fn from_terrain(terrain: &crate::terrain::Terrain, mut image: [u8; 0x1000]) -> Level {
        image[..terrain.stream.len()].copy_from_slice(&terrain.stream);
        let rows = crate::levelmap::transpose(&image);
        let mut map = [0u8; 4096];
        for (row, bytes) in rows.iter().enumerate() {
            map[row * 256..(row + 1) * 256].copy_from_slice(bytes);
        }
        let classes = terrain.classes.map(|class| class << 1);
        let corners = std::array::from_fn(|id| {
            terrain.corners.get(id).copied().unwrap_or_else(|| {
                std::array::from_fn(|table| map[streamed_cell(table * CLASS_COUNT + id)])
            })
        });
        let pages =
            crate::levelmap::attributes_from_classes(&terrain.palette_classes, &rows, &image);
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
    /// it into the map), for `at` within [`crate::levelmap::map_reach`].
    #[must_use]
    pub fn streamed(&self, at: usize) -> u8 {
        self.map[streamed_cell(at)]
    }

    /// The metatile id at a 16 px column and row.
    #[must_use]
    pub fn metatile(&self, column: u8, row: u8, rows: &[u8; 32], vertical: bool) -> u8 {
        self.map[map_offset(column, row, rows, vertical)]
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

    /// How the game's probes answer at level pixel (`x`, `y`), pages in order.
    ///
    /// Outside the level the sides and the bottom are walls and above it is clear;
    /// ids past the class table answer clear (the game reads unrelated data there).
    #[must_use]
    pub fn collision_at(&self, prg: &[u8], x: i32, y: i32) -> Collision {
        use crate::levelmap::{LEVEL_WIDTH_PIXELS, MAP_ROWS, METATILE_PIXELS};

        const WALL: Collision = Collision {
            wall: true,
            block: true,
            hazard: false,
        };
        const CLEAR: Collision = Collision {
            wall: false,
            block: false,
            hazard: false,
        };
        if x < 0 || x >= i32::try_from(LEVEL_WIDTH_PIXELS).expect("fits") {
            return WALL;
        }
        let tile = i32::try_from(METATILE_PIXELS).expect("16");
        let row = y.div_euclid(tile);
        if row >= i32::try_from(MAP_ROWS).expect("small") {
            return WALL;
        }
        if row < 0 {
            return CLEAR;
        }
        let column = u8::try_from(x.div_euclid(tile)).expect("a column");
        let row = u8::try_from(row).expect("a row");
        match self.class(self.metatile(column, row, &IN_ORDER, false)) {
            Some(stored) => collision_of(prg, stored / 2),
            None => CLEAR,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::{fnv1a, table};

    /// The decoder `Level::loaded` had before the terrain import: the reference.
    fn loaded_from_rom(
        prg: &[u8],
        chr: &[u8],
        area: usize,
        mut image: [u8; 0x1000],
    ) -> Option<Level> {
        let rows = crate::levelmap::map_over(prg, chr, area, &mut image)?;
        let mut map = [0u8; 4096];
        for (row, bytes) in rows.iter().enumerate() {
            map[row * 256..(row + 1) * 256].copy_from_slice(bytes);
        }
        let mut classes = class_table(prg, area)?;
        for class in &mut classes {
            *class <<= 1;
        }
        let tables = crate::levelmap::auto_tile_tables(prg, chr, area)?;
        let ids = crate::levelmap::metatile_count(prg, chr, area)?;
        let corners = std::array::from_fn(|id| {
            std::array::from_fn(|table| {
                if id < ids {
                    tables[table][id]
                } else {
                    map[streamed_cell(table * CLASS_COUNT + id)]
                }
            })
        });
        let pages = crate::levelmap::attributes_of(prg, chr, area, &rows, &image)?;
        let mut attributes = [0; 1024];
        for (bytes, page) in attributes.as_chunks_mut::<64>().0.iter_mut().zip(&pages) {
            *bytes = *page;
        }
        let unread = Level::pack_unread(&image);
        Some(Level {
            map,
            corners,
            classes,
            attributes,
            unread,
        })
    }

    /// Each area's terrain built over three images is what the old decoder left: the boot
    /// payload, the cutscene payload and a synthetic image that differs at every byte.
    #[test]
    fn terrain_builds_what_the_rom_decoder_left() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let synthetic: [u8; 0x1000] =
            std::array::from_fn(|i| u8::try_from((i * 37 + i / 256) % 251).expect("byte"));
        let images = [
            crate::levelmap::boot_image(&rom.chr).expect("the boot payload"),
            crate::levelmap::payload_image(&rom.chr).expect("the cutscene payload"),
            synthetic,
        ];
        let mut areas = 0;
        for area in 0..32 {
            let Some(terrain) = crate::terrain::Terrain::import(&rom.prg, &rom.chr, area) else {
                continue;
            };
            areas += 1;
            for (which, &image) in images.iter().enumerate() {
                assert_eq!(
                    Some(Level::from_terrain(&terrain, image)),
                    loaded_from_rom(&rom.prg, &rom.chr, area, image),
                    "area {area}, image {which}"
                );
            }
        }
        assert_eq!(areas, 18, "every area imports");
    }

    /// Area 17 (the last boss) defines only 102 ids; its blob then runs into code.
    const AREA_17_IDS: usize = 102;

    /// The emulator's `$6000-$7FFF` WRAM at frame 700 holds area 0's `$7340`
    /// table (the loader's `2 * class` halves); the oracle reproduces fceux's bytes
    /// (pinned by digest).
    #[test]
    fn area_zero_table_matches_the_capture() {
        let Some(mut scene) = crate::actor::oracle::Scene::power_on(
            (0..=699).map(rotj_oracle::physics_input).collect(),
        ) else {
            return;
        };
        scene.run(699, &[]);
        let captured = &scene.machine.bus.wram[0x1340..0x1340 + CLASS_COUNT];
        assert_eq!(
            fnv1a(captured),
            table(include_str!("../tests/data/digests.txt"), "stage-1-1-class")["-"],
            "the oracle's $7340 differs from fceux's"
        );
        let ours = class_table(&scene.machine.bus.cart.prg, 0).expect("area 0 has a table");
        for (id, (&stored, &class)) in captured.iter().zip(&ours).enumerate() {
            assert_eq!(stored, class * 2, "metatile {id:#04X} class");
        }
    }

    /// Every area the bank 14 `$C000` table lists decodes to a full table, classes 0-16
    /// for the ids it uses (area 17's past its 102 are the code after its blob).
    #[test]
    fn every_area_decodes() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        for area in 0..crate::levelmap::AREA_COUNT {
            let table = class_table(&rom.prg, area).unwrap_or_else(|| panic!("area {area}"));
            let used = if area == 17 { AREA_17_IDS } else { CLASS_COUNT };
            assert!(
                table[..used].iter().all(|&c| c <= 16),
                "area {area} has a class outside 0..=16"
            );
        }
    }

    /// The split the game's two probes express for stage 1-1's own classes.
    #[test]
    fn the_responses_split_solid_empty_and_one_way() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let prg = &rom.prg;
        assert_eq!(
            collision_of(prg, 0),
            Collision {
                wall: true,
                block: true,
                hazard: false
            }
        );
        assert_eq!(
            collision_of(prg, 14),
            Collision {
                wall: false,
                block: true,
                hazard: false
            }
        );
        assert_eq!(
            collision_of(prg, 10),
            Collision {
                wall: false,
                block: false,
                hazard: false
            }
        );
        assert_eq!(collision_of(prg, 0).kind(), Kind::Solid);
        assert_eq!(collision_of(prg, 14).kind(), Kind::OneWay);
        assert_eq!(collision_of(prg, 10).kind(), Kind::Empty);
    }
}
