//! Actor spawn data: what the ROM holds for a level screen, decoded.
//!
//! Two adjacent systems exist, and they are **not** the same thing. Both are
//! decoded here because the per-slot fields they write (`$7D50`, `$7D90`,
//! `$7DB0`) are read together.
//!
//! # 1. The level screen records (bank 14, `$DA18`)
//!
//! The routine at CPU `$D992` (bank [`SCREEN_BANK`] offset `$1992`) is the
//! per-screen *screen metadata* loader that `docs/ACTORS.md` §5 calls the
//! "spawn path". It is table-driven, not a bytecode interpreter:
//!
//! ```text
//! LDX $0400            ; area, a byte offset into a 16-bit pointer table
//! LDA $DA18,x ; STA $30
//! LDA $DA19,x ; STA $31
//! LDY $0402            ; stage, a byte offset into the per-area table
//! LDA ($30),y ; TAX ; INY ; LDA ($30),y ; STA $31 ; STX $30
//! LDY #$00
//! LDA ($30),y ; CMP #$77 ; BEQ empty
//! ```
//!
//! A non-empty record is `[5 bytes -> $D0-$D4][16 bytes -> $70-$7F]
//! [8 bytes -> $60-$67][2 bytes -> $32/$33][JMP ($0032)]`. `$D0-$D4` is also
//! zero-extended to `$D0-$DC`. The trailing pointer is a **plain 6502 entry
//! point**, not a bytecode program: for every record the ROM defines it is one
//! of the three tiny routines at `$D9FF`/`$DA0E`/`$DA0F` that set the level
//! state bytes `$0466`/`$046E`/`$0481` and `RTS`. No record points into the
//! WRAM level payload. See [`script_kind`] and [`ScreenRecord`].
//!
//! # 2. The actual actor spawn records (bank 3, `$A000`)
//!
//! The routine at CPU `$D714` (bank 14) maps bank [`SPAWN_BANK`] into `$A000`
//! and spawns actors from a **12-byte record** per pending slot. The 256-byte
//! schedule copied from [`SPAWN_SCHEDULE`] into RAM `$7410` marks a slot
//! pending with bit 7; bits 0-5 are the record id. The record is reached
//! through three tables:
//!
//! ```text
//! $A000[area]               -> per-area table
//! per_area[stage*2]         -> per-stage record-pointer table
//! record_table[id*2]        -> 12-byte record
//! ```
//!
//! The 12 bytes are written in order to `$7B00`, `$7B20`, `$7B40`, `$7B60`,
//! `$7C00`/`$7C90`, `$7CC0` (bit 7 selects the `$7CC0` slot bank), `$7D50`
//! (**state**), `$7D90` (**behaviour**), `$7DB0` (**type**), then three more
//! (`$7EF0`/`$7BD8`, `$7BE0`, `$7EE0`/`$7C30`). See [`ActorSpawn`].
//!
//! # What is *not* established
//!
//! - The `$0402` value is used directly as a byte offset into the per-area
//!   tables, so it holds `stage * 2`; frame 700 of sublevel 1-1 reads `$0402 =
//!   0` (`$0400 = 0`). The reset value is `1` (`$E351`), which is not even, so
//!   the reset path is a separate state, not a gameplay stage.
//! - The job of the 5/16/8-byte screen blocks is not decoded. They land in
//!   zero page `$D0-$DC`/`$70-$7F`/`$60-$67`; `docs/MAP.md` §10 attributes
//!   `$D0-$D4` to IRQ split state, and bank 14's display-list executor uses
//!   `$60`/`$70` as staging, but no capture exercises it.
//! - `$7D90` and `$7DB0` are **not** handler indexes: `$7D90` selects a 16-bit
//!   knock-back velocity from [`BEHAVIOUR_HI`]/[`BEHAVIOUR_LO`] (bank 0 `$1125`),
//!   and `$7DB0` indexes a per-type position offset at bank 0 `$9932`. The
//!   per-slot *handler* dispatch is `$7DD0` -> a per-type jump table (e.g. bank 0
//!   `$8C74` at `$0A9B`, bank 8 `$8A68` at `$07A8`), and `$7D50` ->
//!   [`STATE_TABLE`]. The two `$7D90` behaviour entries lower to `$FFD8`/`$0028`.
//! - `$7D50` is the per-slot **state byte** (spawn-record byte 6), not a plain
//!   0-4 index. `$AB48` (`LDA $7D50,x; ASL; TAY; LDA $ABA8,y`) indexes the five
//!   [`STATE_TABLE`] entries with the shifted byte, so bit 7 is dropped and
//!   `$80-$84` reach the same entries as `$00-$04` (verified: bank 0 `$89C6` and
//!   bank 8 `$8B6A` count the byte up and wrap it back to `$80` when the next
//!   value would be `$84`). Bit 6 is a "finished / no longer a valid
//!   target" flag (bank 8 `$8D33` sets it; the target finders test it with
//!   `ASL; BMI`). The state's meaning beyond that is per-type: the reachable
//!   code writes and counts `$7D50` in many actor handlers, so values such as
//!   `$20`/`$61` are leads, not decoded. See [`LiveSlot`] and
//!   [`state_dispatch_offset`].

