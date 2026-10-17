//! Type `$3E` (bank 11 `$92B9`): a three-phase walker that stops, hops and
//! throws a `$40` projectile.
//!
//! It calls only routines already in `docs/ACTORS.md` §9c: `$A000` (`load`),
//! `$A56E` (`dying`), `$ABDF` (`player_dx`), `$A852` (`on_screen`), `$AC06`
//! (`tile_bits`), `$C247` (`draw`), `$A9C4` (`contact`), `$AF38`
//! (`free_child_slot`), `$CD50` (`despawn`) and `$A934` (`store`). It never
//! integrates: its position only changes through the other handlers' writes.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::families::Brain;
use crate::actor::slots::Body;
use crate::actor::world::Cx;
use crate::actors::CELL_PENDING;

/// Metasprite pointers indexed by `$7C30` (bank 11 `$9402`, 2-byte entries).
const METASPRITES: u16 = 0x9402;
/// `$826F` maps bank 9 into `$A000` for the draw.
const DRAW_BANK: u8 = 9;
/// The projectile it throws (`$93CD` leaves the kind to `$7CD0`).
const PROJECTILE: u8 = 0x40;
/// The throw sound written to `$42` (`$9341`).
const THROW_SOUND: u8 = 0x2E;
/// The hit box the contact uses (`$938A`).
const HITBOX: u8 = 0x0D;
/// Contact damage (`$938E`).
const DAMAGE: u8 = 2;
/// The player must be closer than this (`$ABDF`'s high byte below 6) to wake it.
const WAKE_DISTANCE: u16 = 0x0600;
/// Frames the walk lasts before it stops (`$9324`).
const WALK_FRAMES: u8 = 0x50;
/// Frames of throwing before it turns back (`$935C`).
const THROW_FRAMES: u8 = 0x3C;
/// Offsets of the projectile from the thrower (`$93BF`, `$93D3`, `$93E2`).
const THROW_OFFSET_X: u16 = 0x00E0;
const THROW_OFFSET_Y: u16 = 0x01D0;

phases! {
    /// The thrower's phase (`$7DD0`): the game stores its doubled index into `$940A`.
    Phase {
        /// Standing until the player is near and it is not mid-hit.
        Standing = 0,
        /// Crouching for eight frames, then holding until it throws or is hurt.
        Crouching = 2,
        /// Throwing a projectile every eighth frame.
        Throwing = 4,
    }
}

/// Type `$3E` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$92B9`; phases through the table at `$940A`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if engine::dying(kind, body, work, 0x80, cx) {
            return;
        }
        work.attr |= body.flags;
        match Phase::from_byte(body.phase) {
            Some(Phase::Standing) => standing(body, work, cx),
            Some(Phase::Crouching) => crouching(body),
            Some(Phase::Throwing) => throwing(body, work, cx),
            None => panic!("type $3E phase {} is past the $940A table", body.phase),
        }
        tail(kind, body, work, cx);
    }
}

/// Standing (`$92DD`): stand until the player is near and the actor is not mid-hit.
fn standing(body: &mut Body, work: &Work, cx: &Cx<'_>) {
    body.sprite = 0;
    body.contact = 0x80;
    if body.anim & 0x80 == 0
        && engine::player_dx(work, &cx.effects.player).unsigned_abs() < WAKE_DISTANCE
    {
        body.phase = Phase::Crouching.byte();
        body.timer = 0;
    }
}

/// Crouching (`$9302`): crouch for eight frames, then hold until it throws or is hurt.
fn crouching(body: &mut Body) {
    body.contact = 0x10;
    body.timer = body.timer.wrapping_add(1);
    if body.timer < 8 {
        body.sprite = 1;
        return;
    }
    if body.anim & 0x80 != 0 {
        body.phase = Phase::Standing.byte();
        return;
    }
    if body.timer >= WALK_FRAMES {
        body.phase = Phase::Throwing.byte();
        body.timer = 0;
    }
    body.sprite = 3;
}

/// Throwing (`$933A`): throw every eighth frame, alternating two poses.
fn throwing(body: &mut Body, work: &Work, cx: &mut Cx<'_>) {
    if body.timer.is_multiple_of(8) {
        cx.effects.sound[0] = THROW_SOUND;
        spawn_projectile(body, work, cx);
    }
    body.sprite = if body.timer & 0x08 == 0 { 2 } else { 3 };
    body.timer = body.timer.wrapping_add(1);
    if body.timer >= THROW_FRAMES {
        body.anim = 0x80;
        body.phase = Phase::Crouching.byte();
    }
}

