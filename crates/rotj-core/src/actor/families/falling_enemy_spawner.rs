//! Type `$B0`, a stage-7 spawner (bank 12 `$8B92`): it counts `$7E90` while counting,
//! then while spawning hands a type `$AE` child to the highest free main slot every
//! frame it runs, re-arming the count after its `$7E10` counter underflows. It never
//! draws and never touches the player.

use crate::actor::engine::Work;
use crate::actor::slots::{Body, Counter, contact};
use crate::actor::world::Cx;

/// The type the handler hands to the highest free main slot.
const CHILD: u8 = 0xAE;

/// The phase (`$7DD0`) in which it counts `$7E90` down.
const COUNTING: u8 = 0;
/// The phase (`$7DD0`) in which it hands out children.
const SPAWNING: u8 = 1;

/// Type `$B0` keeps its whole state in [`Body`] (`$7DD0` phase, `$7E90` timer,
/// `$7E10` counter).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8B92`).
    pub fn step(&mut self, _kind: &mut u8, body: &mut Body, _work: &mut Work, cx: &mut Cx<'_>) {
        if body.phase == COUNTING {
            if body.anim == 0xFF {
                body.phase = SPAWNING;
                body.anim = 0;
                body.counter = 8;
            } else {
                body.anim.tick_down();
            }
            return;
        }
        if body.anim & 0x0F != 0 {
            body.anim.tick_down();
            return;
        }
        body.counter.tick_down();
        if body.counter.ran_out() {
            body.phase = COUNTING;
            body.anim = 0xC0;
        }
        let Some(index) = cx.slots.free_main(0..16) else {
            return;
        };
        // The 6502's `sbc #$03` at `$8C0A` is dead: its result is overwritten by
        // `lda #$FF` and its carry is never read.
        let random_lo = cx.world.rng[0];
        let random_hi = cx.world.rng[1];
        let child = cx.slots.main[index].spawn(CHILD, body.x, body.y);
        child.vy = i16::from((random_lo & 0x3F) + 0x30);
        child.vx_drift = if random_lo & 0x80 != 0 {
            i16::from_be_bytes([0xFF, random_hi | 0xF0])
        } else {
            i16::from_be_bytes([0, (random_hi & 0x0F) + 3])
        };
        child.contact = contact::SMITE_PROOF;
        child.state = 0;
        child.hit = 0;
        child.subtype = 0;
        child.phase = 0;
        child.flags = 0;
        child.ref_vx = 0;
        child.vx = 0;
        child.health = 0;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Area 20:02 (stage 7), slot 0, from the frame it first spawns (38784) through
    /// its whole isolated life (288 loop frames, to 39072). The handler hands a `$AE`
    /// child to the highest free main slot every frame of spawning; the child is freed
    /// at `$D450` each frame (slot 15) so the `$AE` handler never runs. The
    /// child bytes it does write are still compared, so the spawn is measured, not
    /// just sidestepped: spawning runs on the TAS's own `$7E90`/`$F9` values with no
    /// forced state.
    ///
    /// §8b lists the type in slots 0 and 2. Slot 2's spawn (~39050 in the full TAS)
    /// is not reached here: freeing the `$AE` children the handler drops lets the
    /// player go unpestered, and the room ends at 39072, before that spawn.
    #[test]
    fn matches_the_oracle() {
        let Some(mut scene) = Scene::tas(38784) else {
            return;
        };
        for slot in [1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15] {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, 0x7CC0 + slot, 0));
        }
        let coverage = assert_actor_phases(scene, 288, &[0xB0]);
        let expected = Coverage {
            frames: 288,
            occupied: 288,
            drawn: 0,
            phases: BTreeMap::from([(0, 270), (1, 18)]),
        };
        assert_eq!(coverage, expected);
    }

    /// The spawn is non-vacuous: the `$AE` child lands with the handler's computed
    /// `vy`/`vx_drift` from the frame's `$F9`/`$FA`.
    #[test]
    fn the_first_child_carries_the_handlers_bytes() {
        let Some(mut scene) = Scene::tas(38784) else {
            return;
        };
        for slot in [1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15] {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, 0x7CC0 + slot, 0));
        }
        scene.isolate(&[0xB0]);
        let events = scene.run(12, &[0xD450, 0xD453]);
        let spawn = events
            .windows(2)
            .find(|window| {
                (window[0].pc, window[1].pc) == (0xD450, 0xD453) && window[1].wram[0x1CCF] == 0xAE
            })
            .expect("spawning hands out a child");
        let (begin, end) = (&spawn[0], &spawn[1]);
        let random_lo = begin.ram[0xF9];
        let random_hi = begin.ram[0xFA];
        // The child lands in slot 15: `$7E70:$7E80` is its vy, `$7E50:$7E60` its drift.
        let vy = u16::from_be_bytes([end.wram[0x1E7F], end.wram[0x1E8F]]);
        assert_eq!(vy, u16::from((random_lo & 0x3F) + 0x30));
        let drift = u16::from_be_bytes([end.wram[0x1E5F], end.wram[0x1E6F]]);
        let expected = if random_lo & 0x80 != 0 {
            u16::from_be_bytes([0xFF, random_hi | 0xF0])
        } else {
            u16::from_be_bytes([0, (random_hi & 0x0F) + 3])
        };
        assert_eq!(drift, expected);
    }
}
