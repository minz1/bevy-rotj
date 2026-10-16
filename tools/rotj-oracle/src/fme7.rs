//! Sunsoft FME-7 mapper registers.

/// FME-7 command/parameter state and IRQ counter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fme7 {
    /// Last command written to `$8000-$9FFF` (low nibble).
    pub command: u8,
    /// 1 KiB CHR banks for `$0000-$1FFF` (commands 0-7).
    pub chr: [u8; 8],
    /// Commands 8-B: `$6000` (bit 7 RAM select, bit 6 RAM enable) and `$8000/$A000/$C000`.
    pub prg: [u8; 4],
    /// Command C: 0 vertical, 1 horizontal, 2 single-screen low, 3 single-screen high.
    pub mirroring: u8,
    /// Command D: any nonzero value arms the counter (fceux).
    pub irq_control: u8,
    /// Commands E/F: the down-counter, clocked every CPU cycle.
    pub irq_counter: u16,
    /// An IRQ is asserted until the next command-D write acknowledges it.
    pub irq_pending: bool,
}

impl Default for Fme7 {
    fn default() -> Self {
        Fme7 {
            command: 0,
            chr: [0; 8],
            prg: [0; 4],
            mirroring: 0,
            irq_control: 0,
            irq_counter: 0xFFFF,
            irq_pending: false,
        }
    }
}

impl Fme7 {
    /// A write to `$8000-$BFFF`.
    pub(crate) fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFF => self.command = value & 0x0F,
            0xA000..=0xBFFF => match self.command {
                0..=7 => self.chr[usize::from(self.command)] = value,
                8..=0xB => self.prg[usize::from(self.command - 8)] = value,
                0xC => self.mirroring = value & 3,
                0xD => {
                    self.irq_control = value;
                    self.irq_pending = false;
                }
                0xE => self.irq_counter = (self.irq_counter & 0xFF00) | u16::from(value),
                _ => self.irq_counter = (self.irq_counter & 0x00FF) | (u16::from(value) << 8),
            },
            _ => {}
        }
    }

    /// PRG ROM offset of a CPU address; `$6000` is ROM unless command 8 has bit 6 set.
    #[must_use]
    pub(crate) fn prg_offset(&self, addr: u16, prg_len: usize) -> Option<usize> {
        let banks = prg_len / 0x2000;
        let bank = match addr {
            0x6000..=0x7FFF if self.prg[0] & 0x40 == 0 => usize::from(self.prg[0] & 0x3F),
            0x8000..=0xDFFF => usize::from(self.prg[usize::from((addr - 0x6000) >> 13)] & 0x3F),
            0xE000..=0xFFFF => banks - 1,
            _ => return None,
        };
        Some((bank % banks) * 0x2000 + usize::from(addr & 0x1FFF))
    }

    /// The 8 KiB PRG bank a CPU address reads from, or `None` for RAM, WRAM and I/O.
    #[must_use]
    pub(crate) fn bank_at(&self, addr: u16, prg_len: usize) -> Option<u8> {
        let offset = self.prg_offset(addr, prg_len)?;
        if (0x6000..0x8000).contains(&addr) && self.wram_enabled() {
            return None;
        }
        u8::try_from(offset / 0x2000).ok()
    }

    /// `$6000-$7FFF` is RAM and enabled.
    #[must_use]
    pub(crate) fn wram_enabled(&self) -> bool {
        self.prg[0] & 0xC0 == 0xC0
    }

    /// fceux's counter: while command D is nonzero, count down; at zero raise the IRQ and disarm.
    pub(crate) fn clock(&mut self, cycles: u32) {
        if self.irq_control == 0 {
            return;
        }
        let left = i64::from(self.irq_counter) - i64::from(cycles);
        if left > 0 {
            self.irq_counter = u16::try_from(left).expect("below the previous counter");
        } else {
            self.irq_pending = true;
            self.irq_control = 0;
            self.irq_counter = 0xFFFF;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn irq_fires_at_zero_and_disarms() {
        let mut fme7 = Fme7::default();
        assert_eq!(fme7.irq_counter, 0xFFFF);
        fme7.write(0x8000, 0x0E);
        fme7.write(0xA000, 0x02);
        fme7.write(0x8000, 0x0F);
        fme7.write(0xA000, 0x00);
        fme7.write(0x8000, 0x0D);
        fme7.write(0xA000, 0x81);
        fme7.clock(1);
        assert!(!fme7.irq_pending);
        fme7.clock(1);
        assert!(fme7.irq_pending);
        assert_eq!((fme7.irq_control, fme7.irq_counter), (0, 0xFFFF));
        fme7.clock(100);
        assert_eq!(fme7.irq_counter, 0xFFFF);
        fme7.write(0xA000, 0x00);
        assert!(!fme7.irq_pending);
    }
}
