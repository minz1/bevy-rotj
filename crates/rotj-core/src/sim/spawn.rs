//! Spawning from the schedule: column spawns (bank 2 `$87C5`), room starts (`$D714`),
//! and the bank-3 `$A000` actor spawn records.

use thiserror::Error;

use super::families::{Brain, SecondBrain};
use super::oam::attr;
use super::slots::{Field, Slots, Spawner};
use super::world::Schedule;
#[cfg(test)]
use super::world::{View, World};

/// FME-7 PRG bank holding the actor spawn tables.
pub const SPAWN_BANK: u8 = 0x03;

/// CPU window the actor spawn tables live in for [`SPAWN_BANK`].
pub const SPAWN_WINDOW: u16 = 0xA000;

/// CPU `$A000`: 16-bit LE table indexed by `$0400` whose entries point at
/// per-area tables of stage pointers.
pub const SPAWN_TABLE: u16 = 0xA000;

/// CPU `$A2D4`: 16-bit LE table indexed by `$0400` whose entries point at
/// 256-byte spawn-schedule blocks copied to RAM `$7410` (`$D854`).
pub const SPAWN_SCHEDULE: u16 = 0xA2D4;

/// Bytes of one actor spawn record.
pub const SPAWN_RECORD_BYTES: usize = 12;

/// Bytes of a spawn-schedule block.
pub const SPAWN_SCHEDULE_BYTES: usize = 256;

/// A schedule cell's bit 7: its record is pending a spawn.
pub const CELL_PENDING: u8 = 0x80;

/// A schedule cell's two flag bits.
pub const CELL_FLAGS: u8 = 0xC0;

/// A schedule cell whose flags are this is a scroll lock (`$87D2`).
pub const CELL_LOCK: u8 = 0x40;

/// A schedule cell's record id; masking with it clears both flags.
pub const CELL_RECORD: u8 = 0x3F;

/// `$7D50` bit 7: the actor leaves a pickup when it dies, the low seven bits its
/// weapon (bank 13 `$A5C3`, `engine::dying`: bit clear frees the slot instead).
pub const STATE_DROPS_PICKUP: u8 = 0x80;

/// `$7D50` bit 6: the actor is never chosen as a target, and a homing lock on it drops (unlike
/// [`contact::UNTARGETED`](super::slots::contact::UNTARGETED)). Bank 8 `$8D33` sets it
/// (`ORA #$40`); the target finders test it with `LDA $7D50,y; ASL; BMI` (bank 5
/// `$8417`/`$8430`, bank 4 `$9F2C`/`$9F54`).
pub const STATE_UNTARGETED: u8 = 0x40;

/// ROM actor data could not be read.
#[derive(Debug, Error)]
pub enum ActorError {
    /// A bank/offset pair pointed past the end of `prg`.
    #[error("bank {bank:02X} offset ${offset:04X} is outside PRG ({len} bytes)")]
    OutOfRange {
        /// FME-7 PRG bank number.
        bank: u8,
        /// Offset inside that bank.
        offset: u16,
        /// Length of the PRG slice that was searched.
        len: usize,
    },
}

/// The `bank`/`offset` slice of `prg`, or [`ActorError::OutOfRange`].
pub(crate) fn bank_slice(prg: &[u8], bank: u8, offset: u16) -> Result<&[u8], ActorError> {
    crate::prg::from_bank(prg, usize::from(bank), usize::from(offset)).ok_or(
        ActorError::OutOfRange {
            bank,
            offset,
            len: prg.len(),
        },
    )
}

/// Read `len` bytes at a CPU address inside `bank`'s window.
fn bytes_at(prg: &[u8], bank: u8, window: u16, addr: u16, len: usize) -> Result<&[u8], ActorError> {
    let offset = addr.wrapping_sub(window);
    let slice = bank_slice(prg, bank, offset)?;
    slice.get(..len).ok_or(ActorError::OutOfRange {
        bank,
        offset,
        len: prg.len(),
    })
}

