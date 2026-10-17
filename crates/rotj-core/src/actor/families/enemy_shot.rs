//! Type `$24`, the projectile `$22` throws (bank 8 `$9823`), living in slots 16-23.

use crate::actor::collision::{self, Probe};
use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

const METASPRITES: u16 = 0x98BC;
/// The `$8FC4` wrapper maps bank 1 at `$A000`, where its `$B...` entries live.
const DRAW_BANK: u8 = 1;
/// The `$8000`-window entries (`$98CA`, `$98D0`) stay in the handler's own bank.
const HOME_BANK: u8 = 8;

/// The shot's tables (bank 8), by its direction `phase` (0-9: every spawner's value).
pub struct Tables {
    /// The x step, by direction (`$9894` low, `$989E` high).
    pub step_x: [u16; 10],
    /// The y step, by direction (`$98A8` low, `$98B2` high).
    pub step_y: [u16; 10],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        Tables {
            step_x: engine::rom_split_words(prg, 8, 0x989E, 0x9894),
            step_y: engine::rom_split_words(prg, 8, 0x98B2, 0x98A8),
        }
    }
}

/// Type `$24` keeps its direction (`phase`) and metasprite (`sprite`) in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9823`): fly, vanish on a wall, off screen or a hit.
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        work.attr = body.flags;
        let tables = &cx.prg.families.enemy_shot;
        let direction = usize::from(body.phase);
        work.x = work.x.wrapping_add(tables.step_x[direction]);
        work.y = work.y.wrapping_add(tables.step_y[direction]);
        let mut probe = Probe {
            x: work.x,
            y: work.y,
            ..Probe::default()
        };
        if collision::solid(&mut probe, 0x84, cx) & 0x80 != 0 {
            *kind = 0;
            return;
        }
        let entry = METASPRITES + 2 * u16::from(body.sprite);
        work.pointer = engine::rom_word(cx.prg, 8, entry);
        if engine::on_screen(work, cx.world) != Visibility::On {
            *kind = 0;
            return;
        }
        engine::tile_bits(work, cx.prg);
        // `$C247` reads through `$96/$97`: a `$8000` entry from this handler's own
        // bank, a `$A000` entry from the bank the `$8FC4` wrapper mapped there.
        let bank = if work.pointer < 0xA000 {
            HOME_BANK
        } else {
            DRAW_BANK
        };
        engine::draw(work, bank, cx);
        work.hitbox = 3;
        if engine::touch(work, 2, cx) {
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

    /// Area 20:02's `$AC` (`Scene::tas(38884)`, §8b) with its children left alive,
    /// as the oracle does: every `$24` it drops carries `$7C30` = 6 and so draws
    /// the bank-8 `$98D0` metasprite, and every actor loop is diffed byte for byte.
    #[test]
    fn matches_the_oracle_with_the_ac_children_alive() {
        let Some(scene) = Scene::tas(38884) else {
            return;
        };
        let mut probe = scene.clone();
        let mut sixes = 0;
        for _ in 0..300 {
            probe.run(1, &[]);
            sixes += usize::from((16..24).any(|slot| {
                let wram = &probe.machine.bus.wram;
                wram[0x1CC0 + slot] == 0x24 && wram[0x1C30 + slot] == 6
            }));
        }
        assert!(sixes > 0, "the $AC never dropped a $7C30 = 6 child");
        let coverage = assert_actor_phases(scene, 300, &[0xAC, 0x24]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 300,
                occupied: 300,
                drawn: 300,
                phases: BTreeMap::from([(0, 571), (8, 512)]),
            }
        );
    }

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::route(996) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 60, &[0x24]);
        assert_eq!(
            (coverage.frames, coverage.occupied, coverage.drawn),
            (60, 10, 9),
            "{coverage:?}"
        );
    }

    #[test]
    fn matches_the_oracle_when_it_hits() {
        let Some(mut scene) = Scene::route(997) else {
            return;
        };
        let wram = &scene.machine.bus.wram;
        let slot = (16..24).find(|&s| wram[0x1CC0 + s] == 0x24).expect("a $24");
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.player(x, y);
        let coverage = assert_actor_phases(scene, 20, &[0x24]);
        assert!(coverage.occupied >= 1, "{coverage:?}");
    }
}
