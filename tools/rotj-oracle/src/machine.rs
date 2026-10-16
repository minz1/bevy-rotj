//! Whole frames scheduled as fceux 2.6.5's old PPU core does (see the spec's timing table).

use crate::bus::{ExecEvent, NesBus, PeekEvent, WriteLog};
use crate::cart::Cart;
use crate::cpu::{Bus, Cpu, IRQ_DISABLE, base_cycles};
use crate::ppu::Line;

const DOTS_PER_LINE: i32 = 341;
const LINES_PER_FRAME: i32 = 262;
/// `X6502_Run` counts a dot as 16 and a CPU cycle as 48.
const COUNT_PER_DOT: i32 = 16;
const COUNT_PER_CYCLE: i32 = 48;
/// fceux sets the vblank flag, then runs 12 dots before raising the NMI.
const NMI_DELAY_DOTS: i32 = 12;
/// The dot at which fceux's old core ends a line: it steps the render address down a
/// line and reloads its horizontal scroll, so a write later in the line (in hblank)
/// shapes the next line from its first pixel.
const LINE_END_DOT: i32 = 256;
/// fceux runs this many whole frames without vblank after power-on (`ppudead`).
const DEAD_FRAMES: u8 = 2;

/// What one frame did beyond RAM.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FrameLog {
    /// APU register writes, in order.
    pub apu: Vec<WriteLog>,
    /// CPU cycles the frame took, stalls included.
    pub cycles: u32,
    /// NMIs taken.
    pub nmis: u32,
    /// The 240 visible lines as the PPU drew them (empty for a frame without rendering).
    pub lines: Vec<Line>,
    /// Lines a write may split: one made while the line was drawn, or in the hblank
    /// before it (fceux can still draw that line's left part the old way). fceux draws
    /// such a line with the first state up to some pixel, then the second.
    pub splits: Vec<(usize, Line, Line)>,
}

/// The whole console; snapshots are clones.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Machine {
    /// The CPU.
    pub cpu: Cpu,
    /// Everything the CPU addresses.
    pub bus: NesBus,
    /// Frames completed since power-on.
    pub frame: u64,
    /// NMIs taken since power-on.
    pub nmis: u64,
    /// IRQs taken since power-on.
    pub irqs: u64,
    count: i32,
    unhooked: u32,
    prev_p: u8,
    nmi: bool,
    nmi_delayed: bool,
    dead_frames: u8,
    short_frame: bool,
    /// CPU cycles at the start of the frame being run.
    frame_start: u64,
    log: FrameLog,
}

impl Machine {
    /// Powers on with fceux's RAM pattern (`00 00 00 00 FF FF FF FF` repeated) and zeroed WRAM.
    #[must_use]
    pub fn power_on(cart: Cart) -> Machine {
        let mut bus = NesBus::new(cart);
        for (i, byte) in bus.ram.iter_mut().enumerate() {
            *byte = if i & 4 == 0 { 0x00 } else { 0xFF };
        }
        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        Machine {
            prev_p: cpu.p,
            cpu,
            bus,
            frame: 0,
            nmis: 0,
            irqs: 0,
            count: 0,
            unhooked: 0,
            nmi: false,
            nmi_delayed: false,
            dead_frames: DEAD_FRAMES,
            short_frame: false,
            frame_start: 0,
            log: FrameLog::default(),
        }
    }

