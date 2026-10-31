//! The importer: a pack's files into [`Levels`], the game's typed assets.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::de::DeserializeOwned;
use thiserror::Error;

use super::custom::{CUSTOM, CUSTOM_FILE};
use super::model::{Layer, Map, Object, PackFile, PackStage, Property};
use super::{
    Catalog, CatalogArea, ExtraPassword, Levels, NO_HANDLER, PAGE_CELLS, PAGES, PackFiles,
    RoomAssets, Source, Split, Stage, StageAssets, TABLE_ENTRIES, TILESET_TILES, along_across,
    derived_first_spawn_column, kind_of, region_place, schedule_cell, tile_index,
};
use crate::actor::spawn::SpawnTable;
use crate::actor::stage::{Music, OnClear, Stage as Descriptor};
use crate::actor::world::orientation;
use crate::actors::{CELL_LOCK, CELL_PENDING, CELL_RECORD, SpawnRecord};
use crate::collision::CLASS_COUNT;
use crate::render::Mirroring;
use crate::rooms::{Arrival, Room, Start};
use crate::terrain::Terrain;

/// A pack that does not import.
#[derive(Debug, Error)]
pub enum ImportError {
    /// A file the pack names is not in it.
    #[error("{file}: missing")]
    Missing {
        /// The file.
        file: String,
    },
    /// A file is not the JSON its kind needs.
    #[error("{file}: {source}")]
    Json {
        /// The file.
        file: String,
        /// What serde found.
        source: serde_json::Error,
    },
    /// A file's content breaks the format.
    #[error("{file}: {reason}")]
    Invalid {
        /// The file.
        file: String,
        /// What is wrong, naming the layer and object.
        reason: String,
    },
}

/// The gid bits Tiled flips and rotates a tile with.
const FLIP_BITS: u32 = 0xF000_0000;
/// The spawn table's record ids.
const RECORD_IDS: usize = CELL_RECORD as usize + 1;

/// The levels `pack` holds, its tilesets resolved against `catalog`; a `source` of
/// [`Source::User`] refuses `no_handler` objects.
pub fn import(pack: &PackFiles, catalog: &Catalog, source: Source) -> Result<Levels, ImportError> {
    let file: PackFile = parse(pack, "pack.json")?;
    if file.format != 1 || file.engine != "classic" {
        return Err(invalid(
            "pack.json",
            format!(
                "format {} engine `{}`: this release reads format 1, engine `classic`",
                file.format, file.engine
            ),
        ));
    }
    let custom = pack
        .get(CUSTOM_FILE)
        .map(|text| {
            super::custom::parse(text, &catalog.handles())
                .map_err(|reason| invalid(CUSTOM_FILE, reason))
        })
        .transpose()?;
    let stages: Vec<StageAssets> = file
        .stages
        .iter()
        .map(|stage| import_stage(pack, catalog, custom.as_deref(), stage, source))
        .collect::<Result<_, _>>()?;
    if stages
        .first()
        .is_none_or(|first| first.stage.descriptor.password.is_none())
    {
        return Err(invalid("pack.json", "the first stage needs a `password`"));
    }
    if let Some(last) = stages.last()
        && last.stage.descriptor.on_clear == OnClear::NextStage
    {
        return Err(invalid(
            "pack.json",
            format!(
                "stage `{}`: the last stage's `on_clear` is `next-stage`",
                last.stage.name
            ),
        ));
    }
    let extra_passwords = file
        .extra_passwords
        .iter()
        .map(|extra| extra_password(&stages, extra))
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = BTreeSet::new();
    let passwords = stages
        .iter()
        .filter_map(|assets| assets.stage.descriptor.password);
    if let Some(twice) = passwords
        .chain(extra_passwords.iter().map(|extra| extra.letters))
        .find(|&letters| !seen.insert(letters))
    {
        let twice = String::from_utf8_lossy(&twice).into_owned();
        return Err(invalid(
            "pack.json",
            format!("password `{twice}` is used twice"),
        ));
    }
    Ok(Levels {
        stages,
        extra_passwords,
    })
}

fn extra_password(
    stages: &[StageAssets],
    extra: &super::model::ExtraPassword,
) -> Result<ExtraPassword, ImportError> {
    let at = |reason: String| invalid("pack.json", format!("`extra_passwords`: {reason}"));
    let letters = letters(&extra.password)
        .ok_or_else(|| at(format!("unknown password `{}`", extra.password)))?;
    let mut named = (0..stages.len()).filter(|&index| stages[index].stage.name == extra.stage);
    let stage = match (named.next(), named.next()) {
        (Some(stage), None) => stage,
        (None, _) => return Err(at(format!("unknown stage `{}`", extra.stage))),
        (Some(_), Some(_)) => {
            return Err(at(format!(
                "stage `{}` names more than one stage",
                extra.stage
            )));
        }
    };
    if stages[stage].stage.descriptor.password.is_none() {
        return Err(at(format!("stage `{}` is not a checkpoint", extra.stage)));
    }
    Ok(ExtraPassword { letters, stage })
}

