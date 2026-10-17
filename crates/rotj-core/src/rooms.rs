//! A room's layout as data: what the loader `$B40A` reads from the ROM on each room entry,
//! imported so the game builds the room from data (`docs/specs/2026-10-17-rooms-design.md`).

use crate::actors::bank_slice;

/// One room's layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Room {
    /// The background palette selector (room record byte 0).
    pub background_palette: u8,
    /// The sprite palette selector (bank 13 `$B5F5[area]`, by `room >> 1`).
    pub sprite_palette: u8,
    /// The CHR banks for `$E0-$E3` (record bytes 1-4); bit 7 names an animation group.
    pub chr: [u8; 4],
    /// The orientation byte `$DD` (record byte 5; `actor::world::orientation`).
    pub orientation: u8,
    /// How many pages the loader raises the attributes of (record byte 6), read only in
    /// rooms two pages across.
    pub raised_page_count: u8,
    /// The page order (`$0440-$045F`): page numbers 0-15, which RAM holds `<< 4`.
    pub pages: [u8; 32],
    /// Where the room starts.
    pub start: Start,
}

/// A room's start record (bank 13 `$B653[area]`, 15 bytes): the values the loader sets,
/// which the game then changes as it plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Start {
    /// The player's level x (`$0460`).
    pub player_x: u16,
    /// The player's level y (`$0462`).
    pub player_y: u16,
    /// The camera's level x (`$CC`).
    pub camera_x: u16,
    /// The camera's level y.
    pub camera_y: u16,
    /// The vertical scroll ring (`$C8`).
    pub ring_y: u16,
    /// The scroll locks.
    pub scroll_lock: u8,
    /// The scroll mode (`$C3`, the auto-scroll bits).
    pub scroll_mode: u8,
    /// The nametable mirroring (`$EC`).
    pub mirroring: u8,
    /// The first spawn schedule column (`$0403`).
    pub first_spawn_column: u8,
    /// The player's facing.
    pub facing: u8,
}

impl Room {
    /// Room `room` of area `area` (the raw `$0400`/`$0402` values, `room` already doubled),
    /// imported from the ROM: the room record (bank 0 through bank 14 `$C048[area]`), the
    /// sprite palette (bank 13 `$B5F5[area]`), the page order (bank 0 through bank 14
    /// `$C024[area]`) and the start record. `None` where a pointer leaves the PRG.
    #[must_use]
    pub fn import(prg: &[u8], area: u8, room: u8) -> Option<Room> {
        let byte = |bank: u8, addr: u16| Some(*bank_slice(prg, bank, addr & 0x1FFF).ok()?.first()?);
        let word = |bank: u8, addr: u16| {
            Some(u16::from_le_bytes([
                byte(bank, addr)?,
                byte(bank, addr.checked_add(1)?)?,
            ]))
        };
        let pointer = |bank: u8, table: u16, from_bank: u8| {
            word(
                from_bank,
                word(bank, table.checked_add(u16::from(area))?)?.checked_add(u16::from(room))?,
            )
        };
        let record = pointer(14, 0xC048, 0)?;
        let field = |i: u16| byte(0, record.checked_add(i)?);
        let sets = word(13, 0xB5F5 + u16::from(area))?;
        let page_order = pointer(14, 0xC024, 0)?;
        let start = pointer(13, 0xB653, 13)?;
        let start = |i: u16| byte(13, start.checked_add(i)?);
        let start_word = |i: u16| Some(u16::from_be_bytes([start(i)?, start(i + 1)?]));
        let mut pages = [0; 32];
        for (i, page) in (0..).zip(&mut pages) {
            *page = byte(0, page_order.checked_add(i)?)? & 0x0F;
        }
        Some(Room {
            background_palette: field(0)?,
            sprite_palette: byte(13, sets.checked_add(u16::from(room >> 1))?)?,
            chr: [field(1)?, field(2)?, field(3)?, field(4)?],
            orientation: field(5)?,
            raised_page_count: field(6)?,
            pages,
            start: Start {
                player_x: start_word(0)?,
                player_y: start_word(2)?,
                camera_x: start_word(4)?,
                camera_y: start_word(6)?,
                ring_y: start_word(8)?,
                scroll_lock: start(10)?,
                scroll_mode: start(11)?,
                mirroring: start(12)?,
                first_spawn_column: start(13)?,
                facing: start(14)?,
            },
        })
    }
}
