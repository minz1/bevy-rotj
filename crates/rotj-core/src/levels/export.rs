//! The exporter: the original game's levels from the ROM, and any [`Levels`] written as a
//! pack.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::custom::SURFACES;
use super::model::{
    self, Extra, Layer, Map, Object, PackFile, PackStage, Position, Property, Region, TilesetRef,
    World, WorldMap, to_text,
};
use super::{
    Catalog, ExtraPassword, JSON_VERSION, Levels, NO_HANDLER, PAGE_CELLS, PAGES, PackFiles,
    RoomAssets, Source, Stage, StageAssets, TABLE_ENTRIES, TILED_VERSION, along_across, class_of,
    classes, derived_first_spawn_column, import, region_place, schedule_cell, tile_index,
    variant_name,
};
use crate::actor::boot::{PASSWORDS, payload_byte};
use crate::actor::spawn::SpawnTable;
use crate::actor::stage::{Music, OnClear, Stage as Descriptor};
use crate::actor::world::orientation;
use crate::actors::{CELL_FLAGS, CELL_LOCK, CELL_PENDING, CELL_RECORD, spawn_schedule};
use crate::rooms::{Arrival, Room, listed_rooms};
use crate::terrain::Terrain;

/// The `player-start` marker's object id.
const PLAYER_START_ID: u32 = 65;
/// The `camera-start` marker's object id.
const CAMERA_START_ID: u32 = 66;
/// The first `scroll-lock` marker's object id.
const FIRST_LOCK_ID: u32 = 67;
/// The px between stages' rows in the world file.
const WORLD_ROW: i64 = 4352;
/// The px between rooms in a world row.
const WORLD_GAP: i64 = 256;
/// The password screen's place the pause never shows (`$6E6C`'s fourteenth).
const EXTRA_PLACE: u16 = 13;
/// The checkpoint it starts (`check_password`'s `place.min(12)`), counted from 0.
const EXTRA_CHECKPOINT: usize = 12;

impl Levels {
    /// The original game's levels, from today's importers, the stage descriptors and the
    /// password screen; `None` where an area's terrain, schedule or room does not import.
    #[must_use]
    pub fn from_rom(prg: &[u8], chr: &[u8]) -> Option<Levels> {
        let rooms = listed_rooms(prg);
        let mut stages = Vec::new();
        for area in 0u8.. {
            let Some(terrain) = Terrain::import(prg, chr, usize::from(area)) else {
                break;
            };
            let offset = area * 2;
            let descriptor = Descriptor::from_rom(prg, area);
            let rooms = rooms
                .iter()
                .filter(|&&(room_area, _)| room_area == offset)
                .map(|&(_, room)| {
                    Some(RoomAssets {
                        room: Room::import(prg, offset, room)?,
                        spawns: SpawnTable::import(prg, offset, room),
                    })
                })
                .collect::<Option<_>>()?;
            stages.push(StageAssets {
                stage: Stage {
                    name: hud_label(&descriptor),
                    area,
                    descriptor,
                },
                terrain,
                schedule: spawn_schedule(prg, offset).ok()?.try_into().ok()?,
                rooms,
            });
        }
        let stage = (0..stages.len())
            .filter(|&index| stages[index].stage.descriptor.password.is_some())
            .nth(EXTRA_CHECKPOINT)?;
        let letters =
            [0, 1, 2, 3].map(|i| b'A' + payload_byte(chr, PASSWORDS + 4 * EXTRA_PLACE + i));
        Some(Levels {
            stages,
            extra_passwords: vec![ExtraPassword { letters, stage }],
        })
    }

    /// The original game as it plays: its pack imported, with the ROM's schedule cells and
    /// room start mirrorings the format drops put back; `None` where the ROM's levels do not
    /// import.
    #[must_use]
    pub fn original(prg: &[u8], chr: &[u8]) -> Option<Levels> {
        let rom = Levels::from_rom(prg, chr)?;
        let catalog = Catalog::from_levels(&rom);
        let mut levels = import(&write(&rom, &catalog)?, &catalog, Source::Original).ok()?;
        for (stage, rom_stage) in levels.stages.iter_mut().zip(&rom.stages) {
            for (cell, &rom_cell) in stage.schedule.iter_mut().zip(&rom_stage.schedule) {
                *cell |= rom_cell;
            }
            for (room, rom_room) in stage.rooms.iter_mut().zip(&rom_stage.rooms) {
                room.room.start.mirroring = rom_room.room.start.mirroring;
            }
        }
        Some(levels)
    }
}

