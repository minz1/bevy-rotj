//! The importer: a pack's files into [`Levels`], the game's typed assets.

use std::collections::BTreeSet;
use std::path::Path;

use serde::de::DeserializeOwned;
use thiserror::Error;

use super::model::{Layer, Map, Object, PackFile, PackStage, Property};
use super::{
    Catalog, ExtraPassword, Levels, NO_HANDLER, PAGE_CELLS, PAGES, PackFiles, RoomAssets, Source,
    Split, Stage, StageAssets, TABLE_ENTRIES, TILESET_TILES, along_across,
    derived_first_spawn_column, kind_of, region_place, schedule_cell, tile_index,
};
use crate::actor::spawn::SpawnTable;
use crate::actor::stage::{Music, OnClear, Stage as Descriptor};
use crate::actor::world::orientation;
use crate::actors::{CELL_LOCK, CELL_PENDING, CELL_RECORD, SpawnRecord};
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
    let stages: Vec<StageAssets> = file
        .stages
        .iter()
        .map(|stage| import_stage(pack, catalog, stage, source))
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
fn descriptor(stage: &PackStage, area: u8) -> Result<Descriptor, ImportError> {
    let knob =
        |key: &str, value: &str| at_stage(stage, &format!("`{key}`: unknown value `{value}`"));
    let banner = match *stage.hud_label.as_bytes() {
        [tens @ b'0'..=b'9', b'-', ones @ b'0'..=b'9'] => [tens - b'0', ones - b'0'],
        _ => return Err(knob("hud_label", &stage.hud_label)),
    };
    let below_80 = |value: &str, prefix| selector(value, prefix).filter(|&id| id < 0x80);
    let music = match stage.music.as_str() {
        "keep" => Music::Keep,
        song => Music::Song(below_80(song, "song-").ok_or_else(|| knob("music", song))?),
    };
    let picture = stage
        .cutscene
        .as_deref()
        .map(|picture| below_80(picture, "picture-").ok_or_else(|| knob("cutscene", picture)))
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

/// A room map's size and tileset.
struct Shape {
    vertical: bool,
    across: usize,
    along: usize,
    width: usize,
    firstgid: u32,
    area: u8,
    chr: [u8; 4],
}

fn import_stage(
    pack: &PackFiles,
    catalog: &Catalog,
    stage: &PackStage,
    source: Source,
) -> Result<StageAssets, ImportError> {
    let maps: Vec<Map> = stage
        .rooms
        .iter()
        .map(|file| parse(pack, file))
        .collect::<Result<_, _>>()?;
    let shapes: Vec<Shape> = stage
        .rooms
        .iter()
        .zip(&maps)
        .map(|(file, map)| shape(map, catalog).map_err(|reason| invalid(file, reason)))
        .collect::<Result<_, _>>()?;
    let area = shapes
        .first()
        .ok_or_else(|| at_stage(stage, "no rooms"))?
        .area;
    if let Some((file, _)) = stage
        .rooms
        .iter()
        .zip(&shapes)
        .find(|(_, shape)| shape.area != area)
    {
        return Err(invalid(file, "its tileset's area is not the stage's"));
    }
    let (tables, regions) = page_regions(stage)?;
    let stream = stream(stage, &maps, &shapes, &regions)?;
    let catalog_area = catalog
        .area(area)
        .expect("a tileset's area is in the catalog");
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
        let room = import_room(map, shape, pages, catalog_area.split).map_err(in_file)?;
        let spawns = import_spawns(map, &pages, &mut schedule, source).map_err(in_file)?;
        import_locks(map, shape.vertical, &pages, &mut schedule).map_err(in_file)?;
        rooms.push(RoomAssets { room, spawns });
    }
    Ok(StageAssets {
        stage: Stage {
            name: stage.name.clone(),
            area,
            descriptor: descriptor(stage, area)?,
        },
        terrain: Terrain {
            stream,
            ..catalog_area.terrain.clone()
        },
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
    /// Metatile ids.
    Metatiles(Vec<u8>),
}

/// The map stream: each leading page's cells from its first region with tiles, which every
/// other region with tiles of the page must match.
fn stream(
    stage: &PackStage,
    maps: &[Map],
    shapes: &[Shape],
    regions: &[Vec<(usize, usize)>],
) -> Result<Vec<u8>, ImportError> {
    let mut pages = Vec::new();
    for (page, listed) in regions.iter().enumerate() {
        let mut first = None;
        for &(room, entry) in listed {
            let file = &stage.rooms[room];
            let in_terrain = |reason| invalid(file, format!("layer `terrain`: {reason}"));
            let Some(tiles) =
                region_tiles(&shapes[room], &maps[room], entry).map_err(in_terrain)?
            else {
                if entry >= TABLE_ENTRIES {
                    return Err(in_terrain(format!("extra page {entry} is outside the map")));
                }
                continue;
            };
            match &first {
                None => first = Some(tiles),
                Some(first) if *first != tiles => {
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
        pages.push(first.unwrap_or(Tiles::Unwritten));
    }
    let written = pages
        .iter()
        .take_while(|tiles| **tiles != Tiles::Unwritten)
        .count();
    if let Some(page) = (written..PAGES).find(|&page| pages[page] != Tiles::Unwritten) {
        return Err(at_stage(
            stage,
            &format!("page {page} has tiles after page {written}, which has none"),
        ));
    }
    Ok(pages[..written]
        .iter()
        .flat_map(|tiles| match tiles {
            Tiles::Metatiles(cells) => cells.as_slice(),
            Tiles::Unwritten => &[],
        })
        .copied()
        .collect())
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

/// A `bg-xx`-style preset's selector.
fn preset(properties: &[Property], name: &str, prefix: &str) -> Result<u8, String> {
    let value = string(properties, name)?;
    selector(value, prefix).ok_or_else(|| format!("property `{name}`: unknown preset `{value}`"))
}

fn shape(map: &Map, catalog: &Catalog) -> Result<Shape, String> {
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
    let [tileset] = map.tilesets.as_slice() else {
        return Err("a room uses exactly one tileset".into());
    };
    if tileset.firstgid == 0 {
        return Err("a tileset's firstgid is 1 or more".into());
    }
    let name = Path::new(&tileset.source)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    let (area, chr) = catalog
        .tileset(name)
        .ok_or_else(|| format!("unknown tileset `{name}`"))?;
    Ok(Shape {
        vertical,
        across: across_tiles / 16,
        along: along_tiles / 16,
        width,
        firstgid: tileset.firstgid,
        area,
        chr,
    })
}

/// The tiles of `entry`'s region, `None` when the map has no tiles there.
fn region_tiles(shape: &Shape, map: &Map, entry: usize) -> Result<Option<Tiles>, String> {
    let place @ (along, band) = region_place(entry);
    if along >= shape.along || band >= shape.across {
        return Ok(None);
    }
    let terrain = layer(map, "terrain")?;
    let data = terrain
        .data
        .as_deref()
        .filter(|data| data.len() == shape.along * shape.across * PAGE_CELLS)
        .ok_or("not a tile layer the map's size")?;
    let mut cells = [0; PAGE_CELLS];
    let mut empty = 0;
    for (cell, metatile) in cells.iter_mut().enumerate() {
        let gid = data[tile_index(shape.vertical, shape.width, place, cell)];
        if gid == 0 {
            empty += 1;
            continue;
        }
        if gid & FLIP_BITS != 0 {
            return Err(format!("gid {gid:#x} is flipped"));
        }
        *metatile = gid
            .checked_sub(shape.firstgid)
            .filter(|&id| id < TILESET_TILES)
            .and_then(|id| u8::try_from(id).ok())
            .ok_or_else(|| format!("gid {gid} is not in the tileset"))?;
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
    split: Option<Split>,
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
        background_palette: preset(&map.properties, "background_palette", "bg-")?,
        sprite_palette: preset(&map.properties, "sprite_palette", "sprites-")?,
        chr: shape.chr,
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