/// Four letters `A`-`Z`.
fn letters(value: &str) -> Option<[u8; 4]> {
    <[u8; 4]>::try_from(value.as_bytes())
        .ok()
        .filter(|letters| letters.iter().all(u8::is_ascii_uppercase))
}

/// The selector of a `bg-xx`-style name: exactly two lowercase hex digits after `prefix`.
fn selector(value: &str, prefix: &str) -> Option<u8> {
    value
        .strip_prefix(prefix)
        .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        .filter(|selector| format!("{prefix}{selector:02x}") == value)
}

/// A stage's descriptor from its `pack.json` knobs.
/// `user` is the catalog a user pack's knobs must name values from.
fn descriptor(
    stage: &PackStage,
    area: u8,
    user: Option<&Catalog>,
) -> Result<Descriptor, ImportError> {
    let knob =
        |key: &str, value: &str| at_stage(stage, &format!("`{key}`: unknown value `{value}`"));
    let banner = match *stage.hud_label.as_bytes() {
        [tens @ b'0'..=b'9', b'-', ones @ b'0'..=b'9'] => [tens - b'0', ones - b'0'],
        _ => return Err(knob("hud_label", &stage.hud_label)),
    };
    let below_80 = |value: &str, prefix| selector(value, prefix).filter(|&id| id < 0x80);
    let in_game = |key: &str, value: &str, id: u8, known: Option<&BTreeSet<u8>>, what| match known {
        Some(known) if !known.contains(&id) => Err(at_stage(
            stage,
            &format!("`{key}`: `{value}` is not {what}"),
        )),
        _ => Ok(id),
    };
    let music = match stage.music.as_str() {
        "keep" => Music::Keep,
        song => {
            let id = below_80(song, "song-").ok_or_else(|| knob("music", song))?;
            let songs = user.map(|catalog| &catalog.songs);
            Music::Song(in_game(
                "music",
                song,
                id,
                songs,
                "one of the sound test's songs",
            )?)
        }
    };
    let picture = stage
        .cutscene
        .as_deref()
        .map(|picture| {
            let id = below_80(picture, "picture-").ok_or_else(|| knob("cutscene", picture))?;
            let pictures = user.map(|catalog| &catalog.pictures);
            in_game(
                "cutscene",
                picture,
                id,
                pictures,
                "a picture the game's stages show",
            )
        })
        .transpose()?;
    let password = stage
        .password
        .as_deref()
        .map(|password| letters(password).ok_or_else(|| knob("password", password)))
        .transpose()?;
    let on_clear = match stage.on_clear.as_str() {
        "next-stage" => OnClear::NextStage,
        "ending" => OnClear::Ending,
        other => return Err(knob("on_clear", other)),
    };
    Ok(Descriptor {
        area: area * 2,
        banner,
        music,
        picture,
        password,
        on_clear,
    })
}

fn parse<T: DeserializeOwned>(pack: &PackFiles, file: &str) -> Result<T, ImportError> {
    let text = pack
        .get(file)
        .ok_or_else(|| ImportError::Missing { file: file.into() })?;
    serde_json::from_str(text).map_err(|source| ImportError::Json {
        file: file.into(),
        source,
    })
}

fn invalid(file: &str, reason: impl Into<String>) -> ImportError {
    ImportError::Invalid {
        file: file.into(),
        reason: reason.into(),
    }
}

/// A room map's size and tilesets.
struct Shape {
    vertical: bool,
    across: usize,
    along: usize,
    width: usize,
    /// By ascending `firstgid`.
    tilesets: Vec<RoomTileset>,
}

/// A tileset a room's map names.
struct RoomTileset {
    firstgid: u32,
    /// Its tiles: 256 for an area tileset, `custom.tsj`'s `tilecount`.
    count: u32,
    /// The area and `Room::chr` of an area tileset; `None` for `custom.tsj`.
    area: Option<(u8, [u8; 4])>,
}

/// A cell's metatile: the stage area's id (any variant's), another area variant's or a
/// custom one.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Stage(u8),
    Other { area: u8, chr: [u8; 4], id: u8 },
    Custom(u8),
}

/// A pack's custom metatiles by id, `None` where `custom.tsj` defines none.
type Custom<'a> = Option<&'a [Option<Metatile>]>;

