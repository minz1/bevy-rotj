//! Register-level PPU: `$2000-$2007` semantics and PPU memory, no rendering.

use crate::fme7::Fme7;

/// PPU state reachable from the CPU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ppu {
    /// `$2000`.
    pub ctrl: u8,
    /// `$2001`.
    pub mask: u8,
    /// `$2002` bits 5-7.
    pub status: u8,
    /// `$2003`.
    pub oam_addr: u8,
    /// Object attribute memory.
    pub oam: [u8; 256],
    /// 2 KiB nametable RAM.
    pub vram: [u8; 2048],
    /// Palette RAM.
    pub palette: [u8; 32],
    /// Current VRAM address.
    pub v: u16,
    /// Temporary VRAM address.
    pub t: u16,
    /// Fine X scroll.
    pub x: u8,
    /// `$2005/$2006` write toggle.
    pub w: bool,
    /// `$2007` read buffer.
    pub buffer: u8,
    pub(crate) nmi_pending: bool,
    /// From the pre-render line to vblank, when fceux's renderer moves the VRAM address.
    pub(crate) visible: bool,
    /// Last value driven onto the PPU data bus (fceux `PPUGenLatch`).
    pub latch: u8,
    /// The VRAM address the renderer scrolls by, kept apart from `v` (which this model
    /// does not move while rendering): set from `t` on the pre-render line and by a second
    /// `$2006` write, stepped down a line and given `t`'s horizontal bits after each line.
    pub render_v: u16,
    /// The visible line being drawn with rendering on, if any (set by the machine).
    pub(crate) drawing: Option<u8>,
    /// Writes the line model does not represent: OAM, VRAM or palette written while a
    /// line is drawn, or `$2001`'s colour emphasis bits set. The picture tests require 0.
    pub unmodelled: u32,
}

/// What the PPU draws one scanline with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Line {
    /// The VRAM address of the line's first tile: coarse x and y, nametable, fine y.
    pub v: u16,
    /// Fine X scroll.
    pub fine_x: u8,
    /// `$2000`.
    pub ctrl: u8,
    /// `$2001`.
    pub mask: u8,
    /// The FME-7's eight 1 KiB CHR banks.
    pub chr: [u8; 8],
    /// The CHR banks the line's sprites were fetched with, in the previous line's hblank.
    pub sprite_chr: [u8; 8],
    /// The FME-7's mirroring (0 vertical, 1 horizontal, 2/3 single-screen).
    pub mirroring: u8,
}

impl Line {
    /// Equal but for the render address, which moves every line anyway.
    #[must_use]
    pub fn same_but_scroll(&self, other: &Line) -> bool {
        Line {
            v: other.v,
            sprite_chr: other.sprite_chr,
            ..*self
        } == *other
    }
}

impl Default for Ppu {
    fn default() -> Self {
        Ppu {
            ctrl: 0,
            mask: 0,
            status: 0,
            oam_addr: 0,
            oam: [0; 256],
            vram: [0; 2048],
            palette: [0; 32],
            v: 0,
            t: 0,
            x: 0,
            w: false,
            buffer: 0,
            nmi_pending: false,
            visible: false,
            latch: 0,
            render_v: 0,
            drawing: None,
            unmodelled: 0,
        }
    }
}

impl Ppu {
    /// Reads a register (`addr` already folded to `$2000-$2007`).
    pub(crate) fn read_register(&mut self, addr: u16, chr: &[u8], fme7: &Fme7) -> u8 {
        match addr & 7 {
            2 => {
                let value = self.status | (self.latch & 0x1F);
                self.status &= !0x80;
                self.w = false;
                self.latch = value;
                value
            }
            7 => {
                let at = self.v & 0x3FFF;
                let value = if at >= 0x3F00 {
                    self.buffer = self.peek(at - 0x1000, chr, fme7);
                    self.palette[palette_index(at)]
                } else {
                    let value = self.buffer;
                    self.latch = value;
                    self.buffer = self.peek(at, chr, fme7);
                    value
                };
                self.advance();
                value
            }
            // fceux's old core returns `PPUGenLatch` for `$2004` and the write-only registers.
            _ => self.latch,
        }
    }

