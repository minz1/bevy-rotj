//! A room's layout as data: what the loader `$B40A` reads from the ROM on each room entry,
//! imported so the game builds the room from data (`docs/specs/2026-10-17-rooms-design.md`).

use crate::actor::families::phases;
use crate::actor::oam::attr;
use crate::actor::world::orientation;
use crate::prg;
use crate::render::Mirroring;

/// Bank 14 table of per-area room-record tables (`$C048[area]`, records in bank 0).
const ROOM_RECORD_TABLE: usize = 0xC048;
/// Bank 13 table of per-area sprite-palette tables (`$B5F5[area]`).
const SPRITE_PALETTE_TABLE: usize = 0xB5F5;
/// Bank 14 table of per-area page-order tables (`$C024[area]`, orders in bank 0).
const PAGE_ORDER_TABLE: usize = 0xC024;
/// Bank 13 table of per-area start-record tables (`$B653[area]`).
const START_RECORD_TABLE: usize = 0xB653;
/// Bank 14 table of per-area screen-record tables (`$DA18[area]`).
const SCREEN_RECORD_TABLE: usize = 0xDA18;

/// Sentinel first byte of an empty screen record (`$77`).
const EMPTY_SCREEN_SENTINEL: u8 = 0x77;
/// Screen-record offset of the 16 fixed-row flags (`$70-$7F`).
const SCREEN_FIXED_ROWS: usize = 5;
/// Screen-record offset of the 8 fixed-quad flags (`$60-$67`).
const SCREEN_FIXED_QUADS: usize = 21;
/// Screen-record offset of the 16-bit arrival handler pointer.
const SCREEN_ARRIVAL: usize = 29;
/// Arrival handler pointer: leave the player as he was (`$DA0E`).
const ARRIVAL_UNCHANGED: u16 = 0xDA0E;
/// Arrival handler pointer: held (`$DA0F`).
const ARRIVAL_HELD: u16 = 0xDA0F;
/// Arrival handler pointer: flying (`$D9FF`).
const ARRIVAL_FLYING: u16 = 0xD9FF;

/// A pattern table's four 1 KiB CHR banks (`$E0-$E3`); bit 7 marks an animation handle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChrBanks(pub [u8; 4]);

impl ChrBanks {
    /// Bit 7: this entry names a CHR animation group rather than a literal bank.
    pub const ANIMATED: u8 = 0x80;

    /// Whether a single bank byte names an animation group.
    #[must_use]
    pub const fn is_animated(bank: u8) -> bool {
        bank & Self::ANIMATED != 0
    }

    /// Raw bank byte for slot `i` (`0..4`).
    #[must_use]
    pub const fn bank(self, slot: usize) -> u8 {
        self.0[slot]
    }

    /// Whether slot `i` (`0..4`) names an animation group.
    #[must_use]
    pub const fn animated(self, slot: usize) -> bool {
        Self::is_animated(self.0[slot])
    }

    /// The underlying four bytes.
    #[must_use]
    pub const fn raw(self) -> [u8; 4] {
        self.0
    }

    /// Maps each of the four bank bytes.
    #[must_use]
    pub fn map<T>(self, f: impl FnMut(u8) -> T) -> [T; 4] {
        self.0.map(f)
    }

    /// Iterates the four bank bytes.
    pub fn iter(&self) -> std::slice::Iter<'_, u8> {
        self.0.iter()
    }
}

