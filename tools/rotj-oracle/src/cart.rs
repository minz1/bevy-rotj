//! The cartridge: the user's ROM through `rotj-rom`.

pub use rotj_rom::Rom as Cart;

/// The game ROM for a test or tool (`ROTJ_ROM`, else `raw/`), or `None` after the skip line;
/// panics on a wrong ROM, or on none when `ROTJ_REQUIRE_ROM` is set.
#[must_use]
pub fn load_rom() -> Option<Cart> {
    rotj_rom::for_test().cloned()
}
