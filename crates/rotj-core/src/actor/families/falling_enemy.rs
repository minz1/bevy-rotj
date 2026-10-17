//! Type `$AE` (bank 12 `$8A52`): a two-piece enemy that falls and then dies.
//!
//! The handler calls `$A000` (`load`), `$A79B` (`integrate`), `$A852`
//! (`on_screen`), `$A934` (`store`), `$A9C4` (`contact`) and the `$C93B` solid
//! probe. Its draw goes through `$81C6`, which maps bank 9 at `$A000` for
//! `$C247`. It dispatches on `$7DD0` (its doubled index) through the bank-12 table
//! `$8B8C`; the metasprite pointers it draws are the bank-9 words at `$8B82`.
//! It never calls `$A8E4`, so gravity is entirely its own.

use crate::actor::collision::{self, Probe};
use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The `$7C30` -> metasprite pointer table (bank 12, indexed by `$7C30*2`).
const METASPRITES: u16 = 0x8B82;
/// `$81C6` maps bank 9 at `$A000`; those words point into that window.
const DRAW_BANK: u8 = 9;
/// The hit box the contact test uses (`$A5` = `$0C`).
const HITBOX: u8 = 0x0C;
/// Contact damage (`A` = 2).
const DAMAGE: u8 = 2;

phases! {
    /// The enemy's phase (`$7DD0`): the game stores its doubled index into `$8B8C`.
    Phase {
        /// Rising, slowing until it turns to fall.
        Rising = 0,
        /// Falling until it hits a wall or its fall runs out.
        Falling = 2,
        /// Dying: flashing through its death frames, then freed.
        Dying = 4,
    }
}

/// Type `$AE` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8A52`; the `$7DD0` phases are the doubled indices of [`Phase`]).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if body.phase != Phase::Dying.byte() && body.hit & 0x80 != 0 {
            body.hit = 0;
            body.sprite = cx.world.frame & 0x01;
            work.y = work.y.wrapping_sub(0x04C0);
            death(body);
            tail(kind, body, work, cx);
            return;
        }
        work.attr |= body.flags;
        engine::integrate(body, work);
        match Phase::from_byte(body.phase) {
            Some(Phase::Rising) => {
                let vy = body.vy.wrapping_sub(5);
                body.vy = vy;
                if vy < 0 {
                    body.vy = i16::from_be_bytes([vy.to_be_bytes()[0], cx.world.rng[0] | 0xF0]);
                    body.phase = Phase::Falling.byte();
                }
                body.sprite = 1;
            }
            Some(Phase::Falling) => {
                body.sprite = (cx.world.frame & 0x10) >> 4;
                if cx.world.frame_odd {
                    let mut probe = Probe {
                        x: work.x,
                        y: work.y.wrapping_sub(0x80),
                        ..Probe::default()
                    };
                    let left = body.subtype.wrapping_sub(1);
                    body.subtype = left;
                    if left == 0 || collision::solid(&mut probe, 0, cx) & 0x80 != 0 {
                        death(body);
                    }
                }
            }
            Some(Phase::Dying) => {
                body.sprite = ((body.anim & 0x0C) >> 2) + 2;
                body.anim = body.anim.wrapping_add(1);
                if body.anim == 0x0C {
                    *kind = 0;
                    return;
                }
            }
            None => panic!("type $AE phase {} is past the $8B8C table", body.phase),
        }
        tail(kind, body, work, cx);
    }
}

/// `$8B69`: enter the death phase and clear the velocities and the timer.
fn death(body: &mut Body) {
    body.phase = Phase::Dying.byte();
    body.vy = 0;
    body.vx_drift = 0;
    body.anim = 0;
}

/// The shared tail from `$8B11`: project, draw, contact and store.
fn tail(kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    if engine::on_screen(work, cx.world) != Visibility::On {
        *kind = 0;
        return;
    }
    if !cx.world.frame_odd {
        work.pointer = engine::rom_word(cx.prg, 12, METASPRITES + 2 * u16::from(body.sprite));
        work.tile = 0;
        work.attr = 3;
        engine::draw(work, DRAW_BANK, cx);
        if body.phase != Phase::Dying.byte() {
            let energy = cx.effects.player.energy;
            work.hitbox = HITBOX;
            engine::contact(kind, body, work, DAMAGE, cx);
            if cx.effects.player.energy != energy {
                death(body);
            }
        }
    }
    engine::store(body, work);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Area 20:02, from the whole-game TAS: the type's own spawner ($B0's
    /// `$8C20`) puts it in slots 7-15 (`docs/ACTORS.md` §8b), so several run at
    /// once and the per-phase counts exceed the loops. All three `$7DD0` phases
    /// (0, 2 and 4) and the draw are exercised. The window stops at 405 frames,
    /// before the room's schedule spawns a type `$C8` at frame 39199 whose
    /// handler is unported.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(38793) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 405, &[0xAE]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 279,
                occupied: 68,
                drawn: 34,
                phases: BTreeMap::from([(0, 105), (2, 79), (4, 96)]),
            }
        );
    }

    /// The `$A9C4` contact: the whole-game TAS never touches a `$AE` (§8b's
    /// contact 1 is the check running, not a hit), so the player is put on the
    /// on-screen one and the oracle must lose energy to the `$08`-contact actor.
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(38793) else {
            return;
        };
        scene.isolate(&[0xAE]);
        let wram = &scene.machine.bus.wram;
        let camera = u16::from_be_bytes([scene.machine.bus.ram[0xCC], scene.machine.bus.ram[0xCD]]);
        let left = camera.wrapping_add(8) & 0xFFF0;
        let slot = (0..24)
            .filter(|&s| wram[0x1CC0 + s] == 0xAE)
            .min_by_key(|&s| {
                let x = u16::from_be_bytes([wram[0x1B00 + s], wram[0x1B20 + s]]);
                x.abs_diff(left)
            })
            .expect("a live $AE");
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.player(x, y);
        scene.script(120, |_| 0);
        let mut probe = scene.clone();
        let mut hits = 0;
        for _ in 0..120 {
            probe.run(1, &[]);
            hits += usize::from(probe.machine.bus.ram[0x479] != 0);
        }
        assert!(hits > 0, "the oracle takes no contact hit");
        let coverage = assert_actor_phases(scene, 120, &[0xAE]);
        assert!(coverage.frames > 0, "{coverage:?}");
        assert!(coverage.occupied > 0, "{coverage:?}");
    }
}
