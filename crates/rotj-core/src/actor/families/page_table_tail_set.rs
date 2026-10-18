//! Type `$9C` (bank 12 `$82F7`): an area-1C level edit. It stamps `$60` over the
//! second half of the room's page table (`$0450-$045F`) and frees its own
//! slot. It calls no `$A000` load, no projection, no physics and no contact; it
//! never draws.

use crate::actor::engine::Work;
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The value stamped into pages 16-31 of the page table (`$0450-$045F`).
const COLUMN: u8 = 0x60;
/// The first page-table entry the handler writes (`$0450`).
const FIRST_PAGE: usize = 16;

/// Type `$9C` keeps no state of its own: it edits the page table and despawns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$82F7`).
    pub fn step(&mut self, kind: &mut u8, _body: &mut Body, _work: &mut Work, cx: &mut Cx<'_>) {
        cx.schedule.page_columns[FIRST_PAGE..].fill(COLUMN);
        *kind = 0;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{ACTOR_LOOP, Coverage, Scene, assert_actor_phases};

    use super::{COLUMN, FIRST_PAGE};

    /// Area 1C:00, slot 3, from the level-start record (`$7D50` `61`). The handler
    /// runs one loop: it stamps the entries and frees its own slot, so it is occupied
    /// once, never drawn and never contacts the player. It dispatches on no
    /// `$7DD0`, so phase 0 is residue.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(33598) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 300, &[0x9C]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 267,
                occupied: 1,
                drawn: 0,
                phases: BTreeMap::from([(0, 1)]),
            }
        );
    }

    /// The one occupied frame is the first gameplay loop frame (33632, after the
    /// area load). It starts with a page table not already at `COLUMN`, so the diff
    /// above pins the handler's fill rather than passing on unchanged bytes.
    #[test]
    fn the_occupied_frame_rewrites_the_page_table() {
        let Some(mut scene) = Scene::tas(33631) else {
            return;
        };
        scene.isolate(&[0x9C]);
        let mut events = scene.run(1, &[ACTOR_LOOP.0, ACTOR_LOOP.1]);
        let begin = events.remove(0);
        assert_eq!(begin.wram[0x1CC3], 0x9C);
        let entries = &begin.ram[0x440 + FIRST_PAGE..0x460];
        assert!(
            entries.iter().any(|&byte| byte != COLUMN),
            "the page table is already {entries:?}"
        );
    }
}
