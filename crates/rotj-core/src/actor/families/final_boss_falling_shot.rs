//! Type `$BC` (bank 12 `$952E`, area 22:00): a stage-7 boss-area projectile that
//! falls under gravity, drops to row `$0B`, then falls 32 more frames and frees
//! its own slot.
//!
//! It calls only routines in `docs/ACTORS.md` §9c: `$A000` (`load`), `$81C6`
//! (`draw`, which maps bank 9 at `$A000`), `$A852` (`on_screen`), `$AFD6`
//! (`boss_contact`, mode `$80`) and `$A934` (`store`). It never calls `$A79B`
//! (`integrate`) or `$A9C4` (`contact`), setting its position itself from the
//! `$7DB0:$7DD0` x step and the `$7DF0:$7E10` y step.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The handler and its tables live in bank 12.
const HOME_BANK: u8 = 12;
/// The metasprites' low bytes, indexed by `$F1 & $02` (`$95FF`).
const POINTER_LO: u16 = 0x95FF;
/// The metasprites' high bytes, the same index (`$9600`).
const POINTER_HI: u16 = 0x9600;
/// `$81C6` maps bank 9 at `$A000` for `$C247`.
const DRAW_BANK: u8 = 9;
/// Hit box the `$AFD6` contact uses (`$A5` = 1).
const HITBOX: u8 = 1;
/// The boss pool the contact drains (`X:A` = `$0258`).
const CONTACT_AMOUNT: u16 = 0x0258;
/// `$AFD6` mode (`Y` = `$80`): only the player-touch arm runs.
const CONTACT_MODE: u8 = 0x80;
/// The row the fall parks at before counting down (`$92` >= `$0B`).
const FREE_ROW: u8 = 0x0B;
/// Frames of the parked fall before the slot frees (`$7D90` reaches `$20`).
const FALL_FRAMES: u8 = 0x20;

/// Type `$BC` keeps its whole state in [`Body`] (`$7DB0`/`$7DD0` the x step,
/// `$7DF0`/`$7E10` the y step, `$7E90` the fall latch, `$7D90` the countdown).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$952E`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        work.attr = 0;
        work.tile = 0;
        // `$9537`: the 16-bit y step gains 3; its low byte clamps to `$80`
        // whenever its sign bit disagrees with the high byte's.
        let (timer, carry) = body.timer.overflowing_add(3);
        body.timer = timer;
        let counter = body.counter.wrapping_add(u8::from(carry));
        body.counter = counter;
        if (counter ^ body.timer) & 0x80 != 0 {
            body.timer = 0x80;
        }
        // `$9560`: once it is falling the behaviour counts to `$20` and the slot frees.
        if body.anim != 0 {
            body.behaviour = body.behaviour.wrapping_add(1);
            if body.behaviour >= FALL_FRAMES {
                *kind = 0;
                return;
            }
        }
        // `$9572`: the x step is (subtype, phase), the y step (timer, counter).
        work.x = work
            .x
            .wrapping_add(u16::from_be_bytes([body.phase, body.subtype]));
        work.y = work
            .y
            .wrapping_add(u16::from_be_bytes([body.counter, body.timer]));
        if body.anim == 0 && work.y.to_be_bytes()[0] >= FREE_ROW {
            // `$959B`: it crossed row `$0B`, so it parks, falls 32 frames, and
            // takes the RNG's x direction and subtype.
            body.anim = 1;
            work.y = u16::from_be_bytes([FREE_ROW, 0]);
            body.behaviour = 0;
            body.counter = 0xFF;
            body.timer = 0xC0;
            let [seed, noise] = cx.world.rng;
            if seed & 0x80 != 0 {
                body.phase = 0xFF;
                body.subtype = noise | 0xF0;
            } else {
                body.phase = 0;
                body.subtype = noise & 0x0F;
            }
        }
        let index = u16::from(cx.world.frame & 0x02);
        work.pointer = u16::from_le_bytes([
            engine::rom_byte(cx.prg, HOME_BANK, POINTER_LO + index),
            engine::rom_byte(cx.prg, HOME_BANK, POINTER_HI + index),
        ]);
        if engine::on_screen(work, cx.world) == Visibility::On {
            engine::draw(work, DRAW_BANK, cx);
            work.hitbox = HITBOX;
            engine::boss_contact(CONTACT_AMOUNT, CONTACT_MODE, body, work, cx);
        }
        engine::store(body, work);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Area 22:00, the stage-7 boss room: the `$BC` part already alive at frame
    /// 39834 (its `$B4` spawner is unported, so isolation frees it and no more
    /// spawn). Its whole life: 30 loops of the phase-0 fall, then the parked
    /// phase-`$FF` fall and the self-free.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(39834) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 70, &[0xBC]);
        let expected = Coverage {
            frames: 70,
            occupied: 62,
            drawn: 61,
            phases: BTreeMap::from([(0, 30), (0xFF, 32)]),
        };
        assert_eq!(coverage, expected, "{coverage:?}");
    }

    /// Three parts alive at once in slots 18-20 (frame 40045), so the loop runs
    /// their narrow `$7EE0` contact aliases and both RNG branches together.
    #[test]
    fn matches_the_oracle_with_three_alive() {
        let Some(scene) = Scene::tas(40045) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 45, &[0xBC]);
        let expected = Coverage {
            frames: 45,
            occupied: 45,
            drawn: 45,
            phases: BTreeMap::from([(0, 74), (0xFF, 46)]),
        };
        assert_eq!(coverage, expected, "{coverage:?}");
    }

    /// The `$AFD6` contact is real: a `$BC` part pinned onto the player hurts
    /// him, and the loops still diff once the touch is taken. `$0473` is cleared
    /// so `$AFD6` reaches the player check at all.
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(40045) else {
            return;
        };
        scene.isolate(&[0xBC]);
        scene.run(1, &[]);
        let player = (
            u16::from_be_bytes([scene.machine.bus.ram[0x460], scene.machine.bus.ram[0x461]]),
            u16::from_be_bytes([scene.machine.bus.ram[0x462], scene.machine.bus.ram[0x463]]),
        );
        let slot = (16..24)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0xBC)
            .expect("a $BC part");
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
        scene.machine.bus.trace.pokes.push((0xD450, 0x0473, 0));
        let mut probe = scene.clone();
        let mut hurt = 0u8;
        for _ in 0..90 {
            probe.run(1, &[]);
            hurt = hurt.max(probe.machine.bus.ram[0x479]);
        }
        assert!(hurt > 0, "the oracle never took boss contact");
        let coverage = assert_actor_phases(scene, 90, &[0xBC]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }
}
