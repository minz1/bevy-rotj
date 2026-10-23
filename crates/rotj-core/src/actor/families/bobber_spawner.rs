//! Type `$68` (bank 0 `$86F5`): a hidden spawner that drops a child of its own
//! `$7DB0` type to the right of the camera, one every 16 frames, five in all.
//! Area 8's `$B288` record points `$7DB0` at `$66`; the other area-8 records
//! (`$B294`, `$B2A0`, `$B318`, `$B324`) point it at `$6A`, whose handler is not
//! ported.
//!
//! It calls no routine from `docs/ACTORS.md` §9c: no `$A000` load, no `$A852`
//! projection, no draw, no contact. It only steps its own timer (`$7DF0`), scans
//! the main slots for a free one, and writes the child's fields (`$7B00-$7EF0`)
//! directly. Its `$7DD0` byte counts the children it has dropped (0-5).

use crate::actor::engine::Work;
use crate::actor::oam::attr;
use crate::actor::slots::{Body, Counter};
use crate::actor::world::Cx;

/// `$7DF0 & $0F`: the frame within the 16-frame spawn period.
const SPAWN_MASK: u8 = 0x0F;
/// `$8702`: the free-slot scan covers the 16 main slots (15 down to 0).
const SLOTS: usize = 16;
/// Child `$7C00`.
const CHILD_FLAGS: u8 = attr::FLIP_X | 0x01;
/// Child velocity drift `$7E50:$7E60` = `$FFF0`.
const CHILD_DRIFT: [u8; 2] = [0xFF, 0xF0];
/// `$8771`: once the parent has dropped this many children it frees its slot.
const SPAWN_LIMIT: u8 = 5;

/// Type `$68` keeps its whole state in [`Body`] (the `$7DF0` timer and the
/// `$7DD0` child counter).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$86F5`; the `$7DD0` phase is 0-5).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, _work: &mut Work, cx: &mut Cx<'_>) {
        body.timer.tick_up();
        if body.timer & SPAWN_MASK != 0 {
            return;
        }
        let Some(child) = cx.slots.free_main(0..SLOTS) else {
            return;
        };
        let x = cx.world.camera_x.wrapping_add(0x1000);
        let child = cx.slots.main[child].spawn(body.subtype, x, body.y);
        child.behaviour = body.behaviour;
        (child.vy, child.contact, child.health) = (0, 0, 0);
        (child.state, child.hit, child.phase) = (0, 0, 0);
        (child.timer, child.counter, child.anim) = (0, 0, 0);
        child.flags = CHILD_FLAGS;
        child.vx_drift = i16::from_be_bytes(CHILD_DRIFT);
        child.chr_slot = body.chr_slot;
        body.phase = body.phase.wrapping_add(1);
        if body.phase >= SPAWN_LIMIT {
            *kind = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};
    use crate::actors::SpawnRecord;

    /// The stage-3 spawner from its first TAS frame. It lives 80 frames and drops
    /// one `$66` per 16, but the FME-7 IRQ (armed in area 8 from ~9046) splits the
    /// actor loop for its later phases, so only phases 0-1 are compared.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(9017) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 120, &[0x68]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 29,
                occupied: 29,
                drawn: 14,
                phases: BTreeMap::from([(0, 15), (1, 14)]),
            },
            "{coverage:?}"
        );
    }

    /// The area-8 record `$B288` (byte 8 `$66`) placed in a stage-1-1 scene,
    /// where no IRQ splits the loop: the whole 80-frame spawn cycle is compared
    /// against the oracle. The counter's phases 0-4 each hold for 16 frames; the
    /// fifth drop writes phase 5, which frees the slot in the same run, so it
    /// never appears at a loop head (see [`the_fifth_phase_frees_the_slot`]).
    #[test]
    fn every_phase_of_the_counter_runs() {
        let Some(mut scene) = Scene::route(700) else {
            return;
        };
        scene.place(1, &place_record(), 0x1E);
        let coverage = assert_actor_phases(scene, 100, &[0x68]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 100,
                occupied: 80,
                drawn: 84,
                phases: BTreeMap::from([(0, 16), (1, 16), (2, 16), (3, 16), (4, 16)]),
            },
            "{coverage:?}"
        );
    }

    /// A phase of 5 with the timer off its 16-frame boundary is the one state the
    /// free check (`$8771`) reads without immediately dropping the slot; poking it
    /// shows the port holds phase 5 for the frames left before the free.
    #[test]
    fn the_fifth_phase_frees_the_slot() {
        let Some(mut scene) = Scene::route(700) else {
            return;
        };
        scene.place(1, &place_record(), 0x1E);
        let wram = &mut scene.machine.bus.wram;
        wram[0x1DD0 + 1] = 5;
        wram[0x1DF0 + 1] = 2;
        let coverage = assert_actor_phases(scene, 30, &[0x68]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 30,
                occupied: 14,
                drawn: 16,
                phases: BTreeMap::from([(5, 14)]),
            },
            "{coverage:?}"
        );
    }

    /// The area-8 `$68` record (`docs/ACTORS.md` §7b: `$A000[area]` -> per-stage
    /// table -> `id*2`), whose byte 8 selects the `$66` child.
    fn place_record() -> SpawnRecord {
        SpawnRecord::from([
            0x1E, 0x00, 0x08, 0x00, 0x18, 0x34, 0x00, 0x80, 0x66, 0x00, 0x02, 0x00,
        ])
    }
}