    /// Writes a register (`addr` already folded to `$2000-$2007`).
    pub(crate) fn write_register(&mut self, addr: u16, value: u8, fme7: &Fme7) {
        self.latch = value;
        match addr & 7 {
            0 => {
                if value & 0x80 != 0 && self.ctrl & 0x80 == 0 && self.status & 0x80 != 0 {
                    self.nmi_pending = true;
                }
                self.ctrl = value;
                self.t = (self.t & !0x0C00) | (u16::from(value & 3) << 10);
            }
            1 => {
                if value & 0xE0 != 0 {
                    self.unmodelled += 1;
                }
                self.mask = value;
            }
            3 => self.oam_addr = value,
            4 => {
                self.unmodelled += u32::from(self.drawing.is_some());
                self.oam[usize::from(self.oam_addr)] = value;
                self.oam_addr = self.oam_addr.wrapping_add(1);
            }
            5 => {
                if self.w {
                    self.t = (self.t & !0x73E0)
                        | (u16::from(value & 7) << 12)
                        | (u16::from(value & 0xF8) << 2);
                } else {
                    self.t = (self.t & !0x001F) | u16::from(value >> 3);
                    self.x = value & 7;
                }
                self.w = !self.w;
            }
            6 => {
                if self.w {
                    self.t = (self.t & 0xFF00) | u16::from(value);
                    self.v = self.t;
                    self.render_v = self.t;
                } else {
                    self.t = (self.t & 0x00FF) | (u16::from(value & 0x3F) << 8);
                }
                self.w = !self.w;
            }
            7 => {
                self.unmodelled += u32::from(self.drawing.is_some());
                let at = self.v & 0x3FFF;
                match at {
                    0x0000..=0x1FFF => {}
                    0x2000..=0x3EFF => self.vram[nametable_index(at, fme7.mirroring)] = value,
                    _ => self.palette[palette_index(at)] = value & 0x3F,
                }
                self.advance();
            }
            _ => {}
        }
    }

    /// fceux's vblank start: flag on, OAM address reset.
    pub(crate) fn set_vblank(&mut self) {
        self.status |= 0x80;
        self.oam_addr = 0;
        self.visible = false;
    }

    /// Pre-render line: vblank, sprite-0 and overflow flags off.
    pub(crate) fn clear_vblank(&mut self) {
        self.status &= !0xE0;
        self.visible = true;
    }

    /// Whether a `$2007` read now depends on the renderer's VRAM address, which fceux's
    /// old core moves line by line and this model does not.
    pub(crate) fn renders(&self) -> bool {
        self.visible && self.mask & 0x18 != 0
    }

    /// The pre-render line's copy of `t` into the render address, when rendering is on.
    pub(crate) fn start_render(&mut self) {
        if self.mask & 0x18 != 0 {
            self.render_v = self.t;
        }
    }

    /// The line this scanline draws with, given the mapper's state.
    pub(crate) fn line(&self, fme7: &Fme7) -> Line {
        Line {
            v: self.render_v,
            fine_x: self.x,
            ctrl: self.ctrl,
            mask: self.mask,
            chr: fme7.chr,
            sprite_chr: fme7.chr,
            mirroring: fme7.mirroring,
        }
    }

    /// The end of a rendered line: fine y steps down (wrapping into the next nametable row
    /// at 30 tiles), and the horizontal scroll reloads from `t`.
    pub(crate) fn end_line(&mut self) {
        if self.mask & 0x18 == 0 {
            return;
        }
        let mut v = self.render_v;
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
        self.render_v = (v & !0x041F) | (self.t & 0x041F);
    }

    fn advance(&mut self) {
        let step = if self.ctrl & 0x04 == 0 { 1 } else { 32 };
        self.v = self.v.wrapping_add(step) & 0x7FFF;
    }

    fn peek(&self, at: u16, chr: &[u8], fme7: &Fme7) -> u8 {
        if at < 0x2000 {
            if chr.is_empty() {
                return 0;
            }
            let bank = usize::from(fme7.chr[usize::from(at >> 10)]);
            chr[(bank * 0x400 + usize::from(at & 0x3FF)) % chr.len()]
        } else {
            self.vram[nametable_index(at, fme7.mirroring)]
        }
    }
}

fn nametable_index(at: u16, mirroring: u8) -> usize {
    let table = (at >> 10) & 3;
    let physical = match mirroring {
        0 => table & 1,
        1 => table >> 1,
        2 => 0,
        _ => 1,
    };
    usize::from(physical * 0x400 + (at & 0x3FF))
}

