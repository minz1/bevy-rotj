//! Type `$5C` (bank 11 `$9EC2`): a two-metasprite decoration that drifts its
//! stored position. `$F7` bit 7 picks the piece: an odd frame moves the slot's y
//! pair (`$7B40` high by -5, `$7B60` low by +6) and draws the second metasprite,
//! an even frame the x pair (`$7B00` high by -2, `$7B20` low by +3) and the
//! first. A borrow pins the high byte to `$FF`, a carry pins the low byte to 0.
//!
//! It calls only `$826F`, the bank-11 wrapper that maps bank 9 over `$A000` and
//! calls `$C247` (`docs/ACTORS.md` §9c, `engine::draw`), so the port passes bank
//! 9. It does no `$A000` load, no `$A852` projection, no contact and never frees
//! its slot: `$7DD0` stays 0.

use crate::actor::engine::{self, Work};
use crate::actor::slots::{Body, Field};
use crate::actor::world::Cx;

const HOME_BANK: u8 = 11;
const DRAW_BANK: u8 = 9;
/// The 16-bit metasprite pointer table (`$9F3E`), indexed by 0 or 2.
const METASPRITES: u16 = 0x9F3E;
const ATTR: u8 = 2;
/// The x pair (`$7B00`/`$7B20`), stepped -2/+3 on even frames.
const X_PAIR: (Field, Field) = (Field::XHigh, Field::XLow);
/// The y pair (`$7B40`/`$7B60`), stepped -5/+6 on odd frames.
const Y_PAIR: (Field, Field) = (Field::YHigh, Field::YLow);

/// Type `$5C` keeps everything in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9EC2`).
    pub fn step(&mut self, _kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        let (hi_addr, lo_addr, hi_sub, lo_add, piece) = if cx.world.frame_odd {
            (Y_PAIR.0, Y_PAIR.1, 5u8, 6u8, 2u16)
        } else {
            (X_PAIR.0, X_PAIR.1, 2u8, 3u8, 0u16)
        };
        work.screen_x = body.get(hi_addr);
        let (hi, borrow) = work.screen_x.overflowing_sub(hi_sub);
        body.set(hi_addr, if borrow { 0xFF } else { hi });
        if !borrow {
            work.screen_y = body.get(lo_addr);
            let (lo, carry) = work.screen_y.overflowing_add(lo_add);
            body.set(lo_addr, if carry { 0 } else { lo });
        }
        work.pointer = engine::rom_word(cx.prg, HOME_BANK, METASPRITES + piece);
        work.attr = ATTR;
        work.anchor = 0;
        work.tile = 0;
        engine::draw(work, DRAW_BANK, cx);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Area 0A:00: eight `$5C`s sit in slots 0-7 from frame 11925. Over the
    /// compared loops every one stays in phase 0 and draws.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(11925) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 60, &[0x5C]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 27,
                occupied: 27,
                drawn: 27,
                phases: BTreeMap::from([(0, 216)]),
            },
            "{coverage:?}"
        );
    }

    /// The natural scene never takes the x-side clamps (its x high bytes stay
    /// above 0x10 over the compared loops), so poke both pairs into a borrow
    /// and a carry just before the first actor loop. Frame 11959 is even, so the
    /// loop order is x, y, x, y: after four frames each borrow has pinned a high
    /// byte to `$FF` and each carry pinned a low byte to 0.
    #[test]
    fn the_edges_wrap_like_the_oracle() {
        let Some(mut scene) = Scene::tas(11925) else {
            return;
        };
        scene.run(33, &[]); // the actor loop's first bracket is frame 11959
        {
            let wram = &mut scene.machine.bus.wram;
            for slot in 0..8 {
                assert_eq!(wram[0x1CC0 + slot], 0x5C);
                wram[0x1B00 + slot] = 1; // x high: 1 - 2 borrows
                wram[0x1B20 + slot] = 0xFE; // x low: $FE + 3 carries
                wram[0x1B40 + slot] = 4; // y high: 4 - 5 borrows
                wram[0x1B60 + slot] = 0xFB; // y low: $FB + 6 carries
            }
        }
        let coverage = assert_actor_phases(scene.clone(), 60, &[0x5C]);
        assert_eq!(
            (coverage.frames, coverage.occupied),
            (60, 60),
            "{coverage:?}"
        );
        assert_eq!(coverage.phases, BTreeMap::from([(0, 480)]), "{coverage:?}");

        let mut probe = scene;
        probe.run(4, &[]);
        let wram = &probe.machine.bus.wram;
        for slot in 0..8 {
            assert_eq!(
                (wram[0x1B00 + slot], wram[0x1B20 + slot]),
                (0xFD, 0),
                "x clamp at slot {slot}"
            );
            assert_eq!(
                (wram[0x1B40 + slot], wram[0x1B60 + slot]),
                (0xFA, 0),
                "y clamp at slot {slot}"
            );
        }
    }
}