use thiserror::Error;

use crate::ines::FME7_PRG_BANK_SIZE;

/// FME-7 PRG bank holding the bank-14 screen records and the spawn routine.
pub const SCREEN_BANK: u8 = 0x0E;

/// CPU window `$DA18` lives in for [`SCREEN_BANK`].
pub const SCREEN_WINDOW: u16 = 0xC000;

/// CPU `$DA18`: 16-bit LE table indexed by `$0400` (area, as a byte offset)
/// whose entries point at per-area tables of stage pointers.
pub const SCREEN_TABLE: u16 = 0xDA18;

/// First byte of an empty screen record (`CMP #$77`, `$D9AD`).
pub const SCREEN_EMPTY: u8 = 0x77;

/// Bytes of the screen record's first block (staged to `$D0-$D4`).
pub const SCREEN_HEADER_BYTES: usize = 5;

/// Bytes of the screen record's second block (staged to `$70-$7F`).
pub const SCREEN_BLOCK_A_BYTES: usize = 16;

/// Bytes of the screen record's third block (staged to `$60-$67`).
pub const SCREEN_BLOCK_B_BYTES: usize = 8;

/// The three `JMP ($0032)` script entry points the ROM's records use, all of
/// which only set level state and `RTS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptKind {
    /// CPU `$D9FF`: sets `$0466 = $85`, `$046E = $0B`, `$0481 = $A0`.
    LevelState,
    /// CPU `$DA0E`: a bare `RTS`.
    Return,
    /// CPU `$DA0F`: sets `$046E = $0A`, branches back to `$DA0B` to set
    /// `$0481 = $E0`, then `RTS`.
    ScreenState,
    /// An entry point not among the three the ROM defines.
    Other,
}

/// Classify a script entry point.
#[must_use]
pub const fn script_kind(addr: u16) -> ScriptKind {
    match addr {
        0xD9FF => ScriptKind::LevelState,
        0xDA0E => ScriptKind::Return,
        0xDA0F => ScriptKind::ScreenState,
        _ => ScriptKind::Other,
    }
}

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

/// Schedule flag (bit 7 of a schedule byte): the slot is pending a spawn.
pub const SPAWN_PENDING: u8 = 0x80;

/// Record byte 7 -> RAM `$7D90`, the behaviour index.
pub const SPAWN_BEHAVIOUR_INDEX: usize = 7;

/// Record byte 8 -> RAM `$7DB0`, the type index.
pub const SPAWN_TYPE_INDEX: usize = 8;

/// FME-7 PRG bank holding the `$7D90` behaviour tables.
pub const BEHAVIOUR_BANK: u8 = 0x00;

/// CPU window the behaviour tables live in for [`BEHAVIOUR_BANK`].
pub const BEHAVIOUR_WINDOW: u16 = 0x8000;

/// CPU `$87F5`: high-byte table indexed by `$7D90` (bank 0 `$1125`).
pub const BEHAVIOUR_HI: u16 = 0x87F5;

/// CPU `$87F7`: low-byte table indexed by `$7D90` (bank 0 `$1125`).
pub const BEHAVIOUR_LO: u16 = 0x87F7;

/// Entries in each behaviour table: `{ $FFD8, $0028 }` (knock-back +-40).
pub const BEHAVIOUR_ENTRIES: usize = 2;

/// FME-7 PRG bank holding the `$7D50` state dispatch table.
pub const STATE_BANK: u8 = 0x0D;

/// CPU window the state dispatch table lives in for [`STATE_BANK`].
pub const STATE_WINDOW: u16 = 0xA000;

/// CPU `$ABA8`: 16-bit LE handler table indexed by `$7D50 * 2` (`$AB48`).
pub const STATE_TABLE: u16 = 0xABA8;

/// Entries in [`STATE_TABLE`]: states 0-3 -> `$AB5A`, state 4 -> `$AB69`.
pub const STATE_ENTRIES: usize = 5;

/// Slots in each actor array. The spawner's free-slot search walks `0-15`
/// (record byte 5 bit 7 clear) or `24-31` (bit 7 set), so 16-23 stay unused
/// (bank 3 `$D83E`/`$D838`).
pub const SLOT_COUNT: usize = 32;

/// The two slot ranges the spawner scans (bank 3 `$D83E`/`$D838`).
pub const SLOT_SCAN: [core::ops::Range<usize>; 2] = [0..16, 24..32];

/// CPU `$7CC0`: first byte of the per-slot window the decoder reads. Each array
/// is 32 bytes apart, one byte per slot.
pub const SLOT_WINDOW: u16 = 0x7CC0;

/// Bytes from [`SLOT_WINDOW`] to the end of the arrays (`$7CC0-$7DEF`).
pub const SLOT_WINDOW_LEN: usize = 0x130;

/// CPU `$7CC0`: the per-slot occupied byte (the array base, [`SLOT_WINDOW`]).
/// Non-zero marks a live slot and holds the object type
/// ([`ActorSpawn::slot_type`]); the spawner searches it with `LDA $7CC0,x; BEQ`
/// (bank 3 `$D840`) and bank 13 `$AB42` clears it to free a slot.
pub const SLOT_OCCUPIED: u16 = SLOT_WINDOW;

