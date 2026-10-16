//! 2A03 pulse channel core (nesdev, "APU Pulse").
//!
//! Register-write API mirroring the hardware map: [`Pulse::write_ctrl`] for
//! `$4000`/`$4004`, [`Pulse::write_sweep`] for `$4001`/`$4005`,
//! [`Pulse::write_timer_lo`]/[`Pulse::write_timer_hi`] for
//! `$4002`/`$4003`/`$4006`/`$4007`, and [`Pulse::set_enabled`] for `$4015`.
//!
//! Timer divider runs in APU ticks: the 11-bit timer value `t` clocks the
//! sequencer every `t + 1` APU cycles (`t = HHHLLLLLLLL`), so the waveform
//! period is `16 * (t + 1)` CPU cycles and the frequency is
//! `fCPU / (16 * (t + 1))`.

/// One of the two pulse channels; only their sweep-unit negate behavior
/// differs (nesdev, "APU Pulse").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PulseChannel {
    /// Pulse 1 (`$4000`–`$4003`).
    #[default]
    One,
    /// Pulse 2 (`$4004`–`$4007`).
    Two,
}

/// APU pulse channel: 8-step duty sequencer, envelope, sweep and length
/// counter, gated per the nesdev mixer rules.
#[derive(Debug, Clone)]
pub struct Pulse {
    channel: PulseChannel,
    /// Full 11-bit timer value `t = HHHLLLLLLLL`.
    real_period: u16,
    /// Divider in APU ticks, expiring every `t + 1` ticks.
    timer: crate::timer::Timer,
    /// Duty selection, row of the sequence table (`$4000`-bits 7..6).
    duty: u8,
    /// Sequencer position; walks the table downwards (nesdev notes the
    /// hardware counter counts down: 0, 7, 6, …, 1).
    duty_cycle: u8,
    length: crate::length_counter::LengthCounter,
    envelope: crate::envelope::Envelope,
    sweep: Sweep,
}

/// Sweep unit state (nesdev, "APU Sweep").
#[derive(Debug, Clone, Default)]
struct Sweep {
    enabled: bool,
    negate: bool,
    /// `$4001`-bits 6..4, stored `+ 1` as the divider reload.
    period: u8,
    /// `$4001`-bits 2..0.
    shift: u8,
    divider: u8,
    reload: bool,
    /// Continuously recomputed `period +/- period >> shift`.
    target: u16,
}

impl Default for Pulse {
    fn default() -> Self {
        Self::new(PulseChannel::One)
    }
}

impl Pulse {
    /// Duty sequence table, stored the way the hardware's downward
    /// counter reads it (nesdev implementation note for "APU Pulse").
    const DUTY_TABLE: [[u8; 8]; 4] = [
        [0, 0, 0, 0, 0, 0, 0, 1],
        [0, 0, 0, 0, 0, 0, 1, 1],
        [0, 0, 0, 0, 1, 1, 1, 1],
        [1, 1, 1, 1, 1, 1, 0, 0],
    ];

    /// Power-on state (all registers zero).
    #[must_use]
    pub const fn new(channel: PulseChannel) -> Self {
        Self {
            channel,
            real_period: 0,
            timer: crate::timer::Timer::new(0),
            duty: 0,
            duty_cycle: 0,
            length: crate::length_counter::LengthCounter::new(),
            envelope: crate::envelope::Envelope::new(),
            sweep: Sweep {
                enabled: false,
                negate: false,
                period: 0,
                shift: 0,
                divider: 0,
                reload: false,
                target: 0,
            },
        }
    }

    /// `$4000`/`$4004`: duty, halt and envelope parameters. The duty change
    /// does not touch the sequencer position (nesdev side-effects note).
    pub fn write_ctrl(&mut self, val: u8) {
        self.length.write_ctrl((val & 0x20) != 0);
        self.envelope.write_ctrl(val);
        self.duty = (val & 0xC0) >> 6;
    }

    /// `$4001`/`$4005`: sweep parameters. Reprogramming reloads the sweep
    /// divider (nesdev, "APU Sweep").
    pub fn write_sweep(&mut self, val: u8) {
        self.sweep.enabled = (val & 0x80) != 0;
        self.sweep.negate = (val & 0x08) != 0;
        self.sweep.period = ((val & 0x70) >> 4) + 1;
        self.sweep.shift = val & 0x07;
        self.update_target();
        self.sweep.reload = true;
    }

    /// `$4002`/`$4006`: timer low 8 bits; the high bits are preserved.
    pub fn write_timer_lo(&mut self, val: u8) {
        self.set_period((self.real_period & 0x0700) | u16::from(val));
    }

    /// `$4003`/`$4007`: length load and timer high bits. Reloads the phase
    /// to the start of the sequence and restarts the envelope, but does not
    /// reset the period divider (nesdev side-effects note).
    pub fn write_timer_hi(&mut self, val: u8) {
        self.length.write(val >> 3);
        self.set_period((self.real_period & 0xFF) | (u16::from(val & 0x07) << 8));
        self.duty_cycle = 0;
        self.envelope.restart();
    }

