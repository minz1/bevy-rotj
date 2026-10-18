//! Type `$34` (bank 11 `$8A03`): the boss hit-point readout. It draws a
//! metasprite selected by `$7D90` and mirrors the boss's hit points (`$A9:$AA`)
//! and the display value (`$B2:$B3`) into its own slot bytes, queueing the status
//! counters (`$D1F5`) whenever they change.

use crate::actor::engine::{self, Work};
use crate::actor::slots::Body;
use crate::actor::world::{Cx, Motion, sound};

/// The metasprite pointers, indexed by `$7D90` (bank 11 `$8A88`).
const METASPRITES: u16 = 0x8A88;
/// The fixed screen position the actor draws at (`$94`, `$95`).
const SCREEN_X: u8 = 0xDC;
const SCREEN_Y: u8 = 0xEF;

/// Type `$34` keeps no state of its own: its slot bytes are the HP cache.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8A03`).
    pub fn step(&mut self, _kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        // $8A03: clear the anchor, tile and CHR shadow, take the attribute from
        // the slot and the metasprite pointer from the `$7D90` table.
        work.anchor = 0;
        work.tile = 0;
        work.chr_slot = 0;
        work.attr = body.flags;
        work.pointer = engine::rom_word(cx.prg, 11, METASPRITES + 2 * u16::from(body.behaviour));
        work.screen_x = SCREEN_X;
        work.screen_y = SCREEN_Y;
        // $8A27: `$825A` maps bank 1 and draws ($C247).
        engine::draw(work, 1, cx);

        // $8A2A: the level is ending, or the display value is finished: reset it.
        let [b2, b3] = cx.effects.boss_pool;
        let ending = cx.effects.player.motion == Motion::Dying;
        if ending || b3 & 0x80 != 0 || (b2 | b3) == 0 {
            if !ending {
                cx.effects.music = sound::music::STOP;
                engine::level_end(cx.effects);
            }
            cx.effects.boss_pool = [0, 0];
        }

        // $8A48: when any cached byte differs, copy all four and queue the counters.
        let [hp_lo, hp_hi] = cx.effects.boss_hp.to_le_bytes();
        let [b2, b3] = cx.effects.boss_pool;
        if b2 == body.subtype && b3 == body.phase && hp_lo == body.timer && hp_hi == body.counter {
            return;
        }
        body.subtype = b2;
        body.phase = b3;
        body.timer = hp_lo;
        body.counter = hp_hi;
        engine::queue_counters(u16::from_le_bytes([b2, b3]), cx);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::engine;
    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases, assert_routine};

    /// Area 4 (the stage 1 boss room): the actor sits in slot 0 through the whole
    /// window and draws every frame; the `$D1F5` queue runs when the HP cache
    /// first fills.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(6261) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 522, &[0x34]);
        let expected = Coverage {
            frames: 407,
            occupied: 407,
            drawn: 407,
            phases: BTreeMap::from([(0, 1), (0x1F, 406)]),
        };
        assert_eq!(coverage, expected);
    }

    /// The stage 1 boss room (area 4) with the boss's display value `$B2:$B3`
    /// poked to 0, so type `$34` runs `$D274` on its next loop; the bytes it
    /// clears and a projectile are poked live. Alone, its loop misses the FME-7 IRQ.
    fn level_end_scene() -> Option<Scene> {
        let mut scene = Scene::tas(6300)?;
        scene.isolate(&[0x34]);
        let pokes = &mut scene.machine.bus.trace.pokes;
        for (addr, value) in [
            (0x00B2, 0),
            (0x00B3, 0),
            (0x00B8, 0x80),
            (0x0480, 0x5A),
            (0x0465, 0x3C),
            (0x7BD5, 0x01),
        ] {
            pokes.push((0xD450, addr, value));
        }
        Some(scene)
    }

    #[test]
    fn level_end_matches_d274() {
        let Some(mut scene) = level_end_scene() else {
            return;
        };
        let checked = assert_routine(&mut scene, 10, (0xD274, 14), &[0xD29A], |call| {
            engine::level_end(&mut call.state.effects);
        });
        assert_eq!(checked, 1);
    }

    /// `$8A3B`: a finished display value (`$B2:$B3` = 0) stops the music and runs
    /// the level end (`$D274`); every later loop takes the `$046E` = 7 arm.
    #[test]
    fn a_finished_display_value_ends_the_level() {
        let Some(scene) = level_end_scene() else {
            return;
        };
        let mut oracle = scene.clone();
        oracle.run(2, &[]);
        assert_eq!(
            oracle.machine.bus.ram[0x46E], 7,
            "the level end did not run"
        );
        let coverage = assert_actor_phases(scene, 10, &[0x34]);
        assert_eq!(
            (coverage.frames, coverage.occupied),
            (10, 10),
            "{coverage:?}"
        );
    }
}
