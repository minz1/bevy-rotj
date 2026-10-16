//! 2A03 CPU: the 151 official 6502 opcodes, no decimal mode.

/// The CPU's view of the address space.
pub(crate) trait Bus {
    /// Reads one byte.
    fn read(&mut self, addr: u16) -> u8;
    /// Writes one byte.
    fn write(&mut self, addr: u16, value: u8);
}

const C: u8 = 0x01;
const Z: u8 = 0x02;
const I: u8 = 0x04;
const D: u8 = 0x08;
const B: u8 = 0x10;
const U: u8 = 0x20;
const V: u8 = 0x40;
const N: u8 = 0x80;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Imp,
    Acc,
    Imm,
    Zp,
    Zpx,
    Zpy,
    Abs,
    Abx,
    Aby,
    Ind,
    Izx,
    Izy,
    Rel,
}

#[derive(Clone, Copy)]
enum Op {
    Adc,
    And,
    Asl,
    Bcc,
    Bcs,
    Beq,
    Bit,
    Bmi,
    Bne,
    Bpl,
    Brk,
    Bvc,
    Bvs,
    Clc,
    Cld,
    Cli,
    Clv,
    Cmp,
    Cpx,
    Cpy,
    Dec,
    Dex,
    Dey,
    Eor,
    Inc,
    Inx,
    Iny,
    Jmp,
    Jsr,
    Lda,
    Ldx,
    Ldy,
    Lsr,
    Nop,
    Ora,
    Pha,
    Php,
    Pla,
    Plp,
    Rol,
    Ror,
    Rti,
    Rts,
    Sbc,
    Sec,
    Sed,
    Sei,
    Sta,
    Stx,
    Sty,
    Tax,
    Tay,
    Tsx,
    Txa,
    Txs,
    Tya,
}

use Mode::{Abs, Abx, Aby, Acc, Imm, Imp, Ind, Izx, Izy, Rel, Zp, Zpx, Zpy};
use Op::{
    Adc, And, Asl, Bcc, Bcs, Beq, Bit, Bmi, Bne, Bpl, Brk, Bvc, Bvs, Clc, Cld, Cli, Clv, Cmp, Cpx,
    Cpy, Dec, Dex, Dey, Eor, Inc, Inx, Iny, Jmp, Jsr, Lda, Ldx, Ldy, Lsr, Nop, Ora, Pha, Php, Pla,
    Plp, Rol, Ror, Rti, Rts, Sbc, Sec, Sed, Sei, Sta, Stx, Sty, Tax, Tay, Tsx, Txa, Txs, Tya,
};

