//! The FME-7 raster IRQs (bank 15 `$E2A9`, a handler per `$D3` through `$E2CA`) and the
//! picture they make. The NMI arms the first IRQ `$D1:$D2` cycles after it starts when
//! `$D0` bit 7 is set (`$E1E3`); each handler, chosen by `$D4`, writes the PPU or the
//! mapper, changes its RAM and arms the next with a counter of its own (`$E1C0`,
//! `$98AD`). [`lines`] places every write on the scanline and dot it reaches and builds
//! the 240 lines the PPU draws with, by the same rules as the oracle's
//! (`tools/rotj-oracle` `Machine::run_frame`); [`run_irqs`] makes the RAM changes.
//!
//! The cycle at which each IRQ starts and each write lands is the oracle's, measured over
//! the TAS (2026-10-16): the first IRQ `FIRST[$D3]` cycles past its counter, each next
//! the counter plus the handler's own overhead after the one before, each write a fixed
//! offset into its IRQ (`$2005` pairs mostly at +86/+92). An IRQ waits for the
//! instruction it interrupts, so the real ones wander by 10-20 cycles.

use super::Game;
use super::scroll::Scroll;
use crate::render::Line;

/// A register an IRQ writes, by the write that changes what the PPU draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reg {
    /// `$2000`.
    Ctrl(u8),
    /// `$2001`.
    Mask(u8),
    /// `$2005`'s first write (after a `$2002` read): coarse and fine X.
    ScrollX(u8),
    /// `$2005`'s second write: coarse and fine Y.
    ScrollY(u8),
    /// `$2006`'s first write.
    AddrHigh(u8),
    /// `$2006`'s second write, which also sets the render address.
    AddrLow(u8),
    /// A CHR bank (FME-7 commands 0-7).
    Chr(u8, u8),
}

/// The first IRQ's start past its counter, per `$D3`: the NMI's arming write and the
/// IRQ's entry.
const FIRST: [u32; 14] = [
    198, 198, 198, 197, 198, 198, 198, 198, 198, 197, 198, 196, 197, 198,
];

/// Cycles from the frame's start to visible line 0's first dot, in dots (`run_frame`'s
/// post-render line, vblank and pre-render line).
const VISIBLE_DOT: u32 = 22 * 341;
const DOTS_PER_LINE: u32 = 341;
/// Where fceux's old core ends a line (`LINE_END_DOT` in the oracle).
const LINE_END_DOT: u32 = 256;

/// One IRQ being run: where it started and the frame's writes so far.
struct Irq<'a> {
    at: u32,
    writes: &'a mut Vec<(u32, Reg)>,
}

impl Irq<'_> {
    fn write(&mut self, offset: u32, reg: Reg) {
        self.writes.push((self.at + offset, reg));
    }

    /// `$E1B4`/`$98A1`: `$2005` = `x`, then 0, at `offsets`.
    fn scroll(&mut self, (first, second): (u32, u32), x: u8) {
        self.write(first, Reg::ScrollX(x));
        self.write(second, Reg::ScrollY(0));
    }

    /// The three CHR banks `$39-$3B` into slots 0-2, at `offsets` (`$DF79`, `$DFE4`).
    fn status_bar_banks(&mut self, offsets: [u32; 3]) {
        for (slot, offset) in (0..3).zip(offsets) {
            self.write(offset, Reg::Chr(slot, 0x39 + slot));
        }
    }
}

/// The scroll offsets most handlers write `$2005` at (`JSR $E1B4` after their dispatch).
const SCROLL: (u32, u32) = (86, 92);

/// `room[i]` plus `$B6 & mask`, the handlers' band step.
fn step(scroll: &mut Scroll, i: usize, mask: u8) {
    scroll.room[i] = scroll.room[i].wrapping_add(mask & scroll.gate);
}

