//! Type `$0E`, a stage controller (bank 8 `$8B44`): it arms the `$BA` transition
//! and frees its own slot. It never draws and never touches the player, and it
//! calls no engine or collision routine.

use crate::actor::engine::Work;
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// Type `$0E` keeps no state of its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8B44`).
    pub fn step(&mut self, kind: &mut u8, _body: &mut Body, _work: &mut Work, cx: &mut Cx<'_>) {
        cx.effects.transition = 2;
        cx.effects.stage_end.phase = 0;
        *kind = 0;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// The handler's one occupancy: the published TAS frame 1485 (stage 1-1 area
    /// 00:04). `$8B44` runs once, arms `$BA`, frees its slot and returns, so its
    /// only phase is 0, it is never drawn and never contacts the player.
    /// (`Scene::tas(N)` is the state just before frame `N + 1`, so the frame-1485
    /// run is bracketed from 1484.)
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(1484) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 4, &[0x0E]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 4,
                occupied: 1,
                drawn: 0,
                phases: BTreeMap::from([(0, 1)]),
            }
        );
    }

    /// Every `$7D50` state the inventory reports for type `$0E` (`00`-`05`, `60`)
    /// is ignored: forcing each still frees the slot and still matches the oracle.
    #[test]
    fn every_hinted_state_is_ignored() {
        for state in [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x60] {
            let Some(mut scene) = Scene::tas(1484) else {
                return;
            };
            scene.machine.bus.trace.pokes.push((0xD450, 0x7D52, state));
            let coverage = assert_actor_phases(scene, 4, &[0x0E]);
            assert_eq!(coverage.occupied, 1, "state {state:02X}: {coverage:?}");
        }
    }
}