/// Read the 16-bit little-endian word at a CPU address inside `bank`'s window.
fn word_at(prg: &[u8], bank: u8, window: u16, addr: u16) -> Result<u16, ActorError> {
    let bytes = bytes_at(prg, bank, window, addr, 2)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

/// One bank-3 actor spawn record: what the spawners write into a free slot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpawnRecord {
    /// Level x (`$7B00`/`$7B20`), bytes 0-1.
    pub x: u16,
    /// Level y (`$7B40`/`$7B60`), bytes 2-3.
    pub y: u16,
    /// Byte 4 without the CHR slot (`$7C00`).
    pub flags: u8,
    /// Byte 4's bits 2-4 (`$7C90`).
    pub chr_slot: u8,
    /// Byte 5's bit 7: the actor takes a second-class slot (24-31).
    pub second_class: bool,
    /// The occupant type (`$7CC0`): byte 5's low seven bits, shifted left.
    pub kind: u8,
    /// Byte 6 (`$7D50`).
    pub state: u8,
    /// Byte 7 (`$7D90`), a knock-back velocity selector, not a handler index.
    pub behaviour: u8,
    /// Byte 8 (`$7DB0`).
    pub subtype: u8,
    /// Bytes 9-10: hit points in slots 0-15 (`$7EF0`/`$7BE0`); a second-class slot
    /// writes them to `$7BD8`/`$7BE0` instead.
    pub health: u16,
    /// Byte 11: the contact flags in slots 0-15 (`$7EE0`); `$7C30` in a second-class slot.
    pub contact: u8,
}

impl From<[u8; SPAWN_RECORD_BYTES]> for SpawnRecord {
    fn from(bytes: [u8; SPAWN_RECORD_BYTES]) -> SpawnRecord {
        SpawnRecord {
            x: u16::from_be_bytes([bytes[0], bytes[1]]),
            y: u16::from_be_bytes([bytes[2], bytes[3]]),
            flags: bytes[4] & (attr::FLIP_Y | attr::FLIP_X | attr::BEHIND | attr::PALETTE),
            chr_slot: (bytes[4] >> 2) & 7,
            second_class: bytes[5] & 0x80 != 0,
            kind: (bytes[5] & 0x7F) << 1,
            state: bytes[6],
            behaviour: bytes[7],
            subtype: bytes[8],
            health: u16::from_be_bytes([bytes[9], bytes[10]]),
            contact: bytes[11],
        }
    }
}

/// Read the spawn record at CPU `addr` inside `SPAWN_BANK`.
///
/// # Errors
/// Returns [`ActorError::OutOfRange`] if `addr` escapes `prg`.
pub fn spawn_record(prg: &[u8], addr: u16) -> Result<SpawnRecord, ActorError> {
    let bytes = bytes_at(prg, SPAWN_BANK, SPAWN_WINDOW, addr, SPAWN_RECORD_BYTES)?;
    Ok(SpawnRecord::from(
        <[u8; SPAWN_RECORD_BYTES]>::try_from(bytes).expect("length checked above"),
    ))
}

/// Follow [`SPAWN_TABLE`] for `area`/`stage` (RAM `$0400`/`$0402` byte offsets)
/// to the per-stage table of record pointers indexed by spawn id.
///
/// # Errors
/// Returns [`ActorError::OutOfRange`] if any pointer escapes `prg`.
pub fn spawn_record_table(prg: &[u8], area: u8, stage: u8) -> Result<u16, ActorError> {
    let area_table = word_at(prg, SPAWN_BANK, SPAWN_WINDOW, SPAWN_TABLE + u16::from(area))?;
    word_at(prg, SPAWN_BANK, SPAWN_WINDOW, area_table + u16::from(stage))
}