/// The shared tail from `$936A`: draw, contact and store, or store or free off screen.
fn tail(kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    match engine::on_screen(work, cx.world) {
        Visibility::On => {}
        Visibility::Near => {
            engine::store(body, work);
            return;
        }
        Visibility::Far => {
            engine::despawn(kind, body, CELL_PENDING, cx.schedule);
            return;
        }
    }
    work.pointer = engine::rom_word(cx.prg, 11, METASPRITES + 2 * u16::from(body.sprite));
    engine::tile_bits(work, cx.prg);
    engine::draw(work, DRAW_BANK, cx);
    if work.anchor & 0x40 == 0 {
        work.hitbox = HITBOX;
        engine::contact(kind, body, work, DAMAGE, cx);
    }
    engine::store(body, work);
}

/// `$93A8`: writes a `$40` projectile into the highest free slot of 16-23, if any.
fn spawn_projectile(body: &Body, work: &Work, cx: &mut Cx<'_>) {
    let Some(slot) = engine::free_child_slot(cx.slots) else {
        return;
    };
    let facing = work.attr & 0xC0;
    let facing_left = facing & 0x40 != 0;
    let child = &mut cx.slots.main[slot];
    child.kind = PROJECTILE;
    child.body.flags = facing;
    child.body.phase = u8::from(facing_left);
    child.body.x = if facing_left {
        work.x.wrapping_sub(THROW_OFFSET_X)
    } else {
        work.x.wrapping_add(THROW_OFFSET_X)
    };
    child.body.y = work.y.wrapping_sub(THROW_OFFSET_Y);
    child.body.chr_slot = body.chr_slot;
    child.body.state = 0;
    child.body.timer = 0;
    child.brain = Brain::new(PROJECTILE);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// The `$40` projectiles are unported, so each loop frees them at `$D450`.
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
        let Some(scene) = scene_at(18521) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 900, &[0x3E]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 691,
                occupied: 447,
                drawn: 348,
                phases: BTreeMap::from([(0, 292), (2, 104), (4, 72)]),
            },
            "{coverage:?}"
        );
    }

    #[test]
    fn throws_right_when_facing_right() {
        let Some(mut scene) = scene_at(18521) else {
            return;
        };
        for slot in [1u16, 3] {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, 0x7C00 + slot, 0x02));
        }
        let mut probe = scene.clone();
        let mut right = false;
        for _ in 0..300 {
            probe.run(1, &[]);
            let wram = &probe.machine.bus.wram;
            for parent in 0..16 {
                if wram[0x1CC0 + parent] != 0x3E {
                    continue;
                }
                let px = u16::from_be_bytes([wram[0x1B00 + parent], wram[0x1B20 + parent]]);
                for child in 0..8 {
                    if wram[0x1CD0 + child] == 0x40 {
                        let cx = u16::from_be_bytes([wram[0x1B10 + child], wram[0x1B30 + child]]);
                        right |= cx > px;
                    }
                }
            }
        }
        assert!(right, "no right-facing throw in 300 loops");
        let coverage = assert_actor_phases(scene, 300, &[0x3E]);
        assert!(
            coverage.phases.get(&4).is_some_and(|&n| n > 0),
            "{coverage:?}"
        );
    }

    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = scene_at(18521) else {
            return;
        };
        scene.isolate(&[0x3E]);
        let slot = (0..16)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x3E)
            .expect("a live $3E");
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.script(200, |_| 0);
        scene.player(x, y);
        scene.machine.bus.trace.pokes.push((0xD450, 0x47A, 0));
        let mut probe = scene.clone();
        let mut energy = probe.machine.bus.ram[0x478];
        let mut hits = 0;
        for _ in 0..150 {
            probe.run(1, &[]);
            let now = probe.machine.bus.ram[0x478];
            if now < energy {
                hits += 1;
                energy = now;
            }
        }
        assert!(hits > 0, "the oracle takes no contact hit");
        let coverage = assert_actor_phases(scene, 150, &[0x3E]);
        assert_eq!(coverage.frames, 150, "{coverage:?}");
        assert_eq!(coverage.drawn, 150, "{coverage:?}");
    }
}
