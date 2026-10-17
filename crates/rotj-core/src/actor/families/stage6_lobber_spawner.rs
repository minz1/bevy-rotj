//! Type `$94`, an area-1C stage controller (bank 12 `$8000`).

use crate::actor::engine::Work;
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The type the handler hands to the highest free main slot.
const CHILD: u8 = 0x96;

/// `$7DD0` bit 7: a child is out, so the spawner idles until its death clears the bit.
const SPAWNED: u8 = 0x80;
/// Type `$94` keeps its whole state in [`Body`] (`$7DD0` phase, `$7E90` counter).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8000`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        if cx.world.camera_x.to_be_bytes()[0] == 0 {
            *kind = 0;
            return;
        }
        if body.phase & SPAWNED != 0 || !cx.world.frame_odd {
            return;
        }
        body.anim = body.anim.wrapping_sub(1);
        if body.anim & 0x80 == 0 {
            return;
        }
        let Some(index) = (0..16).rev().find(|&y| cx.slots.main[y].kind == 0) else {
            return;
        };
        body.phase = SPAWNED;
        body.anim = 0x80;
        let parent = work.slot;
        let child = &mut cx.slots.main[index];
        child.kind = CHILD;
        child.body.counter = parent;
        child.body.hit = 0;
        child.body.phase = 0;
        child.body.timer = 0;
        child.body.anim = 0;
        child.body.contact = 0x50;
        child.body.health = 0x0008;
        child.body.state = 0;
        child.body.flags = 1;
        child.body.chr_slot = 6;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Area 1C:00, slot 1, from the load's last frame (33631): the first gameplay
    /// loop frame is 33632. The `$96` child the handler hands to slot 15 is
    /// unported, so a `$D450` poke frees it at the loop head each frame; that is
    /// also what stops its death from re-arming the parent.
    ///
    /// Phases: `$7DD0` 0 for the spawn frame, `$80` for the rest (the handler sets
    /// its own phase when it spawns and only the child's death clears it). The
    /// camera-left-edge free (`$8002`) and the no-free-slot return (`$8025`) are
    /// branches, not phases, and the scene reaches neither.
    #[test]
    fn matches_the_oracle() {
        let Some(mut scene) = Scene::tas(33631) else {
            return;
        };
        scene.machine.bus.trace.pokes.push((0xD450, 0x7CCF, 0));
        let coverage = assert_actor_phases(scene, 120, &[0x94]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 120,
                occupied: 120,
                drawn: 0,
                phases: BTreeMap::from([(0, 1), (0x80, 119)]),
            }
        );
    }
}
