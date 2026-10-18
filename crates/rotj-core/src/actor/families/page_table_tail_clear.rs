//! Type `$C8` (bank 12 `$9A26`): an area-20 level edit. It clears the second half
//! of the room's page table (`$0450-$045F`) and frees its own slot. It
//! calls no `$A000` load, no projection, no physics and no contact; it never draws.

use crate::actor::engine::Work;
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The first page-table entry the handler writes (`$0450`).
const FIRST_PAGE: usize = 16;

/// Type `$C8` keeps no state of its own: it clears the page table and despawns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9A26`).
    pub fn step(&mut self, kind: &mut u8, _body: &mut Body, _work: &mut Work, cx: &mut Cx<'_>) {
        cx.schedule.page_columns[FIRST_PAGE..].fill(0);
        *kind = 0;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{ACTOR_LOOP, Coverage, Scene, assert_actor_phases};

    use super::FIRST_PAGE;

    /// Area 20:00, slot 0, from the level-start record (`$7D50` `61`). The handler
    /// runs one loop: it clears the entries and frees its own slot, so it is occupied
    /// once, never drawn and never contacts the player. It dispatches on no
    /// `$7DD0`, so phase 0 is residue.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(38119) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 300, &[0xC8]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 300,
                occupied: 1,
                drawn: 0,
                phases: BTreeMap::from([(0, 1)]),
            }
        );
    }

    /// The one occupied frame is the gameplay loop frame 38120 (after the area
    /// load). It starts with a page table not already clear, so the diff above pins
    /// the handler's fill rather than passing on unchanged bytes.
    #[test]
    fn the_occupied_frame_clears_the_page_table() {
        let Some(mut scene) = Scene::tas(38119) else {
            return;
        };
        scene.isolate(&[0xC8]);
        let mut events = scene.run(1, &[ACTOR_LOOP.0, ACTOR_LOOP.1]);
        let begin = events.remove(0);
        assert_eq!(begin.wram[0x1CC0], 0xC8);
        let entries = &begin.ram[0x440 + FIRST_PAGE..0x460];
        assert!(
            entries.iter().any(|&byte| byte != 0),
            "the page table is already clear: {entries:?}",
        );
    }
}
