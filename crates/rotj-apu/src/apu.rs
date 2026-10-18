//! The 2A03 register file: `$4000-$4017` writes plus channel and frame-counter
//! clocking, so the game's music driver can be replayed from the disassembly
//! register-for-register.

use crate::dmc::{self, Dmc, SampleRom};
use crate::frame_counter::{FrameCounter, FrameEvent};
use crate::mixer;
use crate::noise::{self, Noise};
use crate::pulse::{Pulse, PulseChannel};
use crate::triangle::{self, Triangle};
use crate::{FRAME_COUNTER, STATUS, pulse1, pulse2, status};

/// The whole APU: four channels, the frame sequencer, and the mixer.
#[derive(Debug, Clone)]
pub struct Apu {
    pulse1: Pulse,
    pulse2: Pulse,
    triangle: Triangle,
    noise: Noise,
    dmc: Dmc,
    /// Sample bytes the DMC reads; empty until the ROM bank is supplied.
    sample_rom: SampleRom,
    frame_counter: FrameCounter,
}

impl Apu {
    /// Power-on state: everything off, as after a reset.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pulse1: Pulse::new(PulseChannel::One),
            pulse2: Pulse::new(PulseChannel::Two),
            triangle: Triangle::new(),
            noise: Noise::new(),
            dmc: Dmc::new(),
            sample_rom: SampleRom::empty(),
            frame_counter: FrameCounter::new(),
        }
    }

    /// Supplies the fixed sample bank the DMC fetches from (the game's samples
    /// live in the `$E000`-`$FFFF` PRG bank, `disasm/MAP.md` §8).
    pub fn set_sample_rom(&mut self, rom: SampleRom) {
        self.sample_rom = rom;
    }

    /// Applies a CPU write to the APU register range; anything outside
    /// `$4000-$4017` is ignored.
    pub fn write(&mut self, addr: u16, val: u8) {
        match addr {
            pulse1::CTRL => self.pulse1.write_ctrl(val),
            pulse1::SWEEP => self.pulse1.write_sweep(val),
            pulse1::TIMER_LO => self.pulse1.write_timer_lo(val),
            pulse1::TIMER_HI => self.pulse1.write_timer_hi(val),
            pulse2::CTRL => self.pulse2.write_ctrl(val),
            pulse2::SWEEP => self.pulse2.write_sweep(val),
            pulse2::TIMER_LO => self.pulse2.write_timer_lo(val),
            pulse2::TIMER_HI => self.pulse2.write_timer_hi(val),
            // $4008-$400B: the triangle has no sweep register.
            triangle::CTRL => self.triangle.write_ctrl(val),
            triangle::TIMER_LO => self.triangle.write_timer_lo(val),
            triangle::TIMER_HI => self.triangle.write_timer_hi(val),
            noise::CTRL => self.noise.write_ctrl(val),
            noise::TIMER => self.noise.write_timer(val),
            noise::LENGTH => self.noise.write_length(val),
            dmc::CTRL => self.dmc.write_ctrl(val),
            dmc::LEVEL => self.dmc.write_level(val),
            dmc::ADDRESS => self.dmc.write_address(val),
            dmc::LENGTH => self.dmc.write_length(val),
            STATUS => self.set_enabled(val),
            FRAME_COUNTER => self.frame_counter.write(val),
            _ => {}
        }
    }

    /// `$4015` enable bits, in channel order; a cleared bit silences instantly.
    fn set_enabled(&mut self, val: u8) {
        self.pulse1.set_enabled(val & status::PULSE1 != 0);
        self.pulse2.set_enabled(val & status::PULSE2 != 0);
        self.triangle.set_enabled(val & status::TRIANGLE != 0);
        self.noise.set_enabled(val & status::NOISE != 0);
        self.dmc.set_enabled(val & status::DMC != 0);
    }

    /// One APU tick (two CPU cycles), like the hardware's clock.
    ///
    /// The tone channels clock once per APU tick; the DMC's rate timer is in
    /// CPU cycles, so it is clocked twice here.
    pub fn tick(&mut self) {
        self.pulse1.tick();
        self.pulse2.tick();
        self.triangle.tick();
        self.noise.tick();
        self.dmc.tick(&self.sample_rom);
        self.dmc.tick(&self.sample_rom);
        // A half frame carries its own quarter-frame work, so only one of the
        // two clocks each channel back.
        match self.frame_counter.tick() {
            FrameEvent::None => {}
            FrameEvent::Quarter => {
                self.pulse1.clock_quarter_frame();
                self.pulse2.clock_quarter_frame();
                self.triangle.clock_quarter_frame();
                self.noise.clock_quarter_frame();
            }
            FrameEvent::Half => {
                self.pulse1.clock_half_frame();
                self.pulse2.clock_half_frame();
                self.triangle.clock_half_frame();
                self.noise.clock_half_frame();
            }
        }
    }

    /// Analog output level of the four channels, `0.0..=1.0`.
    #[must_use]
    pub fn output(&self) -> f32 {
        mixer::mix(
            self.pulse1.level(),
            self.pulse2.level(),
            self.triangle.level(),
            self.noise.level(),
            self.dmc.level(),
        )
    }

    /// Current raw level of each channel: pulse 1, pulse 2, triangle, noise,
    /// DMC.
    #[must_use]
    pub fn channel_levels(&self) -> [u8; 5] {
        [
            self.pulse1.level(),
            self.pulse2.level(),
            self.triangle.level(),
            self.noise.level(),
            self.dmc.level(),
        ]
    }

    /// Whether the frame sequencer has raised its (mode-0) IRQ.
    #[must_use]
    pub const fn irq_pending(&self) -> bool {
        self.frame_counter.irq_pending()
    }

    /// Whether the DMC has raised its sample-end IRQ.
    #[must_use]
    pub const fn dmc_irq_pending(&self) -> bool {
        self.dmc.irq_pending()
    }
}

