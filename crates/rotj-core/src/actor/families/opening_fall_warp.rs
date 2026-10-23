//! Type `$C6` (bank 12 `$9953`): falls to the right, then warps the player down.

use crate::actor::collision::{self, Probe};
use crate::actor::engine::{self, Work};
use crate::actor::slots::{Body, Counter};
use crate::actor::world::{Cx, Motion};

const METASPRITES: u16 = 0x9A16;
const DRIFT: i16 = 0x18;
const FLOOR_ROW: u8 = 0x0A;
const WARP_Y: u16 = 0x0BFF;
const LAST_FRAME: u8 = 0x0F;

phases! {
    /// The warp's phase (`$7DD0`), which picks its handler (`$9A22`).
    Phase {
        /// Drifting right until it lands.
        Falling = 0,
        /// Playing out the warp animation, then moving the player.
        Warping = 1,
    }
}

/// Type `$C6` keeps its phase, timer and animation counter in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9953`, phases at `$9A22`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        engine::integrate(body, work);
        match Phase::from_byte(body.phase) {
            Some(Phase::Falling) => {
                body.vx_drift = DRIFT;
                body.sprite = 0;
                if body.y.to_be_bytes()[0] >= FLOOR_ROW {
                    body.sprite = 1;
                    let mut probe = Probe::default();
                    if collision::feet(work, &mut probe, cx) {
                        body.vx_drift = 0;
                        body.phase = Phase::Warping.byte();
                    }
                }
            }
            Some(Phase::Warping) => {
                work.y = WARP_Y;
                body.y = WARP_Y;
                body.sprite = ((body.anim & 0xFC) >> 2).wrapping_add(2);
                body.anim.tick_up();
                if body.anim >= LAST_FRAME {
                    let player = &mut cx.effects.player;
                    player.x = body.x;
                    player.y = WARP_Y;
                    player.vy = 6;
                    player.motion = Motion::Standing;
                    player.stage_script = 0;
                    *kind = 0;
                }
            }
            None => panic!("type $C6 phase {} is past the $9A22 table", body.phase),
        }
        engine::on_screen(work, cx.world);
        work.tile = 0;
        work.attr = 0;
        work.chr_slot = 5;
        work.pointer = engine::rom_word(cx.prg, 12, METASPRITES + 2 * u16::from(body.sprite));
        engine::draw(work, 1, cx);
        engine::end_frame(body, work);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Route frames 641-685: the `$C6` in slot 1 falls then warps.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::route(640) else {
            return;
        };
        let wram = &scene.machine.bus.wram;
        let slot = (0..16).find(|&s| wram[0x1CC0 + s] == 0xC6).expect("a $C6");
        assert_eq!(slot, 1);
        let coverage = assert_actor_phases(scene, 45, &[0xC6]);
        let expected = Coverage {
            frames: 45,
            occupied: 45,
            drawn: 45,
            phases: BTreeMap::from([(0, 31), (1, 14)]),
        };
        assert_eq!(coverage, expected);
    }

    /// Frame 686: the warp relocates the player, sets his y velocity and `$0481`,
    /// and frees the slot.
    #[test]
    fn matches_the_oracle_through_the_warp() {
        let Some(scene) = Scene::route(640) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 46, &[0xC6]);
        let expected = Coverage {
            frames: 46,
            occupied: 46,
            drawn: 46,
            phases: BTreeMap::from([(0, 31), (1, 15)]),
        };
        assert_eq!(coverage, expected);
    }
}
