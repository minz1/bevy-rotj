//! The PRG image and the behaviour tables imported from it once.

use std::ops::Deref;

use crate::actor::engine::Hitboxes;
use crate::actor::families;
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
            bytes,
        }
    }

    /// A zeroed image, for tests that run without a ROM.
    #[cfg(test)]
    pub(crate) fn empty() -> Prg {
        Prg::new(vec![0; PRG_SIZE])
    }
}

impl Deref for Prg {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.bytes
    }
}

/// The test ROM's `Prg`, imported once; `None` as [`rotj_rom::for_test`] has no ROM.
#[cfg(test)]
pub(crate) fn for_test() -> Option<&'static Prg> {
    static PRG: std::sync::OnceLock<Option<Prg>> = std::sync::OnceLock::new();
    PRG.get_or_init(|| rotj_rom::for_test().map(|rom| Prg::new(rom.prg.clone())))
        .as_ref()
}
