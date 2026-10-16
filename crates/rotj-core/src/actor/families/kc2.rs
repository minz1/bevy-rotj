//! Type `$C2`, the area-20:00 `$C4` wave spawner (bank 12 `$97B6`).

use crate::actor::engine::{self, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The type the handler hands to the highest free main slot.
const CHILD: u8 = 0xC4;
/// CPU `$9883`: per-subtype player x high byte the phase-0 wait ends at.
const WAIT_TABLE: u16 = 0x9883;
/// CPU `$9887`: per-subtype `vx_drift`/`vy` pairs the child is thrown with.
const VELOCITY_TABLE: u16 = 0x9887;
/// Frames between two spawns (`$7E10` reload at `$9868`).
const SPAWN_DELAY: u8 = 0x08;
/// Spawns before the handler frees its own slot (`$7DF0` checked at `$9873`).
const SPAWN_LIMIT: u8 = 0x10;

/// Type `$C2` keeps its phase, timer and counter in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$97B6`, phases 0 and 2 of the `$987F` table).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, _work: &mut Work, cx: &mut Cx<'_>) {
        match body.phase {
            0 => {
                let wait = engine::rom_byte(cx.prg, 12, WAIT_TABLE + u16::from(body.subtype));
                if wait == cx.effects.player.x.to_be_bytes()[0] {
                    body.phase = 2;
                }
            }
            2 => {
                body.counter = body.counter.wrapping_sub(1);
                if body.counter & 0x80 == 0 {
                    return;
                }
                if let Some(index) = (0..16).rev().find(|&y| cx.slots.main[y].kind == 0) {
                    let (x, y, subtype) = (body.x, body.y, body.subtype);
                    let at = u16::from(subtype) * 4;
                    let drift = u16::from_be_bytes([
                        engine::rom_byte(cx.prg, 12, VELOCITY_TABLE + 1 + at),
                        engine::rom_byte(cx.prg, 12, VELOCITY_TABLE + at),
                    ]);
                    let velocity = u16::from_be_bytes([
                        engine::rom_byte(cx.prg, 12, VELOCITY_TABLE + 3 + at),
                        engine::rom_byte(cx.prg, 12, VELOCITY_TABLE + 2 + at),
                    ]);
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
                    child.body.contact = 0x88;
                    child.body.health = 0x8888;
                    child.body.state = 0x40;
                    child.body.flags = 1;
                    child.body.chr_slot = 6;
                    child.body.x = x;
                    child.body.y = y;
                    child.body.subtype = subtype;
                    child.body.vx_drift = drift.cast_signed();
                    child.body.vy = velocity.cast_signed();
                }
                body.counter = SPAWN_DELAY;
                body.timer = body.timer.wrapping_add(1);
                if body.timer >= SPAWN_LIMIT {
                    *kind = 0;
                }
            }
            phase => panic!("type $C2 phase {phase} is past the $987F table"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Area 20:00. The runtime spawner hands the `$C2` controller its
    /// highest-free slots; the `$C4` children it spawns (slots 13-15) are
    /// unported, so a `$D450` poke frees them at the loop head each frame. The
    /// three `$C2` slots (0/1/4) can each spawn one a frame, so all three are
    /// cleared: no child ever runs and the diff is the controller's writes.
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
