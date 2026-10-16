//! The CPU address space: RAM, PPU and APU/IO registers, FME-7, WRAM and PRG.

use std::ops::RangeInclusive;
use std::sync::Arc;

use crate::cart::Cart;
use crate::cpu::{Bus, Cpu};
use crate::fme7::Fme7;
use crate::ppu::Ppu;

/// CPU cycles an OAM DMA takes (fceux: 256 reads and 256 writes, no alignment cycle).
const OAM_DMA_CYCLES: u32 = 512;
/// CPU cycles stolen by each DMC sample fetch (fceux: four `X6502_DMR` reads).
const DMC_FETCH_CYCLES: u32 = 4;
const DMC_RATES: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54,
];

/// The DMC's sample fetches as fceux schedules them (no audio): what stalls the CPU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dmc {
    /// Sample bytes still to fetch.
    pub remaining: u16,
    control: u8,
    sample_len: u8,
    timer: i32,
    bits: u8,
    buffer_full: bool,
}

impl Default for Dmc {
    fn default() -> Self {
        Dmc {
            remaining: 0,
            control: 0,
            sample_len: 0,
            timer: 1,
            bits: 0,
            buffer_full: false,
        }
    }
}

impl Dmc {
    /// A write to `$4010-$4013` or `$4015`.
    pub(crate) fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x4010 => self.control = value,
            0x4013 => self.sample_len = value,
            0x4015 if value & 0x10 == 0 => self.remaining = 0,
            0x4015 if self.remaining == 0 => self.restart(),
            _ => {}
        }
    }

    /// fceux's sound hook: fetch if the buffer is free (returning the stall), then run the timer.
    pub(crate) fn clock(&mut self, cycles: u32) -> u32 {
        let mut stall = 0;
        if self.remaining > 0 && !self.buffer_full {
            stall = DMC_FETCH_CYCLES;
            self.buffer_full = true;
            self.remaining -= 1;
            if self.remaining == 0 && self.control & 0x40 != 0 {
                self.restart();
            }
        }
        self.timer -= i32::try_from(cycles).expect("one instruction's cycles");
        while self.timer <= 0 {
            self.timer += i32::from(DMC_RATES[usize::from(self.control & 0x0F)]);
            self.bits = (self.bits + 1) & 7;
            if self.bits == 0 {
                self.buffer_full = false;
            }
        }
        stall
    }

    fn restart(&mut self) {
        self.remaining = (u16::from(self.sample_len) << 4) + 1;
    }
}

/// Write watches, exec trace and pokes for reverse engineering; empty and free by default.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Trace {
    /// CPU address ranges whose writes are logged to `writes`.
    pub watch: Vec<RangeInclusive<u16>>,
    /// PC ranges whose instructions are logged to `execs`.
    pub exec: Vec<RangeInclusive<u16>>,
    /// `(pc, addr, value)`: write `value` to `addr` whenever execution reaches `pc`.
    pub pokes: Vec<(u16, u16, u8)>,
    /// Logged writes, oldest first.
    pub writes: Vec<WriteEvent>,
    /// Logged instructions, oldest first.
    pub execs: Vec<ExecEvent>,
    /// PCs at which RAM and WRAM are copied to `peeked`, before the instruction runs.
    pub peeks: Vec<u16>,
    /// Copies taken at `peeks`, oldest first.
    pub peeked: Vec<PeekEvent>,
    /// The frame being run, 1-based like fceux's `emu.framecount()` (set by the machine).
    pub frame: u64,
    /// A recorded session's pads and the app's random bytes, fed at every NMI wait.
    pub playtest: Option<Playtest>,
}

/// A play session the app recorded, fed to the game one pad per NMI wait, with the
/// random bytes pinned to what the app's NMI makes of them. While set, the pad
/// `run_frame` is given is ignored. Equality leaves `step` out: function pointers have none.
#[derive(Clone, Debug)]
pub struct Playtest {
    /// One pad per NMI wait, in the controller's shift order (A in bit 0); 0 past the end.
    pub pads: Vec<u8>,
    /// The index of the next pad.
    pub next: usize,
    /// The NMI wait's entry, where the next pad is set and the random bytes saved.
    pub wait: u16,
    /// The NMI wait's return, where the saved random bytes are written back stepped.
    pub woke: u16,
    /// How many times the bytes are stepped per wait.
    pub spins: usize,
    /// One step of the game's generator over `$F9/$FA`.
    pub step: fn([u8; 2]) -> [u8; 2],
    /// Written at the first `woke` instead of the stepped bytes: the app's start values.
    pub start: Option<StartBytes>,
    saved: Option<[u8; 2]>,
}

