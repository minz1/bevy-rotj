//! Internal length counter shared by pulse, triangle and noise.
//!
//! Two buffering rules negotiated with Tetanes (the nesdev text doesn't spell
//! either out):
//!
//! 1. The halt flag written to `$4000`-family bits 5 is re-buffered into
//!    `new_halt` and only transferred at the next length-counter clock.
//! 2. A length reload pending from `$4003`-family writes is merged at the
//!    next clock, but only if the counter hasn't counted down since the
//!    write (Tetanes keeps `previous_counter` for this check).

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct LengthCounter {
    enabled: bool,
    halt: bool,
    new_halt: bool,
    counter: u8,
    counter_at_write: u8,
    reload: u8,
}

impl LengthCounter {
    /// nesdev, "APU Length Counter" — the 32-entry load table.
    const LENGTH_TABLE: [u8; 32] = [
        10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96,
        22, 192, 24, 72, 26, 16, 28, 32, 30,
    ];

    pub(super) const fn new() -> Self {
        Self {
            enabled: false,
            halt: false,
            new_halt: false,
            counter: 0,
            counter_at_write: 0,
            reload: 0,
        }
    }

    /// Halt-length bit (`$4000`-bit 5, `$4008`-bit 7, `$400C`-bit 5).
    pub(super) fn write_ctrl(&mut self, halt: bool) {
        self.new_halt = halt;
    }

    /// `$4003`-family bits 7..3: load LENGTH_TABLE entry `index`.
    pub(super) fn write(&mut self, index: u8) {
        if self.enabled {
            self.reload = Self::LENGTH_TABLE[usize::from(index & 0x1F)];
            self.counter_at_write = self.counter;
        }
    }

    /// Merges a pending length load and the buffered halt flag. Called after
    /// every frame-counter event, mirroring Tetanes `apu.rs:458-478`.
    pub(super) fn reload(&mut self) {
        if self.reload > 0 && self.counter == self.counter_at_write {
            self.counter = self.reload;
        }
        self.reload = 0;
        self.halt = self.new_halt;
    }

    /// `$4015` enable bit: disabling zeroes the counter and locks it out.
    pub(super) fn set_enabled(&mut self, enabled: bool) {
        if !enabled {
            self.counter = 0;
        }
        self.enabled = enabled;
    }

    /// One half-frame clock when the halt flag is clear.
    pub(super) fn clock(&mut self) {
        if self.counter > 0 && !self.halt {
            self.counter -= 1;
        }
    }

    pub(super) const fn counter(self) -> u8 {
        self.counter
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The re-buffered halt rule: a halt flag written into the register
    /// takes effect at the length counter's next clock, but that clock
    /// itself still decrements (the value merged at the same moment, from a
    /// pending `$4003` load, only lands if the counter hasn't counted
    /// down since the write).
    #[test]
    fn halt_is_buffered_until_the_next_clock() {
        let mut len = LengthCounter::new();
        len.set_enabled(true);
        len.write(1); // table entry 1 = 254

        // Disable halt buffering: first clock keeps counting.
        len.reload(); // clears halt from new_halt (still false)
        len.clock();
        assert_eq!(len.counter(), 253, "unhalted counter decrements");

        // Now write halt. It is buffered: the very next clock still
        // decrements because the halt flag hasn't been merged yet.
        len.write_ctrl(true);
        len.clock();
        assert_eq!(len.counter(), 252, "buffered halt not yet in effect");

        // After the frame-event merge, the counter holds.
        len.reload();
        len.clock();
        len.clock();
        assert_eq!(len.counter(), 252, "halted counter holds");

        // A pending reload merges only if the counter hasn't counted down.
        let mut len = LengthCounter::new();
        len.set_enabled(true);
        len.write(1); // 254 pending
        len.reload();
        assert_eq!(len.counter(), 254, "pending load lands on merge");
        len.clock();
        len.write(2); // 20 pending, counter_at_write = 253
        len.reload();
        assert_eq!(len.counter(), 20, "intact merge still applies");
        len.clock(); // 19
        len.write(3); // 2 pending, counter_at_write = 19
        len.clock(); // 18
        len.reload();
        assert_eq!(len.counter(), 18, "stale pending load is discarded");
    }
}
