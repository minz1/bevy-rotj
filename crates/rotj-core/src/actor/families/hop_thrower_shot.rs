//! Type `$40` (bank 11 `$9410`): a short-lived sprite that sits 64 px to the
//! side of its slot and cycles through three metasprites.
//!
//! It calls only routines in `docs/ACTORS.md` §9c: `$A000` (`load`), `$A852`
//! (`on_screen`), `$AC06` (`tile_bits`), `$C247` (`draw`, bank 9 as `$826F` maps
//! it), `$AEE3` (`touch`) and `$A934` (`store`).

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

const HOME_BANK: u8 = 11;
const DRAW_BANK: u8 = 9;
const OFFSET_LOW: u16 = 0x9470;
const OFFSET_HIGH: u16 = 0x9472;
const METASPRITES: u16 = 0x9474;
const HITBOX: u8 = 0;
const DAMAGE: u8 = 2;
const LIFETIME: u8 = 0x18;

/// The shot's tables (bank 11), by `phase` (0 or 1).
pub struct Tables {
    /// The x offset added each frame, by `phase` (`$9470`, `$9472`).
    pub side_offset: [u16; 2],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        Tables {
            side_offset: engine::rom_split_words(prg, HOME_BANK, OFFSET_HIGH, OFFSET_LOW),
        }
    }
}

/// Type `$40` keeps its animation timer in [`Body`] (`$7DF0`) and its side in `$7DD0`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9410`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        work.attr = body.flags;
        let offset = cx.prg.families.hop_thrower_shot.side_offset[usize::from(body.phase)];
        work.x = work.x.wrapping_add(offset);
        let frame = u16::from((body.timer & 0x18) >> 2);
        work.pointer = engine::rom_word(cx.prg, HOME_BANK, METASPRITES + frame);
        if engine::on_screen(work, cx.world) != Visibility::On {
            *kind = 0;
            return;
        }
        engine::tile_bits(work, cx.prg);
        engine::draw(work, DRAW_BANK, cx);
        work.hitbox = HITBOX;
        engine::touch(work, DAMAGE, cx);
        body.timer = body.timer.wrapping_add(1);
        if body.timer < LIFETIME {
            engine::store(body, work);
        } else {
            *kind = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    fn live_slot(scene: &Scene) -> usize {
        (0..24)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x40)
            .expect("a live $40")
    }

    /// Area 10:00: the `$40` spawns in the 16-23 array and lives 24 frames, side 1.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(17943) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 600, &[0x40]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 597,
                occupied: 23,
                drawn: 23,
                phases: BTreeMap::from([(1, 23)]),
            },
            "{coverage:?}"
        );
    }

    /// The other side (`$9470` phase 0) is forced by a poke.
    #[test]
    fn the_other_side_matches_the_oracle() {
        let Some(mut scene) = Scene::tas(17943) else {
            return;
        };
        let slot = live_slot(&scene);
        scene.machine.bus.wram[0x1DD0 + slot] = 0;
        let coverage = assert_actor_phases(scene, 60, &[0x40]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 60,
                occupied: 23,
                drawn: 22,
                phases: BTreeMap::from([(0, 23)]),
            },
            "{coverage:?}"
        );
    }

    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(17943) else {
            return;
        };
        scene.isolate(&[0x40]);
        let slot = live_slot(&scene);
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.player(x, y);
        let mut probe = scene.clone();
        let mut energy = probe.machine.bus.ram[0x478];
        let mut hits = 0;
        for _ in 0..20 {
            probe.run(1, &[]);
            let now = probe.machine.bus.ram[0x478];
            if now < energy {
                hits += 1;
                energy = now;
            }
        }
        assert!(hits > 0, "the oracle takes no contact hit");
        let coverage = assert_actor_phases(scene, 20, &[0x40]);
        assert_eq!(coverage.occupied, coverage.frames, "{coverage:?}");
    }
}
