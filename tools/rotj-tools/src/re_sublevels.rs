//! Which stage/sublevel a capture is from (RE dump RAM addresses).

use crate::re_level::SublevelId;

/// RAM `$0402`: stage index, 0-based.
pub const STAGE_ADDR: u16 = 0x0402;
/// RAM `$0401`: sublevel index within the stage.
pub const SUBLEVEL_ADDR: u16 = 0x0401;
/// RAM `$0400`: area/screen index; the game adds 2 per screen.
pub const AREA_ADDR: u16 = 0x0400;
/// RAM `$00AC`: horizontal scroll in pixels.
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
#[must_use]
pub fn directory(id: SublevelId) -> String {
    format!("stage-{}-{}", id.stage, id.sublevel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_position_and_formats_directory() {
        let mut ram = vec![0u8; 0x800];
        ram[usize::from(SUBLEVEL_ADDR)] = 8;
        ram[usize::from(SCROLL_ADDR)] = 54;
        let p = position(&ram).unwrap();
        assert_eq!(
            p,
            Position {
                id: SublevelId {
                    stage: 0,
                    sublevel: 8
                },
                area: 0,
                scroll_x: 54
            }
        );
        assert_eq!(directory(p.id), "stage-0-8");
        assert!(position(&[0u8; 4]).is_none());
    }
}
