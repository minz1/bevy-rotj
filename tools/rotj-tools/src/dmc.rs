//! DMC sample-instrument extraction for `rotj dmc`.
//!
//! The sound driver in PRG bank 10 (`disasm/bank0A.s`, handler at `$89C7`)
//! stores a 16-bit pointer from the song stream into RAM `$06F7/$06F9` when it
//! meets opcode `$F3`, then on a note plays the four-byte record at
//! `pointer + note * 4`, writing the bytes straight to the registers:
//!
//! | byte | register | meaning |
//! | --- | --- | --- |
//! | 0 | `$4010` | IRQ enable (bit 7), loop (bit 6), rate entry (bits 3..0) |
//! | 1 | `$4011` | direct load, 7-bit DAC level |
//! | 2 | `$4012` | sample address = `$C000 + value * 64` |
//! | 3 | `$4013` | sample length = `value * 16 + 1` bytes |
//!
//! Songs run with their PRG bank mapped at `$A000`, so a `$F3` command inside
//! that bank points at `pointer - $A000` within the same bank. The game's
//! samples themselves live in the fixed `$E000`-`$FFFF` bank (PRG bank 15).

use color_eyre::eyre::{Result, bail};

/// PRG bank size (8 KiB FME-7 window).
pub const BANK_SIZE: usize = 8192;
/// Fixed PRG bank at CPU `$E000`-`$FFFF`, where the DMC samples live.
pub const SAMPLE_BANK: u8 = 15;
/// CPU base address of [`SAMPLE_BANK`]'s image.
pub const SAMPLE_ROM_BASE: u16 = 0xE000;
/// A discovered table must decode at least this many records to be accepted.
const MIN_RECORDS: usize = 8;

/// One decoded four-byte DMC record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record {
    /// `$4010` value: IRQ/loop/rate.
    pub ctrl: u8,
    /// `$4011` value: direct load, 7-bit.
    pub load: u8,
    /// `$4012` value: sample address byte.
    pub addr: u8,
    /// `$4013` value: sample length byte.
    pub length: u8,
}

impl Record {
    /// `$4010`-bits 3..0 rate entry.
    #[must_use]
    pub const fn rate_index(self) -> u8 {
        self.ctrl & 0x0F
    }

    /// `$4010`-bit 7 IRQ enable.
    #[must_use]
    pub const fn irq(self) -> bool {
        self.ctrl & 0x80 != 0
    }

    /// `$4010`-bit 6 loop enable.
    #[must_use]
    pub const fn looping(self) -> bool {
        self.ctrl & 0x40 != 0
    }

    /// Sample start CPU address (`$4012` semantics).
    #[must_use]
    pub const fn cpu_addr(self) -> u16 {
        0xC000 + ((self.addr as u16) << 6)
    }

    /// Sample length in bytes (`$4013` semantics).
    #[must_use]
    pub const fn bytes(self) -> u16 {
        self.length as u16 * 16 + 1
    }

    /// ROM offset of the sample's first byte (fixed bank 15).
    #[must_use]
    pub const fn sample_rom_offset(self) -> usize {
        SAMPLE_BANK as usize * BANK_SIZE + (self.cpu_addr() - SAMPLE_ROM_BASE) as usize
    }
}

/// One distinct DMC record table plus a ROM place a song references it from.
#[derive(Debug, Clone)]
pub struct Table {
    /// PRG bank containing both the `$F3` command and the table.
    pub bank: u8,
    /// CPU (`$8000`-window) address of the `$F3` command.
    pub cmd_addr: u16,
    /// The 16-bit pointer the command stored (`$A000`-window).
    pub pointer: u16,
    /// Offset of the table inside `bank`.
    pub offset: usize,
    /// The records, in input (note) order.
    pub records: Vec<Record>,
}

/// Whether four bytes look like a DMC record.
///
/// Deliberately loose (the driver's `$4011` load and `$4013` length have no
/// reserved range): the control byte only uses rate/IRQ/loop bits and samples
/// are addressed at `$E000+`, so the byte after the last real record stops it.
fn is_record(bytes: [u8; 4]) -> bool {
    bytes[0] < 0x40 && bytes[2] >= 0x80 && bytes[3] != 0
}

/// Decodes the contiguous records starting at `offset`, stopping at the first
/// byte pattern that is not a record.
#[must_use]
pub fn decode_records(bank: &[u8], offset: usize) -> Vec<Record> {
    let mut records = Vec::new();
    let mut pos = offset;
    while pos + 4 <= bank.len() {
        let raw = [bank[pos], bank[pos + 1], bank[pos + 2], bank[pos + 3]];
        if !is_record(raw) {
            break;
        }
        records.push(Record {
            ctrl: raw[0],
            load: raw[1],
            addr: raw[2],
            length: raw[3],
        });
        pos += 4;
    }
    records
}