/// `$DE30`/`$DFB0`'s bounce: every eighth frame (`$F1 & 7`) `room[i]` drifts by `$B6 & 1`,
/// up while `room[flag]` bit 7 is clear until it reaches `$20`, down until below `$F0`.
fn bounce(scroll: &mut Scroll, frame: u8, i: usize, flag: usize) {
    if frame.trailing_zeros() < 3 {
        return;
    }
    let delta = 1 & scroll.gate;
    let room = &mut scroll.room;
    if room[flag] & 0x80 == 0 {
        room[i] = room[i].wrapping_add(delta);
        if room[i] & 0x80 == 0 && room[i] >= 0x20 {
            room[flag] = 0x80;
        }
    } else {
        room[i] = room[i].wrapping_sub(delta);
        if room[i] & 0x80 != 0 && room[i] < 0xF0 {
            room[flag] = 0;
        }
    }
}

/// Cycles to the next IRQ: its counter (`$E1C0`, `$98AD`) and the handler's overhead.
fn next(counter: u16, overhead: u32) -> u32 {
    u32::from(counter) + overhead
}

/// The handler `$D3` runs at this IRQ (by `$D4`): its writes and RAM changes, and the
/// cycles to the next IRQ, or `None` when the frame's chain ends.
fn handler(scroll: &mut Scroll, frame: u8, irq: &mut Irq<'_>) -> Option<u32> {
    debug_assert!(
        scroll.room[3] <= 0x0D,
        "raster IRQ $D3 = {:02X} is not ported",
        scroll.room[3]
    );
    match scroll.room[3] {
        0 => bands_and_parallax(scroll, frame, irq),
        1 => three_bands(scroll, frame, irq),
        2 => {
            irq.write(62, Reg::AddrHigh(0x26));
            irq.write(70, Reg::AddrLow(0x80));
            irq.scroll((76, 80), 0);
            None
        }
        3 => boss_floor(scroll, frame, irq),
        4 => {
            irq.status_bar_banks([55, 67, 79]);
            None
        }
        5 | 7 => four_bands(scroll, irq),
        6 => swinging_row(scroll, irq),
        8 => {
            if scroll.room[4] == 0 {
                irq.scroll((85, 91), 0);
                scroll.room[4] = 1;
                return Some(next(0x2400, 166));
            }
            irq.scroll(SCROLL, scroll.scroll_x);
            [scroll.room[1], scroll.room[2]] = [0xC0, 0x39];
            scroll.room[4] = 0;
            None
        }
        9 => intro(scroll, irq),
        0x0A => cutscene(scroll, irq),
        0x0B => {
            irq.scroll(SCROLL, scroll.room[5]);
            if scroll.scroll_x < 0x41 && scroll.room[5] != 0 {
                scroll.room[5] -= 1;
            }
            None
        }
        0x0C => {
            if scroll.room[4] == 0 {
                irq.write(78, Reg::Ctrl(scroll.ppu_ctrl | 1));
                irq.scroll((95, 101), scroll.room[5]);
                scroll.room[4] = 1;
                return Some(next(0x4000, 175));
            }
            irq.write(76, Reg::Ctrl(scroll.ppu_ctrl));
            irq.write(82, Reg::Mask(0x0A));
            irq.scroll((98, 104), 0);
            scroll.room[4] = 0;
            None
        }
        0x0D => credits(scroll, irq),
        _ => None,
    }
}

/// `$D3 = 0`, bank 14 `$DE89`: two bands, then the parallax the next frame's top shows.
fn bands_and_parallax(scroll: &mut Scroll, frame: u8, irq: &mut Irq<'_>) -> Option<u32> {
    match scroll.room[4] {
        0 => {
            irq.scroll(SCROLL, scroll.room[6]);
            step(scroll, 6, 2);
            scroll.room[4] = 1;
            Some(next(0x1300, 180))
        }
        1 => {
            irq.scroll(SCROLL, scroll.room[5]);
            scroll.room[4] = 2;
            Some(next(0x28C0, 167))
        }
        _ => {
            scroll.room[5] = scroll.scroll_x;
            scroll.scroll_x = scroll.room[7];
            scroll.room[4] = 0;
            [scroll.room[1], scroll.room[2]] = [0xA0, 0x2C];
            bounce(scroll, frame, 7, 8);
            None
        }
    }
}