/// Every official opcode: (byte, operation, addressing mode, base cycles).
const OPCODES: [(u8, Op, Mode, u8); 151] = [
    (0x69, Adc, Imm, 2),
    (0x65, Adc, Zp, 3),
    (0x75, Adc, Zpx, 4),
    (0x6D, Adc, Abs, 4),
    (0x7D, Adc, Abx, 4),
    (0x79, Adc, Aby, 4),
    (0x61, Adc, Izx, 6),
    (0x71, Adc, Izy, 5),
    (0x29, And, Imm, 2),
    (0x25, And, Zp, 3),
    (0x35, And, Zpx, 4),
    (0x2D, And, Abs, 4),
    (0x3D, And, Abx, 4),
    (0x39, And, Aby, 4),
    (0x21, And, Izx, 6),
    (0x31, And, Izy, 5),
    (0x0A, Asl, Acc, 2),
    (0x06, Asl, Zp, 5),
    (0x16, Asl, Zpx, 6),
    (0x0E, Asl, Abs, 6),
    (0x1E, Asl, Abx, 7),
    (0x90, Bcc, Rel, 2),
    (0xB0, Bcs, Rel, 2),
    (0xF0, Beq, Rel, 2),
    (0x30, Bmi, Rel, 2),
    (0xD0, Bne, Rel, 2),
    (0x10, Bpl, Rel, 2),
    (0x50, Bvc, Rel, 2),
    (0x70, Bvs, Rel, 2),
    (0x24, Bit, Zp, 3),
    (0x2C, Bit, Abs, 4),
    (0x00, Brk, Imp, 7),
    (0x18, Clc, Imp, 2),
    (0xD8, Cld, Imp, 2),
    (0x58, Cli, Imp, 2),
    (0xB8, Clv, Imp, 2),
    (0xC9, Cmp, Imm, 2),
    (0xC5, Cmp, Zp, 3),
    (0xD5, Cmp, Zpx, 4),
    (0xCD, Cmp, Abs, 4),
    (0xDD, Cmp, Abx, 4),
    (0xD9, Cmp, Aby, 4),
    (0xC1, Cmp, Izx, 6),
    (0xD1, Cmp, Izy, 5),
    (0xE0, Cpx, Imm, 2),
    (0xE4, Cpx, Zp, 3),
    (0xEC, Cpx, Abs, 4),
    (0xC0, Cpy, Imm, 2),
    (0xC4, Cpy, Zp, 3),
    (0xCC, Cpy, Abs, 4),
    (0xC6, Dec, Zp, 5),
    (0xD6, Dec, Zpx, 6),
    (0xCE, Dec, Abs, 6),
    (0xDE, Dec, Abx, 7),
    (0xCA, Dex, Imp, 2),
    (0x88, Dey, Imp, 2),
    (0x49, Eor, Imm, 2),
    (0x45, Eor, Zp, 3),
    (0x55, Eor, Zpx, 4),
    (0x4D, Eor, Abs, 4),
    (0x5D, Eor, Abx, 4),
    (0x59, Eor, Aby, 4),
    (0x41, Eor, Izx, 6),
    (0x51, Eor, Izy, 5),
    (0xE6, Inc, Zp, 5),
    (0xF6, Inc, Zpx, 6),
    (0xEE, Inc, Abs, 6),
    (0xFE, Inc, Abx, 7),
    (0xE8, Inx, Imp, 2),
    (0xC8, Iny, Imp, 2),
    (0x4C, Jmp, Abs, 3),
    (0x6C, Jmp, Ind, 5),
    (0x20, Jsr, Abs, 6),
    (0xA9, Lda, Imm, 2),
    (0xA5, Lda, Zp, 3),
    (0xB5, Lda, Zpx, 4),
    (0xAD, Lda, Abs, 4),
    (0xBD, Lda, Abx, 4),
    (0xB9, Lda, Aby, 4),
    (0xA1, Lda, Izx, 6),
    (0xB1, Lda, Izy, 5),
    (0xA2, Ldx, Imm, 2),
    (0xA6, Ldx, Zp, 3),
    (0xB6, Ldx, Zpy, 4),
    (0xAE, Ldx, Abs, 4),
    (0xBE, Ldx, Aby, 4),
    (0xA0, Ldy, Imm, 2),
    (0xA4, Ldy, Zp, 3),
    (0xB4, Ldy, Zpx, 4),
    (0xAC, Ldy, Abs, 4),
    (0xBC, Ldy, Abx, 4),
    (0x4A, Lsr, Acc, 2),
    (0x46, Lsr, Zp, 5),
    (0x56, Lsr, Zpx, 6),
    (0x4E, Lsr, Abs, 6),
    (0x5E, Lsr, Abx, 7),
    (0xEA, Nop, Imp, 2),
    (0x09, Ora, Imm, 2),
    (0x05, Ora, Zp, 3),
    (0x15, Ora, Zpx, 4),
    (0x0D, Ora, Abs, 4),
    (0x1D, Ora, Abx, 4),
    (0x19, Ora, Aby, 4),
    (0x01, Ora, Izx, 6),
    (0x11, Ora, Izy, 5),
    (0x48, Pha, Imp, 3),
    (0x08, Php, Imp, 3),
    (0x68, Pla, Imp, 4),
    (0x28, Plp, Imp, 4),
    (0x2A, Rol, Acc, 2),
    (0x26, Rol, Zp, 5),
    (0x36, Rol, Zpx, 6),
    (0x2E, Rol, Abs, 6),
    (0x3E, Rol, Abx, 7),
    (0x6A, Ror, Acc, 2),
    (0x66, Ror, Zp, 5),
    (0x76, Ror, Zpx, 6),
    (0x6E, Ror, Abs, 6),
    (0x7E, Ror, Abx, 7),
    (0x40, Rti, Imp, 6),
    (0x60, Rts, Imp, 6),
    (0xE9, Sbc, Imm, 2),
    (0xE5, Sbc, Zp, 3),
    (0xF5, Sbc, Zpx, 4),
    (0xED, Sbc, Abs, 4),
    (0xFD, Sbc, Abx, 4),
    (0xF9, Sbc, Aby, 4),
    (0xE1, Sbc, Izx, 6),
    (0xF1, Sbc, Izy, 5),
    (0x38, Sec, Imp, 2),
    (0xF8, Sed, Imp, 2),
    (0x78, Sei, Imp, 2),
    (0x85, Sta, Zp, 3),
    (0x95, Sta, Zpx, 4),
    (0x8D, Sta, Abs, 4),
    (0x9D, Sta, Abx, 5),
    (0x99, Sta, Aby, 5),
    (0x81, Sta, Izx, 6),
    (0x91, Sta, Izy, 6),
    (0x86, Stx, Zp, 3),
    (0x96, Stx, Zpy, 4),
    (0x8E, Stx, Abs, 4),
    (0x84, Sty, Zp, 3),
    (0x94, Sty, Zpx, 4),
    (0x8C, Sty, Abs, 4),
    (0xAA, Tax, Imp, 2),
    (0xA8, Tay, Imp, 2),
    (0xBA, Tsx, Imp, 2),
    (0x8A, Txa, Imp, 2),
    (0x9A, Txs, Imp, 2),
    (0x98, Tya, Imp, 2),
];

