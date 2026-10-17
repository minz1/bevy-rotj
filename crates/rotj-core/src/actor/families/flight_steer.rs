//! Type `$54`, a stage script that drives the player (bank 11 `$9D49`): it forces
//! his motion state to `$0B`, steps his position with the pad and sets his walk
//! frame. It never draws and calls no engine or collision routine.

use crate::actor::button::Buttons;
use crate::actor::engine::{self, Work};
use crate::actor::slots::Body;
use crate::actor::world::{Cx, Motion, stage_script};

const NO_CLAMP: u8 = 0xFF;
const STEP: u16 = 0x20;
const DRIFT: u16 = 0x0A;
const Y_MAX: u8 = 0x0C;
const Y_MIN: u8 = 0x04;

/// Type `$54`'s tables (bank 11).
pub struct Tables {
    /// The player's walk frame, by `(frame >> 1) & 3` (`$9DF5`).
    pub walk_frames: [u8; 4],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        Tables {
            walk_frames: engine::rom_bytes(prg, 11, 0x9DF5),
        }
    }
}

/// Type `$54` keeps no state of its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9D49`).
    pub fn step(&mut self, _kind: &mut u8, _body: &mut Body, _work: &mut Work, cx: &mut Cx<'_>) {
        let player = &mut cx.effects.player;
        if player.motion == Motion::Dying {
            // `$9D50`: only the projectiles' types (`$7BD0`) are cleared.
            for projectile in &mut cx.effects.projectiles {
                projectile.kind = 0;
            }
            return;
        }
        player.motion = Motion::Flying;
        player.stage_script = stage_script::FROZEN | stage_script::KEEP_PADS;
        cx.effects.cutscene_id = 0xA0;
        player.x = player.x.wrapping_add(DRIFT);
        if cx.effects.scroll_lock != NO_CLAMP {
            let pad = cx.world.pad;
            if pad.intersects(Buttons::SIDEWAYS) {
                player.x = if pad.contains(Buttons::RIGHT) {
                    player.x.wrapping_add(STEP)
                } else {
                    player.x.wrapping_sub(STEP)
                };
            }
            if pad.intersects(Buttons::UP_OR_DOWN) {
                if pad.contains(Buttons::UP) {
                    let (low, borrow) = player.y.to_be_bytes()[1].overflowing_sub(0x20);
                    let mut high = player.y.to_be_bytes()[0];
                    if borrow {
                        high = high.wrapping_sub(1);
                    }
                    player.y = u16::from_be_bytes([high, low]);
                    if borrow && high < Y_MIN {
                        player.y = u16::from(Y_MIN) << 8;
                    }
                } else {
                    player.y = player.y.wrapping_add(STEP);
                    if player.y.to_be_bytes()[0] >= Y_MAX {
                        player.y = u16::from(Y_MAX) << 8;
                    }
                }
            }
        }
        let index = usize::from((cx.world.frame & 0x06) >> 1);
        player.anim = cx.prg.families.flight_steer.walk_frames[index];
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(8600) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 60, &[0x54]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 27,
                occupied: 27,
                drawn: 0,
                phases: BTreeMap::from([(0, 27)]),
            }
        );
    }

    /// `$9D50`: with the player finished (motion 7), only the weapon kinds (`$7BD0`) are
    /// cleared; the rest of each shot is left.
    #[test]
    fn a_finished_script_clears_only_the_weapon_kinds() {
        let Some(mut scene) = Scene::tas(8600) else {
            return;
        };
        for (addr, value) in [(0x7BD0, 0x40), (0x7B80, 0x33), (0x7F30, 0x80)] {
            scene.machine.bus.trace.pokes.push((0xD450, addr, value));
        }
        scene.machine.bus.trace.pokes.push((0xD450, 0x046E, 7));
        let finished = scene.clone().run(40, &[0x9D50]).len();
        assert!(finished > 0, "the oracle never reached $9D50");
        let coverage = assert_actor_phases(scene, 40, &[0x54]);
        assert!(coverage.occupied > 0, "{coverage:?}");
        assert_eq!(coverage.drawn, 0, "{coverage:?}");
    }
}
