use std::collections::BTreeSet;

use super::model::{
    Extra, Layer, Map, Object, PackFile, Position, Property, Region, World, to_text,
};
use super::*;
use crate::actor::slots::Slots;
use crate::actor::spawn::{lock_at, spawn_at, spawn_row};
use crate::actor::stage::{Music, OnClear};
use crate::actor::world::{Schedule, orientation};
use crate::actors::{
    CELL_FLAGS, CELL_LOCK, CELL_PENDING, CELL_RECORD, SpawnRecord, spawn_schedule,
};
use crate::collision::CLASS_COUNT;
use crate::render::Mirroring;
use crate::rooms::{Arrival, Start, listed_rooms};

/// The ids a room's read cells name: the pending cells on the pages of its table.
fn read_ids(room: &Room, schedule: &[u8; PAGE_CELLS]) -> BTreeSet<usize> {
    (0..TABLE_ENTRIES * 16)
        .map(|column| schedule[schedule_cell(&room.pages, column)])
        .filter(|&value| value & CELL_PENDING != 0)
        .map(|value| usize::from(value & CELL_RECORD))
        .collect()
}

/// The rooms whose ROM first spawn column is not the derived one (area 11 rooms 2 and 3).
const FIRST_SPAWN_COLUMN_ROOMS: [&str; 2] = ["rooms/5-1-3.tmj", "rooms/5-1-4.tmj"];

/// The read pending cells that also set bit 6 (`$C0 | id`, all in 7-2), which the format drops.
const BIT_6_CELLS: usize = 24;

/// The schedule cells [`Levels::original`]'s ROM overlay changes: the residue the format drops
/// and the bit 6 cells.
const RESTORED_CELLS: usize = 226;

/// The page-table actors' kinds and the page each writes into the table at runtime.
const PAGE_TABLE_WRITERS: [(u8, u8); 4] = [(0x9C, 6), (0x76, 7), (0xC8, 0), (0xB2, 0)];

/// The equivalence: what the game reads of `import(export(rom))` is today's assets. Failures
/// name the place, never the ROM's bytes. The read records are 590, not the survey's 613:
/// those count every id a pending cell anywhere in the area's schedule names. The metatile
/// tables, CHR and screen split come back from the catalog, itself read from the same ROM, so
/// for those the test proves the tileset names and `hud_split` round trip, not a derivation.
#[test]
fn the_original_game_imports_to_the_assets_the_game_reads() {
    let Some(rom) = rotj_rom::for_test() else {
        return;
    };
    let levels = Levels::from_rom(&rom.prg, &rom.chr).expect("the ROM's levels");
    let catalog = Catalog::from_rom(&rom.prg, &rom.chr).expect("the ROM's catalog");
    let files = write(&levels, &catalog);
    let imported = import(&files, &catalog, Source::Original).expect("the export imports");
    assert_eq!(imported.stages.len(), levels.stages.len());
    let (mut rooms, mut records, mut writers, mut bit_6_cells) = (0, 0, 0, 0);
    assert_eq!(imported.stages.len(), 18);
    for ((index, stage), back) in (0..).zip(&levels.stages).zip(&imported.stages) {
        let at = &stage.stage.name;
        assert!(back.stage == stage.stage, "{at}: stage");
        assert!(
            back.stage.descriptor == Descriptor::from_rom(&rom.prg, index),
            "{at}: descriptor"
        );
        assert!(back.terrain == stage.terrain, "{at}: terrain");
        let shown: BTreeSet<u8> = stage
            .rooms
            .iter()
            .flat_map(|assets| assets.room.pages)
            .collect();
        for (cell, (&value, &imported_value)) in
            stage.schedule.iter().zip(&back.schedule).enumerate()
        {
            let shown = shown.contains(&u8::try_from(cell >> 4).expect("a page"));
            let expected = if shown && value & CELL_PENDING != 0 {
                bit_6_cells += usize::from(value & CELL_FLAGS == CELL_FLAGS);
                CELL_PENDING | (value & CELL_RECORD)
            } else if shown && value & CELL_FLAGS == CELL_LOCK {
                value
            } else {
                0
            };
            assert!(
                imported_value == expected,
                "{at}: schedule cell {cell:#04x}"
            );
        }
        assert_eq!(back.rooms.len(), stage.rooms.len(), "{at}");
        for (index, (assets, back)) in stage.rooms.iter().zip(&back.rooms).enumerate() {
            let file = format!("rooms/{at}-{}.tmj", index + 1);
            let mut layout = back.room.clone();
            if !orientation::horizontal_two_page(assets.room.orientation) {
                layout.raised_page_count = assets.room.raised_page_count;
            }
            let map: Map = serde_json::from_str(&files[&file]).expect("a room");
            assert_eq!(
                map.properties
                    .iter()
                    .any(|property| property.name == "first_spawn_column"),
                FIRST_SPAWN_COLUMN_ROOMS.contains(&file.as_str()),
                "{file}: first_spawn_column"
            );
            assert!(layout == assets.room, "{file}: room");
            let ids = read_ids(&assets.room, &stage.schedule);
            for &id in &ids {
                let record = assets.spawns.records[id];
                assert!(back.spawns.records[id] == record, "{file}: record {id}");
                let record = record.expect("a read record");
                for (kind, page) in PAGE_TABLE_WRITERS {
                    if !record.second_class && record.kind == kind {
                        assert!(assets.room.pages.contains(&page), "{file}: record {id}");
                        writers += 1;
                    }
                }
            }
            rooms += 1;
            records += ids.len();
        }
    }
    let variants: usize = catalog.areas.iter().map(|area| area.variants.len()).sum();
    let pack: PackFile = serde_json::from_str(&files["pack.json"]).expect("pack.json");
    let extra_pages = pack
        .stages
        .iter()
        .flat_map(|stage| stage.pages.iter().flatten())
        .filter(|region| region.entry >= 32)
        .count();
    assert_eq!(
        (rooms, variants, records, extra_pages),
        (36, 27, 590, 6),
        "rooms, variants, read records, extra pages"
    );
    assert!(writers > 0, "no page-table actor was checked");
    assert_eq!(
        bit_6_cells, BIT_6_CELLS,
        "read pending cells with bit 6, which imports as 0"
    );
    assert_eq!(imported.extra_passwords, levels.extra_passwords);
    assert_eq!(levels.extra_passwords.len(), 1);
}

