//! Spawning from the schedule: column spawns (bank 2 `$87C5`) and room starts (`$D714`).

use super::families::Brain;
use super::slots::{Placed, Slots, Spawner};
use super::world::{Schedule, View, World};
use crate::actors::{ActorError, ActorSpawn, actor_spawn, spawn_record_pointer, spawn_schedule};

const IDS: u8 = 64;

/// A level room's spawn data as the ROM defines it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnTable {
    /// The initial schedule (copied to `$7410` at level load).
    pub schedule: [u8; 256],
    /// Records by id; `None` where the id's pointer leaves the bank.
    pub records: Vec<Option<ActorSpawn>>,
}

impl SpawnTable {
    /// Decodes the schedule of `area` and the records of `area`/`room` (`$0400`, `$0402`).
    pub fn from_rom(prg: &[u8], area: u8, room: u8) -> Result<SpawnTable, ActorError> {
        let schedule = spawn_schedule(prg, area)?
            .try_into()
            .expect("the schedule is 256 bytes");
        let records = (0..IDS)
            .map(|id| {
                spawn_record_pointer(prg, area, room, id)
                    .and_then(|addr| actor_spawn(prg, addr))
                    .ok()
            })
            .collect();
        Ok(SpawnTable { schedule, records })
    }

    fn record(&self, cell_value: u8) -> ActorSpawn {
        let id = cell_value & (IDS - 1);
        self.records
            .get(usize::from(id))
            .copied()
            .flatten()
            .unwrap_or_else(|| panic!("schedule names record {id}, which the ROM lacks"))
    }
}

impl View {
    /// The schedule cell of the 16 px column at the leading edge, if the row table has one.
    #[must_use]
    pub fn column_cell(&self, camera_x: u16, schedule: &Schedule, right: bool) -> Option<u8> {
        let left = i32::from(camera_x);
        let edge = if right { left + self.width } else { left };
        let column = edge >> 8;
        let row = *schedule.rows.get(usize::try_from(column >> 4).ok()?)?;
        Some(row | u8::try_from(column & 0x0F).expect("a nibble"))
    }
}

/// The scroll locks after the left-edge cell is read (`$87C5-$87E8`).
#[must_use]
pub fn scroll_lock(schedule: &Schedule, world: &World, mask: u8, locks: u8) -> u8 {
    let Some(cell) = world.view.column_cell(world.camera_x, schedule, false) else {
        return locks;
    };
    lock_at(schedule, cell, mask, 0x0C, locks)
}

/// The scroll locks after reading `cell`: when it is a lock, the `keep` bits of `locks`
/// plus its `mask` bits (`$87D2`, `$8786`).
#[must_use]
pub fn lock_at(schedule: &Schedule, cell: u8, mask: u8, keep: u8, locks: u8) -> u8 {
    let value = schedule.cells[usize::from(cell)];
    if value & 0xC0 == 0x40 {
        (locks & keep) | (value & mask)
    } else {
        locks
    }
}

/// Spawns the leading-edge cell when it is pending (`$87EA-$88FE`).
pub fn spawn_column(
    slots: &mut Slots,
    schedule: &mut Schedule,
    table: &SpawnTable,
    world: &World,
    right: bool,
) {
    let Some(cell) = world.view.column_cell(world.camera_x, schedule, right) else {
        return;
    };
    spawn_at(slots, schedule, table, cell);
}

/// Spawns `cell` when it is pending (`$87FC-$88FE`).
pub fn spawn_at(slots: &mut Slots, schedule: &mut Schedule, table: &SpawnTable, cell: u8) {
    let value = schedule.cells[usize::from(cell)];
    if value & 0x80 == 0 {
        return;
    }
    let record = table.record(value);
    let Some(free) = free_slot(slots, &record) else {
        assert!(
            cell < 0x80,
            "cell {cell:02X} with no free slot takes the unported $89AF path"
        );
        return;
    };
    place(slots, free, &record, cell, Spawner::Column);
    schedule.cells[usize::from(cell)] &= 0x3F;
}

/// Spawns every pending cell of the 16 from `start` (`$D74B-$D75E`).
pub fn spawn_row(slots: &mut Slots, schedule: &mut Schedule, table: &SpawnTable, start: u8) {
    for cell in (0..16).map(|i| start.wrapping_add(i)) {
        let value = schedule.cells[usize::from(cell)];
        if value & 0x80 == 0 {
            continue;
        }
        let record = table.record(value);
        let free = free_slot(slots, &record)
            .unwrap_or_else(|| panic!("room spawn of cell {cell:02X} found no free slot"));
        place(slots, free, &record, cell, Spawner::Room);
        schedule.cells[usize::from(cell)] &= 0x3F;
    }
}

