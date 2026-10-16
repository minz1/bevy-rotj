//! Type `$50`, the boss-beaten stage script (bank 11 `$9C13`): each frame it
//! forces the player's scripted state and clears the projectiles, and every
//! fourth frame it spawns a `$52` at an offset around the slot in `$7DF0`
//! (`$9C67`). After a 192-frame countdown it arms the stage end (`$BA` 3), or
//! frees its own slot in the last area. It never loads the working set, draws or
//! collides.

use crate::actor::engine::{self, Work};
use crate::actor::slots::Body;
use crate::actor::world::{Cx, Motion};

const CHILD: u8 = 0x52;
const COUNTDOWN: u8 = 0xC0;
const FINAL_AREA: u8 = 0x22;
const SPAWN_SOUND: u8 = 0x23;
/// The `$52` spawn deltas (`$9CE4`/`$9CEA` x, `$9CF0`/`$9CF6` y), by `$7E10`.
const X_LO: u16 = 0x9CE4;
const X_HI: u16 = 0x9CEA;
const Y_LO: u16 = 0x9CF0;
const Y_HI: u16 = 0x9CF6;

/// Type `$50` keeps no state of its own: the countdown is in [`Body::anim`], the
/// spawn offset in [`Body::counter`] and the source slot in [`Body::timer`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9C13`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, _work: &mut Work, cx: &mut Cx<'_>) {
        cx.effects.player.motion = Motion::Held;
        cx.effects.player.flags[2] = 0x40;
        cx.effects.player.stage_script = 0x80;
        cx.effects.attack &= 0x80;
        cx.effects.misc_flag = 0;
        for weapon in &mut cx.effects.weapons {
            weapon.kind = 0;
        }
        if body.anim.is_multiple_of(4) {
            spawn_52(body, cx);
        }
        body.anim = body.anim.wrapping_add(1);
        if body.anim < COUNTDOWN {
            return;
        }
        if cx.world.area == FINAL_AREA {
            *kind = 0;
        } else {
            cx.effects.player.energy = 0x0E;
            cx.effects.cutscene_id = 0x0E;
            cx.effects.hud = 0;
            cx.effects.transition = 3;
        }
    }
}

/// `$9C67`: spawn a `$52` next to the source slot; always advances the counter.
fn spawn_52(body: &mut Body, cx: &mut Cx<'_>) {
    if body.anim & 0x1C == 0 {
        cx.effects.sound[0] = SPAWN_SOUND;
    }
    if let Some((child, x, y)) = child_slot(body, cx) {
        let [x_hi, x_lo] = x.to_be_bytes();
        let [y_hi, y_lo] = y.to_be_bytes();
        for (array, value) in [
            (0x7E90, 0),
            (0x7C90, 0x04),
            (0x7CC0, CHILD),
            (0x7B20, x_lo),
            (0x7B00, x_hi),
            (0x7B60, y_lo),
            (0x7B40, y_hi),
        ] {
            cx.set_byte(array + u16::try_from(child).expect("< 16"), value);
        }
    }
    body.counter = body.counter.wrapping_add(1);
    if body.counter >= 6 {
        body.counter = 0;
    }
}

/// The free slot and position the child takes, or `None` when the offset leaves
/// the map (`$9C8A`/`$9C9F`) or no slot is free (`$9CAD`).
fn child_slot(body: &Body, cx: &Cx<'_>) -> Option<(usize, u16, u16)> {
    let index = u16::from(body.counter);
    let source = u16::from(body.timer);
    let x = word(cx, 0x7B00, source).wrapping_add(delta(cx.prg, X_LO, X_HI, index));
    let y = word(cx, 0x7B40, source).wrapping_add(delta(cx.prg, Y_LO, Y_HI, index));
    if x.to_be_bytes()[0] & 0xF0 != 0 || y.to_be_bytes()[0] & 0xF0 != 0 {
        return None;
    }
    let child = (0..16).rev().find(|&slot| cx.slots.main[slot].kind == 0)?;
    Some((child, x, y))
}

/// A slot's 16-bit array pair (`high` at `base`, `low` at `base + $20`).
fn word(cx: &Cx<'_>, base: u16, slot: u16) -> u16 {
    u16::from_be_bytes([cx.byte(base + slot), cx.byte(base + 0x20 + slot)])
}