/// The bytes a session starts from that depend on when Start was pressed on the title.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartBytes {
    /// `$F9/$FA`.
    pub rng: [u8; 2],
    /// `$F1`.
    pub frame: u8,
    /// `$F7` bit 7.
    pub frame_odd: bool,
    /// The OAM page, `$0200-$02FF`, where the title's last sprites are parked.
    pub oam: [u8; 256],
    /// `$FB`, the OAM entry the title's last frame left next.
    pub oam_next: u8,
}

impl PartialEq for Playtest {
    fn eq(&self, other: &Playtest) -> bool {
        (&self.pads, self.next, self.wait, self.woke, self.spins)
            == (&other.pads, other.next, other.wait, other.woke, other.spins)
            && (self.start, self.saved) == (other.start, other.saved)
    }
}

impl Eq for Playtest {}

impl Playtest {
    /// `pads` fed at `wait`, the random bytes stepped `spins` times by `step` at `woke`.
    #[must_use]
    pub fn new(
        pads: Vec<u8>,
        (wait, woke): (u16, u16),
        spins: usize,
        step: fn([u8; 2]) -> [u8; 2],
    ) -> Playtest {
        Playtest {
            pads,
            next: 0,
            wait,
            woke,
            spins,
            step,
            start: None,
            saved: None,
        }
    }

    /// Execution reached `pc`: set the pad at the wait, pin the random bytes at its return.
    pub(crate) fn reach(&mut self, pc: u16, pad: &mut u8, ram: &mut [u8; 2048]) {
        const RNG: usize = 0xF9;
        if pc == self.wait {
            *pad = self.pads.get(self.next).copied().unwrap_or(0);
            self.next += 1;
            self.saved = Some([ram[RNG], ram[RNG + 1]]);
        } else if pc == self.woke {
            if let Some(start) = self.start.take() {
                ram[RNG..RNG + 2].copy_from_slice(&start.rng);
                ram[0xF1] = start.frame;
                ram[0xF7] = (ram[0xF7] & 0x7F) | (u8::from(start.frame_odd) << 7);
                ram[0x200..0x300].copy_from_slice(&start.oam);
                ram[0xFB] = start.oam_next;
            } else if let Some(mut rng) = self.saved {
                for _ in 0..self.spins {
                    rng = (self.step)(rng);
                }
                ram[RNG..RNG + 2].copy_from_slice(&rng);
            }
        }
    }
}

/// A watched write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteEvent {
    /// Frame it happened in.
    pub frame: u64,
    /// Address of the writing instruction.
    pub pc: u16,
    /// PRG bank mapped at `pc` (`None` when it runs from RAM or WRAM).
    pub bank: Option<u8>,
    /// Address written.
    pub addr: u16,
    /// Value written.
    pub value: u8,
}

/// A traced instruction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecEvent {
    /// Frame it ran in.
    pub frame: u64,
    /// Its address.
    pub pc: u16,
    /// PRG bank mapped at `pc` (`None` when it runs from RAM or WRAM).
    pub bank: Option<u8>,
    /// CPU cycles since the frame began (`run_frame`), when it ran.
    pub cycle: u32,
}

/// RAM, WRAM and the PPU's memory as they were when execution reached a peeked PC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeekEvent {
    /// Frame it happened in.
    pub frame: u64,
    /// The peeked PC.
    pub pc: u16,
    /// PRG bank mapped at `pc` (`None` when it runs from RAM or WRAM).
    pub bank: Option<u8>,
    /// NMIs taken since power-on.
    pub nmis: u64,
    /// IRQs taken since power-on.
    pub irqs: u64,
    /// CPU registers.
    pub cpu: Cpu,
    /// CPU RAM.
    pub ram: Box<[u8; 2048]>,
    /// Cartridge WRAM.
    pub wram: Box<[u8; 8192]>,
    /// The PPU's nametable RAM, physical table A then B.
    pub vram: Box<[u8; 2048]>,
    /// The PPU's palette RAM, each entry masked `& 0x3F`.
    pub palette: [u8; 32],
}

/// One register write and the instruction that made it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteLog {
    /// Address of the writing instruction.
    pub pc: u16,
    /// Register written.
    pub addr: u16,
    /// Value written.
    pub value: u8,
}