/// Read the spawn id -> record-pointer table entry for `id`.
///
/// # Errors
/// Returns [`ActorError::OutOfRange`] if any pointer escapes `prg`.
pub fn spawn_record_pointer(prg: &[u8], area: u8, stage: u8, id: u8) -> Result<u16, ActorError> {
    let table = spawn_record_table(prg, area, stage)?;
    word_at(prg, SPAWN_BANK, SPAWN_WINDOW, table + u16::from(id) * 2)
}

/// Copy the 256-byte spawn schedule for `area` (RAM `$7410`, loaded by `$D854`).
///
/// # Errors
/// Returns [`ActorError::OutOfRange`] if the schedule pointer or block escapes `prg`.
pub fn spawn_schedule(prg: &[u8], area: u8) -> Result<Vec<u8>, ActorError> {
    let addr = word_at(
        prg,
        SPAWN_BANK,
        SPAWN_WINDOW,
        SPAWN_SCHEDULE + u16::from(area),
    )?;
    Ok(bytes_at(prg, SPAWN_BANK, SPAWN_WINDOW, addr, SPAWN_SCHEDULE_BYTES)?.to_vec())
}

const IDS: u8 = CELL_RECORD + 1;

/// A level room's spawn records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnTable {
    /// Records by id; `None` where the id's pointer leaves the bank.
    pub records: Vec<Option<SpawnRecord>>,
}

impl SpawnTable {
    /// The records of `area`/`room` (`$0400`, `$0402`), imported from the ROM.
    #[must_use]
    pub fn import(prg: &[u8], area: u8, room: u8) -> SpawnTable {
        let records = (0..IDS)
            .map(|id| {
                spawn_record_pointer(prg, area, room, id)
                    .and_then(|addr| spawn_record(prg, addr))
                    .ok()
            })
            .collect();
        SpawnTable { records }
    }

    fn record(&self, cell_value: u8) -> SpawnRecord {
        let id = cell_value & CELL_RECORD;
        self.records
            .get(usize::from(id))
            .copied()
            .flatten()
            .unwrap_or_else(|| panic!("schedule names record {id}, which the ROM lacks"))
    }
}

#[cfg(test)]
impl View {
    /// The schedule cell of the 16 px column at the leading edge, if the page table has one.
    #[must_use]
    pub fn column_cell(self, camera_x: u16, schedule: &Schedule, right: bool) -> Option<u8> {
        let left = i32::from(camera_x);
        let edge = if right { left + self.width } else { left };
        let column = edge >> 8;
        let page_column = *schedule
            .page_columns
            .get(usize::try_from(column >> 4).ok()?)?;
        Some(page_column | u8::try_from(column & 0x0F).expect("a nibble"))
    }
}

/// The scroll locks after the left-edge cell is read (`$87C5-$87E8`).
#[cfg(test)]
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
    if value & CELL_FLAGS == CELL_LOCK {
        (locks & keep) | (value & mask)
    } else {
        locks
    }
}

/// Spawns the leading-edge cell when it is pending (`$87EA-$88FE`).
#[cfg(test)]
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
    if value & CELL_PENDING == 0 {
        return;
    }
    let record = table.record(value);
    let Some(free) = free_slot(slots, &record) else {
        // On cells >= $80 with no free slot the ROM calls `$89AF`, which jumps to
        // zero-page `$0000`; the original game never reaches it, and skipping the
        // spawn matches `cell < $80` safely for user packs.
        return;
    };
    place(slots, free, &record, cell, Spawner::Column);
    schedule.cells[usize::from(cell)] &= CELL_RECORD;
}

/// Spawns every pending cell of the 16 from `start` (`$D74B-$D75E`).
pub fn spawn_row(slots: &mut Slots, schedule: &mut Schedule, table: &SpawnTable, start: u8) {
    for cell in (0..16).map(|i| start.wrapping_add(i)) {
        let value = schedule.cells[usize::from(cell)];
        if value & CELL_PENDING == 0 {
            continue;
        }
        let record = table.record(value);
        let free = free_slot(slots, &record)
            .unwrap_or_else(|| panic!("room spawn of cell {cell:02X} found no free slot"));
        place(slots, free, &record, cell, Spawner::Room);
        schedule.cells[usize::from(cell)] &= CELL_RECORD;
    }
}

