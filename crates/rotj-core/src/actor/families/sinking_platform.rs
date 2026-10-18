//! Second-class type `$08`, the platform that sinks under the player (bank 8 `$8ED5`).

use crate::actor::collision::{Probe, probe, solid};
use crate::actor::engine::Work;
use crate::actor::slots::Body;
use crate::actor::world::Cx;

const STEP: u16 = 0x10;

/// The platform keeps its home row and the player's weight where the game does
/// ([`crate::actor::slots::Slots::platform_parameter`], [`crate::actor::slots::Slots::pressed`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame (`$8ED5`): sink 1 px while stood on, unless the level is solid below;
    /// otherwise rise 1 px a frame back to its home row.
    pub fn step(&mut self, index: usize, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        work.x = body.x;
        work.y = body.y;
        work.hitbox = 1;
        let [row, fraction] = body.y.to_be_bytes();
        if body.state == 0 {
            cx.slots.set_platform_parameter(index, row);
            body.state = 1;
        }
        body.hit = 0x18;
        body.behaviour = 0;
        if cx.slots.pressed(index) & 0x80 == 0 {
            if fraction != 0 || cx.slots.platform_parameter(index) != row {
                body.y = body.y.wrapping_sub(STEP);
            }
            return;
        }
        body.scratch = 0;
        body.anim = 0;
        body.y = body.y.wrapping_add(STEP);
        let mut probe = Probe {
            x: body.x,
            y: body.y,
            ..Probe::default()
        };
        if solid(&mut probe, 0x80 | probe::FLOORS_SOLID, cx) {
            body.y = body.y.wrapping_sub(STEP);
        }
        cx.slots.set_pressed(index, 0);
    }
}

#[cfg(test)]
mod tests {
    use crate::actor::oracle::{Scene, assert_second_phases};

    #[test]
    fn matches_the_oracle_on_the_tas() {
        let Some(scene) = Scene::tas(1060) else {
            return;
        };
        let coverage = assert_second_phases(scene, 180, &[0x08]);
        assert!(coverage.occupied > 100, "{coverage:?}");
        assert!(coverage.drawn > 0, "{coverage:?}");
    }
}
