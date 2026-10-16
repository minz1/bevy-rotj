//! Type `$02`, a level trigger (bank 8 `$81D4`): when the player is in range it
//! arms the `$BA` transition and passes `$7D50` on in `$BB` (bank 14 `$D4E7`).

use crate::actor::engine::{self, Proximity, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// Type `$02` keeps no state of its own; it reuses its spawn record's state byte
/// as the transition parameter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$81D4`).
    pub fn step(&mut self, _kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        if cx.effects.player.energy & 0x80 != 0 || cx.effects.player.flags[2] & 0x80 != 0 {
            return;
        }
        work.hitbox = 0;
        engine::load(body, work);
        if engine::player_proximity(work, cx) == Proximity::Touching {
            cx.effects.transition = 1;
            cx.effects.transition_arg = body.state;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// The trigger's one phase (`$7DD0` is never written), on screen but never
    /// drawn, from its spawn at frame 968 through frame 1024, where it fires.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::replay("stage1-inputs.csv", 967) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 57, &[0x02]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 57,
                occupied: 57,
                drawn: 0,
                phases: BTreeMap::from([(0, 57)]),
            }
        );
    }
}