/// The banner's digits as `pack.json` writes them, `3-2`.
fn hud_label(descriptor: &Descriptor) -> String {
    format!("{}-{}", descriptor.banner[0], descriptor.banner[1])
}

fn letters_text(letters: [u8; 4]) -> String {
    letters.iter().copied().map(char::from).collect()
}

/// The original game's pack, "The Original Game", from the ROM.
#[must_use]
pub fn export(prg: &[u8], chr: &[u8]) -> Option<PackFiles> {
    let levels = Levels::from_rom(prg, chr)?;
    write(&levels, &Catalog::from_levels(&levels))
}

/// `levels` as a pack's files. It reads only what the format carries: the schedule's
/// pending and lock cells on the rooms' pages, and the records they name. `None` where a
/// stage's metatiles are not its catalog area's or a room's CHR is not one of the area's
/// variants: it writes the original game's terrain only.
///
/// # Panics
/// On a room page of 16 or more, a first spawn column off the room's pages, an extra
/// password's stage past the stages, or a read cell naming a record the room lacks or whose
/// kind has no class: [`import`](super::import) and [`Levels::from_rom`] give none of these.
#[must_use]
pub fn write(levels: &Levels, catalog: &Catalog) -> Option<PackFiles> {
    if !levels.stages.iter().all(|stage| catalogued(stage, catalog)) {
        return None;
    }
    let mut files = PackFiles::new();
    let mut world = Vec::new();
    let mut stages = Vec::new();
    let mut uses_no_handler = false;
    for (row, stage) in (0i64..).zip(&levels.stages) {
        let descriptor = &stage.stage.descriptor;
        let names: Vec<String> = (1..=stage.rooms.len())
            .map(|room| format!("rooms/{}-{room}.tmj", stage.stage.name))
            .collect();
        let pages = page_map(stage);
        let mut x = 0;
        for (index, name) in names.iter().enumerate() {
            let map = room_map(stage, &pages, index, catalog);
            let (width, height) = (map.width * 16, map.height * 16);
            uses_no_handler |= map
                .layers
                .iter()
                .flat_map(|layer| layer.objects.iter().flatten())
                .any(|object| object.kind == NO_HANDLER.0);
            files.insert(name.clone(), to_text(&map));
            world.push(WorldMap {
                file_name: name.clone(),
                height,
                width,
                x,
                y: row * WORLD_ROW,
                extra: Extra::new(),
            });
            x += i64::from(width) + WORLD_GAP;
        }
        stages.push(PackStage {
            name: stage.stage.name.clone(),
            rooms: names.clone(),
            pages: pages
                .iter()
                .map(|regions| {
                    regions
                        .iter()
                        .map(|&(room, entry)| Region {
                            room: names[room].clone(),
                            entry: u32::try_from(entry).expect("an entry fits"),
                            extra: Extra::new(),
                        })
                        .collect()
                })
                .collect(),
            hud_label: hud_label(descriptor),
            music: match descriptor.music {
                Music::Song(song) => format!("song-{song:02x}"),
                Music::Keep => "keep".into(),
            },
            cutscene: descriptor
                .picture
                .map(|picture| format!("picture-{picture:02x}")),
            password: descriptor.password.map(letters_text),
            on_clear: match descriptor.on_clear {
                OnClear::NextStage => "next-stage",
                OnClear::Ending => "ending",
            }
            .into(),
            extra: Extra::new(),
        });
    }
    let extra_passwords = levels
        .extra_passwords
        .iter()
        .map(|extra| model::ExtraPassword {
            password: letters_text(extra.letters),
            stage: levels.stages[extra.stage].stage.name.clone(),
            extra: Extra::new(),
        })
        .collect();
    let pack = PackFile {
        format: 1,
        name: "The Original Game".into(),
        author: "Sunsoft".into(),
        engine: "classic".into(),
        stages,
        extra_passwords,
        extra: Extra::new(),
    };
    files.insert("pack.json".into(), to_text(&pack));
    let world = World {
        maps: world,
        only_show_adjacent_maps: false,
        kind: "world".into(),
        extra: Extra::new(),
    };
    files.insert("world.world".into(), to_text(&world));
    files.insert(
        "rotj.tiled-project".into(),
        to_text(&project(catalog, uses_no_handler)),
    );
    Some(files)
}

