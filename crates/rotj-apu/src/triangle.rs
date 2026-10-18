//! 2A03 triangle channel core (nesdev, "APU Triangle").
//!
//! Register-write API mirroring the hardware map: [`Triangle::write_ctrl`]
//! for `$4008`, [`Triangle::write_timer_lo`]/[`Triangle::write_timer_hi`]
//! for `$400A`/`$400B`, [`Triangle::set_enabled`] for `$4015`.
//!
//! Unlike the pulse channel, the triangle timer ticks at the CPU rate, so
//! its divider is advanced twice per [caller APU tick](Triangle::tick).

/// Linear counter control and reload value.
pub const CTRL: u16 = 0x4008;
/// Timer period, low byte.
pub const TIMER_LO: u16 = 0x400A;
/// Length-counter load and timer period, high bits.
pub const TIMER_HI: u16 = 0x400B;

/// APU triangle channel: linear counter, length counter and the looping
/// 32-step sequence, clocked by the CPU-rate timer.
#[derive(Debug, Clone)]
pub struct Triangle {
    /// Divider in CPU ticks, expiring every `t + 1` CPU ticks.
    timer: crate::timer::Timer,
    /// Index into the 32-step sequence, advancing only when both
    /// counters are non-zero (the held value keeps playing).
    sequence: u8,
    /// The DAC value the channel is currently holding. Silence (zeroed
    /// counters) freezes the sequencer here rather than emitting 0, which is
    /// what the hardware does: `level()` returns this whenever the channel is
    /// not actively stepping.
    dac: u8,
    length: crate::length_counter::LengthCounter,
    linear: LinearCounter,
}

/// Linear counter state.
#[derive(Debug, Clone, Copy, Default)]
struct LinearCounter {
    /// `$4008`-bit 7, also the length counter halt flag.
    control: bool,
    reload_flag: bool,
    reload_value: u8,
    counter: u8,
}

impl Default for Triangle {
    fn default() -> Self {
        Self::new()
    }
}

impl Triangle {
    /// nesdev, "APU Triangle": the looping 32-step waveform.
    const SEQUENCE: [u8; 32] = [
        15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11,
        12, 13, 14, 15,
    ];

    /// Power-on state (all registers zero).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            timer: crate::timer::Timer::new(0),
            sequence: 0,
            dac: 0,
            length: crate::length_counter::LengthCounter::new(),
            linear: LinearCounter {
                control: false,
                reload_flag: false,
                reload_value: 0,
                counter: 0,
            },
        }
    }

    /// `$4008`: linear counter control and reload value (`CRRR RRRR`).
    pub fn write_ctrl(&mut self, val: u8) {
        self.linear.control = (val & 0x80) != 0;
        self.linear.reload_value = val & 0x7F;
        self.length.write_ctrl(self.linear.control);
    }

    /// `$400A`: timer low 8 bits.
    pub fn write_timer_lo(&mut self, val: u8) {
        self.timer
            .set_period((self.timer.period() & 0xFF00) | u16::from(val));
    }

    /// `$400B`: length load and timer high bits; sets the linear counter
    /// reload flag (nesdev side-effects note).
    pub fn write_timer_hi(&mut self, val: u8) {
        self.length.write(val >> 3);
        self.timer
            .set_period((self.timer.period() & 0x00FF) | (u16::from(val & 0x07) << 8));
        self.linear.reload_flag = true;
    }

    /// `$4015` enable bit.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.length.set_enabled(enabled);
    }

    /// One APU tick = two CPU ticks: advances the CPU-rate timer and steps
    /// the sequencer on expiry while both counters hold.
    pub fn tick(&mut self) {
        for _ in 0..2 {
            if self.timer.tick() {
                self.step_sequencer();
            }
        }
    }

    /// Quarter-frame clock: linear counter.
    pub fn clock_quarter_frame(&mut self) {
        self.linear.clock();
        self.length.reload();
    }

    /// Half-frame clock: linear counter and length counter.
    pub fn clock_half_frame(&mut self) {
        self.linear.clock();
        self.length.clock();
        self.length.reload();
    }

    /// Raw channel level 0..=15 (the triangle has no volume envelope).
    ///
    /// While both counters hold and the timer period is below 2, this reports
    /// the low-pass-filtered average of the ultrasonic waveform (7.5 on the
    /// hardware; 7 here), per nesdev's "APU Triangle". Otherwise it reports the
    /// held DAC value, so a silenced channel keeps its last level (and stays at
    /// 0 after power-on, as a reset console is silent).
    #[must_use]
    pub fn level(&self) -> u8 {
        if self.is_active() && self.timer.period() < 2 {
            7
        } else {
            self.dac
        }
    }

    /// Whether both counters currently let the sequencer run.
    const fn is_active(&self) -> bool {
        self.length.counter() > 0 && self.linear.counter > 0
    }

    fn step_sequencer(&mut self) {
        if self.length.counter() > 0 && self.linear.counter > 0 {
            self.sequence = (self.sequence + 1) & 0x1F;
            self.dac = Self::SEQUENCE[usize::from(self.sequence)];
        }
    }
}