/// The readers are listed in docs/specs/2026-10-24-level-packs-release-2-design.md.
#[test]
fn bit_6_of_a_pending_cell_is_read_by_nothing() {
    let Some(rom) = rotj_rom::for_test() else {
        return;
    };
    let levels = Levels::from_rom(&rom.prg, &rom.chr).expect("the ROM's levels");
    let mut checked = 0;
    for stage in &levels.stages {
        let cells: Vec<u8> = (0..=u8::MAX)
            .filter(|&cell| stage.schedule[usize::from(cell)] & CELL_FLAGS == CELL_FLAGS)
            .collect();
        if cells.is_empty() {
            continue;
        }
        assert!(
            stage.terrain.stream.iter().all(|&id| id < 0xD0),
            "{}: a metatile class_at reads from the schedule",
            stage.stage.name
        );
        for assets in &stage.rooms {
            let schedule = Schedule {
                cells: stage.schedule,
                ..Schedule::default()
            };
            for &cell in cells
                .iter()
                .filter(|&&cell| assets.room.pages.contains(&(cell >> 4)))
            {
                let mut cleared = schedule.clone();
                cleared.cells[usize::from(cell)] &= !0x40;
                let after = |mut schedule: Schedule, row: bool| {
                    let mut slots = Slots::default();
                    if row {
                        spawn_row(&mut slots, &mut schedule, &assets.spawns, cell & 0xF0);
                    } else {
                        spawn_at(&mut slots, &mut schedule, &assets.spawns, cell);
                    }
                    (slots, schedule.cells[usize::from(cell)])
                };
                for row in [false, true] {
                    assert!(
                        after(schedule.clone(), row) == after(cleared.clone(), row),
                        "cell {cell:#04x}"
                    );
                }
                assert_eq!(
                    lock_at(&schedule, cell, 0x0F, 0xF0, 0x55),
                    lock_at(&cleared, cell, 0x0F, 0xF0, 0x55)
                );
                checked += 1;
            }
        }
    }
    assert!(checked >= BIT_6_CELLS, "{checked} bit 6 cells checked");
}