fn import_stage(
    pack: &PackFiles,
    catalog: &Catalog,
    custom: Custom<'_>,
    stage: &PackStage,
    source: Source,
) -> Result<StageAssets, ImportError> {
    let maps: Vec<Map> = stage
        .rooms
        .iter()
        .map(|file| parse(pack, file))
        .collect::<Result<_, _>>()?;
    let names_custom = maps
        .iter()
        .flat_map(|map| &map.tilesets)
        .any(|tileset| stem(&tileset.source) == CUSTOM);
    if names_custom && custom.is_none() {
        return Err(ImportError::Missing {
            file: CUSTOM_FILE.into(),
        });
    }
    let custom_count = custom.map_or(0, |custom| {
        u32::try_from(custom.len()).expect("at most 208 tiles")
    });
    let shapes: Vec<Shape> = stage
        .rooms
        .iter()
        .zip(&maps)
        .map(|(file, map)| {
            shape(map, catalog, custom_count).map_err(|reason| invalid(file, reason))
        })
        .collect::<Result<_, _>>()?;
    let (area, _) = shapes
        .first()
        .ok_or_else(|| at_stage(stage, "no rooms"))?
        .tilesets
        .iter()
        .find_map(|tileset| tileset.area)
        .ok_or_else(|| {
            invalid(
                &stage.rooms[0],
                "the stage's first room names no area tileset",
            )
        })?;
    let user = (source == Source::User).then_some(catalog);
    let (tables, regions) = page_regions(stage)?;
    let written_pages = stream(stage, &maps, &shapes, &regions, area)?;
    let catalog_area = catalog
        .area(area)
        .expect("a tileset's area is in the catalog");
    let (terrain, chrs) = stage_terrain(
        stage,
        catalog,
        custom,
        catalog_area,
        &shapes,
        &regions,
        &written_pages,
    )?;
    let mut schedule = [None::<u8>; PAGE_CELLS];
    let mut rooms = Vec::new();
    for (index, file) in stage.rooms.iter().enumerate() {
        let mut pages = [0; TABLE_ENTRIES];
        for (entry, (page, slot)) in pages.iter_mut().zip(tables[index]).enumerate() {
            *page = slot
                .ok_or_else(|| at_stage(stage, &format!("`{file}` entry {entry} is on no page")))?;
        }
        let (map, shape) = (&maps[index], &shapes[index]);
        let in_file = |reason| invalid(file, reason);
        let written = written_pages.len();
        if source == Source::User
            && let Some(entry) =
                held_entries(shape).find(|&entry| usize::from(pages[entry]) >= written)
        {
            return Err(in_file(format!(
                "layer `terrain`: entry {entry} is page {}, past the stream's {written} pages",
                pages[entry]
            )));
        }
        let room = import_room(map, shape, pages, chrs[index], catalog_area.split, user)
            .map_err(in_file)?;
        let spawns = import_spawns(map, &pages, &mut schedule, source).map_err(in_file)?;
        import_locks(map, shape.vertical, &pages, &mut schedule).map_err(in_file)?;
        rooms.push(RoomAssets { room, spawns });
    }
    Ok(StageAssets {
        stage: Stage {
            name: stage.name.clone(),
            area,
            descriptor: descriptor(stage, area, user)?,
        },
        terrain,
        schedule: schedule.map(|cell| cell.unwrap_or(0)),
        rooms,
    })
}

/// Each room's page table so far (entry to physical page) and each page's (room, entry)
/// regions.
type PageRegions = (Vec<[Option<u8>; TABLE_ENTRIES]>, Vec<Vec<(usize, usize)>>);

fn at_stage(stage: &PackStage, reason: &str) -> ImportError {
    invalid("pack.json", format!("stage `{}`: {reason}", stage.name))
}

fn page_regions(stage: &PackStage) -> Result<PageRegions, ImportError> {
    if stage.pages.len() != PAGES {
        return Err(at_stage(
            stage,
            &format!("{} pages, not 16", stage.pages.len()),
        ));
    }
    let mut tables = vec![[None; TABLE_ENTRIES]; stage.rooms.len()];
    let mut seen = BTreeSet::new();
    let mut regions = vec![Vec::new(); PAGES];
    for (page, listed) in (0u8..).zip(&stage.pages) {
        for region in listed {
            let room = stage
                .rooms
                .iter()
                .position(|file| *file == region.room)
                .ok_or_else(|| {
                    at_stage(
                        stage,
                        &format!("page {page}: unknown room `{}`", region.room),
                    )
                })?;
            let entry = usize::try_from(region.entry).expect("a u32 fits");
            if entry >= TABLE_ENTRIES && room != 0 {
                return Err(at_stage(
                    stage,
                    &format!(
                        "`{}` entry {entry}: only the first room holds extra pages",
                        region.room
                    ),
                ));
            }
            if !seen.insert((room, entry)) {
                return Err(at_stage(
                    stage,
                    &format!("`{}` entry {entry} is listed twice", region.room),
                ));
            }
            if let Some(slot) = tables[room].get_mut(entry) {
                *slot = Some(page);
            }
            regions[usize::from(page)].push((room, entry));
        }
    }
    Ok((tables, regions))
}

/// What a region with tiles holds.
#[derive(Clone, PartialEq, Eq)]
enum Tiles {
    /// Gid 0: a page the stream does not write.
    Unwritten,
    /// Metatiles.
    Metatiles(Vec<Key>),
}

/// A page the stream writes: its cells, from its first region with tiles `(room, entry)`.
struct Page {
    cells: Vec<Key>,
    first: (usize, usize),
}