fn palette_index(at: u16) -> usize {
    let index = usize::from(at & 0x1F);
    if index & 0x13 == 0x10 {
        index & !0x10
    } else {
        index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Rig {
        ppu: Ppu,
        fme7: Fme7,
        chr: Vec<u8>,
    }

    impl Rig {
        fn new() -> Rig {
            Rig {
                ppu: Ppu::default(),
                fme7: Fme7::default(),
                chr: vec![0; 0x40000],
            }
        }
        fn write(&mut self, addr: u16, value: u8) {
            self.ppu.write_register(addr, value, &self.fme7);
        }
        fn read(&mut self, addr: u16) -> u8 {
            self.ppu.read_register(addr, &self.chr, &self.fme7)
        }
        fn seek(&mut self, addr: u16) {
            let [lo, hi] = addr.to_le_bytes();
            self.write(0x2006, hi);
            self.write(0x2006, lo);
        }
    }

    #[test]
    fn vram_write_increments_1_or_32() {
        let mut rig = Rig::new();
        rig.seek(0x2000);
        rig.write(0x2007, 0xAA);
        rig.write(0x2007, 0xBB);
        assert_eq!(
            (rig.ppu.vram[0], rig.ppu.vram[1], rig.ppu.v),
            (0xAA, 0xBB, 0x2002)
        );
        rig.write(0x2000, 0x04);
        rig.seek(0x2000);
        rig.write(0x2007, 0xCC);
        rig.write(0x2007, 0xDD);
        assert_eq!(
            (rig.ppu.vram[0], rig.ppu.vram[32], rig.ppu.v),
            (0xCC, 0xDD, 0x2040)
        );
    }

    #[test]
    fn ppudata_read_is_buffered() {
        let mut rig = Rig::new();
        rig.ppu.vram[5] = 0x11;
        rig.ppu.vram[6] = 0x22;
        rig.seek(0x2005);
        assert_eq!(rig.read(0x2007), 0x00);
        assert_eq!(rig.read(0x2007), 0x11);
        assert_eq!(rig.read(0x2007), 0x22);
    }

    #[test]
    fn palette_read_is_not_buffered() {
        let mut rig = Rig::new();
        rig.seek(0x3F01);
        rig.write(0x2007, 0x2A);
        rig.seek(0x3F01);
        assert_eq!(rig.read(0x2007), 0x2A);
    }

    #[test]
    fn palette_mirrors_3f10_to_3f00() {
        let mut rig = Rig::new();
        rig.seek(0x3F10);
        rig.write(0x2007, 0x15);
        rig.seek(0x3F14);
        rig.write(0x2007, 0x16);
        assert_eq!((rig.ppu.palette[0x00], rig.ppu.palette[0x04]), (0x15, 0x16));
        rig.seek(0x3F00);
        assert_eq!(rig.read(0x2007), 0x15);
    }

    #[test]
    fn status_read_clears_vblank_and_toggle() {
        let mut rig = Rig::new();
        rig.ppu.set_vblank();
        rig.write(0x2006, 0x21);
        assert!(rig.ppu.w);
        assert_eq!(rig.read(0x2002) & 0x80, 0x80);
        assert_eq!(rig.ppu.status & 0x80, 0);
        assert!(!rig.ppu.w);
        assert_eq!(rig.read(0x2002) & 0x80, 0);
    }

    #[test]
    fn write_only_registers_and_oam_data_read_the_latch() {
        let mut rig = Rig::new();
        rig.write(0x2003, 0x40);
        rig.write(0x2004, 0x5A);
        for register in [0x2000, 0x2001, 0x2003, 0x2004, 0x2005, 0x2006] {
            assert_eq!(rig.read(register), 0x5A, "${register:04X}");
        }
        rig.ppu.set_vblank();
        assert_eq!(rig.read(0x2002), 0x80 | 0x1A);
        assert_eq!(rig.read(0x2000), 0x9A);
    }

    #[test]
    fn nmi_enable_during_vblank_fires() {
        let mut rig = Rig::new();
        rig.ppu.set_vblank();
        assert!(!rig.ppu.nmi_pending);
        rig.write(0x2000, 0x80);
        assert!(rig.ppu.nmi_pending);
    }

    #[test]
    fn chr_read_goes_through_fme7_banks() {
        let mut rig = Rig::new();
        rig.fme7.chr[0] = 5;
        rig.chr[5 * 1024] = 0x77;
        rig.seek(0x0000);
        rig.read(0x2007);
        assert_eq!(rig.read(0x2007), 0x77);
    }

    #[test]
    fn mirroring_modes() {
        for (mode, expected) in [(0, (3, 4)), (1, (2, 4)), (2, (4, 0)), (3, (0, 4))] {
            let mut rig = Rig::new();
            rig.fme7.mirroring = mode;
            for (table, value) in [(0x2000, 1), (0x2400, 2), (0x2800, 3), (0x2C00, 4)] {
                rig.seek(table);
                rig.write(0x2007, value);
            }
            assert_eq!(
                (rig.ppu.vram[0], rig.ppu.vram[0x400]),
                expected,
                "mode {mode}"
            );
        }
    }

    #[test]
    fn vblank_start_resets_the_oam_address() {
        let mut rig = Rig::new();
        rig.write(0x2003, 0x40);
        rig.ppu.set_vblank();
        assert_eq!((rig.ppu.oam_addr, rig.ppu.status & 0x80), (0, 0x80));
    }
}
