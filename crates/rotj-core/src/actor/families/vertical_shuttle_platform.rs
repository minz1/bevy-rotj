//! Second-class type `$04`, the moving platform (bank 8 `$807C`).

use crate::actor::slots::{Body, Field};
use crate::actor::world::Cx;

const STEP: u8 = 0x18;
/// The travel's low byte, which the game keeps in health's low byte.
const TRAVEL_LOW: Field = Field::HealthLow;

/// `$7DD0` when the platform turns to go down: the cleared state.
const REST: u8 = 0;
/// The platform keeps its direction (`state` bits 2-3) and travel (`timer:health`) in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame (`$807C`): move 1.5 px, turn round after the travel in `$7BD8,x`.
    pub fn step(&mut self, index: usize, body: &mut Body, cx: &mut Cx<'_>) {
        body.scratch = 0;
        body.anim = 0;
        body.hit = STEP;
        body.behaviour = 0;
        body.y = if body.state & 0x08 == 0 {
            body.y.wrapping_add(u16::from(STEP))
        } else {
            body.y.wrapping_sub(u16::from(STEP))
        };
        let travel =
            u16::from_be_bytes([body.timer, body.get(TRAVEL_LOW)]).wrapping_add(u16::from(STEP));
        let [timer, distance] = travel.to_be_bytes();
        body.timer = timer;
        body.set(TRAVEL_LOW, distance);
        let limit = cx.slots.platform_parameter(index);
        if timer == limit {
            body.timer = 0;
            body.set(TRAVEL_LOW, 0);
            body.state ^= 0x0C;
            if body.state & 0x08 == 0 {
                body.subtype = 0;
                body.phase = REST;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::actor::oracle::{Scene, assert_second_phases};

    #[test]
    fn matches_the_oracle_in_the_red_brick_room() {
        let Some(scene) = Scene::replay("stage1-inputs.csv", 1060) else {
            return;
        };
        let coverage = assert_second_phases(scene, 300, &[0x04]);
        assert_eq!(
            (coverage.frames, coverage.occupied),
            (286, 286),
            "{coverage:?}"
        );
        assert!(coverage.drawn > 0, "{coverage:?}");
    }
}
