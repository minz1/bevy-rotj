//! Level packs: the game's levels as Tiled maps and a `pack.json`, and the typed assets they
//! import to (`docs/specs/2026-10-18-level-packs-design.md`, release 1 in
//! `docs/specs/2026-10-18-level-packs-release-1-design.md`). The game reads its levels through
//! [`Levels`] (`Prg::levels`), by default [`Levels::original`]; release 6 lets a pack replace it.

use std::collections::BTreeMap;

use crate::actor::families::{MAIN, SECOND};
use crate::actor::spawn::SpawnTable;
use crate::actor::stage::Stage as Descriptor;
use crate::rooms::{Raster, Room};
use crate::terrain::Terrain;

pub mod export;
pub mod import;
pub mod model;

pub use export::{export, write};
pub use import::{ImportError, import};

/// A pack's files: each path, relative to the pack's folder, to its text.
pub type PackFiles = BTreeMap<String, String>;

/// The cells of a page, and of a schedule.
const PAGE_CELLS: usize = 256;
/// The entries of a room's page table.
const TABLE_ENTRIES: usize = 32;
/// An area's physical pages.
const PAGES: usize = 16;
/// The tiles of every tileset a pack names.
const TILESET_TILES: u32 = 256;

/// Every stage's assets, in play order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Levels {
    /// The stages.
    pub stages: Vec<StageAssets>,
    /// The password screen's passwords the pause never shows.
    pub extra_passwords: Vec<ExtraPassword>,
}

/// A password the pause never shows, which starts a checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtraPassword {
    /// Its letters, `A`-`Z` in ASCII, as [`Descriptor::password`].
    pub letters: [u8; 4],
    /// The index in [`Levels::stages`] of the checkpoint it starts.
    pub stage: usize,
}

/// Whose pack is imported: only the original game's export keeps `no_handler` objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The original game's export.
    Original,
    /// Anyone else's pack.
    User,
}

/// One stage's assets: an area's map, schedule and rooms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageAssets {
    /// What the stage is.
    pub stage: Stage,
    /// The area's terrain.
    pub terrain: Terrain,
    /// The spawn schedule (`$7410`), reloaded on every room load.
    pub schedule: [u8; PAGE_CELLS],
    /// The rooms in play order.
    pub rooms: Vec<RoomAssets>,
}

/// One room's assets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomAssets {
    /// Its layout.
    pub room: Room,
    /// Its spawn records.
    pub spawns: SpawnTable,
}

/// A stage's identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stage {
    /// Its name; the original game's are the banner's (`3-2`).
    pub name: String,
    /// The area (0-17) its metatiles, CHR and screen record come from.
    pub area: u8,
    /// What the engine reads by stage; its `area` is `area * 2`.
    pub descriptor: Descriptor,
}

/// What a pack names and the player's ROM resolves: each area's tilesets (one per CHR
/// variant), metatiles and screen record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    areas: Vec<CatalogArea>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CatalogArea {
    area: u8,
    /// Each variant's `Room::chr`, in order of first appearance across the area's rooms.
    variants: Vec<[u8; 4]>,
    /// The area's metatiles, with no stream.
    terrain: Terrain,
    /// The area's non-empty screen record.
    split: Option<Split>,
}

/// A non-empty screen record's fields beside its arrival.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Split {
    raster: Raster,
    fixed_rows: [bool; 16],
    fixed_quads: [bool; 8],
}

impl Catalog {
    /// The original game's catalog, from the ROM; `None` where its levels do not import.
    #[must_use]
    pub fn from_rom(prg: &[u8], chr: &[u8]) -> Option<Catalog> {
        Some(Catalog::from_levels(&Levels::from_rom(prg, chr)?))
    }

    /// The catalog of `levels`' areas: their CHR variants, metatiles and first screen record.
    pub(crate) fn from_levels(levels: &Levels) -> Catalog {
        let areas = levels
            .stages
            .iter()
            .map(|stage| {
                let mut variants = Vec::new();
                for assets in &stage.rooms {
                    if !variants.contains(&assets.room.chr) {
                        variants.push(assets.room.chr);
                    }
                }
                let split = stage.rooms.iter().find_map(|assets| {
                    Some(Split {
                        raster: assets.room.raster?,
                        fixed_rows: assets.room.fixed_rows,
                        fixed_quads: assets.room.fixed_quads,
                    })
                });
                CatalogArea {
                    area: stage.stage.area,
                    variants,
                    terrain: Terrain {
                        stream: Vec::new(),
                        ..stage.terrain.clone()
                    },
                    split,
                }
            })
            .collect();
        Catalog { areas }
    }

    fn area(&self, area: u8) -> Option<&CatalogArea> {
        self.areas.iter().find(|known| known.area == area)
    }