#[test]
fn the_original_game_round_trips_byte_for_byte() {
    let Some(rom) = rotj_rom::for_test() else {
        return;
    };
    let files = export(&rom.prg, &rom.chr).expect("the ROM exports");
    let catalog = Catalog::from_rom(&rom.prg, &rom.chr).expect("the ROM's catalog");
    let imported = import(&files, &catalog, Source::Original).expect("the export imports");
    assert!(
        write(&imported, &catalog) == files,
        "export(import(export(rom))) differs"
    );
    assert_eq!(files.len(), 38);
    for (path, text) in &files {
        let saved = match path.as_str() {
            "pack.json" => to_text(&serde_json::from_str::<PackFile>(text).expect("pack.json")),
            "world.world" => to_text(&serde_json::from_str::<World>(text).expect("world.world")),
            _ => to_text(&serde_json::from_str::<Map>(text).expect("a room")),
        };
        assert!(saved == *text, "{path} does not save back byte-equal");
    }
    let path = "rooms/3-2-1.tmj";
    let mut map: Map = serde_json::from_str(&files[path]).expect("a room");
    map.extra
        .insert("compressionlevel".into(), serde_json::Value::from(-1));
    let mut notes = map.layers[1].clone();
    notes.id = 4;
    notes.name = "notes".into();
    notes.extra.insert("locked".into(), true.into());
    map.layers.push(notes);
    let text = to_text(&map);
    let saved = to_text(&serde_json::from_str::<Map>(&text).expect("the edited room"));
    assert!(saved == text, "unknown fields do not survive a save");
    assert!(text.contains("\"compressionlevel\": -1") && text.contains("\"locked\": true"));
    let mut edited = files.clone();
    edited.insert(path.into(), text);
    assert!(
        import(&edited, &catalog, Source::Original).expect("the edited pack imports") == imported,
        "an unknown field or layer changed the import"
    );
}

/// What the game reads: the export imported, with the ROM's schedules, its stages by
/// `$0400 >> 1` and its rooms by `$0402 >> 1`. The rooms' other exceptions are the
/// equivalence's.
#[test]
fn the_original_levels_are_the_imported_export_with_the_roms_schedules() {
    let Some(rom) = rotj_rom::for_test() else {
        return;
    };
    let levels = Levels::original(&rom.prg, &rom.chr).expect("the original levels");
    let catalog = Catalog::from_rom(&rom.prg, &rom.chr).expect("the ROM's catalog");
    let files = export(&rom.prg, &rom.chr).expect("the ROM exports");
    let mut imported = import(&files, &catalog, Source::Original).expect("the export imports");
    let listed = listed_rooms(&rom.prg);
    let mut restored = 0;
    for ((index, stage), back) in (0u8..).zip(&levels.stages).zip(&mut imported.stages) {
        let (at, area) = (&stage.stage.name, 2 * index);
        assert_eq!(stage.stage.area, index, "{at}: area");
        let schedule = spawn_schedule(&rom.prg, area).expect("the area's schedule");
        assert!(stage.schedule[..] == schedule[..], "{at}: schedule");
        restored += (back.schedule.iter().zip(&stage.schedule))
            .filter(|(imported, rom)| imported != rom)
            .count();
        back.schedule = stage.schedule;
        let rooms = listed.iter().filter(|&&(room_area, _)| room_area == area);
        assert_eq!(rooms.count(), stage.rooms.len(), "{at}: rooms");
        for (room, assets) in (0u8..).step_by(2).zip(&stage.rooms) {
            assert!(listed.contains(&(area, room)), "{at}: room {room}");
            let mut layout = Room::import(&rom.prg, area, room).expect("the room's layout");
            if !orientation::horizontal_two_page(layout.orientation) {
                layout.raised_page_count = assets.room.raised_page_count;
            }
            assert!(assets.room == layout, "{at}: room {room}");
        }
    }
    assert!(
        levels == imported,
        "the levels differ from the import beyond the schedules"
    );
    assert_eq!(restored, RESTORED_CELLS, "cells the overlay restored");
}

const ROOM_A: &str = "rooms/9-9-1.tmj";
const ROOM_B: &str = "rooms/9-9-2.tmj";
const ROOM_C: &str = "rooms/9-9-3.tmj";
const ROOM_D: &str = "rooms/9-9-4.tmj";

fn record(x: u16, second_class: bool, kind: u8) -> SpawnRecord {
    SpawnRecord {
        x,
        y: 0x0B80,
        flags: 0x40,
        chr_slot: 3,
        second_class,
        kind,
        state: 0x61,
        behaviour: 1,
        subtype: 2,
        health: 0x0102,
        contact: 0x88,
    }
}

/// A one-page horizontal room with the camera on its first page.
fn room(pages: [u8; TABLE_ENTRIES], chr: [u8; 4], records: &[(usize, SpawnRecord)]) -> RoomAssets {
    let mut table = vec![None; 64];
    for &(id, record) in records {
        table[id] = Some(record);
    }
    let last = pages[15];
    let trailing = pages[..16].iter().rev().take_while(|&&p| p == last).count();
    RoomAssets {
        room: Room {
            background_palette: 0x0C,
            sprite_palette: 0x03,
            chr,
            orientation: 0,
            raised_page_count: u8::try_from(16 - trailing).expect("a count"),
            pages,
            start: Start {
                player_x: 0x0300,
                player_y: 0x0A00,
                camera_x: 0x0100,
                camera_y: 0,
                ring_y: 0,
                scroll_lock: 1,
                scroll_mode: 2,
                mirroring: Mirroring::SingleLow,
                first_spawn_column: pages[0] << 4,
                facing: 1,
            },
            raster: None,
            fixed_rows: [false; 16],
            fixed_quads: [false; 8],
            arrival: Arrival::Unchanged,
        },
        spawns: SpawnTable { records: table },
    }
}

