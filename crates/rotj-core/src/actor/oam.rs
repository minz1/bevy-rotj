//! Metasprites and the OAM builder (bank 14 `$C247` and `$C145`).

use super::engine::adc_overflows;
use crate::actors::{ActorError, bank_slice};
#[cfg(test)]
use crate::ines::FME7_PRG_BANK_SIZE;

/// One 8x16 sprite of a metasprite, relative to the draw position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Piece {
    /// Tile byte, ORed with the call's tile bits.
    pub tile: u8,
    /// X offset.
    pub dx: i8,
    /// Attribute byte, ORed with the call's attribute bits.
    pub attr: u8,
    /// Y offset.
    pub dy: i8,
}

/// A metasprite: its pieces in drawing order and the CHR bank it selects (0 = none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metasprite {
    /// CHR bank written to the call's CHR shadow, or 0.
    pub chr: u8,
    /// Pieces, in the order they are drawn (last in ROM first).
    pub pieces: Vec<Piece>,
    /// The length byte is not `4k + 1`, so `$C247`'s walk never ends and the pieces repeat.
    pub endless: bool,
}

impl Metasprite {
    /// Decodes `[n][tile, x, attr, y]×k[chr]` at `addr` in `bank` (`addr & $1FFF` is the offset).
    pub fn from_rom(prg: &[u8], bank: u8, addr: u16) -> Result<Metasprite, ActorError> {
        let offset = addr & 0x1FFF;
        let rest = bank_slice(prg, bank, offset)?;
        Metasprite::from_bytes(rest).ok_or(ActorError::OutOfRange {
            bank,
            offset,
            len: prg.len(),
        })
    }

    /// Decodes `[n][tile, x, attr, y]×k[chr]` from the start of `bytes` as `$C247`
    /// walks it: a piece ending at offset `n - 1`, then every 4 bytes down until the
    /// offset reaches 0, at most one lap of 64.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Metasprite> {
        let len = *bytes.first()?;
        let at = |offset: u8| bytes.get(usize::from(offset)).copied();
        let mut pieces = Vec::new();
        let mut offset = len.wrapping_sub(1);
        loop {
            pieces.push(Piece {
                tile: at(offset.wrapping_sub(3))?,
                dx: at(offset.wrapping_sub(2))?.cast_signed(),
                attr: at(offset.wrapping_sub(1))?,
                dy: at(offset)?.cast_signed(),
            });
            offset = offset.wrapping_sub(4);
            if offset == 0 || pieces.len() == 64 {
                break;
            }
        }
        Some(Metasprite {
            chr: at(len)?,
            pieces,
            endless: offset != 0,
        })
    }
}

/// One `$C247` call: where and how a metasprite is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawCall {
    /// Screen x (`$94`).
    pub x: i16,
    /// Screen y (`$95`).
    pub y: i16,
    /// Attribute bits ORed into every piece (`$99`).
    pub attr: u8,
    /// Tile bits ORed into every piece (`$A4`).
    pub tile: u8,
    /// Which CHR shadow the metasprite's bank goes to (`$98`).
    pub chr_slot: u8,
    /// Bottom-anchored: pieces move up 16 px and those past 255 are dropped (`$9A` bit 6).
    pub bottom: bool,
}

/// The game's OAM page and its allocator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OamBuilder {
    /// The OAM page (`$0200`).
    pub page: [u8; 256],
    /// Next entry's byte offset (`$FB`).
    pub next: u8,
    /// Entries left before overflow (`$BD`).
    pub left: u8,
    /// Overflowed this frame; later calls draw nothing (`$BC`).
    pub overflow: bool,
    /// `$C247` never returns: an endless metasprite placed nothing in a lap, or kept
    /// placing on a frame that draws past a full page. The game is frozen.
    pub hung: bool,
    /// First entry of the next frame (`$0471`).
    pub start: u8,
    /// CHR bank shadows (`$E0-$E7`).
    pub chr: [u8; 8],
    /// Visible width in pixels; pieces outside `0..width` are dropped.
    pub width: i16,
}