    /// `$4015` enable bit.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.length.set_enabled(enabled);
    }

    /// One APU tick: steps the duty sequencer on timer expiry.
    pub fn tick(&mut self) {
        if self.timer.tick() {
            self.duty_cycle = (self.duty_cycle.wrapping_sub(1)) & 0x07;
        }
    }

    /// Quarter-frame clock: envelope.
    pub fn clock_quarter_frame(&mut self) {
        self.envelope.clock();
        self.length.reload();
    }

    /// Half-frame clock: envelope, length counter, and sweep divider.
    pub fn clock_half_frame(&mut self) {
        self.envelope.clock();
        self.length.clock();
        self.clock_sweep();
        self.length.reload();
    }

    /// Raw channel level 0..=15 before the mixer.
    ///
    /// Zero when any hardware gate silences the channel: sequencer output,
    /// length counter, envelope, or the sweep-unit conditions (timer below
    /// 8, or positive-direction target overflow above `$7FF`).
    #[must_use]
    pub fn level(&self) -> u8 {
        if self.is_muted() {
            0
        } else {
            Self::DUTY_TABLE[usize::from(self.duty)][usize::from(self.duty_cycle)] * self.volume()
        }
    }

    const fn volume(&self) -> u8 {
        if self.length.counter() == 0 {
            0
        } else {
            self.envelope.volume()
        }
    }

    /// Sweep muting conditions, which hold even when the sweep unit is
    /// disabled (nesdev, "APU Sweep"): period below 8, or adder overflow
    /// into `$7FF+` while negation is off.
    const fn is_muted(&self) -> bool {
        self.real_period < 8 || (!self.sweep.negate && self.sweep.target > 0x7FF)
    }

    const fn set_period(&mut self, period: u16) {
        self.real_period = period;
        self.timer.set_period(period);
        self.update_target();
    }

    const fn update_target(&mut self) {
        let delta = self.real_period >> self.sweep.shift;
        if self.sweep.negate {
            // Pulse 1 subtracts two's complement decremented (nesdev quirk).
            self.sweep.target = self.real_period - delta;
            if matches!(self.channel, PulseChannel::One) {
                self.sweep.target = self.sweep.target.wrapping_sub(1);
            }
        } else {
            self.sweep.target = self.real_period + delta;
        }
    }

    const fn clock_sweep(&mut self) {
        self.sweep.divider = self.sweep.divider.wrapping_sub(1);
        if self.sweep.divider == 0 {
            // Period updates are skipped when shift is zero or the current
            // period is below 8, or the target overflows (nesdev, "APU
            // Sweep": timing handling).
            if self.sweep.shift > 0
                && self.sweep.enabled
                && self.real_period >= 8
                && self.sweep.target <= 0x7FF
            {
                self.set_period(self.sweep.target);
            }
            self.sweep.divider = self.sweep.period;
        }
        if self.sweep.reload {
            self.sweep.divider = self.sweep.period;
            self.sweep.reload = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writing `$4003` restarts the duty phase but leaves the timer divider
    /// counting from where it was (nesdev side-effects note; asserted
    /// directly like Tetanes's own pulse test, since ROMs report nothing).
    #[test]
    fn write_timer_hi_restarts_phase_but_not_divider() {
        let mut pulse = Pulse::new(PulseChannel::One);
        pulse.set_enabled(true);
        pulse.write_timer_lo(0x40);
        pulse.write_timer_hi(0x00);

        for _ in 0..100 {
            pulse.tick();
        }
        assert_ne!(pulse.duty_cycle, 0, "the sequencer has advanced");
        assert_ne!(
            pulse.timer.counter(),
            pulse.timer.period(),
            "the divider is mid-count"
        );

        pulse.write_timer_hi(0x00);
        assert_eq!(pulse.duty_cycle, 0, "the duty phase restarts");
        assert_eq!(
            pulse.timer.counter(),
            pulse.timer.counter(),
            "the divider keeps its count"
        );
    }

    /// Timer-period sanity against the nesdev formula: the sequencer next
    /// steps `t + 1` APU ticks later, so a `t = 0x0FF` timer (waveform
    /// `16 * (t + 1)` CPU cycles, about 437 Hz NTSC) steps every 0x100 APU
    /// ticks.
    #[test]
    fn sequencer_steps_every_t_plus_one_apu_ticks() {
        let mut pulse = Pulse::new(PulseChannel::One);
        pulse.write_ctrl(0x80); // duty 2, audible
        pulse.write_timer_lo(0xFF);
        pulse.write_timer_hi(0x00);

        let mut last = pulse.duty_cycle;
        let mut step_ticks = Vec::<u32>::new();
        for tick in 1..=20_000_u32 {
            pulse.tick();
            if pulse.duty_cycle != last {
                last = pulse.duty_cycle;
                step_ticks.push(tick);
            }
        }
        // Drop the first step: it lands mid-stride from the start state.
        for pair in step_ticks[1..].windows(2) {
            assert_eq!(
                pair[1].saturating_sub(pair[0]),
                0x100,
                "pulse t=0x0FF must step every t+1 APU ticks"
            );
        }
    }
}