/// Four rooms of one area sharing page 1 (a two-trigger record in each room's table), a lock
/// on page 0, page 2 shown only below room A (an extra page), page 3 past the stream (gid
/// 0), a vertical room (C) and a two-page room (D), and residue: a non-pending id, a cell on
/// a page no table shows, an unread record.
fn hand_made() -> Levels {
    let mut a_pages = [3; TABLE_ENTRIES];
    a_pages[0] = 0;
    a_pages[1] = 1;
    a_pages[16..].fill(2);
    let mut b_pages = [1; TABLE_ENTRIES];
    b_pages[16..].fill(3);
    let mut c_pages = [1; TABLE_ENTRIES];
    c_pages[0] = 0;
    c_pages[16..].fill(4);
    let mut d_pages = [1; TABLE_ENTRIES];
    d_pages[1..16].fill(3);
    let mut a = room(
        a_pages,
        [1, 2, 3, 4],
        &[
            (5, record(29 * 16 + 15, false, 0x0C)),
            (9, record(0x0900, false, 0x06)),
            (20, record(0, false, 0x0C)),
        ],
    );
    a.room.start.first_spawn_column = 0x13;
    let mut b = room(b_pages, [5, 6, 7, 0x88], &[(5, record(0x1200, true, 0x02))]);
    b.room.raster = Some(Raster {
        armed: true,
        first_irq_cycles: 0x1234,
        handler: 3,
        next_irq: 1,
    });
    b.room.fixed_rows[0] = true;
    b.room.arrival = Arrival::Held;
    let mut c = room(
        c_pages,
        [1, 2, 3, 4],
        &[
            (5, record(0x0800, false, 0x5A)),
            (9, record(0x0400, false, 0x0C)),
        ],
    );
    c.room.orientation = orientation::VERTICAL;
    let start = &mut c.room.start;
    (start.camera_x, start.camera_y, start.ring_y) = (0, 0x1800, 0x1800);
    (start.mirroring, start.first_spawn_column) = (Mirroring::SideBySide, 0x10);
    let mut d = room(d_pages, [1, 2, 3, 4], &[(5, record(0x0280, false, 0x0C))]);
    d.room.orientation = orientation::TWO_PAGES;
    let start = &mut d.room.start;
    (start.camera_y, start.ring_y) = (0x1000, 0x1000);
    (start.mirroring, start.first_spawn_column) = (Mirroring::Stacked, 0x10);
    let mut schedule = [0; PAGE_CELLS];
    schedule[0x12] = CELL_PENDING | 5;
    schedule[0x17] = CELL_PENDING | 5;
    schedule[0x09] = CELL_PENDING | 9;
    schedule[0x04] = CELL_LOCK | 3;
    schedule[0x20] = 0x05;
    schedule[0x51] = CELL_PENDING | 1;
    let stream = (0..3 * PAGE_CELLS)
        .map(|cell| u8::try_from(cell % 251).expect("a byte"))
        .collect();
    Levels {
        stages: vec![StageAssets {
            stage: Stage {
                name: "9-9".into(),
                area: 0,
                descriptor: Descriptor {
                    area: 0,
                    banner: [9, 9],
                    music: Music::Song(0x03),
                    picture: Some(0x05),
                    password: Some(*b"ABCD"),
                    on_clear: OnClear::Ending,
                },
            },
            terrain: Terrain {
                stream,
                corners: vec![[1, 2, 3, 4]; 251],
                classes: [7; CLASS_COUNT],
                palette_classes: vec![2; 251],
            },
            schedule,
            rooms: vec![a, b, c, d],
        }],
        extra_passwords: vec![ExtraPassword {
            letters: *b"WXYZ",
            stage: 0,
        }],
    }
}

