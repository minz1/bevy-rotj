//! An area's terrain as data: what the loader `$89FE` would decode from the ROM, imported
//! once, so the game builds its [`Level`](crate::collision::Level) from data rather than
//! from the ROM (`docs/specs/2026-10-17-levels-as-assets-design.md`). It holds only what the
//! ROM gives; what the stream does not reach comes from WRAM at load time.

use crate::collision::{CLASS_COUNT, class_table};
use crate::levelmap::{auto_tile_tables, classes_from_chr, metatile_count, streamed_map};

/// One area's terrain.
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
