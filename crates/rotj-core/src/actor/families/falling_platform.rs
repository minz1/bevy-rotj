//! Second-class type `$10`, a platform that falls away once stood on (bank 12 `$8952`).

use crate::actor::families::crumbling_block;
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// It keeps its fall speed in `hit` (1/16 px) and `behaviour` (px).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame (`$8952`): still until stood on, then falls, accelerating, and is
    /// freed past row 16.
    pub fn step(&mut self, index: usize, kind: &mut u8, body: &mut Body, cx: &Cx<'_>) {
        body.scratch = 0;
        body.anim = 0;
        if cx.slots.pressed(index) & 0x80 == 0 {
            (body.hit, body.behaviour, body.subtype, body.phase) = (0, 0, 0, 0);
            return;
        }
        crumbling_block::accelerate_fall(body);
        if body.y >= 0x1000 {
            *kind = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::actor::oracle::{Scene, assert_second_phases};

    #[test]
    fn matches_the_oracle_in_stage_7_1() {
        let Some(scene) = Scene::tas(39270) else {
            return;
        };
        let coverage = assert_second_phases(scene, 80, &[0x10]);
        assert!(coverage.occupied > 40, "{coverage:?}");
        assert!(coverage.drawn > 0, "{coverage:?}");
    }
}
