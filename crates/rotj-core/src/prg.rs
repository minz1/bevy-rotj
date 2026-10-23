//! The PRG image and the behaviour tables imported from it once.

use std::ops::Deref;

use crate::actor::engine::{self, Hitboxes};
use crate::actor::families;
use crate::actor::player;
use crate::actor::stage::Stage;
use crate::actor::weapons;
use crate::ines::FME7_PRG_BANK_SIZE;

/// The PRG image's size: 16 banks of 8 KiB.
pub const PRG_SIZE: usize = 16 * FME7_PRG_BANK_SIZE;

/// The PRG image with its behaviour tables; reads as the image's bytes.
pub struct Prg {
    bytes: Vec<u8>,
    /// The actors' hit boxes.
    pub hitboxes: Hitboxes,
    /// The handler families' own tables.
    pub families: families::Tables,
    /// The player's own tables.
    pub player: player::Tables,
    /// The weapons' own tables.
    pub weapons: weapons::Tables,
    /// The HUD's digit glyph tiles (bank 14 `$D26A`).
    pub digit_tiles: [u8; 10],
    /// Each stage's descriptor, by `$0400 / 2`.
    pub stages: Vec<Stage>,
}

impl Prg {
    /// Imports the tables from `bytes`.
    ///
    /// # Panics
    /// On an image too short to hold a table.
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Prg {
        Prg {
            hitboxes: Hitboxes::import(&bytes),
            families: families::Tables::import(&bytes),
            player: player::Tables::import(&bytes),
            weapons: weapons::Tables::import(&bytes),
            digit_tiles: engine::rom_bytes(&bytes, 14, 0xD26A),
            stages: Stage::all_from_rom(&bytes),
            bytes,
        }
    }
}

impl Deref for Prg {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.bytes
    }
}

/// One PRG ROM byte, addressed inside an 8 KiB window mapped from `bank`.
pub(crate) fn byte(prg: &[u8], bank: usize, addr: usize) -> Option<u8> {
    prg.get(bank.checked_mul(0x2000)? + (addr & 0x1FFF))
        .copied()
}

/// A little-endian 16-bit word, addressed as in [`byte`].
pub(crate) fn word(prg: &[u8], bank: usize, addr: usize) -> Option<u16> {
    Some(u16::from_le_bytes([
        byte(prg, bank, addr)?,
        byte(prg, bank, addr + 1)?,
    ]))
}

/// The test ROM's `Prg`, imported once; `None` as [`rotj_rom::for_test`] has no ROM.
#[cfg(test)]
pub(crate) fn for_test() -> Option<&'static Prg> {
    static PRG: std::sync::OnceLock<Option<Prg>> = std::sync::OnceLock::new();
    PRG.get_or_init(|| rotj_rom::for_test().map(|rom| Prg::new(rom.prg.clone())))
        .as_ref()
}