/// The pages the stream writes: each leading page's cells from its first region with tiles,
/// which every other region with tiles of the page must match.
fn stream(
    stage: &PackStage,
    maps: &[Map],
    shapes: &[Shape],
    regions: &[Vec<(usize, usize)>],
    area: u8,
) -> Result<Vec<Page>, ImportError> {
    let mut pages = Vec::new();
    for (page, listed) in regions.iter().enumerate() {
        let mut first = None;
        for &(room, entry) in listed {
            let file = &stage.rooms[room];
            let in_terrain = |reason| invalid(file, format!("layer `terrain`: {reason}"));
            let Some(tiles) =
                region_tiles(&shapes[room], &maps[room], entry, area).map_err(in_terrain)?
            else {
                if entry >= TABLE_ENTRIES {
                    return Err(in_terrain(format!("extra page {entry} is outside the map")));
                }
                continue;
            };
            match &first {
                None => first = Some((tiles, (room, entry))),
                Some((first, _)) if *first != tiles => {
                    return Err(invalid(
                        file,
                        format!(
                            "layer `terrain`: entry {entry} disagrees with page {page}'s first region"
                        ),
                    ));
                }
                Some(_) => {}
            }
        }
        pages.push(match first {
            Some((Tiles::Metatiles(cells), first)) => Some(Page { cells, first }),
            _ => None,
        });
    }
    let written = pages.iter().take_while(|page| page.is_some()).count();
    if let Some(page) = (written..PAGES).find(|&page| pages[page].is_some()) {
        return Err(at_stage(
            stage,
            &format!("page {page} has tiles after page {written}, which has none"),
        ));
    }
    Ok(pages
        .into_iter()
        .map_while(std::convert::identity)
        .collect())
}

/// A metatile a stage appends after its area's: its corners as (bank, tile 0-63), its palette
/// and collision classes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Metatile {
    pub(super) corners: [(u8, u8); 4],
    pub(super) palette: u8,
    pub(super) class: u8,
}

/// The metatile another area's or a custom `key` names.
fn appended(key: Key, catalog: &Catalog, custom: Custom<'_>) -> Metatile {
    match key {
        Key::Stage(_) => unreachable!("the stage area's metatiles keep their ids"),
        Key::Custom(id) => custom
            .and_then(|custom| custom[usize::from(id)].clone())
            .expect("appended_keys checks every custom tile is defined"),
        Key::Other { area, chr, id } => {
            let terrain = &catalog.area(area).expect("a tileset's area").terrain;
            let id = usize::from(id);
            Metatile {
                corners: terrain.corners[id]
                    .map(|corner| (chr[usize::from(corner >> 6)], corner & 63)),
                palette: terrain.palette_classes[id],
                class: terrain.classes[id],
            }
        }
    }
}

/// The error for `cell` of `page`, in its first region's room.
fn at_cell(
    stage: &PackStage,
    shapes: &[Shape],
    page: &Page,
    cell: usize,
    reason: &str,
) -> ImportError {
    let (room, entry) = page.first;
    let (x, y) = tile_at(&shapes[room], entry, cell);
    invalid(
        &stage.rooms[room],
        format!("layer `terrain`: tile ({x}, {y}) {reason}"),
    )
}