    /// The tileset name of `area`'s variant `chr`: `area-06`, then `area-06-b` and on.
    fn tileset_name(&self, area: u8, chr: [u8; 4]) -> Option<String> {
        let variant = self.area(area)?.variants.iter().position(|&v| v == chr)?;
        Some(tileset_name(area, variant))
    }

    /// The area and `Room::chr` a tileset name resolves to.
    fn tileset(&self, name: &str) -> Option<(u8, [u8; 4])> {
        self.areas.iter().find_map(|known| {
            let variant = (0..known.variants.len())
                .find(|&variant| tileset_name(known.area, variant) == name)?;
            Some((known.area, known.variants[variant]))
        })
    }
}

fn tileset_name(area: u8, variant: usize) -> String {
    let number = u16::from(area) + 1;
    match u8::try_from(variant).ok().filter(|&v| v > 0) {
        Some(letter) => format!("area-{number:02}-{}", char::from(b'a' + letter)),
        None => format!("area-{number:02}"),
    }
}

/// Main `$00`: the cell clears and nothing spawns.
const NO_ACTOR: (&str, &[u8]) = ("no_actor", &[0x00]);
/// Second-class kinds with no `$CEC3` entry: record ids past area 17 room 0's table, never reached.
const NO_HANDLER: (&str, &[u8]) = ("no_handler", &[0x4A, 0x88, 0xE0]);

/// Each class of a slot class, with its kinds.
fn classes(second_class: bool) -> impl Iterator<Item = (&'static str, &'static [u8])> {
    let (handled, unhandled) = if second_class {
        (SECOND, NO_HANDLER)
    } else {
        (MAIN, NO_ACTOR)
    };
    handled.iter().copied().chain([unhandled])
}

/// The class a record of `kind` is written as, and its `variant` when the class has
/// several kinds.
fn class_of(second_class: bool, kind: u8) -> Option<(&'static str, Option<String>)> {
    let (class, kinds) = classes(second_class).find(|(_, kinds)| kinds.contains(&kind))?;
    Some((class, (kinds.len() > 1).then(|| format!("type-{kind:02x}"))))
}

/// The slot class and kind a class and variant name.
fn kind_of(class: &str, variant: Option<&str>) -> Result<(bool, u8), String> {
    let (second_class, kinds) = [false, true]
        .into_iter()
        .find_map(|second_class| {
            classes(second_class)
                .find(|&(name, _)| name == class)
                .map(|(_, kinds)| (second_class, kinds))
        })
        .ok_or_else(|| format!("unknown class `{class}`"))?;
    if let [kind] = kinds {
        return Ok((second_class, *kind));
    }
    let variant = variant.ok_or_else(|| format!("class `{class}` needs a `variant`"))?;
    kinds
        .iter()
        .find(|&&kind| format!("type-{kind:02x}") == variant)
        .map(|&kind| (second_class, kind))
        .ok_or_else(|| format!("unknown `{class}` variant `{variant}`"))
}

/// Where page-table entry `entry` sits in a room's map, as (page along, band across); an
/// extra page (32 and up) sits after page 15 in the first band.
fn region_place(entry: usize) -> (usize, usize) {
    if entry < TABLE_ENTRIES {
        (entry & 15, entry >> 4)
    } else {
        (entry - PAGES, 0)
    }
}

/// The index in a map's tile data of cell `cell` of the page at `along`, `band`.
fn tile_index(vertical: bool, width: usize, (along, band): (usize, usize), cell: usize) -> usize {
    let (page_x, page_y) = if vertical {
        (band, along)
    } else {
        (along, band)
    };
    (page_y * 16 + cell / 16) * width + page_x * 16 + cell % 16
}

/// The schedule cell a room's schedule column (entry × 16 + column, 0-511) names.
fn schedule_cell(pages: &[u8; TABLE_ENTRIES], column: usize) -> usize {
    usize::from(pages[column >> 4]) * 16 + (column & 15)
}

/// The first spawn column the camera start implies: the first cell of the page under it.
fn derived_first_spawn_column(
    vertical: bool,
    camera_x: u16,
    camera_y: u16,
    pages: &[u8; TABLE_ENTRIES],
) -> u8 {
    let (along, across) = along_across(vertical, camera_x, camera_y);
    let entry_under_camera = usize::from(across >= 0x1000) * PAGES + usize::from(along >> 12);
    pages[entry_under_camera] << 4
}

/// A level position's coordinates (along the scroll, across it).
fn along_across(vertical: bool, x: u16, y: u16) -> (u16, u16) {
    if vertical { (y, x) } else { (x, y) }
}

#[cfg(test)]
mod tests;
