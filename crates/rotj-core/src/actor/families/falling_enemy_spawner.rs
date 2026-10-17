//! Type `$B0`, a stage-7 spawner (bank 12 `$8B92`): it counts `$7E90` in phase 0,
//! then in phase 1 hands a type `$AE` child to the highest free main slot every
//! frame it runs, re-arming phase 0 after its `$7E10` counter underflows. It never
//! draws and never touches the player.

use crate::actor::engine::Work;
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The type the handler hands to the highest free main slot.
const CHILD: u8 = 0xAE;

/// Type `$B0` keeps its whole state in [`Body`] (`$7DD0` phase, `$7E90` timer,
/// `$7E10` counter).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8B92`).
    pub fn step(&mut self, _kind: &mut u8, body: &mut Body, _work: &mut Work, cx: &mut Cx<'_>) {
        if body.phase == 0 {
            if body.anim == 0xFF {
                body.phase = 1;
                body.anim = 0;
                body.counter = 8;
            } else {
                body.anim = body.anim.wrapping_sub(1);
            }
            return;
        }
        if body.anim & 0x0F != 0 {
            body.anim = body.anim.wrapping_sub(1);
            return;
        }
        body.counter = body.counter.wrapping_sub(1);
        if body.counter & 0x80 != 0 {
            body.phase = 0;
            body.anim = 0xC0;
        }
        let Some(index) = (0..16).rev().find(|&y| cx.slots.main[y].kind == 0) else {
            return;
        };
        // The 6502's `sbc #$03` at `$8C0A` is dead: its result is overwritten by
        // `lda #$FF` and its carry is never read.
        let random_lo = cx.world.rng[0];
        let random_hi = cx.world.rng[1];
        let child = &mut cx.slots.main[index];
        child.kind = CHILD;
        child.body.x = body.x;
        child.body.y = body.y;
        child.body.vy = i16::from((random_lo & 0x3F) + 0x30);
        child.body.vx_drift = if random_lo & 0x80 != 0 {
            i16::from_be_bytes([0xFF, random_hi | 0xF0])
        } else {
            i16::from_be_bytes([0, (random_hi & 0x0F) + 3])
        };
        child.body.contact = 8;
        child.body.state = 0;
        child.body.hit = 0;
        child.body.subtype = 0;
        child.body.phase = 0;
        child.body.flags = 0;
        child.body.ref_vx = 0;
        child.body.vx = 0;
        child.body.health = 0;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Area 20:02 (stage 7), slot 0, from the frame it first spawns (38784) through
    /// its whole isolated life (288 loop frames, to 39072). The handler hands a `$AE`
    /// child to the highest free main slot every frame of phase 1; the child is freed
    /// at `$D450` each frame (slot 15) so the unported `$AE` handler never runs. The
    /// child bytes it does write are still compared, so the spawn is measured, not
    /// just sidestepped: phase 1 runs on the TAS's own `$7E90`/`$F9` values with no
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
            .expect("phase 1 spawns a child");
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
