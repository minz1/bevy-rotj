//! Metasprites and the OAM builder (bank 14 `$C247` and `$C145`).

use super::engine::adc_overflows;
use crate::actors::{ActorError, bank_slice};
#[cfg(test)]
use crate::ines::FME7_PRG_BANK_SIZE;
use crate::render::{Line, ctrl, mask};

/// The bits of an OAM attribute byte.
pub mod attr {
    /// The sprite is mirrored horizontally.
    pub const FLIP_X: u8 = 0x40;
    /// The sprite is mirrored vertically.
    pub const FLIP_Y: u8 = 0x80;
    /// The sprite is drawn behind the background's opaque pixels.
    pub const BEHIND: u8 = 0x20;
    /// The sprite palette (0-3).
    pub const PALETTE: u8 = 0x03;
}

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

/// Which of the game's drawers placed an OAM entry: metadata beside the page, not RAM, so a
/// renderer can show the game's own UI sprites another way and place the rest by who drew them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mark {
    /// Anything no drawer below claims.
    #[default]
    Scene,
    /// The actor in this main slot (0-23).
    Actor(u8),
    /// The second-class actor in this slot (0-7, the game's 24-31).
    Second(u8),
    /// Batman.
    Player,
    /// The projectile in this slot (0-15).
    Projectile(u8),
    /// The HUD's energy and meter gauges.
    Hud,
    /// The HUD's L/E label, drawn on odd frames.
    Label,
    /// The pause card.
    Pause,
    /// The title's cursor.
    Title,
    /// The password screen's entry or its rejection.
    Password,
}

impl Mark {
    /// One of the game's own UI drawers, which a renderer may show as UI.
    #[must_use]
    pub fn is_ui(self) -> bool {
        matches!(
            self,
            Mark::Hud | Mark::Label | Mark::Pause | Mark::Title | Mark::Password
        )
    }
}

/// One entry of the OAM page, `[y, tile, attribute, x]`, which the PPU shows as an 8x16 sprite.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct OamEntry {
    /// OAM y, one less than the sprite's top line.
    pub y: u8,
    /// Tile byte.
    pub tile: u8,
    /// Attribute byte.
    pub attr: u8,
    /// OAM x.
    pub x: u8,
}

impl OamEntry {
    /// An OAM y below every line the PPU draws.
    pub const PARKED_Y: u8 = 0xF4;

    /// Whether the sprite is above the last line the PPU draws.
    #[must_use]
    pub fn is_visible(self) -> bool {
        self.y < 0xEF
    }
}

impl From<[u8; 4]> for OamEntry {
    fn from([y, tile, attr, x]: [u8; 4]) -> OamEntry {
        OamEntry { y, tile, attr, x }
    }
}

/// One draw the builder took, as it was asked, before any piece was clipped or dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawRecord {
    /// Who drew it.
    pub mark: Mark,
    /// Where and how.
    pub call: DrawCall,
    /// What: the decoded metasprite.
    pub sprite: Metasprite,
}

impl DrawRecord {
    /// The OAM entries (`[y, tile, attribute, x]`) the record's pieces become on a page `width`
    /// pixels wide, without the allocator: one lap of an endless metasprite, nothing dropped.
    pub fn sprites(&self, width: i16) -> impl Iterator<Item = OamEntry> + '_ {
        self.sprite.pieces.iter().filter_map(move |&piece| {
            let placed = place(self.call, piece)?;
            (0..width).contains(&placed.x).then(|| placed.entry())
        })
    }
}

/// Where one piece of a draw lands: its OAM y, tile, attribute and screen x, the x before
/// the visible width is checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    /// OAM y.
    pub y: u8,
    /// The piece's tile ORed with the call's.
    pub tile: u8,
    /// The piece's attribute ORed with the call's.
    pub attr: u8,
    /// Screen x, which may be off the page.
    pub x: i16,
}

impl Placed {
    /// The OAM entry's four bytes.
    #[must_use]
    pub fn entry(self) -> OamEntry {
        [self.y, self.tile, self.attr, self.x.to_le_bytes()[0]].into()
    }
}

/// Where `$C247` places `piece` of `call`, or `None` when its y drops it.
#[must_use]
pub fn place(call: DrawCall, piece: Piece) -> Option<Placed> {
    let y = piece_y(call, piece.dy)?;
    let attr = piece.attr | call.attr;
    Some(Placed {
        y,
        tile: piece.tile | call.tile,
        attr,
        x: call.x + i16::from(piece_dx(attr, piece.dx)),
    })
}