impl Default for OamBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl OamBuilder {
    /// An empty page as the game starts a frame: entry 0 first, 63 left, 256 px wide.
    #[must_use]
    pub fn new() -> OamBuilder {
        OamBuilder {
            page: [PARKED_Y; 256],
            next: 0,
            left: ENTRIES - 1,
            overflow: false,
            hung: false,
            start: 0,
            chr: [0; 8],
            width: 256,
        }
    }

    /// Draws `sprite` (`$C247`), returning the overflow flag it leaves: `BIT $BC`, then
    /// each piece's `BIT $9A` or `SBC #$0F` for y and its `ADC $94` for x.
    pub fn draw(&mut self, sprite: &Metasprite, call: DrawCall, frame_odd: bool) -> bool {
        if self.overflow {
            return true;
        }
        let mut cpu_overflow = false;
        if sprite.chr != 0 {
            debug_assert!(call.chr_slot < 8, "chr_slot {} past $E7", call.chr_slot);
            self.chr[usize::from(call.chr_slot)] = sprite.chr;
        }
        let mut next = self.next;
        let mut lap_placed = false;
        for (n, piece) in sprite.pieces.iter().cycle().enumerate() {
            if n > 0 && n % sprite.pieces.len() == 0 {
                if !sprite.endless {
                    break;
                }
                if frame_odd || !lap_placed {
                    self.hung = true;
                    return cpu_overflow;
                }
                #[cfg(test)]
                super::oracle::seen(0xC2A3);
                lap_placed = false;
            }
            let entry = usize::from(next);
            cpu_overflow = y_overflow(call, piece.dy);
            let Some(y) = piece_y(call, piece.dy) else {
                continue;
            };
            let attr = piece.attr | call.attr;
            let dx = if attr & 0x40 == 0 {
                piece.dx
            } else {
                (!piece.dx).wrapping_sub(7)
            };
            let x = call.x + i16::from(dx);
            cpu_overflow = adc_overflows(dx.cast_unsigned(), call.x.to_le_bytes()[0], false);
            self.page[entry] = y;
            self.page[entry + 2] = attr;
            self.page[entry + 3] = x.to_le_bytes()[0];
            if !(0..self.width).contains(&x) {
                continue;
            }
            self.page[entry + 1] = piece.tile | call.tile;
            lap_placed = true;
            next = next.wrapping_add(NEXT_ENTRY);
            self.left = self.left.wrapping_sub(1);
            if self.left & 0x80 != 0 {
                cpu_overflow = false;
                if !frame_odd {
                    self.overflow = true;
                    return cpu_overflow;
                }
            }
        }
        self.next = next;
        cpu_overflow
    }

    /// Parks every entry from the first and rotates the start (`$C16F`).
    pub fn clear(&mut self) {
        self.next = 0;
        self.left = ENTRIES;
        self.overflow = false;
        self.finish();
    }

    /// Parks the entries left and rotates the start (`$C145`).
    pub fn finish(&mut self) {
        if !self.overflow && self.left != 0 {
            let mut entry = self.next;
            let mut count = self.left;
            loop {
                self.page[usize::from(entry)] = PARKED_Y;
                entry = entry.wrapping_add(NEXT_ENTRY);
                count = count.wrapping_sub(1);
                if count & 0x80 != 0 {
                    break;
                }
            }
        }
        self.overflow = false;
        self.start = self.start.wrapping_add(START_STEP);
        self.next = self.start;
        self.left = ENTRIES - 1;
    }
}

const NEXT_ENTRY: u8 = 0xC4;
const START_STEP: u8 = 0x4C;
const PARKED_Y: u8 = 0xF4;
const ENTRIES: u8 = 64;

/// The overflow flag `$C247`'s y step leaves: `BIT $9A`, then the bottom anchor's `SBC #$0F`.
fn y_overflow(call: DrawCall, dy: i8) -> bool {
    let sum = u16::from(call.y.to_le_bytes()[0]) + u16::from(dy.cast_unsigned());
    let [low, high] = sum.to_le_bytes();
    call.bottom && (high != 0 || adc_overflows(low, !0x0F, false))
}