impl LinearCounter {
    /// One quarter-frame clock, following the nesdev action order:
    /// reload-or-decrement, then clear the reload flag unless control is
    /// set.
    fn clock(&mut self) {
        if self.reload_flag {
            self.counter = self.reload_value;
        } else if self.counter > 0 {
            self.counter -= 1;
        }
        if !self.control {
            self.reload_flag = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With both counters loaded, the sequence walks the nesdev 32-step
    /// table in order (15, 14, …, 0, 0, 1, …, 15), one step per timer
    /// expiry, and freezes when a counter runs out.
    #[test]
    fn sequence_walks_and_freezes() {
        let mut tri = Triangle::new();
        tri.set_enabled(true);
        tri.write_ctrl(0xFF); // control set, reload value 127
        tri.write_timer_lo(0x03);
        tri.write_timer_hi(0x00); // t = 3: expiry every 4 CPU ticks
        tri.clock_quarter_frame(); // linear counter reloads to 127

        // Two CPU ticks per APU tick, expiry on CPU ticks 1, 5, 9, … → one
        // step on every odd APU tick. The timer carries a fresh counter 0,
        // so tick k has advanced ceil(k / 2) steps.
        for k in 1..=63_u32 {
            tri.tick();
            assert_eq!(
                tri.level(),
                Triangle::SEQUENCE[(usize::try_from(k.div_ceil(2))).expect("small") % 32],
                "sequence position after tick {k}"
            );
        }
        assert_eq!(tri.level(), 15, "the 32-step sequence wraps");

        // Gated sequencer: the output holds its last value.
        let mut tri = Triangle::new();
        tri.set_enabled(true);
        tri.write_ctrl(0x01); // control clear, reload value 1
        tri.write_timer_lo(0x03);
        tri.write_timer_hi(0x00); // length = 10, linear reload flag set
        tri.clock_quarter_frame(); // linear counter = 1
        tri.tick();
        tri.tick();
        tri.tick(); // two steps, on ticks 1 and 3
        let held = tri.level();
        tri.clock_quarter_frame(); // the linear counter reaches 0
        tri.tick();
        tri.tick();
        tri.tick();
        assert_eq!(tri.level(), held, "an empty counter freezes the sequence");
    }

    /// Timer value 0 or 1 pins an *active* channel near the hardware
    /// low-pass average of 7.5 (nesdev, "APU Triangle"); a silenced one stays
    /// at its held DAC value (0 after power-on).
    #[test]
    fn ultrasonic_period_outputs_constant() {
        let mut tri = Triangle::new();
        assert_eq!(tri.level(), 0, "power-on is silent");
        tri.set_enabled(true);
        tri.write_ctrl(0xFF); // control set, linear reload 127
        tri.write_timer_lo(0x00);
        tri.write_timer_hi(0x08); // length load 1, timer high bits 0 -> t = 0
        tri.clock_quarter_frame(); // linear counter reloads to 127
        tri.tick();
        assert_eq!(tri.level(), 7, "an active period < 2 approximates 7.5");
    }
}
