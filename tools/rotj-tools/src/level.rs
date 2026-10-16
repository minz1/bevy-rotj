//! `rotj level`: decode a sublevel's level data from the ROM.
//!
//! Writes the decoded collision bitmap and a summary into the stage asset
//! directory, and (with `--dump`) runs the acceptance comparison against the
//! emulator captures: the ROM's nametable patches vs `nametables.bin`, and the
//! decoded collision bitmap vs `collision.bin`.

use std::fmt::Write as _;
use std::path::Path;

use color_eyre::eyre::{Context, Result};
use rotj_core::level::{self, LevelData, SublevelId};

/// Decode the ROM's level data for one sublevel and (optionally) verify it
/// against an emulator capture directory.
pub fn extract(rom_path: &Path, id: SublevelId, out: &Path, dump: Option<&Path>) -> Result<()> {
    let data =
        std::fs::read(rom_path).wrap_err_with(|| format!("reading {}", rom_path.display()))?;
    let rom = rotj_core::InesRom::from_bytes(&data).wrap_err("parsing iNES")?;
    let prg: Vec<u8> = rom.prg_banks().flatten().copied().collect();

    let level = LevelData::decode(&prg, id)?;

    std::fs::create_dir_all(out)?;
    std::fs::write(out.join("level.bin"), &level.collision)
        .wrap_err_with(|| format!("writing {}", out.join("level.bin").display()))?;

    let mut summary = String::new();
    let _ = writeln!(
        summary,
        "stage {} sublevel {} — decoded level data",
        id.stage, id.sublevel
    );
    let _ = writeln!(
        summary,
        "nametable patches: {} entries, {} bytes",
        level.patches.len(),
        level.patches.iter().map(|p| p.data.len()).sum::<usize>()
    );
    for patch in &level.patches {
        let _ = writeln!(
            summary,
            "  ${:04X}  {:3} bytes",
            patch.addr,
            patch.data.len()
        );
    }
    let nonzero = level.collision.iter().filter(|b| **b != 0).count();
    let _ = writeln!(
        summary,
        "collision bitmap: {} bytes, {} set (${:02X})",
        level.collision.len(),
        nonzero,
        level::COLLISION_SET
    );
    summary.push_str(
        "note: the streamed background (metatile map -> nametable) is not decoded;\n\
         see docs/LEVELDATA.md for what is proven and what remains unknown.\n",
    );
    std::fs::write(out.join("level.txt"), &summary)
        .wrap_err_with(|| format!("writing {}", out.join("level.txt").display()))?;
    print!("{summary}");

    if let Some(dump) = dump {
        verify(&level, out, dump)?;
    }
    println!("{}", out.join("level.bin").display());
    Ok(())
}

/// The acceptance comparison: ROM patches vs `nametables.bin`, decoded
/// collision vs `collision.bin`, plus the frame-700/900 streaming delta.
fn verify(level: &LevelData, out: &Path, dump: &Path) -> Result<()> {
    let nametables = read_asset(&out.join("nametables.bin"))?;
    let collision = read_asset(&out.join("collision.bin"))?;

    // (1) The first screen, as the ROM's own patches render it.
    let image = level.nametable_image();
    let (mismatched, total) = level::diff_bytes(&image, &nametables);
    let (agree, patch_total) = level.patch_agreement(&nametables);
    println!("-- acceptance: first screen vs nametables.bin");
    println!("   patch bytes in range of the capture: {agree}/{patch_total} agree");
    println!(
        "   blank-image diff: {mismatched}/{total} bytes differ \
         (the capture is a streamed metatile render, not a stored screen)"
    );
    if mismatched == 0 && total == nametables.len() {
        println!("   RESULT: MATCH — the decoded screen reproduces the capture");
    } else {
        println!(
            "   RESULT: NO MATCH — the ROM's level lists only patch {} of {} nametable \
             bytes; the rest is generated at run time (see docs/LEVELDATA.md).",
            patch_total,
            nametables.len()
        );
    }

    // (2) The collision bitmap.
    let (cmis, ctot) = level::diff_bytes(&level.collision, &collision);
    println!("-- acceptance: collision bitmap vs collision.bin");
    println!(
        "   {cmis}/{ctot} bytes differ (decoded {} of the captured {} WRAM bytes)",
        level.collision.len(),
        collision.len()
    );

    // (3) Second data point: frame 900, 200 frames later. Differs only where
    //     the game streamed new columns, which shows the background is state,
    //     not static data.
    if let Some(ppu900) = read_optional(&dump.join("ppu-900.bin"))?
        && ppu900.len() >= 0x2800
    {
        let nt900 = &ppu900[0x2000..0x2800];
        let (d, n) = level::diff_bytes(&nametables, nt900);
        println!(
            "-- frame 900 cross-check: {d}/{n} nametable bytes differ from frame 700 \
             (streamed columns; static ROM data cannot explain them)"
        );
    }
    Ok(())
}

/// Read a required asset, with a clear error if the capture is missing.
fn read_asset(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).wrap_err_with(|| format!("reading capture {}", path.display()))
}

/// Read an optional capture file (`None` when absent).
fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).wrap_err_with(|| format!("reading {}", path.display())),
    }
}
