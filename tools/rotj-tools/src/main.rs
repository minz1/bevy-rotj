//! `rotj` — asset extraction and disassembly tooling CLI.
//!
//! Subcommands land here as the extraction pipeline grows (CHR rendering,
//! pattern dumps, PPU write-lists, level data, music data).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use color_eyre::eyre::{Context, Result, bail, eyre};
use rotj_core::ines::FME7_PRG_BANK_SIZE;
use rotj_core::{GRAY_PREVIEW_PALETTE, InesRom, TILES_PER_BANK, WriteList};

mod actors;

mod stage;

mod dmc;

mod level;

mod levelrom;

mod music;

mod sublevels;

#[derive(Parser)]
#[command(
    name = "rotj",
    about = "Asset extraction tools for Batman: Return of the Joker"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Dump the ROM's parsed iNES header.
    Header {
        /// Path to the .nes ROM file.
        #[arg(short, long)]
        rom: PathBuf,
    },
    /// Render every 8 KiB CHR bank to a PNG tile sheet in the output dir.
    Chr {
        /// Path to the .nes ROM file.
        #[arg(short, long)]
        rom: PathBuf,
        /// Directory to write `bank{NN}.png` files into.
        #[arg(short, long, default_value = "assets/graphics/chr")]
        out: PathBuf,
    },
    /// Dump the ROM's whole PPU pattern space (all CHR banks, raw) for the
    /// engine to sample tiles from.
    Pattern {
        /// Path to the .nes ROM file.
        #[arg(short, long)]
        rom: PathBuf,
        /// File to write, CHR banks concatenated in bank order.
        #[arg(short, long, default_value = "assets/graphics/chr/pattern-space.bin")]
        out: PathBuf,
    },
    /// Dump the ROM's whole PRG (all banks, raw) for the port's game logic.
    Prg {
        /// Path to the .nes ROM file.
        #[arg(short, long)]
        rom: PathBuf,
        /// File to write, PRG banks concatenated in bank order.
        #[arg(short, long, default_value = "assets/prg.bin")]
        out: PathBuf,
    },
    /// Render the debug tile-grid scene through the engine renderer to a PNG,
    /// for eyeballing the video math without launching the game.
    Preview {
        /// Extracted PPU pattern space to sample tiles from.
        #[arg(short, long, default_value = "assets/graphics/chr/pattern-space.bin")]
        pattern: PathBuf,
        /// PNG file to write.
        #[arg(short, long, default_value = "assets/preview/debug-scene.png")]
        out: PathBuf,
    },
    /// Extract a stage's assets (palette, nametables, collision map, live
    /// pattern view) from an emulator dump, with a CHR-bank provenance report.
    Stage {
        /// Directory written by `tools/emu/dump.lua`.
        #[arg(short, long)]
        dump: PathBuf,
        /// Frame number of the dump to read.
        #[arg(short, long)]
        frame: u32,
        /// Directory to write the stage assets into.
        #[arg(short, long, default_value = "assets/levels/stage-1-1")]
        out: PathBuf,
        /// Extracted PPU pattern space, for the bank provenance report.
        #[arg(short, long, default_value = "assets/graphics/chr/pattern-space.bin")]
        pattern: PathBuf,
    },
    /// Decode a sublevel's level data from the ROM (nametable patches +
    /// collision bitmap), and verify it against an emulator capture.
    Level {
        /// Path to the .nes ROM file.
        #[arg(short, long)]
        rom: PathBuf,
        /// Stage index (RAM `$0402`).
        #[arg(long, default_value_t = 0)]
        stage: u8,
        /// Sublevel index (RAM `$0401`).
        #[arg(long, default_value_t = 8)]
        sublevel: u8,
        /// Directory to write `level.bin`/`level.txt` into.
        #[arg(short, long, default_value = "assets/levels/stage-1-1")]
        out: PathBuf,
        /// Emulator frame-dump directory (`tools/emu/stage`) for the acceptance
        /// comparison; also reads `ppu-900.bin` as a second data point.
        #[arg(short, long)]
        dump: Option<PathBuf>,
    },
    /// Dump a level screen's actor spawn data (screen record, actor spawn
    /// records, behaviour table, state dispatch table).
    Actors {
        /// Path to the .nes ROM file.
        #[arg(short, long)]
        rom: PathBuf,
        /// Stage index (RAM `$0402`), 0-based.
        #[arg(long, default_value_t = 0)]
        stage: u8,
        /// Sublevel index (RAM `$0401`), 0-based.
        #[arg(long, default_value_t = 8)]
        sublevel: u8,
        /// Area byte offset into the ROM tables (RAM `$0400`); increments by 2
        /// per screen.
        #[arg(long, default_value_t = 0)]
        area: u8,
    },
    /// Write a sublevel area's metatile map (`$6000-$6FFF`) from the ROM's
    /// CHR-resident compressed map, independent of any emulator capture.
    LevelRom {
        /// Path to the .nes ROM file.
        #[arg(short, long)]
        rom: PathBuf,
        /// Area index into the level's per-area tables (RAM `$0400 / 2`).
        #[arg(long, default_value_t = 0)]
        area: usize,
        /// Level index (`$20`, bank 14 `$D94E`); overrides `--area`. Stage 1-1
        /// is level 0.
        #[arg(long)]
        level: Option<usize>,
        /// Directory to write `map.bin`/`map.txt` into.
        #[arg(short, long, default_value = "assets/levels/stage-1-1")]
        out: PathBuf,
    },
    /// Write `stage-<S>-<L>/` for a captured sublevel: the nametable tiles are
    /// derived from the metatile map (unlike `stage`, which copies the PPU).
    Sublevel {
        /// Directory written by `tools/emu/dump.lua`.
        #[arg(short, long)]
        dump: PathBuf,
        /// Frame number of the dump to read.
        #[arg(short, long)]
        frame: u32,
        /// Base directory to create the `stage-<S>-<L>/` directory in.
        #[arg(short, long, default_value = "assets/levels")]
        out: PathBuf,
    },
    /// Extract the sound driver's data tables and the song banks.
    Music {
        /// Path to the .nes ROM file.
        #[arg(short, long)]
        rom: PathBuf,
        /// Directory to write `driver.bin`/`song-banks.bin` into.
        #[arg(short, long, default_value = "assets/audio/music")]
        out: PathBuf,
    },
    /// Decode a PPU write-list (`[hi,lo,count,bytes...]`, `$FF`-terminated).
    Ppulist {
        /// Path to the .nes ROM file.
        #[arg(short, long)]
        rom: PathBuf,
        /// FME-7 PRG bank holding the list (hex or decimal).
        #[arg(short, long, value_parser = parse_hex_u8)]
        bank: u8,
        /// Offset inside that bank (hex or decimal).
        #[arg(short, long, value_parser = parse_hex_u16)]
        offset: u16,
        /// Also check the lists recorded in `disasm/MAP.md` against the ROM.
        #[arg(long)]
        verify: bool,
    },
    /// Write the original game's levels as a level pack (`pack.json`, `world.world`,
    /// `rotj.tiled-project` and `rooms/`); the pack is ROM-derived, so keep it out of the
    /// repository. Put it in `<data>/packs/<pack>/`, `<data>` the game's folder, for its maps
    /// to find the tilesets the game writes into `<data>/tilesets/`.
    ExportLevels {
        /// Path to the .nes ROM file.
        #[arg(short, long)]
        rom: PathBuf,
        /// The pack's folder.
        #[arg(short, long)]
        out: PathBuf,
    },
    /// Write the tilesets a pack's maps name (a `.tsj` and a `.png` per area and CHR
    /// variant), as the game writes them into `<data>/tilesets/`; they are ROM-derived, so
    /// keep them out of the repository.
    Tilesets {
        /// Path to the .nes ROM file.
        #[arg(short, long)]
        rom: PathBuf,
        /// The tilesets' folder.
        #[arg(short, long)]
        out: PathBuf,
    },
    /// Decode the sound driver's DMC record tables and extract the samples.
    Dmc {
        /// Path to the .nes ROM file.
        #[arg(short, long)]
        rom: PathBuf,
        /// Directory to write the sample files and manifest into.
        #[arg(short, long, default_value = "assets/audio/dmc")]
        out: PathBuf,
    },
}

