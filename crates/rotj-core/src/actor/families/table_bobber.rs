//! Type `$64` (bank 0 `$8606`): a still two-piece sprite that bobs on screen by
//! the `$83CC` table, indexed by a timer that steps 4 a frame and wraps at `$80`.
//!
//! It calls only routines already in `docs/ACTORS.md` §9c: `$A000` (`load`),
//! `$A79B` (`integrate`), `$A852` (`on_screen`), `$AC06` (`tile_bits`), `$C247`
//! (`draw`, through `$84CA`'s bank 9 map), `$A9C4` (`contact`) and `$A934`
//! (`store`). Its velocity is zero and it never uses gravity.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The metasprite pointer the handler copies to `$96:$97` (bank 0 `$8663`).
const METASPRITE_PTR: u16 = 0x8663;
/// The bank `$84CA` maps at `$A000` for the draw.
const DRAW_BANK: u8 = 9;
/// The y offset table the timer indexes (`$83CC`).
const OFFSETS: u16 = 0x83CC;
/// The hit box the contact uses.
const HITBOX: u8 = 0x0C;
/// Contact damage.
const DAMAGE: u8 = 4;

/// `$7DD0` bit 7: it has been drawn on screen, so leaving the screen frees it.
const DRAWN: u8 = 0x80;
/// Type `$64` keeps its whole state in [`Body`] (`$7DD0`, `$7DF0`, `$7E70:$7E80`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8606`; the `$7DD0` phase is 0 or `$80`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        body.vy = 0;
        work.attr = 1;
        body.timer = body.timer.wrapping_add(4);
        if body.timer & 0x80 != 0 {
            body.timer = 0;
        }
        engine::integrate(body, work);
        if engine::on_screen(work, cx.world) != Visibility::On {
            // `$8660` -> `$8908`: free a slot that has been drawn, else just store.
            if body.phase & DRAWN != 0 {
                *kind = 0;
            } else {
                engine::store(body, work);
            }
            return;
        }
        body.phase = DRAWN;
        work.pointer = engine::rom_word(cx.prg, 0, METASPRITE_PTR);
        work.screen_y = work.screen_y.wrapping_add(engine::rom_byte(
            cx.prg,
            0,
            OFFSETS + u16::from(body.timer),
        ));
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

    fn first_slot(scene: &Scene) -> usize {
        (0..16)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x64)
            .expect("a live $64")
    }

    /// Two `$64` on screen in level 3 (area 8), drawn every frame with the timer
    /// wrapping, until they leave the screen and free.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(9581) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 300, &[0x64]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 113,
                occupied: 113,
                drawn: 113,
                phases: BTreeMap::from([(0x80, 194)]),
            },
            "{coverage:?}"
        );
    }

    /// `$8908`'s `bpl`: a slot that has not been drawn stores instead of freeing.
    #[test]
    fn off_screen_with_a_clear_phase_is_stored() {
        let Some(mut scene) = Scene::tas(9581) else {
            return;
        };
        let slot = first_slot(&scene);
        let wram = &mut scene.machine.bus.wram;
        let camera = u16::from_be_bytes([scene.machine.bus.ram[0xCC], scene.machine.bus.ram[0xCD]]);
        let [hi, lo] = camera.wrapping_add(0x4000).to_be_bytes();
        (wram[0x1B00 + slot], wram[0x1B20 + slot]) = (hi, lo);
        wram[0x1DD0 + slot] = 0;
        let coverage = assert_actor_phases(scene, 30, &[0x64]);
        assert_eq!(coverage.drawn, 0, "{coverage:?}");
        assert!(coverage.occupied > 0, "{coverage:?}");
        assert_eq!(
            coverage.phases,
            BTreeMap::from([(0, coverage.occupied)]),
            "{coverage:?}"
        );
    }

    /// The `$A9C4` contact costs the player energy up close.
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(9581) else {
            return;
        };
        scene.isolate(&[0x64]);
        let slot = first_slot(&scene);
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.script(200, |_| 0);
        scene.player(x, y + 0x400);
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
        let coverage = assert_actor_phases(scene, 40, &[0x64]);
        assert_eq!(coverage.occupied, coverage.frames, "{coverage:?}");
        assert!(coverage.frames > 0, "{coverage:?}");
    }
}
