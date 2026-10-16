//! Type `$1A`, a level trigger (bank 8 `$8205`): like `$02`, but only while the
//! camera is at y 0, and it frees itself (re-armed `$C0`) when the player is far.

use crate::actor::engine::{self, Proximity, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

const REARM: u8 = 0xC0;

/// Type `$1A` keeps no state of its own; its `$7D50` is the transition parameter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8205`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        if cx.effects.player.energy & 0x80 != 0
            || cx.effects.player.flags[2] & 0x80 != 0
            || cx.world.camera_y != 0
        {
            return;
        }
        work.hitbox = 0;
        engine::load(body, work);
        match engine::player_proximity(work, cx) {
            Proximity::Touching => {
                cx.effects.transition = 1;
                cx.effects.transition_arg = body.state;
            }
            Proximity::Far => engine::despawn(kind, body, REARM, cx.schedule),
            Proximity::Clear => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    const SPAWN: u64 = 13933;
    const LOOPS: u64 = 40;

    fn scene() -> Option<Scene> {
        let scene = Scene::tas(SPAWN)?;
        assert_eq!(scene.machine.bus.wram[0x1CC1], 0x1A);
        Some(scene)
    }

    fn actor_at(scene: &Scene) -> (u16, u16) {
        let wram = &scene.machine.bus.wram;
        (
            u16::from_be_bytes([wram[0x1B01], wram[0x1B21]]),
            u16::from_be_bytes([wram[0x1B41], wram[0x1B61]]),
        )
    }

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = scene() else {
            return;
        };
        let coverage = assert_actor_phases(scene, LOOPS, &[0x1A]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 31,
                occupied: 31,
                drawn: 0,
                phases: BTreeMap::from([(0, 31)]),
            }
        );
    }

    #[test]
    fn arms_the_transition_when_the_player_reaches_it() {
        let Some(mut scene) = scene() else {
            return;
        };
        let (x, y) = actor_at(&scene);
        scene.player(x, y);
        let mut oracle = scene.clone();
        oracle.run(2, &[]);
        assert_eq!(oracle.machine.bus.ram[0xBA], 1);
        assert_eq!(oracle.machine.bus.ram[0xBB], 0xE1);
        assert_eq!(oracle.machine.bus.wram[0x1CC1], 0x1A);
        let coverage = assert_actor_phases(scene, 2, &[0x1A]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }

    #[test]
    fn frees_itself_when_the_player_is_far() {
        let Some(mut scene) = scene() else {
            return;
        };
        let (x, y) = actor_at(&scene);
        scene.player(x.wrapping_sub(0x2000), y);
        let mut oracle = scene.clone();
        oracle.run(2, &[]);
        assert_eq!(oracle.machine.bus.wram[0x1CC1], 0);
        let coverage = assert_actor_phases(scene, 4, &[0x1A]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }

    #[test]
    fn a_scrolled_camera_or_a_hurt_player_skips_it() {
        for poke in [(0xCE, 0x10), (0xCF, 0x01), (0x0478, 0x80), (0x047A, 0x80)] {
            let Some(mut scene) = scene() else {
                return;
            };
            let (x, y) = actor_at(&scene);
            scene.player(x, y);
            scene.machine.bus.trace.pokes.push((0xD450, poke.0, poke.1));
            let mut oracle = scene.clone();
            oracle.run(3, &[]);
            assert_eq!(oracle.machine.bus.ram[0xBA], 0, "{poke:?}");
            let coverage = assert_actor_phases(scene, 3, &[0x1A]);
            assert_eq!(coverage.occupied, coverage.frames, "{poke:?}: {coverage:?}");
        }
    }
}
