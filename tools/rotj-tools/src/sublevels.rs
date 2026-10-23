//! `rotj sublevel`: a stage directory for any captured sublevel.
//!
//! Where `rotj stage` copies the PPU's nametables, this derives the tile area
//! from the metatile map (`rotj_core::levelmap::Map`) and reads the
//! stage/sublevel/scroll out of RAM, so one command covers every capture.
//!
//! The attribute table is **not** derivable from the map: the metatile map and
//! the four auto-tile tables carry tile ids only. It is copied from the capture
//! and `meta.txt` says so.

use std::fmt::Write as _;
use std::path::Path;

use color_eyre::eyre::{Context, Result, eyre};
use rotj_core::levelmap::Map;
use rotj_core::stage::{NAMETABLE_OFFSET, PALETTE_OFFSET, PPU_BYTES, RAM_BYTES, WRAM_BYTES};
use rotj_core::sublevels;

/// Bytes of an attribute table inside one physical nametable.
const ATTRIBUTE_BYTES: usize = 0x40;
/// Offset of the attribute table inside one physical nametable (`$23C0`).
const ATTRIBUTE_OFFSET: usize = 0x3C0;

/// Reads one dump file, checking its expected length.
fn read_exact(path: &Path, len: usize) -> Result<Vec<u8>> {
    let data = std::fs::read(path).wrap_err_with(|| format!("reading {}", path.display()))?;
    if data.len() != len {
        return Err(eyre!(
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

/// Writes the stage directory for the sublevel captured in `dump` at `frame`.
///
/// The directory is `base/stage-<stage>-<sublevel>/`, named after the RAM
/// values [`sublevels::position`] reads. Files: `nametables.bin` (both physical
/// nametables, tiles from the map plus the captured attribute table),
/// `attributes.bin`, `palette.bin`, `collision.bin` (WRAM, the metatile map is
/// `$6000-$6EFF`) and `meta.txt`.
///
/// # Errors
/// A missing or wrong-length dump file, or an unwritable output path.
pub fn extract(dump: &Path, frame: u32, base: &Path) -> Result<()> {
    let ppu = read_exact(&dump.join(format!("ppu-{frame}.bin")), PPU_BYTES)?;
    let ram = read_exact(&dump.join(format!("ram-{frame}.bin")), RAM_BYTES)?;
    let wram = read_exact(&dump.join(format!("wram-{frame}.bin")), WRAM_BYTES)?;

    let position = sublevels::position(&ram)
        .ok_or_else(|| eyre!("ram-{frame}.bin is too short to hold the position"))?;
    let (images, _) = wram.as_chunks::<0x1000>();
    let [map, tables] = images else {
        return Err(eyre!("wram-{frame}.bin is not two 4 KiB images"));
    };
    let map = Map::from_bytes(map);
    let out = base.join(sublevels::directory(position.id));

    // Tiles come from the map; the attribute table cannot, so copy it from the
    // capture (the only bytes in the nametable the map does not explain).
    let mut nametable = map.render_nametable(tables, position.scroll_x);
    let attrs = &ppu[NAMETABLE_OFFSET + ATTRIBUTE_OFFSET
        ..NAMETABLE_OFFSET + ATTRIBUTE_OFFSET + ATTRIBUTE_BYTES];
    nametable[ATTRIBUTE_OFFSET..ATTRIBUTE_OFFSET + ATTRIBUTE_BYTES].copy_from_slice(attrs);
    // Single-screen mirroring: both physical nametables alias, as captured.
    let nametables: Vec<u8> = nametable.iter().chain(nametable.iter()).copied().collect();

    write(&out.join("nametables.bin"), &nametables)?;
    write(&out.join("attributes.bin"), attrs)?;
    write(
        &out.join("palette.bin"),
        &ppu[PALETTE_OFFSET..PALETTE_OFFSET + 32],
    )?;
    write(&out.join("collision.bin"), &wram)?;

    let mut meta = String::new();
    let _ = writeln!(meta, "stage: {}", position.id.stage);
    let _ = writeln!(meta, "sublevel: {}", position.id.sublevel);
    let _ = writeln!(meta, "frame: {frame}");
    let _ = writeln!(meta, "area: {}", position.area);
    let _ = writeln!(meta, "scroll_x: {}", position.scroll_x);
    meta.push_str(
        "nametables: tile area derived from the metatile map at scroll_x\n\
         map: 15x256 metatiles, WRAM $6000-$6EFF (in collision.bin)\n\
         attributes: copied from the capture ($23C0), not derivable from the map\n",
    );
    write(&out.join("meta.txt"), meta.as_bytes())?;

    println!(
        "\nstage {} sublevel {} (frame {frame}, area {}, scroll {})\n{}",
        position.id.stage,
        position.id.sublevel,
        position.area,
        position.scroll_x,
        out.display()
    );
    Ok(())
}
