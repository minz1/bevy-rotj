//! Type `$88`, a walker that turns at ledges and walls (bank 0 `$95F6`).

use crate::actor::collision::{self, Probe};
use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// Metasprite pointers indexed by `$7C30` (bank 0 `$9750`, byte-indexed).
const METASPRITES: u16 = 0x9750;
/// The walk velocity low bytes (`$975E`), indexed by the facing bit.
const WALK_LO: u16 = 0x975E;
/// The walk velocity high bytes (`$9760`).
const WALK_HI: u16 = 0x9760;
/// The bank `$84CA` maps at `$A000` for the draw.
const DRAW_BANK: u8 = 9;
/// Metasprite index while turning round.
const TURNING_SPRITE: u8 = 8;
/// The turn-round ends when its counter reaches this.
const TURN_LENGTH: u8 = 0x10;
/// The counter value at which the facing flips.
const TURN_FLIP: u8 = 4;
/// How far ahead of the feet the ledge probe looks.
const LEDGE_REACH: u16 = 0x40;
const GROUND: u8 = 0x40;
const WALL: u8 = 0x80;
const AIRBORNE_FLOOR: u8 = 0x20;

/// Type `$88` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$95F6`; the `$7DD0` phase is 0 or 2).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if engine::dying(kind, body, work, 0x80, cx) {
            return;
        }
        work.attr |= body.flags;
        engine::integrate(body, work);
        let mut probe = Probe::default();
        let mut flags = 0u8;
        if collision::feet(work, &mut probe, cx) & 0x80 == 0 {
            flags |= GROUND;
        }
        collision::head(work, &mut probe, cx);
        if collision::ahead(work, &mut probe, 0xFF, cx) & 0x80 == 0 {
            flags |= WALL;
        }
        if work.vy >= 0 && flags & GROUND == 0 {
            // The game passes rise = Y:A; `corners` only uses it while rising.
            if collision::corners(work, &mut probe, 0, 0x00C0, cx) & 0x80 == 0 {
                flags |= AIRBORNE_FLOOR;
            }
        }
        if flags & (GROUND | AIRBORNE_FLOOR) != 0 {
            body.vx_drift = i16::from_be_bytes([probe.landing[1], probe.landing[0]]);
        }
        match body.phase {
            0 => walk(body, work, flags, &mut probe, cx),
            2 => turn(body, work),
            phase => panic!("type $88 phase {phase} is past the $975A table"),
        }
        tail(kind, body, work, cx);
    }
}

/// Phase 0 (`$9667`): walk, and turn round at a wall or a ledge.
fn walk(body: &mut Body, work: &mut Work, flags: u8, probe: &mut Probe, cx: &mut Cx<'_>) {
    let facing_left = work.attr & 0x40 != 0;
    let at_edge = flags & WALL != 0 || {
        probe.x = if facing_left {
            work.x.wrapping_sub(LEDGE_REACH)
        } else {
            work.x.wrapping_add(LEDGE_REACH)
        };
        probe.y = work.y.wrapping_add(6);
        collision::solid(probe, 0x84, cx) & 0x80 == 0
    };
    if at_edge {
        body.phase = 2;
        body.ref_vx = 0;
        body.anim = 0;
        body.vx = 0;
        work.ax = 0;
    } else {
        let facing = u16::from(facing_left);
        body.ref_vx = i16::from_be_bytes([
            engine::rom_byte(cx.prg, 0, WALK_HI + facing),
            engine::rom_byte(cx.prg, 0, WALK_LO + facing),
        ]);
        body.anim = body.anim.wrapping_add(1);
        body.sprite = (body.anim & 0x18) >> 2;
    }
}

/// Phase 2 (`$96E4`): stand still, flip the facing, then go back to walking.
fn turn(body: &mut Body, work: &mut Work) {
    body.sprite = TURNING_SPRITE;
    body.anim = body.anim.wrapping_add(1);
    if body.anim == TURN_FLIP {
        work.attr ^= 0x40;
    } else if body.anim >= TURN_LENGTH {
        body.phase = 0;
        body.anim = 0;
    }
}

/// The shared tail from `$9708`: draw, contact and store.
fn tail(kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    match engine::on_screen(work, cx.world) {
        Visibility::On => {
            work.pointer = engine::rom_word(cx.prg, 0, METASPRITES + u16::from(body.sprite));
            engine::tile_bits(work, cx.prg);
            engine::draw(work, DRAW_BANK, cx);
            if work.anchor & 0x40 == 0 {
                work.hitbox = 9;
                engine::contact(kind, body, work, 2, cx);
            }
        }
        Visibility::Near => {}
        Visibility::Far => {
            engine::despawn(kind, body, 0x80, cx.schedule);
            return;
        }
    }
    work.age = work.age.wrapping_add(1);
    engine::end_frame(body, work);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    fn scene_with_walker() -> Option<(Scene, usize)> {
        let scene = Scene::tas(14141)?;
        let slot = (0..16)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x88)
            .expect("a live $88");
        Some((scene, slot))
    }

    #[test]
    fn matches_the_oracle() {
        let Some((scene, _)) = scene_with_walker() else {
            return;
        };
        let coverage = assert_actor_phases(scene, 300, &[0x88]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 299,
                occupied: 142,
                drawn: 142,
                phases: BTreeMap::from([(0, 183)]),
            }
        );
    }

    #[test]
    fn turning_round_matches_the_oracle() {
        let Some((mut scene, slot)) = scene_with_walker() else {
            return;
        };
        scene.machine.bus.wram[0x1DD0 + slot] = 2;
        scene.machine.bus.wram[0x1E90 + slot] = 0;
        let coverage = assert_actor_phases(scene, 40, &[0x88]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 40,
                occupied: 40,
                drawn: 40,
                phases: BTreeMap::from([(0, 24), (2, 16)]),
            }
        );
    }

    #[test]
    fn hurts_the_player_up_close() {
        let Some((mut scene, slot)) = scene_with_walker() else {
            return;
        };
        scene.isolate(&[0x88]);
        scene.script(200, |_| 0);
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.player(x, y);
        let mut probe = scene.clone();
        let mut hits = 0;
        for _ in 0..40 {
            probe.run(1, &[]);
            hits += usize::from(probe.machine.bus.ram[0x479] != 0);
        }
        assert!(hits > 0, "the oracle takes no contact hit");
        let coverage = assert_actor_phases(scene, 40, &[0x88]);
        assert!(coverage.frames > 0, "{coverage:?}");
        assert!(coverage.occupied > 0, "{coverage:?}");
    }
}
