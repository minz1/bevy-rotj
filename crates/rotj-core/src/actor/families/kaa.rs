//! Type `$AA`, a stage 6 boss part (bank 12 `$8870`).

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// Phase 0's sprite selector, indexed by `$7E90 >> 3` (`$893F`).
const SELECTOR: u16 = 0x893F;
/// The first draw's metasprite pointers, indexed by `$7C30` (`$8944`, bank 9).
const FIRST: u16 = 0x8944;
/// The second draw's pointers, indexed by `$7E10` (`$894E`, bank 9).
const SECOND: u16 = 0x894E;
/// The draws map bank 9 at `$A000` (`$8900`).
const DRAW_BANK: u8 = 9;
/// Phase 0 flips to phase 1 when `$7E90` reaches this (`$889D`).
const PHASE_1: u8 = 0x28;

/// Type `$AA` keeps its whole state in [`Body`] (`$7DD0` phase, `$7E90` counter,
/// `$7E10` draw index, `$7C30` sprite index, `$7E70:$7E80` velocity).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8870`, phases at `$893B`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        match body.phase {
            0 => {
                body.sprite = engine::rom_byte(cx.prg, 12, SELECTOR + u16::from(body.anim >> 3));
                body.counter = 0;
                body.anim = body.anim.wrapping_add(1);
                if body.anim >= PHASE_1 {
                    body.vy = 0;
                    body.phase = 1;
                }
            }
            1 => {
                body.counter = 1;
                body.sprite = 3 + ((cx.world.frame & 0x04) >> 2);
                work.y = work.y.wrapping_add_signed(body.vy);
                let [hi, lo] = body.vy.to_be_bytes();
                let (lo, borrow) = lo.overflowing_sub(1);
                let hi = if borrow { hi.wrapping_sub(1) } else { hi };
                body.vy = i16::from_be_bytes([hi, if lo >= 0xD0 { lo } else { 0xC0 }]);
            }
            phase => panic!("type $AA phase {phase} is past the $893B table"),
        }
        if engine::on_screen(work, cx.world) != Visibility::On {
            *kind = 0;
            return;
        }
        work.tile = 0;
        work.attr = 0;
        work.pointer = engine::rom_word(cx.prg, 12, FIRST + 2 * u16::from(body.sprite));
        engine::draw(work, DRAW_BANK, cx);
        work.pointer = engine::rom_word(cx.prg, 12, SECOND + 2 * u16::from(body.counter));
        engine::draw(work, DRAW_BANK, cx);
        engine::store(body, work);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Stage 6's boss room, area 1E:00, where the `$A8` parent's two `$AA` parts
    /// run in slots 12 and 14 from frame 37456. `$7DD0` phase 0 is the `$28`-frame
    /// countdown, phase 1 the rise it then runs until it leaves the screen and
    /// frees its own slot (`$8931`). The handler calls no contact routine, so
    /// there is no touch to test; `drawn` counts the loops that changed the OAM
    /// page (the two metasprites per part).
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(37457) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 110, &[0xAA]);
        let expected = Coverage {
            frames: 110,
            occupied: 97,
            drawn: 81,
            phases: BTreeMap::from([(0, 79), (1, 114)]),
        };
        assert_eq!(coverage, expected);
    }
}