/// `$D3 = 1`, `$DF15`: three bands, then the parallax.
fn three_bands(scroll: &mut Scroll, frame: u8, irq: &mut Irq<'_>) -> Option<u32> {
    match scroll.room[4] {
        0 => {
            irq.scroll(SCROLL, scroll.room[6]);
            step(scroll, 6, 3);
            scroll.room[4] = 1;
            Some(next(0x0F00, 181))
        }
        1 => {
            irq.scroll(SCROLL, scroll.room[8]);
            step(scroll, 8, 6);
            scroll.room[4] = 2;
            Some(next(0x0600, 180))
        }
        2 => {
            irq.scroll(SCROLL, scroll.room[5]);
            scroll.room[4] = 3;
            Some(next(0x08C0, 167))
        }
        _ => {
            scroll.room[5] = scroll.scroll_x;
            scroll.scroll_x = scroll.room[7];
            scroll.room[4] = 0;
            [scroll.room[1], scroll.room[2]] = [0xA0, 0x31];
            if frame.trailing_zeros() >= 3 {
                step(scroll, 7, 1);
            }
            None
        }
    }
}

/// `$D3 = 3`, `$DFDE`: the stage 1 boss: a band, the floor, the status bar's banks.
fn boss_floor(scroll: &mut Scroll, frame: u8, irq: &mut Irq<'_>) -> Option<u32> {
    match scroll.room[4] {
        0 => {
            irq.scroll(SCROLL, scroll.room[6]);
            step(scroll, 6, 4);
            scroll.room[4] = 1;
            Some(next(0x1400, 180))
        }
        1 => {
            irq.scroll((85, 91), 0);
            scroll.room[4] = 2;
            Some(next(0x1C20, 166))
        }
        _ => {
            irq.status_bar_banks([81, 93, 105]);
            scroll.scroll_x = scroll.room[5];
            scroll.room[4] = 0;
            [scroll.room[1], scroll.room[2]] = [0xA0, 0x2C];
            bounce(scroll, frame, 5, 8);
            None
        }
    }
}

/// `$D3 = 5`, `$E076`, and 7, `$E163`: four bands, the last the playfield's.
fn four_bands(scroll: &mut Scroll, irq: &mut Irq<'_>) -> Option<u32> {
    let five = scroll.room[3] == 5;
    match scroll.room[4] {
        0 => {
            irq.scroll(SCROLL, scroll.room[6]);
            step(scroll, 6, 4);
            scroll.room[4] = 1;
            Some(next(if five { 0x0D00 } else { 0x0C00 }, 180))
        }
        1 => {
            irq.scroll(SCROLL, scroll.room[7]);
            step(scroll, 7, 1);
            scroll.room[4] = 2;
            Some(next(if five { 0x3100 } else { 0x3800 }, 181))
        }
        2 => {
            irq.scroll(SCROLL, scroll.room[6]);
            scroll.room[4] = 3;
            Some(next(0x0A00, 167))
        }
        _ => {
            irq.scroll(SCROLL, scroll.room[5]);
            [scroll.room[1], scroll.room[2]] = if five { [0x00, 0x18] } else { [0x40, 0x16] };
            step(scroll, 5, 8);
            scroll.scroll_x = scroll.room[5];
            scroll.room[4] = 0;
            None
        }
    }
}

/// `$D3 = 6`, `$E0EA`: the bottom from nametable row `$2620`, its y swinging between
/// `$B0` and 0 (`$E0B0`; reaching `$B0` falls into the climb, as the code does).
fn swinging_row(scroll: &mut Scroll, irq: &mut Irq<'_>) -> Option<u32> {
    if scroll.room[4] == 0 {
        irq.write(79, Reg::AddrHigh(0x26));
        irq.write(85, Reg::AddrLow(0x20));
        irq.scroll((92, 98), scroll.scroll_x);
        scroll.room[4] = 1;
        return Some(next(0x0700, 167));
    }
    scroll.ppu_ctrl |= 2;
    let delta = 1 & scroll.gate;
    let room = &mut scroll.room;
    let mut climbing = room[6] & 0x80 != 0;
    if !climbing {
        room[5] = room[5].wrapping_sub(delta);
        if room[5] == 0xB0 {
            room[6] = 0x80;
            climbing = true;
        }
    }
    if climbing {
        room[5] = room[5].wrapping_add(delta);
        if room[5] == 0 {
            room[6] = 0;
        }
    }
    scroll.scroll_y = room[5].wrapping_add(0x50);
    room[4] = 0;
    None
}

