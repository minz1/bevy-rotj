//! Internal envelope unit shared by pulse and noise (nesdev, "APU Envelope").

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Envelope {
    start: bool,
    loop_enable: bool,
    constant_volume: bool,
    /// Divider period VVVV from `$4000`-bits 3..0 / `$400C`-bits 3..0.
    period: u8,
    divider: u8,
    counter: u8,
}

impl Envelope {
    pub(super) const fn new() -> Self {
        Self {
            start: false,
            loop_enable: false,
            constant_volume: false,
            period: 0,
            divider: 0,
            counter: 0,
        }
    }

    /// `$4000`/$4004`/`$400C` bits 5..0: halt/loop, constant volume, period.
    pub(super) fn write_ctrl(&mut self, val: u8) {
        self.loop_enable = (val & 0x20) != 0;
        self.constant_volume = (val & 0x10) != 0;
        self.period = val & 0x0F;
    }

    /// Side effect of `$4003`/$4007`/`$400F`: restart the envelope.
    pub(super) fn restart(&mut self) {
        self.start = true;
    }

    /// One quarter-frame clock.
    pub(super) fn clock(&mut self) {
        if self.start {
            self.start = false;
            self.counter = 15;
            self.divider = self.period;
        } else if self.divider > 0 {
            self.divider -= 1;
        } else {
            self.divider = self.period;
            if self.counter > 0 {
                self.counter -= 1;
            } else if self.loop_enable {
                self.counter = 15;
            }
        }
    }

    /// Volume currently sent to the mixer (decay level or constant period).
    pub(super) const fn volume(self) -> u8 {
        if self.constant_volume {
            self.period
        } else {
            self.counter
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// nesdev "APU Envelope" clock order: a restart (`$4003`-family side
    /// effect) loads 15 and the divider immediately on the next quarter
    /// frame; decay then proceeds one level every `period + 1` quarter
    /// frames (period 5 in this test).
    #[test]
    fn restart_and_decay_rhythm() {
        let mut env = Envelope::new();
        env.write_ctrl(0x05); // loop clear, constant volume clear, period 5
        env.restart();

        env.clock();
        assert_eq!(env.volume(), 15, "restart resets to 15");
        // The divider runs 5 quarters; the decay level drops on the
        // divider's reload: six quarters per level.
        for _ in 0..6 {
            env.clock();
        }
        assert_eq!(env.volume(), 14);
        for _ in 0..6 {
            env.clock();
        }
        assert_eq!(env.volume(), 13);
    }
}