#[derive(Clone, Copy)]
enum Free {
    Main(usize),
    Second(usize),
}

/// The first free slot of the record's class (`$8992`, `$D830`).
fn free_slot(slots: &Slots, record: &ActorSpawn) -> Option<Free> {
    if record.slot_bank() == 0 {
        slots.main[..16]
            .iter()
            .position(|s| s.kind == 0)
            .map(Free::Main)
    } else {
        slots
            .second
            .iter()
            .position(|s| s.kind == 0)
            .map(Free::Second)
    }
}

fn place(slots: &mut Slots, free: Free, record: &ActorSpawn, cell: u8, by: Spawner) {
    let kind = record.slot_type();
    match free {
        Free::Main(i) => {
            let slot = &mut slots.main[i];
            slot.kind = kind;
            slot.body.spawn(record, cell);
            slot.brain = Brain::new(kind);
        }
        Free::Second(i) => {
            let at = |array: u16| array + 24 + u16::try_from(i).expect("< 8");
            let r = record.raw;
            for (array, value) in [
                (0x7B00, r[0]),
                (0x7B20, r[1]),
                (0x7B40, r[2]),
                (0x7B60, r[3]),
                (0x7C00, r[4] & 0xE3),
                (0x7C90, (r[4] >> 2) & 7),
                (0x7CC0, kind),
                (0x7D50, r[6]),
                (0x7D90, r[7]),
                (0x7DB0, r[8]),
                (0x7D70, 0),
                (0x7DD0, 0),
                (0x7DF0, 0),
                (0x7E10, 0),
                (0x7E90, 0),
                (0x7C30, 0),
                (0x7BD8, r[9]),
                (0x7BE0, r[10]),
                (0x7C30, r[11]),
                (0x7EF8, 0),
            ] {
                slots.set_byte(at(array), value);
            }
            if by == Spawner::Column {
                slots.set_byte(at(0x7F00), 0);
            }
            slots.set_byte(at(0x7CE0), cell);
            let second = &mut slots.second[i];
            second.placed = Some(Placed {
                cell,
                record: *record,
                by,
            });
            second.brain = Brain::new_second(kind);
        }
    }
}

#[cfg(test)]
mod tests {
    use rotj_oracle::PeekEvent;

    use super::super::oracle::{Bracket, Scene, State, brackets, diff};
    use super::super::slots::Body;
    use super::*;

    const COLUMN: u16 = 0x87C5;
    const COLUMN_RETURNS: [u16; 2] = [0x8668, 0x86AB];
    const ROOM: (u16, u16) = (0xD714, 0xD75E);

    fn green() -> ActorSpawn {
        ActorSpawn {
            addr: 0xACB8,
            raw: [
                0x1D, 0xFF, 0x05, 0xFF, 0x01, 0x05, 0x20, 0x00, 0x05, 0x00, 0xFF, 0x88,
            ],
        }
    }

    fn table(prg: &[u8], ram: &[u8; 2048]) -> SpawnTable {
        SpawnTable::from_rom(prg, ram[0x400], ram[0x402]).expect("spawn table")
    }

    /// Diffs every column-spawn call in `events`; returns (calls, main spawns, second-class spawns).
    fn check_columns(events: &[PeekEvent], prg: &[u8]) -> (usize, usize, usize) {
        let pairs = brackets(events, COLUMN, &COLUMN_RETURNS);
        let (mut main, mut second) = (0, 0);
        for Bracket { begin, end } in &pairs {
            assert_eq!(begin.bank, Some(2));
            let mut state = State::lift(begin);
            let right = begin.ram[0xC4] & 0x80 == 0;
            let locks = scroll_lock(
                &state.schedule,
                &state.world,
                begin.ram[0x20],
                begin.ram[0xC2],
            );
            let before = state.slots.clone();
            let table = table(prg, &begin.ram);
            spawn_column(
                &mut state.slots,
                &mut state.schedule,
                &table,
                &state.world,
                right,
            );
            main += usize::from(state.slots.main != before.main);
            second += usize::from(state.slots.second != before.second);
            let (mut ram, wram) = state.project(begin);
            ram[0xC2] = locks;
            if let Some((addr, port, oracle)) = diff(begin, (&ram, &wram), end) {
                panic!(
                    "frame {}: ${addr:04X} port {port:02X} oracle {oracle:02X}",
                    begin.frame
                );
            }
        }
        (pairs.len(), main, second)
    }

