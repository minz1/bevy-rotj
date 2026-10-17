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
    /// The raster split the room starts (screen record bytes 0-4), `None` for the empty
    /// record (`$77`).
    pub raster: Option<Raster>,
    /// The metatile rows the column stream leaves alone (bytes 5-20, `$70-$7F` bit 7).
    pub fixed_rows: [bool; 16],
    /// The attribute half-rows the column stream leaves alone (bytes 21-28, `$60-$67`
    /// bit 7).
    pub fixed_quads: [bool; 8],
    /// How the player arrives (bytes 29-30, a handler pointer).
    pub arrival: Arrival,
}

/// A room's raster split (`$D0-$D4`), from its screen record (bank 14 `$DA18[area]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Raster {
    /// `$D0` bit 7: the NMI arms the first IRQ.
    pub armed: bool,
    /// `$D1:$D2`: the CPU cycles to the first IRQ.
    pub first_irq_cycles: u16,
    /// `$D3`: the IRQ handler (`actor::raster`'s 0-13).
    pub handler: u8,
    /// `$D4`: which IRQ of the handler's chain runs next.
    pub next_irq: u8,
}

/// How the player arrives in a room (its screen record's handler pointer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrival {
    /// As he was (`$DA0E`, and the empty record).
    Unchanged,
    /// Held (`$DA0F`): motion `Held`, the stage script `$E0`.
    Held,
    /// Flying (`$D9FF`): motion `Flying`, the stage script `$A0`.
    Flying,
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
    /// `$C024[area]`), the start record and the screen record (bank 14 `$DA18[area]`).
    /// `None` where a pointer leaves the PRG, a screen record's flag byte is not 0 or `$80`,
    /// or its handler pointer is not one of the three arrivals.
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
        let screen = pointer(14, 0xDA18, 14)?;
        let screen = |i: u16| byte(14, screen.checked_add(i)?);
        let flag = |i: u16| match screen(i)? {
            0 => Some(false),
            0x80 => Some(true),
            _ => None,
        };
        let empty = screen(0)? == 0x77;
        let (mut fixed_rows, mut fixed_quads) = ([false; 16], [false; 8]);
        let (raster, arrival) = if empty {
            (None, Arrival::Unchanged)
        } else {
            for (i, fixed) in (5..).zip(&mut fixed_rows) {
                *fixed = flag(i)?;
            }
            for (i, fixed) in (21..).zip(&mut fixed_quads) {
                *fixed = flag(i)?;
            }
            let raster = Raster {
                armed: flag(0)?,
                first_irq_cycles: u16::from_le_bytes([screen(1)?, screen(2)?]),
                handler: screen(3)?,
                next_irq: screen(4)?,
            };
            let arrival = match u16::from_le_bytes([screen(29)?, screen(30)?]) {
                0xDA0E => Arrival::Unchanged,
                0xDA0F => Arrival::Held,
                0xD9FF => Arrival::Flying,
                _ => return None,
            };
            (Some(raster), arrival)
        };
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
            raster,
            fixed_rows,
            fixed_quads,
            arrival,
        })
    }
}

/// Every room the per-area tables list, as `(area, room)`: the tables (bank 14 from `$DA18`)
/// are contiguous, so each area's rooms run to the next table, the last to the first record.
#[cfg(test)]
pub(crate) fn listed_rooms(prg: &[u8]) -> Vec<(u8, u8)> {
    let word = |addr: u16| {
        let bytes = bank_slice(prg, 14, addr & 0x1FFF).expect("bank 14");
        u16::from_le_bytes([bytes[0], bytes[1]])
    };
    let areas = (word(0xDA18) - 0xDA18) / 2;
    let tables: Vec<u16> = (0..areas).map(|area| word(0xDA18 + 2 * area)).collect();
    let first_record = tables
        .iter()
        .map(|&table| word(table))
        .min()
        .expect("a table");
    let mut rooms = Vec::new();
    for (area, &table) in (0u8..).step_by(2).zip(&tables) {
        let end = tables
            .iter()
            .copied()
            .filter(|&other| other > table)
            .min()
            .unwrap_or(first_record);
        let count = usize::from((end - table) / 2);
        rooms.extend((0u8..).step_by(2).take(count).map(|room| (area, room)));
    }
    rooms
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every room the screen tables list imports: their flag bytes are 0 or `$80` and their
    /// arrivals one of the three.
    #[test]
    fn every_listed_room_imports() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let prg = &rom.prg;
        let rooms = listed_rooms(prg);
        let mut empty = 0;
        for &(area, room) in &rooms {
            let layout =
                Room::import(prg, area, room).unwrap_or_else(|| panic!("area {area} room {room}"));
            if layout.raster.is_none() {
                assert_eq!(layout.arrival, Arrival::Unchanged);
                assert_eq!(
                    (layout.fixed_rows, layout.fixed_quads),
                    ([false; 16], [false; 8])
                );
                empty += 1;
            }
        }
        assert_eq!((rooms.len(), empty), (36, 22));
    }
}