/// The keys a stage appends after its area's metatiles, in the order the stream first writes
/// them.
fn appended_keys(
    stage: &PackStage,
    catalog: &Catalog,
    custom: Custom<'_>,
    area: &CatalogArea,
    shapes: &[Shape],
    pages: &[Page],
) -> Result<Vec<Key>, ImportError> {
    let mut keys = Vec::new();
    for page in pages {
        for (cell, &key) in page.cells.iter().enumerate() {
            let refusal = match key {
                Key::Stage(_) => continue,
                Key::Custom(id) => custom
                    .is_none_or(|custom| custom[usize::from(id)].is_none())
                    .then(|| format!("is custom tile {id}, which `{CUSTOM_FILE}` does not define")),
                Key::Other { area, chr, id } => {
                    let count = catalog
                        .area(area)
                        .expect("a tileset's area")
                        .terrain
                        .corners
                        .len();
                    if usize::from(id) >= count {
                        let name = catalog.tileset_name(area, chr).expect("a catalog variant");
                        Some(format!("is metatile {id}, past {name}'s {count}"))
                    } else if usize::from(id) >= CLASS_COUNT {
                        Some(format!(
                            "is metatile {id}, past the {CLASS_COUNT} the class table holds"
                        ))
                    } else {
                        None
                    }
                }
            };
            if let Some(reason) = refusal {
                return Err(at_cell(stage, shapes, page, cell, &reason));
            }
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    if keys.is_empty() {
        return Ok(keys);
    }
    let count = area.terrain.corners.len();
    for page in pages {
        for (cell, &key) in page.cells.iter().enumerate() {
            if let Key::Stage(id) = key
                && usize::from(id) >= count
            {
                let name = super::tileset_name(area.area, 0);
                let reason = format!("is metatile {id}, past {name}'s {count}");
                return Err(at_cell(stage, shapes, page, cell, &reason));
            }
        }
    }
    let total = count + keys.len();
    if total > CLASS_COUNT {
        return Err(at_stage(
            stage,
            &format!("{total} metatiles, past the {CLASS_COUNT} the class table holds"),
        ));
    }
    Ok(keys)
}

/// The stage's terrain, its area's metatiles then the ones it appends, and each room's `chr`.
fn stage_terrain(
    stage: &PackStage,
    catalog: &Catalog,
    custom: Custom<'_>,
    area: &CatalogArea,
    shapes: &[Shape],
    regions: &[Vec<(usize, usize)>],
    pages: &[Page],
) -> Result<(Terrain, Vec<[u8; 4]>), ImportError> {
    let keys = appended_keys(stage, catalog, custom, area, shapes, pages)?;
    let metatiles: Vec<Metatile> = keys
        .iter()
        .map(|&key| appended(key, catalog, custom))
        .collect();
    let stage_slots: Vec<usize> = (0..4)
        .filter(|&slot| {
            let mut corners = area.terrain.corners.iter().flatten();
            corners.any(|&corner| usize::from(corner >> 6) == slot)
        })
        .collect();
    let name = |chr| {
        catalog
            .tileset_name(area.area, chr)
            .expect("a catalog variant")
    };
    let mut bases = Vec::new();
    for (file, shape) in stage.rooms.iter().zip(shapes) {
        let mut own = shape.tilesets.iter().filter_map(|tileset| {
            let (tileset_area, chr) = tileset.area?;
            (tileset_area == area.area).then_some(chr)
        });
        let base = own.next().unwrap_or(area.variants[0]);
        if let Some((other, slot)) = own.find_map(|chr| {
            let slot = stage_slots.iter().find(|&&slot| chr[slot] != base[slot])?;
            Some((chr, slot))
        }) {
            return Err(invalid(
                file,
                format!(
                    "tilesets `{}` and `{}` disagree on CHR slot {slot}, which the room needs",
                    name(base),
                    name(other)
                ),
            ));
        }
        bases.push(base);
    }
    let index: BTreeMap<Key, usize> = keys.iter().enumerate().map(|(i, &key)| (key, i)).collect();
    let mut shown = vec![BTreeSet::new(); stage.rooms.len()];
    for (page, listed) in pages.iter().zip(regions) {
        let on_page: BTreeSet<usize> = page
            .cells
            .iter()
            .filter_map(|key| index.get(key).copied())
            .collect();
        for &(room, entry) in listed {
            if in_map(&shapes[room], entry) {
                shown[room].extend(&on_page);
            }
        }
    }
    let mut taken: Vec<[Option<u8>; 4]> = bases
        .iter()
        .map(|base| std::array::from_fn(|slot| stage_slots.contains(&slot).then_some(base[slot])))
        .collect();
    let slot_of = assign_slots(stage, &metatiles, &shown, &bases, &mut taken)?;
    let mut terrain = Terrain {
        stream: Vec::new(),
        ..area.terrain.clone()
    };
    for metatile in &metatiles {
        let id = terrain.corners.len();
        terrain.corners.push(
            metatile
                .corners
                .map(|(bank, tile)| slot_of[&bank] * 64 + tile),
        );
        terrain.classes[id] = metatile.class;
        if terrain.palette_classes.len() <= id {
            terrain.palette_classes.resize((id / 4 + 1) * 4, 0);
        }
        terrain.palette_classes[id] = metatile.palette;
    }
    terrain.stream = pages
        .iter()
        .flat_map(|page| &page.cells)
        .map(|key| match *key {
            Key::Stage(id) => id,
            other @ (Key::Other { .. } | Key::Custom(_)) => {
                u8::try_from(area.terrain.corners.len() + index[&other])
                    .expect("at most 208 metatiles")
            }
        })
        .collect();
    let chrs = taken
        .iter()
        .zip(&bases)
        .map(|(taken, base)| std::array::from_fn(|slot| taken[slot].unwrap_or(base[slot])))
        .collect();
    Ok((terrain, chrs))
}

/// Gives every bank the appended `metatiles` use one CHR slot for the whole stage, in first-use
/// order, marking it in `taken` for each room that `shown` says shows one of them.
fn assign_slots(
    stage: &PackStage,
    metatiles: &[Metatile],
    shown: &[BTreeSet<usize>],
    bases: &[[u8; 4]],
    taken: &mut [[Option<u8>; 4]],
) -> Result<BTreeMap<u8, u8>, ImportError> {
    let mut slot_of = BTreeMap::new();
    for (bank, _) in metatiles.iter().flat_map(|metatile| metatile.corners) {
        if slot_of.contains_key(&bank) {
            continue;
        }
        let users: Vec<usize> = (0..shown.len())
            .filter(|&room| {
                shown[room]
                    .iter()
                    .any(|&i| metatiles[i].corners.iter().any(|&(used, _)| used == bank))
            })
            .collect();
        let gives = |slot: usize| {
            users
                .iter()
                .any(|&room| taken[room][slot].unwrap_or(bases[room][slot]) == bank)
        };
        let fits = |slot: usize| {
            users
                .iter()
                .all(|&room| taken[room][slot].is_none_or(|held| held == bank))
        };
        let slot = (0..4)
            .filter(|&slot| gives(slot))
            .chain((0..4).filter(|&slot| !gives(slot)))
            .find(|&slot| fits(slot))
            .ok_or_else(|| {
                let rooms: Vec<&str> = users.iter().map(|&room| stem(&stage.rooms[room])).collect();
                at_stage(
                    stage,
                    &format!(
                        "bank ${bank:02x} finds no CHR slot free in every room that uses it \
                         (rooms {})",
                        rooms.join(", ")
                    ),
                )
            })?;
        for &room in &users {
            taken[room][slot] = Some(bank);
        }
        slot_of.insert(bank, u8::try_from(slot).expect("a slot"));
    }
    Ok(slot_of)
}

/// The map tile, `(x, y)`, holding `cell` of `entry`'s region.
fn tile_at(shape: &Shape, entry: usize, cell: usize) -> (usize, usize) {
    let index = tile_index(shape.vertical, shape.width, region_place(entry), cell);
    (index % shape.width, index / shape.width)
}

fn layer<'a>(map: &'a Map, name: &str) -> Result<&'a Layer, String> {
    map.layers
        .iter()
        .find(|layer| layer.name == name)
        .ok_or_else(|| format!("no layer `{name}`"))
}

