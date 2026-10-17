//! Type `$6C` (bank 0 `$87F9`): a stage 3/9 enemy that cycles its sprite and
//! throws `$24` shots.
//!
//! It calls only routines already in `docs/ACTORS.md` §9c: `$A000` (`load`),
//! `$A56E` (`dying`), `$A79B` (`integrate`), `$A852` (`on_screen`), `$AF38`
//! (`free_child_slot`), `$AC06` (`tile_bits`), `$C247` (`draw`), `$A9C4`
//! (`contact`) and `$A934` (`store`). `$84CA` maps bank 9 at `$A000` around the
//! draw.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::families::Brain;
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The four metasprite pointers, indexed by `$7C30` (bank 0 `$8900`).
const METASPRITES: u16 = 0x8900;
const SPRITE_BANK: u8 = 9;
const HITBOX: u8 = 0x0C;
const DAMAGE: u8 = 4;
const CHILD: u8 = 0x24;

/// Type `$6C` keeps its state in [`Body`]: `$7E10` arms the shooting spell,
/// `$7E90` counts it, `$7DF0` is the rest timer and `$7DD0` only an "on screen"
/// flag (`$80`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$87F9`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if engine::dying(kind, body, work, 0, cx) {
            return;
        }
        work.attr |= body.flags;
        engine::integrate(body, work);
        if engine::on_screen(work, cx.world) != Visibility::On {
            if body.phase & engine::DRAWN != 0 {
                *kind = 0;
            } else {
                engine::store(body, work);
            }
            return;
        }
        if body.counter != 0 {
            shoot(body, work, cx);
        } else {
            rest(body, work);
        }
        body.phase = engine::DRAWN;
        work.pointer = engine::rom_word(cx.prg, 0, METASPRITES + u16::from(body.sprite));
        engine::tile_bits(work, cx.prg);
        engine::draw(work, SPRITE_BANK, cx);
        work.hitbox = HITBOX;
        engine::contact(kind, body, work, DAMAGE, cx);
        engine::store(body, work);
    }
}

/// `$882E`: the spell. The sprite advances every eighth count and a shot leaves
/// on each of the first four.
fn shoot(body: &mut Body, work: &Work, cx: &mut Cx<'_>) {
    body.anim = body.anim.wrapping_add(1);
    if body.anim & 0x80 == 0 || body.anim & 0x07 != 0 {
        return;
    }
    body.sprite += 2;
    if body.sprite >= 8 {
        body.sprite = 0;
    }
    let round = (body.anim & 0x7F) >> 3;
    if round < 4 {
        if let Some(slot) = engine::free_child_slot(cx.slots) {
            spawn_child(body, work, round, slot, cx);
        }
    } else {
        body.timer = 0x80;
        body.counter = 0;
        body.vy = i16::from_be_bytes([0, body.vy.to_be_bytes()[1]]);
        body.vx_drift = 0x28;
    }
}

/// `$886F`: waiting for the player. A rest timer with bit 7 set holds it; else it
/// stops dead and, once within `$A0` of the screen's left edge, starts a spell.
fn rest(body: &mut Body, work: &Work) {
    if body.timer & 0x80 != 0 {
        return;
    }
    body.vy = 0;
    if work.screen_x < 0xA0 {
        body.anim = 0x40;
        body.counter = 0x80;
        body.vx_drift = 0x0A;
    }
}

