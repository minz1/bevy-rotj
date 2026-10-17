//! Type `$6A`, a stage 3/9 enemy (bank 0 `$877D`). It is spawned as a copy by
//! the bank 0 `$8762` routine the `$68` handler falls into (`$7CC0 = $7DB0`).
//!
//! It calls only routines already in `docs/ACTORS.md` §9c: `$A000` (`load`),
//! `$A56E` (`dying`), `$A79B` (`integrate`), `$A852` (`on_screen`), `$ABDF`
//! (`player_dx`), `$AC06` (`tile_bits`), `$A9C4` (`contact`) and `$A934`
//! (`store`). `$84CA` maps bank 9 at `$A000` around the draw (the port passes
//! bank 9 to `engine::draw`), and the handler ends a frame with `$A934`, never
//! `$A8E4`, so it keeps its velocity instead of falling.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The metasprite pointer the handler copies to `$96:$97` (bank 0 `$87F3`).
const METASPRITE_PTR: u16 = 0x87F3;
/// The bank `$84CA` maps at `$A000` for the draw.
const DRAW_BANK: u8 = 9;
/// The hit box the contact uses (`$87DF`).
const HITBOX: u8 = 0x0C;
/// Contact damage (`$87E3`).
const DAMAGE: u8 = 4;
/// The `$7E50:$7E60` drift the knock-back writes (`$87C5`).
const DRIFT: i16 = 0x48;
/// The `|dx|` high byte that arms the knock-back (`$87A9`).
const TRIGGER_HI: u8 = 0x02;
/// `$7DF0` once the knock-back has fired (`$87AD`).
const TRIGGERED: u8 = 0x80;

/// Type `$6A`'s tables (bank 0).
pub struct Tables {
    /// The knock-back vy, by `behaviour`, any byte (`$87F5`/`$87F7`).
    pub knockback: [i16; 256],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        Tables {
            knockback: engine::rom_split_words(prg, 0, 0x87F5, 0x87F7).map(u16::cast_signed),
        }
    }
}

