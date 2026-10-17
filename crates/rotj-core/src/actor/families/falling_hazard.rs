//! Type `$A2` (bank 12 `$8412`): a falling sprite the `$A0` controller drops into
//! slots 16-23 (`$84C2`). It descends 4 px a frame, drawn until it leaves the
//! bottom, and damages the player on contact.
//!
//! It calls only routines in `docs/ACTORS.md` §9c: `$A000` (`load`), `$A852`
//! (`on_screen`, off screen it frees its own slot), `$C247` (`draw`, bank 1 as
//! `$81C8` maps it), `$AEE3` (`touch`) and `$A934` (`store`). It never reads
//! `$7DD0`/`$7D50`, so its phase is always the spawn value.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

const HOME_BANK: u8 = 12;
const DRAW_BANK: u8 = 1;
/// The metasprite pointer's low/high bytes (`$8468`/`$8469`).
const POINTER: u16 = 0x8468;
const FALL: u16 = 0x40;
const BOTTOM: u8 = 0xC0;
const HITBOX: u8 = 6;
const DAMAGE: u8 = 2;

/// Type `$A2` keeps no state of its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8412`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        work.attr = body.flags;
        work.y = work.y.wrapping_add(FALL);
        work.pointer = engine::rom_word(cx.prg, HOME_BANK, POINTER);
        if engine::on_screen(work, cx.world) != Visibility::On {
            *kind = 0;
            return;
        }
        if work.screen_y >= BOTTOM {
            work.screen_y = BOTTOM;
        }
        work.tile = 0;
        engine::draw(work, DRAW_BANK, cx);
        work.hitbox = HITBOX;
        engine::touch(work, DAMAGE, cx);
        if work.screen_y >= BOTTOM {
            *kind = 0;
            return;
        }
        engine::store(body, work);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    fn live_slot(scene: &Scene) -> usize {
        (16..24)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0xA2)
            .expect("a live $A2")
    }

    /// Area 1A:02: two `$A2` children fall off the bottom in 28 frames, drawn each.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(32675) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 40, &[0xA2]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 40,
                occupied: 28,
                drawn: 28,
                phases: BTreeMap::from([(0, 56)]),
            },
            "{coverage:?}"
        );
    }

    /// Pushed off screen, the off-screen check frees both slots before any draw.
    #[test]
    fn off_screen_frees_its_slot() {
        let Some(mut scene) = Scene::tas(32675) else {
            return;
        };
        let camera = u16::from_be_bytes([scene.machine.bus.ram[0xCC], scene.machine.bus.ram[0xCD]]);
        let [hi, lo] = camera.wrapping_add(0x4000).to_be_bytes();
        let slots: Vec<usize> = (16..24)
            .filter(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0xA2)
            .collect();
        assert_eq!(slots.len(), 2);
        let pokes = &mut scene.machine.bus.trace.pokes;
        for slot in slots {
            let at = u16::try_from(slot).expect("a slot index");
            pokes.push((0xD450, 0x7B00 + at, hi));
            pokes.push((0xD450, 0x7B20 + at, lo));
        }
        let coverage = assert_actor_phases(scene, 3, &[0xA2]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 3,
                occupied: 1,
                drawn: 0,
                phases: BTreeMap::from([(0, 2)]),
            },
            "{coverage:?}"
        );
    }

    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(32675) else {
            return;
        };
        scene.isolate(&[0xA2]);
        let slot = live_slot(&scene);
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.player(x, y);
        let mut probe = scene.clone();
        let mut energy = probe.machine.bus.ram[0x478];
        let mut hits = 0;
        for _ in 0..30 {
            probe.run(1, &[]);
            let now = probe.machine.bus.ram[0x478];
            if now < energy {
                hits += 1;
                energy = now;
            }
        }
        assert!(hits > 0, "the oracle takes no contact hit");
        let coverage = assert_actor_phases(scene, 30, &[0xA2]);
        assert_eq!(
            (coverage.occupied, coverage.drawn),
            (28, 28),
            "{coverage:?}"
        );
    }
}
