//! Type `$36` (bank 11 `$8A92`): a drifting flyer that drops a `$24` bomb.
//!
//! It calls only routines already in `docs/ACTORS.md` §9c: `$A000` (`load`),
//! `$A56E` (`dying`), `$A79B` (`integrate`), `$A852` (`on_screen`), `$AC06`
//! (`tile_bits`), `$C247` (`draw`), `$A9C4` (`contact`) and `$A934` (`store`).
//! It ends a frame with `$A934` alone, never `$A8E4`, so it keeps its velocity
//! instead of falling.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::families::Brain;
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// Metasprites indexed by `$7C30` (bank 11 `$8D0F`, 2-byte entries).
const METASPRITES: u16 = 0x8D0F;
/// The dying-animation metasprites indexed by `$7E90 & $7E` (bank 11 `$8D19`).
const ANIMATION: u16 = 0x8D19;
/// The `$24` child it drops (`$8CAE`).
const CHILD: u8 = 0x24;

phases! {
    /// The bomber's phase (`$7DD0`), which picks its handler (`$8D0B`).
    Phase {
        /// Drifting, then arming after a run of frames.
        Drifting = 0,
        /// Dropping a bomb, then drifting until the facing settles.
        Bombing = 1,
    }
}

/// Type `$36` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8A92`; phases through the table at `$8D0B`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if engine::dying(kind, body, work, 0x80, cx) {
            return;
        }
        work.attr |= body.flags;
        engine::integrate(body, work);
        match Phase::from_byte(body.phase) {
            Some(Phase::Drifting) => drifting(body, work),
            Some(Phase::Bombing) => bombing(body, work, cx),
            None => panic!("type $36 phase {} is past the $8D0B table", body.phase),
        }
        tail(kind, body, work, cx);
    }
}

/// Drifting (`$8ABB`): drift, then arm after `$20` frames of a non-zero behaviour.
fn drifting(body: &mut Body, work: &mut Work) {
    drift(body, work);
    body.sprite = 0;
    if body.behaviour == 0 || body.timer & 0x80 != 0 {
        return;
    }
    body.counter = body.counter.wrapping_add(1);
    if body.counter < 0x20 {
        return;
    }
    body.counter = 0;
    body.vx_drift = 0;
    body.phase = Phase::Bombing.byte();
    body.sprite = 2;
}

/// Bombing (`$8AF0`): drop a `$24` bomb, then drift until the facing settles.
fn bombing(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    if body.timer == 0 {
        body.counter = body.counter.wrapping_add(1);
        if body.counter < 0x10 {
            body.sprite = 2;
            return;
        }
        cx.effects.sound[0] = 0x3D;
        if let Some(slot) = engine::free_child_slot(cx.slots) {
            spawn_child(body, work, slot, cx);
        }
        body.sprite = 4;
        body.anim = 0x80;
        body.timer = 1;
        body.subtype = body.subtype.wrapping_add(1);
        body.vx_drift = if work.attr & 0x40 != 0 { 0x20 } else { -0x20 };
        return;
    }
    drift(body, work);
    let settled = if work.attr & 0x40 != 0 {
        body.vx_drift < 0 || body.vx_drift.to_be_bytes()[1] < 0x08
    } else {
        body.vx_drift >= 0 || body.vx_drift.to_be_bytes()[1] >= 0xF8
    };
    if !settled {
        return;
    }
    if body.behaviour & 0x01 == 0 || body.subtype >= 0x02 {
        body.timer = 0x80;
        body.phase = Phase::Drifting.byte();
        body.sprite = 0;
    } else {
        body.timer = 0;
        body.counter = 0;
        body.sprite = 2;
    }
}

/// `$8CBC`: accelerate `$7E50:$7E60` by two toward the facing, clamped to +-64.
fn drift(body: &mut Body, work: &Work) {
    if work.attr & 0x40 != 0 {
        body.vx_drift = body.vx_drift.wrapping_sub(2);
        if body.vx_drift >= 0 || body.vx_drift.to_be_bytes()[1] >= 0xC0 {
            return;
        }
        body.vx_drift = -0x40;
    } else {
        body.vx_drift = body.vx_drift.wrapping_add(2);
        if body.vx_drift < 0 || body.vx_drift.to_be_bytes()[1] < 0x40 {
            return;
        }
        body.vx_drift = 0x40;
    }
}