/// A piece's OAM y, or `None` when `$C247` drops it.
fn piece_y(call: DrawCall, dy: i8) -> Option<u8> {
    let y = u8::try_from(call.y).ok()?;
    let sum = u16::from(y) + u16::from(dy.cast_unsigned());
    let [low, high] = sum.to_le_bytes();
    if call.bottom {
        (high == 0).then(|| low.wrapping_sub(16))
    } else {
        (low < y).then_some(low)
    }
}

#[cfg(test)]
impl OamBuilder {
    pub(crate) fn lift(ram: &[u8; 2048]) -> OamBuilder {
        OamBuilder {
            page: ram[0x200..0x300].try_into().expect("256 bytes"),
            next: ram[0xFB],
            left: ram[0xBD],
            overflow: ram[0xBC] & 0x80 != 0,
            hung: false,
            start: ram[0x471],
            chr: ram[0xE0..0xE8].try_into().expect("8 bytes"),
            width: 256,
        }
    }

    pub(crate) fn project(&self, ram: &mut [u8; 2048]) {
        ram[0x200..0x300].copy_from_slice(&self.page);
        ram[0xFB] = self.next;
        ram[0xBD] = self.left;
        ram[0xBC] = if self.overflow { 0xFF } else { 0 };
        ram[0x471] = self.start;
        ram[0xE0..0xE8].copy_from_slice(&self.chr);
    }
}

#[cfg(test)]
impl DrawCall {
    pub(crate) fn lift(ram: &[u8; 2048]) -> DrawCall {
        DrawCall {
            x: i16::from(ram[0x94]),
            y: i16::from(ram[0x95]),
            attr: ram[0x99],
            tile: ram[0xA4],
            chr_slot: ram[0x98],
            bottom: ram[0x9A] & 0x40 != 0,
        }
    }
}

/// The metasprite `addr` points at, with the bank the game had mapped there.
#[cfg(test)]
pub(crate) fn metasprite_at(prg: &[u8], ram: &[u8; 2048], addr: u16) -> Metasprite {
    let shadow = match addr {
        0x8000..=0x9FFF => ram[0xE9],
        0xA000..=0xBFFF => ram[0xEA],
        0xC000..=0xDFFF => 14,
        _ => 15,
    };
    let bank = usize::from(shadow & 0x3F) % (prg.len() / FME7_PRG_BANK_SIZE);
    Metasprite::from_rom(prg, u8::try_from(bank).expect("< 64"), addr).expect("metasprite")
}

#[cfg(test)]
mod tests {
    use rotj_oracle::{Machine, PeekEvent, data_path, power_on, read_inputs};

    use super::*;

    const ENTRY: u16 = 0xC247;
    const EXITS: [u16; 4] = [0xC246, 0xC2A7, 0xC2B0, 0xC2B7];
    const FINISH: u16 = 0xC145;
    const FINISHED: u16 = 0xC16E;

    fn route(to: usize, pokes: &[(u16, u16, u8)], frames: usize) -> Option<Vec<PeekEvent>> {
        let mut machine: Machine = power_on()?;
        let pads = read_inputs(&data_path("route-inputs.csv"));
        for &pad in &pads[1..to] {
            machine.run_frame(pad);
        }
        machine.bus.trace.pokes = pokes.to_vec();
        machine.bus.trace.peeks = [ENTRY, FINISH, FINISHED].into_iter().chain(EXITS).collect();
        for frame in to..to + frames {
            machine.run_frame(pads.get(frame).copied().unwrap_or(0));
        }
        Some(std::mem::take(&mut machine.bus.trace.peeked))
    }

    fn compare(port: &[u8; 2048], oracle: &PeekEvent, finish: bool) {
        let mut addrs: Vec<usize> = (0x200..0x300).chain(0xE0..0xE8).collect();
        addrs.extend([0xFB, 0xBC, 0xBD]);
        if finish {
            addrs.push(0x471);
        }
        for a in addrs {
            assert_eq!(
                port[a], oracle.ram[a],
                "${a:04X} at frame {} (pc ${:04X})",
                oracle.frame, oracle.pc
            );
        }
    }