#[test]
fn a_hand_made_pack_imports_what_the_game_reads() {
    let levels = hand_made();
    let catalog = Catalog::from_levels(&levels);
    let files = write(&levels, &catalog);
    let imported = import(&files, &catalog, Source::User).expect("imports");
    let mut expected = levels.clone();
    let stage = &mut expected.stages[0];
    stage.schedule[0x20] = 0;
    stage.schedule[0x51] = 0;
    stage.rooms[0].spawns.records[20] = None;
    assert_eq!(imported, expected);
    assert_eq!(write(&imported, &catalog), files);

    let pack: PackFile = serde_json::from_str(&files["pack.json"]).expect("pack.json");
    let page_2: Vec<(&str, u32)> = pack.stages[0].pages[2]
        .iter()
        .map(|region| (region.room.as_str(), region.entry))
        .collect();
    assert_eq!(
        page_2,
        (16..=32).map(|entry| (ROOM_A, entry)).collect::<Vec<_>>()
    );
    let a: Map = serde_json::from_str(&files[ROOM_A]).expect("room A");
    assert_eq!((a.width, a.height), (17 * 16, 16));
    assert_eq!(a.tilesets[0].source, "../../../tilesets/area-01.tsj");
    let data = a.layers[0].data.as_ref().expect("terrain");
    assert_eq!(data[16 * 17 * 16 - 1], 1 + (2 * 256 + 255) % 251);
    assert_eq!(data[2 * 16], 0, "page 3 is past the stream");
    let b: Map = serde_json::from_str(&files[ROOM_B]).expect("room B");
    assert_eq!(b.tilesets[0].source, "../../../tilesets/area-01-b.tsj");
    let c: Map = serde_json::from_str(&files[ROOM_C]).expect("room C");
    assert_eq!((c.width, c.height), (16, 256));
    let lock = &c.layers[2].objects.as_ref().expect("markers")[2];
    assert_eq!(
        (lock.kind.as_str(), lock.x, lock.y),
        ("scroll-lock", Position(0), Position(4 * 256))
    );
    let d: Map = serde_json::from_str(&files[ROOM_D]).expect("room D");
    assert_eq!((d.width, d.height), (256, 32));
    let spawns = a.layers[1].objects.as_ref().expect("spawns");
    assert_eq!(spawns.iter().map(|o| o.id).collect::<Vec<_>>(), [6, 10]);
    let triggers = spawns[0].properties.iter().find(|p| p.name == "triggers");
    assert_eq!(
        triggers.map(|p| &p.value),
        Some(&serde_json::json!([{"type": "int", "value": 18}, {"type": "int", "value": 23}]))
    );
    assert!(files[ROOM_A].contains("\"x\": 29.9375"));
    let first_spawn_column = a.properties.iter().find(|p| p.name == "first_spawn_column");
    assert_eq!(first_spawn_column.map(|p| &p.value), Some(&19.into()));
    assert!(!b.properties.iter().any(|p| p.name == "first_spawn_column"));
    let stage = serde_json::to_value(&pack.stages[0]).expect("a stage");
    assert_eq!(
        (&stage["hud_label"], &stage["music"], &stage["cutscene"]),
        (&"9-9".into(), &"song-03".into(), &"picture-05".into())
    );
    assert_eq!(
        (&stage["password"], &stage["on_clear"]),
        (&"ABCD".into(), &"ending".into())
    );
    assert_eq!(pack.extra_passwords[0].password, "WXYZ");
    assert_eq!(pack.extra_passwords[0].stage, "9-9");
}

/// The hand-made pack's files and catalog, to break one thing at a time.
struct Fixture {
    files: PackFiles,
    catalog: Catalog,
}

impl Fixture {
    fn new(levels: &Levels) -> Fixture {
        let catalog = Catalog::from_levels(levels);
        Fixture {
            files: write(levels, &catalog),
            catalog,
        }
    }

    /// Asserts the pack, after `edit`, is refused with an error that mentions `needle`.
    fn refuses(&self, needle: &str, edit: impl FnOnce(&mut PackFiles)) {
        let mut files = self.files.clone();
        edit(&mut files);
        let Err(error) = import(&files, &self.catalog, Source::User) else {
            panic!("a pack that should fail on `{needle}` imports");
        };
        let error = error.to_string();
        assert!(error.contains(needle), "`{error}` does not say `{needle}`");
    }

    fn refuses_map(&self, needle: &str, path: &str, edit: impl FnOnce(&mut Map)) {
        self.refuses(needle, |files| {
            let mut map: Map = serde_json::from_str(&files[path]).expect("a room");
            edit(&mut map);
            files.insert(path.into(), to_text(&map));
        });
    }

    fn refuses_pack(&self, needle: &str, edit: impl FnOnce(&mut PackFile)) {
        self.refuses(needle, |files| {
            let mut pack: PackFile = serde_json::from_str(&files["pack.json"]).expect("pack.json");
            edit(&mut pack);
            files.insert("pack.json".into(), to_text(&pack));
        });
    }
}

fn objects(map: &mut Map, layer: usize) -> &mut Vec<Object> {
    map.layers[layer].objects.as_mut().expect("an object group")
}

