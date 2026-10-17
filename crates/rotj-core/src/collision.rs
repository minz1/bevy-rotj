//! The game's own per-metatile collision classes — its "solid tile" rule.
//!
//! Every probe the game makes reads the metatile id under a point, looks it up in a
//! 208-byte class table at WRAM `$7340` and dispatches on the class through one of three
//! bank-14 handler tables; [`Surface`] names what each class does to them
//! (`actor::collision` runs the probes).
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
    ///
    /// # Panics
    /// If the stream is longer than the image or there are more corners than ids.
    pub fn from_terrain(terrain: &crate::terrain::Terrain, mut image: [u8; 0x1000]) -> Level {
        assert!(
            terrain.stream.len() <= image.len(),
            "the stream overruns $7000-$7FFF"
        );
        assert!(
            terrain.corners.len() <= CLASS_COUNT,
            "more corners than metatile ids"
        );
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
    /// it into the map), for `at` within the stream ([`crate::levelmap::streamed_map`]).
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

    /// Each class's surface stands for the handlers the ROM's three tables give it: the
    /// wall handler (`$CADC`), the floor handler (`$CD2E`) and the point test's pair
    /// (`$C97D`: `None` for the conditional sign, else its fixed answer).
    #[test]
    fn surfaces_match_the_handler_tables() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let prg = &rom.prg;
        let handlers = |surface| match surface {
            Surface::Solid => (0xC9DA, 0xCB39, Some(0x80)),
            Surface::BeltRight => (0xCA2C, 0xCB80, Some(0x80)),
            Surface::BeltLeft => (0xCA3D, 0xCBB8, Some(0x80)),
            Surface::HazardBehind => (0xCA4E, 0xCBF0, Some(0)),
            Surface::SolidHazard => (0xCA66, 0xCC08, Some(0x80)),
            Surface::Slippery => (0xCA86, 0xCC1A, Some(0x80)),
            Surface::Behind => (0xCA97, 0xCC61, Some(0)),
            Surface::Open => (0xCAA0, 0xCC6A, Some(0)),
            Surface::SolidToBodies => (0xC9DA, 0xCB39, Some(0)),
            Surface::Crumbling => (0xCAA3, 0xCC6D, None),
            Surface::OneWay => (0xCAA6, 0xCCFC, None),
            Surface::OneWayBehind => (0xCA97, 0xCCFC, None),
            Surface::Spikes => (0xCAA9, 0xCD09, None),
        };
        for class in 0..=16u8 {
            let at = usize::from(class * 2);
            let surface = Surface::from_stored(class * 2).expect("a class");
            let point = if byte(prg, 14, 0xC97D + at).expect("the sign") & 0x80 == 0 {
                byte(prg, 14, 0xC97E + at)
            } else {
                None
            };
            let rom = (
                word(prg, 14, 0xCADC + at).expect("the wall handler"),
                word(prg, 14, 0xCD2E + at).expect("the floor handler"),
                point,
            );
            assert_eq!(rom, handlers(surface), "class {class}");
        }
        assert!(
            (0..=u8::MAX)
                .filter(|&stored| stored & 1 != 0 || stored > 32)
                .all(|stored| Surface::from_stored(stored).is_none())
        );
    }
}
