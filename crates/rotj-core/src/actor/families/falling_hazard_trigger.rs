//! Type `$A0`, an area-1A proximity spawner (bank 12 `$83CD`).

use crate::actor::engine::{self, Work};
use crate::actor::slots::{Body, Counter};
use crate::actor::world::Cx;
use crate::actors::CELL_PENDING;

/// The type the handler hands to a free child slot (`$84E2`).
const CHILD: u8 = 0xA2;
/// Past this distance from the player it despawns (`$21` >= `$18`).
const FAR: u16 = 0x1800;
/// At or past this distance the spawn timer resets to `$28` (`$21` >= `$04`).
const NEAR: u16 = 0x0400;

/// Type `$A0` keeps its whole state in [`Body`]: the spawn timer `$7E10` and its
/// countdown `$7E90`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$83CD`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        let dx = engine::player_dx(work, &cx.effects.player).unsigned_abs();
        if dx >= FAR {
            engine::despawn(kind, body, CELL_PENDING, cx.schedule);
            return;
        }
        body.anim.tick_down();
        if !body.anim.ran_out() {
            return;
        }
        if let Some(index) = engine::free_child_slot(cx.slots) {
            let child = &mut cx.slots.main[index];
            child.body.x = body.x;
            child.body.y = body.y;
            child.kind = CHILD;
            child.body.state = 0;
        }
        if dx >= NEAR {
            body.counter = 0x28;
            body.anim = 0x28;
        } else {
            if body.counter >= 9 {
                body.counter = body.counter.wrapping_sub(4);
            }
            body.anim = body.counter;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Area 1A:02 from the level-start record. The `$A2` children the handler
    /// hands to slots 16-23 are freed by a `$D450` poke at the loop head each
    /// frame, before their turn.
    ///
    /// The handler dispatches on nothing: its `$7DD0` "phase" (0) is never read
    /// or written, so [`Coverage::phases`] shows phase 0 only, over both slots.
    /// It never draws (`drawn` 0, as §8b's 0 on-screen frames) and never calls
    /// `$A9C4`, so there is no contact to test. Both of its distance arms run in
    /// this scene: the `$21` >= `$04` timer reset and, on each slot's last frame
    /// (32887), the `$21` >= `$18` far despawn.
    fn scene() -> Option<Scene> {
        let mut scene = Scene::tas(32657)?;
        let pokes = &mut scene.machine.bus.trace.pokes;
        for slot in 16..24 {
            pokes.push((0xD450, 0x7CC0 + slot, 0));
        }
        // Residue on the child slot the spawn always reuses (23), so the position
        // and state it writes are visible to the diff.
        for (at, value) in [
            (0x7B17, 0x11),
            (0x7B37, 0x22),
            (0x7B57, 0x33),
            (0x7B77, 0x44),
            (0x7D67, 0x55),
        ] {
            pokes.push((0xD450, at, value));
        }
        Some(scene)
    }

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = scene() else {
            return;
        };
        let coverage = assert_actor_phases(scene, 300, &[0xA0]);
        let expected = Coverage {
            frames: 284,
            occupied: 214,
            drawn: 0,
            phases: BTreeMap::from([(0, 397)]),
        };
        assert_eq!(coverage, expected);
    }
}
