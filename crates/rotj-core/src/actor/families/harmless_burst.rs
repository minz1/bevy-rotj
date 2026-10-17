//! Type `$08`, the four-part burst `$AC16` spawns for type `$0A` (bank 8 `$8506`).

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The four parts' six array pointers each (`$85CD`): x low/high, y low/high, x
/// velocity, y velocity. Written into `$20-$2B` one part at a time.
const PARTS: u16 = 0x85CD;
/// The metasprite pointer of the even and odd frame (`$85BB`).
const SPRITES: u16 = 0x85BB;
/// The attribute base of the even and odd frame (`$85B8`).
const ATTRS: u16 = 0x85B8;
/// The pair's lifetime counter, the high byte of `ref_vx` (`$7ED0`).
const LIFETIME: u16 = 0x7ED0;
const GRAVITY: u8 = 0x0C;

/// Type `$08` has no handler phase: its `$7DD0` byte is part 0's x velocity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8506`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        // `$F1 & 1` picks the metasprite and attribute base; the NMI increments
        // `$F1` and flips `$F7` bit 7 together, so it is `frame_odd`.
        let odd = cx.world.frame_odd;
        let index = 2 * u16::from(odd);
        work.pointer = engine::rom_word(cx.prg, 8, SPRITES + index);
        work.attr = engine::rom_byte(cx.prg, 8, ATTRS + index) | body.flags;
        work.tile = 0;
        // The 6502 walks the four records as `$12` = 3, 2, 1, 0 and moves the part
        // whose `$12 ^ $F1` is odd, so a frame touches two of the four.
        for state in (0..4u8).rev() {
            if (state ^ u8::from(odd)) & 1 == 0 {
                continue;
            }
            let record = PARTS + 12 * u16::from(3 - state);
            let at = |i: u16| engine::rom_word(cx.prg, 8, record + 2 * i);
            let (x_lo, x_hi, y_lo, y_hi, vx, vy) = (at(0), at(1), at(2), at(3), at(4), at(5));
            work.x = engine::move_by_velocity(body, x_lo, x_hi, vx);
            work.y = engine::move_by_velocity(body, y_lo, y_hi, vy);
            body.set_byte(vy, body.byte(vy).wrapping_add(GRAVITY));
            if engine::on_screen(work, cx.world) == Visibility::On {
                engine::draw(work, 8, cx);
            }
        }
        let lifetime = body.byte(LIFETIME).wrapping_sub(1);
        body.set_byte(LIFETIME, lifetime);
        if lifetime & 0x80 != 0 {
            *kind = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    #[test]
    fn matches_the_oracle_next_to_its_parent() {
        let Some(mut scene) = Scene::route(953) else {
            return;
        };
        let wram = &scene.machine.bus.wram;
        assert_eq!(wram[0x1CC3], 0x0A);
        let x = u16::from_be_bytes([wram[0x1B03], wram[0x1B23]]);
        scene.player(x - 0x200, 0x0BFF);
        let coverage = assert_actor_phases(scene, 200, &[0x0A, 0x08]);
        // The `$0A` parent walks phases 0-3; the `$08` child has no phase dispatch
        // (`$7DD0` is part 0's x velocity, `$E0` for the `$0A`'s variant-2 burst) and
        // its `$7ED0` lifetime of `$20` frames shows as the same count.
        let expected = Coverage {
            frames: 200,
            occupied: 200,
            drawn: 200,
            phases: BTreeMap::from([(0, 207), (1, 47), (2, 33), (3, 102), (0xE0, 33)]),
        };
        assert_eq!(coverage, expected);
    }
}