    fn check(events: &[PeekEvent], prg: &[u8]) -> usize {
        let mut checked = 0;
        for pair in events.windows(2) {
            let (begin, end) = (&pair[0], &pair[1]);
            if begin.nmis != end.nmis {
                continue;
            }
            let frame_odd = begin.ram[0xF7] & 0x80 != 0;
            let mut builder = OamBuilder::lift(&begin.ram);
            match (begin.pc, end.pc) {
                (ENTRY, exit) if EXITS.contains(&exit) => {
                    let addr = u16::from_le_bytes([begin.ram[0x96], begin.ram[0x97]]);
                    let sprite = metasprite_at(prg, &begin.ram, addr);
                    builder.draw(&sprite, DrawCall::lift(&begin.ram), frame_odd);
                }
                (FINISH, FINISHED) => builder.finish(),
                _ => continue,
            }
            let mut ram = *begin.ram;
            builder.project(&mut ram);
            compare(&ram, end, begin.pc == FINISH);
            checked += 1;
        }
        checked
    }

    fn prg() -> Vec<u8> {
        rotj_rom::for_test().expect("ROM").prg.clone()
    }

    #[test]
    fn calls_match_the_oracle() {
        let Some(events) = route(841, &[], 159) else {
            return;
        };
        let checked = check(&events, &prg());
        assert!(checked > 1000, "{checked} calls checked");
    }

    #[test]
    fn forced_cases_match_the_oracle() {
        let cases: [&[(u16, u16, u8)]; 5] = [
            &[(ENTRY, 0x94, 0xF4)],
            &[(ENTRY, 0x95, 0x08)],
            &[(ENTRY, 0x95, 0x08), (ENTRY, 0x9A, 0x40)],
            &[(ENTRY, 0xBD, 0x02)],
            &[
                (ENTRY, 0x96, 0x20),
                (ENTRY, 0x97, 0xD0),
                (ENTRY, 0x95, 0x80),
                (ENTRY, 0xF7, 0x00),
            ],
        ];
        for pokes in cases {
            let Some(events) = route(900, pokes, 4) else {
                return;
            };
            let checked = check(&events, &prg());
            assert!(checked > 10, "{pokes:X?}: {checked} calls");
        }
        assert!(
            crate::actor::oracle::was_seen(0xC2A3),
            "no endless draw ran a second lap"
        );
    }

    #[test]
    fn from_rom_reads_n_pieces() {
        let mut prg = vec![0; 2 * FME7_PRG_BANK_SIZE];
        let at = FME7_PRG_BANK_SIZE + 0x10;
        prg[at..at + 10].copy_from_slice(&[9, 1, 0xFC, 2, 0xF0, 3, 4, 0x40, 0xE0, 0x77]);
        let sprite = Metasprite::from_rom(&prg, 1, 0xA010).expect("decodes");
        assert_eq!(sprite.chr, 0x77);
        assert_eq!(
            sprite.pieces,
            [
                Piece {
                    tile: 3,
                    dx: 4,
                    attr: 0x40,
                    dy: -32
                },
                Piece {
                    tile: 1,
                    dx: -4,
                    attr: 2,
                    dy: -16
                },
            ]
        );
        prg[at] = 8;
        let endless = Metasprite::from_rom(&prg, 1, 0xA010).expect("decodes");
        assert!(endless.endless);
        assert_eq!(endless.pieces.len(), 64);
    }

    #[test]
    fn y_overflow_edge_cases() {
        let call = |y: i16, bottom: bool| DrawCall {
            x: 0,
            y,
            attr: 0,
            tile: 0,
            chr_slot: 0,
            bottom,
        };
        // 0x7F + 1 crosses into -128; only the bottom anchor's pending SBC #$0F counts.
        assert!(!y_overflow(call(0x7F, false), 1));
        assert!(y_overflow(call(0x7F, true), 1));
        // 0x80 + 0x80 carries into the high byte, which overflows on its own.
        assert!(y_overflow(call(0x80, true), -128));
        // The bottom-anchor skip: without it the byte sum never sets overflow.
        assert!(!y_overflow(call(0x80, false), -128));
        assert!(!y_overflow(call(0x1000, false), -1));
    }
}