/// CPU `$7CE0`: the spawn-schedule slot the object came from (bank 14 `$D820`).
pub const SLOT_SCHEDULE: u16 = 0x7CE0;

/// CPU `$7D50`: the per-slot state byte ([`ActorSpawn::state`], record byte 6).
pub const SLOT_STATE: u16 = 0x7D50;

/// CPU `$7D90`: the per-slot behaviour index (record byte 7).
pub const SLOT_BEHAVIOUR: u16 = 0x7D90;

/// CPU `$7DB0`: the per-slot type index (record byte 8).
pub const SLOT_KIND: u16 = 0x7DB0;

/// CPU `$7DD0`: the per-slot behaviour phase / child counter (bank 0 `$8A9B`
/// indexes a handler table with it; bank 0 `$8704` increments it and frees the
/// slot at 5).
pub const SLOT_PHASE: u16 = 0x7DD0;

/// `$7D50` bit 7: the state counter keeps this bit set (`$80-$84`). Bank 13
/// `$A5C3` reads it as "still alive" (bit clear frees the slot); bank 8 `$9342`
/// branches on it (`BPL`).
pub const STATE_ACTIVE: u8 = 0x80;

/// `$7D50` bit 6: the slot has finished and must not be chosen as a target.
/// Bank 8 `$8D33` sets it (`ORA #$40`); the target finders test it with
/// `LDA $7D50,y; ASL; BMI` (bank 5 `$8417`/`$8430`, bank 4 `$9F2C`/`$9F54`).
pub const STATE_DONE: u8 = 0x40;

/// The `$ABA8` byte offset the state dispatch computes (`LDA $7D50,x; ASL`,
/// bank 13 `$AB48`). `ASL` drops bit 7, so `$80-$84` index the same entries as
/// `$00-$04`.
#[must_use]
pub const fn state_dispatch_offset(state: u8) -> u8 {
    state << 1
}

/// One live actor slot, read from a [`SLOT_WINDOW`] WRAM slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveSlot {
    /// Slot index (0-15 or 24-31).
    pub slot: usize,
    /// `$7CC0`: occupied byte / type.
    pub occupied: u8,
    /// `$7CE0`: the spawn-schedule slot it came from.
    pub schedule: u8,
    /// `$7D50`: the state/flags byte.
    pub state: u8,
    /// `$7D90`: the behaviour index.
    pub behaviour: u8,
    /// `$7DB0`: the type index.
    pub kind: u8,
    /// `$7DD0`: the behaviour phase / child counter.
    pub phase: u8,
}

impl LiveSlot {
    /// Whether the state's [`STATE_ACTIVE`] bit is set.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.state & STATE_ACTIVE != 0
    }

    /// Whether the state's [`STATE_DONE`] bit is set (not a valid target).
    #[must_use]
    pub const fn is_done(&self) -> bool {
        self.state & STATE_DONE != 0
    }
}

/// Decode the live slots from a WRAM slice covering [`SLOT_WINDOW`]
/// (`$7CC0-$7DEF`, [`SLOT_WINDOW_LEN`] bytes). A slot is live when its `$7CC0`
/// byte is non-zero.
#[must_use]
pub fn live_slots(wram: &[u8]) -> Vec<LiveSlot> {
    let field = |addr: u16, slot: usize| wram[usize::from(addr - SLOT_WINDOW) + slot];
    let mut out = Vec::new();
    for slot in SLOT_SCAN.into_iter().flatten() {
        let occupied = field(SLOT_OCCUPIED, slot);
        if occupied == 0 {
            continue;
        }
        out.push(LiveSlot {
            slot,
            occupied,
            schedule: field(SLOT_SCHEDULE, slot),
            state: field(SLOT_STATE, slot),
            behaviour: field(SLOT_BEHAVIOUR, slot),
            kind: field(SLOT_KIND, slot),
            phase: field(SLOT_PHASE, slot),
        });
    }
    out
}

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
    let base = usize::from(bank) * FME7_PRG_BANK_SIZE + usize::from(offset);
    prg.get(base..).ok_or(ActorError::OutOfRange {
        bank,
        offset,
        len: prg.len(),
    })
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

/// A byte-count constant as a `u16` address offset.
///
/// The block sizes are tiny, so the conversion cannot fail.
fn block_offset(bytes: usize) -> u16 {
    u16::try_from(bytes).expect("block sizes are far below u16::MAX")
}

/// One bank-14 `$DA18` screen record: a `[5][16][8]` staged block plus the
/// `JMP ($0032)` target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRecord {
    /// CPU address the record was read from.
    pub addr: u16,
    /// First block, staged to `$D0-$D4` (and zero-extended to `$D0-$DC`).
    pub header: [u8; SCREEN_HEADER_BYTES],
    /// Second block, staged to `$70-$7F`.
    pub block_a: [u8; SCREEN_BLOCK_A_BYTES],
    /// Third block, staged to `$60-$67`.
    pub block_b: [u8; SCREEN_BLOCK_B_BYTES],
    /// CPU address the record's `JMP ($0032)` enters (plain 6502, not a
    /// bytecode). Classify it with [`script_kind`].
    pub script: u16,
}