/// `$D3 = 9`, bank 2 `$95DA`: the intro's ten bands, sliding in (`$96A1`).
fn intro(scroll: &mut Scroll, irq: &mut Irq<'_>) -> Option<u32> {
    let phase = scroll.room[4];
    if phase >= 9 {
        irq.write(78, Reg::Ctrl(scroll.ppu_ctrl | 1));
        irq.scroll((94, 100), 0);
        let room = &mut scroll.room;
        room[4] = 0;
        for (band, step) in [0x20u8, 0x28, 0x40, 0x50, 0x58].into_iter().enumerate() {
            let bit = 1 << band;
            if room[10] & bit != 0 {
                continue;
            }
            let (value, wrapped) = room[5 + band].overflowing_add(step);
            room[5 + band] = if wrapped { 0 } else { value };
            if wrapped {
                room[10] |= bit;
            }
        }
        return None;
    }
    // Bands 1-4 down and back up (`$9615-$9669`), the first and last at `$D5`.
    let (band, ctrl_at, scroll_at, counter, overhead) = match phase {
        0 => (0u8, 81, (114, 120), 0x0480, 193),
        1 | 7 => (1, 83, (100, 106), 0x0300, 180),
        2 | 6 => (2, 85, (102, 108), 0x0300, 182),
        3 | 5 => (3, 87, (104, 110), 0x0300, 184),
        4 => (4, 89, (106, 112), 0x0300, 189),
        _ => (0, 81, (98, 104), 0x0600, 177),
    };
    let nametable = (scroll.room[10] >> band) & 1;
    irq.write(ctrl_at, Reg::Ctrl(nametable | scroll.ppu_ctrl));
    if phase == 0 {
        irq.write(91, Reg::AddrHigh(0x22));
        irq.write(97, Reg::AddrLow(0x40));
    }
    irq.scroll(scroll_at, scroll.room[5 + usize::from(band)]);
    scroll.room[4] += 1;
    Some(next(counter, overhead))
}

/// `$D3 = $0A`, bank 2 `$9742`: the cutscene's two bands sliding home (`$977D`).
fn cutscene(scroll: &mut Scroll, irq: &mut Irq<'_>) -> Option<u32> {
    match scroll.room[4] {
        0 => {
            irq.write(81, Reg::Ctrl((scroll.room[10] & 1) | scroll.ppu_ctrl));
            irq.scroll((98, 104), scroll.room[5]);
            scroll.room[4] = 1;
            Some(next(0x1000, 180))
        }
        1 => {
            irq.write(
                83,
                Reg::Ctrl(((scroll.room[10] >> 1) & 1) | scroll.ppu_ctrl),
            );
            irq.scroll((100, 106), scroll.room[6]);
            scroll.room[4] = 2;
            Some(next(0x1000, 182))
        }
        _ => {
            irq.write(78, Reg::Ctrl(scroll.ppu_ctrl & 0xFE));
            irq.scroll((94, 100), 0);
            let room = &mut scroll.room;
            room[4] = 0;
            if room[10] & 1 != 0 || room[5] != 0 {
                room[10] &= 0xFE;
                room[5] = room[5].wrapping_sub(8);
            } else if room[10] & 2 != 0 || room[6] != 0 {
                let (value, wrapped) = room[6].overflowing_add(8);
                room[6] = value;
                if wrapped {
                    room[10] &= 0xFD;
                }
            }
            None
        }
    }
}

