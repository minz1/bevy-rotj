//! Type `$28` (bank 11 `$8000`): a platform that follows a waypoint script.
//!
//! It calls `$A000` (`load`), `$A79B` (`integrate`), `$AF38` (`free_child_slot`),
//! `$A852` (`on_screen`), `$C247` (`draw`) and `$A9C4` (`contact`). It stores its
//! position itself and never runs `$A8E4`, and any frame it is not on screen frees it.

use crate::actor::engine::{self, Visibility, Work, anchor};
use crate::actor::families::Brain;
use crate::actor::slots::Body;
use crate::actor::world::{Cx, sound};

const WAYPOINTS: u16 = 0x81DF;
const SEQUENCES: u16 = 0x81ED;
const METASPRITE: u16 = 0x81CB;
const CHILD: u8 = 0x24;

/// Type `$28`'s tables (bank 11), by `subtype & $3F` (the 6502's 8-bit `subtype << 2`).
pub struct Tables {
    /// The `(vx, vy)` a sequence entry names, by `subtype & $3F` (`$81FB`).
    pub velocities: [(i16, i16); 64],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        let bytes: [u8; 256] = engine::rom_bytes(prg, 11, 0x81FB);
        Tables {
            velocities: std::array::from_fn(|i| {
                (
                    i16::from_be_bytes([bytes[4 * i + 1], bytes[4 * i]]),
                    i16::from_be_bytes([bytes[4 * i + 3], bytes[4 * i + 2]]),
                )
            }),
        }
    }
}

/// Type `$28` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8000`; the direction picked by `$7DB0` through `$81D7`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        engine::integrate(body, work);
        // `ASL` twice on a byte: the index wraps at 8 bits, as the 6502's `TAY` does.
        let table = engine::rom_word(
            cx.prg,
            11,
            WAYPOINTS + u16::from(body.behaviour.wrapping_shl(1)),
        );
        let y = body.timer.wrapping_shl(1);
        let target = u16::from_le_bytes([
            engine::rom_byte(cx.prg, 11, table + u16::from(y)),
            engine::rom_byte(cx.prg, 11, table + u16::from(y.wrapping_add(1))),
        ]);
        let reached = match body.subtype {
            0 => {
                (body.vx, body.vy) = (0x10, 0);
                target < work.x
            }
            1 => {
                (body.vx, body.vy) = (-0x10, 0);
                work.x < target
            }
            2 => {
                (body.vx, body.vy) = (0, 0x10);
                work.y >= target
            }
            3 => {
                (body.vx, body.vy) = (0, -0x10);
                target >= work.y
            }
            subtype => panic!("type $28 subtype {subtype} is past the $81D7 table"),
        };
        if reached {
            if body.subtype < 2 {
                work.x = target;
            } else {
                work.y = target;
            }
            advance(body, cx);
        }
        if engine::on_screen(work, cx.world) != Visibility::On || work.anchor & anchor::BOTTOM != 0
        {
            *kind = 0;
            return;
        }
        work.pointer = engine::rom_word(cx.prg, 11, METASPRITE);
        work.chr_slot = 0;
        work.tile = 0;
        work.attr = 1;
        engine::draw(work, 11, cx);
        work.hitbox = 1;
        engine::contact(kind, body, work, 2, cx);
        body.anim = body.anim.wrapping_add(1);
        if body.anim.is_multiple_of(0x40) {
            spawn_children(body, cx);
        }
        body.x = work.x;
        body.y = work.y;
    }
}

/// `$8134`: the next waypoint and the velocity its sequence entry names.
fn advance(body: &mut Body, cx: &Cx<'_>) {
    body.timer = body.timer.wrapping_add(1);
    let table = engine::rom_word(
        cx.prg,
        11,
        SEQUENCES + u16::from(body.behaviour.wrapping_shl(1)),
    );
    body.subtype = engine::rom_byte(cx.prg, 11, table + u16::from(body.timer));
    (body.vx, body.vy) =
        cx.prg.families.waypoint_platform.velocities[usize::from(body.subtype & 0x3F)];
}

