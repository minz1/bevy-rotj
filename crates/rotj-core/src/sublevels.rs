//! Which stage/sublevel a capture is from.
//!
//! The game keeps the running position in CPU RAM. `docs/STATUS.md` and
//! `docs/SUBLEVELS.md` record the evidence for each address; this module reads
//! them from a `tools/emu/dump.lua` RAM image so the extraction tools can name
//! their output after the sublevel instead of a hardcoded path.
//!
//! | RAM | meaning |
//! | --- | --- |
//! | [`STAGE_ADDR`] | stage index, 0-based |
//! | [`SUBLEVEL_ADDR`] | sublevel index within the stage |
//! | [`AREA_ADDR`] | area/screen index, stepped by 2 per screen |
//! | [`SCROLL_ADDR`] | horizontal scroll in pixels the nametable is keyed to |

use crate::level::SublevelId;

/// RAM `$0402`: stage index, 0-based.
pub const STAGE_ADDR: u16 = 0x0402;
/// RAM `$0401`: sublevel index within the stage.
pub const SUBLEVEL_ADDR: u16 = 0x0401;
/// RAM `$0400`: area/screen index; the game adds 2 per screen.
pub const AREA_ADDR: u16 = 0x0400;
/// RAM `$00AC`: horizontal scroll in pixels, the nametable's window origin
/// (`docs/LEVELDATA.md` §12b).
pub const SCROLL_ADDR: u16 = 0x00AC;

/// The position the game reports for a running sublevel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// Stage/sublevel pair (`$0402`/`$0401`).
    pub id: SublevelId,
    /// Area/screen index (`$0400`).
    pub area: u8,
    /// Horizontal scroll in pixels (`$00AC`).
    pub scroll_x: u16,
}

/// Read a position out of a CPU-RAM image (`$0000-$07FF`).
///
/// `None` when `ram` does not reach [`STAGE_ADDR`].
#[must_use]
pub fn position(ram: &[u8]) -> Option<Position> {
    Some(Position {
        id: SublevelId {
            stage: *ram.get(usize::from(STAGE_ADDR))?,
            sublevel: *ram.get(usize::from(SUBLEVEL_ADDR))?,
        },
        area: *ram.get(usize::from(AREA_ADDR))?,
        scroll_x: u16::from(*ram.get(usize::from(SCROLL_ADDR))?),
    })
}

/// The asset directory name for a captured sublevel: `stage-<stage>-<sublevel>`.
///
/// Both numbers are the raw RAM values, so the name never mixes in a human
/// label the game has not been shown to use ("stage 1-1" is the hand-written name for
/// the same data, stage 0 / sublevel 8).
#[must_use]
pub fn directory(id: SublevelId) -> String {
    format!("stage-{}-{}", id.stage, id.sublevel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ram() -> Vec<u8> {
        vec![0u8; 0x800]
    }

    #[test]
    fn reads_the_captured_stage_1_1_position() {
        // fceux's frame-700 stage 1-1 capture (`tools/emu/stage`):
        // stage 0, sublevel 8, area 0, scroll 0 (`docs/LEVELDATA.md` §0/§12c).
        let mut ram = ram();
        ram[usize::from(SUBLEVEL_ADDR)] = 8;
        let p = position(&ram).unwrap();
        assert_eq!(
            p.id,
            SublevelId {
                stage: 0,
                sublevel: 8
            }
        );
        assert_eq!(p.area, 0);
        assert_eq!(p.scroll_x, 0);
    }

    #[test]
    fn scroll_is_read_from_00ac() {
        let mut ram = ram();
        ram[usize::from(SCROLL_ADDR)] = 54;
        assert_eq!(position(&ram).unwrap().scroll_x, 54);
    }

    #[test]
    fn rejects_an_image_that_does_not_reach_the_stage() {
        assert!(position(&[0u8; 4]).is_none());
    }

    #[test]
    fn directory_names_the_raw_indexes() {
        assert_eq!(
            directory(SublevelId {
                stage: 0,
                sublevel: 8
            }),
            "stage-0-8"
        );
    }
}
