//! Type `$66` (bank 0 `$8665`): the two-piece sprite type `$68` spawns, bobbing
//! vertically between `-$20` and `+$20`.
//!
//! It calls only routines already in `docs/ACTORS.md` §9c: `$A000` (`load`),
//! `$A56E` (`dying`), `$A79B` (`integrate`), `$A852` (`on_screen`), `$AC06`
//! (`tile_bits`), `$C247` (`draw`), `$A9C4` (`contact`) and `$A934` (`store`).
//! `$84CA` maps bank 9 at `$A000` around the draw (the port passes bank 9 to
//! `engine::draw`), and the handler ends a frame with `$A934`, never `$A8E4`, so
//! its velocity is the bob, not gravity.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The metasprite pointer the handler copies to `$96:$97` (bank 0 `$86F3`).
const METASPRITE_PTR: u16 = 0x86F3;
/// The bank `$84CA` maps at `$A000` for the draw.
const DRAW_BANK: u8 = 9;
/// The hit box the contact uses (`$86DF`).
const HITBOX: u8 = 0x0C;
/// Contact damage (`$86E3`).
const DAMAGE: u8 = 4;
/// The bob's negative clamp (`$8696`).
const DOWN_CLAMP: u8 = 0xE0;
/// The bob's positive clamp (`$86B8`).
const UP_CLAMP: u8 = 0x20;

/// `$7DD0` bit 7: it has been drawn on screen, so leaving the screen frees it.
const DRAWN: u8 = 0x80;
/// Type `$66` keeps its whole state in [`Body`] (`$7DD0`, `$7D90`, `$7E70:$7E80`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8665`; the `$7DD0` phase is 0 or `$80`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if engine::dying(kind, body, work, 0, cx) {
            return;
        }
        work.attr |= body.flags;
        bob(body);
        engine::integrate(body, work);
        if engine::on_screen(work, cx.world) != Visibility::On {
            // `$86F0` -> `$8908`: free a slot that has been drawn, else just store.
            if body.phase & DRAWN != 0 {
                *kind = 0;
            } else {
                engine::store(body, work);
            }
            return;
        }
        body.phase = DRAWN;
        work.pointer = engine::rom_word(cx.prg, 0, METASPRITE_PTR);
        engine::tile_bits(work, cx.prg);
        engine::draw(work, DRAW_BANK, cx);
        work.hitbox = HITBOX;
        engine::contact(kind, body, work, DAMAGE, cx);
        engine::store(body, work);
    }
}

/// `$8679`: `$7D90` bit 7 picks the direction; `vy` (`$7E70:$7E80`) steps one
/// unit a frame, flipping `$7D90` at `-$20`/`+$20`.
fn bob(body: &mut Body) {
    let [hi, lo] = body.vy.to_be_bytes();
    if body.behaviour & 0x80 == 0 {
        let (lo, borrow) = lo.overflowing_sub(1);
        let hi = hi.wrapping_sub(u8::from(borrow));
        body.vy = i16::from_be_bytes([hi, lo]);
        if hi & 0x80 != 0 && lo < DOWN_CLAMP {
            body.vy = i16::from_be_bytes([hi, DOWN_CLAMP]);
            body.behaviour = DOWN_CLAMP;
        }
    } else {
        let (lo, carry) = lo.overflowing_add(1);
        let hi = hi.wrapping_add(u8::from(carry));
        body.vy = i16::from_be_bytes([hi, lo]);
        if hi & 0x80 == 0 && lo >= UP_CLAMP {
            body.vy = i16::from_be_bytes([hi, UP_CLAMP]);
            body.behaviour = UP_CLAMP;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// The `$66` spawned in level 3 (area 8): one phase-0 frame at its spawn, then
    /// it is drawn (`$80`) until it leaves the screen and frees.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(9032) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 200, &[0x66]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 154,
                occupied: 113,
                drawn: 112,
                phases: BTreeMap::from([(0, 1), (0x80, 112)]),
            },
            "{coverage:?}"
        );
    }

    /// `$8908`'s `bpl`: a slot that has not been drawn stores instead of freeing.
    #[test]
    fn off_screen_with_a_clear_phase_is_stored() {
        let Some(mut scene) = Scene::tas(9042) else {
            return;
        };
        let wram = &mut scene.machine.bus.wram;
        let slot = (0..16)
            .find(|&slot| wram[0x1CC0 + slot] == 0x66)
            .expect("a live $66");
        let camera = u16::from_be_bytes([scene.machine.bus.ram[0xCC], scene.machine.bus.ram[0xCD]]);
        let [hi, lo] = camera.wrapping_add(0x4000).to_be_bytes();
        (wram[0x1B00 + slot], wram[0x1B20 + slot]) = (hi, lo);
        wram[0x1DD0 + slot] = 0;
        let coverage = assert_actor_phases(scene, 30, &[0x66]);
        assert_eq!(coverage.drawn, 0, "{coverage:?}");
        assert_eq!(coverage.occupied, coverage.frames, "{coverage:?}");
        assert_eq!(
            coverage.phases,
            BTreeMap::from([(0, coverage.frames)]),
            "{coverage:?}"
        );
    }

    /// The handler's `$A9C4` contact costs the player energy up close.
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(9042) else {
            return;
        };
        scene.isolate(&[0x66]);
        let slot = (0..16)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x66)
            .expect("a live $66");
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.script(200, |_| 0);
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
        let coverage = assert_actor_phases(scene, 40, &[0x66]);
        assert_eq!(coverage.occupied, coverage.frames, "{coverage:?}");
        assert!(coverage.frames > 0, "{coverage:?}");
    }
}