const DECODE: [Option<(Op, Mode, u8)>; 256] = {
    let mut table = [None; 256];
    let mut i = 0;
    while i < OPCODES.len() {
        let (code, op, mode, cycles) = OPCODES[i];
        table[code as usize] = Some((op, mode, cycles));
        i += 1;
    }
    table
};

/// Cycles an opcode costs before page-cross and branch penalties (0 when unofficial).
#[must_use]
pub(crate) fn base_cycles(opcode: u8) -> u32 {
    DECODE[usize::from(opcode)].map_or(0, |(_, _, cycles)| u32::from(cycles))
}

/// I, the interrupt-disable flag.
pub(crate) const IRQ_DISABLE: u8 = I;

/// 2A03 register file and cycle counter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cpu {
    /// Accumulator.
    pub a: u8,
    /// X index.
    pub x: u8,
    /// Y index.
    pub y: u8,
    /// Stack pointer (page 1).
    pub s: u8,
    /// Status flags NV-BDIZC (B is never stored; it exists only on the stack).
    pub p: u8,
    /// Program counter.
    pub pc: u16,
    /// CPU cycles since power-on.
    pub cycles: u64,
}

impl Cpu {
    /// Power-on registers; call `reset` before stepping.
    #[must_use]
    pub(crate) fn new() -> Cpu {
        Cpu {
            a: 0,
            x: 0,
            y: 0,
            s: 0,
            p: U,
            pc: 0,
            cycles: 0,
        }
    }

    /// Reset: PC from `$FFFC`, S drops by 3, I set; free, as in fceux.
    pub(crate) fn reset(&mut self, bus: &mut impl Bus) {
        self.s = self.s.wrapping_sub(3);
        self.p |= I;
        self.pc = read_word(bus, 0xFFFC);
    }

    /// Takes the NMI: pushes PC and P (B clear), jumps through `$FFFA`, 7 cycles.
    pub(crate) fn nmi(&mut self, bus: &mut impl Bus) {
        self.interrupt(bus, 0xFFFA, false);
        self.cycles += 7;
    }

    /// Takes an IRQ through `$FFFE` (7 cycles); the caller decides whether I allows it.
    pub(crate) fn irq(&mut self, bus: &mut impl Bus) {
        self.interrupt(bus, 0xFFFE, false);
        self.cycles += 7;
    }

