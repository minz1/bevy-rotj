//! Write a sublevel area's metatile map straight from the ROM.
//!
//! Unlike `rotj level` / `rotj sublevel` (which read an emulator capture), this
//! reads the *CHR-resident compressed map* the game's loader at bank 02 `$89FE`
//! decompresses, via [`rotj_core::levelmap::map_from_rom`]. It needs only the
//! ROM, so it is the path to stages the port cannot capture.

use std::fmt::Write as _;
use std::path::Path;

use color_eyre::eyre::{Context, Result, eyre};
use rotj_core::levelmap::{
    MAP_COLUMNS, ROM_MAP_ROWS, area_bank, area_for_level, area_ppu, map_from_rom,
};

/// Decode a level's metatile map and write `map.bin`/`map.txt` into `out`.
///
/// `level` is the game's level index (`$20`, the value `sub_D3D8` looks up in
/// bank 14's level -> area table); when `None`, `area` (`$0400 / 2`) is used
/// directly. Both select the same 18-entry bank-2 CHR-set tables.
///
/// # Errors
/// Fails if the ROM cannot be read/parsed, the index is outside the tables, or
/// the output cannot be written.
pub fn write(rom: &Path, area: usize, level: Option<usize>, out: &Path) -> Result<()> {
    let data = std::fs::read(rom).wrap_err_with(|| format!("reading {}", rom.display()))?;
    let rom = rotj_rom::Rom::from_ines(&data).map_err(|e| eyre!("{e}"))?;
    let area = match level {
        Some(l) => area_for_level(l)
            .ok_or_else(|| eyre!("level {l} is outside bank 14's level table (0-12)"))?,
        None => area,
    };
    let map = map_from_rom(&rom.prg, &rom.chr, area)
        .ok_or_else(|| eyre!("area {area} is outside the ROM's per-area level tables (0-17)"))?;

    std::fs::create_dir_all(out).wrap_err_with(|| format!("creating {}", out.display()))?;

    let mut raw = Vec::with_capacity(ROM_MAP_ROWS * MAP_COLUMNS);
    for row in &map {
        raw.extend_from_slice(row);
    }
    std::fs::write(out.join("map.bin"), &raw).wrap_err("writing map.bin")?;

    let nonzero = raw.iter().filter(|&&b| b != 0).count();
    let mut text = String::new();
    let bank = area_bank(&rom.prg, area).expect("area in the table");
    writeln!(
        text,
        "area {area} — metatile map from the ROM ({}x{} = {} bytes)",
        ROM_MAP_ROWS,
        MAP_COLUMNS,
        raw.len()
    )?;
    writeln!(
        text,
        "CHR set: banks ${bank:02X}..${:02X} at PPU $0000-$1FFF, stream at PPU ${:04X} \
         (bank 2 $8B63/$8B75[$0400/2])",
        bank + 7,
        area_ppu(&rom.prg, area).expect("area in the table")
    )?;
    writeln!(
        text,
        "columns 0-159 are the level (10 x 16-column strips); 160-255 are the \
         loader's unwritten window (payload code), reproduced for fidelity."
    )?;
    writeln!(text, "nonzero metatiles: {nonzero}")?;
    for (r, row) in map.iter().enumerate() {
        write!(text, "{r:2}:")?;
        for &b in row {
            write!(text, " {b:02X}")?;
        }
        text.push('\n');
    }
    std::fs::write(out.join("map.txt"), text).wrap_err("writing map.txt")?;

    println!(
        "area {area}: wrote map.bin ({} bytes) and map.txt to {}",
        raw.len(),
        out.display()
    );
    Ok(())
}
