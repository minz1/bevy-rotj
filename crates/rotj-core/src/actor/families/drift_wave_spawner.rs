//! Type `$C2`, the area-20:00 `$C4` wave spawner (bank 12 `$97B6`).

use crate::actor::engine::{self, Work};
use crate::actor::slots::{Body, contact};
use crate::actor::world::Cx;
use crate::actors::STATE_UNTARGETED;

/// The type the handler hands to the highest free main slot.
const CHILD: u8 = 0xC4;
/// Frames between two spawns (`$7E10` reload at `$9868`).
const SPAWN_DELAY: u8 = 0x08;
/// Spawns before the handler frees its own slot (`$7DF0` checked at `$9873`).
const SPAWN_LIMIT: u8 = 0x10;

/// Type `$C2`'s tables (bank 12), by its spawn record's `subtype` (`0`-`3`).
pub struct Tables {
    /// The player x page the wait ends at, by `subtype` (`$9883`).
    pub wait_x_pages: [u8; 4],
    /// The child's thrown `(vx_drift, vy)`, by `subtype` (`$9887`).
    pub velocities: [(i16, i16); 4],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        let bytes: [u8; 16] = engine::rom_bytes(prg, 12, 0x9887);
        Tables {
            wait_x_pages: engine::rom_bytes(prg, 12, 0x9883),
            velocities: std::array::from_fn(|i| {
                (
                    i16::from_be_bytes([bytes[4 * i + 1], bytes[4 * i]]),
                    i16::from_be_bytes([bytes[4 * i + 3], bytes[4 * i + 2]]),
                )
            }),
        }
    }
}

phases! {
    /// The spawner's phase (`$7DD0`): the game stores its doubled index into `$987F`.
    Phase {
        /// Waiting for the player to reach its x page.
        Waiting = 0,
        /// Spawning a `$C4` every few frames.
        Spawning = 2,
    }
}

/// Type `$C2` keeps its phase, timer and counter in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$97B6`, phases through the `$987F` table).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, _work: &mut Work, cx: &mut Cx<'_>) {
        match Phase::from_byte(body.phase) {
            Some(Phase::Waiting) => {
                let wait =
                    cx.prg.families.drift_wave_spawner.wait_x_pages[usize::from(body.subtype)];
                if wait == cx.effects.player.x.to_be_bytes()[0] {
                    body.phase = Phase::Spawning.byte();
                }
            }
            Some(Phase::Spawning) => {
                body.counter = body.counter.wrapping_sub(1);
                if body.counter & 0x80 == 0 {
                    return;
                }
                if let Some(index) = (0..16).rev().find(|&y| cx.slots.main[y].kind == 0) {
                    let (x, y, subtype) = (body.x, body.y, body.subtype);
                    let (drift, velocity) =
                        cx.prg.families.drift_wave_spawner.velocities[usize::from(subtype)];
                    let child = &mut cx.slots.main[index];
                    child.kind = CHILD;
                    child.body.hit = 0;
                    child.body.behaviour = 0;
                    child.body.phase = 0;
                    child.body.timer = 0;
                    child.body.counter = 0;
                    child.body.anim = 0;
                    child.body.ref_vx = 0;
                    child.body.vx = 0;
                    child.body.contact = contact::SHIELDED | contact::SMITE_PROOF;
                    child.body.health = 0x8888;
                    child.body.state = STATE_UNTARGETED;
                    child.body.flags = 1;
                    child.body.chr_slot = 6;
                    child.body.x = x;
                    child.body.y = y;
                    child.body.subtype = subtype;
                    child.body.vx_drift = drift;
                    child.body.vy = velocity;
                }
                body.counter = SPAWN_DELAY;
                body.timer = body.timer.wrapping_add(1);
                if body.timer >= SPAWN_LIMIT {
                    *kind = 0;
                }
            }
            None => panic!("type $C2 phase {} is past the $987F table", body.phase),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};
    use crate::actor::spawn::SpawnTable;

    /// Every `$C2` spawn record's `subtype` indexes the tables (`$9883`, `$9887`).
    #[test]
    fn every_spawned_subtype_is_in_the_tables() {
        let Some(prg) = crate::prg::for_test() else {
            return;
        };
        let tables = &prg.families.drift_wave_spawner;
        let mut checked = 0;
        for (area, room) in crate::rooms::listed_rooms(prg) {
            for record in SpawnTable::import(prg, area, room).records.iter().flatten() {
                if record.kind == 0xC2 {
                    assert!(
                        usize::from(record.subtype) < tables.wait_x_pages.len(),
                        "area {area} room {room}"
                    );
                    assert!(
                        usize::from(record.subtype) < tables.velocities.len(),
                        "area {area} room {room}"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 0, "no $C2 records were checked");
    }

    /// Area 20:00. The runtime spawner hands the `$C2` controller its
    /// highest-free slots; the `$C4` children it spawns (slots 13-15) are freed
    /// by a `$D450` poke at the loop head each frame. The three `$C2` slots
    /// (0/1/4) can each spawn one a frame, so all three are cleared: no child
    /// ever runs and the diff is the controller's writes.
    ///
    /// Phases: `$7DD0` 0 while it waits for the player's x page (`$9883[0]`),
    /// then 2 while it spawns a child every 9 frames. It never draws (the `$C4`
    /// child is the drawn actor), so `drawn` is 0.
    #[test]
    fn matches_the_oracle() {
        let Some(mut scene) = Scene::tas(38180) else {
            return;
        };
        for addr in [0x7CCD, 0x7CCE, 0x7CCF] {
            scene.machine.bus.trace.pokes.push((0xD450, addr, 0));
        }
        let coverage = assert_actor_phases(scene, 440, &[0xC2]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 440,
                occupied: 440,
                drawn: 0,
                phases: BTreeMap::from([(0, 405), (2, 136)]),
            }
        );
    }
}
