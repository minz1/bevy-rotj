//! Second-class type `$0C`, an unseen floor that follows the camera (bank 8 `$9319`).

use crate::sim::slots::Body;
use crate::sim::world::Cx;

/// Riding it runs `$CE3E` (the `$CE79` table's second entry): the level's floor read
/// screen-relative from map pages 16-31.
const RIDE_ON_SCREEN: u8 = 2;
/// `body.anim` bit 7: the second-class runner skips drawing this floor.
const HIDDEN: u8 = 0x80;

/// No state of its own: every field is rewritten each frame.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame (`$9319`): hidden, 120 px wide (`$CE7E`), at camera y + `$C01`.
    pub fn step(body: &mut Body, cx: &Cx<'_>) {
        body.anim = HIDDEN;
        body.scratch = 1;
        body.y = cx.world.camera_y.wrapping_add(0x0C01);
        body.counter = RIDE_ON_SCREEN;
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::oracle::{Scene, assert_second_phases};

    #[test]
    fn matches_the_oracle_in_stage_4_2() {
        let Some(scene) = Scene::tas(20500) else {
            return;
        };
        let coverage = assert_second_phases(scene, 300, &[0x0C]);
        assert_eq!(coverage.occupied, coverage.frames, "{coverage:?}");
        assert!(coverage.frames > 250, "{coverage:?}");
    }
}
