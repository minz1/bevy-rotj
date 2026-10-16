//! DMC (Delta Modulation Channel) core and register map.
//!
//! The game's sampled bass lines play through this channel: bank 10 of the
//! sound driver (`disasm/bank0A.s`) pushes four-byte records at the per-decoded
//! `$4010`/`$4011`/`$4012`/`$4013` registers, with the sample bytes living in
//! the fixed `$E000` PRG bank. See `disasm/MAP.md` §8 (`rotj dmc` extracts the samples).
//!
//! Register map (nesdev, "APU Dmc"):
//!
//! - `$4010` — replay control: bit 7 IRQ enable, bit 6 loop, bits 3..0 rate
//!   entry. `$4015`-bit 4 acknowledges the DMC IRQ.
//! - `$4011` — output level, 7 bits (direct load).
//! - `$4012` — sample address: `$C000 + value * 64`.
//! - `$4013` — sample length: `value * 16 + 1` bytes.
//! - `$4015` — bit 4: channel enable; bit 7 (read): DMC IRQ status.
//!
//! The DMC is clocked at the **CPU rate** (one [`Dmc::tick`] per CPU cycle),
//! unlike the tone channels whose tick is an APU tick (two CPU cycles, see
//! [`crate::apu::Apu::tick`]). Each rate period shifts one bit out of the shift
//! register: a set bit raises the 7-bit DAC by 2, a clear bit lowers it by 2,
//! clamped to `0..=127`.
//!
//! `shortcut: the port gates the DAC contribution on the $4015 enable bit, so a
//! disabled channel mixes as 0; real hardware holds the last level, upgrade if
//! waveform parity needs the held level.`

use crate::timer::Timer;

/// `$4010` replay control.
pub const CTRL_ADDR: u16 = 0x4010;
/// `$4011` output level.
pub const LEVEL_ADDR: u16 = 0x4011;
/// `$4012` sample address.
pub const SAMPLE_ADDR_ADDR: u16 = 0x4012;
/// `$4013` sample length.
pub const SAMPLE_LENGTH_ADDR: u16 = 0x4013;
/// `$4015`-bit 4: DMC enable.
pub const ENABLE_BIT: u8 = 0x10;
/// `$4015`-bit 7 (read): DMC IRQ flag, cleared by the access.
pub const IRQ_FLAG_BIT: u8 = 0x80;

/// `$4012`-bits 7..0 bias: a written value `v` plays from `$C000 + v * 64`.
pub const SAMPLE_BASE: u16 = 0xC000;

/// `$4010`-bits 3..0 rate table, NTSC: CPU cycles between output bits.
///
/// | entry | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | A | B | C | D | E | F |
/// | cycles | 428 | 380 | 340 | 320 | 286 | 254 | 226 | 214 | 190 | 160 | 142 | 128 | 106 | 84 | 72 | 54 |
pub const RATE_TABLE_NTSC: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54,
];

/// Source of the sample bytes the DMC fetches.
///
/// The core stays decoupled from the bus: callers hand it a view of the fixed
/// sample bank and it only ever asks for one byte at a time.
pub trait SampleMemory {
    /// Reads the byte at CPU `addr`; out-of-range reads as 0.
    fn read(&self, addr: u16) -> u8;
}

/// A flat sample image mapped at a CPU base address.
///
/// The game's samples live in the fixed `$E000`-`$FFFF` window, so
/// `SampleRom::new(0xE000, bank15_bytes)` serves the core.
#[derive(Debug, Clone, Default)]
pub struct SampleRom {
    base: u16,
    bytes: Vec<u8>,
}

impl SampleRom {
    /// An empty image (every read is 0).
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            base: 0,
            bytes: Vec::new(),
        }
    }

    /// An image whose byte 0 sits at CPU address `base`.
    #[must_use]
    pub const fn new(base: u16, bytes: Vec<u8>) -> Self {
        Self { base, bytes }
    }

    /// Number of sample bytes in the image.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether the image has no bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl SampleMemory for SampleRom {
    fn read(&self, addr: u16) -> u8 {
        self.bytes
            .get(usize::from(addr.wrapping_sub(self.base)))
            .copied()
            .unwrap_or(0)
    }
}

/// The `$4015` enable bit and whether a sample is mid-playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    /// `$4015` bit 4 clear.
    Disabled,
    /// Enabled, but the sample has ended.
    Idle,
    /// Enabled and playing a sample.
    Playing,
}

/// DMC channel state: registers, address/length counters and the 7-bit DAC.
#[derive(Debug, Clone)]
pub struct Dmc {
    status: Status,
    irq_enabled: bool,
    loop_enabled: bool,
    rate_index: u8,
    timer: Timer,
    /// DAC level, `0..=127`.
    output_level: u8,
    irq_flag: bool,
    /// `$4013` derived: total bytes the sample spans.
    sample_length: u16,
    /// `$4012` derived: first CPU address read.
    sample_addr: u16,
    current_addr: u16,
    bytes_remaining: u16,
    shift_register: u8,
    bits_remaining: u8,
}

