//! Type `$8C` (bank 0 `$98A3`): the child type `$8A` spits, living in slots 16-23.
//!
//! It calls only routines in `docs/ACTORS.md` §9c: `$A000` (`load`), `$A852`
//! (`on_screen`), `$AC06` (`tile_bits`), `$C247` (`draw`, through `$84CA`'s bank
//! 9 mapping), `$AEE3` (`touch`) and `$A934` (`store`). It never calls `$A79B`:
//! the `$7DF0:$7DD0` pair is its own y velocity, and the handler integrates with
//! it itself.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The per-subtype x offset (`$FFxx`), bank 0 `$9932`.
const OFFSETS: u16 = 0x9932;
/// The two metasprites, picked by `$F1` bit 2 (bank 0 `$9936`).
const METASPRITES: u16 = 0x9936;
/// The bank `$84CA` maps at `$A000` for the draw.
const DRAW_BANK: u8 = 9;
/// The y velocity's step per frame while `$7D90` bit 7 is set (`$98BA`).
const GRAVITY: i16 = -8;
/// Its floor (`$98D0`); below it the pair is forced back up.
const FLOOR: i16 = -64;
/// The fixed x offset index when `$7D90` bit 7 is clear (`$98B2`).
const STRAIGHT: u8 = 1;
/// Contact damage (`$9919`).
const DAMAGE: u8 = 2;

/// Type `$8C` keeps everything in [`Body`]: `$7DA0` behaviour, `$7DC0` subtype
/// and `$7DF0:$7DD0` the y velocity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$98A3`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        work.attr = body.flags & 0xDF;
        let subtype = if body.behaviour & 0x80 == 0 {
            STRAIGHT
        } else {
            body.subtype
        };
        if body.behaviour & 0x80 != 0 {
            let mut vy = i16::from_le_bytes([body.phase, body.timer]).wrapping_add(GRAVITY);
            if vy < 0 && vy.to_le_bytes()[0] < 0xC0 {
                vy = FLOOR;
            }
            [body.phase, body.timer] = vy.to_le_bytes();
        }
        let offset = engine::rom_byte(cx.prg, 0, OFFSETS + u16::from(subtype));
        let [x_hi, x_lo] = work.x.to_be_bytes();
        let (x_lo, carry) = x_lo.overflowing_add(offset);
        work.x = u16::from_be_bytes([x_hi.wrapping_add(0xFF).wrapping_add(u8::from(carry)), x_lo]);
        let vy = i16::from_le_bytes([body.phase, body.timer]);
        work.y = work.y.wrapping_add(vy.cast_unsigned());
        let frame = u16::from((cx.world.frame & 0x04) >> 1);
        work.pointer = engine::rom_word(cx.prg, 0, METASPRITES + frame);
        if engine::on_screen(work, cx.world) != Visibility::On {
            *kind = 0;
            return;
        }
        engine::tile_bits(work, cx.prg);
        engine::draw(work, DRAW_BANK, cx);
        work.hitbox = 0;
        if engine::touch(work, DAMAGE, cx) {
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

    /// Area `0A:00` (level 4): the `$8A` parent of slots 8-9 spits `$8C` children.
    /// With the parent kept the children keep coming, so a straight child
    /// (behaviour bit 7 clear, phase 0) and a swinging one (bit 7 set, the
    /// `$7DD0` countdown `$60` -> `$00` -> `$F8` -> its `$C0` floor) both run,
    /// drawn until they leave the screen. Phases 0 and 1 are also the `$8A`'s.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(12099) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 360, &[0x8A, 0x8C]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 360,
                occupied: 101,
                drawn: 100,
                phases: BTreeMap::from([
                    (0, 47),
                    (1, 39),
                    (8, 1),
                    (16, 1),
                    (24, 1),
                    (32, 1),
                    (40, 1),
                    (48, 1),
                    (56, 1),
                    (64, 1),
                    (72, 1),
                    (80, 1),
                    (88, 1),
                    (96, 1),
                    (192, 28),
                    (200, 1),
                    (208, 1),
                    (216, 1),
                    (224, 1),
                    (232, 1),
                    (240, 1),
                    (248, 1),
                ]),
            },
            "{coverage:?}"
        );
    }

    /// A lone swinging child (area `0A:00`, slot 23, subtype 3): the whole
    /// `$7DD0` countdown `$A0` -> `$00` -> `$F8` -> the `$C0` floor, held there
    /// for 55 frames, then the off-screen free (`drawn` one below `occupied`).
    #[test]
    fn matches_the_oracle_while_it_swings() {
        let Some(scene) = Scene::tas(13011) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 120, &[0x8C]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 120,
                occupied: 83,
                drawn: 82,
                phases: BTreeMap::from([
                    (0, 1),
                    (8, 1),
                    (16, 1),
                    (24, 1),
                    (32, 1),
                    (40, 1),
                    (48, 1),
                    (56, 1),
                    (64, 1),
                    (72, 1),
                    (80, 1),
                    (88, 1),
                    (96, 1),
                    (104, 1),
                    (112, 1),
                    (120, 1),
                    (128, 1),
                    (136, 1),
                    (144, 1),
                    (152, 1),
                    (160, 1),
                    (192, 55),
                    (200, 1),
                    (208, 1),
                    (216, 1),
                    (224, 1),
                    (232, 1),
                    (240, 1),
                    (248, 1),
                ]),
            },
            "{coverage:?}"
        );
    }

    /// Every `$9932` x-offset entry runs: the subtype of the swinging slot-23
    /// child is forced each loop, and the oracle takes the same byte.
    #[test]
    fn every_x_offset_matches_the_oracle() {
        for subtype in 0..=3u8 {
            let Some(mut scene) = Scene::tas(13011) else {
                return;
            };
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, 0x7DC7, subtype));
            let coverage = assert_actor_phases(scene, 40, &[0x8C]);
            assert!(
                coverage.occupied > 0 && coverage.drawn > 0,
                "subtype {subtype}: {coverage:?}"
            );
        }
    }

    /// `$AEE3` costs the player energy when he stands in the child's way. The
    /// whole-game TAS never takes this hit (`docs/ACTORS.md` §8b's contact 0).
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(12783) else {
            return;
        };
        scene.isolate(&[0x8C]);
        let Some(slot) = (0..24).find(|&s| scene.machine.bus.wram[0x1CC0 + s] == 0x8C) else {
            return;
        };
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
        let coverage = assert_actor_phases(scene, 40, &[0x8C]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }
}