    /// Executes one instruction and returns its cycle count; panics on an unofficial opcode.
    pub(crate) fn step(&mut self, bus: &mut impl Bus) -> u32 {
        let at = self.pc;
        let opcode = self.fetch(bus);
        let Some((op, mode, base)) = DECODE[usize::from(opcode)] else {
            panic!("unofficial opcode ${opcode:02X} at ${at:04X}");
        };
        let mut cycles = u32::from(base);
        match op {
            Adc | Sbc | And | Ora | Eor | Cmp | Cpx | Cpy | Bit | Lda | Ldx | Ldy => {
                let (addr, crossed) = self.address(bus, mode);
                cycles += u32::from(crossed);
                let value = bus.read(addr);
                self.read_op(op, value);
            }
            Sta | Stx | Sty => {
                let addr = self.address(bus, mode).0;
                let value = match op {
                    Sta => self.a,
                    Stx => self.x,
                    _ => self.y,
                };
                bus.write(addr, value);
            }
            Asl | Lsr | Rol | Ror | Inc | Dec => {
                if mode == Acc {
                    self.a = self.modify_op(op, self.a);
                } else {
                    let addr = self.address(bus, mode).0;
                    let value = bus.read(addr);
                    let value = self.modify_op(op, value);
                    bus.write(addr, value);
                }
            }
            Bcc | Bcs | Bne | Beq | Bpl | Bmi | Bvc | Bvs => {
                let (mask, set) = match op {
                    Bcc => (C, false),
                    Bcs => (C, true),
                    Bne => (Z, false),
                    Beq => (Z, true),
                    Bpl => (N, false),
                    Bmi => (N, true),
                    Bvc => (V, false),
                    _ => (V, true),
                };
                let offset = self.fetch(bus).cast_signed();
                if (self.p & mask != 0) == set {
                    let target = self.pc.wrapping_add_signed(i16::from(offset));
                    cycles += if target & 0xFF00 == self.pc & 0xFF00 {
                        1
                    } else {
                        2
                    };
                    self.pc = target;
                }
            }
            Jmp => self.pc = self.address(bus, mode).0,
            Jsr => {
                let target = self.address(bus, mode).0;
                self.push_word(bus, self.pc.wrapping_sub(1));
                self.pc = target;
            }
            Rts => self.pc = self.pull_word(bus).wrapping_add(1),
            Brk => {
                self.pc = self.pc.wrapping_add(1);
                self.interrupt(bus, 0xFFFE, true);
            }
            Rti => {
                let status = self.pull(bus);
                self.set_status(status);
                self.pc = self.pull_word(bus);
            }
            Pha => self.push(bus, self.a),
            Php => self.push(bus, self.p | B | U),
            Pla => {
                let value = self.pull(bus);
                self.a = self.nz(value);
            }
            Plp => {
                let status = self.pull(bus);
                self.set_status(status);
            }
            _ => self.implied_op(op),
        }
        self.cycles += u64::from(cycles);
        cycles
    }

    fn read_op(&mut self, op: Op, value: u8) {
        match op {
            Adc => self.adc(value),
            Sbc => self.adc(!value),
            And => self.a = self.nz(self.a & value),
            Ora => self.a = self.nz(self.a | value),
            Eor => self.a = self.nz(self.a ^ value),
            Cmp => self.compare(self.a, value),
            Cpx => self.compare(self.x, value),
            Cpy => self.compare(self.y, value),
            Bit => {
                self.set(Z, self.a & value == 0);
                self.p = (self.p & !(N | V)) | (value & (N | V));
            }
            Lda => self.a = self.nz(value),
            Ldx => self.x = self.nz(value),
            _ => self.y = self.nz(value),
        }
    }

    fn modify_op(&mut self, op: Op, value: u8) -> u8 {
        let carry_in = self.p & C;
        let result = match op {
            Asl => value << 1,
            Lsr => value >> 1,
            Rol => (value << 1) | carry_in,
            Ror => (value >> 1) | (carry_in << 7),
            Inc => value.wrapping_add(1),
            _ => value.wrapping_sub(1),
        };
        match op {
            Asl | Rol => self.set(C, value & 0x80 != 0),
            Lsr | Ror => self.set(C, value & 0x01 != 0),
            _ => {}
        }
        self.nz(result)
    }

    fn implied_op(&mut self, op: Op) {
        match op {
            Inx => self.x = self.nz(self.x.wrapping_add(1)),
            Iny => self.y = self.nz(self.y.wrapping_add(1)),
            Dex => self.x = self.nz(self.x.wrapping_sub(1)),
            Dey => self.y = self.nz(self.y.wrapping_sub(1)),
            Tax => self.x = self.nz(self.a),
            Tay => self.y = self.nz(self.a),
            Tsx => self.x = self.nz(self.s),
            Txa => self.a = self.nz(self.x),
            Tya => self.a = self.nz(self.y),
            Txs => self.s = self.x,
            Clc => self.p &= !C,
            Cld => self.p &= !D,
            Cli => self.p &= !I,
            Clv => self.p &= !V,
            Sec => self.p |= C,
            Sed => self.p |= D,
            Sei => self.p |= I,
            _ => {}
        }
    }