/// The Tiled project file: the tileset folder, an enum per `propertytype` the rooms name and
/// a class per actor family and marker, `no_handler` only where the rooms have one.
fn project(catalog: &Catalog, uses_no_handler: bool) -> Value {
    let selectors = |prefix: &str, used: &BTreeSet<u8>| {
        used.iter()
            .map(|selector| format!("{prefix}{selector:02x}"))
            .collect::<Vec<_>>()
    };
    let mut types = vec![
        enumeration("arrival", ["unchanged", "held", "flying"]),
        enumeration("hud_split", ["standard", "none"]),
        enumeration("scroll", ["horizontal", "vertical"]),
        enumeration(
            "background_palette",
            selectors("bg-", &catalog.background_palettes),
        ),
        enumeration(
            "sprite_palette",
            selectors("sprites-", &catalog.sprite_palettes),
        ),
        enumeration("surface", SURFACES.map(|(name, _)| name)),
    ];
    let families: Vec<(&str, &[u8])> = classes(false)
        .chain(classes(true))
        .filter(|&(family, _)| uses_no_handler || family != NO_HANDLER.0)
        .collect();
    for &(family, kinds) in families.iter().filter(|(_, kinds)| kinds.len() > 1) {
        let variants = kinds.iter().copied().map(variant_name);
        types.push(enumeration(&format!("{family}_variant"), variants));
    }
    for &(family, kinds) in &families {
        let mut members: Vec<Value> = [
            "behaviour",
            "chr_slot",
            "contact",
            "flags",
            "health",
            "state",
            "subtype",
        ]
        .map(int_member)
        .into();
        members.push(json!({ "name": "triggers", "type": "list", "value": [] }));
        if kinds.len() > 1 {
            members.push(json!({
                "name": "variant",
                "propertyType": format!("{family}_variant"),
                "type": "string",
                "value": variant_name(kinds[0]),
            }));
        }
        types.push(class(family, members));
    }
    types.push(class(
        "player-start",
        ["facing", "scroll_lock", "scroll_mode"]
            .map(int_member)
            .into(),
    ));
    types.push(class("camera-start", Vec::new()));
    types.push(class("scroll-lock", vec![int_member("locks")]));
    for (id, entry) in (1..).zip(&mut types) {
        entry["id"] = Value::from(id);
    }
    json!({
        "automappingRulesFile": "",
        "commands": [],
        "compatibilityVersion": 1100,
        "extensionsPath": "extensions",
        "folders": [".", "../../tilesets"],
        "properties": [],
        "propertyTypes": types,
    })
}

fn enumeration(name: &str, values: impl IntoIterator<Item = impl Into<Value>>) -> Value {
    json!({
        "name": name,
        "storageType": "string",
        "type": "enum",
        "values": values.into_iter().map(Into::into).collect::<Vec<Value>>(),
        "valuesAsFlags": false,
    })
}

fn class(name: &str, members: Vec<Value>) -> Value {
    json!({
        "color": "#ffa0a0a4",
        "drawFill": true,
        "members": Value::from(members),
        "name": name,
        "type": "class",
        "useAs": ["object"],
    })
}

/// A class member: an `int`, 0 when an object leaves it out.
fn int_member(name: &str) -> Value {
    json!({ "name": name, "type": "int", "value": 0 })
}

/// Whether `stage`'s metatiles are its catalog area's and its rooms' CHR the area's variants.
fn catalogued(stage: &StageAssets, catalog: &Catalog) -> bool {
    catalog.area(stage.stage.area).is_some_and(|area| {
        let (ours, theirs) = (&stage.terrain, &area.terrain);
        (&ours.corners, &ours.classes, &ours.palette_classes)
            == (&theirs.corners, &theirs.classes, &theirs.palette_classes)
            && stage
                .rooms
                .iter()
                .all(|assets| area.variants.contains(&assets.room.chr))
    })
}

/// The stage's physical page map: each page's (room, entry) regions, sorted, with the
/// extra pages appended to the first room as entries 32 and up.
fn page_map(stage: &StageAssets) -> Vec<Vec<(usize, usize)>> {
    let mut pages = vec![Vec::new(); PAGES];
    for (room, assets) in stage.rooms.iter().enumerate() {
        for (entry, &page) in assets.room.pages.iter().enumerate() {
            pages[usize::from(page)].push((room, entry));
        }
    }
    let has_tiles = |&(room, entry): &(usize, usize)| {
        entry < PAGES || stage.rooms[room].room.orientation & orientation::TWO_PAGES != 0
    };
    let reach = stage.terrain.stream.len() / PAGE_CELLS;
    let extras: Vec<usize> = (0..reach.min(PAGES))
        .filter(|&page| !pages[page].iter().any(has_tiles))
        .collect();
    for (extra, &page) in extras.iter().enumerate() {
        pages[page].push((0, TABLE_ENTRIES + extra));
    }
    for regions in &mut pages {
        regions.sort_unstable();
    }
    pages
}

