//! Song-data extraction for `rotj music`.
//!
//! The sound driver is PRG bank 10 (window `$8000`); its *data* tables (note
//! frequency, dispatch, per-instrument records) live in the same bank, and the
//! *songs* live in banks 6 and 7, addressed through the `$A000` window. Rather
//! than transcribe every table by hand, the port loads two raw images:
//!
//! * `driver.bin` — PRG bank 10 (8 KiB, CPU `$8000..$9FFF`).
//! * `song-banks.bin` — PRG banks 6 and 7 (16 KiB, in that order); a song bank
//!   `n` occupies `(n - 6) * 8 KiB`.
//!
//! The driver's own code is reimplemented in `rotj-music`; only its data is
//! extracted here. See `docs/MUSIC.md`.

use color_eyre::eyre::{Context, Result, bail};
use rotj_core::InesRom;
use rotj_core::ines::FME7_PRG_BANK_SIZE;
use std::path::Path;

/// PRG bank holding the sound driver and its tables.
pub const DRIVER_BANK: u8 = 10;
/// First PRG bank holding song data (the `$A000` window bank).
pub const SONG_BANK_BASE: u8 = 6;
/// Number of contiguous song banks to extract.
pub const SONG_BANK_COUNT: u8 = 2;
/// Number of songs in the driver's song table.
pub const SONG_COUNT: usize = 48;

/// Writes `driver.bin`, `song-banks.bin` and a manifest into `out`.
///
/// # Errors
///
/// Errors if `rom` cannot be read/parsed, or the expected banks are missing.
pub fn extract(rom_path: &Path, out: &Path) -> Result<()> {
    let data =
        std::fs::read(rom_path).wrap_err_with(|| format!("reading {}", rom_path.display()))?;
    let rom = InesRom::from_bytes(&data).wrap_err("parsing iNES")?;
    let prg: Vec<u8> = rom.prg_banks().flatten().copied().collect();
    let bank = |n: u8| -> Result<&[u8]> {
        let start = usize::from(n) * FME7_PRG_BANK_SIZE;
        prg.get(start..start + FME7_PRG_BANK_SIZE)
            .ok_or_else(|| color_eyre::eyre::eyre!("PRG bank {n} is outside the ROM"))
    };

    std::fs::create_dir_all(out)?;
    std::fs::write(out.join("driver.bin"), bank(DRIVER_BANK)?).wrap_err("writing driver.bin")?;

    let mut songs = Vec::with_capacity(SONG_BANK_COUNT as usize * FME7_PRG_BANK_SIZE);
    for n in SONG_BANK_BASE..SONG_BANK_BASE + SONG_BANK_COUNT {
        songs.extend_from_slice(bank(n)?);
    }
    std::fs::write(out.join("song-banks.bin"), &songs).wrap_err("writing song-banks.bin")?;

    let driver = bank(DRIVER_BANK)?;
    if driver[0..3] != [0x4C, 0x0C, 0x80] {
        bail!("bank {DRIVER_BANK} does not start with the driver's `JMP $800C` entry");
    }
    // The song table at `$8B91`/`$8B92` (bank-offset `$0B91`): index, bank pairs.
    let manifest = format!(
        "# Batman: Return of the Joker — sound-driver data\n\
         # driver.bin    PRG bank {DRIVER_BANK} (CPU $8000..$9FFF): code tables the port reads\n\
         # song-banks.bin PRG banks {SONG_BANK_BASE}..{} (CPU $A000..$BFFF): song headers + streams\n\
         # song table    CPU $8B91 index / $8B92 bank, {SONG_COUNT} entries\n",
        SONG_BANK_BASE + SONG_BANK_COUNT - 1
    );
    std::fs::write(out.join("manifest.txt"), manifest).wrap_err("writing manifest.txt")?;

    println!(
        "{} ({} bytes), {} ({} bytes)",
        out.join("driver.bin").display(),
        FME7_PRG_BANK_SIZE,
        out.join("song-banks.bin").display(),
        songs.len()
    );
    Ok(())
}