impl Default for Dmc {
    fn default() -> Self {
        Self::new()
    }
}

impl Dmc {
    /// Power-on state: disabled, DAC 0, no sample loaded.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            status: Status::Disabled,
            irq_enabled: false,
            loop_enabled: false,
            rate_index: 0,
            timer: Timer::new(RATE_TABLE_NTSC[0] - 1),
            output_level: 0,
            irq_flag: false,
            sample_length: 1,
            sample_addr: SAMPLE_BASE,
            current_addr: SAMPLE_BASE,
            bytes_remaining: 0,
            shift_register: 0,
            bits_remaining: 0,
        }
    }

    /// `$4010`: IRQ enable, loop, and rate entry.
    pub fn write_ctrl(&mut self, val: u8) {
        self.irq_enabled = (val & 0x80) != 0;
        self.loop_enabled = (val & 0x40) != 0;
        self.rate_index = val & 0x0F;
        self.timer
            .set_period(RATE_TABLE_NTSC[usize::from(self.rate_index)] - 1);
        if !self.irq_enabled {
            self.irq_flag = false;
        }
    }

    /// `$4011`: 7-bit direct load straight into the DAC.
    pub fn write_level(&mut self, val: u8) {
        self.output_level = val & 0x7F;
    }

    /// `$4012`: sample address, `$C000 + value * 64`.
    pub fn write_address(&mut self, val: u8) {
        self.sample_addr = SAMPLE_BASE + (u16::from(val) << 6);
    }

    /// `$4013`: sample length, `value * 16 + 1` bytes.
    pub fn write_length(&mut self, val: u8) {
        self.sample_length = u16::from(val) * 16 + 1;
    }

    /// `$4015`-bit 4: enable; a 0-to-1 transition restarts the sample and any
    /// access acknowledges the IRQ (nesdev, "APU Dmc").
    pub fn set_enabled(&mut self, enabled: bool) {
        if enabled {
            if self.status == Status::Disabled {
                self.restart();
            }
            self.irq_flag = false;
        } else {
            self.status = Status::Disabled;
            self.bytes_remaining = 0;
            self.bits_remaining = 0;
        }
    }

    /// Advances one **CPU cycle**, shifting a bit and possibly fetching a byte.
    pub fn tick(&mut self, mem: &impl SampleMemory) {
        if self.status != Status::Disabled && self.timer.tick() {
            self.clock_output(mem);
        }
    }

    /// DAC level fed to the mixer, `0..=127`; 0 while disabled.
    #[must_use]
    pub const fn level(&self) -> u8 {
        if matches!(self.status, Status::Disabled) {
            0
        } else {
            self.output_level
        }
    }

    /// Whether the DMC has raised its sample-end IRQ.
    #[must_use]
    pub const fn irq_pending(&self) -> bool {
        self.irq_flag
    }

    /// Current sample length in bytes (`$4013` derived).
    #[must_use]
    pub const fn sample_length(&self) -> u16 {
        self.sample_length
    }

    /// Current sample start address (`$4012` derived).
    #[must_use]
    pub const fn sample_addr(&self) -> u16 {
        self.sample_addr
    }

    /// Starts (or loops) the sample from its reload point.
    fn restart(&mut self) {
        self.current_addr = self.sample_addr;
        self.bytes_remaining = self.sample_length;
        self.bits_remaining = 0;
        self.shift_register = 0;
        self.status = Status::Playing;
        self.timer
            .set_period(RATE_TABLE_NTSC[usize::from(self.rate_index)] - 1);
    }

    /// One output bit: refill the shift register when empty, then step the DAC.
    fn clock_output(&mut self, mem: &impl SampleMemory) {
        if self.status != Status::Playing {
            return;
        }
        if self.bits_remaining == 0 {
            if self.bytes_remaining == 0 {
                self.end_sample();
                return;
            }
            self.shift_register = mem.read(self.current_addr);
            self.current_addr = self.current_addr.wrapping_add(1);
            self.bytes_remaining -= 1;
            self.bits_remaining = 8;
        }

        let bit = self.shift_register & 0x01;
        self.shift_register >>= 1;
        self.bits_remaining -= 1;
        if bit == 0 {
            self.output_level = self.output_level.saturating_sub(2);
        } else {
            self.output_level = self.output_level.saturating_add(2).min(127);
        }

        if self.bits_remaining == 0 && self.bytes_remaining == 0 {
            self.end_sample();
        }
    }

    /// End of sample: reload on loop, else stop and raise IRQ if enabled.
    fn end_sample(&mut self) {
        if self.loop_enabled {
            self.current_addr = self.sample_addr;
            self.bytes_remaining = self.sample_length;
            self.bits_remaining = 0;
            self.shift_register = 0;
        } else {
            self.status = Status::Idle;
            self.bits_remaining = 0;
            if self.irq_enabled {
                self.irq_flag = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rate entry 0 is the slowest bit clock at 428 CPU cycles.
    #[test]
    fn rate_table_is_the_standard_ntsc_table() {
        assert_eq!(RATE_TABLE_NTSC[0], 428);
        assert_eq!(RATE_TABLE_NTSC[15], 54);
    }

    /// Direct load writes the 7-bit DAC level, and the port only mixes it
    /// while the channel is enabled.
    #[test]
    fn direct_load_sets_the_dac_level() {
        let mut dmc = Dmc::new();
        assert_eq!(dmc.level(), 0);
        dmc.write_level(0x40);
        assert_eq!(dmc.level(), 0, "disabled channel mixes silence");
        dmc.set_enabled(true);
        assert_eq!(dmc.level(), 64);
        dmc.set_enabled(false);
        assert_eq!(dmc.level(), 0);
    }

    /// A one-byte sample `$55` (bit pattern 1,0,1,0,...) walks the DAC up and
    /// down by 2 once per rate period.
    #[test]
    fn known_sample_produces_expected_levels_at_the_rate() {
        let mut dmc = Dmc::new();
        let rom = SampleRom::new(SAMPLE_BASE, vec![0x55]);
        dmc.write_address(0); // $C000
        dmc.write_length(0); // 1 byte
        dmc.write_ctrl(0x0F); // rate 15 = 54 CPU cycles, no IRQ, no loop
        dmc.set_enabled(true);

        dmc.tick(&rom);
        assert_eq!(dmc.level(), 2, "first set bit raises the DAC");
        // 53 more cycles: no new bit yet (one bit per 54 cycles).
        for _ in 0..53 {
            dmc.tick(&rom);
        }
        assert_eq!(dmc.level(), 2);
        dmc.tick(&rom);
        assert_eq!(dmc.level(), 0, "second (clear) bit lowers the DAC");

        let mut seen = vec![dmc.level()];
        for _ in 0..6 {
            for _ in 0..54 {
                dmc.tick(&rom);
            }
            seen.push(dmc.level());
        }
        assert_eq!(seen, [0, 2, 0, 2, 0, 2, 0], "alternating $55 walk");
    }

    /// A non-looping sample raises the IRQ flag and freezes at end of sample.
    #[test]
    fn end_of_sample_sets_irq_and_stops() {
        let mut dmc = Dmc::new();
        let rom = SampleRom::new(SAMPLE_BASE, vec![0xFF]);
        dmc.write_address(0);
        dmc.write_length(0); // 1 byte = 8 bits
        dmc.write_ctrl(0x80); // IRQ enabled, no loop, rate 0
        dmc.set_enabled(true);
        assert!(!dmc.irq_pending());

        for _ in 0..8 {
            dmc.tick(&rom);
            for _ in 0..(RATE_TABLE_NTSC[0] - 1) {
                dmc.tick(&rom);
            }
        }
        let at_end = dmc.level();
        assert!(dmc.irq_pending(), "end of a 1-byte sample raises the IRQ");
        for _ in 0..1000 {
            dmc.tick(&rom);
        }
        assert_eq!(dmc.level(), at_end, "stopped channel holds its level");
    }

    /// The loop flag reloads the sample instead of stopping.
    #[test]
    fn loop_flag_restarts_the_sample() {
        let mut dmc = Dmc::new();
        let rom = SampleRom::new(SAMPLE_BASE, vec![0xFF]);
        dmc.write_address(0);
        dmc.write_length(0);
        dmc.write_ctrl(0xC0); // loop + IRQ, rate 0
        dmc.set_enabled(true);

        let mut changes = 0;
        let mut last = dmc.level();
        for _ in 0..(RATE_TABLE_NTSC[0] * 20) {
            dmc.tick(&rom);
            if dmc.level() != last {
                changes += 1;
                last = dmc.level();
            }
        }
        assert!(changes > 8, "looping keeps producing bits");
        assert!(!dmc.irq_pending(), "a looping sample never raises the IRQ");
    }

    /// Out-of-range reads are open-bus zeros, not panics.
    #[test]
    fn reads_outside_the_image_are_zero() {
        let rom = SampleRom::new(0xE000, vec![0xAB]);
        assert_eq!(rom.read(0xE000), 0xAB);
        assert_eq!(rom.read(0xE001), 0);
        assert_eq!(rom.read(0xC000), 0);
    }
}
