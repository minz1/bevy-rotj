//! Type `$A8`, the four parts of the stage 6 boss (bank 12 `$87EC`).
//!
//! The spawner bank 12 `$8737` writes each part into the highest free slot
//! 16-23 (`$AF38`) at the parent's position less one y pixel, with the x
//! velocity from `$87E8`/`$87EA` and the y velocity `$FFC0` (rising). The
//! handler adds both velocities to its position each frame; the y one gains 3 a
//! frame and is clamped at `+$7F`, so the part rises, slows and then falls. It
//! is freed when its y row reaches `$0C` or the projection leaves the screen,
//! and otherwise drawn (bank 9, `$81C6`) and checked against the player
//! (`engine::boss_contact`, mode `$80`: touch only, no weapon scan).

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The bank-12 pointer table for the part's metasprite, indexed by `$F1 & 2`.
const METASPRITES: u16 = 0x886C;
/// `$81C6` maps bank 9 at `$A000` for the draw.
const DRAW_BANK: u8 = 9;
/// The y row (`$92`) the free test compares against (`$8811`).
const FLOOR_ROW: u8 = 0x0C;
/// The y velocity low byte's per-frame step (`$7E10 += 3`).
const STEP: u8 = 3;
/// The amount `$AFD6` drains from the player on touch (`X:A` = `$01F4`).
const CONTACT: u16 = 0x01F4;
/// The `$AFD6` mode (`Y` = `$80`).
const MODE: u8 = 0x80;

/// Type `$A8` keeps its whole state in [`Body`] (`$7DD0`/`$7DF0` the x velocity,
/// `$7E10`/`$7E90` the y velocity); it has none of its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$87EC`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        work.attr = 0;
        work.x = work
            .x
            .wrapping_add(u16::from_be_bytes([body.timer, body.phase]));
        work.y = work
            .y
            .wrapping_add(u16::from_be_bytes([body.anim, body.counter]));
        if work.y.to_be_bytes()[0] >= FLOOR_ROW {
            *kind = 0;
            return;
        }
        let (counter, carry) = body.counter.overflowing_add(STEP);
        body.counter = counter;
        body.anim = body.anim.wrapping_add(u8::from(carry));

        if body.anim & 0x80 == 0 && body.counter & 0x80 != 0 {
            body.counter = 0x7F;
        }
        work.pointer = engine::rom_word(cx.prg, 12, METASPRITES + u16::from(cx.world.frame & 2));
        if engine::on_screen(work, cx.world) != Visibility::On {
            *kind = 0;
            return;
        }
        work.tile = 0;
        engine::draw(work, DRAW_BANK, cx);
        work.hitbox = 1;
        engine::boss_contact(CONTACT, MODE, body, work, cx);
        engine::store(body, work);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Area 1E:00 (the stage 6 boss room), the `$A6` boss's `$A8` parts in slots
    /// 16-23, from frame 36967. `isolate` frees the unported `$A6` parent, so no
    /// new parts spawn and the two alive here live out their 60-frame fall; both
    /// `$7DD0` x-velocity phases (`$10`, `$F0`) run and the part is drawn on 59 of
    /// them (`$A8` is never on screen without a draw).
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(36968) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 120, &[0xA8]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 120,
                occupied: 60,
                drawn: 59,
                phases: BTreeMap::from([(0x10, 60), (0xF0, 60)]),
            }
        );
    }

    /// `$AFD6` with mode `$80`: a part pinned onto the player drains his life. The
    /// oracle is probed first so the poke is known to land the contact.
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(36968) else {
            return;
        };
        scene.isolate(&[0xA8]);
        scene.run(1, &[]);
        let player = (
            u16::from_be_bytes([scene.machine.bus.ram[0x460], scene.machine.bus.ram[0x461]]),
            u16::from_be_bytes([scene.machine.bus.ram[0x462], scene.machine.bus.ram[0x463]]),
        );
        let slot = (16..24)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0xA8)
            .expect("a $A8 part");
        let at = u16::try_from(slot).expect("slot < 24");
        for (addr, value) in [
            (0x7B00, player.0.to_be_bytes()[0]),
            (0x7B20, player.0.to_be_bytes()[1]),
            (0x7B40, player.1.to_be_bytes()[0]),
            (0x7B60, player.1.to_be_bytes()[1]),
        ] {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, addr + at, value));
        }
        let mut probe = scene.clone();
        let mut hurt = 0u8;
        for _ in 0..90 {
            probe.run(1, &[]);
            hurt = hurt.max(probe.machine.bus.ram[0x479]);
        }
        assert!(hurt > 0, "the oracle never took boss contact");
        let coverage = assert_actor_phases(scene, 90, &[0xA8]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }
}
