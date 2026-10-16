//! 2A03 noise channel core (nesdev, "APU Noise").
//!
//! Register-write API mirroring the hardware map: [`Noise::write_ctrl`] for
//! `$400C`, [`Noise::write_timer`] for `$400E`, [`Noise::write_length`] for
//! `$400F`, [`Noise::set_enabled`] for `$4015`.

/// 15-bit LFSR feedback mode: mode 1 XORs bits 0 and 6, mode 0 bits 0 and 1
/// (nesdev, "APU Noise").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShiftMode {
    /// `$400E`-bit 7 clear: 32767-step cycle.
    #[default]
    Mode0,
    /// `$400E`-bit 7 set: randomly 93- or 31-step cycle.
    Mode1,
}

/// APU noise channel: 15-bit linear-feedback shift register behind an
/// envelope and length counter gate.
#[derive(Debug, Clone)]
pub struct Noise {
    timer: crate::timer::Timer,
    /// Power-on value 1 (nesdev, "APU Noise").
    shift: u16,
    mode: ShiftMode,
    length: crate::length_counter::LengthCounter,
    envelope: crate::envelope::Envelope,
}

impl Default for Noise {
    fn default() -> Self {
        Self::new()
    }
}

impl Noise {
    /// `$400E`-bits 3..0 period table, NTSC, in CPU cycles between shift
    /// register clocks.
    const PERIOD_TABLE_NTSC: [u16; 16] = [
        4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
    ];

    /// Power-on state (shift register 1, period entry 0).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            timer: crate::timer::Timer::new(Self::PERIOD_TABLE_NTSC[0] / 2 - 1),
            shift: 1,
            mode: ShiftMode::Mode0,
            length: crate::length_counter::LengthCounter::new(),
            envelope: crate::envelope::Envelope::new(),
        }
    }

    /// `$400C`: halt and envelope parameters.
    pub fn write_ctrl(&mut self, val: u8) {
        self.length.write_ctrl((val & 0x20) != 0);
        self.envelope.write_ctrl(val);
    }

    /// `$400E`: mode flag and period entry.
    pub fn write_timer(&mut self, val: u8) {
        self.timer
            .set_period(Self::PERIOD_TABLE_NTSC[usize::from(val & 0x0F)] / 2 - 1);
        self.mode = if (val & 0x80) != 0 {
            ShiftMode::Mode1
        } else {
            ShiftMode::Mode0
        };
    }

    /// `$400F`: length load; restarts the envelope (nesdev side effect).
    pub fn write_length(&mut self, val: u8) {
        self.length.write(val >> 3);
        self.envelope.restart();
    }

    /// `$4015` enable bit.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.length.set_enabled(enabled);
    }

    /// One APU tick: shifts the register on timer expiry.
    pub fn tick(&mut self) {
        if self.timer.tick() {
            self.step_shift();
        }
    }

    /// Quarter-frame clock: envelope.
    pub fn clock_quarter_frame(&mut self) {
        self.envelope.clock();
        self.length.reload();
    }

    /// Half-frame clock: envelope and length counter.
    pub fn clock_half_frame(&mut self) {
        self.envelope.clock();
        self.length.clock();
        self.length.reload();
    }

    /// Raw channel level 0..=15.
    ///
    /// The mixer receives the envelope volume unless bit 0 of the shift
    /// register is set or the length counter has expired (nesdev, "APU
    /// Noise").
    #[must_use]
    pub fn level(&self) -> u8 {
        if self.shift & 0x01 == 0x01 || self.length.counter() == 0 {
            0
        } else {
            self.envelope.volume()
        }
    }

    /// nesdev action order at a shift clock: feedback, shift right, write
    /// feedback to bit 14.
    fn step_shift(&mut self) {
        let other_bit = if self.mode == ShiftMode::Mode1 { 6 } else { 1 };
        let feedback = (self.shift ^ (self.shift >> other_bit)) & 0x01;
        self.shift >>= 1;
        self.shift |= feedback << 14;
    }

    /// Current shift register contents.
    #[must_use]
    pub const fn shift(&self) -> u16 {
        self.shift
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// nesdev table bounds: entries run 4..4068 CPU cycles, and
    /// `$400E` stores them as (CPU_cycles / 2) - 1 in an APU-tick divider
    /// (every table entry is even).
    #[test]
    fn period_table_bounds() {
        assert_eq!(Noise::PERIOD_TABLE_NTSC[0], 4);
        assert_eq!(Noise::PERIOD_TABLE_NTSC[15], 4068);
        let mut noise = Noise::new();
        noise.write_timer(0x80);
        assert_eq!(noise.timer.period(), 4 / 2 - 1, "fastest period, mode 1");
        noise.write_timer(0x0F);
        assert_eq!(noise.timer.period(), 4068 / 2 - 1, "slowest period");
    }

    /// nesdev action order, checked on a known first steps plus
    /// structural anchors: the mode-0 register cycles through all 32767
    /// non-zero states, and the mode-1 cycle divides 93 steps.
    #[test]
    fn shift_register_sequence() {
        let mut noise = Noise::new();
        assert_eq!(noise.shift(), 1, "power-up value");

        // First expiry: feedback = bit0 XOR bit1 of 1 → 1, written to
        // bit 14.
        noise.tick();
        assert_eq!(noise.shift(), 0x4000, "mode 0 first step");
        // Run down the zero half of the register, then wrap through bit 0
        // and back into bit 14: (1 >> 1) has the low bit live.
        for expected in [
            0x2000_u16, 0x1000, 0x0800, 0x0400, 0x0200, 0x0100, 0x0080, 0x0040, 0x0020, 0x0010,
            0x0008, 0x0004, 0x0002, 0x4001, 0x6000, 0x3000,
        ] {
            noise.tick();
            noise.tick(); // interval 4 CPU cycles = 2 APU ticks
            assert_eq!(noise.shift(), expected, "mode 0 walk");
        }

        // Structural: returns to the power-up state after 32767 shifts.
        for _ in 0..32767 - 17 {
            noise.tick();
            noise.tick();
        }
        assert_eq!(noise.shift(), 1, "mode 0 cycles over all 32767 states");

        // Mode 1: the cycle divides 93 steps.
        noise.write_timer(0x80); // mode 1, period entry 0
        let at_start = noise.shift();
        let mut seen = vec![at_start];
        for _ in 0..93 {
            noise.tick();
            noise.tick();
            let shift = noise.shift();
            assert_ne!(shift, 0, "the LFSR never reaches the all-zero state");
            seen.push(shift);
        }
        assert_eq!(noise.shift(), at_start, "the mode-1 cycle divides 93 steps");
        assert!(seen.contains(&(0x0001)), "the cycle revisits state 1");
    }
}
