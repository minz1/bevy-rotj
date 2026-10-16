//! Dev-only verification harness.
//!
//! Renders emulator-dump frames (`tools/emu/dump.lua`) through the port's own
//! renderer, and emits the palette-index plane alongside the image so
//! `tools/emu/compare.py` can diff the port's frame against the emulator's own
//! screen output index-for-index (RGB tables differ between emulator and port,
//! so only colour indices are comparable).
//!
//! This is how we prove `rotj_core::render` reproduces the PPU's picture —
//! tile selection, attribute palettes, scrolling, sprite composition — instead
//! of assuming it. The crate never ships: the game has no dependency on it.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use color_eyre::eyre::{Context, Result, bail};
use rotj_core::render::{
    Background, Frame, Mirroring, NAMETABLE_BYTES, PALETTE_BYTES, SCREEN_H, SCREEN_W, Sprites,
};

/// Scroll overrides for a render, so a scroll can be swept when hunting a
/// match against an emulator frame.
#[derive(Debug, Clone, Copy, Default)]
struct Scroll {
    /// Horizontal scroll, or the value in RAM `$00AC`.
    x: Option<u16>,
    /// Vertical scroll, or the value in RAM `$00AD`.
    y: Option<u16>,
}

/// PPU dump length from the emulator ($0000-$3FFF).
const PPU_DUMP_BYTES: usize = 0x4000;
/// CPU RAM dump length ($0000-$07FF).
const RAM_DUMP_BYTES: usize = 0x0800;

#[derive(Parser)]
#[command(
    name = "rotj-verify",
    about = "Check the port's renderer against emulator output"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Render one dump frame through the port's renderer.
    Screen {
        /// `ppu-<frame>.bin` ($0000-$3FFF).
        ppu: PathBuf,
        /// `ram-<frame>.bin`, for OAM and the control/scroll shadows.
        ram: PathBuf,
        /// Nametable arrangement: `vertical` or `horizontal`.
        #[arg(long, default_value = "vertical")]
        mirroring: String,
        /// PNG to write.
        #[arg(short, long, default_value = "assets/preview/frame.png")]
        out: PathBuf,
        /// Raw index plane to write (256x240 bytes, one NES colour index per
        /// pixel, row-major) for `tools/emu/compare.py`.
        #[arg(long, default_value = "assets/preview/frame.indices")]
        indices: PathBuf,
        /// Override the horizontal scroll instead of reading RAM `$00AC`.
        #[arg(long)]
        scroll_x: Option<u16>,
        /// Override the vertical scroll instead of reading RAM `$00AD`.
        #[arg(long)]
        scroll_y: Option<u16>,
        /// Take the pattern tables (CHR banks) from this other PPU dump, to
        /// test whether the dumped banks belong to a different frame than the
        /// screen being compared.
        #[arg(long)]
        pattern_from: Option<PathBuf>,
        /// Take OAM from this other RAM dump (the same question for sprites:
        /// sprite DMA runs in vblank, so a dump's OAM describes the next frame).
        #[arg(long)]
        ram_from: Option<PathBuf>,
        /// Render the background only, to tell a background difference from a
        /// sprite difference.
        #[arg(long)]
        no_sprites: bool,
        /// Take palette RAM from this other PPU dump (palettes are also written
        /// during vblank, so a dump's palette can describe the next frame).
        #[arg(long)]
        palette_from: Option<PathBuf>,
        /// Take the nametables from this other PPU dump (streaming writes them
        /// during vblank as well).
        #[arg(long)]
        nametables_from: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    color_eyre::install()?;
    match Cli::parse().cmd {
        Cmd::Screen {
            ppu,
            ram,
            mirroring,
            out,
            indices,
            scroll_x,
            scroll_y,
            pattern_from,
            ram_from,
            no_sprites,
            palette_from,
            nametables_from,
        } => screen(&Job {
            ppu,
            ram,
            mirroring,
            out,
            indices,
            scroll: Scroll {
                x: scroll_x,
                y: scroll_y,
            },
            pattern_from,
            ram_from,
            palette_from,
            nametables_from,
            no_sprites,
        }),
    }
}

/// Everything one render needs: which dumps to read, and the overrides that
/// exist because a dump's vblank-updated components (pattern banks, OAM,
/// palettes) describe the *next* frame.
#[derive(Debug, Clone)]
struct Job {
    /// `ppu-<frame>.bin` ($0000-$3FFF).
    ppu: PathBuf,
    /// `ram-<frame>.bin`, for OAM and the `$AB-$AD` shadows.
    ram: PathBuf,
    /// Nametable arrangement: `vertical` or `horizontal`.
    mirroring: String,
    /// PNG to write.
    out: PathBuf,
    /// Raw index plane to write, for `tools/emu/compare.py`.
    indices: PathBuf,
    /// Scroll overrides.
    scroll: Scroll,
    /// Pattern tables from another dump.
    pattern_from: Option<PathBuf>,
    /// OAM from another dump.
    ram_from: Option<PathBuf>,
    /// Palette RAM from another dump.
    palette_from: Option<PathBuf>,
    /// Nametables from another dump (they are written during vblank too).
    nametables_from: Option<PathBuf>,
    /// Render the background only.
    no_sprites: bool,
}

