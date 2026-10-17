//! Type `$7E` (bank 0 `$9032`): the spinner of stage 8 (area 16:00), spawned by
//! the `$80` handler (`$9297`) in slots 12-15.
//!
//! It calls only routines in `docs/ACTORS.md` §9c: `$A000` (`load`), `$A79B`
//! (`integrate`), `$A7DD` (`collision::feet`), `$AF7D` (`collision::below`),
//! `$A852` (`on_screen`), `$C247` (`draw`, bank 9 as `$84CA` maps it), `$AEE3`
//! (`touch`) and `$A934` (`store`). Its spawning phase also calls the bank-0 helper
//! `$8D84`, which spawns a type `$7A` child (bank 0 `$8DB5`, unported) at the
//! parent's x and sets the parent's `$7D50` to `$40`.

use crate::actor::collision::{self, Probe};
use crate::actor::engine::{self, Visibility, Work};
use crate::actor::families::Brain;
use crate::actor::slots::Body;
use crate::actor::world::Cx;
use crate::actors::STATE_UNTARGETED;

/// `$9251`: the metasprite pointer, overlapping words indexed by `$7C30`.
const METASPRITES: u16 = 0x9251;
/// `$9257`: the idling vy low every `$7E90` 8-15 (index `$7E90 - 8`).
const PHASE0_VY: u16 = 0x9257;
/// `$925F`/`$926B`: the swinging vy low/high with `$7D90` bit 7 clear (index `$7E90`).
const PHASE2_VY: (u16, u16) = (0x925F, 0x926B);
/// `$9277`/`$927F`: the swinging vy low/high with `$7D90` bit 7 set.
const PHASE2_VY_SET: (u16, u16) = (0x9277, 0x927F);
/// `$9287`/`$928F`: the falling grounded vy low/high (index `$F1 & 7`).
const PHASE3_VY: (u16, u16) = (0x9287, 0x928F);
/// `$84CA` maps bank 9 at `$A000` for the draw.
const DRAW_BANK: u8 = 9;
/// The hit box and damage of the tail's `$AEE3` touch.
const HITBOX: u8 = 3;
const DAMAGE: u8 = 2;
/// The type `$8D84` spawns, at the parent's x with `$7B40:$7B60` = `$0C00`.
const CHILD: u8 = 0x7A;

phases! {
    /// The spinner's phase (`$7DD0`), which picks its handler (`$9243`).
    Phase {
        /// Counting up on the spot, bobbing slightly, until it wakes.
        Idling = 0,
        /// Rising towards the head row until the feet leave the ground.
        Rising = 1,
        /// Swinging through the vy table, then rising again or falling.
        Swinging = 2,
        /// Falling with a drift once the ground below is gone.
        Falling = 3,
        /// Flashing for eight frames.
        Flashing = 4,
        /// Descending towards the head row.
        Descending = 5,
        /// Spawning the `$7A` child and freeing the slot.
        Spawning = 6,
    }
}

/// Type `$7E` keeps its whole state in [`Body`]: `$7DD0` phase, `$7E90` its
/// timer, `$7C30` the metasprite index and `$7E70:$7E80` its vy.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9032`; the phase table is at `$9243`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        work.attr = (body.flags & 0x03) | 0x01;
        engine::integrate(body, work);
        match Phase::from_byte(body.phase) {
            Some(Phase::Idling) => idling(body, cx),
            Some(Phase::Rising) => rising(body, work, cx),
            Some(Phase::Swinging) => swinging(body, cx),
            Some(Phase::Falling) => falling(body, work, cx),
            Some(Phase::Flashing) => flashing(body, work),
            Some(Phase::Descending) => descending(body, work),
            Some(Phase::Spawning) => {
                // `$91E5`: spawn the `$7A` child, free the slot, no store.
                if spawn_child(body, cx) {
                    body.state = STATE_UNTARGETED;
                }
                *kind = 0;
                return;
            }
            None => panic!("type $7E phase {} is past the $9243 table", body.phase),
        }
        tail(kind, body, work, cx);
    }
}

/// `$9053`: count `$7E90` up; set the idle vy, a slight one, then wake.
fn idling(body: &mut Body, cx: &Cx<'_>) {
    body.anim = body.anim.wrapping_add(1);
    let timer = body.anim;
    if timer < 0x08 {
        body.vy = 0;
    } else if timer < 0x10 {
        let low = engine::rom_byte(cx.prg, 0, PHASE0_VY + u16::from(timer.wrapping_sub(0x08)));
        body.vy = i16::from_be_bytes([0, low]);
    } else {
        body.phase = Phase::Rising.byte();
        body.anim = 0;
    }
    body.sprite = 0;
}