    fn fetch(&mut self, bus: &mut impl Bus) -> u8 {
        let value = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        value
    }

    fn fetch_word(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = self.fetch(bus);
        let hi = self.fetch(bus);
        u16::from_le_bytes([lo, hi])
    }

    /// Effective address of a memory operand, and whether indexing crossed a page.
    fn address(&mut self, bus: &mut impl Bus, mode: Mode) -> (u16, bool) {
        match mode {
            Mode::Imm => {
                let at = self.pc;
                self.pc = self.pc.wrapping_add(1);
                (at, false)
            }
            Mode::Zp => (u16::from(self.fetch(bus)), false),
            Mode::Zpx => (u16::from(self.fetch(bus).wrapping_add(self.x)), false),
            Mode::Zpy => (u16::from(self.fetch(bus).wrapping_add(self.y)), false),
            Mode::Abs => (self.fetch_word(bus), false),
            Mode::Abx => indexed(self.fetch_word(bus), self.x),
            Mode::Aby => indexed(self.fetch_word(bus), self.y),
            Mode::Ind => {
                let ptr = self.fetch_word(bus);
                (read_word_in_page(bus, ptr), false)
            }
            Mode::Izx => {
                let zero_page = self.fetch(bus).wrapping_add(self.x);
                (read_word_in_page(bus, u16::from(zero_page)), false)
            }
            Mode::Izy => {
                let zero_page = self.fetch(bus);
                indexed(read_word_in_page(bus, u16::from(zero_page)), self.y)
            }
            Mode::Imp | Mode::Acc | Mode::Rel => unreachable!("no memory operand"),
        }
    }

    fn adc(&mut self, value: u8) {
        let sum = u16::from(self.a) + u16::from(value) + u16::from(self.p & C);
        let [result, carry] = sum.to_le_bytes();
        self.set(C, carry != 0);
        self.set(V, (!(self.a ^ value) & (self.a ^ result) & 0x80) != 0);
        self.a = self.nz(result);
    }

    fn compare(&mut self, reg: u8, value: u8) {
        self.set(C, reg >= value);
        self.nz(reg.wrapping_sub(value));
    }

    fn interrupt(&mut self, bus: &mut impl Bus, vector: u16, brk: bool) {
        self.push_word(bus, self.pc);
        self.push(bus, self.p | U | if brk { B } else { 0 });
        self.p |= I;
        self.pc = read_word(bus, vector);
    }

    fn set_status(&mut self, status: u8) {
        self.p = (status & !B) | U;
    }

    fn set(&mut self, mask: u8, on: bool) {
        if on {
            self.p |= mask;
        } else {
            self.p &= !mask;
        }
    }

    fn nz(&mut self, value: u8) -> u8 {
        self.set(Z, value == 0);
        self.set(N, value & 0x80 != 0);
        value
    }

    fn push(&mut self, bus: &mut impl Bus, value: u8) {
        bus.write(0x0100 | u16::from(self.s), value);
        self.s = self.s.wrapping_sub(1);
    }

    fn pull(&mut self, bus: &mut impl Bus) -> u8 {
        self.s = self.s.wrapping_add(1);
        bus.read(0x0100 | u16::from(self.s))
    }

    fn push_word(&mut self, bus: &mut impl Bus, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.push(bus, hi);
        self.push(bus, lo);
    }

    fn pull_word(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = self.pull(bus);
        let hi = self.pull(bus);
        u16::from_le_bytes([lo, hi])
    }
}

fn indexed(base: u16, index: u8) -> (u16, bool) {
    let addr = base.wrapping_add(u16::from(index));
    (addr, addr & 0xFF00 != base & 0xFF00)
}

fn read_word(bus: &mut impl Bus, addr: u16) -> u16 {
    u16::from_le_bytes([bus.read(addr), bus.read(addr.wrapping_add(1))])
}