impl<'a> IntoIterator for &'a ChrBanks {
    type Item = &'a u8;
    type IntoIter = std::slice::Iter<'a, u8>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// A room's orientation (`$DD`, room record byte 5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Orientation {
    /// `$DD` bit 6: the room scrolls vertically.
    pub vertical: bool,
    /// `$DD` bit 7: the room is two pages across.
    pub two_pages: bool,
}

impl Orientation {
    /// Horizontal, one page tall (`$00`).
    pub const HORIZONTAL: Self = Self {
        vertical: false,
        two_pages: false,
    };
    /// Vertical, one page wide (`$40`).
    pub const VERTICAL: Self = Self {
        vertical: true,
        two_pages: false,
    };
    /// Horizontal, two pages tall (`$80`).
    pub const HORIZONTAL_TWO_PAGES: Self = Self {
        vertical: false,
        two_pages: true,
    };

    /// Decodes `$DD`; rejects bytes with bits outside `VERTICAL | TWO_PAGES`.
    #[must_use]
    pub const fn from_byte(byte: u8) -> Option<Self> {
        if byte & !(orientation::VERTICAL | orientation::TWO_PAGES) != 0 {
            return None;
        }
        Some(Self {
            vertical: byte & orientation::VERTICAL != 0,
            two_pages: byte & orientation::TWO_PAGES != 0,
        })
    }

    /// Encodes back to `$DD`.
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        (if self.vertical {
            orientation::VERTICAL
        } else {
            0
        }) | (if self.two_pages {
            orientation::TWO_PAGES
        } else {
            0
        })
    }

    /// Two pages across, scrolling sideways (`$DD` bits 7-6 = `10`).
    #[must_use]
    pub const fn horizontal_two_page(self) -> bool {
        self.two_pages && !self.vertical
    }

    /// Number of pages across the scroll axis (`1` or `2`).
    #[must_use]
    pub const fn across(self) -> usize {
        if self.two_pages { 2 } else { 1 }
    }
}

/// Initial scroll-lock mask (`$C2`, start record byte 10).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScrollLock(pub u8);

impl ScrollLock {
    /// The raw `$C2` byte.
    #[must_use]
    pub const fn raw(self) -> u8 {
        self.0
    }
}

/// Initial scroll-mode byte (`$C3`, start record byte 11).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScrollMode(pub u8);

impl ScrollMode {
    /// The raw `$C3` byte.
    #[must_use]
    pub const fn raw(self) -> u8 {
        self.0
    }
}

/// Initial player facing / attribute byte (`$0470`, start record byte 14).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Facing(pub u8);

impl Facing {
    /// Facing right (`$00`).
    pub const RIGHT: Self = Self(0);
    /// Facing left (`attr::FLIP_X`).
    pub const LEFT: Self = Self(attr::FLIP_X);

    /// The raw `$0470` byte.
    #[must_use]
    pub const fn raw(self) -> u8 {
        self.0
    }