/// The shared tail from `$8B87`: draw, the two extra draws, contact and store.
fn tail(kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    match engine::on_screen(work, cx.world) {
        Visibility::On => {}
        Visibility::Near => {
            engine::store(body, work);
            return;
        }
        Visibility::Far => {
            *kind = 0;
            return;
        }
    }
    work.pointer = engine::rom_word(cx.prg, 11, METASPRITES + 2 * u16::from(body.sprite));
    engine::tile_bits(work, cx.prg);
    engine::draw(work, 1, cx);
    let (saved_x, saved_y) = (work.screen_x, work.screen_y);
    if cx.world.frame_odd {
        let y = if body.sprite == 0 { 2 } else { 6 };
        work.pointer = engine::rom_word(cx.prg, 11, METASPRITES + y);
        engine::draw(work, 1, cx);
    }
    if body.anim & 0x80 != 0 {
        let frame = u16::from(body.anim & 0x7E);
        work.pointer = engine::rom_word(cx.prg, 11, ANIMATION + frame);
        body.anim = body.anim.wrapping_add(1);
        if body.anim >= 0x86 {
            body.anim = 0;
        }
        work.screen_y = work.screen_y.wrapping_sub(0x1D);
        work.screen_x = if work.attr & 0x40 != 0 {
            work.screen_x.wrapping_sub(0x10)
        } else {
            work.screen_x.wrapping_add(0x10)
        };
        let attr = work.attr;
        work.attr = 0;
        engine::draw(work, 1, cx);
        work.attr = attr;
    }
    work.screen_x = saved_x;
    work.screen_y = saved_y;
    if work.anchor & 0x40 == 0 {
        work.hitbox = 2;
        engine::contact(kind, body, work, 2, cx);
    }
    engine::store(body, work);
}

/// Writes a `$24` child into slot `slot` (16-23), as `$8C59` does.
fn spawn_child(body: &Body, work: &Work, slot: usize, cx: &mut Cx<'_>) {
    let child = &mut cx.slots.main[slot];
    let facing_left = work.attr & 0x40 != 0;
    child.kind = CHILD;
    child.body.flags = 0;
    child.body.phase = u8::from(facing_left);
    child.body.x = if facing_left {
        // `$8C8A: $91 - $80`, then `$7B00,x` (the *stored* x high) `- $01`.
        let [body_hi, _] = body.x.to_be_bytes();
        let (lo, carry) = work.x.to_be_bytes()[1].overflowing_sub(0x80);
        let hi = body_hi.wrapping_sub(1).wrapping_sub(u8::from(carry));
        u16::from_be_bytes([hi, lo])
    } else {
        let (lo, carry) = work.x.to_be_bytes()[1].overflowing_add(0x80);
        let hi = work.x.to_be_bytes()[0]
            .wrapping_add(1)
            .wrapping_add(u8::from(carry));
        u16::from_be_bytes([hi, lo])
    };
    let (lo, carry) = work.y.to_be_bytes()[1].overflowing_sub(0xD0);
    let hi = work.y.to_be_bytes()[0]
        .wrapping_sub(1)
        .wrapping_sub(u8::from(carry));
    child.body.y = u16::from_be_bytes([hi, lo]);
    child.body.chr_slot = body.chr_slot;
    child.body.state = 0;
    child.body.sprite = 3;
    child.brain = Brain::new(CHILD);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// The `$36`'s child slots are freed each loop so the parent keeps dropping bombs.
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

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = scene_at(7426) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 400, &[0x36]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 280,
                occupied: 222,
                drawn: 189,
                phases: BTreeMap::from([(0, 249), (1, 116)]),
            }
        );
    }

    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = scene_at(7426) else {
            return;
        };
        scene.isolate(&[0x36]);
        let slot = (0..16)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x36)
            .expect("a live $36");
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.script(200, |_| 0);
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
        let coverage = assert_actor_phases(scene, 40, &[0x36]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 40,
                occupied: 40,
                drawn: 40,
                phases: BTreeMap::from([(0, 61), (1, 12)]),
            }
        );
    }

    /// The bomb: the handler's `$24` spawn lands in the child slots 16-23.
    #[test]
    fn drops_its_bomb() {
        let Some(mut scene) = Scene::tas(7426) else {
            return;
        };
        scene.isolate(&[0x36]);
        for _ in 0..200 {
            scene.run(1, &[]);
            if (16..24).any(|slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x24) {
                return;
            }
        }
        panic!("the oracle never spawned a $24 child");
    }
}
