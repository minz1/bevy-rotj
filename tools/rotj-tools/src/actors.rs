//! `rotj actors`: dump a level screen's spawn data from the ROM.
//!
//! Decodes, for one `$0400`/`$0402` pair, the bank-14 `$DA18` screen record and
//! the bank-3 `$A000` actor spawn records, and prints the located `$7D90`
//! behaviour table and `$7D50` state dispatch table. See
//! `rotj_core::actors` and `docs/ACTORS.md` for the evidence and the open
//! questions.

use std::fmt::Write as _;
use std::path::Path;

use color_eyre::eyre::{Context, Result};
use rotj_core::actors;

/// Decode the spawn data for `area`/`stage` and print it.
pub fn dump(rom_path: &Path, stage: u8, sublevel: u8, area: u8) -> Result<()> {
    let data =
        std::fs::read(rom_path).wrap_err_with(|| format!("reading {}", rom_path.display()))?;
    let rom = rotj_core::InesRom::from_bytes(&data).wrap_err("parsing iNES")?;
    let prg: Vec<u8> = rom.prg_banks().flatten().copied().collect();

    let mut out = String::new();
    let _ = writeln!(
        out,
        "stage {stage} sublevel {sublevel}, area ${area:02X} ($0400={area:02X} $0402={stage:02X})"
    );

    let _ = writeln!(
        out,
        "-- level screen record (bank {:02X} ${:04X})",
        actors::SCREEN_BANK,
        actors::SCREEN_TABLE
    );
    match actors::screen_record_at(&prg, area, stage)? {
        None => {
            let _ = writeln!(out, "   empty record (${:02X})", actors::SCREEN_EMPTY);
        }
        Some(rec) => {
            let _ = writeln!(
                out,
                "   record ${:04X}: header {}",
                rec.addr,
                hex(&rec.header)
            );
            let _ = writeln!(out, "   $70 {}", hex(&rec.block_a));
            let _ = writeln!(out, "   $60 {}", hex(&rec.block_b));
            let _ = writeln!(
                out,
                "   script ${:04X} ({:?}, plain 6502)",
                rec.script,
                actors::script_kind(rec.script)
            );
        }
    }

    let _ = writeln!(
        out,
        "-- actor spawns (bank {:02X} ${:04X}, schedule ${:04X} -> $7410)",
        actors::SPAWN_BANK,
        actors::SPAWN_TABLE,
        actors::SPAWN_SCHEDULE
    );
    let spawns = actors::pending_spawns(&prg, area, stage)?;
    if spawns.is_empty() {
        let _ = writeln!(out, "   (no slot pending)");
    }
    for pending in &spawns {
        let spawn = pending.spawn;
        let _ = writeln!(
            out,
            "   slot {:2} id {:2} -> record ${:04X}  state ${:02X} behaviour ${:02X} type ${:02X}  {}",
            pending.slot,
            pending.id,
            spawn.addr,
            spawn.state(),
            spawn.behaviour(),
            spawn.kind(),
            hex(&spawn.raw)
        );
    }

    let _ = writeln!(
        out,
        "-- behaviour table (bank {:02X} ${:04X}/${:04X}, indexed by $7D90)",
        actors::BEHAVIOUR_BANK,
        actors::BEHAVIOUR_HI,
        actors::BEHAVIOUR_LO
    );
    for (i, (hi, lo)) in actors::behaviour_table(&prg)?.iter().enumerate() {
        let value = i16::from_le_bytes([*lo, *hi]);
        let _ = writeln!(out, "   idx {i}: ${hi:02X}{lo:02X} ({value})");
    }

    let _ = writeln!(
        out,
        "-- state dispatch (bank {:02X} ${:04X}, indexed by $7D50)",
        actors::STATE_BANK,
        actors::STATE_TABLE
    );
    for (i, handler) in actors::state_handlers(&prg)?.iter().enumerate() {
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

/// Render bytes as space-separated uppercase hex.
fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}