/// A piece's x offset, mirrored when its attribute flips it.
fn piece_dx(attr: u8, dx: i8) -> i8 {
    if attr & attr::FLIP_X == 0 {
        dx
    } else {
        (!dx).wrapping_sub(7)
    }
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
    /// The drawer that last wrote each entry.
    pub marks: [Mark; 64],
    /// The drawer drawing now.
    pub marking: Mark,
    /// This frame's draws so far.
    pub records: Vec<DrawRecord>,
    /// The draws of the page last finished.
    pub shown: Vec<DrawRecord>,
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
            page: [OamEntry::PARKED_Y; 256],
            next: 0,
            left: ENTRIES - 1,
            overflow: false,
            hung: false,
            start: 0,
            chr: [0; 8],
            width: 256,
            marks: [Mark::Scene; 64],
            marking: Mark::Scene,
            records: Vec::new(),
            shown: Vec::new(),
        }
    }

    /// Draws `sprite` (`$C247`), returning the overflow flag it leaves: `BIT $BC`, then
    /// each piece's `BIT $9A` or `SBC #$0F` for y and its `ADC $94` for x.
    pub fn draw(&mut self, sprite: &Metasprite, call: DrawCall, frame_odd: bool) -> bool {
        self.records.push(DrawRecord {
            mark: self.marking,
            call,
            sprite: sprite.clone(),
        });
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
            let at = usize::from(next);
            cpu_overflow = y_overflow(call, piece.dy);
            let Some(placed) = place(call, *piece) else {
                continue;
            };
            let dx = piece_dx(placed.attr, piece.dx);
            cpu_overflow = adc_overflows(dx.cast_unsigned(), call.x.to_le_bytes()[0], false);
            let entry = placed.entry();
            self.page[at] = entry.y;
            self.marks[at >> 2] = self.marking;
            self.page[at + 2] = entry.attr;
            self.page[at + 3] = entry.x;
            if !(0..self.width).contains(&placed.x) {
                continue;
            }
            self.page[at + 1] = entry.tile;
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
                self.page[usize::from(entry)] = OamEntry::PARKED_Y;
                self.marks[usize::from(entry >> 2)] = Mark::Scene;
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
        self.shown = std::mem::take(&mut self.records);
    }
}

/// Why a marked entry the PPU shows could not be shown as UI exactly as the PPU shows it,
/// if one could not: behind the background, outside 8x16 mode, across a change of sprite
/// banks, or at the sides that scrolled lines black out and `$2001`'s left-8 clip hides.
#[must_use]
pub fn undrawable(page: &[u8; 256], marks: &[Mark; 64], lines: &[Line]) -> Option<String> {
    (0..64).find_map(|entry| {
        let [y, _, attr, x] = page[entry * 4..entry * 4 + 4].try_into().expect("4 bytes");
        let top = usize::from(y) + 1;
        let first = lines.get(top)?;
        if !marks[entry].is_ui() || first.mask & mask::SPRITES == 0 {
            return None;
        }
        let rows = &lines[top..(top + 16).min(lines.len())];
        let why = if attr & attr::BEHIND != 0 {
            "behind the background"
        } else if first.ctrl & ctrl::TALL_SPRITES == 0 {
            "in 8x8 mode"
        } else if rows.iter().any(|line| line.sprite_chr != first.sprite_chr) {
            "across a sprite bank change"
        } else if !(8..=240).contains(&x) {
            "at the screen's side"
        } else {
            return None;
        };
        Some(format!(
            "{:?} entry {entry} at ({x}, {y}) {why}",
            marks[entry]
        ))
    })
}