impl Default for Apu {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One second of APU ticks, enough to walk quarter and half frames.
    const ONE_SECOND: u32 = 1_789_773;

    /// Configure pulse 1 as a loud constant square and enable it.
    ///
    /// Order matters: length-counter loads are ignored while the channel is
    /// disabled (`$4015`), as on the hardware.
    fn playing_pulse1(apu: &mut Apu) {
        apu.write(STATUS, status::PULSE1); // enable pulse 1
        apu.write(pulse1::CTRL, 0xbf); // duty 2, halt, constant volume 15
        apu.write(pulse1::TIMER_LO, 0x40); // timer low
        apu.write(pulse1::TIMER_HI, 0x08); // length load + timer high
    }

    /// Highest mixed level seen over `ticks` APU ticks.
    fn peak_after(apu: &mut Apu, ticks: u32) -> f32 {
        let mut peak = 0.0f32;
        for _ in 0..ticks {
            apu.tick();
            peak = peak.max(apu.output());
        }
        peak
    }

    #[test]
    fn enabled_pulse_channel_produces_output() {
        let mut apu = Apu::new();
        assert_eq!(apu.output(), 0.0);
        playing_pulse1(&mut apu);
        // The length counter only loads at the first frame-counter event, so a
        // full second of ticks is the honest budget.
        let peak = peak_after(&mut apu, ONE_SECOND);
        assert!(peak > 0.0);
        assert!(peak <= 1.0);
    }

    #[test]
    fn clearing_the_enable_bit_silences_the_channel() {
        let mut apu = Apu::new();
        playing_pulse1(&mut apu);
        assert!(peak_after(&mut apu, ONE_SECOND) > 0.0);
        apu.write(STATUS, 0);
        assert_eq!(apu.output(), 0.0);
    }

    #[test]
    fn triangle_plays_and_stays_in_range() {
        let mut apu = Apu::new();
        apu.write(STATUS, status::TRIANGLE); // enable triangle before its length load
        apu.write(triangle::CTRL, 0xff); // control flag + linear counter 127
        apu.write(triangle::TIMER_LO, 0x20);
        apu.write(triangle::TIMER_HI, 0x08);
        let peak = peak_after(&mut apu, ONE_SECOND);
        assert!(peak > 0.0);
        assert!(peak <= 1.0);
    }

    #[test]
    fn dmc_reaches_the_mixer_only_when_enabled() {
        let mut apu = Apu::new();
        apu.write(dmc::LEVEL, 0x7f); // direct load, full DAC
        assert_eq!(apu.output(), 0.0, "a disabled DMC mixes silence");
        apu.write(STATUS, status::DMC); // $4015 bit 4 enables the DMC
        assert!(apu.output() > 0.0, "the DMC level reaches the mixer");
        apu.write(STATUS, 0);
        assert_eq!(apu.output(), 0.0);
    }

    #[test]
    fn unrelated_writes_are_ignored() {
        let mut apu = Apu::new();
        apu.write(0x2007, 0xff);
        apu.write(0x0000, 0xff);
        assert_eq!(apu.output(), 0.0);
    }
}
