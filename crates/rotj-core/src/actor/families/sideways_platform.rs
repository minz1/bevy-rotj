//! Second-class type `$02`, the sideways platform (bank 8 `$8000`).

use crate::actor::slots::{Body, Field};
use crate::actor::world::Cx;

const STEP: u16 = 0x10;
/// The travel's low byte, which the game keeps in health's low byte.
const TRAVEL_LOW: Field = Field::HealthLow;

/// The platform keeps its direction (`state` bits 0-1) and travel (`timer:$7BE0`) in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame (`$8000`): move 1 px, turn round after the travel in `$7BD8,x`.
    pub fn step(&mut self, index: usize, body: &mut Body, cx: &mut Cx<'_>) {
        body.scratch = 0;
        let velocity = if body.state & 1 != 0 {
            body.x = body.x.wrapping_add(STEP);
            STEP
        } else {
            body.x = body.x.wrapping_sub(STEP);
            STEP.wrapping_neg()
        };
        [body.phase, body.subtype] = velocity.to_be_bytes();
        let travel = u16::from_be_bytes([body.timer, body.get(TRAVEL_LOW)]).wrapping_add(STEP);
        let [timer, distance] = travel.to_be_bytes();
        body.timer = timer;
        body.set(TRAVEL_LOW, distance);
        if timer == cx.slots.platform_parameter(index) {
            body.timer = 0;
            body.set(TRAVEL_LOW, 0);
            [body.phase, body.subtype] = velocity.wrapping_neg().to_be_bytes();
            body.state ^= 0x03;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::actor::oracle::{Scene, assert_second_phases};

    #[test]
    fn matches_the_oracle_on_the_tas() {
        let Some(scene) = Scene::tas(960) else {
            return;
        };
        let coverage = assert_second_phases(scene, 280, &[0x02]);
        assert!(coverage.occupied > 200, "{coverage:?}");
        assert!(coverage.drawn > 0, "{coverage:?}");
    }
}
