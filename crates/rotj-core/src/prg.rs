//! The PRG image and the behaviour tables imported from it once.

use std::ops::Deref;

use crate::levels::Levels;
use crate::sim::engine::{self, Hitboxes};
use crate::sim::families;
use crate::sim::player;
use crate::sim::weapons;

/// Size of one FME-7 8 KiB PRG bank.
pub const BANK_BYTES: usize = 8 * 1024;

/// The PRG image's size: 16 banks of 8 KiB.
pub const PRG_SIZE: usize = 16 * BANK_BYTES;

/// The PRG image with its behaviour tables; reads as the image's bytes.
pub struct Prg {
    bytes: Vec<u8>,
    /// The actors' hit boxes.
    pub(crate) hitboxes: Hitboxes,
    /// The handler families' own tables.
    pub(crate) families: families::Tables,
    /// The player's own tables.
    pub(crate) player: player::Tables,
    /// The weapons' own tables.
    pub(crate) weapons: weapons::Tables,
    /// The HUD's digit glyph tiles (bank 14 `$D26A`).
    pub(crate) digit_tiles: [u8; 10],
    /// The levels the game plays, each stage by `$0400 >> 1`.
    pub levels: Levels,
}

impl Prg {
    /// Imports the tables from `bytes`, to play `levels`.
    ///
    /// # Panics
    /// On an image too short to hold a table.
    #[must_use]
    pub fn new(bytes: Vec<u8>, levels: Levels) -> Prg {
        Prg {
            hitboxes: Hitboxes::import(&bytes),
            families: families::Tables::import(&bytes),
            player: player::Tables::import(&bytes),
            weapons: weapons::Tables::import(&bytes),
            digit_tiles: engine::rom_bytes(&bytes, 14, 0xD26A),
            levels,
            bytes,
        }
    }

    /// One 8 KiB FME-7 PRG bank slice, or `None` if out of range.
    #[must_use]
    pub fn bank(&self, index: usize) -> Option<&[u8]> {
        bank(&self.bytes, index)
    }

    /// One PRG ROM byte, addressed inside an 8 KiB window mapped from `index`.
    #[must_use]
    pub fn byte(&self, index: usize, addr: usize) -> Option<u8> {
        byte(&self.bytes, index, addr)
    }

    /// A little-endian 16-bit word, addressed as in [`Self::byte`].
    #[must_use]
    pub fn word(&self, index: usize, addr: usize) -> Option<u16> {
        word(&self.bytes, index, addr)
    }
}

impl Deref for Prg {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.bytes
    }
}

/// One 8 KiB FME-7 PRG bank slice, or `None` if `bank` escapes `prg`.
#[must_use]
pub fn bank(prg: &[u8], bank: usize) -> Option<&[u8]> {
    let start = bank.checked_mul(BANK_BYTES)?;
    let end = start.checked_add(BANK_BYTES)?;
    prg.get(start..end)
}

/// The slice of `prg` starting at `offset` inside `bank` (extending to the end of `prg`),
/// or `None` if that start escapes `prg`.
#[must_use]
pub fn from_bank(prg: &[u8], bank: usize, offset: usize) -> Option<&[u8]> {
    let base = bank.checked_mul(BANK_BYTES)?.checked_add(offset)?;
    prg.get(base..)
}

/// One PRG ROM byte, addressed inside an 8 KiB window mapped from `bank`.
#[must_use]
pub fn byte(prg: &[u8], bank: usize, addr: usize) -> Option<u8> {
    prg.get(bank.checked_mul(BANK_BYTES)?.checked_add(addr & 0x1FFF)?)
        .copied()
}

/// A little-endian 16-bit word, addressed as in [`byte`].
#[must_use]
pub fn word(prg: &[u8], bank: usize, addr: usize) -> Option<u16> {
    Some(u16::from_le_bytes([
        byte(prg, bank, addr)?,
        byte(prg, bank, addr.checked_add(1)?)?,
    ]))
}

/// The test ROM's `Prg`, imported once; `None` as [`rotj_rom::for_test`] has no ROM.
#[cfg(test)]
pub(crate) fn for_test() -> Option<&'static Prg> {
    static PRG: std::sync::OnceLock<Option<Prg>> = std::sync::OnceLock::new();
    PRG.get_or_init(|| {
        rotj_rom::for_test().map(|rom| {
            let levels = Levels::original(&rom.prg, &rom.chr).expect("the original levels");
            Prg::new(rom.prg.clone(), levels)
        })
    })
    .as_ref()
}