/// Everything the CPU can reach.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NesBus {
    /// 2 KiB internal RAM, mirrored through `$1FFF`.
    pub ram: [u8; 2048],
    /// 8 KiB cartridge RAM at `$6000` when the FME-7 selects and enables it.
    pub wram: [u8; 8192],
    /// Cartridge ROM, shared between snapshots.
    pub cart: Arc<Cart>,
    /// Mapper state.
    pub fme7: Fme7,
    /// PPU registers and memory.
    pub ppu: Ppu,
    pub(crate) pad: u8,
    pub(crate) pc: u16,
    pub(crate) apu_log: Vec<WriteLog>,
    pub(crate) dma_stall: u32,
    /// DMC sample fetches.
    pub dmc: Dmc,
    /// Write watches, exec trace and pokes.
    pub trace: Trace,
    shift: u8,
    strobe: bool,
}

impl NesBus {
    /// A powered-on bus with zeroed memory.
    #[must_use]
    pub(crate) fn new(cart: Cart) -> NesBus {
        NesBus {
            ram: [0; 2048],
            wram: [0; 8192],
            cart: Arc::new(cart),
            fme7: Fme7::default(),
            ppu: Ppu::default(),
            pad: 0,
            pc: 0,
            apu_log: Vec::new(),
            dma_stall: 0,
            dmc: Dmc::default(),
            trace: Trace::default(),
            shift: 0,
            strobe: false,
        }
    }

    fn unexpected_read(&self, addr: u16) -> u8 {
        panic!(
            "read of ${addr:04X} at ${:04X}: the game is not known to read it",
            self.pc
        );
    }
}