/// Accepts `0x11B7` or `4535`.
fn parse_hex_u16(text: &str) -> Result<u16, std::num::ParseIntError> {
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u16::from_str_radix(hex, 16),
        None => text.parse(),
    }
}

/// Accepts `0x0C` or `12`.
fn parse_hex_u8(text: &str) -> Result<u8, std::num::ParseIntError> {
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u8::from_str_radix(hex, 16),
        None => text.parse(),
    }
}

impl Cli {
    fn run(self) -> Result<()> {
        match self.cmd {
            Cmd::Header { rom } => header(&rom),
            Cmd::Chr { rom, out } => chr(&rom, &out),
            Cmd::Pattern { rom, out } => pattern(&rom, &out),
            Cmd::Prg { rom, out } => prg(&rom, &out),
            Cmd::Preview { pattern, out } => preview(&pattern, &out),
            Cmd::Ppulist {
                rom,
                bank,
                offset,
                verify,
            } => ppulist(&rom, bank, offset, verify),
            Cmd::Stage {
                dump,
                frame,
                out,
                pattern,
            } => stage::extract(&dump, frame, &out, &pattern),
            Cmd::Level {
                rom,
                stage,
                sublevel,
                out,
                dump,
            } => level::extract(
                &rom,
                rotj_core::level::SublevelId { stage, sublevel },
                &out,
                dump.as_deref(),
            ),
            Cmd::LevelRom {
                rom,
                area,
                level,
                out,
            } => levelrom::write(&rom, area, level, &out),
            Cmd::Actors {
                rom,
                stage,
                sublevel,
                area,
            } => actors::dump(&rom, stage, sublevel, area),
            Cmd::Sublevel { dump, frame, out } => sublevels::extract(&dump, frame, &out),
            Cmd::Dmc { rom, out } => dmc_extract(&rom, &out),
            Cmd::Music { rom, out } => music::extract(&rom, &out),
            Cmd::ExportLevels { rom, out } => export_levels(&rom, &out),
            Cmd::Tilesets { rom, out } => tilesets(&rom, &out),
        }
    }
}

