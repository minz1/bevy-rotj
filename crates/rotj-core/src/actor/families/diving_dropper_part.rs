//! Type `$74` (bank 0 `$8C9C`): a sprite that slides along y until its offset
//! crosses zero, then holds; `$7DD0` bit 7 picks the direction.
//!
//! It calls only routines in `docs/ACTORS.md` §9c: `$A000` (`load`), `$A852`
//! (`on_screen`), `$AC06` (`tile_bits`), `$C247` (`draw`, bank 9 as `$84CA` maps
//! it), `$AEE3` (`touch`) and `$A934` (`store`).

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

const METASPRITE_PTR: u16 = 0xA0FA;
const DRAW_BANK: u8 = 9;
const ATTR: u8 = 0x41;
const HITBOX: u8 = 3;
const DAMAGE: u8 = 4;
const STEP: u8 = 3;
const SHIFT_X: u16 = 0xFFD8;

/// Type `$74` keeps its direction, offset and hold flag in [`Body`] (`$7DD0`, `$7DF0:$7E10`, `$7E90`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8C9C`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if body.anim & 0x80 != 0 {
            work.vy = 0;
        } else {
            let falling = body.phase & 0x80 == 0;
            let (timer, carry) = if falling {
                body.timer.overflowing_add(STEP)
            } else {
                body.timer.overflowing_sub(STEP)
            };
            body.timer = timer;
            body.counter = if falling {
                body.counter.wrapping_add(u8::from(carry))
            } else {
                body.counter.wrapping_sub(u8::from(carry))
            };
            work.vy = i16::from_be_bytes([body.counter, body.timer]);
            if (body.counter & 0x80 != 0) != falling {
                body.timer = 0;
                body.counter = 0;
                body.anim = 0x80;
            }
        }
        work.x = work.x.wrapping_add(SHIFT_X);
        work.y = work.y.wrapping_add_signed(work.vy);
        if engine::on_screen(work, cx.world) != Visibility::On {
            *kind = 0;
            return;
        }
        work.pointer = METASPRITE_PTR;
        work.attr = ATTR;
        engine::tile_bits(work, cx.prg);
        engine::draw(work, DRAW_BANK, cx);
        work.hitbox = HITBOX;
        engine::touch(work, DAMAGE, cx);
        engine::store(body, work);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(10320) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 220, &[0x74]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 211,
                occupied: 52,
                drawn: 51,
                phases: BTreeMap::from([(0, 82), (0x80, 104)]),
            },
            "{coverage:?}"
        );
    }

    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(10320) else {
            return;
        };
        scene.isolate(&[0x74]);
        let slot = (16..24)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x74)
            .expect("a live $74");
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
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
        let coverage = assert_actor_phases(scene, 40, &[0x74]);
        assert_eq!(coverage.occupied, coverage.frames, "{coverage:?}");
    }
}