/// `$908D`: rise until the head row, then drop when the feet leave the ground.
fn rising(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    if work.y.to_be_bytes()[0] >= 0x0D {
        body.phase = Phase::Spawning.byte();
        return;
    }
    let low = body.vy.to_be_bytes()[1].wrapping_add(1);
    body.set_byte(0x7E80, if low >= 0x40 { 0x3F } else { low });
    if body.anim & 0x80 != 0 {
        return;
    }
    let mut probe = Probe::default();
    if collision::feet(work, &mut probe, cx) & 0x80 == 0 {
        body.vy = 0;
        body.anim = 0;
        body.phase = Phase::Swinging.byte();
    }
}

/// `$90C9`: swing through the vy table for `$7D90 & $80` frames, then fall.
fn swinging(body: &mut Body, cx: &Cx<'_>) {
    body.anim = body.anim.wrapping_add(1);
    let time = body.anim;
    let (low_table, high_table) = if body.behaviour & 0x80 == 0 {
        if time >= 0x0C {
            body.vy = 0;
            body.phase = Phase::Rising.byte();
            body.anim = 0x80;
            return;
        }
        PHASE2_VY
    } else {
        if time >= 0x08 {
            body.vy = 0;
            body.phase = Phase::Falling.byte();
            return;
        }
        PHASE2_VY_SET
    };
    let low = engine::rom_byte(cx.prg, 0, low_table + u16::from(time));
    let high = engine::rom_byte(cx.prg, 0, high_table + u16::from(time));
    body.vy = i16::from_be_bytes([high, low]);
}

/// `$911F`: probe a step down for ground; land there or come down with a drift.
fn falling(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    work.y = work.y.wrapping_add(0x40);
    let mut probe = Probe::default();
    let below = collision::below(work, &mut probe, cx);
    work.y = work.y.wrapping_sub(0x40);
    if below & 0x80 != 0 {
        let index = u16::from(cx.world.frame & 0x07);
        let low = engine::rom_byte(cx.prg, 0, PHASE3_VY.0 + index);
        let high = engine::rom_byte(cx.prg, 0, PHASE3_VY.1 + index);
        body.vy = i16::from_be_bytes([high, low]);
        body.vx_drift = if body.behaviour & 0x01 == 0 {
            i16::from_be_bytes([0xFF, 0xF0])
        } else {
            0x0010
        };
        return;
    }
    body.vy = 0x10;
    body.anim = 0;
    body.sprite = 2;
    body.phase = Phase::Flashing.byte();
}

/// `$918E`: flash (`$40` in the attr) while `$7E90` counts to 8, then hold.
fn flashing(body: &mut Body, work: &mut Work) {
    if body.behaviour & 0x01 == 0 {
        work.attr |= 0x40;
    }
    body.anim = body.anim.wrapping_add(1);
    if body.anim >= 0x08 {
        body.phase = Phase::Descending.byte();
        body.vx_drift = 0;
        body.sprite = 4;
    }
}

/// `$91B9`: flash while descending to the head row, else ramp the vy.
fn descending(body: &mut Body, work: &mut Work) {
    if body.behaviour & 0x01 == 0 {
        work.attr |= 0x40;
    }
    if work.y.to_be_bytes()[0] >= 0x0D {
        body.phase = Phase::Spawning.byte();
        return;
    }
    let low = body.vy.to_be_bytes()[1].wrapping_add(3);
    body.set_byte(0x7E80, if low >= 0x50 { 0x4F } else { low });
}

/// `$8D84`: puts a type `$7A` in the highest free main slot at the parent's x,
/// `$0C00` y and CHR `$7`. Returns whether one was free.
fn spawn_child(parent: &Body, cx: &mut Cx<'_>) -> bool {
    let Some(index) = (0..16).rev().find(|&slot| cx.slots.main[slot].kind == 0) else {
        return false;
    };
    let child = &mut cx.slots.main[index];
    let body = &mut child.body;
    body.set_byte(0x7DD0, 0);
    body.set_byte(0x7B60, 0);
    body.set_byte(0x7B40, 0x0C);
    body.set_byte(0x7B20, parent.x.to_be_bytes()[1]);
    body.set_byte(0x7B00, parent.x.to_be_bytes()[0]);
    body.set_byte(0x7C90, 0x07);
    child.kind = CHILD;
    child.brain = Brain::new(CHILD);
    true
}