fn objects<'a>(map: &'a Map, name: &str) -> Result<&'a [Object], String> {
    layer(map, name)?
        .objects
        .as_deref()
        .ok_or_else(|| format!("layer `{name}` is not an object group"))
}

fn property<'a>(properties: &'a [Property], name: &str) -> Option<&'a Property> {
    properties.iter().find(|property| property.name == name)
}

/// A string property's value.
fn string<'a>(properties: &'a [Property], name: &str) -> Result<&'a str, String> {
    property(properties, name)
        .ok_or_else(|| format!("no property `{name}`"))?
        .value
        .as_str()
        .ok_or_else(|| format!("property `{name}` is not a string"))
}

/// An int property's value, 0 when it is missing.
fn int<T: TryFrom<i64> + Default>(properties: &[Property], name: &str) -> Result<T, String> {
    let Some(property) = property(properties, name) else {
        return Ok(T::default());
    };
    property
        .value
        .as_i64()
        .and_then(|value| T::try_from(value).ok())
        .ok_or_else(|| format!("property `{name}` is not an int in range"))
}

/// A `bg-xx`-style preset's selector, which in a user pack must be one of `known`.
fn preset(
    properties: &[Property],
    name: &str,
    prefix: &str,
    known: Option<&BTreeSet<u8>>,
) -> Result<u8, String> {
    let value = string(properties, name)?;
    let preset = selector(value, prefix)
        .ok_or_else(|| format!("property `{name}`: unknown preset `{value}`"))?;
    match known {
        Some(known) if !known.contains(&preset) => Err(format!(
            "property `{name}`: `{value}` is not a preset the game's rooms use"
        )),
        _ => Ok(preset),
    }
}

/// The file stem of a map's tileset `source`, the tileset's name.
fn stem(source: &str) -> &str {
    Path::new(source)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
}