/// Read the screen record at CPU `addr` inside `SCREEN_BANK`.
///
/// Returns `Ok(None)` for the empty record ([`SCREEN_EMPTY`]).
pub fn screen_record(prg: &[u8], addr: u16) -> Result<Option<ScreenRecord>, ActorError> {
    let (first, _) = bytes_at(prg, SCREEN_BANK, SCREEN_WINDOW, addr, 1)?
        .split_first()
        .ok_or(ActorError::OutOfRange {
            bank: SCREEN_BANK,
            offset: addr.wrapping_sub(SCREEN_WINDOW),
            len: prg.len(),
        })?;
    if *first == SCREEN_EMPTY {
        return Ok(None);
    }
    let header = bytes_at(prg, SCREEN_BANK, SCREEN_WINDOW, addr, SCREEN_HEADER_BYTES)?;
    let block_a = bytes_at(
        prg,
        SCREEN_BANK,
        SCREEN_WINDOW,
        addr + block_offset(SCREEN_HEADER_BYTES),
        SCREEN_BLOCK_A_BYTES,
    )?;
    let block_b = bytes_at(
        prg,
        SCREEN_BANK,
        SCREEN_WINDOW,
        addr + block_offset(SCREEN_HEADER_BYTES + SCREEN_BLOCK_A_BYTES),
        SCREEN_BLOCK_B_BYTES,
    )?;
    let script = word_at(
        prg,
        SCREEN_BANK,
        SCREEN_WINDOW,
        addr + block_offset(SCREEN_HEADER_BYTES + SCREEN_BLOCK_A_BYTES + SCREEN_BLOCK_B_BYTES),
    )?;
    Ok(Some(ScreenRecord {
        addr,
        header: header.try_into().expect("length checked above"),
        block_a: block_a.try_into().expect("length checked above"),
        block_b: block_b.try_into().expect("length checked above"),
        script,
    }))
}

/// Follow [`SCREEN_TABLE`] for `area`/`stage` (the RAM `$0400`/`$0402` values,
/// each a byte offset) and decode the screen record it names.
pub fn screen_record_at(
    prg: &[u8],
    area: u8,
    stage: u8,
) -> Result<Option<ScreenRecord>, ActorError> {
    let area_table = word_at(
        prg,
        SCREEN_BANK,
        SCREEN_WINDOW,
        SCREEN_TABLE + u16::from(area),
    )?;
    let addr = word_at(
        prg,
        SCREEN_BANK,
        SCREEN_WINDOW,
        area_table + u16::from(stage),
    )?;
    screen_record(prg, addr)
}

/// One bank-3 actor spawn record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorSpawn {
    /// CPU address the record was read from.
    pub addr: u16,
    /// The raw 12 record bytes.
    pub raw: [u8; SPAWN_RECORD_BYTES],
}

impl ActorSpawn {
    /// Record byte 6 -> RAM `$7D50`, the slot's state index.
    #[must_use]
    pub const fn state(&self) -> u8 {
        self.raw[6]
    }

    /// Record byte 7 -> RAM `$7D90`, the behaviour index (a [`BEHAVIOUR_HI`]
    /// velocity selector, not a handler index).
    #[must_use]
    pub const fn behaviour(&self) -> u8 {
        self.raw[SPAWN_BEHAVIOUR_INDEX]
    }

    /// Record byte 8 -> RAM `$7DB0`, the type index.
    #[must_use]
    pub const fn kind(&self) -> u8 {
        self.raw[SPAWN_TYPE_INDEX]
    }

    /// Record byte 5: bit 7 selects the second slot bank (`$7CC0` at `$18+`).
    #[must_use]
    pub const fn slot_bank(&self) -> u8 {
        self.raw[5] >> 7
    }

    /// Record byte 5 low bits, shifted left as the ROM does for `$7CC0`.
    #[must_use]
    pub const fn slot_type(&self) -> u8 {
        (self.raw[5] & 0x7F) << 1
    }
}

/// Read the actor spawn record at CPU `addr` inside `SPAWN_BANK`.
pub fn actor_spawn(prg: &[u8], addr: u16) -> Result<ActorSpawn, ActorError> {
    let bytes = bytes_at(prg, SPAWN_BANK, SPAWN_WINDOW, addr, SPAWN_RECORD_BYTES)?;
    Ok(ActorSpawn {
        addr,
        raw: bytes.try_into().expect("length checked above"),
    })
}

/// Follow [`SPAWN_TABLE`] for `area`/`stage` (RAM `$0400`/`$0402` byte offsets)
/// to the per-stage table of record pointers indexed by spawn id.
pub fn spawn_record_table(prg: &[u8], area: u8, stage: u8) -> Result<u16, ActorError> {
    let area_table = word_at(prg, SPAWN_BANK, SPAWN_WINDOW, SPAWN_TABLE + u16::from(area))?;
    word_at(prg, SPAWN_BANK, SPAWN_WINDOW, area_table + u16::from(stage))
}

