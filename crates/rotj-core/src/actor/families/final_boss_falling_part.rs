//! Type `$BE` (bank 12 `$9603`): a stage-7 boss-area part (`docs/ACTORS.md` §8b,
//! area `22:00`, slots 20-23, spawned by the `$B4` controller's `$8F89` helper).
//!
//! It accelerates downward, steps by its `$7DD0:$7DB0` / `$7E10:$7DF0`
//! velocities, bounces off the `$0B00` floor, draws one of its four `$96A9`
//! metasprites by `$F1` bits 1-2, brushes the player (`$AFD6`, mode `$80`) and
//! frees its own slot as soon as it leaves the screen.
//!
//! It calls only routines in `docs/ACTORS.md` §9c: `$A000` (`load`), `$A852`
//! (`on_screen`), `$A934` (`store`) and `$AFD6` (`boss_contact`). `$81C6` is the
//! bank-12 draw wrapper (it maps bank 9 at `$A000` and restores bank 13), so the
//! draw is `engine::draw(work, 9, cx)`.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::{Body, Metatile};
use crate::actor::world::Cx;

/// The handler's bank, for its metasprite pointer table.
const BANK: u8 = 12;
/// Metasprite pointers, four `$F1 & 6` frames (`$96A9`, 8 bytes).
const METASPRITES: u16 = 0x96A9;
/// The bank `$81C6` maps while it draws (`$C247`).
const DRAW_BANK: u8 = 9;
/// `$AFD6`'s amount (X:A = `$0258`) and mode (Y = `$80`) at the `$968E` call.
const CONTACT: u16 = 0x0258;
const CONTACT_MODE: u8 = 0x80;
/// The y high byte that bounces the part.
const FLOOR: u8 = 0x0B;

/// Type `$BE` keeps its whole state in [`Body`] (`$7DB0`/`$7DD0` are the x
/// velocity's low/high bytes and `$7DF0`/`$7E10` the y velocity's).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9603`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        work.tile = 0;
        work.attr = 0x03;
        // `$960E`: the y velocity gains 3, its low byte clamped to `$80` whenever
        // its sign disagrees with the high byte's.
        let carry = body.timer.overflowing_add(3).1;
        body.timer = body.timer.wrapping_add(3);
        body.counter = body.counter.wrapping_add(u8::from(carry));
        if (body.counter & 0x80 != 0) != (body.timer & 0x80 != 0) {
            body.timer = 0x80;
        }
        // `$9632`: step by `($7DD0:$7DB0)` and `($7E10:$7DF0)`.
        work.x = work
            .x
            .wrapping_add_signed(i16::from_be_bytes([body.phase, body.subtype]));
        work.y = work
            .y
            .wrapping_add_signed(i16::from_be_bytes([body.counter, body.timer]));
        // `$9650`: the `$0B00` floor bounces the part.
        if work.y.row() >= FLOOR {
            work.y = u16::from_be_bytes([FLOOR, 0]);
            let vy = i16::from_be_bytes([body.counter, body.timer]).wrapping_neg();
            [body.counter, body.timer] = vy.to_be_bytes();
        }
        // `$9673`: the frame's metasprite, then project.
        let index = cx.world.frame & 0x06;
        work.pointer = engine::rom_word(cx.prg, BANK, METASPRITES + u16::from(index));
        if engine::on_screen(work, cx.world) != Visibility::On {
            *kind = 0;
            return;
        }
        engine::draw(work, DRAW_BANK, cx);
        work.hitbox = 1;
        engine::boss_contact(CONTACT, CONTACT_MODE, body, work, cx);
        engine::store(body, work);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Area 22:00 (stage 7), the `$B4` controller's `$BE` parts from their first
    /// TAS frame (40532) through the frame they free themselves. The bounce, the
    /// draw, the player brush and the off-screen free are all compared.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(40532) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 120, &[0xBE]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 120,
                occupied: 107,
                drawn: 106,
                phases: BTreeMap::from([(0, 334)]),
            }
        );
    }

    /// The `$AFD6` mode-`$80` touch: pin a part onto the player each loop with
    /// `$0473` clear so he is not mid-attack, and standing in it hurts him.
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(40532) else {
            return;
        };
        scene.isolate(&[0xBE]);
        scene.run(1, &[]);
        let player = &scene.machine.bus.ram[0x460..0x464];
        let (x_hi, x_lo, y_hi, y_lo) = (player[0], player[1], player[2], player[3]);
        let slot = (16..24)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0xBE)
            .expect("a $BE part");
        let at = u16::try_from(slot).expect("slot < 24");
        for (array, value) in [
            (0x7B00, x_hi),
            (0x7B20, x_lo),
            (0x7B40, y_hi),
            (0x7B60, y_lo),
        ] {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, array + at, value));
        }
        scene.machine.bus.trace.pokes.push((0xD450, 0x0473, 0));
        let mut probe = scene.clone();
        let mut hurt = 0u8;
        for _ in 0..90 {
            probe.run(1, &[]);
            hurt = hurt.max(probe.machine.bus.ram[0x479]);
        }
        assert!(hurt > 0, "the oracle never took boss contact");
        let coverage = assert_actor_phases(scene, 90, &[0xBE]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }
}