/// A pointer fetch whose high byte comes from the same page (the `JMP ($xxFF)` and zero-page wrap).
fn read_word_in_page(bus: &mut impl Bus, addr: u16) -> u16 {
    let hi_addr = (addr & 0xFF00) | (addr.wrapping_add(1) & 0x00FF);
    u16::from_le_bytes([bus.read(addr), bus.read(hi_addr)])
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FlatBus(Vec<u8>);

    impl Bus for FlatBus {
        fn read(&mut self, addr: u16) -> u8 {
            self.0[usize::from(addr)]
        }
        fn write(&mut self, addr: u16, value: u8) {
            self.0[usize::from(addr)] = value;
        }
    }

    fn boot(at: u16, program: &[u8]) -> (Cpu, FlatBus) {
        let mut bus = FlatBus(vec![0; 0x10000]);
        let start = usize::from(at);
        bus.0[start..start + program.len()].copy_from_slice(program);
        bus.0[0xFFFC..0xFFFE].copy_from_slice(&at.to_le_bytes());
        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        (cpu, bus)
    }

    fn flag(cpu: &Cpu, mask: u8) -> bool {
        cpu.p & mask != 0
    }

    #[test]
    fn adc_sets_carry_and_overflow() {
        let (mut cpu, mut bus) = boot(
            0x8000,
            &[0xA9, 0x50, 0x69, 0x50, 0xA9, 0xFF, 0x18, 0x69, 0x01],
        );
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert_eq!(cpu.a, 0xA0);
        assert!(flag(&cpu, V) && !flag(&cpu, C) && flag(&cpu, N));
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert_eq!(cpu.a, 0x00);
        assert!(flag(&cpu, C) && flag(&cpu, Z) && !flag(&cpu, V));
    }

    #[test]
    fn sbc_borrows() {
        let (mut cpu, mut bus) = boot(
            0x8000,
            &[0x38, 0xA9, 0x00, 0xE9, 0x01, 0x38, 0xA9, 0x05, 0xE9, 0x03],
        );
        for _ in 0..3 {
            cpu.step(&mut bus);
        }
        assert_eq!(cpu.a, 0xFF);
        assert!(!flag(&cpu, C) && flag(&cpu, N));
        for _ in 0..3 {
            cpu.step(&mut bus);
        }
        assert_eq!(cpu.a, 0x02);
        assert!(flag(&cpu, C) && !flag(&cpu, N));
    }

    #[test]
    fn branch_cycles() {
        let (mut cpu, mut bus) = boot(0x8000, &[0x38, 0x90, 0x10, 0x18, 0x90, 0x02]);
        cpu.step(&mut bus);
        assert_eq!(cpu.step(&mut bus), 2);
        assert_eq!(cpu.pc, 0x8003);
        cpu.step(&mut bus);
        assert_eq!(cpu.step(&mut bus), 3);
        assert_eq!(cpu.pc, 0x8008);

        let (mut cpu, mut bus) = boot(0x80FD, &[0x90, 0x10]);
        assert_eq!(cpu.step(&mut bus), 4);
        assert_eq!(cpu.pc, 0x810F);
    }

    #[test]
    fn page_cross_adds_a_cycle() {
        let (mut cpu, mut bus) = boot(0x8000, &[0xA2, 0x01, 0xBD, 0xFF, 0x12, 0xBD, 0x00, 0x12]);
        cpu.step(&mut bus);
        assert_eq!(cpu.step(&mut bus), 5);
        assert_eq!(cpu.step(&mut bus), 4);
    }

    #[test]
    fn jsr_rts_round_trip() {
        let (mut cpu, mut bus) = boot(0x8000, &[0x20, 0x00, 0x90]);
        bus.0[0x9000] = 0x60;
        let stack = cpu.s;
        assert_eq!(cpu.step(&mut bus), 6);
        assert_eq!(cpu.pc, 0x9000);
        assert_eq!(cpu.s, stack.wrapping_sub(2));
        assert_eq!(bus.0[0x0100 + usize::from(stack)], 0x80);
        assert_eq!(bus.0[0x0100 + usize::from(stack) - 1], 0x02);
        assert_eq!(cpu.step(&mut bus), 6);
        assert_eq!(cpu.pc, 0x8003);
        assert_eq!(cpu.s, stack);
    }

    #[test]
    fn brk_rti_round_trip() {
        let (mut cpu, mut bus) = boot(0x8000, &[0x58, 0x00, 0xEA]);
        bus.0[0xFFFE..0x10000].copy_from_slice(&[0x00, 0x90]);
        bus.0[0x9000] = 0x40;
        cpu.step(&mut bus);
        let (status, stack) = (cpu.p, cpu.s);
        assert_eq!(cpu.step(&mut bus), 7);
        assert_eq!(cpu.pc, 0x9000);
        assert!(flag(&cpu, I) && !flag(&cpu, B));
        let pushed = bus.0[0x0100 + usize::from(stack) - 2];
        assert_eq!(pushed, status | B | U);
        assert_eq!(cpu.step(&mut bus), 6);
        assert_eq!(cpu.pc, 0x8003);
        assert_eq!((cpu.p, cpu.s), (status, stack));
    }

    #[test]
    fn jmp_indirect_wraps_within_page() {
        let (mut cpu, mut bus) = boot(0x8000, &[0x6C, 0xFF, 0x02]);
        bus.0[0x02FF] = 0x34;
        bus.0[0x0200] = 0x12;
        bus.0[0x0300] = 0x56;
        assert_eq!(cpu.step(&mut bus), 5);
        assert_eq!(cpu.pc, 0x1234);
    }

    #[test]
    fn zero_page_indexing_wraps() {
        let (mut cpu, mut bus) = boot(0x8000, &[0xA2, 0x02, 0xB5, 0xFF, 0xA0, 0x05, 0xB1, 0xFF]);
        bus.0[0x0001] = 0xAB;
        bus.0[0x0101] = 0xEE;
        bus.0[0x00FF] = 0x00;
        bus.0[0x0000] = 0x03;
        bus.0[0x0305] = 0xCD;
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert_eq!(cpu.a, 0xAB);
        cpu.step(&mut bus);
        assert_eq!(cpu.step(&mut bus), 5);
        assert_eq!(cpu.a, 0xCD);
    }

    #[test]
    fn cmp_flags() {
        let (mut cpu, mut bus) = boot(0x8000, &[0xA9, 0x40, 0xC9, 0x40, 0xC9, 0x41, 0xC9, 0x30]);
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert!(flag(&cpu, Z) && flag(&cpu, C) && !flag(&cpu, N));
        cpu.step(&mut bus);
        assert!(!flag(&cpu, Z) && !flag(&cpu, C) && flag(&cpu, N));
        cpu.step(&mut bus);
        assert!(!flag(&cpu, Z) && flag(&cpu, C) && !flag(&cpu, N));
    }

    #[test]
    fn shifts_and_rotates_through_memory() {
        let (mut cpu, mut bus) = boot(0x8000, &[0x38, 0x66, 0x10, 0x06, 0x10, 0x4A]);
        bus.0[0x10] = 0x02;
        cpu.step(&mut bus);
        assert_eq!(cpu.step(&mut bus), 5);
        assert_eq!(bus.0[0x10], 0x81);
        assert!(!flag(&cpu, C) && flag(&cpu, N));
        cpu.step(&mut bus);
        assert_eq!(bus.0[0x10], 0x02);
        assert!(flag(&cpu, C) && !flag(&cpu, N));
        cpu.a = 0x01;
        assert_eq!(cpu.step(&mut bus), 2);
        assert_eq!(cpu.a, 0x00);
        assert!(flag(&cpu, C) && flag(&cpu, Z));
    }

    #[test]
    fn stores_and_rmw_never_pay_page_cross() {
        let (mut cpu, mut bus) = boot(0x8000, &[0xA2, 0x01, 0x9D, 0xFF, 0x12, 0xFE, 0xFF, 0x12]);
        cpu.step(&mut bus);
        assert_eq!(cpu.step(&mut bus), 5);
        assert_eq!(cpu.step(&mut bus), 7);
        assert_eq!(bus.0[0x1300], 0x01);
    }

    #[test]
    fn php_pushes_b_and_u_plp_drops_b() {
        let (mut cpu, mut bus) = boot(0x8000, &[0x08, 0x28]);
        let stack = cpu.s;
        cpu.step(&mut bus);
        assert_eq!(bus.0[0x0100 + usize::from(stack)], cpu.p | B | U);
        bus.0[0x0100 + usize::from(stack)] = 0xFF;
        cpu.step(&mut bus);
        assert_eq!(cpu.p, !B);
    }

    #[test]
    fn indexed_indirect_pointer_wraps_in_zero_page() {
        let (mut cpu, mut bus) = boot(0x8000, &[0xA2, 0x01, 0xA1, 0xFE]);
        bus.0[0x00FF] = 0x34;
        bus.0[0x0000] = 0x12;
        bus.0[0x0100] = 0x56;
        bus.0[0x1234] = 0x99;
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert_eq!(cpu.a, 0x99);
    }
}
