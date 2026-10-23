//! Type `$12`, the stage-1-2 controller that seeds type `$14` scenery (bank 8 `$8C56`).

use crate::actor::engine::{self, Work};
use crate::actor::slots::{Body, Counter};
use crate::actor::world::{Cx, sound};
use crate::actors::STATE_UNTARGETED;

/// The type `$14` child written at `$8D19`.
const CHILD: u8 = 0x14;

/// How many scenery phases there are (`$7DD0` wraps back at this count).
const PHASE_COUNT: u8 = 0x10;
/// The phase `$7DD0` wraps back to.
const FIRST_PHASE: u8 = 0;
/// The spawner's tables (bank 8), by its `phase` (0-15, wrapped at `$10`).
pub struct Tables {
    /// The countdown target's low byte, by `phase` (`$8D3E`).
    pub target_low: [u8; 16],
    /// The countdown target's high byte, by `phase` (`$8D4E`).
    pub target_high: [u8; 16],
    /// The child's x high-byte add selector, by `phase`; only bit 7 is used (`$8D5E`).
    pub high_selectors: [u8; 16],
    /// The child's x low-byte offset, by whether the camera is on page `$90` (`$8D6E`).
    pub x_offsets: [u8; 2],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        Tables {
            target_low: engine::rom_bytes(prg, 8, 0x8D3E),
            target_high: engine::rom_bytes(prg, 8, 0x8D4E),
            high_selectors: engine::rom_bytes(prg, 8, 0x8D5E),
            x_offsets: engine::rom_bytes(prg, 8, 0x8D6E),
        }
    }
}

/// Type `$12` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8C56`; phases 0-`$0F`, wrapping at `$10`).
    pub fn step(&mut self, _kind: &mut u8, body: &mut Body, _work: &mut Work, cx: &mut Cx<'_>) {
        let tables = &cx.prg.families.scenery_spawner;
        let phase = usize::from(body.phase);
        let (low, high) = (tables.target_low[phase], tables.target_high[phase]);
        if body.timer != low || body.counter != high {
            let (timer, carry) = body.timer.overflowing_add(1);
            body.timer = timer;
            if carry {
                body.counter.tick_up();
            }
            return;
        }
        let Some(slot) = engine::free_child_slot(cx.slots) else {
            return;
        };
        cx.effects.sound[0] = sound::DROP;
        spawn(slot, body.phase, cx);
        body.phase = body.phase.wrapping_add(1);
        if body.phase >= PHASE_COUNT {
            body.phase = FIRST_PHASE;
            body.timer = 0;
            body.counter = 0;
        }
        body.state |= STATE_UNTARGETED;
    }
}

/// Writes the `$14` child at `camera - $D7` px into `slot` (`$8C94`-`$8D19`).
fn spawn(slot: usize, phase: u8, cx: &mut Cx<'_>) {
    let tables = &cx.prg.families.scenery_spawner;
    let distance = cx.world.spawn_distance;
    let [cam_hi, _] = cx.world.camera_x.to_be_bytes();
    let index = if distance & 0x80 == 0 {
        distance
    } else {
        distance.wrapping_neg()
    };
    let x = if distance & 0x80 == 0 {
        cx.world.camera_x.wrapping_sub(u16::from(index) << 4)
    } else {
        cx.world.camera_x.wrapping_add(u16::from(index) << 4)
    };
    let [hi, lo] = x.to_be_bytes();
    let offset = tables.x_offsets[usize::from(cam_hi == 0x90)];
    let (lo, carry) = lo.overflowing_add(offset);
    let table = tables.high_selectors[usize::from(phase)];
    let hi = hi
        .wrapping_add(if table & 0x80 != 0 { 0x08 } else { 0x0A })
        .wrapping_add(u8::from(carry));
    let x = u16::from_be_bytes([hi, lo]);
    let child = cx.slots.main[slot].spawn(CHILD, x, 0x0400);
    child.phase = 0;
    child.timer = 0;
    child.counter = 0;
    child.state = STATE_UNTARGETED;
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