/// Type `$6A` keeps its whole state in [`Body`]. `$7DD0` is only an "it was on
/// screen" flag (`$80`), read back by the shared off-screen tail at `$8908`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$877D`; the `$7DD0` phase is 0 or `$80`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if engine::dying(kind, body, work, 0, cx) {
            return;
        }
        work.attr |= body.flags;
        engine::integrate(body, work);
        if engine::on_screen(work, cx.world) != Visibility::On {
            // `$87F0` -> `$8908`: free a slot that has been drawn, else store.
            if body.phase & engine::DRAWN != 0 {
                *kind = 0;
            } else {
                engine::store(body, work);
            }
            return;
        }
        // `$8799`: once, when the player is 32-47 px to its right, hand it the
        // `$87F5`/`$87F7` knock-back velocity and a `$7E50:$7E60` drift.
        if body.timer & 0x80 == 0 {
            let dx = engine::player_dx(work, &cx.effects.player);
            if dx < 0 && dx.unsigned_abs() >> 8 == u16::from(TRIGGER_HI) {
                body.timer = TRIGGERED;
                body.vx_drift = DRIFT;
                body.vy = cx.prg.families.lunging_enemy.knockback[usize::from(body.behaviour)];
            }
        }
        body.phase = engine::DRAWN;
        work.pointer = engine::rom_word(cx.prg, 0, METASPRITE_PTR);
        engine::tile_bits(work, cx.prg);
        engine::draw(work, DRAW_BANK, cx);
        work.hitbox = HITBOX;
        engine::contact(kind, body, work, DAMAGE, cx);
        engine::store(body, work);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    fn slot_of(scene: &Scene) -> usize {
        (0..16)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x6A)
            .expect("a live $6A")
    }

    /// The stage 3 `$68` parent (subtype `$6A`) spawns this copy at frame 9263:
    /// phase 0 on its first loop, `$80` while drawn, then the shared `$8908`
    /// tail frees it once it leaves the view. The 120 loops also run its
    /// `$A9C4` contact (one player hit) and the `$AA57` weapon scan (one hit).
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(9263) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 120, &[0x6A]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 65,
                occupied: 54,
                drawn: 54,
                phases: BTreeMap::from([(0, 1), (0x80, 53)]),
            },
            "{coverage:?}"
        );
    }

    /// `$8908`'s `bpl`: a slot that has not been drawn stores instead of freeing.
    #[test]
    fn off_screen_before_the_first_draw_is_stored() {
        let Some(mut scene) = Scene::tas(9263) else {
            return;
        };
        scene.isolate(&[0x6A]);
        let slot = slot_of(&scene);
        let camera = u16::from_be_bytes([scene.machine.bus.ram[0xCC], scene.machine.bus.ram[0xCD]]);
        let [hi, lo] = camera.wrapping_add(0x4000).to_be_bytes();
        let wram = &mut scene.machine.bus.wram;
        (wram[0x1B00 + slot], wram[0x1B20 + slot]) = (hi, lo);
        wram[0x1DD0 + slot] = 0;
        let coverage = assert_actor_phases(scene, 30, &[0x6A]);
        assert_eq!(coverage.drawn, 0, "{coverage:?}");
        assert_eq!(coverage.occupied, coverage.frames, "{coverage:?}");
        assert_eq!(
            coverage.phases,
            BTreeMap::from([(0, coverage.frames)]),
            "{coverage:?}"
        );
    }

    /// The handler's `$A9C4` contact (`$7EE0` is 0, so the player check runs)
    /// costs the player energy up close.
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(9263) else {
            return;
        };
        scene.isolate(&[0x6A]);
        let slot = slot_of(&scene);
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.script(60, |_| 0);
        scene.player(x, y);
        let mut probe = scene.clone();
        let mut energy = probe.machine.bus.ram[0x478];
        let mut hits = 0;
        for _ in 0..40 {
            probe.run(1, &[]);
            let now = probe.machine.bus.ram[0x478];
            if now < energy {
                hits += 1;
                energy = now;
            }
        }
        assert!(hits > 0, "the oracle takes no contact hit");
        let coverage = assert_actor_phases(scene, 40, &[0x6A]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }

    /// The knock-back (`$8799`) fires when the player is 32-47 px to its right:
    /// `$7DF0` latches `$80`, `$7E50:$7E60` gets `+$48` and `$7E70:$7E80` the
    /// `$87F5`/`$87F7` entry. Its one firing in [`matches_the_oracle`] is diffed
    /// there; this runs the branch directly.
    #[test]
    fn knock_back_fires_when_the_player_is_close() {
        let Some(mut scene) = Scene::tas(9263) else {
            return;
        };
        scene.isolate(&[0x6A]);
        let slot = slot_of(&scene);
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.script(4, |_| 0);
        scene.player(x + 0x0280, y);
        let mut probe = scene.clone();
        let mut fired = false;
        for _ in 0..4 {
            probe.run(1, &[]);
            let wram = &probe.machine.bus.wram;
            if wram[0x1DD0 + slot] == 0x80 && wram[0x1DF0 + slot] == 0x80 {
                fired = true;
                assert_eq!(wram[0x1D90 + slot], 1, "behaviour picks the $87F7 entry");
                assert_eq!(wram[0x1E50 + slot], 0x00, "drift hi");
                assert_eq!(wram[0x1E60 + slot], 0x48, "drift lo");
                assert_eq!(wram[0x1E70 + slot], 0x00, "vy hi");
                assert_eq!(wram[0x1E80 + slot], 0x28, "vy lo");
            }
        }
        assert!(fired, "the knock-back never fired in the oracle");
        let coverage = assert_actor_phases(scene, 4, &[0x6A]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }
}
