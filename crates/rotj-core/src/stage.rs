//! Stage assets as captured from the running game.
//!
//! The emulator harness (`tools/emu/dump.lua`) dumps whole address spaces, so a
//! stage can be extracted without decoding the game's own level format: the
//! nametables, palette RAM and pattern view it left behind *are* the level.

use thiserror::Error;

/// One PPU pattern window (the FME-7 CHR banking granularity).
pub const WINDOW_BYTES: usize = 1024;

/// PPU address space as dumped (`$0000-$3FFF`).
pub const PPU_BYTES: usize = 0x4000;
/// CPU RAM as dumped (`$0000-$07FF`).
pub const RAM_BYTES: usize = 0x0800;
/// Cartridge WRAM as dumped (`$6000-$7FFF`).
pub const WRAM_BYTES: usize = 0x2000;
/// Offset of palette RAM inside the PPU dump (`$3F00`).
pub const PALETTE_OFFSET: usize = 0x3F00;
/// Offset of the nametables inside the PPU dump (`$2000`).
pub const NAMETABLE_OFFSET: usize = 0x2000;
/// Size of the pattern-table region (`$0000-$1FFF`).
pub const PATTERN_BYTES: usize = 0x2000;
/// Both physical nametables together.
pub const NAMETABLES_BYTES: usize = 0x0800;

/// A dump slice was the wrong size.
#[derive(Debug, Error)]
pub enum StageError {
    /// The PPU dump was not [`PPU_BYTES`] long.
    #[error("PPU dump is {actual} bytes, expected {expected}")]
    PpuLength {
        /// Length found.
        actual: usize,
        /// Length required.
        expected: usize,
    },
    /// The WRAM dump was not [`WRAM_BYTES`] long.
    #[error("WRAM dump is {actual} bytes, expected {expected}")]
    WramLength {
        /// Length found.
        actual: usize,
        /// Length required.
        expected: usize,
    },
}

/// One stage's captured state.
#[derive(Debug, Clone)]
pub struct Stage {
    /// Pattern tables (`$0000-$1FFF`) as mapped by the cart's CHR banks.
    pub pattern: Vec<u8>,
    /// Both physical nametables (`$2000-$27FF`).
    pub nametables: Vec<u8>,
    /// The 32 palette-RAM bytes.
    pub palette: [u8; 32],
    /// The collision map the level loader wrote to WRAM.
    pub collision: Vec<u8>,
}

impl Stage {
    /// Splits emulator dumps into the stage's parts.
    pub fn from_dump(ppu: &[u8], wram: &[u8]) -> Result<Self, StageError> {
        if ppu.len() != PPU_BYTES {
            return Err(StageError::PpuLength {
                actual: ppu.len(),
                expected: PPU_BYTES,
            });
        }
        if wram.len() != WRAM_BYTES {
            return Err(StageError::WramLength {
                actual: wram.len(),
                expected: WRAM_BYTES,
            });
        }
        let mut palette = [0u8; 32];
        palette.copy_from_slice(&ppu[PALETTE_OFFSET..PALETTE_OFFSET + 32]);
        Ok(Self {
            pattern: ppu[..PATTERN_BYTES].to_vec(),
            nametables: ppu[NAMETABLE_OFFSET..NAMETABLE_OFFSET + NAMETABLES_BYTES].to_vec(),
            palette,
            collision: wram.to_vec(),
        })
    }

    /// Number of distinct tile indices a physical nametable uses.
    #[must_use]
    pub fn nametable_tile_count(&self, table: usize) -> usize {
        let start = table * WINDOW_BYTES;
        let mut seen = [false; 256];
        for tile in &self.nametables[start..start + 960] {
            seen[usize::from(*tile)] = true;
        }
        seen.iter().filter(|s| **s).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_dumps_into_parts() {
        let mut ppu = vec![0u8; PPU_BYTES];
        ppu[PALETTE_OFFSET] = 0x0f;
        ppu[PALETTE_OFFSET + 31] = 0x30;
        ppu[NAMETABLE_OFFSET] = 7;
        ppu[NAMETABLE_OFFSET + NAMETABLES_BYTES] = 9; // past the nametables
        let mut wram = vec![0u8; WRAM_BYTES];
        wram[0] = 0xff;
        let stage = Stage::from_dump(&ppu, &wram).unwrap();
        assert_eq!(stage.palette[0], 0x0f);
        assert_eq!(stage.palette[31], 0x30);
        assert_eq!(stage.nametables[0], 7);
        assert_eq!(stage.nametables.len(), NAMETABLES_BYTES);
        assert_eq!(stage.collision[0], 0xff);
        // Tile 0 fills the rest of the nametable, so tile 7 plus tile 0.
        assert_eq!(stage.nametable_tile_count(0), 2);
    }

    #[test]
    fn wrong_lengths_are_rejected() {
        assert!(matches!(
            Stage::from_dump(&[0u8; 4], &[0u8; WRAM_BYTES]),
            Err(StageError::PpuLength { .. })
        ));
        assert!(matches!(
            Stage::from_dump(&[0u8; PPU_BYTES], &[0u8; 4]),
            Err(StageError::WramLength { .. })
        ));
    }
}
