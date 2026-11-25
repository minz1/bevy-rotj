//! Level packs: the game's levels as Tiled maps and a `pack.json`, and the typed assets they
//! import to (`docs/specs/2026-10-18-level-packs-design.md`, release 1 in
//! `docs/specs/2026-10-18-level-packs-release-1-design.md`). The game reads its levels through
//! [`Levels`] (`Prg::levels`), by default [`Levels::original`]; release 6 lets a pack replace it.

use std::collections::{BTreeMap, BTreeSet};

use crate::map::Terrain;
use crate::map::{ChrBanks, Raster, Room};
use crate::render::Mirroring;
use crate::sim::families::next_area_gate::PASSES_STAGE_ARGS;
use crate::sim::families::{MAIN, SECOND};
use crate::sim::spawn::SpawnRecord;
use crate::sim::spawn::SpawnTable;
use crate::sim::stage::{Music, StageDescriptor};

pub(crate) mod custom;
pub(crate) mod export;
pub(crate) mod folder;
pub(crate) mod import;
pub mod model;
pub(crate) mod tilesets;

pub use custom::custom_picture;
pub use export::{export, write};
pub use folder::{FolderError, read, save};
pub use import::{ImportError, import};
pub use tilesets::{TILESET_PX, Tileset, tilesets};

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
/// The Tiled version a pack's files say saved them.
const TILED_VERSION: &str = "1.12.0";
/// Tiled 1.12.0's `FileFormat::versionString` still writes JSON format "1.11".
const JSON_VERSION: &str = "1.11";

/// Every stage's assets, in play order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Levels {
    /// The stages.
    pub(crate) stages: Vec<StageAssets>,
    /// The password screen's passwords the pause never shows.
    pub(crate) extra_passwords: Vec<ExtraPassword>,
}

/// A password the pause never shows, which starts a checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExtraPassword {
    /// Its letters, `A`-`Z` in ASCII, as [`StageDescriptor::password`].
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
pub(crate) struct StageAssets {
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
pub(crate) struct RoomAssets {
    /// Its layout.
    pub room: Room,
    /// Its spawn records.
    pub spawns: SpawnTable,
}

/// A stage's identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stage {
    /// Its name; the original game's are the banner's (`3-2`).
    pub name: String,
    /// The area (0-17) its metatiles, CHR and screen record come from.
    pub area: u8,
    /// What the engine reads by stage; its `area` is `area * 2`.
    pub descriptor: StageDescriptor,
}

/// What a pack names and the player's ROM resolves: each area's tilesets (one per CHR
/// variant), metatiles and screen record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    areas: Vec<CatalogArea>,
    /// The background and sprite presets the rooms load.
    background_palettes: BTreeSet<u8>,
    sprite_palettes: BTreeSet<u8>,
    /// The pictures the stages' cutscenes show.
    pictures: BTreeSet<u8>,
    /// The songs a stage may start: the sound test's, from the ROM.
    songs: BTreeSet<u8>,
    /// The bosses the `next_area_gate`s introduce.
    bosses: BTreeSet<u8>,
    /// The boss-status (`$34`) portrait indices (`behaviour`) the ROM's table holds.
    boss_status_portraits: BTreeSet<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CatalogArea {
    area: u8,
    /// Each variant's `Room::chr`, in order of first appearance across the area's rooms.
    variants: Vec<ChrBanks>,
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
        Some(Catalog {
            songs: crate::sim::sound_test::songs(prg).into(),
            ..Catalog::from_levels(&Levels::from_rom(prg, chr)?)
        })
    }

    /// The catalog of `levels`' areas (their CHR variants, metatiles and first screen record)
    /// and of the presets, pictures and songs they use.
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
        let rooms = || levels.stages.iter().flat_map(|stage| &stage.rooms);
        let descriptors = || levels.stages.iter().map(|stage| &stage.stage.descriptor);
        Catalog {
            areas,
            background_palettes: rooms()
                .map(|assets| assets.room.background_palette)
                .collect(),
            sprite_palettes: rooms().map(|assets| assets.room.sprite_palette).collect(),
            pictures: descriptors()
                .filter_map(|descriptor| descriptor.picture)
                .collect(),
            songs: descriptors()
                .filter_map(|descriptor| match descriptor.music {
                    Music::Song(song) => Some(song),
                    Music::Keep => None,
                })
                .collect(),
            bosses: rooms()
                .flat_map(|assets| assets.spawns.records.iter().flatten())
                .filter_map(boss)
                .collect(),
            boss_status_portraits: rooms()
                .flat_map(|assets| assets.spawns.records.iter().flatten())
                .filter(|record| !record.second_class && record.kind == 0x34)
                .map(|record| record.behaviour)
                .collect(),
        }
    }

    fn area(&self, area: u8) -> Option<&CatalogArea> {
        self.areas.iter().find(|known| known.area == area)
    }

    /// The animation handles (bit 7) the rooms load as a bank.
    fn handles(&self) -> BTreeSet<u8> {
        let banks = self
            .areas
            .iter()
            .flat_map(|area| area.variants.iter().flat_map(|chr| chr.0));
        banks.filter(|&bank| ChrBanks::is_animated(bank)).collect()
    }

    /// The tileset name of `area`'s variant `chr`: `area-06`, then `area-06-b` and on.
    fn tileset_name(&self, area: u8, chr: ChrBanks) -> Option<String> {
        let variant = self.area(area)?.variants.iter().position(|&v| v == chr)?;
        Some(tileset_name(area, variant))
    }

    /// The area and `Room::chr` a tileset name resolves to.
    fn tileset(&self, name: &str) -> Option<(u8, ChrBanks)> {
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
    Some((class, (kinds.len() > 1).then(|| variant_name(kind))))
}