    /// Runs one fceux frame with `pad` as controller 1 (unless a playtest feeds the pads).
    pub fn run_frame(&mut self, pad: u8) -> FrameLog {
        if self.bus.trace.playtest.is_none() {
            self.bus.pad = pad;
        }
        self.bus.trace.frame = self.frame + 1;
        let start = self.cpu.cycles;
        self.frame_start = start;
        if self.dead_frames > 0 {
            self.dead_frames -= 1;
            self.run(LINES_PER_FRAME * DOTS_PER_LINE);
        } else {
            self.run(DOTS_PER_LINE);
            self.bus.ppu.set_vblank();
            self.run(NMI_DELAY_DOTS);
            if self.bus.ppu.ctrl & 0x80 != 0 {
                self.nmi = true;
            }
            self.run(20 * DOTS_PER_LINE - NMI_DELAY_DOTS);
            self.bus.ppu.clear_vblank();
            self.run(DOTS_PER_LINE - i32::from(self.short_frame));
            self.short_frame = !self.short_frame;
            self.bus.ppu.start_render();
            let mut lines = Vec::with_capacity(240);
            let mut splits = Vec::new();
            let mut previous: Option<Line> = None;
            for y in 0..240 {
                self.bus.ppu.drawing = (self.bus.ppu.mask & 0x18 != 0)
                    .then(|| u8::try_from(y).expect("a visible line"));
                let mut line = self.bus.ppu.line(&self.bus.fme7);
                if let Some(before) = previous {
                    line.sprite_chr = before.chr;
                }
                lines.push(line);
                if let Some(before) = previous.filter(|before| !before.same_but_scroll(&line)) {
                    splits.push((
                        y,
                        Line {
                            v: line.v,
                            ..before
                        },
                        line,
                    ));
                }
                self.run(LINE_END_DOT);
                let after = Line {
                    sprite_chr: line.sprite_chr,
                    ..self.bus.ppu.line(&self.bus.fme7)
                };
                if after != line {
                    splits.push((y, line, after));
                }
                previous = Some(after);
                self.bus.ppu.end_line();
                self.run(DOTS_PER_LINE - LINE_END_DOT);
            }
            self.bus.ppu.drawing = None;
            self.log.lines = lines;
            self.log.splits = splits;
        }
        self.frame += 1;
        let mut log = std::mem::take(&mut self.log);
        log.apu = std::mem::take(&mut self.bus.apu_log);
        log.cycles = u32::try_from(self.cpu.cycles - start).expect("a frame fits in u32 cycles");
        log
    }

    fn run(&mut self, dots: i32) {
        self.count += dots * COUNT_PER_DOT;
        while self.count > 0 {
            if self.bus.ppu.nmi_pending {
                self.bus.ppu.nmi_pending = false;
                self.nmi_delayed = true;
            }
            if self.interrupt() && self.count <= 0 {
                self.prev_p = self.cpu.p;
                return;
            }
            self.instruction();
        }
    }

    /// fceux's interrupt check at an instruction boundary; true when one was taken.
    fn interrupt(&mut self) -> bool {
        if self.nmi_delayed {
            self.nmi_delayed = false;
            self.nmi = true;
            false
        } else if self.nmi {
            self.nmi = false;
            self.cpu.nmi(&mut self.bus);
            self.log.nmis += 1;
            self.nmis += 1;
            self.add_cycles(7);
            true
        } else if self.bus.fme7.irq_pending && self.prev_p & IRQ_DISABLE == 0 {
            self.cpu.irq(&mut self.bus);
            self.irqs += 1;
            self.add_cycles(7);
            true
        } else {
            false
        }
    }

    fn instruction(&mut self) {
        let pc = self.cpu.pc;
        if self.bus.trace.exec.iter().any(|range| range.contains(&pc)) {
            let event = ExecEvent {
                frame: self.bus.trace.frame,
                pc,
                bank: self.bus.fme7.bank_at(pc, self.bus.cart.prg.len()),
                cycle: u32::try_from(self.cpu.cycles - self.frame_start).unwrap_or(u32::MAX),
            };
            self.bus.trace.execs.push(event);
        }
        if let Some(playtest) = &mut self.bus.trace.playtest {
            playtest.reach(pc, &mut self.bus.pad, &mut self.bus.ram);
        }
        for i in 0..self.bus.trace.pokes.len() {
            let (at, addr, value) = self.bus.trace.pokes[i];
            if at == pc {
                self.bus.write(addr, value);
            }
        }
        if self.bus.trace.peeks.contains(&pc) {
            let event = PeekEvent {
                frame: self.bus.trace.frame,
                pc,
                bank: self.bus.fme7.bank_at(pc, self.bus.cart.prg.len()),
                nmis: self.nmis,
                irqs: self.irqs,
                cpu: self.cpu.clone(),
                ram: Box::new(self.bus.ram),
                wram: Box::new(self.bus.wram),
                vram: Box::new(self.bus.ppu.vram),
                palette: self.bus.ppu.palette,
            };
            self.bus.trace.peeked.push(event);
        }
        self.prev_p = self.cpu.p;
        let opcode = self.bus.read(self.cpu.pc);
        let base = base_cycles(opcode);
        self.count -= i32::try_from(base).expect("small") * COUNT_PER_CYCLE;
        let hooked = std::mem::take(&mut self.unhooked) + base;
        self.bus.fme7.clock(hooked);
        let stall = self.bus.dmc.clock(hooked);
        self.add_cycles(stall);
        self.cpu.cycles += u64::from(stall);
        self.bus.pc = self.cpu.pc;
        self.bus.cycle = u32::try_from(self.cpu.cycles - self.frame_start).unwrap_or(u32::MAX);
        let cycles = self.cpu.step(&mut self.bus);
        let dma = std::mem::take(&mut self.bus.dma_stall);
        self.cpu.cycles += u64::from(dma);
        self.add_cycles(cycles - base + dma);
        match opcode {
            0x40 => self.prev_p = self.cpu.p,
            0x00 => self.prev_p |= IRQ_DISABLE,
            _ => {}
        }
    }

