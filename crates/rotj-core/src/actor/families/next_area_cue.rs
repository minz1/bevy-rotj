//! Type `$82`, a stage transition controller (bank 0 `$931F`): on its first step
//! it stops the music (`$9326`) and arms an `$80` guard in `$7E10`; it then counts `$7E90`
//! up, requests song `$44` at `$41` (`$933B`), selects CHR bank 2 (`$E2`), and once `$7E90`
//! reaches `$C0` clears the HUD and the contact gates and arms the next-area
//! transition (`$BA` 3). It never loads the working set, draws, or contacts the
//! player, and it calls no engine or collision routine.

use crate::actor::engine::Work;
use crate::actor::slots::{Body, Counter};
use crate::actor::world::{Cx, sound};

/// The `$7E10` guard written with it (`$932A`).
const GUARD: u8 = 0x80;
/// The `$7E90` count that arms the next-area transition (`$9333`).
const ARMED: u8 = 0xC0;
/// The CHR bank 2 value (`$E2`), by `$7E90` bit 2 (`$9345`).
const CHR_ODD: u8 = 0x0E;
const CHR_EVEN: u8 = 0x0F;
/// `$E2` is CHR bank 2 of the OAM builder's shadow (`$E0-$E7`).
const CHR_BANK: usize = 2;

/// Type `$82` keeps its whole state in [`Body`]: the guard in [`Body::counter`]
/// (`$7E10`) and the count in [`Body::anim`] (`$7E90`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$931F`).
    pub fn step(&mut self, _kind: &mut u8, body: &mut Body, _work: &mut Work, cx: &mut Cx<'_>) {
        if body.counter == 0 {
            cx.effects.music = sound::music::STOP;
            body.counter = GUARD;
        }
        body.anim.tick_up();
        if body.anim < 0x40 {
            return;
        }
        if body.anim == 0x40 {
            cx.effects.song = sound::song::NEXT_AREA;
        }
        if body.anim >= ARMED {
            cx.effects.hud = 0;
            cx.effects.player.power_up = 0;
            cx.effects.transition = 3;
            return;
        }
        cx.oam.chr[CHR_BANK] = if body.anim & 0x04 != 0 {
            CHR_ODD
        } else {
            CHR_EVEN
        };
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Area `1A:04` (level 10): the level-start record in slot 0 runs its whole
    /// `$C0`-frame count, phase 0, never on screen. `Scene::tas(33305)` is the
    /// first frame the inventory lists for type `$82`; the actor loop's first
    /// bracket is the frame it appears, and the window stops just before the
    /// next-area transition its last frame arms.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(33305) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 208, &[0x82]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 192,
                occupied: 192,
                drawn: 0,
                phases: BTreeMap::from([(0, 192)]),
            }
        );
    }
}