/// `$D3 = $0D`, bank 2 `$9831`: the credits' parallax, its three speeds (`$985B`).
fn credits(scroll: &mut Scroll, irq: &mut Irq<'_>) -> Option<u32> {
    match scroll.room[4] {
        0 | 2 => {
            irq.scroll(SCROLL, scroll.room[8]);
            scroll.room[4] += 1;
            Some(next(0x1000, 166))
        }
        1 => {
            irq.scroll(SCROLL, scroll.room[10]);
            scroll.room[4] += 1;
            Some(next(0x0700, 168))
        }
        _ => {
            irq.scroll((85, 91), 0);
            let room = &mut scroll.room;
            room[4] = 0;
            for (at, step) in [(5, 0x80u16), (7, 0x100), (9, 0x40)] {
                let value = u16::from_le_bytes([room[at], room[at + 1]]).wrapping_add(step);
                [room[at], room[at + 1]] = value.to_le_bytes();
            }
            scroll.scroll_x = room[6];
            None
        }
    }
}

/// The frame's raster IRQs from the NMI's arming on: their writes, by CPU cycle since the
/// frame began, and their RAM changes to `scroll`. `frame` is `$F1`.
pub(crate) fn chain(scroll: &mut Scroll, frame: u8) -> Vec<(u32, Reg)> {
    let mut writes = Vec::new();
    let d3 = usize::from(scroll.room[3]);
    let mut at = u32::from(u16::from_le_bytes([scroll.room[1], scroll.room[2]]))
        + FIRST.get(d3).copied().unwrap_or(FIRST[0]);
    for _ in 0..16 {
        let mut irq = Irq {
            at,
            writes: &mut writes,
        };
        match handler(scroll, frame, &mut irq) {
            Some(gap) => at += gap,
            None => break,
        }
    }
    writes
}

/// The frame's raster IRQs' RAM changes; the caller runs it when the NMI armed them
/// (`$D0` bit 7).
pub fn run_irqs(game: &mut Game) {
    let frame = game.world.frame;
    chain(&mut game.scroll, frame);
}

/// The PPU's scroll and banks as the CPU sets them.
#[derive(Clone, Copy)]
struct Regs {
    t: u16,
    v: u16,
    fine_x: u8,
    ctrl: u8,
    mask: u8,
    chr: [u8; 8],
    mirroring: u8,
}

impl Regs {
    fn apply(&mut self, reg: Reg) {
        match reg {
            Reg::Ctrl(value) => {
                self.ctrl = value;
                self.t = (self.t & !0x0C00) | (u16::from(value & 3) << 10);
            }
            Reg::Mask(value) => self.mask = value,
            Reg::ScrollX(value) => {
                self.t = (self.t & !0x001F) | u16::from(value >> 3);
                self.fine_x = value & 7;
            }
            Reg::ScrollY(value) => {
                self.t = (self.t & !0x73E0)
                    | (u16::from(value & 7) << 12)
                    | (u16::from(value & 0xF8) << 2);
            }
            Reg::AddrHigh(value) => self.t = (self.t & 0x00FF) | (u16::from(value & 0x3F) << 8),
            Reg::AddrLow(value) => {
                self.t = (self.t & 0xFF00) | u16::from(value);
                self.v = self.t;
            }
            Reg::Chr(slot, bank) => self.chr[usize::from(slot)] = bank,
        }
    }

    fn line(&self, sprite_chr: [u8; 8]) -> Line {
        Line {
            v: self.v,
            fine_x: self.fine_x,
            ctrl: self.ctrl,
            mask: self.mask,
            chr: self.chr,
            sprite_chr,
            mirroring: self.mirroring,
        }
    }