/// `$91FA`: draw through `$84CA` when on screen and touch the player, else store
/// (near off screen) or free the slot (far off screen).
fn tail(kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    match engine::on_screen(work, cx.world) {
        Visibility::On => {
            work.pointer = engine::rom_word(cx.prg, 0, METASPRITES + u16::from(body.sprite));
            work.tile = 0;
            engine::draw(work, DRAW_BANK, cx);
            if work.anchor & 0x40 == 0 {
                work.hitbox = HITBOX;
                engine::touch(work, DAMAGE, cx);
            }
            engine::store(body, work);
        }
        Visibility::Near => engine::store(body, work),
        Visibility::Far => *kind = 0,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};
    use crate::actor::slots::{NARROW, WIDE};

    /// Moves slot `from`'s actor to slot `to` and frees `from`.
    fn move_slot(scene: &mut Scene, from: usize, to: usize) {
        let wram = &mut scene.machine.bus.wram;
        for array in WIDE.into_iter().chain(NARROW) {
            let at = usize::from(array - 0x6000);
            wram[at + to] = wram[at + from];
        }
        wram[0x1CC0 + to] = wram[0x1CC0 + from];
        wram[0x1CC0 + from] = 0;
    }

    /// One `$7E` moved to slot 0 with every other slot freed, so its `$7A` child
    /// (`$8D84` puts it in the highest free slot, 15) is freed each loop before it
    /// runs: the child's handler (bank 0 `$8DB5`) is not ported.
    fn solo(frame: u64) -> Option<Scene> {
        let mut scene = Scene::tas(frame)?;
        let slot = (0..16).find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x7E)?;
        move_slot(&mut scene, slot, 0);
        for other in 1..32 {
            scene.machine.bus.wram[0x1CC0 + other] = 0;
        }
        scene.isolate(&[0x7E]);
        scene.machine.bus.trace.pokes.push((0xD450, 0x7CC0 + 15, 0));
        Some(scene)
    }

    /// The room at 27931: the `$7E` (behaviour `$80`) rises, swings and lands.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = solo(27931) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 80, &[0x7E]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 80,
                occupied: 76,
                drawn: 51,
                phases: BTreeMap::from([(1, 39), (2, 8), (3, 29)]),
            }
        );
    }

    /// The first spawn of the room, from the TAS frame just before it: it wakes
    /// from phase 0 and reaches phase 6, which spawns the `$7A` and frees the slot.
    #[test]
    fn matches_the_oracle_from_the_first_spawn() {
        let Some(scene) = solo(27599) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 140, &[0x7E]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 140,
                occupied: 124,
                drawn: 93,
                phases: BTreeMap::from([(0, 16), (1, 95), (2, 12), (6, 1)]),
            }
        );
    }

    /// Phase 3 at a ledge: it holds while `$AF7D` finds the floor (9 frames, drift
    /// `+$10` with `$7D90` bit 0 set), then its `$912F` arm drops it into phase 4
    /// (8 frames), phase 5 ramps the vy down to the head row and phase 6 frees it.
    #[test]
    fn falls_off_the_ledge_and_reaches_the_head_row() {
        let Some(mut scene) = solo(27931) else {
            return;
        };
        let wram = &mut scene.machine.bus.wram;
        wram[0x1DD0] = 3;
        wram[0x1B40] = 0x01;
        wram[0x1B60] = 0x00;
        wram[0x1E70] = 0;
        wram[0x1E80] = 0;
        wram[0x1D90] = 0x81;
        let coverage = assert_actor_phases(scene, 180, &[0x7E]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 180,
                occupied: 65,
                drawn: 54,
                phases: BTreeMap::from([(3, 9), (4, 8), (5, 47), (6, 1)]),
            }
        );
    }

    /// The tail's Near off-screen arm (`$9236`): store without drawing or freeing.
    #[test]
    fn near_off_screen_is_stored() {
        let Some(mut scene) = solo(27931) else {
            return;
        };
        let camera = u16::from_be_bytes([scene.machine.bus.ram[0xCC], scene.machine.bus.ram[0xCD]]);
        let [cam_hi, cam_lo] = camera.to_be_bytes();
        let [hi, lo] = camera.wrapping_add(320 * 16).to_be_bytes();
        for (addr, value) in [(0xCC, cam_hi), (0xCD, cam_lo), (0x7B00, hi), (0x7B20, lo)] {
            scene.machine.bus.trace.pokes.push((0xD450, addr, value));
        }
        let coverage = assert_actor_phases(scene, 30, &[0x7E]);
        assert_eq!((coverage.occupied, coverage.drawn), (30, 0), "{coverage:?}");
    }

    /// The tail's `$AEE3` touch (spawning never draws, so it cannot hurt).
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = solo(27931) else {
            return;
        };
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00], wram[0x1B20]]);
        let y = u16::from_be_bytes([wram[0x1B40], wram[0x1B60]]);
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
        let coverage = assert_actor_phases(scene, 40, &[0x7E]);
        assert_eq!(coverage.occupied, coverage.frames, "{coverage:?}");
    }
}
