//! Type `$76`, a one-shot level-start record (bank 0 `$8D3F`): it stores `$70`
//! in `$0451`/`$0452`, the page order rows 17-18 (`Schedule::rows`), and frees its own slot. It never draws and never touches the player, and it calls no engine or
//! collision routine.

use crate::actor::engine::Work;
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// Type `$76` keeps no state of its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8D3F`).
    pub fn step(&mut self, kind: &mut u8, _body: &mut Body, _work: &mut Work, cx: &mut Cx<'_>) {
        cx.schedule.rows[17..19].copy_from_slice(&[0x70; 2]);
        *kind = 0;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(8091) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 60, &[0x76]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 12,
                occupied: 1,
                drawn: 0,
                phases: BTreeMap::from([(0, 1)]),
            }
        );
    }
}