/// The map's shape; `custom_count` is `custom.tsj`'s `tilecount`.
fn shape(map: &Map, catalog: &Catalog, custom_count: u32) -> Result<Shape, String> {
    if map.kind != "map"
        || map.orientation != "orthogonal"
        || map.infinite
        || (map.tilewidth, map.tileheight) != (16, 16)
    {
        return Err("not a finite orthogonal map of 16 px tiles".into());
    }
    let vertical = match string(&map.properties, "scroll")? {
        "horizontal" => false,
        "vertical" => true,
        other => return Err(format!("property `scroll`: unknown value `{other}`")),
    };
    let (width, height) = (
        usize::try_from(map.width).expect("a u32 fits"),
        usize::try_from(map.height).expect("a u32 fits"),
    );
    let (across_tiles, along_tiles) = if vertical {
        (width, height)
    } else {
        (height, width)
    };
    if !matches!(across_tiles, 16 | 32) || along_tiles % 16 != 0 || along_tiles < 256 {
        return Err(format!(
            "a {width}x{height} map is not a strip of 16 or more whole pages, 1 or 2 across"
        ));
    }
    let mut customs_seen = false;
    let mut tilesets = map
        .tilesets
        .iter()
        .map(|tileset| {
            if tileset.firstgid == 0 {
                return Err("a tileset's firstgid is 1 or more".to_string());
            }
            let name = stem(&tileset.source);
            if name == CUSTOM {
                if customs_seen {
                    return Err(format!("`{CUSTOM_FILE}` is listed twice"));
                }
                customs_seen = true;
                return Ok(RoomTileset {
                    firstgid: tileset.firstgid,
                    count: custom_count,
                    area: None,
                });
            }
            let area = catalog
                .tileset(name)
                .ok_or_else(|| format!("unknown tileset `{name}`"))?;
            Ok(RoomTileset {
                firstgid: tileset.firstgid,
                count: TILESET_TILES,
                area: Some(area),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    tilesets.sort_by_key(|tileset| tileset.firstgid);
    if tilesets.windows(2).any(|pair| {
        u64::from(pair[0].firstgid) + u64::from(pair[0].count) > u64::from(pair[1].firstgid)
    }) {
        return Err("two tilesets' gids overlap".into());
    }
    Ok(Shape {
        vertical,
        across: across_tiles / 16,
        along: along_tiles / 16,
        width,
        tilesets,
    })
}

/// The page-table entries whose region lies inside the room's map.
fn held_entries(shape: &Shape) -> impl Iterator<Item = usize> {
    (0..TABLE_ENTRIES).filter(|&entry| in_map(shape, entry))
}

/// Whether `entry`'s region lies inside the room's map.
fn in_map(shape: &Shape, entry: usize) -> bool {
    let (along, band) = region_place(entry);
    along < shape.along && band < shape.across
}

/// The tiles of `entry`'s region, `None` when the map has no tiles there; `area` is the
/// stage's.
fn region_tiles(shape: &Shape, map: &Map, entry: usize, area: u8) -> Result<Option<Tiles>, String> {
    if !in_map(shape, entry) {
        return Ok(None);
    }
    let place = region_place(entry);
    let terrain = layer(map, "terrain")?;
    let data = terrain
        .data
        .as_deref()
        .filter(|data| data.len() == shape.along * shape.across * PAGE_CELLS)
        .ok_or("not a tile layer the map's size")?;
    let mut cells = [Key::Stage(0); PAGE_CELLS];
    let mut empty = 0;
    for (cell, key) in cells.iter_mut().enumerate() {
        let gid = data[tile_index(shape.vertical, shape.width, place, cell)];
        if gid == 0 {
            empty += 1;
            continue;
        }
        if gid & FLIP_BITS != 0 {
            return Err(format!("gid {gid:#x} is flipped"));
        }
        let tileset = shape
            .tilesets
            .iter()
            .rev()
            .find(|tileset| tileset.firstgid <= gid);
        let (tileset, id) = tileset
            .and_then(|tileset| {
                let id = gid - tileset.firstgid;
                let id = u8::try_from(id).ok().filter(|_| id < tileset.count)?;
                Some((tileset, id))
            })
            .ok_or_else(|| format!("gid {gid} is not in a tileset"))?;
        *key = match tileset.area {
            None => Key::Custom(id),
            Some((tileset_area, _)) if tileset_area == area => Key::Stage(id),
            Some((area, chr)) => Key::Other { area, chr, id },
        };
    }
    match empty {
        0 => Ok(Some(Tiles::Metatiles(cells.to_vec()))),
        PAGE_CELLS => Ok(Some(Tiles::Unwritten)),
        _ => Err(format!("entry {entry}'s page mixes gid 0 and tiles")),
    }
}

fn marker<'a>(map: &'a Map, class: &str) -> Result<&'a Object, String> {
    let mut found = objects(map, "markers")?
        .iter()
        .filter(|object| object.kind == class);
    match (found.next(), found.next()) {
        (Some(marker), None) => Ok(marker),
        (None, _) => Err(format!("layer `markers`: no `{class}`")),
        (Some(_), Some(_)) => Err(format!("layer `markers`: more than one `{class}`")),
    }
}

fn import_room(
    map: &Map,
    shape: &Shape,
    pages: [u8; TABLE_ENTRIES],
    chr: [u8; 4],
    split: Option<Split>,
    user: Option<&Catalog>,
) -> Result<Room, String> {
    let player = marker(map, "player-start")?;
    let camera = marker(map, "camera-start")?;
    let at =
        |object: &Object, reason: String| format!("layer `markers` object {}: {reason}", object.id);
    let (camera_x, camera_y) = (camera.x.0, camera.y.0);
    let first_spawn_column = match property(&map.properties, "first_spawn_column") {
        Some(property) => {
            let column = column(&property.value).ok_or_else(|| {
                format!(
                    "property `first_spawn_column` {} is not a column 0-511",
                    property.value
                )
            })?;
            u8::try_from(schedule_cell(&pages, column)).expect("a page's cell")
        }
        None => derived_first_spawn_column(shape.vertical, camera_x, camera_y, &pages),
    };
    let last = pages[PAGES - 1];
    let trailing = pages[..PAGES]
        .iter()
        .rev()
        .take_while(|&&page| page == last)
        .count();
    let arrival = match string(&map.properties, "arrival")? {
        "unchanged" => Arrival::Unchanged,
        "held" => Arrival::Held,
        "flying" => Arrival::Flying,
        other => return Err(format!("property `arrival`: unknown value `{other}`")),
    };
    let split = match string(&map.properties, "hud_split")? {
        "standard" => Some(split.ok_or("property `hud_split`: the area has no standard split")?),
        "none" if arrival == Arrival::Unchanged => None,
        "none" => return Err("property `hud_split`: `none` needs arrival `unchanged`".into()),
        other => return Err(format!("property `hud_split`: unknown value `{other}`")),
    };
    let player_int = |name| int(&player.properties, name).map_err(|reason| at(player, reason));
    Ok(Room {
        background_palette: preset(
            &map.properties,
            "background_palette",
            "bg-",
            user.map(|catalog| &catalog.background_palettes),
        )?,
        sprite_palette: preset(
            &map.properties,
            "sprite_palette",
            "sprites-",
            user.map(|catalog| &catalog.sprite_palettes),
        )?,
        chr,
        orientation: (u8::from(shape.vertical) * orientation::VERTICAL)
            | (u8::from(shape.across == 2) * orientation::TWO_PAGES),
        raised_page_count: u8::try_from(PAGES - trailing).expect("a page count"),
        pages,
        start: Start {
            player_x: player.x.0,
            player_y: player.y.0,
            camera_x,
            camera_y,
            ring_y: camera_y,
            scroll_lock: player_int("scroll_lock")?,
            scroll_mode: player_int("scroll_mode")?,
            mirroring: match (shape.vertical, camera_y >= 0x1000) {
                (true, _) => Mirroring::SideBySide,
                (false, true) => Mirroring::Stacked,
                (false, false) => Mirroring::SingleLow,
            },
            first_spawn_column,
            facing: player_int("facing")?,
        },
        raster: split.map(|split| split.raster),
        fixed_rows: split.map_or([false; 16], |split| split.fixed_rows),
        fixed_quads: split.map_or([false; 8], |split| split.fixed_quads),
        arrival,
    })
}

/// Writes `value` into a schedule cell, which a room may already have set.
fn set_cell(schedule: &mut [Option<u8>; PAGE_CELLS], cell: usize, value: u8) -> Result<(), String> {
    match schedule[cell] {
        Some(held) if held != value => Err(format!(
            "schedule cell {cell:#04x} is both {held:#04x} and {value:#04x}"
        )),
        _ => {
            schedule[cell] = Some(value);
            Ok(())
        }
    }
}

/// A schedule column (0-511) from a JSON value.
fn column(value: &serde_json::Value) -> Option<usize> {
    usize::try_from(value.as_u64()?)
        .ok()
        .filter(|&column| column < TABLE_ENTRIES * 16)
}

fn import_spawns(
    map: &Map,
    pages: &[u8; TABLE_ENTRIES],
    schedule: &mut [Option<u8>; PAGE_CELLS],
    source: Source,
) -> Result<SpawnTable, String> {
    let mut records = vec![None; RECORD_IDS];
    for object in objects(map, "spawns")? {
        let at = |reason: String| format!("layer `spawns` object {}: {reason}", object.id);
        let id = usize::try_from(object.id)
            .ok()
            .and_then(|id| id.checked_sub(1))
            .filter(|&id| id < RECORD_IDS)
            .ok_or_else(|| at("an actor's id is 1-64".into()))?;
        if records[id].is_some() {
            return Err(at("the id is used twice".into()));
        }
        let properties = &object.properties;
        let variant = match property(properties, "variant") {
            Some(_) => Some(string(properties, "variant").map_err(at)?),
            None => None,
        };
        let (second_class, kind) = kind_of(&object.kind, variant).map_err(at)?;
        if source == Source::User && object.kind == NO_HANDLER.0 {
            return Err(at("the game has no handler for it".into()));
        }
        let field = |name| int(properties, name).map_err(at);
        records[id] = Some(SpawnRecord {
            x: object.x.0,
            y: object.y.0,
            flags: field("flags")?,
            chr_slot: field("chr_slot")?,
            second_class,
            kind,
            state: field("state")?,
            behaviour: field("behaviour")?,
            subtype: field("subtype")?,
            health: int(properties, "health").map_err(at)?,
            contact: field("contact")?,
        });
        let triggers = match property(properties, "triggers") {
            Some(triggers) => triggers
                .value
                .as_array()
                .ok_or_else(|| at("property `triggers` is not a list".into()))?
                .as_slice(),
            None => &[],
        };
        let record = u8::try_from(id).expect("an id fits");
        for trigger in triggers {
            let column = column(&trigger["value"])
                .ok_or_else(|| at(format!("trigger {trigger} is not a column 0-511")))?;
            set_cell(
                schedule,
                schedule_cell(pages, column),
                CELL_PENDING | record,
            )
            .map_err(at)?;
        }
    }
    Ok(SpawnTable { records })
}

fn import_locks(
    map: &Map,
    vertical: bool,
    pages: &[u8; TABLE_ENTRIES],
    schedule: &mut [Option<u8>; PAGE_CELLS],
) -> Result<(), String> {
    let locks = objects(map, "markers")?
        .iter()
        .filter(|object| object.kind == "scroll-lock");
    for object in locks {
        let at = |reason: String| format!("layer `markers` object {}: {reason}", object.id);
        let (along, across) = along_across(vertical, object.x.0, object.y.0);
        let column = usize::from(across >> 12) * 256 + usize::from(along >> 8);
        if column >= TABLE_ENTRIES * 16 {
            return Err(at("a scroll lock past the second band".into()));
        }
        let locks: u8 = int(&object.properties, "locks").map_err(at)?;
        if locks > CELL_RECORD {
            return Err(at(format!("locks {locks:#04x} is more than six bits")));
        }
        set_cell(schedule, schedule_cell(pages, column), CELL_LOCK | locks).map_err(at)?;
    }
    Ok(())
}
