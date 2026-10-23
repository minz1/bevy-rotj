//! Type `$BA`, a stage-7 boss part (bank 12 `$94BC`): a `$B6` projectile that
//! drifts by a fixed 16-bit velocity, cycles four metasprites and runs the
//! `$AFD6` boss contact, freeing itself below row `$0B`. It calls `$A000`
//! (`load`), `$A852` (`on_screen`), `$81C6` (`draw`, bank 9) and `$AFD6`
//! (`boss_contact`); it never calls `$A79B` (`integrate`) or `$A9C4`.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::{Body, Counter};
use crate::actor::world::Cx;

/// The handler and its tables live in bank 12.
const HOME_BANK: u8 = 12;
/// The four metasprites, indexed by `($7E90 >> 1) & $06` (`$9526`).
const METASPRITES: u16 = 0x9526;
/// `$81C6` maps bank 9 at `$A000` for `$C247`.
const DRAW_BANK: u8 = 9;
/// Hit box the `$AFD6` contact uses (`$A5` = 1).
const HITBOX: u8 = 1;
/// The boss pool the contact drains (`X:A` = `$0258`).
const CONTACT_AMOUNT: u16 = 0x0258;
/// `$AFD6` mode (`Y` = `$80`): only the player-touch arm runs.
const CONTACT_MODE: u8 = 0x80;
/// The row at or below which the part frees itself (`$92` >= `$0B`).
const FREE_ROW: u8 = 0x0B;

/// Type `$BA` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$94BC`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        work.tile = 0;
        work.attr = 0;
        work.x = work
            .x
            .wrapping_add(u16::from_be_bytes([body.phase, body.subtype]));
        work.y = work
            .y
            .wrapping_add(u16::from_be_bytes([body.counter, body.timer]));
        let [row, below] = work.y.to_be_bytes();
        if below & 0x80 != 0 && row >= FREE_ROW {
            *kind = 0;
            return;
        }
        let index = u16::from((body.anim >> 1) & 0x06);
        work.pointer = engine::rom_word(cx.prg, HOME_BANK, METASPRITES + index);
        if engine::on_screen(work, cx.world) != Visibility::On {
            *kind = 0;
            return;
        }
        engine::draw(work, DRAW_BANK, cx);
        work.hitbox = HITBOX;
        engine::boss_contact(CONTACT_AMOUNT, CONTACT_MODE, body, work, cx);
        body.anim.tick_up();
        engine::store(body, work);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Area 22:00, the stage-7 boss room, from the whole-game TAS: the `$BA`
    /// parts the `$B6` spawner drops into slots 21-23; every other kind is
    /// isolated away.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(40013) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 120, &[0xBA]);
        let expected = Coverage {
            frames: 120,
            occupied: 97,
            drawn: 96,
            phases: BTreeMap::from([(0, 244)]),
        };
        assert_eq!(coverage, expected, "{coverage:?}");
    }

    /// The `$AFD6` mode-`$80` touch: the whole-game TAS never contacts a `$BA`
    /// (§8b's contact 0), so a part is pinned onto the player at the actor-loop
    /// entry each frame, with `$0473` cleared so the player's attack does not
    /// gate the hit. The oracle must take the `$AFD6` contact, which sets the
    /// hurt timer (`$0479`).
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(40013) else {
            return;
        };
        scene.isolate(&[0xBA]);
        scene.run(1, &[]);
        let player = (
            u16::from_be_bytes([scene.machine.bus.ram[0x460], scene.machine.bus.ram[0x461]]),
            u16::from_be_bytes([scene.machine.bus.ram[0x462], scene.machine.bus.ram[0x463]]),
        );
        let slot = (16..24)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0xBA)
            .expect("a $BA part");
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
        let coverage = assert_actor_phases(scene, 90, &[0xBA]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }
}