#[derive(Clone, Copy)]
enum Free {
    Main(usize),
    Second(usize),
}

/// The first free slot of the record's class (`$8992`, `$D830`).
fn free_slot(slots: &Slots, record: &SpawnRecord) -> Option<Free> {
    if record.second_class {
        slots
            .second
            .iter()
            .position(super::slots::Second::is_free)
            .map(Free::Second)
    } else {
        slots.main[..16]
            .iter()
            .position(super::slots::Slot::is_free)
            .map(Free::Main)
    }
}

fn place(slots: &mut Slots, free: Free, record: &SpawnRecord, cell: u8, by: Spawner) {
    let kind = record.kind;
    match free {
        Free::Main(i) => {
            let slot = &mut slots.main[i];
            slot.kind = kind;
            slot.body.spawn(record, cell);
            slot.brain = Brain::new(kind);
        }
        Free::Second(i) => {
            let slot = 24 + i;
            let [x_high, x_low] = record.x.to_be_bytes();
            let [y_high, y_low] = record.y.to_be_bytes();
            let [health_high, health_low] = record.health.to_be_bytes();
            for (field, value) in [
                (Field::XHigh, x_high),
                (Field::XLow, x_low),
                (Field::YHigh, y_high),
                (Field::YLow, y_low),
                (Field::Flags, record.flags),
                (Field::ChrSlot, record.chr_slot),
            ] {
                slots.set(slot, field, value);
            }
            slots.set_kind(slot, kind);
            for (field, value) in [
                (Field::State, record.state),
                (Field::Behaviour, record.behaviour),
                (Field::Subtype, record.subtype),
                (Field::Hit, 0),
                (Field::Phase, 0),
                (Field::Timer, 0),
                (Field::Counter, 0),
                (Field::Anim, 0),
            ] {
                slots.set(slot, field, value);
            }
            slots.set_platform_parameter(i, health_high);
            slots.set(slot, Field::HealthLow, health_low);
            slots.set(slot, Field::Sprite, record.contact);
            slots.set_pressed(i, 0);
            if by == Spawner::Column {
                slots.set(slot, Field::DyingTimer, 0);
            }
            slots.set(slot, Field::Cell, cell);
            slots.second[i].brain = SecondBrain::new(kind);
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

    fn green() -> SpawnRecord {
        SpawnRecord::from([
            0x1D, 0xFF, 0x05, 0xFF, 0x01, 0x05, 0x20, 0x00, 0x05, 0x00, 0xFF, 0x88,
        ])
    }

    fn table(prg: &[u8], ram: &[u8; 2048]) -> SpawnTable {
        SpawnTable::import(prg, ram[0x400], ram[0x402])
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
        let prg = Scene::prg();
        scene.machine.bus.trace.exec.push(0x89B5..=0x89B5);
        let events = scene.run(359, &[COLUMN, COLUMN_RETURNS[0], COLUMN_RETURNS[1]]);
        let (calls, main, _) = check_columns(&events, prg);
        assert!(calls > 10 && main >= 3, "{calls} calls, {main} spawns");
        assert!(scene.machine.bus.trace.execs.is_empty(), "$89AF path taken");
    }

    #[test]
    fn second_class_column_spawn_matches_the_oracle() {
        let Some(mut scene) = Scene::level(2, 650) else {
            return;
        };
        let prg = Scene::prg();
        let events = scene.play(40, 0x80, &[COLUMN, COLUMN_RETURNS[0], COLUMN_RETURNS[1]]);
        let (calls, _, second) = check_columns(&events, prg);
        assert!(second >= 1, "{calls} calls, {second} second-class spawns");
    }

    #[test]
    fn room_spawn_matches_the_oracle() {
        let Some(mut scene) = Scene::route(599) else {
            return;
        };
        let prg = Scene::prg();
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
        let table = table(prg, &begin.ram);
        assert_eq!(
            spawn_schedule(prg, begin.ram[0x400]).expect("the schedule")[..],
            begin.wram[0x1410..0x1510]
        );
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
            page_columns: [0; 32],
        };
        schedule.cells[0x03] = CELL_PENDING;
        let table = SpawnTable {
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
        assert_eq!(schedule.cells[0x03], CELL_PENDING);

        schedule.cells[0x83] = CELL_PENDING;
        spawn_at(&mut slots, &mut schedule, &table, 0x83);
        assert_eq!(slots, before);
        assert_eq!(schedule.cells[0x83], CELL_PENDING);
    }

    fn put16(prg: &mut [u8], at: usize, value: u16) {
        prg[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn synthetic_prg() -> Vec<u8> {
        let bank_bytes = crate::prg::BANK_BYTES;
        let mut prg = vec![0u8; 16 * bank_bytes];
        let spawn = usize::from(SPAWN_BANK) * bank_bytes;
        let spawn_off = |addr: u16| spawn + usize::from(addr - SPAWN_WINDOW);

        put16(&mut prg, spawn, 0xA024);
        put16(&mut prg, spawn + 2, 0xA030);
        put16(&mut prg, spawn + 0x24, 0xA050);
        put16(&mut prg, spawn + 0x26, 0xA060);
        put16(&mut prg, spawn + 0x30, 0xA060);
        put16(&mut prg, spawn + 0x50, 0xA100);
        put16(&mut prg, spawn + 0x56, 0xA110);
        put16(&mut prg, spawn + 0x60, 0xA120);

        let rec0 = spawn_off(0xA100);
        prg[rec0..rec0 + SPAWN_RECORD_BYTES].copy_from_slice(&[
            0x01, 0x02, 0x03, 0x04, 0x80, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
        ]);
        let rec1 = spawn_off(0xA110);
        prg[rec1..rec1 + SPAWN_RECORD_BYTES].copy_from_slice(&[
            0x10, 0x20, 0x30, 0x40, 0x00, 0xA5, 0x00, 0x0C, 0x68, 0x00, 0x05, 0x40,
        ]);
        put16(&mut prg, spawn + 0x2D4, 0xA200);
        prg[spawn + 0x200 + 1] = 0x83;
        prg[spawn + 0x200 + 4] = 0x80;
        prg
    }

    #[test]
    fn spawn_record_fields() {
        let prg = synthetic_prg();
        assert_eq!(
            spawn_record(&prg, 0xA110).unwrap(),
            SpawnRecord {
                x: 0x1020,
                y: 0x3040,
                flags: 0,
                chr_slot: 0,
                second_class: true,
                kind: 0x4A,
                state: 0x00,
                behaviour: 0x0C,
                subtype: 0x68,
                health: 0x0005,
                contact: 0x40,
            }
        );
        let split = |byte4, byte5| {
            let record = SpawnRecord::from([0, 0, 0, 0, byte4, byte5, 0, 0, 0, 0, 0, 0]);
            (
                record.flags,
                record.chr_slot,
                record.second_class,
                record.kind,
            )
        };
        assert_eq!(split(0xE3, 0x80), (0xE3, 0, true, 0));
        assert_eq!(split(0x1C, 0x7F), (0, 7, false, 0xFE));
    }

    #[test]
    fn spawn_ids_resolve_through_the_id_table() {
        let prg = synthetic_prg();
        assert_eq!(spawn_record_pointer(&prg, 0, 0, 3).unwrap(), 0xA110);
        assert_eq!(spawn_record_pointer(&prg, 0, 0, 0).unwrap(), 0xA100);
        assert_eq!(spawn_record_pointer(&prg, 2, 0, 0).unwrap(), 0xA120);
    }
}
