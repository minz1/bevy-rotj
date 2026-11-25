//! `rotj actors`: dump a room's spawn data from the ROM.
//!
//! Prints, for one `$0400`/`$0402` pair, the room's screen record (`rotj_core::rooms`)
//! and the bank-3 `$A000` actor spawn records, then the located `$7D90` behaviour table
//! and `$7D50` state dispatch table.

use std::fmt::Write as _;
use std::path::Path;

use color_eyre::eyre::{Context, Result, eyre};
use rotj_core::map::rom as spawn;
use rotj_core::prg;

const BEHAVIOUR_BANK: usize = 0x00;
const BEHAVIOUR_HI: usize = 0x87F5;
const BEHAVIOUR_LO: usize = 0x87F7;
const BEHAVIOUR_ENTRIES: usize = 2;

const STATE_BANK: usize = 0x0D;
const STATE_TABLE: usize = 0xABA8;
const STATE_ENTRIES: usize = 5;

fn behaviour_table(prg_bytes: &[u8]) -> Result<Vec<(u8, u8)>> {
    let mut out = Vec::with_capacity(BEHAVIOUR_ENTRIES);
    for i in 0..BEHAVIOUR_ENTRIES {
        let hi = prg::byte(prg_bytes, BEHAVIOUR_BANK, BEHAVIOUR_HI + i)
            .ok_or_else(|| eyre!("behaviour hi table out of range"))?;
        let lo = prg::byte(prg_bytes, BEHAVIOUR_BANK, BEHAVIOUR_LO + i)
            .ok_or_else(|| eyre!("behaviour lo table out of range"))?;
        out.push((hi, lo));
    }
    Ok(out)
}

fn state_handlers(prg_bytes: &[u8]) -> Result<Vec<u16>> {
    (0..STATE_ENTRIES)
        .map(|i| {
            prg::word(prg_bytes, STATE_BANK, STATE_TABLE + i * 2)
                .ok_or_else(|| eyre!("state table out of range"))
        })
        .collect()
}

/// Decode the spawn data for `area`/`stage` and print it.
pub fn dump(rom_path: &Path, stage: u8, sublevel: u8, area: u8) -> Result<()> {
    let data =
        std::fs::read(rom_path).wrap_err_with(|| format!("reading {}", rom_path.display()))?;
    let rom = rotj_rom::Rom::parse_ines(&data).wrap_err("parsing iNES")?;
    let prg = &rom.prg;

    let mut out = String::new();
    let _ = writeln!(
        out,
        "stage {stage} sublevel {sublevel}, area ${area:02X} ($0400={area:02X} $0402={stage:02X})"
    );

    let _ = writeln!(out, "-- room (screen record: bank 14 $DA18)");
    match rotj_core::map::Room::import(prg, area, stage) {
        Some(layout) => {
            let _ = writeln!(
                out,
                "   raster {:?} arrival {:?}\n   fixed rows {:?}\n   fixed quads {:?}",
                layout.raster, layout.arrival, layout.fixed_rows, layout.fixed_quads
            );
        }
        None => {
            let _ = writeln!(out, "   (no room, or its records are malformed)");
        }
    }

    let _ = writeln!(
        out,
        "-- actor spawns (bank {:02X} ${:04X}, schedule ${:04X} -> $7410)",
        spawn::SPAWN_BANK,
        spawn::SPAWN_TABLE,
        spawn::SPAWN_SCHEDULE
    );
    let mut pending = 0;
    for (cell, &value) in spawn::spawn_schedule(prg, area)?.iter().enumerate() {
        if value & spawn::CELL_PENDING == 0 {
            continue;
        }
        pending += 1;
        let id = value & spawn::CELL_RECORD;
        let addr = spawn::spawn_record_pointer(prg, area, stage, id)?;
        let record = spawn::spawn_record(prg, addr)?;
        let _ = writeln!(
            out,
            "   cell ${cell:02X} id {id:2} -> record ${addr:04X}  {record:?}"
        );
    }
    if pending == 0 {
        let _ = writeln!(out, "   (no cell pending)");
    }

    let _ = writeln!(
        out,
        "-- behaviour table (bank {BEHAVIOUR_BANK:02X} ${BEHAVIOUR_HI:04X}/${BEHAVIOUR_LO:04X}, indexed by $7D90)"
    );
    for (i, (hi, lo)) in behaviour_table(prg)?.iter().enumerate() {
        let value = i16::from_le_bytes([*lo, *hi]);
        let _ = writeln!(out, "   idx {i}: ${hi:02X}{lo:02X} ({value})");
    }

    let _ = writeln!(
        out,
        "-- state dispatch (bank {STATE_BANK:02X} ${STATE_TABLE:04X}, indexed by $7D50)"
    );
    for (i, handler) in state_handlers(prg)?.iter().enumerate() {
        let _ = writeln!(out, "   idx {i}: handler ${handler:04X}");
    }

    out.push_str(
        "note: the $DA18 record's script pointer is plain 6502 (here it only sets\n\
         level state), not a bytecode; a spawned object takes the first free $7CC0\n\
         slot, its state is record byte 6 ($7D50) and $7DD0 its behaviour phase.\n\
         See docs/ACTORS.md §7d for what is decoded, §7e for what is still a lead.\n",
    );
    print!("{out}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn behaviour_and_state_tables_decode() {
        let mut prg = vec![0u8; 16 * rotj_core::prg::BANK_BYTES];
        prg[0x7F5] = 0xFF;
        prg[0x7F6] = 0x00;
        prg[0x7F7] = 0xD8;
        prg[0x7F8] = 0x28;
        let state = STATE_BANK * rotj_core::prg::BANK_BYTES;
        for i in 0..4 {
            prg[state + 0xBA8 + i * 2..][..2].copy_from_slice(&0xAB5Au16.to_le_bytes());
        }
        prg[state + 0xBA8 + 8..][..2].copy_from_slice(&0xAB69u16.to_le_bytes());
        assert_eq!(behaviour_table(&prg).unwrap(), [(0xFF, 0xD8), (0x00, 0x28)]);
        assert_eq!(
            state_handlers(&prg).unwrap(),
            [0xAB5A, 0xAB5A, 0xAB5A, 0xAB5A, 0xAB69]
        );
    }
}