/// `$815B`: a `$24` to the right, then one to the left, in the highest free child slots.
fn spawn_children(body: &Body, cx: &mut Cx<'_>) {
    for right in [true, false] {
        let Some(slot) = engine::free_child_slot(cx.slots) else {
            return;
        };
        cx.effects.sound[0] = sound::SPAWN;
        let child = &mut cx.slots.main[slot];
        child.kind = CHILD;
        child.body.x = if right {
            body.x.wrapping_add(0x80)
        } else {
            body.x.wrapping_sub(0x80)
        };
        child.body.phase = if right { 3 } else { 2 };
        child.body.y = body.y;
        child.body.chr_slot = body.chr_slot;
        child.body.state = 0;
        child.body.sprite = 1;
        child.body.flags = 3;
        child.brain = Brain::new(CHILD);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// The TAS scene at `frame` with the `$24` children the handler spawns freed at
    /// every loop head. They belong to `enemy_shot`: a `$24` with `$7C30` 1 reads its
    /// metasprite from `$98CA`, which the CPU fetches from the `$8000` window
    /// (bank 8), but the port's `enemy_shot` passes bank 1 and panics there (a lead in
    /// `REPORT.md`). `hover_diver` clears other families' children the same way.
    fn scene_at(frame: u64) -> Option<Scene> {
        let mut scene = Scene::tas(frame)?;
        for slot in 16..24 {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, 0x7CC0 + slot, 0));
        }
        Some(scene)
    }

    fn platform_slot(scene: &Scene) -> usize {
        (0..16)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x28)
            .expect("a live $28")
    }

    /// The slot's level position (`$7B00:$7B20`, `$7B40:$7B60`).
    fn position(scene: &Scene, slot: usize) -> (u16, u16) {
        let wram = &scene.machine.bus.wram;
        (
            u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]),
            u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]),
        )
    }

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = scene_at(21029) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 400, &[0x28]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 250,
                occupied: 209,
                drawn: 209,
                phases: BTreeMap::from([(0, 209)]),
            }
        );
    }

    /// The TAS only ever picks direction `$7DB0` 1 (the platform rides left across
    /// its whole window). Directions 0, 2 and 3 are forced by a `$7DB0` poke at
    /// the loop head; forcing 0 or 2 snaps the platform onto that axis's waypoint
    /// 0 (`$8134`, off screen, so it takes the `$8946` free tail) and 3 rises. The
    /// diff of every forced actor loop is the proof that branch is byte-exact.
    #[test]
    fn every_direction_matches_the_oracle() {
        if Scene::tas(21029).is_none() {
            return;
        }
        let mut ends = Vec::new();
        for subtype in 0..4u8 {
            let mut scene = scene_at(21029).expect("scene");
            let slot = platform_slot(&scene);
            scene.machine.bus.trace.pokes.push((
                0xD450,
                0x7DB0 + u16::try_from(slot).expect("slot"),
                subtype,
            ));
            let start = position(&scene, slot);
            let mut probe = scene.clone();
            probe.run(30, &[]);
            let end = position(&probe, slot);
            ends.push(end);
            let coverage = assert_actor_phases(scene, 30, &[0x28]);
            assert!(coverage.occupied >= 1, "direction {subtype}: {coverage:?}");
            if subtype == 1 {
                // The natural direction: -1 px/frame, no snap, so exactly 30 px.
                assert_eq!(end.0, start.0 - 30 * 16, "{start:?} -> {end:?}");
                assert_eq!(end.1, start.1, "{start:?} -> {end:?}");
            }
        }
        // The poke demonstrably changed the path: the four runs do not all end alike.
        assert!(ends.iter().any(|&e| e != ends[1]), "directions: {ends:?}");
    }

    /// `$815B`: every 64 hits of `$7E90` the handler writes a `$24` pair into the
    /// highest free child slots, right first (`x + $80`, phase 3) then left
    /// (`x - $80`, phase 2), carrying the parent's x, y and CHR shadow.
    #[test]
    fn spawns_the_24_pair() {
        let Some(mut scene) = scene_at(21029) else {
            return;
        };
        scene.isolate(&[0x28]);
        let slot = platform_slot(&scene);
        for _ in 0..400 {
            let parent = position(&scene, slot);
            let parent_chr = scene.machine.bus.wram[0x1C90 + slot];
            scene.run(1, &[]);
            let wram = &scene.machine.bus.wram;
            let children: Vec<usize> = (16..24).filter(|&s| wram[0x1CC0 + s] == 0x24).collect();
            if children.len() != 2 {
                continue;
            }
            let child = |s: usize| {
                (
                    u16::from_be_bytes([wram[0x1B00 + s], wram[0x1B20 + s]]),
                    u16::from_be_bytes([wram[0x1B40 + s], wram[0x1B60 + s]]),
                    wram[0x1DD0 + s],
                    wram[0x1C00 + s],
                    wram[0x1C30 + s],
                    wram[0x1D50 + s],
                    wram[0x1C90 + s],
                )
            };
            let kids: Vec<_> = children.iter().map(|&slot| child(slot)).collect();
            let (Some(&right), Some(&left)) = (
                kids.iter().find(|kid| kid.2 == 3),
                kids.iter().find(|kid| kid.2 == 2),
            ) else {
                continue;
            };
            assert_eq!(right.0, parent.0 + 0x80, "{right:?}");
            assert_eq!(left.0, parent.0 - 0x80, "{left:?}");
            for (_, y, _, flags, sprite, state, chr) in [right, left] {
                assert_eq!(
                    (y, flags, sprite, state, chr),
                    (parent.1, 3, 1, 0, parent_chr)
                );
            }
            return;
        }
        panic!("the oracle never spawned a $24 pair");
    }

    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = scene_at(21029) else {
            return;
        };
        scene.isolate(&[0x28]);
        let slot = platform_slot(&scene);
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.script(200, |_| 0);
        scene.player(x, y);
        let mut probe = scene.clone();
        let energy = probe.machine.bus.ram[0x478];
        probe.run(20, &[]);
        assert!(
            probe.machine.bus.ram[0x478] < energy,
            "the oracle takes no contact hit"
        );
        let coverage = assert_actor_phases(scene, 20, &[0x28]);
        assert_eq!(coverage.occupied, coverage.drawn);
    }
}