/// The pieces a render needs, after resolving the source overrides.
struct Sources {
    nametables: Vec<u8>,
    pattern: Vec<u8>,
    palette: [u8; PALETTE_BYTES],
    oam: [u8; 256],
    ctrl: u8,
    scroll_x: u16,
    scroll_y: u16,
}

/// Reads a file or explains which one failed.
fn read(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).wrap_err_with(|| format!("reading {}", path.display()))
}

/// Resolves a job's sources, applying the per-component frame overrides.
fn sources(job: &Job) -> Result<Sources> {
    let ppu = read(&job.ppu)?;
    let ram = read(&job.ram)?;
    if ppu.len() < PPU_DUMP_BYTES {
        bail!("{} is too short to be a PPU dump", job.ppu.display());
    }
    if ram.len() < RAM_DUMP_BYTES {
        bail!("{} is too short to be a RAM dump", job.ram.display());
    }

    // $00AC is the horizontal pixel scroll and $00AD the vertical one (proven
    // from a walk capture: $00AC ramps 0->54 over frames 709-735 while $00AD
    // stays 0). Overrides let a scroll be swept when hunting a match.
    let scroll_x = job.scroll.x.unwrap_or(u16::from(ram[0x00AC]));
    let scroll_y = job.scroll.y.unwrap_or(u16::from(ram[0x00AD]));

    let mut pattern = ppu[..rotj_core::stage::PATTERN_BYTES].to_vec();
    if let Some(path) = &job.pattern_from {
        let other = read(path)?;
        pattern.copy_from_slice(&other[..rotj_core::stage::PATTERN_BYTES]);
    }

    let palette_source = match &job.palette_from {
        Some(path) => read(path)?,
        None => ppu.clone(),
    };
    let palette: [u8; PALETTE_BYTES] = palette_source[0x3F00..0x3F00 + PALETTE_BYTES]
        .try_into()
        .expect("palette slice is exactly PALETTE_BYTES");

    let oam_source = match &job.ram_from {
        Some(path) => read(path)?,
        None => ram.clone(),
    };
    let oam: [u8; 256] = oam_source[0x0200..0x0300]
        .try_into()
        .expect("OAM page is exactly 256 bytes");

    let nametable_source = match &job.nametables_from {
        Some(path) => read(path)?,
        None => ppu.clone(),
    };
    Ok(Sources {
        nametables: nametable_source[0x2000..0x2000 + NAMETABLE_BYTES * 2].to_vec(),
        pattern,
        palette,
        oam,
        ctrl: ram[0x00AB],
        scroll_x,
        scroll_y,
    })
}

/// Renders a job: PPU memory supplies pattern tables, nametables and palette
/// RAM; RAM supplies OAM and the `$AB-$AD` shadows for PPUCTRL and scroll.
fn screen(job: &Job) -> Result<()> {
    let sources = sources(job)?;
    let mirroring = match job.mirroring.as_str() {
        "vertical" | "v" => Mirroring::Vertical,
        "horizontal" | "h" => Mirroring::Horizontal,
        other => bail!("unknown mirroring `{other}`"),
    };

    let mut frame = Frame::new();
    frame.background(
        0..SCREEN_H,
        &Background {
            nametables: &sources.nametables,
            pattern: &sources.pattern,
            pattern_base: if sources.ctrl & 0x10 != 0 { 0x1000 } else { 0 },
            palette: &sources.palette,
            mirroring,
            scroll_x: sources.scroll_x,
            scroll_y: sources.scroll_y,
        },
    );
    let sprite_rows = if job.no_sprites { 0..0 } else { 0..SCREEN_H };
    frame.sprites(
        sprite_rows,
        &Sprites {
            oam: &sources.oam,
            pattern: &sources.pattern,
            pattern_base: if sources.ctrl & 0x08 != 0 { 0x1000 } else { 0 },
            tall: sources.ctrl & 0x20 != 0,
            palette: &sources.palette,
        },
    );

    for path in [&job.out, &job.indices] {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
    }
    image::RgbaImage::from_raw(
        u32::try_from(SCREEN_W).expect("const tiny"),
        u32::try_from(SCREEN_H).expect("const tiny"),
        frame.rgba().to_vec(),
    )
    .expect("frame buffer matches dimensions")
    .save(&job.out)
    .wrap_err_with(|| format!("writing {}", job.out.display()))?;
    std::fs::write(&job.indices, frame.indices())
        .wrap_err_with(|| format!("writing {}", job.indices.display()))?;

    println!(
        "{}: ctrl ${:02X} scroll ({}, {}) palette {:02X?}",
        job.out.display(),
        sources.ctrl,
        sources.scroll_x,
        sources.scroll_y,
        sources.palette
    );
    Ok(())
}