    fn add_cycles(&mut self, cycles: u32) {
        self.count -= i32::try_from(cycles).expect("small") * COUNT_PER_CYCLE;
        self.unhooked += cycles;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::{Playtest, StartBytes};
    use crate::cart::load_rom;

    fn rom() -> Option<Cart> {
        load_rom()
    }

    const HANDLER: u16 = 0xF000;

    /// A cart running `program` from `$E000`, with NMI and IRQ/BRK vectors at `HANDLER`.
    fn cart(program: &[u8], handler: &[u8]) -> Cart {
        let mut prg = vec![0; 0x20000];
        let last = prg.len() - 0x2000;
        prg[last..last + program.len()].copy_from_slice(program);
        let at = last + usize::from(HANDLER - 0xE000);
        prg[at..at + handler.len()].copy_from_slice(handler);
        let prg_len = prg.len();
        for (vector, target) in [(6, HANDLER), (4, 0xE000), (2, HANDLER)] {
            prg[prg_len - vector..prg_len - vector + 2].copy_from_slice(&target.to_le_bytes());
        }
        Cart {
            prg,
            chr: Vec::new(),
        }
    }

    /// The return address the first interrupt pushed (S starts at `$FD`).
    fn returned_to(machine: &Machine) -> u16 {
        u16::from_le_bytes([machine.bus.ram[0x1FC], machine.bus.ram[0x1FD]])
    }

    const SPIN: [u8; 3] = [0x4C, 0x00, 0xF0];

    #[test]
    fn power_on_ram_is_fceux_pattern() {
        let machine = Machine::power_on(cart(&[], &[]));
        assert_eq!(
            &machine.bus.ram[..16],
            &[
                0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF
            ]
        );
    }

    #[test]
    fn cli_delays_a_pending_irq_by_one_instruction() {
        let program = [0x58, 0xEA, 0xEA, 0x4C, 0x02, 0xE0];
        let mut machine = Machine::power_on(cart(&program, &SPIN));
        machine.bus.fme7.irq_pending = true;
        machine.run_frame(0);
        assert_eq!(returned_to(&machine), 0xE002);
    }

    #[test]
    fn sei_after_a_pending_irq_still_takes_it() {
        let program = [0x58, 0x78, 0xEA, 0x4C, 0x02, 0xE0];
        let mut machine = Machine::power_on(cart(&program, &SPIN));
        machine.bus.fme7.irq_pending = true;
        machine.run_frame(0);
        assert_eq!(returned_to(&machine), 0xE002);
        assert_eq!(machine.cpu.pc, HANDLER);
    }

    #[test]
    fn rti_updates_the_poll_flags_at_once() {
        let program = [0x58, 0xEA, 0xEA, 0x4C, 0x02, 0xE0];
        let mut machine = Machine::power_on(cart(&program, &[0x40]));
        machine.bus.fme7.irq_pending = true;
        machine.bus.trace.exec.push(0xE002..=0xE003);
        machine.run_frame(0);
        assert_eq!(machine.bus.trace.execs, []);
    }

    #[test]
    fn brk_sets_i_for_the_next_poll() {
        let program = [0x58, 0x00, 0xEA, 0x4C, 0x03, 0xE0];
        let mut machine = Machine::power_on(cart(&program, &SPIN));
        machine.bus.fme7.irq_pending = true;
        machine.run_frame(0);
        assert_eq!(machine.cpu.s, 0xFA);
        assert_eq!(returned_to(&machine), 0xE003);
        assert_ne!(machine.bus.ram[0x1FB] & 0x10, 0);
    }

    #[test]
    fn nmi_follows_a_2000_rise_in_vblank_after_one_instruction() {
        let program = [
            0xA9, 0x80, 0xA2, 0x00, 0x8D, 0x00, 0x20, 0x8E, 0x00, 0x20, 0xEA, 0x4C, 0x0A, 0xE0,
        ];
        let mut machine = Machine::power_on(cart(&program, &SPIN));
        machine.bus.ppu.status = 0x80;
        assert_eq!(machine.run_frame(0).nmis, 1);
        assert_eq!(returned_to(&machine), 0xE00A);
    }

    #[test]
    fn interrupt_at_budget_end_runs_no_instruction() {
        let mut machine = Machine::power_on(cart(&[0xEA], &[0xEA]));
        machine.count = 1;
        machine.nmi = true;
        machine.run(0);
        assert_eq!((machine.cpu.pc, machine.cpu.cycles), (HANDLER, 7));
    }

    #[test]
    fn nmi_arrives_12_dots_after_vblank() {
        let mut program = vec![0xA9, 0x80, 0x8D, 0x00, 0x20];
        program.resize(0x0F00, 0xEA);
        program.extend([0x4C, 0x05, 0xE0]);
        let mut machine = Machine::power_on(cart(&program, &SPIN));
        machine.run_frame(0);
        machine.run_frame(0);
        machine.bus.trace.exec.push(0xE005..=0xEEFF);
        machine.bus.trace.exec.push(HANDLER..=HANDLER);
        let start = machine.count;
        machine.run_frame(0);
        let nops = machine
            .bus
            .trace
            .execs
            .iter()
            .position(|event| event.pc == HANDLER);
        let to_vblank = (start + 341 * 16 + 95) / 96;
        let left = start + 341 * 16 - 96 * to_vblank;
        let to_nmi = (left + 12 * 16 + 95).max(0) / 96;
        assert_eq!(nops, usize::try_from(to_vblank + to_nmi).ok());
    }

    #[test]
    fn peeks_copy_memory_before_the_instruction() {
        let program = [
            0xA9, 0x80, 0x8D, 0x00, 0x20, 0xA9, 0x12, 0x8D, 0x10, 0x00, 0xA9, 0x34, 0x8D, 0x10,
            0x00, 0x4C, 0x05, 0xE0,
        ];
        let mut machine = Machine::power_on(cart(&program, &[0x40]));
        machine.bus.trace.peeks.push(0xE00C);
        for _ in 0..4 {
            machine.run_frame(0);
        }
        let peeked = &machine.bus.trace.peeked;
        assert!(peeked.len() > 100);
        assert!(
            peeked
                .iter()
                .all(|peek| peek.pc == 0xE00C && peek.ram[0x10] == 0x12)
        );
        assert!(
            peeked
                .iter()
                .all(|peek| peek.cpu.a == 0x34 && peek.cpu.pc == 0xE00C)
        );
        assert!(peeked.iter().all(|peek| peek.bank == Some(15)));
        assert!(peeked.windows(2).all(|pair| pair[0].nmis <= pair[1].nmis));
        assert!(machine.nmis >= 1);
        assert_eq!(peeked.last().map(|peek| peek.nmis), Some(machine.nmis));
        assert_eq!(peeked.last().map(|peek| peek.frame), Some(4));
    }

    #[test]
    fn boots_to_the_reset_vector() {
        let Some(cart) = rom() else { return };
        let prg_len = cart.prg.len();
        let reset = u16::from_le_bytes([cart.prg[prg_len - 4], cart.prg[prg_len - 3]]);
        assert_eq!(Machine::power_on(cart).cpu.pc, reset);
    }

    #[test]
    fn runs_a_frame_without_panicking() {
        let Some(cart) = rom() else { return };
        let mut machine = Machine::power_on(cart);
        for _ in 0..60 {
            let log = machine.run_frame(0);
            assert!(log.cycles.abs_diff(29_781) < 200, "{}", log.cycles);
        }
        assert_eq!(machine.frame, 60);
    }

    #[test]
    fn nmi_handler_runs_once_per_frame() {
        let Some(cart) = rom() else { return };
        let mut machine = Machine::power_on(cart);
        for _ in 0..20 {
            machine.run_frame(0);
        }
        for _ in 0..10 {
            assert_eq!(machine.run_frame(0).nmis, 1);
        }
    }

    #[test]
    fn snapshot_restores_exactly() {
        let Some(cart) = rom() else { return };
        let mut machine = Machine::power_on(cart);
        for _ in 0..100 {
            machine.run_frame(0);
        }
        let mut copy = machine.clone();
        for _ in 0..50 {
            machine.run_frame(0);
            copy.run_frame(0);
        }
        assert_eq!(machine, copy);
    }

    #[test]
    fn a_playtest_sets_the_pad_at_each_wait_and_pins_the_rng_at_its_return() {
        const WOKE: u16 = 0xE000;
        const WAIT: u16 = 0xE001;
        let program = [0xEA, 0xEA, 0x4C, 0x00, 0xE0];
        let mut machine = Machine::power_on(cart(&program, &[]));
        let step = |[hi, lo]: [u8; 2]| [hi.wrapping_add(1), lo ^ 0x0F];
        let mut playtest = Playtest::new(vec![0x81, 0x42], (WAIT, WOKE), 3, step);
        playtest.start = Some(StartBytes {
            rng: [0xAA, 0xBB],
            frame: 7,
            frame_odd: true,
            oam: [0x5A; 256],
            oam_next: 0x4C,
        });
        machine.bus.trace.playtest = Some(playtest);
        machine.bus.ram[0xF7] = 0x05;
        machine.instruction();
        assert_eq!(machine.bus.ram[0xF1], 7);
        assert_eq!(machine.bus.ram[0xF7], 0x85);
        assert_eq!(machine.bus.ram[0xF9..0xFB], [0xAA, 0xBB]);
        assert_eq!(machine.bus.ram[0xFB], 0x4C);
        assert!(machine.bus.ram[0x200..0x300].iter().all(|&b| b == 0x5A));
        let mut pads = Vec::new();
        for _ in 0..3 {
            machine.bus.ram[0xF9..0xFB].copy_from_slice(&[0x10, 0x20]);
            machine.instruction();
            pads.push(machine.bus.pad);
            machine.bus.ram[0xF9] = 0x55;
            machine.instruction();
            machine.instruction();
            assert_eq!(machine.bus.ram[0xF9..0xFB], [0x13, 0x2F]);
        }
        assert_eq!(pads, [0x81, 0x42, 0]);
        assert_eq!((machine.bus.ram[0xF1], machine.bus.ram[0xF7]), (7, 0x85));
        let playtest = machine.bus.trace.playtest.as_ref().expect("still set");
        assert_eq!(playtest.next, 3);
        machine.run_frame(0x10);
        assert_ne!(machine.bus.pad, 0x10, "run_frame's pad ignored");
    }

    #[test]
    fn pokes_and_exec_trace_follow_the_pc() {
        let program = [0xA5, 0x20, 0x8D, 0x00, 0x03, 0x4C, 0x00, 0xE0];
        let mut machine = Machine::power_on(cart(&program, &[]));
        machine.bus.trace.exec.push(0xE000..=0xE002);
        machine.bus.trace.pokes.push((0xE000, 0x0020, 7));
        machine.run_frame(0);
        assert_eq!(machine.bus.ram[0x0300], 7);
        let execs = &machine.bus.trace.execs;
        assert_eq!(
            (execs[0].frame, execs[0].pc, execs[0].bank, execs[1].pc),
            (1, 0xE000, Some(15), 0xE002)
        );
        assert_eq!(execs[1].cycle - execs[0].cycle, 3, "LDA zp takes 3 cycles");
        let traced = &machine.bus.trace.execs;
        assert!(
            traced
                .iter()
                .all(|event| event.pc == 0xE000 || event.pc == 0xE002)
        );
    }

    #[test]
    fn render_address_follows_the_ppu() {
        let program = [
            0xA9, 0x1E, 0x8D, 0x01, 0x20, 0xA0, 0x03, 0xA2, 0x00, 0xCA, 0xD0, 0xFD, 0x88, 0xD0,
            0xF8, 0xA9, 0x24, 0x8D, 0x06, 0x20, 0xA9, 0x00, 0x8D, 0x06, 0x20, 0x4C, 0x19, 0xE0,
        ];
        let mut machine = Machine::power_on(cart(&program, &SPIN));
        machine.dead_frames = 0;
        let log = machine.run_frame(0);
        let v: Vec<u16> = log.lines[10..15].iter().map(|line| line.v).collect();
        assert_eq!(v, [0x2020, 0x3020, 0x4020, 0x3400, 0x4400]);
        let drawn = log.lines[12];
        let after = Line { v: 0x2400, ..drawn };
        assert_eq!(log.splits, [(12, drawn, after)]);
    }
}