/// Finds every DMC record table referenced by the driver's `$F3` command.
#[must_use]
pub fn find_tables(prg: &[u8]) -> Vec<Table> {
    let mut tables = Vec::new();
    for bank in 0..prg.len() / BANK_SIZE {
        let chunk = &prg[bank * BANK_SIZE..(bank + 1) * BANK_SIZE];
        for i in 0..chunk.len().saturating_sub(2) {
            if chunk[i] != 0xF3 {
                continue;
            }
            let pointer = u16::from_le_bytes([chunk[i + 1], chunk[i + 2]]);
            if !(0xA000..0xC000).contains(&pointer) {
                continue;
            }
            let offset = usize::from(pointer - 0xA000);
            let records = decode_records(chunk, offset);
            if records.len() >= MIN_RECORDS {
                tables.push(Table {
                    bank: u8::try_from(bank).unwrap_or(0),
                    cmd_addr: 0x8000 + u16::try_from(i).expect("in-bank"),
                    pointer,
                    offset,
                    records,
                });
            }
        }
    }
    tables
}

/// Validates that every record's sample range lies inside `prg`.
///
/// # Errors
///
/// Returns an error for the first record whose computed range escapes the ROM.
pub fn validate(prg: &[u8], records: &[Record]) -> Result<()> {
    for (i, record) in records.iter().enumerate() {
        let offset = record.sample_rom_offset();
        let end = offset + usize::from(record.bytes());
        if record.cpu_addr() < SAMPLE_ROM_BASE || end > prg.len() {
            bail!(
                "record {i}: sample ${:04X}+{} escapes the ROM (offset {offset:#X}..{end:#X})",
                record.cpu_addr(),
                record.bytes(),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a 128 KiB PRG image with a `$F3` command and a two-record table
    /// in bank 6 at offset `$006C`.
    fn synthetic_prg() -> Vec<u8> {
        let mut prg = vec![0u8; 16 * BANK_SIZE];
        let base = 6 * BANK_SIZE;
        // $F3 <lo> <hi> at bank offset 0; points at $A06C -> offset $006C.
        prg[base] = 0xF3;
        prg[base + 1] = 0x6C;
        prg[base + 2] = 0xA0;
        // Eight records (the driver's real tables carry 50); the last is made up, so the
        // fixture is not a copy of the ROM's table.
        let records: [[u8; 4]; 8] = [
            [0x07, 0x7F, 0x94, 0x3F],
            [0x07, 0x7F, 0xC4, 0x3E],
            [0x07, 0x7F, 0xA4, 0x3F],
            [0x08, 0x7F, 0xC4, 0x3E],
            [0x08, 0x7F, 0xA4, 0x3F],
            [0x09, 0x7F, 0x94, 0x3F],
            [0x0A, 0x7F, 0xB4, 0x3F],
            [0x0B, 0x7F, 0xA4, 0x3E],
        ];
        for (i, record) in records.iter().enumerate() {
            prg[base + 0x6C + 4 * i..base + 0x70 + 4 * i].copy_from_slice(record);
        }
        prg
    }

    #[test]
    fn finds_and_decodes_a_table() {
        let prg = synthetic_prg();
        let tables = find_tables(&prg);
        assert_eq!(tables.len(), 1);
        let table = &tables[0];
        assert_eq!(table.bank, 6);
        assert_eq!(table.pointer, 0xA06C);
        assert_eq!(table.records.len(), 8);
        assert_eq!(table.records[0].rate_index(), 7);
        assert_eq!(table.records[0].cpu_addr(), 0xE500);
        assert_eq!(table.records[0].bytes(), 1009);
        assert_eq!(table.records[1].cpu_addr(), 0xF100);
    }

    /// A decoded record's sample range must sit inside the ROM.
    #[test]
    fn decoded_range_is_inside_the_rom() {
        let prg = synthetic_prg();
        let tables = find_tables(&prg);
        let records = &tables[0].records;
        validate(&prg, records).expect("range is inside the ROM");
        let first = records[0];
        assert!(first.sample_rom_offset() + usize::from(first.bytes()) <= prg.len());
        assert_eq!(
            first.sample_rom_offset(),
            SAMPLE_BANK as usize * BANK_SIZE + 0x500
        );
    }

    /// Structurally impossible ranges are rejected.
    #[test]
    fn out_of_range_records_are_rejected() {
        let prg = synthetic_prg();
        let bank = &prg[6 * BANK_SIZE..7 * BANK_SIZE];
        let mut bad = decode_records(bank, 0x6C)[..1].to_vec();
        bad[0].addr = 0xFF; // $FFC0 + length escapes the 8 KiB bank
        bad[0].length = 0xFF;
        assert!(validate(&prg, &bad).is_err());
    }
}