    #[test]
    fn column_spawns_match_the_oracle() {
        let Some(mut scene) = Scene::route(640) else {
            return;
        };
        let prg = scene.prg();
        scene.machine.bus.trace.exec.push(0x89B5..=0x89B5);
        let events = scene.run(359, &[COLUMN, COLUMN_RETURNS[0], COLUMN_RETURNS[1]]);
        let (calls, main, _) = check_columns(&events, &prg);
        assert!(calls > 10 && main >= 3, "{calls} calls, {main} spawns");
        assert!(scene.machine.bus.trace.execs.is_empty(), "$89AF path taken");
    }

    #[test]
    fn second_class_column_spawn_matches_the_oracle() {
        let Some(mut scene) = Scene::level(2, 650) else {
            return;
        };
        let prg = scene.prg();
        let events = scene.play(40, 0x80, &[COLUMN, COLUMN_RETURNS[0], COLUMN_RETURNS[1]]);
        let (calls, _, second) = check_columns(&events, &prg);
        assert!(second >= 1, "{calls} calls, {second} second-class spawns");
    }

    #[test]
    fn room_spawn_matches_the_oracle() {
        let Some(mut scene) = Scene::route(599) else {
            return;
        };
        let prg = scene.prg();
        let events = scene.run(41, &[ROOM.0, ROOM.1]);
        let pairs = brackets(&events, ROOM.0, &[ROOM.1]);
        assert_eq!(pairs.len(), 1);
        let Bracket { begin, end } = &pairs[0];
        assert_ne!(
            begin.ram[0xDD] & 0xC0,
            0x80,
            "the camera-row start is not ported"
        );
        let mut state = State::lift(begin);
        let table = table(&prg, &begin.ram);
        assert_eq!(table.schedule[..], begin.wram[0x1410..0x1510]);
        spawn_row(
            &mut state.slots,
            &mut state.schedule,
            &table,
            begin.ram[0x403],
        );
        assert!(state.slots.main[..3].iter().all(|s| s.kind != 0));
        let (ram, wram) = state.project(begin);
        if let Some((addr, port, oracle)) = diff(begin, (&ram, &wram), end) {
            panic!(
                "frame {}: ${addr:04X} port {port:02X} oracle {oracle:02X}",
                begin.frame
            );
        }
    }

    /// A pending `$0A` record in cell 3 with the camera on column 3 and `taken` slots full.
    fn pending_green(taken: usize) -> (Slots, Schedule, SpawnTable, World) {
        let mut slots = Slots::default();
        slots.main[..taken].iter_mut().for_each(|s| s.kind = 2);
        let mut schedule = Schedule {
            cells: [0; 256],
            rows: [0; 32],
        };
        schedule.cells[0x03] = 0x80;
        let table = SpawnTable {
            schedule: schedule.cells,
            records: vec![Some(green())],
        };
        (slots, schedule, table, World::at(0x0300))
    }

    #[test]
    fn spawn_keeps_residue() {
        let (mut slots, mut schedule, table, world) = pending_green(3);
        slots.main[3].body = Body::lift(&[0x5A; 8192], 3);
        let mut before = Box::new([0x5A; 8192]);
        slots.project(&mut before);
        spawn_column(&mut slots, &mut schedule, &table, &world, false);
        assert_eq!(slots.main[3].kind, 0x0A);
        let mut after = before.clone();
        slots.project(&mut after);
        let written: Vec<u16> = [
            0x7B00, 0x7B20, 0x7B40, 0x7B60, 0x7C00, 0x7C90, 0x7CC0, 0x7D50, 0x7D90, 0x7DB0, 0x7D70,
            0x7DD0, 0x7DF0, 0x7E10, 0x7E90, 0x7C30, 0x7E30, 0x7E40, 0x7E50, 0x7E60, 0x7E70, 0x7E80,
            0x7BA0, 0x7EC0, 0x7ED0, 0x7EF0, 0x7BE0, 0x7EE0, 0x7CE0,
        ]
        .into_iter()
        .map(|a| a + 3)
        .collect();
        for i in 0..8192u16 {
            let addr = 0x6000 + i;
            if before[usize::from(i)] != after[usize::from(i)] {
                assert!(written.contains(&addr), "${addr:04X} changed");
            }
        }
        assert_eq!(schedule.cells[0x03], 0x00);
    }

    #[test]
    fn full_slots_leave_the_cell_pending() {
        let (mut slots, mut schedule, table, world) = pending_green(16);
        let before = slots.clone();
        spawn_column(&mut slots, &mut schedule, &table, &world, false);
        assert_eq!(slots, before);
        assert_eq!(schedule.cells[0x03], 0x80);
    }
}