fn export_levels(rom: &Path, out: &Path) -> Result<()> {
    let rom = rotj_rom::Rom::load(Some(rom)).map_err(|error| eyre!(error))?;
    let files = rotj_core::levels::export(&rom.prg, &rom.chr)
        .ok_or_else(|| eyre!("the ROM's levels do not import"))?;
    write_pack(&files, out)?;
    println!("{} ({} files)", out.display(), files.len());
    Ok(())
}

fn tilesets(rom: &Path, out: &Path) -> Result<()> {
    let rom = rotj_rom::Rom::load(Some(rom)).map_err(|error| eyre!(error))?;
    let tilesets = rotj_core::levels::tilesets(&rom.prg, &rom.chr)
        .ok_or_else(|| eyre!("the ROM's levels do not import"))?;
    write_tilesets(&tilesets, out)?;
    println!("{} ({} files)", out.display(), 2 * tilesets.len());
    Ok(())
}

/// Writes each tileset's `.tsj` and `.png` into `out`.
fn write_tilesets(tilesets: &[rotj_core::levels::Tileset], out: &Path) -> Result<()> {
    std::fs::create_dir_all(out)?;
    for tileset in tilesets {
        let tsj = out.join(format!("{}.tsj", tileset.name));
        std::fs::write(&tsj, &tileset.text)
            .wrap_err_with(|| format!("writing {}", tsj.display()))?;
        let side = rotj_core::levels::TILESET_PX;
        let png = out.join(format!("{}.png", tileset.name));
        write_png(&png, side, side, tileset.picture.clone())?;
    }
    Ok(())
}

