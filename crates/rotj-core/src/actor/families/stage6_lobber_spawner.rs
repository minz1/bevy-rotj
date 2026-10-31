//! Type `$94`, an area-1C stage controller (bank 12 `$8000`).

use crate::actor::engine::Work;
use crate::actor::slots::{Body, Counter, contact};
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
        body.anim.tick_down();
        if !body.anim.ran_out() {
            return;
        }
        let Some(index) = cx.slots.free_main(0..16) else {
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
        child.body.contact = contact::PASSIVE | contact::SLIDE_HURTS;
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
    /// loop frame is 33632. The `$96` child the handler hands to slot 15 is freed
    /// by a `$D450` poke at the loop head each frame; that is also what stops its
    /// death from re-arming the parent.
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

    /// Slot 23's narrow fields are slot 7's next-array bytes: with slots 8-15 held by idle
    /// spawners (`$7DD0` = `$80`), slot 23 hands its child slot 7, and the child's `anim`,
    /// `contact` and `health` high (`$7E97`, `$7EE7`, `$7EF7`) are slot 23's `VyLow`,
    /// `RefVxHigh` and `Contact`. The game keeps those writes; the actor loop must too.
    #[test]
    fn a_child_in_a_byte_slot_23_aliases_keeps_its_bytes() {
        let Some(mut scene) = Scene::tas(33631) else {
            return;
        };
        let wram = &mut scene.machine.bus.wram;
        for slot in std::iter::once(1).chain(8..16) {
            wram[0x1CC0 + slot] = 0x94;
            wram[0x1DD0 + slot] = 0x80;
        }
        wram[0x1CC0 + 23] = 0x94;
        wram[0x1DD0 + 23] = 0;
        wram[0x1E90 + 23] = 0;
        let mut probe = scene.clone();
        probe.isolate(&[0x94]);
        probe.run(2, &[]);
        let wram = &probe.machine.bus.wram;
        assert_eq!(wram[0x1CC0 + 7], 0x96, "the child lands in slot 7");
        assert_eq!(wram[0x1E10 + 7], 23, "slot 23 spawned it");
        let coverage = assert_actor_phases(scene, 2, &[0x94]);
        assert_eq!((coverage.frames, coverage.occupied), (2, 2));
    }
}