/// One `$9CE4`-style delta, low byte at `lo`, high byte at `hi`.
fn delta(prg: &[u8], lo: u16, hi: u16, index: u16) -> u16 {
    (u16::from(engine::rom_byte(prg, 11, hi + index)) << 8)
        | u16::from(engine::rom_byte(prg, 11, lo + index))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};
    use crate::actor::slots::Slots;

    const FIFTY: u8 = 0x50;
    /// Slot 0's `$7E90` (the countdown), `$7D50` (state) and `$7DD0` (phase).
    const ANIM: u16 = 0x7E90;
    const STATE: u16 = 0x7D50;
    const PHASE: u16 = 0x7DD0;

    fn poke(scene: &mut Scene, addr: u16, value: u8) {
        scene.machine.bus.trace.pokes.push((0xD450, addr, value));
    }

    /// Frees every slot but slot 0 each loop, so the spawned `$52` never runs.
    fn free_others(scene: &mut Scene) {
        for slot in 1..24u16 {
            poke(scene, 0x7CC0 + slot, 0);
        }
    }

    /// Moves the scene's `$50` to slot 0, where the `$52` it spawns (into slot
    /// 15) runs before it and is freed at `$D450` the next frame.
    fn to_slot_zero(scene: &mut Scene) {
        let wram = &mut scene.machine.bus.wram;
        let mut slots = Slots::lift(wram);
        let source = slots
            .main
            .iter()
            .position(|slot| slot.kind == FIFTY)
            .expect("a $50");
        slots.main.swap(0, source);
        slots.project(wram);
    }

    /// The stage 1 boss room just after the boss spawns the `$50` (frame 6576,
    /// area 04:00, slot 15, `timer` = the boss's slot 1), isolated and moved.
    fn prepared(frame: u64) -> Option<Scene> {
        let mut scene = Scene::tas(frame)?;
        scene.isolate(&[FIFTY]);
        to_slot_zero(&mut scene);
        free_others(&mut scene);
        Some(scene)
    }

    /// The whole every-frame body: the player script, the projectile clear, the
    /// countdown and the offsets — anim 1-30 spawns counter 0-5 at least once.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = prepared(6576) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 30, &[FIFTY]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 30,
                occupied: 30,
                drawn: 0,
                phases: BTreeMap::from([(0, 30)]),
            }
        );
    }

    /// Makes every byte the handler rewrites distinctive first, so the diff is
    /// not vacuous: the motion, flags, `$0481`, `$B8` and the weapons.
    #[test]
    fn rewrites_the_player_and_clears_the_weapons() {
        let Some(mut scene) = prepared(6576) else {
            return;
        };
        for (addr, value) in [
            (0x046E, 0x05),
            (0x047A, 0x77),
            (0x0481, 0x77),
            (0x00B8, 0xFF),
        ] {
            poke(&mut scene, addr, value);
        }
        for slot in 0..16u16 {
            poke(&mut scene, 0x7BD0 + slot, 0x77);
        }
        let coverage = assert_actor_phases(scene, 4, &[FIFTY]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 4,
                occupied: 4,
                drawn: 0,
                phases: BTreeMap::from([(0, 4)]),
            }
        );
    }

    /// The countdown arm: anim `$C0` sets energy `$0E`, `$0406` `$0E`, `$0404`
    /// `0` and `$BA` 3, all of which the loop diff pins.
    #[test]
    fn arms_the_level_end_at_the_countdown() {
        let Some(mut scene) = prepared(6576) else {
            return;
        };
        poke(&mut scene, ANIM, 0xBF);
        let coverage = assert_actor_phases(scene, 1, &[FIFTY]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 1,
                occupied: 1,
                drawn: 0,
                phases: BTreeMap::from([(0, 1)]),
            }
        );
    }

    /// In the last area (22:00) the same arm frees the slot instead.
    #[test]
    fn frees_itself_in_the_last_area() {
        let Some(mut scene) = prepared(6576) else {
            return;
        };
        poke(&mut scene, 0x0400, 0x22);
        poke(&mut scene, ANIM, 0xBF);
        let coverage = assert_actor_phases(scene, 2, &[FIFTY]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 2,
                occupied: 1,
                drawn: 0,
                phases: BTreeMap::from([(0, 1)]),
            }
        );
    }

    /// When an offset leaves the map (`$9C8A`/`$9C9F`) the child is skipped but
    /// the counter still advances; a zeroed source slot makes every offset bail.
    #[test]
    fn bails_when_the_offset_leaves_the_map() {
        let Some(mut scene) = prepared(6576) else {
            return;
        };
        for array in [0x7B00, 0x7B20, 0x7B40, 0x7B60] {
            poke(&mut scene, array + 1, 0);
        }
        let coverage = assert_actor_phases(scene, 30, &[FIFTY]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 30,
                occupied: 30,
                drawn: 0,
                phases: BTreeMap::from([(0, 30)]),
            }
        );
    }

    /// `$7D50`/`$7DD0` are residue, not a dispatch: the inventory saw `$7D50`
    /// `00`/`64`/`A1`/`FF` and `$7DD0` `00`/`5E`/`FF`, and the handler ignores
    /// every one of them.
    #[test]
    fn every_hinted_state_is_ignored() {
        for state in [0x00, 0x64, 0xA1, 0xFF] {
            for phase in [0x00, 0x5E, 0xFF] {
                let Some(mut scene) = prepared(6576) else {
                    return;
                };
                poke(&mut scene, STATE, state);
                poke(&mut scene, PHASE, phase);
                let coverage = assert_actor_phases(scene, 4, &[FIFTY]);
                assert_eq!(
                    coverage,
                    Coverage {
                        frames: 4,
                        occupied: 4,
                        drawn: 0,
                        phases: BTreeMap::from([(phase, 4)]),
                    },
                    "state {state:02X} phase {phase:02X}"
                );
            }
        }
    }
}
