//! Type `$1C`, a one-shot level-start record (bank 8 `$933A`): it stores `$37` in
//! `$43`, paints three map cells with `$60` (`$67AF`, `$68AF`, `$69AF` when its
//! `$7D50` bit 7 is set, else `$6740`, `$6840`, `$6940`) and frees its own slot.
//! It never draws and never touches the player, and it calls no engine or
//! collision routine.

use crate::map::Cell;
use crate::sim::engine::Work;
use crate::sim::slots::{Body, EMPTY};
use crate::sim::world::{Cx, sound};

/// Type `$1C` keeps no state of its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$933A`).
    pub fn step(kind: &mut u8, body: &mut Body, _work: &mut Work, cx: &mut Cx<'_>) {
        cx.effects.sound[1] = sound::LEVEL_START;
        let column = if body.state & 0x80 != 0 { 0xAF } else { 0x40 };
        for row in 7..10 {
            cx.effects.level.map[Cell { row, column }] = 0x60;
        }
        *kind = EMPTY;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::sim::oracle::{Coverage, Scene, assert_actor_phases};

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(20499) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 60, &[0x1C]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 44,
                occupied: 1,
                drawn: 0,
                phases: BTreeMap::from([(0, 1)]),
            }
        );
    }

    #[test]
    fn both_branches_match_the_oracle() {
        for state in [0x20, 0xFF] {
            let Some(mut scene) = Scene::tas(20499) else {
                return;
            };
            scene.machine.bus.trace.pokes.push((0xD450, 0x7D50, state));
            let coverage = assert_actor_phases(scene, 60, &[0x1C]);
            assert_eq!(coverage.occupied, 1, "state {state:02X}: {coverage:?}");
        }
    }
}