/// The `variant` a kind of a class with several kinds is written as.
fn variant_name(kind: u8) -> String {
    format!("type-{kind:02x}")
}

/// The slot class and kind a class and variant name; a class with several kinds and no
/// variant is its first kind.
fn kind_of(class: &str, variant: Option<&str>) -> Result<(bool, u8), String> {
    let (second_class, kinds) = [false, true]
        .into_iter()
        .find_map(|second_class| {
            classes(second_class)
                .find(|&(name, _)| name == class)
                .map(|(_, kinds)| (second_class, kinds))
        })
        .ok_or_else(|| format!("unknown class `{class}`"))?;
    let Some(variant) = variant.filter(|_| kinds.len() > 1) else {
        return Ok((second_class, kinds[0]));
    };
    kinds
        .iter()
        .find(|&&kind| variant_name(kind) == variant)
        .map(|&kind| (second_class, kind))
        .ok_or_else(|| format!("unknown `{class}` variant `{variant}`"))
}

/// The boss a `next_area_gate` record introduces: `health`'s low byte, which indexes the
/// ROM's boss tables (`$BCA6`, `$BE4E`) when the gate passes its stage arguments.
fn boss(record: &SpawnRecord) -> Option<u8> {
    let (class, _) = class_of(record.second_class, record.kind)?;
    (class == "next_area_gate" && record.state & PASSES_STAGE_ARGS != 0)
        .then(|| record.health.to_be_bytes()[1])
}

/// A page-table / schedule position split into its page (or band) and column within it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Place {
    page: usize,
    column: usize,
}

impl Place {
    /// Splits a 0..512 schedule column into its page-table entry (`0..32`) and column (`0..16`).
    const fn from_schedule_column(column: usize) -> Self {
        Self {
            page: column >> 4,
            column: column & 15,
        }
    }

    /// Splits a page-table entry (`0..32`) into `(band, along)`; extra pages (`32+`) sit past
    /// page 15 in band 0.
    const fn from_region_entry(entry: usize) -> Self {
        if entry < TABLE_ENTRIES {
            Self {
                page: entry >> 4,
                column: entry & 15,
            }
        } else {
            Self {
                page: 0,
                column: entry - PAGES,
            }
        }
    }

    /// Joins `page` and `column` into a schedule cell index (`pages[page] * 16 + column`).
    const fn schedule_cell(self, pages: &[u8; TABLE_ENTRIES]) -> usize {
        (pages[self.page] as usize) * 16 + self.column
    }
}

/// One corner tile of a metatile: its 1 KiB CHR bank slot (`0..4`) and tile (`0..64`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Corner {
    slot: u8,
    tile: u8,
}

impl Corner {
    /// Splits an auto-tiler corner byte into its CHR slot (bits 7-6) and tile (bits 5-0).
    const fn decode(byte: u8) -> Self {
        Self {
            slot: byte >> 6,
            tile: byte & 63,
        }
    }

    /// Packs a CHR slot (`0..4`) and tile (`0..64`) into an auto-tiler corner byte.
    const fn encode(self) -> u8 {
        (self.slot << 6) | self.tile
    }
}

/// Where page-table entry `entry` sits in a room's map, as (page along, band across); an
/// extra page (32 and up) sits after page 15 in the first band.
fn region_place(entry: usize) -> (usize, usize) {
    let place = Place::from_region_entry(entry);
    (place.column, place.page)
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
    Place::from_schedule_column(column).schedule_cell(pages)
}

/// The first spawn column the camera start implies: the first cell of the page under it.
fn derived_first_spawn_column(
    vertical: bool,
    camera_x: u16,
    camera_y: u16,
    pages: &[u8; TABLE_ENTRIES],
) -> u8 {
    let (along, across) = along_across(vertical, camera_x, camera_y);
    let place = Place {
        page: usize::from(across >= 0x1000) * PAGES + usize::from(along >> 12),
        column: 0,
    };
    u8::try_from(place.schedule_cell(pages)).expect("a page's first cell fits")
}

/// The `mirroring` property's values.
const MIRRORINGS: [(&str, Mirroring); 4] = [
    ("side-by-side", Mirroring::SideBySide),
    ("stacked", Mirroring::Stacked),
    ("single-low", Mirroring::SingleLow),
    ("single-high", Mirroring::SingleHigh),
];

/// The start mirroring a room without a `mirroring` property gets.
fn derived_mirroring(vertical: bool, camera_y: u16) -> Mirroring {
    match (vertical, camera_y >= 0x1000) {
        (true, _) => Mirroring::SideBySide,
        (false, true) => Mirroring::Stacked,
        (false, false) => Mirroring::SingleLow,
    }
}

/// A level position's coordinates (along the scroll, across it).
fn along_across(vertical: bool, x: u16, y: u16) -> (u16, u16) {
    if vertical { (y, x) } else { (x, y) }
}

#[cfg(test)]
mod tests;