fn tiles(map: &mut Map) -> &mut Vec<u32> {
    map.layers[0].data.as_mut().expect("terrain")
}

fn set_property(object: &mut Object, name: &str, value: serde_json::Value) {
    object.properties.retain(|property| property.name != name);
    object.properties.push(Property {
        value,
        ..Property::int(name, 0)
    });
}

fn region(room: &str, entry: u32) -> Region {
    Region {
        room: room.into(),
        entry,
        extra: Extra::new(),
    }
}

#[test]
fn a_pack_that_breaks_the_pack_file_is_refused() {
    let fixture = Fixture::new(&hand_made());
    fixture.refuses("rooms/9-9-2.tmj: missing", |files| {
        files.remove(ROOM_B);
    });
    fixture.refuses("rooms/9-9-2.tmj: EOF", |files| {
        files.insert(ROOM_B.into(), "{".into());
    });
    fixture.refuses_pack("this release reads format 1", |pack| pack.format = 2);
    fixture.refuses_pack("engine `modern`", |pack| pack.engine = "modern".into());
    fixture.refuses_pack("no rooms", |pack| pack.stages[0].rooms.clear());
    fixture.refuses_pack("15 pages, not 16", |pack| {
        pack.stages[0].pages.pop();
    });
    fixture.refuses_pack("unknown room `rooms/nope.tmj`", |pack| {
        pack.stages[0].pages[5].push(region("rooms/nope.tmj", 0));
    });
    fixture.refuses_pack("listed twice", |pack| {
        pack.stages[0].pages[0].push(region(ROOM_B, 0));
    });
    fixture.refuses_pack("entry 3 is on no page", |pack| {
        pack.stages[0].pages[3]
            .retain(|region| (region.room.as_str(), region.entry) != (ROOM_A, 3));
    });
    fixture.refuses_pack("only the first room holds extra pages", |pack| {
        pack.stages[0].pages[5].push(region(ROOM_B, 32));
    });
    fixture.refuses_pack("extra page 33 is outside the map", |pack| {
        pack.stages[0].pages[5].push(region(ROOM_A, 33));
    });
    fixture.refuses_pack("page 5 has tiles after page 2", |pack| {
        let pages = &mut pack.stages[0].pages;
        pages[2].retain(|region| region.entry != 32);
        pages[5].push(region(ROOM_A, 32));
    });
}

#[test]
fn a_room_that_breaks_the_map_format_is_refused() {
    let fixture = Fixture::new(&hand_made());
    fixture.refuses_map("not a finite orthogonal map", ROOM_A, |map| {
        map.kind = "world".into();
    });
    fixture.refuses_map("not a finite orthogonal map", ROOM_A, |map| {
        map.orientation = "isometric".into();
    });
    fixture.refuses_map("not a finite orthogonal map", ROOM_A, |map| {
        map.infinite = true;
    });
    fixture.refuses_map("not a finite orthogonal map", ROOM_A, |map| {
        map.tilewidth = 8;
    });
    fixture.refuses_map("not a strip of 16 or more whole pages", ROOM_A, |map| {
        map.width = 100;
    });
    fixture.refuses_map("unknown tileset `area-99`", ROOM_A, |map| {
        map.tilesets[0].source = "../../../tilesets/area-99.tsj".into();
    });
    fixture.refuses_map("exactly one tileset", ROOM_A, |map| {
        let second = map.tilesets[0].clone();
        map.tilesets.push(second);
    });
    fixture.refuses_map("no layer `spawns`", ROOM_A, |map| {
        map.layers[1].name = "x".into();
    });
    fixture.refuses_map("layer `spawns` is not an object group", ROOM_A, |map| {
        map.layers[1].objects = None;
    });
    fixture.refuses_map("not a tile layer the map's size", ROOM_A, |map| {
        tiles(map).pop();
    });
    for (name, value) in [
        ("scroll", "diagonal"),
        ("arrival", "warped"),
        ("hud_split", "half"),
    ] {
        fixture.refuses_map(
            &format!("property `{name}`: unknown value"),
            ROOM_A,
            |map| {
                let property = map.properties.iter_mut().find(|p| p.name == name);
                property.expect("a property").value = value.into();
            },
        );
    }
    fixture.refuses_map("property `scroll` is not a string", ROOM_A, |map| {
        let scroll = map.properties.iter_mut().find(|p| p.name == "scroll");
        scroll.expect("a scroll").value = 1.into();
    });
    fixture.refuses_map("firstgid is 1 or more", ROOM_A, |map| {
        map.tilesets[0].firstgid = 0;
    });
    fixture.refuses_map("disagrees", ROOM_B, |map| {
        tiles(map)[5] += 1;
    });
    fixture.refuses_map("flipped", ROOM_A, |map| {
        tiles(map)[0] |= 0x8000_0000;
    });
    fixture.refuses_map("gid 257 is not in the tileset", ROOM_A, |map| {
        tiles(map)[0] = 257;
    });
    fixture.refuses_map("mixes gid 0 and tiles", ROOM_A, |map| {
        tiles(map)[0] = 0;
    });
    for preset in ["bg-c", "bg-+c", "bg-00c", "bg-0C"] {
        fixture.refuses_map("unknown preset", ROOM_A, |map| {
            let palette = map
                .properties
                .iter_mut()
                .find(|p| p.name == "background_palette");
            palette.expect("a palette").value = preset.into();
        });
    }
    fixture.refuses_map("`none` needs arrival `unchanged`", ROOM_A, |map| {
        let arrival = map.properties.iter_mut().find(|p| p.name == "arrival");
        arrival.expect("an arrival").value = "held".into();
    });
}