/// Writes each of a pack's files under `out`.
fn write_pack(files: &rotj_core::levels::PackFiles, out: &Path) -> Result<()> {
    for (path, text) in files {
        let file = out.join(path);
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&file, text).wrap_err_with(|| format!("writing {}", file.display()))?;
    }
    Ok(())
}

/// Read and parse the ROM.
fn load_rom(path: &Path) -> Result<Vec<u8>> {
    let data = std::fs::read(path).wrap_err_with(|| format!("reading {}", path.display()))?;
    InesRom::from_bytes(&data).wrap_err("parsing iNES")?;
    Ok(data)
}

/// Write `rgba` as a PNG of the given size.
fn write_png(path: &Path, width: u32, height: u32, rgba: Vec<u8>) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let img = image::RgbaImage::from_raw(width, height, rgba).expect("buffer matches dimensions");
    img.save(path)
        .wrap_err_with(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn header(path: &Path) -> Result<()> {
    let data = load_rom(path)?;
    let rom = InesRom::from_bytes(&data)?;
    println!(
        "PRG banks: {}\nCHR banks: {}\nMapper:    {}",
        rom.header().prg_banks(),
        rom.header().chr_banks(),
        rom.header().mapper()
    );
    Ok(())
}

fn chr(path: &Path, out: &Path) -> Result<()> {
    let data = load_rom(path)?;
    let rom = InesRom::from_bytes(&data)?;
    let width = u32::try_from(rotj_core::chr::TILE_SHEET_W).expect("const tiny");
    let height = u32::try_from(rotj_core::chr::TILE_SHEET_H).expect("const tiny");
    for (i, bank) in rom.chr_banks().enumerate() {
        let rgba = rotj_core::chr::bank_to_rgba(bank, GRAY_PREVIEW_PALETTE);
        let file = out.join(format!("bank{i:02}.png"));
        write_png(&file, width, height, rgba)?;
        println!("{} ({TILES_PER_BANK} tiles)", file.display());
    }
    Ok(())
}

fn pattern(path: &Path, out: &Path) -> Result<()> {
    let data = load_rom(path)?;
    let rom = InesRom::from_bytes(&data)?;
    let pattern: Vec<u8> = rom.chr_banks().flatten().copied().collect();
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(out, &pattern).wrap_err_with(|| format!("writing {}", out.display()))?;
    println!("{} ({} bytes)", out.display(), pattern.len());
    Ok(())
}

fn prg(path: &Path, out: &Path) -> Result<()> {
    let data = load_rom(path)?;
    let rom = InesRom::from_bytes(&data)?;
    let prg: Vec<u8> = rom.prg_banks().flatten().copied().collect();
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(out, &prg).wrap_err_with(|| format!("writing {}", out.display()))?;
    println!("{} ({} bytes)", out.display(), prg.len());
    Ok(())
}

fn preview(pattern_path: &Path, out: &Path) -> Result<()> {
    let pattern = std::fs::read(pattern_path).wrap_err("reading pattern space")?;
    let mut nametables = vec![0u8; rotj_core::render::NAMETABLE_BYTES * 2];
    for (i, tile) in nametables.iter_mut().take(32 * 30).enumerate() {
        *tile = u8::try_from(i % 256).expect("masked to a byte");
    }
    let mut palette = [0x0f; 32];
    palette[..4].copy_from_slice(&[0x0f, 0x00, 0x10, 0x30]);

    let lines: Vec<rotj_core::render::Line> = (0..rotj_core::render::SCREEN_H)
        .map(|y| rotj_core::render::Line {
            v: u16::try_from(((y / 8) << 5) | ((y % 8) << 12)).expect("below 15 bits"),
            mask: 0x0A,
            chr: [0, 1, 2, 3, 4, 5, 6, 7],
            sprite_chr: [0, 1, 2, 3, 4, 5, 6, 7],
            ..rotj_core::render::Line::default()
        })
        .collect();
    let mut frame = rotj_core::render::Frame::new();
    frame.render(
        &rotj_core::render::Picture {
            lines: &lines,
            vram: &nametables,
            palette: &palette,
            oam: &[0; 256],
            chr: &pattern,
        },
        rotj_core::render::MAX_SPRITES_PER_LINE,
    );
    write_png(
        out,
        u32::try_from(rotj_core::render::SCREEN_W).expect("const tiny"),
        u32::try_from(rotj_core::render::SCREEN_H).expect("const tiny"),
        frame.rgba().to_vec(),
    )?;
    println!("{}", out.display());
    Ok(())
}

fn ppulist(path: &Path, bank: u8, offset: u16, verify: bool) -> Result<()> {
    let data = load_rom(path)?;
    let rom = InesRom::from_bytes(&data)?;
    let prg: Vec<u8> = rom.prg_banks().flatten().copied().collect();

    let base = usize::from(bank) * FME7_PRG_BANK_SIZE + usize::from(offset);
    let slice = prg
        .get(base..)
        .ok_or_else(|| color_eyre::eyre::eyre!("bank ${bank:02X} ${offset:04X} is outside PRG"))?;
    let mut list = WriteList::new(slice);
    let mut entries = 0;
    for entry in list.by_ref() {
        entries += 1;
        let bytes = entry
            .data
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(" ");
        println!(
            "  ${:04X}  {:3} bytes  {bytes}",
            entry.addr,
            entry.data.len()
        );
    }
    match list.end_offset() {
        Some(end) => println!(
            "bank {bank:02X} ${offset:04X}: {entries} entries, terminator at ${:04X}",
            offset + u16::try_from(end).unwrap_or(0) - 1
        ),
        None => println!("bank {bank:02X} ${offset:04X}: {entries} entries, no terminator"),
    }

    if verify {
        println!("-- lists recorded in disasm/MAP.md:");
        let mut all_ok = true;
        for line in rotj_core::ppulist::verify_known_lists(&prg) {
            all_ok &= !line.contains("MISMATCH");
            println!("  {line}");
        }
        if !all_ok {
            bail!("a recorded write-list did not match the ROM");
        }
    }
    Ok(())
}

/// `rotj dmc`: decode the sound driver's DMC record tables and extract samples.
///
/// The tables are found from the driver's `$F3` stream command rather than a
/// hardcoded address, so the extraction follows the ROM if it is patched.
fn dmc_extract(path: &Path, out: &Path) -> Result<()> {
    let data = load_rom(path)?;
    let rom = InesRom::from_bytes(&data)?;
    let prg: Vec<u8> = rom.prg_banks().flatten().copied().collect();

    let tables = dmc::find_tables(&prg);
    if tables.is_empty() {
        bail!("no DMC record table found (no $F3 pointer into the $A000 window)");
    }

    // The ROM keeps byte-identical copies in each song bank; keep one, and
    // remember every place it lives.
    let mut unique: Vec<&dmc::Table> = Vec::new();
    for table in &tables {
        if !unique.iter().any(|u| u.records == table.records) {
            unique.push(table);
        }
    }
    let table = unique[0];
    dmc::validate(&prg, &table.records)?;

    let mut sites: BTreeMap<(u8, u16), String> = BTreeMap::new();
    for table in &tables {
        sites.insert(
            (table.bank, table.cmd_addr),
            format!(
                "bank{:02X} ${:04X} -> ${:04X} (records @ +${:04X})",
                table.bank, table.cmd_addr, table.pointer, table.offset
            ),
        );
    }
    let sites = sites.values().cloned().collect::<Vec<_>>().join("\n#   ");

    std::fs::create_dir_all(out)?;

    // One file per distinct instrument, keyed by (address, length).
    let mut files: BTreeMap<(u16, u16), String> = BTreeMap::new();
    for record in &table.records {
        files
            .entry((record.cpu_addr(), record.bytes()))
            .or_insert_with(|| format!("dmc_{:04x}_{}.bin", record.cpu_addr(), record.bytes()));
    }
    let mut written: BTreeSet<(u16, u16)> = BTreeSet::new();
    for record in &table.records {
        let key = (record.cpu_addr(), record.bytes());
        if !written.insert(key) {
            continue;
        }
        let name = &files[&key];
        let start = record.sample_rom_offset();
        let range = start..start + usize::from(record.bytes());
        std::fs::write(out.join(name), &prg[range])
            .wrap_err_with(|| format!("writing {}", out.join(name).display()))?;
    }

    let bank_start = usize::from(dmc::SAMPLE_BANK) * dmc::BANK_SIZE;
    std::fs::write(
        out.join("sample-bank.bin"),
        &prg[bank_start..bank_start + dmc::BANK_SIZE],
    )
    .wrap_err("writing sample-bank.bin")?;

    let mut manifest = String::from(
        "# Batman: Return of the Joker — DMC sample instruments\n\
         # record bytes: $4010 ctrl, $4011 load, $4012 addr, $4013 length\n",
    );
    let _ = writeln!(manifest, "# table copies: {sites}");
    manifest.push_str("# idx ctrl rate irq loop load addr cpu    bytes file\n");
    for (i, record) in table.records.iter().enumerate() {
        let key = (record.cpu_addr(), record.bytes());
        let _ = writeln!(
            manifest,
            "{i:3}  {ctrl:02X}   {rate:2}   {irq}    {lp}   {load:02X}  {addr:02X}  ${cpu:04X} {bytes:5} {file}",
            ctrl = record.ctrl,
            rate = record.rate_index(),
            irq = u8::from(record.irq()),
            lp = u8::from(record.looping()),
            load = record.load,
            addr = record.addr,
            cpu = record.cpu_addr(),
            bytes = record.bytes(),
            file = files[&key],
        );
    }
    std::fs::write(out.join("manifest.txt"), manifest).wrap_err("writing manifest.txt")?;

    println!(
        "{} records -> {} distinct instruments ({} sample bytes in bank {})",
        table.records.len(),
        files.len(),
        dmc::BANK_SIZE,
        dmc::SAMPLE_BANK
    );
    println!("table copies: {sites}");
    println!("{}", out.join("manifest.txt").display());
    Ok(())
}

fn main() -> Result<()> {
    color_eyre::install()?;
    Cli::parse().run()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pack lands in a temporary folder, its rooms under `rooms/`, and is removed.
    #[test]
    fn export_levels_writes_the_pack_under_its_folder() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let files = rotj_core::levels::export(&rom.prg, &rom.chr).expect("the ROM exports");
        let out = std::env::temp_dir().join(format!("rotj-export-levels-{}", std::process::id()));
        write_pack(&files, &out).expect("writes");
        let rooms = std::fs::read_dir(out.join("rooms"))
            .expect("rooms/")
            .count();
        let pack = std::fs::read_to_string(out.join("pack.json")).expect("pack.json");
        let world = out.join("world.world").exists();
        std::fs::remove_dir_all(&out).expect("removes the pack");
        assert_eq!((rooms, world), (38, true));
        assert!(pack == files["pack.json"], "pack.json differs");
    }

    /// Every tileset's two files land in a temporary folder, which is removed.
    #[test]
    fn tilesets_writes_a_tsj_and_a_png_per_tileset() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let tilesets = rotj_core::levels::tilesets(&rom.prg, &rom.chr).expect("the tilesets");
        let out = std::env::temp_dir().join(format!("rotj-tilesets-{}", std::process::id()));
        write_tilesets(&tilesets, &out).expect("writes");
        let files = std::fs::read_dir(&out).expect("the folder").count();
        let png = image::open(out.join("area-06-b.png")).expect("a PNG");
        std::fs::remove_dir_all(&out).expect("removes the tilesets");
        assert_eq!((files, png.width(), png.height()), (54, 256, 256));
    }
}
