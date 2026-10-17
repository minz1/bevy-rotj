//! Type `$12`, the stage-1-2 controller that seeds type `$14` scenery (bank 8 `$8C56`).

use crate::actor::engine::{self, Work};
use crate::actor::families::Brain;
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The 16-bit spawn countdown targets: low byte (bank 8 `$8D3E`), high byte (`$8D4E`).
const TARGET_LOW: u16 = 0x8D3E;
const TARGET_HIGH: u16 = 0x8D4E;
/// The per-phase x high-byte add selector (bank 8 `$8D5E`).
const PHASE_HIGH: u16 = 0x8D5E;
/// The x low-byte offset at the camera's `$90` page (bank 8 `$8D6E`).
const X_OFFSET: u16 = 0x8D6E;
/// The camera-relative x pair (bank 14 `$C5F2` high, `$C6F2` low): `index << 4`.
const X_HIGH: u16 = 0xC5F2;
const X_LOW: u16 = 0xC6F2;
/// The type `$14` child written at `$8D19`.
const CHILD: u8 = 0x14;

/// Type `$12` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8C56`; phases 0-`$0F`, wrapping at `$10`).
    pub fn step(&mut self, _kind: &mut u8, body: &mut Body, _work: &mut Work, cx: &mut Cx<'_>) {
        let phase = u16::from(body.phase);
        let low = engine::rom_byte(cx.prg, 8, TARGET_LOW + phase);
        let high = engine::rom_byte(cx.prg, 8, TARGET_HIGH + phase);
        if body.timer != low || body.counter != high {
            let (timer, carry) = body.timer.overflowing_add(1);
            body.timer = timer;
            if carry {
                body.counter = body.counter.wrapping_add(1);
            }
            return;
        }
        let Some(slot) = engine::free_child_slot(cx.slots) else {
            return;
        };
        cx.effects.sound[0] = 0x3D;
        spawn(slot, body.phase, cx);
        body.phase = body.phase.wrapping_add(1);
        if body.phase >= 0x10 {
            body.phase = 0;
            body.timer = 0;
            body.counter = 0;
        }
        body.state |= 0x40;
    }
}

/// Writes the `$14` child at `camera - $D7` px into `slot` (`$8C94`-`$8D19`).
fn spawn(slot: usize, phase: u8, cx: &mut Cx<'_>) {
    let prg = cx.prg;
    let distance = cx.world.spawn_distance;
    let [cam_hi, cam_lo] = cx.world.camera_x.to_be_bytes();
    let index = if distance & 0x80 == 0 {
        distance
    } else {
        distance.wrapping_neg()
    };
    let off_hi = engine::rom_byte(prg, 14, X_HIGH + u16::from(index));
    let off_lo = engine::rom_byte(prg, 14, X_LOW + u16::from(index));
    let (lo, carry) = if distance & 0x80 == 0 {
        cam_lo.overflowing_sub(off_lo)
    } else {
        cam_lo.overflowing_add(off_lo)
    };
    let hi = if distance & 0x80 == 0 {
        cam_hi.wrapping_sub(off_hi).wrapping_sub(u8::from(carry))
    } else {
        cam_hi.wrapping_add(off_hi).wrapping_add(u8::from(carry))
    };
    let offset = engine::rom_byte(prg, 8, X_OFFSET + u16::from(cam_hi == 0x90));
    let (lo, carry) = lo.overflowing_add(offset);
    let table = engine::rom_byte(prg, 8, PHASE_HIGH + u16::from(phase));
    let hi = hi
        .wrapping_add(if table & 0x80 != 0 { 0x08 } else { 0x0A })
        .wrapping_add(u8::from(carry));
    let child = &mut cx.slots.main[slot];
    child.body.x = u16::from_be_bytes([hi, lo]);
    child.body.y = 0x0400;
    child.body.phase = 0;
    child.body.timer = 0;
    child.body.counter = 0;
    child.body.state = 0x40;
    child.kind = CHILD;
    child.brain = Brain::new(CHILD);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Stage 1-2's `$12` controller: it seeds one `$14` per phase over a 1440-frame
    /// cycle. From frame 3000 the cycle survives isolation; from 2116 an isolated
    /// room change clears the slot at frame 3593, one target short of phase 13.
    ///
    /// The `$14` children reach falling_scenery phase 3, which spawns a `$18` without setting its
    /// brain (a `$14` bug outside this family), and the column spawner keeps arming
    /// cells after the one-time `isolate`. Free every other slot each frame so this
    /// scene diffs the slot-0 `$12` alone: it is never drawn and never contacts.
    #[test]
    fn matches_the_oracle() {
        let Some(mut scene) = Scene::tas(3000) else {
            return;
        };
        for slot in 1..24u16 {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, 0x7CC0 + slot, 0));
        }
        let coverage = assert_actor_phases(scene, 1700, &[0x12]);
        let expected = Coverage {
            frames: 1574,
            occupied: 1574,
            drawn: 0,
            phases: BTreeMap::from([
                (0, 258),
                (1, 258),
                (2, 237),
                (3, 129),
                (4, 87),
                (5, 51),
                (6, 65),
                (7, 65),
                (8, 129),
                (9, 65),
                (10, 65),
                (11, 33),
                (12, 33),
                (13, 33),
                (14, 33),
                (15, 33),
            ]),
        };
        assert_eq!(coverage, expected);
    }
}
