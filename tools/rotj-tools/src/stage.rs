//! Stage-asset extraction from emulator dumps.
//!
//! `tools/emu/dump.lua` captures, for a given frame, CPU RAM, cart WRAM and the
//! whole PPU address space while the game runs. From those files this module
//! writes the assets the engine consumes, plus a provenance report saying which
//! CHR bank each 1 KiB PPU window was mapped to — the runtime bank schedule.

use std::fmt::Write as _;
use std::path::Path;

use color_eyre::eyre::{Context, Result};
use rotj_core::render::SCREEN_W;
use rotj_core::stage::{PATTERN_BYTES, PPU_BYTES, RAM_BYTES, Stage, WINDOW_BYTES, WRAM_BYTES};

/// Reads one dump file, checking its expected length.
fn read_exact(path: &Path, len: usize) -> Result<Vec<u8>> {
    let data = std::fs::read(path).wrap_err_with(|| format!("reading {}", path.display()))?;
    if data.len() != len {
        return Err(color_eyre::eyre::eyre!(
            "{} is {} bytes, expected {len}",
            path.display(),
            data.len()
        ));
    }
    Ok(data)
}

/// Writes a file, creating parent directories.
fn write(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, data).wrap_err_with(|| format!("writing {}", path.display()))?;
    println!("{} ({} bytes)", path.display(), data.len());
    Ok(())
}

/// For each 1 KiB PPU pattern window, the 1 KiB slots of the extracted pattern
/// space whose bytes it equals (empty when nothing matches, e.g. PRG-RAM CHR).
fn window_provenance(ppu: &[u8], pattern_space: &[u8]) -> Vec<Vec<usize>> {
    (0..PATTERN_BYTES / WINDOW_BYTES)
        .map(|w| {
            let window = &ppu[w * WINDOW_BYTES..(w + 1) * WINDOW_BYTES];
            pattern_space
                .as_chunks::<WINDOW_BYTES>()
                .0
                .iter()
                .enumerate()
                .filter(|(_, slot)| *slot == window)
                .map(|(i, _)| i)
                .collect()
        })
        .collect()
}

/// Extracts a stage's assets from a dump directory.
pub fn extract(dump: &Path, frame: u32, out: &Path, pattern_space: &Path) -> Result<()> {
    let ppu_path = dump.join(format!("ppu-{frame}.bin"));
    let wram_path = dump.join(format!("wram-{frame}.bin"));
    let ram_path = dump.join(format!("ram-{frame}.bin"));
    let ppu = read_exact(&ppu_path, PPU_BYTES)?;
    let wram = read_exact(&wram_path, WRAM_BYTES)?;
    let ram = read_exact(&ram_path, RAM_BYTES)?;
    let pattern_space = std::fs::read(pattern_space)
        .wrap_err_with(|| format!("reading {}", pattern_space.display()))?;

    let stage = Stage::from_dump(&ppu, &wram)?;
    write(&out.join("palette.bin"), &stage.palette)?;
    write(&out.join("nametables.bin"), stage.nametables.as_slice())?;
    write(&out.join("collision.bin"), &stage.collision)?;
    write(&out.join("pattern.bin"), &stage.pattern)?;

    let provenance = window_provenance(&ppu, &pattern_space);
    let mut report = String::from(
        "# PPU pattern windows for this frame: which 1 KiB slot of\n\
         # assets/graphics/chr/pattern-space.bin each $0000-$1FFF window equals.\n\
         # slot = bank * 8 + offset/1024 (bank = 8 KiB CHR bank number).\n",
    );
    for (w, slots) in provenance.iter().enumerate() {
        let matches = if slots.is_empty() {
            "no match (PRG-RAM CHR or mid-frame state)".to_string()
        } else {
            slots
                .iter()
                .map(|s| format!("bank {:02X} + {} KiB", s / 8, s % 8))
                .collect::<Vec<_>>()
                .join(", ")
        };
        writeln!(report, "${:04X}: {matches}", w * WINDOW_BYTES)
            .expect("String writes cannot fail");
    }
    write(&out.join("windows.txt"), report.as_bytes())?;
    let sublevel = ram[0x0401];
    let stage_index = ram[0x0402];
    let meta = format!(
        "frame: {frame}\nstage: {stage_index}\nsublevel: {sublevel}\n\
         nametable bytes: {}\nscreen width: {SCREEN_W}\n\
         nametable0 distinct tiles: {}\nnametable1 distinct tiles: {}\n\
         collision nonzero bytes: {}\n",
        stage.nametables.len(),
        stage.nametable_tile_count(0),
        stage.nametable_tile_count(1),
        stage.collision.iter().filter(|b| **b != 0).count()
    );
    write(&out.join("meta.txt"), meta.as_bytes())?;

    // The report also goes to stdout: it is the interesting part of a run.
    println!(
        "\nstage {stage_index} sublevel {sublevel}, frame {frame}\n{report}",
        report = report.lines().skip(3).collect::<Vec<_>>().join("\n")
    );
    Ok(())
}