/// Room `index`'s map.
fn room_map(
    stage: &StageAssets,
    pages: &[Vec<(usize, usize)>],
    index: usize,
    catalog: &Catalog,
) -> Map {
    let RoomAssets { room, spawns } = &stage.rooms[index];
    let vertical = room.orientation & orientation::VERTICAL != 0;
    let terrain = terrain_layer(stage, pages, index);
    let (spawn_objects, lock_objects) = schedule_objects(room, spawns, &stage.schedule, vertical);
    let next_object = FIRST_LOCK_ID + u32::try_from(lock_objects.len()).expect("ids fit");
    let start = &room.start;
    let mut markers = vec![
        point(
            PLAYER_START_ID,
            "player-start",
            (start.player_x, start.player_y),
            vec![
                Property::int("facing", start.facing),
                Property::int("scroll_lock", start.scroll_lock),
                Property::int("scroll_mode", start.scroll_mode),
            ],
        ),
        point(
            CAMERA_START_ID,
            "camera-start",
            (start.camera_x, start.camera_y),
            Vec::new(),
        ),
    ];
    markers.extend(lock_objects);
    let tileset = catalog
        .tileset_name(stage.stage.area, room.chr)
        .expect("the catalog has the room's CHR");
    Map {
        height: terrain.height.expect("a tile layer"),
        infinite: false,
        width: terrain.width.expect("a tile layer"),
        layers: vec![
            terrain,
            Layer {
                draworder: Some("index".into()),
                objects: Some(spawn_objects),
                ..layer(2, "spawns", "objectgroup")
            },
            Layer {
                draworder: Some("index".into()),
                objects: Some(markers),
                ..layer(3, "markers", "objectgroup")
            },
        ],
        nextlayerid: 4,
        nextobjectid: next_object,
        orientation: "orthogonal".into(),
        properties: room_properties(room, vertical),
        renderorder: "right-down".into(),
        tiledversion: TILED_VERSION.into(),
        tileheight: 16,
        tilesets: vec![TilesetRef {
            firstgid: 1,
            source: format!("../../../tilesets/{tileset}.tsj"),
            extra: Extra::new(),
        }],
        tilewidth: 16,
        kind: "map".into(),
        version: JSON_VERSION.into(),
        extra: Extra::new(),
    }
}

/// Room `index`'s `terrain` layer: each region with tiles holds its page's metatiles, gid 0
/// past the stream's reach.
fn terrain_layer(stage: &StageAssets, pages: &[Vec<(usize, usize)>], index: usize) -> Layer {
    let room = &stage.rooms[index].room;
    let vertical = room.orientation & orientation::VERTICAL != 0;
    let across = if room.orientation & orientation::TWO_PAGES == 0 {
        1
    } else {
        2
    };
    let regions: Vec<(usize, usize)> = pages
        .iter()
        .enumerate()
        .flat_map(|(page, regions)| {
            regions
                .iter()
                .filter(|&&(room, _)| room == index)
                .map(move |&(_, entry)| (entry, page))
        })
        .filter(|&(entry, _)| entry >= TABLE_ENTRIES || entry >> 4 < across)
        .collect();
    let along = PAGES + regions.iter().filter(|&&(e, _)| e >= TABLE_ENTRIES).count();
    let (width, height) = if vertical {
        (across * 16, along * 16)
    } else {
        (along * 16, across * 16)
    };
    let mut data = vec![0; width * height];
    for &(entry, page) in &regions {
        let Some(cells) = stage
            .terrain
            .stream
            .get(page * PAGE_CELLS..(page + 1) * PAGE_CELLS)
        else {
            continue;
        };
        for (cell, &metatile) in cells.iter().enumerate() {
            data[tile_index(vertical, width, region_place(entry), cell)] = 1 + u32::from(metatile);
        }
    }
    Layer {
        data: Some(data),
        height: Some(u32::try_from(height).expect("a map fits")),
        width: Some(u32::try_from(width).expect("a map fits")),
        ..layer(1, "terrain", "tilelayer")
    }
}

