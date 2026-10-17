//! Type `$B2`, the stage's room-start marker (bank 12 `$8C4E`): on its first step
//! it clears a row base and frees its own slot.

use crate::actor::engine::Work;
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// Type `$B2` keeps no state of its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8C4E`): clear `$0443` and free the slot. The
    /// 6502 then jumps to the dispatch tail (`$CEDE`) and runs no other code.
    pub fn step(&mut self, kind: &mut u8, _body: &mut Body, _work: &mut Work, cx: &mut Cx<'_>) {
        cx.schedule.rows[3] = 0;
        *kind = 0;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{ACTOR_LOOP, Coverage, Scene, assert_actor_phases};

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::route(640) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 3, &[0xB2]);
        let expected = Coverage {
            frames: 3,
            occupied: 1,
            drawn: 0,
            phases: BTreeMap::from([(0, 1)]),
        };
        assert_eq!(coverage, expected);
    }

    /// The one occupied frame starts with a set row base, so the diff above pins
    /// the handler's clear rather than passing on an already-zero byte.
    #[test]
    fn the_occupied_frame_starts_with_a_set_row_base() {
        let Some(mut scene) = Scene::route(640) else {
            return;
        };
        scene.isolate(&[0xB2]);
        let mut events = scene.run(1, &[ACTOR_LOOP.0, ACTOR_LOOP.1]);
        let begin = events.remove(0);
        assert_eq!(begin.wram[0x1CC0], 0xB2);
        assert_eq!(begin.ram[0x443], 0x60);
    }
}