    /// Dot 256: fine y down a line (into the next nametable row at 30 tiles), the
    /// horizontal scroll from `t`.
    fn end_line(&mut self) {
        if self.mask & 0x18 == 0 {
            return;
        }
        let mut v = self.v;
        if v & 0x7000 == 0x7000 {
            v &= !0x7000;
            let coarse_y = (v >> 5) & 0x1F;
            v = match coarse_y {
                29 => (v & !0x03E0) ^ 0x0800,
                31 => v & !0x03E0,
                _ => (v & !0x03E0) | ((coarse_y + 1) << 5),
            };
        } else {
            v += 0x1000;
        }
        self.v = (v & !0x041F) | (self.t & 0x041F);
    }
}

/// What an NMI wrote to the PPU and the mapper for the frame after it: the scroll
/// (`$FD`, `$FC`, `$FF`), mask (`$FE`) and mirroring (`$EC`) shadows, the CHR banks
/// (`$E0-$E7`), and whether it armed the raster IRQs (`$D0` bit 7).
#[derive(Clone, Debug)]
pub struct Registers {
    scroll: Scroll,
    chr: [u8; 8],
}

impl Registers {
    /// `game`'s, as its NMI just left them.
    #[must_use]
    pub fn of(game: &Game) -> Registers {
        Registers {
            scroll: game.scroll.clone(),
            chr: game.oam.chr,
        }
    }
}

