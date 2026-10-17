//! Type `$98` (bank 12 `$81DB`): a two-piece sprite at a fixed screen position.
//! It calls no `$A000` load, no projection, no physics and no contact: it sets
//! `$94`-`$99` itself, then `$AC06` (`tile_bits`) and the bank-9 draw `$C247`
//! reached through `$81C6` (which maps bank 9 at `$A000` and restores bank 13).
//! Every value it does not set stays as the previous slot left it.

use crate::actor::engine::{self, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The metasprite pointer (`$96/$97`): bank 9's `$A95C`.
const POINTER: u16 = 0xA95C;
/// `$81C6` maps bank 9 at `$A000` for the draw.
const DRAW_BANK: u8 = 9;
/// Screen x (`$94`).
const SCREEN_X: u8 = 0x74;
/// Screen y (`$95`).
const SCREEN_Y: u8 = 0x75;
/// CHR shadow the metasprite selects (`$98`).
const CHR_SLOT: u8 = 0x06;
/// Attribute bits for the draw (`$99`).
const ATTR: u8 = 0x01;
/// `$E2` the handler compares against to nudge the sprite down one pixel.
const LOWER_CHR: u8 = 0x0F;

/// Type `$98` reads only the carried working set, so it has no state of its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$81DB`).
    pub fn step(&mut self, _kind: &mut u8, _body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        work.attr = ATTR;
        work.chr_slot = CHR_SLOT;
        work.pointer = POINTER;
        work.screen_x = SCREEN_X;
        work.screen_y = SCREEN_Y;
        if cx.oam.chr[2] == LOWER_CHR {
            work.screen_y = work.screen_y.wrapping_add(1);
        }
        engine::tile_bits(work, cx.prg);
        engine::draw(work, DRAW_BANK, cx);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    use super::LOWER_CHR;

    /// Area 1C:00 from the level-start record in slot 2 (`$7D50` `$61`). The
    /// handler dispatches on no `$7DD0` and never contacts, so phase 0 is residue;
    /// it redraws the same two pieces every loop, so `drawn` counts only the loops
    /// whose OAM page differs from the previous one.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(33598) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 300, &[0x98]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 267,
                occupied: 267,
                drawn: 264,
                phases: BTreeMap::from([(0, 267)]),
            }
        );
    }

    /// The `$E2` branch: the handler nips the sprite one pixel down only when the
    /// CHR shadow is `$0F`. Poking each value in turn matches the oracle, so both
    /// arms are exercised whichever one the scene takes.
    #[test]
    fn the_chr_shadow_branch_matches_the_oracle() {
        for shadow in [0x00, LOWER_CHR] {
            let Some(mut scene) = Scene::tas(33598) else {
                return;
            };
            scene.machine.bus.trace.pokes.push((0xD450, 0xE2, shadow));
            let coverage = assert_actor_phases(scene, 60, &[0x98]);
            assert_eq!(
                coverage,
                Coverage {
                    frames: 27,
                    occupied: 27,
                    drawn: 26,
                    phases: BTreeMap::from([(0, 27)]),
                },
                "shadow {shadow:02X}"
            );
        }
    }
}