#[test]
fn a_room_whose_areas_or_splits_do_not_resolve_is_refused() {
    let mut levels = hand_made();
    let mut other = levels.stages[0].clone();
    other.stage.area = 1;
    other.stage.name = "9-8".into();
    other.stage.descriptor.area = 2;
    other.stage.descriptor.password = None;
    for assets in &mut other.rooms {
        (assets.room.raster, assets.room.arrival) = (None, Arrival::Unchanged);
    }
    levels.stages.push(other);
    let fixture = Fixture::new(&levels);
    fixture.refuses_map("its tileset's area is not the stage's", ROOM_B, |map| {
        map.tilesets[0].source = "../../../tilesets/area-02-b.tsj".into();
    });
    fixture.refuses_map("the area has no standard split", "rooms/9-8-1.tmj", |map| {
        let split = map.properties.iter_mut().find(|p| p.name == "hud_split");
        split.expect("a split").value = "standard".into();
    });
}

#[test]
fn a_room_whose_objects_break_the_format_is_refused() {
    let fixture = Fixture::new(&hand_made());
    fixture.refuses_map("is both", ROOM_B, |map| {
        objects(map, 1)[0].id = 7;
    });
    fixture.refuses_map("unknown class `nonsense`", ROOM_A, |map| {
        objects(map, 1)[0].kind = "nonsense".into();
    });
    for variant in ["type-99", "type-6", "type-0x06"] {
        fixture.refuses_map("unknown `breakable_block` variant", ROOM_A, |map| {
            set_property(&mut objects(map, 1)[1], "variant", variant.into());
        });
    }
    fixture.refuses_map("class `breakable_block` needs a `variant`", ROOM_A, |map| {
        objects(map, 1)[1]
            .properties
            .retain(|p| p.name != "variant");
    });
    fixture.refuses_map("property `flags` is not an int in range", ROOM_A, |map| {
        set_property(&mut objects(map, 1)[0], "flags", 256.into());
    });
    fixture.refuses_map("property `triggers` is not a list", ROOM_A, |map| {
        set_property(&mut objects(map, 1)[0], "triggers", 18.into());
    });
    fixture.refuses_map("the id is used twice", ROOM_A, |map| {
        objects(map, 1)[1].id = 6;
    });
    for id in [0, 65] {
        fixture.refuses_map("an actor's id is 1-64", ROOM_A, |map| {
            objects(map, 1)[0].id = id;
        });
    }
    fixture.refuses_map("is not a column 0-511", ROOM_A, |map| {
        let column = serde_json::json!([{"type": "int", "value": 512}]);
        set_property(&mut objects(map, 1)[0], "triggers", column);
    });
    for class in ["player-start", "camera-start"] {
        fixture.refuses_map(&format!("no `{class}`"), ROOM_A, |map| {
            objects(map, 2).retain(|object| object.kind != class);
        });
        fixture.refuses_map(&format!("more than one `{class}`"), ROOM_A, |map| {
            let markers = objects(map, 2);
            let copy = markers.iter().find(|object| object.kind == class).cloned();
            markers.push(copy.expect("a marker"));
        });
    }
    fixture.refuses_map("past the second band", ROOM_A, |map| {
        objects(map, 2)[2].y = Position(0x2000);
    });
    fixture.refuses_map("more than six bits", ROOM_A, |map| {
        set_property(&mut objects(map, 2)[2], "locks", 64.into());
    });
}