impl Bus for NesBus {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[usize::from(addr & 0x07FF)],
            0x2000..=0x3FFF if addr & 7 == 7 && self.ppu.renders() => panic!(
                "read of ${addr:04X} at ${:04X} while the PPU renders: fceux answers from its \
                 renderer's VRAM address, which is not modelled",
                self.pc
            ),
            0x2000..=0x3FFF => self
                .ppu
                .read_register(addr & 0x2007, &self.cart.chr, &self.fme7),
            0x4000..=0x4015 | 0x4017 => self.unexpected_read(addr),
            0x4016 => {
                let bit = if self.strobe {
                    self.pad & 1
                } else {
                    self.shift & 1
                };
                if !self.strobe {
                    self.shift = (self.shift >> 1) | 0x80;
                }
                0x40 | bit
            }
            0x6000..=0x7FFF if self.fme7.wram_enabled() => self.wram[usize::from(addr & 0x1FFF)],
            _ => match self.fme7.prg_offset(addr, self.cart.prg.len()) {
                Some(offset) => self.cart.prg[offset],
                None => self.unexpected_read(addr),
            },
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        if self.trace.watch.iter().any(|range| range.contains(&addr)) {
            let event = WriteEvent {
                frame: self.trace.frame,
                pc: self.pc,
                bank: self.fme7.bank_at(self.pc, self.cart.prg.len()),
                addr,
                value,
            };
            self.trace.writes.push(event);
        }
        match addr {
            0x0000..=0x1FFF => self.ram[usize::from(addr & 0x07FF)] = value,
            0x2000..=0x3FFF => self.ppu.write_register(addr & 0x2007, value, &self.fme7),
            0x4014 => {
                let page = u16::from(value) << 8;
                for i in 0..=255u8 {
                    let byte = self.read(page | u16::from(i));
                    self.ppu.write_register(0x2004, byte, &self.fme7);
                }
                self.dma_stall = OAM_DMA_CYCLES;
            }
            0x4016 => {
                self.strobe = value & 1 != 0;
                self.shift = self.pad;
            }
            0x4000..=0x4013 | 0x4015 | 0x4017 => {
                assert!(
                    addr != 0x4010 || value & 0x80 == 0,
                    "DMC IRQ enabled at ${:04X}: not modelled",
                    self.pc
                );
                self.dmc.write(addr, value);
                self.apu_log.push(WriteLog {
                    pc: self.pc,
                    addr,
                    value,
                });
            }
            0x6000..=0x7FFF if self.fme7.wram_enabled() => {
                self.wram[usize::from(addr & 0x1FFF)] = value;
            }
            0x8000..=0xBFFF => self.fme7.write(addr, value),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cart::Cart;

    fn bus() -> NesBus {
        let mut prg = vec![0; 16 * 0x2000];
        for bank in 0..16u8 {
            prg[usize::from(bank) * 0x2000] = bank;
        }
        NesBus::new(Cart {
            prg,
            chr: vec![0; 0x40000],
        })
    }

    fn command(bus: &mut NesBus, cmd: u8, value: u8) {
        bus.write(0x8000, cmd);
        bus.write(0xA000, value);
    }

    #[test]
    fn fme7_maps_windows_per_actors_md() {
        let mut bus = bus();
        command(&mut bus, 0x9, 8);
        command(&mut bus, 0xA, 13);
        command(&mut bus, 0xB, 14);
        assert_eq!(bus.read(0x8000), 8);
        assert_eq!(bus.read(0xA000), 13);
        assert_eq!(bus.read(0xC000), 14);
        assert_eq!(bus.read(0xE000), 15);
        command(&mut bus, 0x8, 0x03);
        assert_eq!(bus.read(0x6000), 3);
    }

    #[test]
    fn wram_requires_select_and_enable() {
        let mut bus = bus();
        command(&mut bus, 0x8, 0xC0);
        bus.write(0x6000, 0x55);
        assert_eq!(bus.read(0x6000), 0x55);
        command(&mut bus, 0x8, 0x80);
        bus.write(0x6001, 0x66);
        command(&mut bus, 0x8, 0x01);
        bus.write(0x6002, 0x77);
        assert_eq!(bus.read(0x6000), 1);
        assert_eq!(&bus.wram[..3], &[0x55, 0, 0]);
    }

    #[test]
    fn ram_mirrors_every_2k() {
        let mut bus = bus();
        bus.write(0x0800, 7);
        assert_eq!(bus.read(0x0000), 7);
        assert_eq!(bus.read(0x1800), 7);
    }

    #[test]
    fn controller_shifts_a_first() {
        let mut bus = bus();
        bus.pad = 0b1000_0001;
        bus.write(0x4016, 1);
        bus.write(0x4016, 0);
        let bits: Vec<u8> = (0..9).map(|_| bus.read(0x4016) & 1).collect();
        assert_eq!(bits, [1, 0, 0, 0, 0, 0, 0, 1, 1]);
    }

    #[test]
    fn oam_dma_copies_page_and_stalls() {
        let mut bus = bus();
        for i in 0..=255u8 {
            bus.write(0x0200 + u16::from(i), i);
        }
        bus.write(0x4014, 0x02);
        assert!(
            bus.ppu
                .oam
                .iter()
                .enumerate()
                .all(|(i, &byte)| usize::from(byte) == i)
        );
        assert_eq!(bus.dma_stall, OAM_DMA_CYCLES);
    }

    #[test]
    fn apu_writes_are_logged_with_pc() {
        let mut bus = bus();
        bus.pc = 0xC123;
        bus.write(0x4015, 0x0F);
        bus.write(0x4014, 0x02);
        assert_eq!(
            bus.apu_log,
            [WriteLog {
                pc: 0xC123,
                addr: 0x4015,
                value: 0x0F
            }]
        );
    }

    #[test]
    #[should_panic(expected = "read of $4015 at $C123")]
    fn unknown_io_reads_panic() {
        let mut bus = bus();
        bus.pc = 0xC123;
        bus.read(0x4015);
    }

    #[test]
    fn write_only_ppu_register_reads_return_the_latch() {
        let mut bus = bus();
        bus.write(0x2001, 0x1E);
        assert_eq!(bus.read(0x3FFD), 0x1E);
    }

    #[test]
    #[should_panic(expected = "read of $2FFF at $0000 while the PPU renders")]
    fn vram_reads_while_rendering_panic() {
        let mut bus = bus();
        bus.write(0x2001, 0x1E);
        bus.ppu.clear_vblank();
        bus.read(0x2FFF);
    }

    #[test]
    fn dmc_fetch_stalls_the_cpu() {
        let mut dmc = Dmc::default();
        dmc.write(0x4010, 0x4F);
        dmc.write(0x4013, 0x00);
        dmc.write(0x4015, 0x10);
        assert_eq!(dmc.clock(1), DMC_FETCH_CYCLES);
        assert_eq!(dmc.remaining, 1);
        assert_eq!(dmc.clock(54 * 7), 0);
        assert_eq!(dmc.clock(1), DMC_FETCH_CYCLES);
        dmc.write(0x4015, 0x00);
        assert_eq!(dmc.clock(1000), 0);
    }

    #[test]
    fn watched_writes_name_frame_pc_and_bank() {
        let mut bus = bus();
        command(&mut bus, 0x9, 8);
        bus.trace.watch.push(0x0300..=0x0301);
        bus.trace.frame = 5;
        bus.pc = 0x8123;
        bus.write(0x0300, 7);
        bus.write(0x0302, 1);
        bus.pc = 0x0400;
        bus.write(0x0301, 2);
        let event = |pc, bank, addr, value| WriteEvent {
            frame: 5,
            pc,
            bank,
            addr,
            value,
        };
        assert_eq!(
            bus.trace.writes,
            [
                event(0x8123, Some(8), 0x0300, 7),
                event(0x0400, None, 0x0301, 2)
            ]
        );
    }
}
