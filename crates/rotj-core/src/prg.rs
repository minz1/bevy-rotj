//! The PRG image and the behaviour tables imported from it once.

use std::ops::Deref;

use crate::actor::engine::Hitboxes;

/// The PRG image with its behaviour tables; reads as the image's bytes.
pub struct Prg {
    bytes: Vec<u8>,
    /// The actors' hit boxes.
    pub hitboxes: Hitboxes,
}

impl Prg {
    /// Imports the tables from `bytes`.
    ///
    /// # Panics
    /// On an image too short to hold a table.
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Prg {
        let hitboxes = Hitboxes::import(&bytes);
        Prg { bytes, hitboxes }
    }

    /// No image and zeroed tables, for tests that run without a ROM.
    #[cfg(test)]
    pub(crate) fn empty() -> Prg {
        Prg {
            bytes: Vec::new(),
            hitboxes: Hitboxes::default(),
        }
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