#[test]
fn unknown_layers_are_kept_and_ignored() {
    let levels = hand_made();
    let catalog = Catalog::from_levels(&levels);
    let mut files = write(&levels, &catalog);
    let imported = import(&files, &catalog, Source::User).expect("imports");
    let mut map: Map = serde_json::from_str(&files[ROOM_A]).expect("room A");
    let sky = Layer {
        name: "sky".into(),
        id: 4,
        ..map.layers[0].clone()
    };
    map.layers.insert(0, sky);
    files.insert(ROOM_A.into(), to_text(&map));
    assert!(files[ROOM_A].contains("\"sky\""));
    assert_eq!(
        import(&files, &catalog, Source::User).expect("imports"),
        imported
    );
}

#[test]
fn a_stage_whose_knobs_break_the_format_is_refused() {
    let fixture = Fixture::new(&hand_made());
    fixture.refuses_pack("stage `9-9`: `hud_label`: unknown value `10-1`", |pack| {
        pack.stages[0].hud_label = "10-1".into();
    });
    fixture.refuses_pack("`music`: unknown value `song-80`", |pack| {
        pack.stages[0].music = "song-80".into();
    });
    fixture.refuses_pack("`music`: unknown value `song-3`", |pack| {
        pack.stages[0].music = "song-3".into();
    });
    fixture.refuses_pack("`cutscene`: unknown value `picture-zz`", |pack| {
        pack.stages[0].cutscene = Some("picture-zz".into());
    });
    fixture.refuses_pack("`password`: unknown value `abcd`", |pack| {
        pack.stages[0].password = Some("abcd".into());
    });
    fixture.refuses_pack("`password`: unknown value `ABCDE`", |pack| {
        pack.stages[0].password = Some("ABCDE".into());
    });
    fixture.refuses_pack("`on_clear`: unknown value `restart`", |pack| {
        pack.stages[0].on_clear = "restart".into();
    });
    fixture.refuses("missing field `music`", |files| {
        let mut pack: serde_json::Value = serde_json::from_str(&files["pack.json"]).expect("json");
        pack["stages"][0]
            .as_object_mut()
            .expect("a stage")
            .remove("music");
        files.insert("pack.json".into(), to_text(&pack));
    });
    fixture.refuses_pack("the first stage needs a `password`", |pack| {
        pack.stages[0].password = None;
        pack.extra_passwords.clear();
    });
    fixture.refuses_pack("the last stage's `on_clear` is `next-stage`", |pack| {
        pack.stages[0].on_clear = "next-stage".into();
    });
    fixture.refuses_pack("password `ABCD` is used twice", |pack| {
        pack.extra_passwords[0].password = "ABCD".into();
    });
    fixture.refuses_pack("password `ABCD` is used twice", |pack| {
        let twin = pack.stages[0].clone();
        pack.stages.push(twin);
        pack.extra_passwords.clear();
    });
    fixture.refuses_pack("`extra_passwords`: unknown password `wxyz`", |pack| {
        pack.extra_passwords[0].password = "wxyz".into();
    });
    fixture.refuses_pack("`extra_passwords`: unknown stage `9-7`", |pack| {
        pack.extra_passwords[0].stage = "9-7".into();
    });
    fixture.refuses_pack("stage `9-9` names more than one stage", |pack| {
        let twin = pack.stages[0].clone();
        pack.stages.push(twin);
    });
    fixture.refuses_pack("stage `9-7` is not a checkpoint", |pack| {
        let mut other = pack.stages[0].clone();
        (other.name, other.password) = ("9-7".into(), None);
        pack.stages.push(other);
        pack.extra_passwords[0].stage = "9-7".into();
    });
    fixture.refuses_map(
        "property `first_spawn_column` 512 is not a column 0-511",
        ROOM_A,
        |map| {
            let column = map
                .properties
                .iter_mut()
                .find(|p| p.name == "first_spawn_column");
            column.expect("a first spawn column").value = 512.into();
        },
    );
}

#[test]
fn a_user_pack_naming_an_actor_the_game_has_no_handler_for_is_refused() {
    let fixture = Fixture::new(&hand_made());
    fixture.refuses_map("object 6: the game has no handler for it", ROOM_B, |map| {
        let object = &mut objects(map, 1)[0];
        object.kind = "no_handler".into();
        set_property(object, "variant", "type-4a".into());
    });
    let Some(rom) = rotj_rom::for_test() else {
        return;
    };
    let files = export(&rom.prg, &rom.chr).expect("the ROM exports");
    let catalog = Catalog::from_rom(&rom.prg, &rom.chr).expect("the ROM's catalog");
    assert!(files.values().any(|text| text.contains("\"no_handler\"")));
    let error = import(&files, &catalog, Source::User).expect_err("a user pack");
    assert!(
        error.to_string().contains("the game has no handler for it"),
        "{error}"
    );
    assert!(import(&files, &catalog, Source::Original).is_ok());
}