/// The 240 lines the PPU draws a frame with: its NMI's `registers`, changed by the
/// raster IRQs at the scanline and dot each write reaches. The IRQs run during the
/// frame's loop and read the RAM it has written by then, so they run from `game` after
/// that loop (`$D3` 6 and 8 read the scroll the loop just moved, a pause stops the bands
/// at once).
#[must_use]
pub fn lines(registers: &Registers, game: &Game) -> Vec<Line> {
    let scroll = &registers.scroll;
    let (x, y) = (scroll.scroll_x, scroll.scroll_y);
    let t = (u16::from(scroll.ppu_ctrl & 3) << 10)
        | (u16::from(y & 7) << 12)
        | (u16::from(y >> 3) << 5)
        | u16::from(x >> 3);
    let mut regs = Regs {
        t,
        v: t,
        fine_x: x & 7,
        ctrl: scroll.ppu_ctrl,
        mask: scroll.mask,
        chr: registers.chr,
        mirroring: scroll.mirroring & 3,
    };
    let writes = if scroll.room[0] & 0x80 == 0 {
        Vec::new()
    } else {
        chain(&mut game.scroll.clone(), game.world.frame)
    };
    let mut writes = writes
        .into_iter()
        .map(|(cycle, reg)| (i64::from(cycle) * 3 - i64::from(VISIBLE_DOT), reg))
        .peekable();
    let mut apply_until = |regs: &mut Regs, dot: i64| {
        while let Some((_, reg)) = writes.next_if(|&(at, _)| at < dot) {
            regs.apply(reg);
        }
    };
    apply_until(&mut regs, 0);
    if regs.mask & 0x18 != 0 {
        regs.v = regs.t;
    }
    let mut lines = Vec::with_capacity(240);
    let mut sprite_chr = regs.chr;
    for line in 0..240 {
        lines.push(regs.line(sprite_chr));
        let start = i64::from(line * DOTS_PER_LINE);
        apply_until(&mut regs, start + i64::from(LINE_END_DOT));
        sprite_chr = regs.chr;
        regs.end_line();
        apply_until(&mut regs, start + i64::from(DOTS_PER_LINE));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The handler code's writes the chain models: the PPU's and the FME-7's CHR banks.
    fn handler_write(w: &rotj_oracle::WriteEvent) -> bool {
        (0xDDF4..0xE1D0).contains(&w.pc) || (w.bank == Some(2) && (0x95C9..0x9900).contains(&w.pc))
    }

    /// An IRQ's register writes as [`Reg`]s, by cycle (`$2005`/`$2006` pairs follow a
    /// `$2002` read; `$8000` picks the FME-7 command `$A000` sets).
    fn regs(writes: &[rotj_oracle::WriteEvent]) -> Vec<(u32, Reg)> {
        let (mut out, mut second, mut command) = (Vec::new(), false, 0xFF);
        for w in writes {
            let reg = match w.addr {
                0x2000 => Reg::Ctrl(w.value),
                0x2001 => Reg::Mask(w.value),
                0x2005 => {
                    second = !second;
                    if second {
                        Reg::ScrollX(w.value)
                    } else {
                        Reg::ScrollY(w.value)
                    }
                }
                0x2006 => {
                    second = !second;
                    if second {
                        Reg::AddrHigh(w.value)
                    } else {
                        Reg::AddrLow(w.value)
                    }
                }
                0x8000..=0x9FFF => {
                    command = w.value & 0x0F;
                    continue;
                }
                0xA000..=0xBFFF if command < 8 => Reg::Chr(command, w.value),
                _ => continue,
            };
            out.push((w.cycle, reg));
        }
        out
    }

    /// Every frame of the TAS whose NMI armed the raster IRQs: from the oracle's RAM at
    /// the first IRQ, the port's chain makes the same writes in the same order, each
    /// within the IRQs' jitter of the real one, and leaves the bytes the handlers own as
    /// the last IRQ left them.
    #[test]
    #[ignore = "the whole TAS (release, 20 s); the gates run it in release with --ignored"]
    fn the_chain_is_the_games_over_the_tas() {
        let Some(mut m) = rotj_oracle::power_on() else {
            return;
        };
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/emu/tas/inputs.csv");
        let inputs = rotj_oracle::read_inputs(&path);
        m.bus.trace.peeks = vec![0xE2A9, 0xE2A8];
        m.bus.trace.watch = vec![0x2000..=0x2007, 0x8000..=0xBFFF];
        let (mut frames, mut by_d3, mut worst) = (0, BTreeMap::<u8, usize>::new(), 0i64);
        for frame in 1..=48100usize {
            m.run_frame(inputs.get(frame).copied().unwrap_or(0));
            let peeked = std::mem::take(&mut m.bus.trace.peeked);
            let writes: Vec<_> = m.bus.trace.writes.drain(..).filter(handler_write).collect();
            let Some(first) = peeked.iter().position(|p| p.pc == 0xE2A9) else {
                continue;
            };
            let last_entry = peeked
                .iter()
                .rposition(|p| p.pc == 0xE2A9)
                .expect("an entry");
            let exit = peeked[last_entry..]
                .iter()
                .find(|p| p.pc == 0xE2A8)
                .expect("its RTI");
            let entry = &peeked[first];
            if peeked[first..=last_entry]
                .iter()
                .any(|p| p.ram[0x400] != entry.ram[0x400])
            {
                continue;
            }
            let mut scroll = Scroll::lift(&entry.ram);
            let port = chain(&mut scroll, entry.ram[0xF1]);
            let mut ram = *entry.ram;
            scroll.project(&mut ram);
            for addr in [
                0xD1, 0xD2, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xFC, 0xFD, 0xFF,
            ] {
                assert_eq!(
                    ram[addr], exit.ram[addr],
                    "frame {frame}: ${addr:04X} after $D3 = {:02X}'s IRQs",
                    entry.ram[0xD3]
                );
            }
            let game = regs(&writes);
            let (ours, theirs): (Vec<Reg>, Vec<Reg>) = (
                port.iter().map(|w| w.1).collect(),
                game.iter().map(|w| w.1).collect(),
            );
            let n = theirs.len().min(ours.len());
            assert_eq!(
                ours[..n],
                theirs[..n],
                "frame {frame}: $D3 = {:02X}'s writes",
                entry.ram[0xD3]
            );
            for (p, g) in port.iter().zip(&game) {
                let off = i64::from(p.0) - i64::from(g.0);
                worst = worst.max(off.abs());
                assert!(
                    off.abs() <= 30,
                    "frame {frame}: {:?} at cycle {} where the game's was at {}",
                    p.1,
                    p.0,
                    g.0
                );
            }
            frames += 1;
            *by_d3.entry(entry.ram[0xD3]).or_default() += 1;
        }
        println!("{frames} frames of IRQs; by $D3 {by_d3:02X?}; worst write offset {worst} cycles");
        assert!(frames > 30_000, "{frames}");
    }
}