const NEXT_ENTRY: u8 = 0xC4;
const START_STEP: u8 = 0x4C;
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
            marks: [Mark::Scene; 64],
            marking: Mark::Scene,
            records: Vec::new(),
            shown: Vec::new(),
        }
    }

    /// `records` drawn on a copy of this builder and the page finished, as the frame that
    /// took them drew it.
    pub(crate) fn replayed(&self, records: &[DrawRecord], frame_odd: bool) -> OamBuilder {
        let mut oam = self.clone();
        for record in records {
            oam.marking = record.mark;
            oam.draw(&record.sprite, record.call, frame_odd);
        }
        oam.finish();
        oam
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
            bottom: ram[0x9A] & super::engine::anchor::BOTTOM != 0,
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

    fn one_piece_at(dx: i8) -> Piece {
        Piece {
            tile: 1,
            dx,
            attr: 0,
            dy: -16,
        }
    }

    fn sprite(pieces: Vec<Piece>) -> Metasprite {
        Metasprite {
            chr: 0,
            pieces,
            endless: false,
        }
    }

    fn at(x: i16) -> DrawCall {
        DrawCall {
            x,
            y: 0x40,
            attr: 0,
            tile: 0,
            chr_slot: 0,
            bottom: false,
        }
    }

    #[test]
    fn draw_marks_the_entries_it_writes() {
        let mut oam = OamBuilder::new();
        oam.marking = Mark::Hud;
        oam.draw(
            &sprite(vec![one_piece_at(0), one_piece_at(100)]),
            at(200),
            false,
        );
        let dropped = usize::from(NEXT_ENTRY >> 2);
        assert_eq!((oam.marks[0], oam.marks[dropped]), (Mark::Hud, Mark::Hud));
        oam.marking = Mark::Scene;
        oam.finish();
        assert_eq!((oam.marks[0], oam.marks[dropped]), (Mark::Hud, Mark::Scene));
    }

    #[test]
    fn finish_keeps_drawn_marks_and_clears_parked_ones() {
        let mut oam = OamBuilder::new();
        oam.marks = [Mark::Hud; 64];
        oam.marking = Mark::Pause;
        oam.draw(&sprite(vec![one_piece_at(0)]), at(16), false);
        oam.finish();
        assert_eq!(oam.marks[0], Mark::Pause);
        assert!(oam.marks[1..].iter().all(|&mark| mark == Mark::Scene));
    }

    #[test]
    fn clear_makes_every_mark_scene() {
        let mut oam = OamBuilder::new();
        oam.marks = [Mark::Title; 64];
        oam.clear();
        assert_eq!(oam.marks, [Mark::Scene; 64]);
    }

    #[test]
    fn a_full_page_keeps_its_last_writers_marks() {
        let mut oam = OamBuilder::new();
        oam.marking = Mark::Pause;
        for _ in 0..63 {
            oam.draw(&sprite(vec![one_piece_at(0)]), at(16), false);
        }
        assert_eq!(oam.left, 0);
        let last = usize::from(oam.next >> 2);
        oam.marking = Mark::Hud;
        oam.draw(&sprite(vec![one_piece_at(100)]), at(200), false);
        oam.finish();
        assert_eq!(oam.marks[last], Mark::Hud);
    }

    #[test]
    fn a_records_sprites_are_its_placed_pieces() {
        let flipped = Piece {
            tile: 2,
            dx: 4,
            attr: attr::FLIP_X,
            dy: -16,
        };
        let record = DrawRecord {
            mark: Mark::Player,
            call: at(250),
            sprite: sprite(vec![one_piece_at(0), one_piece_at(8), flipped]),
        };
        // x 258 is off the page; the flip mirrors dx 4 to -12.
        let sprites: Vec<OamEntry> = record.sprites(256).collect();
        assert_eq!(
            sprites,
            [
                OamEntry {
                    y: 0x30,
                    tile: 1,
                    attr: 0,
                    x: 250
                },
                OamEntry {
                    y: 0x30,
                    tile: 2,
                    attr: 0x40,
                    x: 238
                }
            ]
        );
        let clipped = place(record.call, one_piece_at(8)).expect("y keeps it");
        assert_eq!(
            (clipped.x, clipped.entry()),
            (
                258,
                OamEntry {
                    y: 0x30,
                    tile: 1,
                    attr: 0,
                    x: 2
                }
            )
        );
    }

    #[test]
    fn an_endless_metasprite_gives_one_lap() {
        let endless = Metasprite {
            chr: 0,
            pieces: vec![one_piece_at(0); 64],
            endless: true,
        };
        let record = DrawRecord {
            mark: Mark::Hud,
            call: at(16),
            sprite: endless,
        };
        assert_eq!(record.sprites(256).count(), 64);
    }

    #[test]
    fn only_the_games_own_ui_drawers_are_ui() {
        let ui = [
            Mark::Hud,
            Mark::Label,
            Mark::Pause,
            Mark::Title,
            Mark::Password,
        ];
        let scene = [
            Mark::Scene,
            Mark::Actor(0),
            Mark::Second(7),
            Mark::Player,
            Mark::Projectile(15),
        ];
        assert!(ui.iter().all(|mark| mark.is_ui()));
        assert!(!scene.iter().any(|mark| mark.is_ui()));
    }

    #[test]
    fn finish_moves_the_frames_records_to_shown() {
        let mut oam = OamBuilder::new();
        oam.marking = Mark::Actor(3);
        oam.draw(&sprite(vec![one_piece_at(0)]), at(16), false);
        oam.marking = Mark::Player;
        oam.draw(&sprite(vec![one_piece_at(100)]), at(200), false);
        oam.finish();
        let marks: Vec<Mark> = oam.shown.iter().map(|record| record.mark).collect();
        assert_eq!(marks, [Mark::Actor(3), Mark::Player]);
        assert_eq!(oam.records, []);
        assert_eq!(oam.shown[1].call, at(200));
        oam.clear();
        assert_eq!(oam.shown, []);
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