/// Read the spawn id -> record-pointer table entry for `id`.
pub fn spawn_record_pointer(prg: &[u8], area: u8, stage: u8, id: u8) -> Result<u16, ActorError> {
    let table = spawn_record_table(prg, area, stage)?;
    word_at(prg, SPAWN_BANK, SPAWN_WINDOW, table + u16::from(id) * 2)
}

/// Copy the 256-byte spawn schedule for `area` (RAM `$7410`, loaded by `$D854`).
pub fn spawn_schedule(prg: &[u8], area: u8) -> Result<Vec<u8>, ActorError> {
    let addr = word_at(
        prg,
        SPAWN_BANK,
        SPAWN_WINDOW,
        SPAWN_SCHEDULE + u16::from(area),
    )?;
    Ok(bytes_at(prg, SPAWN_BANK, SPAWN_WINDOW, addr, SPAWN_SCHEDULE_BYTES)?.to_vec())
}

/// One slot the schedule marks pending, with the record it selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingSpawn {
    /// Slot index (RAM `$7410` index, 0-31 as the scanner walks it).
    pub slot: usize,
    /// Spawn id (schedule byte bits 0-5).
    pub id: u8,
    /// The record the id resolves to.
    pub spawn: ActorSpawn,
}

/// All spawns `area`/`stage` schedules: schedule bytes with bit 7 set, resolved
/// through the id -> record table.
///
/// The scanner walks slots 0-15 (first branch) or 24-31 (second branch, taken
/// when the record's byte 5 has bit 7 set), so schedule bytes 16-23 are never
/// read as slot ids and are skipped.
pub fn pending_spawns(prg: &[u8], area: u8, stage: u8) -> Result<Vec<PendingSpawn>, ActorError> {
    let schedule = spawn_schedule(prg, area)?;
    let mut out = Vec::new();
    for slot in (0..16).chain(24..32) {
        let byte = schedule[slot];
        if byte & SPAWN_PENDING == 0 {
            continue;
        }
        let id = byte & 0x3F;
        let addr = spawn_record_pointer(prg, area, stage, id)?;
        out.push(PendingSpawn {
            slot,
            id,
            spawn: actor_spawn(prg, addr)?,
        });
    }
    Ok(out)
}

/// The `$7D90` behaviour table as `(high, low)` velocity pairs.
pub fn behaviour_table(prg: &[u8]) -> Result<Vec<(u8, u8)>, ActorError> {
    let mut out = Vec::with_capacity(BEHAVIOUR_ENTRIES);
    for i in 0..BEHAVIOUR_ENTRIES {
        let hi = bytes_at(prg, BEHAVIOUR_BANK, BEHAVIOUR_WINDOW, BEHAVIOUR_HI, i + 1)?[i];
        let lo = bytes_at(prg, BEHAVIOUR_BANK, BEHAVIOUR_WINDOW, BEHAVIOUR_LO, i + 1)?[i];
        out.push((hi, lo));
    }
    Ok(out)
}

