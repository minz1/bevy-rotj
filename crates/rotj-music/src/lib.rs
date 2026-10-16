//! Batman: Return of the Joker — sound-driver port.
//!
//! The game's music driver (PRG bank 10) is reimplemented in safe Rust here.
//! It reads the driver's data tables (bank 10) and the songs (banks 6-7) from the
//! user's ROM's PRG, and emits `$4000-$4017` register writes on a 60 Hz clock.
//!
//! ```no_run
//! use rotj_music::Music;
//! # let prg = vec![0; 0x20000];
//! let mut music = Music::from_prg(&prg);
//! music.request(4); // stage 1-1's song
//! let writes = music.tick();
//! # let _ = writes;
//! ```

mod driver;

pub use driver::{Music, Write};

/// The song id the game requests for area 0 — stage 1-1 (bank 6, header
/// index 3). Equal to [`AREA_SONGS`]`[0]`; kept as a name for the current
/// stage in the port.
pub const STAGE_1_1_SONG: u8 = AREA_SONGS[0];

/// The area → song request table at PRG bank 14 CPU `$D980`, indexed by the
/// game's area index `$0400 / 2`. `$FF` (bit 7 set) means the area requests no
/// music. Source: ROM bank 14 offset `$1980`; the handler is bank 14 `$D973`,
/// which stores the entry to the request byte `$0041`.
///
/// Proved for area 0: the emulator trace requests `$0041=$04` at frame 609 of
/// stage 1-1 (`tools/emu/music-frame802/ram-writes.csv`, PC `$D97F`) with
/// `$0400=$00`; the table read is byte-for-byte the ROM. The other entries are
/// the ROM table, not yet traced (`docs/MUSIC.md`).
pub const AREA_SONGS: [u8; 18] = [
    0x04, 0x04, 0xFF, 0x0B, 0x07, 0x09, 0x09, 0xFF, 0x05, 0x05, 0xFF, 0x0B, 0x07, 0x04, 0x04, 0xFF,
    0x0C, 0xFF,
];

/// The song the game requests for an area (`$0400 / 2`), or `None` when the
/// area's `$D980` entry has bit 7 set (no music).
#[must_use]
pub fn song_for_area(area: u8) -> Option<u8> {
    AREA_SONGS
        .get(usize::from(area))
        .copied()
        .filter(|&song| song < 0x80)
}

/// The jump sound effect (request id `$24`). Played through request slot
/// `$0042`; a finite noise sweep on voice 3, not a song (`docs/MUSIC.md`).
pub const JUMP_SFX: u8 = 0x24;

#[cfg(test)]
mod tests {
    use super::*;

    /// The area table is the ROM's `$D980`, and the port's current stage takes
    /// its song from it. `$FF` entries are "no music", not song 255.
    #[test]
    fn area_songs_match_the_rom() {
        assert_eq!(song_for_area(0), Some(STAGE_1_1_SONG));
        assert_eq!(song_for_area(0), Some(4), "stage 1-1");
        assert_eq!(song_for_area(1), Some(4));
        assert_eq!(song_for_area(2), None, "bit 7 set: no music");
        assert_eq!(song_for_area(3), Some(0x0B));
        assert_eq!(song_for_area(16), Some(0x0C));
        assert_eq!(song_for_area(17), None);
        assert_eq!(song_for_area(200), None, "out of range");
    }

    /// The jump sound is the effect id the trace tied to the jump.
    #[test]
    fn jump_sfx_is_the_traced_id() {
        assert_eq!(JUMP_SFX, 0x24);
    }
}