    /// Whether bit 6 (`attr::FLIP_X`) is set.
    #[must_use]
    pub const fn is_left(self) -> bool {
        self.0 & attr::FLIP_X != 0
    }
}

/// One room's layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Room {
    /// The background palette selector (room record byte 0).
    pub background_palette: u8,
    /// The sprite palette selector (bank 13 `$B5F5[area]`, by `room >> 1`).
    pub sprite_palette: u8,
    /// The CHR banks for `$E0-$E3` (record bytes 1-4); bit 7 names an animation group.
    pub chr: ChrBanks,
    /// The orientation byte `$DD` (record byte 5).
    pub orientation: Orientation,
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Raster {
    /// `$D0` bit 7: the NMI arms the first IRQ.
    pub armed: bool,
    /// `$D1:$D2`: the CPU cycles to the first IRQ.
    pub first_irq_cycles: u16,
    /// `$D3`: the IRQ handlers the frame runs.
    pub chain: Chain,
    /// `$D4`: the index of the IRQ the chain runs next, back to 0 when the chain ends.
    pub next_irq: u8,
}

phases! {
    /// A raster split's IRQ handlers (`$D3`, by bank 15 `$E2CA`), by what they draw.
    #[derive(Default)]
    pub Chain at "$D3", chain_bytes_round_trip {
        /// Two bands, then the parallax the next frame's top shows (bank 14 `$DE89`, area `$02`).
        #[default]
        TwoBands = 0,
        /// Three bands, then the parallax (`$DF15`, area `$10`).
        ThreeBands = 1,
        /// The bottom from nametable row `$2680` (area `$12`).
        FixedRow = 2,
        /// The stage 1 boss: a band, the floor, the status bar's banks (`$DFDE`, areas `$00`, `$04`).
        BossFloor = 3,
        /// The boss bar's CHR banks (areas `$0E`, `$14`, `$1E`, `$22`).
        BossBar = 4,
        /// Four bands, the last the playfield's (`$E076`, area `$08`).
        FourBands = 5,
        /// The bottom from nametable row `$2620`, its y swinging (`$E0EA`, area `$0C`).
        SwingingRow = 6,
        /// [`Chain::FourBands`] with a shorter top and inner band, a taller middle (`$E163`, area `$18`).
        FourBandsTall = 7,
        /// The top at x 0, the rest the playfield (area `$1C`).
        FixedTop = 8,
        /// The intro's ten bands sliding in (bank 2 `$95DA`).
        Intro = 9,
        /// The cutscene's two bands sliding home (bank 2 `$9742`).
        Cutscene = 0x0A,
        /// One band closing to x 0 while the camera is left of `$41`; nothing sets it.
        Closing = 0x0B,
        /// The ending title's two nametables (bank 2 `$97DD`).
        EndingTitle = 0x0C,
        /// The credits' parallax (bank 2 `$9831`).
        Credits = 0x0D,
    }
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
    /// The scroll locks (`$C2`).
    pub scroll_lock: ScrollLock,
    /// The scroll mode (`$C3`, the auto-scroll bits).
    pub scroll_mode: ScrollMode,
    /// The nametable mirroring (`$EC`).
    pub mirroring: Mirroring,
    /// The first spawn schedule column (`$0403`).
    pub first_spawn_column: u8,
    /// The player's facing (`$0470`).
    pub facing: Facing,
}

/// Decoded bank-14 screen record (`$DA18[area]`).
struct ScreenRecord {
    raster: Option<Raster>,
    fixed_rows: [bool; 16],
    fixed_quads: [bool; 8],
    arrival: Arrival,
}

impl ScreenRecord {
    fn import(prg: &[u8], addr: usize) -> Option<Self> {
        let byte = |i: usize| prg::byte(prg, 14, addr.checked_add(i)?);
        let flag = |i: usize| match byte(i)? {
            0 => Some(false),
            0x80 => Some(true),
            _ => None,
        };
        if byte(0)? == EMPTY_SCREEN_SENTINEL {
            return Some(Self {
                raster: None,
                fixed_rows: [false; 16],
                fixed_quads: [false; 8],
                arrival: Arrival::Unchanged,
            });
        }
        let mut fixed_rows = [false; 16];
        for (i, fixed) in (SCREEN_FIXED_ROWS..).zip(&mut fixed_rows) {
            *fixed = flag(i)?;
        }
        let mut fixed_quads = [false; 8];
        for (i, fixed) in (SCREEN_FIXED_QUADS..).zip(&mut fixed_quads) {
            *fixed = flag(i)?;
        }
        let raster = Raster {
            armed: flag(0)?,
            first_irq_cycles: prg::word(prg, 14, addr.checked_add(1)?)?,
            chain: Chain::from_byte(byte(3)?)?,
            next_irq: byte(4)?,
        };
        let arrival = match prg::word(prg, 14, addr.checked_add(SCREEN_ARRIVAL)?)? {
            ARRIVAL_UNCHANGED => Arrival::Unchanged,
            ARRIVAL_HELD => Arrival::Held,
            ARRIVAL_FLYING => Arrival::Flying,
            _ => return None,
        };
        Some(Self {
            raster: Some(raster),
            fixed_rows,
            fixed_quads,
            arrival,
        })
    }
}

impl Room {
    /// Room `room` of area `area` (the raw `$0400`/`$0402` values, `room` already doubled),
    /// imported from the ROM: the room record (bank 0 through bank 14 `$C048[area]`), the
    /// sprite palette (bank 13 `$B5F5[area]`), the page order (bank 0 through bank 14
    /// `$C024[area]`), the start record and the screen record (bank 14 `$DA18[area]`).
    /// `None` where a pointer leaves the PRG, a screen record's flag byte is not 0 or `$80`,
    /// its `$D3` is not a [`Chain`], or its handler pointer is not one of the three arrivals.
    #[must_use]
    pub fn import(prg: &[u8], area: u8, room: u8) -> Option<Room> {
        let pointer = |bank: usize, table: usize, from_bank: usize| -> Option<usize> {
            let area_table = prg::word(prg, bank, table.checked_add(usize::from(area))?)?;
            let entry = usize::from(area_table).checked_add(usize::from(room))?;
            Some(usize::from(prg::word(prg, from_bank, entry)?))
        };
        let record = pointer(14, ROOM_RECORD_TABLE, 0)?;
        let field = |i: usize| prg::byte(prg, 0, record.checked_add(i)?);
        let sets = usize::from(prg::word(
            prg,
            13,
            SPRITE_PALETTE_TABLE + usize::from(area),
        )?);
        let page_order = pointer(14, PAGE_ORDER_TABLE, 0)?;
        let start_addr = pointer(13, START_RECORD_TABLE, 13)?;
        let start_byte = |i: usize| prg::byte(prg, 13, start_addr.checked_add(i)?);
        let start_word = |i: usize| Some(u16::from_be_bytes([start_byte(i)?, start_byte(i + 1)?]));
        let screen = ScreenRecord::import(prg, pointer(14, SCREEN_RECORD_TABLE, 14)?)?;
        let mut pages = [0; 32];
        for (i, page) in (0..).zip(&mut pages) {
            *page = prg::byte(prg, 0, page_order.checked_add(i)?)? & 0x0F;
        }
        Some(Room {
            background_palette: field(0)?,
            sprite_palette: prg::byte(prg, 13, sets.checked_add(usize::from(room >> 1))?)?,
            chr: ChrBanks([field(1)?, field(2)?, field(3)?, field(4)?]),
            orientation: Orientation::from_byte(field(5)?)?,
            raised_page_count: field(6)?,
            pages,
            start: Start {
                player_x: start_word(0)?,
                player_y: start_word(2)?,
                camera_x: start_word(4)?,
                camera_y: start_word(6)?,
                ring_y: start_word(8)?,
                scroll_lock: ScrollLock(start_byte(10)?),
                scroll_mode: ScrollMode(start_byte(11)?),
                mirroring: Mirroring::from_register(start_byte(12)?),
                first_spawn_column: start_byte(13)?,
                facing: Facing(start_byte(14)?),
            },
            raster: screen.raster,
            fixed_rows: screen.fixed_rows,
            fixed_quads: screen.fixed_quads,
            arrival: screen.arrival,
        })
    }
}

/// Area 0's table (`$DA3C`) runs into the one at `$DA3E`, so the gap rule finds 1 of its 3
/// rooms; the tapes load rooms 2 and 4.
const OVERLAPPED: [(u8, usize); 1] = [(0, 3)];

/// Every room the per-area tables list, as `(area, room)`: each area's rooms run to the next
/// table, the last to the first record, except where [`OVERLAPPED`] names the count.
pub(crate) fn listed_rooms(prg: &[u8]) -> Vec<(u8, u8)> {
    try_listed_rooms(prg).unwrap_or_default()
}

fn try_listed_rooms(prg: &[u8]) -> Option<Vec<(u8, u8)>> {
    let word = |addr: usize| prg::word(prg, 14, addr);
    let table_base = u16::try_from(SCREEN_RECORD_TABLE).expect("screen table fits u16");
    let areas = word(SCREEN_RECORD_TABLE)?.checked_sub(table_base)? / 2;
    let tables: Vec<u16> = (0..areas)
        .map(|area| word(SCREEN_RECORD_TABLE.checked_add(2 * usize::from(area))?))
        .collect::<Option<_>>()?;
    let first_record = tables
        .iter()
        .map(|&table| word(usize::from(table)))
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .min()?;
    let mut rooms = Vec::new();
    for (area, &table) in (0u8..).step_by(2).zip(&tables) {
        let end = tables
            .iter()
            .copied()
            .filter(|&other| other > table)
            .min()
            .unwrap_or(first_record);
        let count = match OVERLAPPED
            .iter()
            .find(|&&(overlapped, _)| overlapped == area)
        {
            Some(&(_, count)) => count,
            None => usize::from(end.checked_sub(table)? / 2),
        };
        rooms.extend((0u8..).step_by(2).take(count).map(|room| (area, room)));
    }
    Some(rooms)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every room the screen tables list imports: their flag bytes are 0 or `$80`, their
    /// `$D3` a [`Chain`] and their arrivals one of the three.
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
        assert_eq!((rooms.len(), empty), (38, 24));
    }
}