fn room_properties(room: &Room, vertical: bool) -> Vec<Property> {
    let arrival = match room.arrival {
        Arrival::Unchanged => "unchanged",
        Arrival::Held => "held",
        Arrival::Flying => "flying",
    };
    let split = if room.raster.is_some() {
        "standard"
    } else {
        "none"
    };
    let start = &room.start;
    let derived = derived_first_spawn_column(vertical, start.camera_x, start.camera_y, &room.pages);
    let first_spawn_column = (start.first_spawn_column != derived).then(|| {
        let column = (0..TABLE_ENTRIES * 16)
            .find(|&column| {
                schedule_cell(&room.pages, column) == usize::from(start.first_spawn_column)
            })
            .expect("the first spawn column is on the room's pages");
        Property::int(
            "first_spawn_column",
            i64::try_from(column).expect("a column fits"),
        )
    });
    let mut properties = vec![
        Property::enumerated("arrival", "arrival", arrival),
        Property::enumerated(
            "background_palette",
            "background_palette",
            &format!("bg-{:02x}", room.background_palette),
        ),
        Property::enumerated("hud_split", "hud_split", split),
        Property::enumerated(
            "scroll",
            "scroll",
            if vertical { "vertical" } else { "horizontal" },
        ),
        Property::enumerated(
            "sprite_palette",
            "sprite_palette",
            &format!("sprites-{:02x}", room.sprite_palette),
        ),
    ];
    properties.extend(first_spawn_column);
    properties
}

/// The room's spawn objects (one per record a pending cell on its pages names) and its
/// `scroll-lock` markers (one per schedule column holding a lock cell).
fn schedule_objects(
    room: &Room,
    spawns: &SpawnTable,
    schedule: &[u8; PAGE_CELLS],
    vertical: bool,
) -> (Vec<Object>, Vec<Object>) {
    let mut triggers: BTreeMap<u8, Vec<i64>> = BTreeMap::new();
    let mut locks = Vec::new();
    for column in 0..TABLE_ENTRIES * 16 {
        let value = schedule[schedule_cell(&room.pages, column)];
        let column_number = i64::try_from(column).expect("a column fits");
        if value & CELL_PENDING != 0 {
            triggers
                .entry(value & CELL_RECORD)
                .or_default()
                .push(column_number);
        } else if value & CELL_FLAGS == CELL_LOCK {
            let along = u16::try_from(column & 0xFF).expect("a byte") * 256;
            let across = u16::try_from(column >> 8).expect("a band") * 0x1000;
            let (x, y) = along_across(vertical, along, across);
            let id = FIRST_LOCK_ID + u32::try_from(locks.len()).expect("ids fit");
            locks.push(point(
                id,
                "scroll-lock",
                (x, y),
                vec![Property::int("locks", value & CELL_RECORD)],
            ));
        }
    }
    let spawn_objects = triggers
        .into_iter()
        .map(|(id, columns)| {
            let record = spawns.records[usize::from(id)]
                .unwrap_or_else(|| panic!("a read cell names record {id}, which the room lacks"));
            let (class, variant) = class_of(record.second_class, record.kind)
                .unwrap_or_else(|| panic!("record {id}'s kind {:02X} has no class", record.kind));
            let mut properties = vec![
                Property::int("behaviour", record.behaviour),
                Property::int("chr_slot", record.chr_slot),
                Property::int("contact", record.contact),
                Property::int("flags", record.flags),
                Property::int("health", record.health),
                Property::int("state", record.state),
                Property::int("subtype", record.subtype),
                Property::int_list("triggers", columns),
            ];
            if let Some(variant) = variant {
                properties.push(Property::enumerated(
                    "variant",
                    &format!("{class}_variant"),
                    &variant,
                ));
            }
            point(u32::from(id) + 1, class, (record.x, record.y), properties)
        })
        .collect();
    (spawn_objects, locks)
}

fn point(id: u32, class: &str, (x, y): (u16, u16), properties: Vec<Property>) -> Object {
    Object {
        height: 0.into(),
        id,
        name: String::new(),
        point: Some(true),
        properties,
        rotation: 0.into(),
        kind: class.into(),
        visible: true,
        width: 0.into(),
        x: Position(x),
        y: Position(y),
        extra: Extra::new(),
    }
}

fn layer(id: u32, name: &str, kind: &str) -> Layer {
    Layer {
        data: None,
        draworder: None,
        height: None,
        id,
        name: name.into(),
        objects: None,
        opacity: 1.into(),
        kind: kind.into(),
        visible: true,
        width: None,
        x: 0,
        y: 0,
        extra: Extra::new(),
    }
}