/// The `$7D50` state dispatch table at [`STATE_TABLE`] as handler addresses.
pub fn state_handlers(prg: &[u8]) -> Result<Vec<u16>, ActorError> {
    (0..STATE_ENTRIES)
        .map(|i| {
            let addr = STATE_TABLE + u16::try_from(i).expect("table is tiny") * 2;
            word_at(prg, STATE_BANK, STATE_WINDOW, addr)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::oracle::Scene;

    /// WRAM offset of `$7CC0` in the oracle's `$6000`-based 8 KiB image.
    const SLOT_WRAM: usize = 0x1CC0;

    /// The `digests.txt` entry `name`, unit `-`.
    fn digest(name: &str) -> u64 {
        crate::digest::table(include_str!("../tests/data/digests.txt"), name)["-"]
    }

    /// Byte offset of the start of `bank` in the flattened PRG.
    fn base(bank: u8) -> usize {
        usize::from(bank) * FME7_PRG_BANK_SIZE
    }

    /// Bank-3 offset for a CPU `$A000`-window address.
    fn spawn_off(addr: u16) -> usize {
        base(SPAWN_BANK) + usize::from(addr - SPAWN_WINDOW)
    }

    /// A 16-bank PRG holding one screen record and one actor spawn.
    fn synthetic_prg() -> Vec<u8> {
        let mut prg = vec![0u8; 16 * FME7_PRG_BANK_SIZE];
        let screen = base(SCREEN_BANK);
        let spawn = base(SPAWN_BANK);
        let behaviour = base(BEHAVIOUR_BANK);
        let state = base(STATE_BANK);

        // $DA18[0] = $DA30, $DA18[2] = $DA40.
        put16(&mut prg, screen + 0x1A18, 0xDA30);
        put16(&mut prg, screen + 0x1A1A, 0xDA40);
        // $DA30[0] = $DA80 (stage 0 full record), $DA30[2] = $DAC0 (empty).
        put16(&mut prg, screen + 0x1A30, 0xDA80);
        put16(&mut prg, screen + 0x1A32, 0xDAC0);
        // $DA40 area 1 -> stage 0 record at $DAD0 (empty).
        put16(&mut prg, screen + 0x1A40, 0xDAD0);
        prg[screen + 0x1AC0] = SCREEN_EMPTY;
        prg[screen + 0x1AD0] = SCREEN_EMPTY;
        // $DA80: a full record (31 bytes, $DA80-$DA9E).
        let rec = screen + 0x1A80;
        prg[rec..rec + 5].copy_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55]);
        for i in 0..16 {
            prg[rec + 5 + i] = u8::try_from(0xA0 + i).expect("fits");
        }
        for i in 0..8 {
            prg[rec + 21 + i] = u8::try_from(0xB0 + i).expect("fits");
        }
        put16(&mut prg, rec + 29, 0xDB00);
        // The script the record names: LDA #$85, RTS at $DB00.
        prg[screen + 0x1B00] = 0xA9;
        prg[screen + 0x1B01] = 0x85;
        prg[screen + 0x1B02] = 0x60;

        // Bank 3: $A000[0] = $A024, $A000[2] = $A030.
        put16(&mut prg, spawn, 0xA024);
        put16(&mut prg, spawn + 2, 0xA030);
        // $A024[0] = $A050 (stage 0), $A024[2] = $A060.
        put16(&mut prg, spawn + 0x24, 0xA050);
        put16(&mut prg, spawn + 0x26, 0xA060);
        // $A030[0] = $A060.
        put16(&mut prg, spawn + 0x30, 0xA060);
        // $A050 id 0 -> $A100, id 3 -> $A110.
        put16(&mut prg, spawn + 0x50, 0xA100);
        put16(&mut prg, spawn + 0x56, 0xA110);
        // $A060 (area 1 stage 0) id 0 -> $A120.
        put16(&mut prg, spawn + 0x60, 0xA120);
        // Records.
        let rec0 = spawn_off(0xA100);
        prg[rec0..rec0 + SPAWN_RECORD_BYTES].copy_from_slice(&[
            0x01, 0x02, 0x03, 0x04, 0x80, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
        ]);
        let rec1 = spawn_off(0xA110);
        prg[rec1..rec1 + SPAWN_RECORD_BYTES].copy_from_slice(&[
            0x10, 0x20, 0x30, 0x40, 0x00, 0xA5, 0x00, 0x0C, 0x68, 0x00, 0x05, 0x40,
        ]);
        // Schedule: $A2D4[0] = $A200. Slot 1 pending id 3, slot 4 pending id 0.
        put16(&mut prg, spawn + 0x2D4, 0xA200);
        prg[spawn + 0x200 + 1] = 0x83;
        prg[spawn + 0x200 + 4] = 0x80;
        // Behaviour tables: { $FF, $00 } and { $D8, $28 }.
        prg[behaviour + 0x7F5] = 0xFF;
        prg[behaviour + 0x7F6] = 0x00;
        prg[behaviour + 0x7F7] = 0xD8;
        prg[behaviour + 0x7F8] = 0x28;
        // State dispatch: 5 entries -> $AB5A x4 then $AB69.
        for i in 0..4 {
            put16(&mut prg, state + 0xBA8 + i * 2, 0xAB5A);
        }
        put16(&mut prg, state + 0xBA8 + 8, 0xAB69);
        prg
    }

    /// Write a 16-bit LE word at an absolute PRG offset.
    fn put16(prg: &mut [u8], at: usize, value: u16) {
        let bytes = value.to_le_bytes();
        prg[at] = bytes[0];
        prg[at + 1] = bytes[1];
    }

    #[test]
    fn screen_record_decodes_blocks_and_script() {
        let prg = synthetic_prg();
        let rec = screen_record_at(&prg, 0, 0).unwrap().unwrap();
        assert_eq!(rec.addr, 0xDA80);
        assert_eq!(rec.header, [0x11, 0x22, 0x33, 0x44, 0x55]);
        assert_eq!(rec.block_a[0], 0xA0);
        assert_eq!(rec.block_a[15], 0xAF);
        assert_eq!(rec.block_b[0], 0xB0);
        assert_eq!(rec.block_b[7], 0xB7);
        assert_eq!(rec.script, 0xDB00);
        assert_eq!(script_kind(rec.script), ScriptKind::Other);
    }

    #[test]
    fn script_kind_classifies_the_three_rom_entry_points() {
        assert_eq!(script_kind(0xD9FF), ScriptKind::LevelState);
        assert_eq!(script_kind(0xDA0E), ScriptKind::Return);
        assert_eq!(script_kind(0xDA0F), ScriptKind::ScreenState);
        assert_eq!(script_kind(0x1234), ScriptKind::Other);
    }

    #[test]
    fn empty_screen_record_is_none() {
        let prg = synthetic_prg();
        assert_eq!(screen_record_at(&prg, 2, 0).unwrap(), None);
        assert_eq!(screen_record_at(&prg, 0, 2).unwrap(), None);
    }

    #[test]
    fn screen_record_out_of_range_is_an_error() {
        let prg = vec![0u8; 16];
        assert!(matches!(
            screen_record(&prg, 0xDA60),
            Err(ActorError::OutOfRange { .. })
        ));
    }

    #[test]
    fn actor_spawn_exposes_state_behaviour_and_type() {
        let prg = synthetic_prg();
        let spawn = actor_spawn(&prg, 0xA110).unwrap();
        assert_eq!(spawn.state(), 0x00);
        assert_eq!(spawn.behaviour(), 0x0C);
        assert_eq!(spawn.kind(), 0x68);
        assert_eq!(spawn.slot_bank(), 1);
        assert_eq!(spawn.slot_type(), 0x4A);
    }

    #[test]
    fn pending_spawns_resolve_through_the_id_table() {
        let prg = synthetic_prg();
        let spawns = pending_spawns(&prg, 0, 0).unwrap();
        assert_eq!(spawns.len(), 2);
        assert_eq!(spawns[0].slot, 1);
        assert_eq!(spawns[0].id, 3);
        assert_eq!(spawns[0].spawn.addr, 0xA110);
        assert_eq!(spawns[1].slot, 4);
        assert_eq!(spawns[1].id, 0);
        assert_eq!(spawns[1].spawn.addr, 0xA100);
        // Area 1 stage 0 resolves a different id table.
        assert_eq!(spawn_record_pointer(&prg, 2, 0, 0).unwrap(), 0xA120);
    }

    #[test]
    fn behaviour_and_state_tables_decode() {
        let prg = synthetic_prg();
        assert_eq!(behaviour_table(&prg).unwrap(), [(0xFF, 0xD8), (0x00, 0x28)]);
        assert_eq!(
            state_handlers(&prg).unwrap(),
            [0xAB5A, 0xAB5A, 0xAB5A, 0xAB5A, 0xAB69]
        );
    }

    /// The opening screen's WRAM slice `$7CC0-$7DEF`: the oracle replaying the
    /// physics capture's input to `machine.frame == 699` (frame 700). The bytes are
    /// pinned to fceux's `wram-700.bin` by `digests.txt`.
    fn captured_slots() -> Option<Vec<u8>> {
        let pads = (0..=699usize).map(rotj_oracle::physics_input).collect();
        let mut scene = Scene::power_on(pads)?;
        scene.run(699, &[]);
        let wram = scene.machine.bus.wram[SLOT_WRAM..SLOT_WRAM + SLOT_WINDOW_LEN].to_vec();
        assert_eq!(
            crate::digest::fnv1a(&wram),
            digest("actors-seek-700"),
            "the oracle's physics WRAM differs from fceux's wram-700.bin \
             (regenerate with dump.lua INPUT=physics)"
        );
        Some(wram)
    }

    /// The live-slot byte is `$7CC0`, not `$7BD0`: at frame 700 only slot 2 is
    /// occupied, though slots 0 and 1 still hold the data of the objects that
    /// spawned at frame 607 (`$7D50-$7D52 = 81 00 81`, `$7DB0 = 68 00 68`).
    /// The values are the area-0 records the write trace resolved: slot 2 came
    /// from schedule slot `$0E` (record id 1, `$AC94`); `$AC94` byte 5 `$03`,
    /// byte 6 `$81`, byte 7 `$0C`, byte 8 `$68`.
    #[test]
    fn live_slots_decode_the_captured_opening_screen() {
        let Some(wram) = captured_slots() else {
            return;
        };
        let slots = live_slots(&wram);
        assert_eq!(slots.len(), 1, "exactly one occupied slot");
        let slot = slots[0];
        assert_eq!(slot.slot, 2);
        assert_eq!(slot.occupied, 0x06);
        assert_eq!(slot.schedule, 0x0E);
        assert_eq!(slot.state, 0x81);
        assert_eq!(slot.behaviour, 0x0C);
        assert_eq!(slot.kind, 0x68);
        assert_eq!(slot.phase, 0x00);
        assert!(slot.is_active());
        assert!(!slot.is_done());
    }

    /// The scripted stage-1-1 route's WRAM slice `$7CC0-$7DEF`: the oracle
    /// replaying `route-inputs.csv` to `machine.frame == 899` (frame 900). Pinned to
    /// fceux's `wram-900.bin` by `digests.txt`. Four slots are live; slots 0 and 3
    /// hold the two actors that are drawn as sprites (the orange soldier and the
    /// green enemy), while slots 1 and 2 are `$81` state and are not drawn.
    fn route_slots() -> Option<Vec<u8>> {
        let scene = Scene::route(899)?;
        let wram = scene.machine.bus.wram[SLOT_WRAM..SLOT_WRAM + SLOT_WINDOW_LEN].to_vec();
        assert_eq!(
            crate::digest::fnv1a(&wram),
            digest("actors-route-900"),
            "the oracle's route WRAM differs from fceux's wram-900.bin \
             (regenerate with dump.lua INPUT=route ROUTE_JUMPS=735..760,792..817)"
        );
        Some(wram)
    }

    /// Slot -> spawn record on the route capture, established by the capture's
    /// slot-window write trace (`tools/emu/route-dense/ram-writes.csv`). At
    /// frame 846 the spawn routine writes slot 0 (`$7CC0 = $22`, `$7D50 = $84`,
    /// `$7DB0 = $00`, schedule `$18` = 24 -> id 7 -> record `$ACDC`); at frame
    /// 894 it writes slot 3 (`$0A`/`$20`/`$05`, schedule `$1D` = 29 -> id 4 ->
    /// record `$ACB8`). The orange soldier's sprites are already on screen at
    /// frames 850-854, before slot 1 spawns at 855, so the orange cluster is
    /// slot 0; the green enemy appears at 896, two frames after slot 3 spawns.
    #[test]
    fn live_slots_decode_the_route_capture() {
        let Some(wram) = route_slots() else {
            return;
        };
        let slots = live_slots(&wram);
        assert_eq!(slots.len(), 4, "four occupied slots");
        let by_slot = |slot: usize| {
            *slots
                .iter()
                .find(|s| s.slot == slot)
                .unwrap_or_else(|| panic!("slot {slot} live"))
        };

        // Slots 0 and 3: the two actors the capture draws as sprites.
        let orange = by_slot(0);
        assert_eq!(orange.occupied, 0x22);
        assert_eq!(orange.schedule, 0x18);
        assert_eq!(
            (orange.state, orange.behaviour, orange.kind),
            (0x84, 0x00, 0x00)
        );
        let green = by_slot(3);
        assert_eq!(green.occupied, 0x0A);
        assert_eq!(green.schedule, 0x1D);
        assert_eq!(
            (green.state, green.behaviour, green.kind),
            (0x20, 0x00, 0x05)
        );

        // Slots 1 and 2 are `$81` state: live in the arrays, not drawn.
        for slot in [1, 2] {
            assert_eq!(by_slot(slot).state, 0x81, "slot {slot} is not drawn");
        }
    }

    /// The two drawn slots carry exactly their bank-3 spawn records' bytes 5-8:
    /// `$7CC0 = (byte5 & $7F) << 1`, then state/behaviour/kind = bytes 6/7/8.
    /// The records are `$ACDC` (id 7, schedule slot 24) and `$ACB8` (id 4,
    /// schedule slot 29), decoded from bank 3 in `docs/ACTORS.md` §7b.
    #[test]
    fn route_slots_match_their_spawn_records() {
        let Some(wram) = route_slots() else {
            return;
        };
        let slots = live_slots(&wram);
        let records: [([u8; SPAWN_RECORD_BYTES], u16); 2] = [
            (
                [
                    0x18, 0xF8, 0x02, 0x00, 0x5E, 0x11, 0x84, 0x00, 0x00, 0x00, 0x04, 0x10,
                ],
                0xACDC,
            ),
            (
                [
                    0x1D, 0xFF, 0x05, 0xFF, 0x01, 0x05, 0x20, 0x00, 0x05, 0x00, 0xFF, 0x88,
                ],
                0xACB8,
            ),
        ];
        for (raw, addr) in records {
            let spawn = ActorSpawn { addr, raw };
            let slot = *slots
                .iter()
                .find(|s| s.occupied == spawn.slot_type())
                .unwrap_or_else(|| panic!("a live slot for record ${addr:04X}"));
            assert_eq!(spawn.slot_bank(), 0);
            assert_eq!(slot.state, spawn.state(), "record ${addr:04X} state");
            assert_eq!(
                slot.behaviour,
                spawn.behaviour(),
                "record ${addr:04X} behaviour"
            );
            assert_eq!(slot.kind, spawn.kind(), "record ${addr:04X} kind");
        }
    }

    /// The occupied byte the spawner writes is `(byte5 & $7F) << 1` (bank 14
    /// `$D7A5`) and the schedule byte is the `$7410` slot it found (`$D820`):
    /// the capture's slot 2 matches record `$AC94` on both.
    #[test]
    fn occupied_byte_matches_the_record_and_schedule() {
        let mut raw = [0u8; SPAWN_RECORD_BYTES];
        raw[5] = 0x03;
        let spawn = ActorSpawn { addr: 0xAC94, raw };
        let Some(wram) = captured_slots() else {
            return;
        };
        let slot = live_slots(&wram)[0];
        assert_eq!(spawn.slot_type(), slot.occupied);
        assert_eq!(spawn.slot_bank(), 0);
        assert_eq!(slot.schedule, 0x0E);
    }

    /// `$AB48` shifts the state before indexing `$ABA8`, so bit 7 is dropped and
    /// `$80-$84` reach the same five entries as `$00-$04`; bit 6 pushes the
    /// index past the table, which is why a `$40` state is filtered out first.
    #[test]
    fn state_dispatch_indexes_through_the_shifted_byte() {
        assert_eq!(state_dispatch_offset(0x00), 0);
        assert_eq!(state_dispatch_offset(0x01), 2);
        assert_eq!(state_dispatch_offset(0x04), 8);
        assert_eq!(state_dispatch_offset(0x80), 0);
        assert_eq!(state_dispatch_offset(0x81), 2);
        assert_eq!(state_dispatch_offset(0x84), 8);
        assert_eq!(state_dispatch_offset(0x40), 0x80);
    }
}
