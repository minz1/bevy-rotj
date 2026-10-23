//! Type `$4C` (bank 11 `$9ADE`): a spawner that releases one `$4E` into the
//! highest free slot each time its `$C0`-frame counter fills, from the side
//! `$7DF0` picks. It never draws or contacts and calls no `docs/ACTORS.md` §9c
//! routine.

use crate::actor::engine::{self, Work};
use crate::actor::families::Brain;
use crate::actor::slots::{Body, contact};
use crate::actor::world::Cx;

const KIND: u8 = 0x4C;
const CHILD: u8 = 0x4E;
const BANK: u8 = 11;
const SIDE_X_HIGH: u16 = 0x9B5C;
const SIDE_VX_LOW: u16 = 0x9B5E;
const SIDE_VX_HIGH: u16 = 0x9B60;
const RELOAD: u8 = 0xC0;
const FIRED: u8 = 0x80;
const CHILD_X_LOW: u8 = 0x80;
const CHILD_Y: u16 = 0x0A80;
const CHILD_CONTACT: u8 = contact::SLIDE_HURTS;
const CHILD_HEALTH: u16 = 3;

/// The spawner's tables (bank 11), by the side `timer` toggles (0 or 1).
pub struct Tables {
    /// The released child's x high byte, by `timer` (`$9B5C`).
    pub side_x_high: [u8; 2],
    /// The child's x velocity, by `timer` (`$9B5E`, `$9B60`).
    pub side_vx: [i16; 2],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        Tables {
            side_x_high: engine::rom_bytes(prg, BANK, SIDE_X_HIGH),
            side_vx: engine::rom_split_words(prg, BANK, SIDE_VX_HIGH, SIDE_VX_LOW)
                .map(u16::cast_signed),
        }
    }
}

/// Type `$4C` keeps its whole state in [`Body`] (`$7DD0`, `$7DF0`, `$7E10`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9ADE`; `$7DD0` bit 7 parks it, `$7DF0` picks the side).
    pub fn step(&mut self, _kind: &mut u8, body: &mut Body, _work: &mut Work, cx: &mut Cx<'_>) {
        if body.phase & FIRED != 0 || cx.world.frame_odd {
            return;
        }
        if body.counter < RELOAD {
            body.counter += 1;
            return;
        }
        let Some(child) = cx.slots.free_main(0..16) else {
            body.counter = 0;
            return;
        };
        let parent = own_slot(cx);
        body.phase = FIRED;
        body.timer ^= 1;
        let tables = &cx.prg.families.shell_spawner;
        let side = usize::from(body.timer);
        let x = u16::from_be_bytes([tables.side_x_high[side], CHILD_X_LOW]);
        let spawned = cx.slots.main[child].spawn(CHILD, x, CHILD_Y);
        spawned.vy = 0;
        spawned.health = CHILD_HEALTH;
        spawned.counter = 0;
        spawned.hit = 0;
        spawned.state = 0;
        spawned.contact = CHILD_CONTACT;
        spawned.vx_drift = tables.side_vx[side];
        spawned.anim = parent;
    }
}

/// The slot the running `$4C` occupies: the actor loop leaves it a blank body.
fn own_slot(cx: &Cx<'_>) -> u8 {
    let index = cx
        .slots
        .main
        .iter()
        .position(|slot| slot.kind == KIND && slot.brain == Brain::Empty)
        .expect("the running $4C slot");
    u8::try_from(index).expect("slot < 24")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    #[test]
    fn matches_the_oracle() {
        let Some(mut scene) = Scene::tas(26260) else {
            return;
        };
        scene.machine.bus.trace.pokes.push((0xD450, 0x7CCF, 0));
        let coverage = assert_actor_phases(scene, 900, &[0x4C]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 880,
                occupied: 880,
                drawn: 0,
                phases: BTreeMap::from([(0, 386), (0x80, 494)]),
            }
        );
    }

    #[test]
    fn every_slot_busy_resets_the_counter() {
        let Some(mut scene) = Scene::tas(26260) else {
            return;
        };
        for slot in 0..16u16 {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, 0x7CC0 + slot, 0x4C));
        }
        let coverage = assert_actor_phases(scene, 500, &[0x4C]);
        assert_eq!(
            (coverage.occupied, coverage.frames),
            (coverage.frames, coverage.frames)
        );
        assert_eq!(coverage.drawn, 0);
        // 16 slots occupy every compared loop, and >384 even frames pass $C0.
        assert_eq!(coverage.phases, BTreeMap::from([(0, 16 * coverage.frames)]));
        assert!(coverage.frames > 400, "{}", coverage.frames);
    }
}