/// `$88BD`: the `$24` shot goes into main slot `slot` (16-23), `$80` up and left.
fn spawn_child(body: &Body, work: &Work, round: u8, slot: usize, cx: &mut Cx<'_>) {
    let [x_hi, x_lo] = work.x.to_be_bytes();
    let [y_hi, y_lo] = work.y.to_be_bytes();
    let child = &mut cx.slots.main[slot];
    child.kind = CHILD;
    child.body.x = u16::from_be_bytes([
        x_hi.wrapping_sub(u8::from(x_lo < 0x80)),
        x_lo.wrapping_sub(0x80),
    ]);
    child.body.y = u16::from_be_bytes([
        y_hi.wrapping_add(u8::from(y_lo >= 0x80)),
        y_lo.wrapping_sub(0x80),
    ]);
    child.body.chr_slot = body.chr_slot;
    child.body.state = 0;
    child.body.sprite = 4;
    child.body.phase = 4 + round;
    child.brain = Brain::new(CHILD);
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    fn slot_of(scene: &Scene) -> usize {
        (0..16)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x6C)
            .expect("a live $6C")
    }

    fn only_this(scene: &mut Scene, keep: usize) {
        let wram = &mut scene.machine.bus.wram;
        for slot in 0..16 {
            if slot != keep && wram[0x1CC0 + slot] == 0x6C {
                wram[0x1CC0 + slot] = 0;
            }
        }
    }

    /// What the oracle's `$6C` slots do over `frames`: which sprite indices
    /// they reach, whether the spell is armed, and the `$24` shots in flight.
    #[derive(Default, Debug)]
    struct Seen {
        sprites: BTreeSet<u8>,
        armed: bool,
        resting: bool,
        shots: usize,
    }

    fn observe(mut scene: Scene, frames: u64) -> Seen {
        let mut seen = Seen::default();
        for _ in 0..frames {
            scene.run(1, &[]);
            let wram = &scene.machine.bus.wram;
            for slot in (0..16).filter(|&slot| wram[0x1CC0 + slot] == 0x6C) {
                seen.sprites.insert(wram[0x1C30 + slot]);
                seen.armed |= wram[0x1E10 + slot] != 0;
                seen.resting |= wram[0x1E10 + slot] == 0 && wram[0x1DF0 + slot] & 0x80 != 0;
            }
            seen.shots = seen
                .shots
                .max((16..24).filter(|&slot| wram[0x1CC0 + slot] == 0x24).count());
        }
        seen
    }

    /// Level 3 (area 8): four `$6C` slots idle on screen, never drawn before
    /// their first loop.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(9735) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 300, &[0x6C]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 194,
                occupied: 194,
                drawn: 194,
                phases: BTreeMap::from([(0, 1), (0x80, 371)]),
            },
            "{coverage:?}"
        );
    }

    /// The spell forced on one slot (`$7E10`/`$7E90`), timed so all four `$24`
    /// shots leave inside the frames the oracle brackets cleanly (from 9801): the
    /// sprite cycles, then the rest timer holds the slot.
    #[test]
    fn the_spell_throws_four_shots_then_rests() {
        let Some(mut scene) = Scene::tas(9735) else {
            return;
        };
        let slot = slot_of(&scene);
        only_this(&mut scene, slot);
        let wram = &mut scene.machine.bus.wram;
        wram[0x1E10 + slot] = 0x80;
        wram[0x1E90 + slot] = 0x3C;
        let seen = observe(scene.clone(), 130);
        assert_eq!(seen.sprites, BTreeSet::from([0, 2, 4, 6]), "{seen:?}");
        assert!(seen.armed && seen.resting && seen.shots == 4, "{seen:?}");
        let coverage = assert_actor_phases(scene, 130, &[0x6C]);
        assert_eq!(coverage.occupied, coverage.frames, "{coverage:?}");
        assert_eq!(coverage.drawn, coverage.frames, "{coverage:?}");
    }

    /// A slot that has never been drawn stores when it is off screen.
    #[test]
    fn off_screen_with_a_clear_phase_is_stored() {
        let Some(mut scene) = Scene::tas(9735) else {
            return;
        };
        let slot = slot_of(&scene);
        let wram = &mut scene.machine.bus.wram;
        let camera = u16::from_be_bytes([scene.machine.bus.ram[0xCC], scene.machine.bus.ram[0xCD]]);
        let [hi, lo] = camera.wrapping_add(0x4000).to_be_bytes();
        (wram[0x1B00 + slot], wram[0x1B20 + slot]) = (hi, lo);
        wram[0x1DD0 + slot] = 0;
        let coverage = assert_actor_phases(scene, 30, &[0x6C]);
        assert_eq!(coverage.drawn, 0, "{coverage:?}");
        assert_eq!(coverage.occupied, coverage.frames, "{coverage:?}");
        assert_eq!(
            coverage.phases,
            BTreeMap::from([(0, coverage.frames)]),
            "{coverage:?}"
        );
    }

    /// A drawn slot (`$80`) that leaves the screen frees itself.
    #[test]
    fn off_screen_after_a_draw_frees_the_slot() {
        let Some(mut scene) = Scene::tas(9735) else {
            return;
        };
        let slot = slot_of(&scene);
        let wram = &mut scene.machine.bus.wram;
        let camera = u16::from_be_bytes([scene.machine.bus.ram[0xCC], scene.machine.bus.ram[0xCD]]);
        let [hi, lo] = camera.wrapping_add(0x4000).to_be_bytes();
        (wram[0x1B00 + slot], wram[0x1B20 + slot]) = (hi, lo);
        wram[0x1DD0 + slot] = 0x80;
        let coverage = assert_actor_phases(scene, 30, &[0x6C]);
        assert_eq!(coverage.drawn, 0, "{coverage:?}");
        assert!(coverage.occupied > 0, "{coverage:?}");
    }

    /// Level 7 (area `$18`, TAS 29178): the same enemy, with the spell arming.
    #[test]
    fn matches_the_oracle_in_level_7() {
        let Some(scene) = Scene::tas(29178) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 300, &[0x6C]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 181,
                occupied: 175,
                drawn: 179,
                phases: BTreeMap::from([(0, 2), (0x80, 228)]),
            },
            "{coverage:?}"
        );
    }

    /// The handler's `$A9C4` contact costs the player energy up close.
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(9735) else {
            return;
        };
        let slot = slot_of(&scene);
        only_this(&mut scene, slot);
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
        let coverage = assert_actor_phases(scene, 40, &[0x6C]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }
}
