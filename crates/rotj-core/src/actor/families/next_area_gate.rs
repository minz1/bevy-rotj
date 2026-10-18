//! Type `$32` (bank 11 `$8950`): a screen-transition controller. It never draws.
//!
//! With `$7D90 != 0` it waits until the camera page (`$CC`, or `$CE` on a
//! vertical level) equals its `$7EF0` byte, then plays music `$FC` (`$8986`, `$89B1`) and counts
//! `$7DD0` down from `$C0`; at 0 it arms the transition. With `$7D90 == 0` it
//! arms the transition while the player is within reach (`$A025`), despawns when
//! he is far, and does nothing when he is clear. It calls only `$A000` (`load`),
//! `$A025` (`player_proximity`) and `$CD50` (`despawn`) — all in §9c.

use crate::actor::engine::{self, Proximity, Work};
use crate::actor::slots::Body;
use crate::actor::world::{Cx, Motion, Weapon, attack, hurt, orientation, power_up, sound};
use crate::actors::{CELL_LOCK, CELL_PENDING};

/// The transition selector it arms (`$89CF`).
const TRANSITION: u8 = 3;
/// The countdown's first value (`$898F`).
const COUNTDOWN: u8 = 0xC0;

/// The countdown's end: `$7DD0` reaching it arms the transition.
const ELAPSED: u8 = 0;
/// Type `$32` keeps its whole state in [`Body`] (`$7D90`, `$7DF0`, `$7DD0`,
/// `$7D50`, `$7EF0`/`$7BE0`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8950`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        if cx.effects.player.power_up & power_up::ON != 0
            && cx.byte(0x7BD0 + u16::from(work.slot)) & 0x80 != 0
        {
            return;
        }
        if cx.effects.player.out_of_energy() || cx.effects.player.hurt & hurt::STUNNED != 0 {
            return;
        }
        if body.behaviour != 0 {
            if body.timer & 0x80 != 0 {
                // $8970, $8996: count the `$7DD0` timer down.
                body.phase = body.phase.wrapping_sub(1);
                if body.phase != ELAPSED {
                    cx.effects.scroll_lock = 0xFF;
                    return;
                }
            } else {
                // $8975: wait for the camera page to reach `$7EF0`.
                let camera = if cx.world.orientation & orientation::VERTICAL != 0 {
                    cx.world.camera_y
                } else {
                    cx.world.camera_x
                };
                if body.health.to_be_bytes()[0] != camera.to_be_bytes()[0] {
                    return;
                }
                cx.effects.music = sound::music::FADE_OUT;
                body.timer = 0x80;
                body.phase = COUNTDOWN;
                cx.effects.scroll_lock = 0xFF;
                return;
            }
        } else {
            // $89A2: arm when the player is in reach, despawn when far.
            work.hitbox = 0;
            engine::load(body, work);
            match engine::player_proximity(work, cx) {
                Proximity::Clear => return,
                Proximity::Far => {
                    engine::despawn(kind, body, CELL_PENDING | CELL_LOCK, cx.schedule);
                    return;
                }
                Proximity::Touching => cx.effects.music = sound::music::FADE_OUT,
            }
        }
        transition(body, cx);
    }
}

/// `$89B5`: arm the screen transition (`$0404`, `$040E`/`$040F`, `$BA`, and the
/// weapon/energy carry-over).
fn transition(body: &Body, cx: &mut Cx<'_>) {
    let effects = &mut *cx.effects;
    if body.state & 0x01 != 0 {
        effects.stage_args = [body.subtype, body.health.to_be_bytes()[1]];
        effects.hud = 0x80;
    } else {
        effects.hud = 0;
    }
    effects.transition = TRANSITION;
    if effects.player.power_up & power_up::ON != 0 {
        effects.weapon = effects.stored_weapon;
        effects.attack = if effects.stored_weapon == Weapon::Bolt {
            0
        } else {
            attack::CHARGEABLE
        };
        effects.player.power_up &= !power_up::ON;
    }
    if effects.player.motion == Motion::Flying {
        effects.player.energy = 0x0E;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    use super::COUNTDOWN;

    /// A watched clone of `scene`, run `frames`, gives the writes the oracle made.
    fn oracle_writes(mut scene: Scene, frames: u64, watch: &[(u16, u16)]) -> Vec<(u64, u16, u8)> {
        for &(lo, hi) in watch {
            scene.machine.bus.trace.watch.push(lo..=hi);
        }
        let _ = scene.run(frames, &[]);
        scene
            .machine
            .bus
            .trace
            .writes
            .iter()
            .map(|write| (write.frame, write.addr, write.value))
            .collect()
    }

    /// The `$7D90 != 0` arm: idle, then the `$C0` countdown, then the transition
    /// with `$7D50` bit 0 set (`$040E`/`$040F` and `$0404 = $80`).
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(5810) else {
            return;
        };
        // The transition is inside the covered window (not vacuous).
        let writes = oracle_writes(scene.clone(), 220, &[(0x404, 0x40F)]);
        assert!(writes.contains(&(6028, 0x40E, 0x06)), "{writes:?}");
        assert!(writes.contains(&(6028, 0x40F, 0x00)), "{writes:?}");
        assert!(writes.contains(&(6028, 0x404, 0x80)), "{writes:?}");

        let coverage = assert_actor_phases(scene, 286, &[0x32]);
        // One `$7DD0` value per frame; four loops are dropped where area 2's
        // FME-7 IRQ splits the bracket (§9d-ii).
        let mut phases: BTreeMap<u8, usize> = (1..=COUNTDOWN).map(|phase| (phase, 1)).collect();
        phases.insert(0, 22);
        for gap in [141, 149, 152, 190] {
            phases.remove(&gap);
        }
        assert_eq!(
            coverage,
            Coverage {
                frames: 210,
                occupied: 210,
                drawn: 0,
                phases,
            }
        );
    }

    /// The same countdown with `$7D50` bit 0 clear: the transition writes
    /// `$0404 = 0` and no `$040E`/`$040F`.
    #[test]
    fn matches_the_oracle_from_state_40() {
        let Some(scene) = Scene::tas(11014) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 500, &[0x32]);
        assert_eq!(
            (coverage.frames, coverage.occupied, coverage.drawn),
            (465, 465, 0)
        );
        assert_eq!(coverage.phases.get(&0), Some(&273));
        for phase in 1..=COUNTDOWN {
            assert_eq!(coverage.phases.get(&phase), Some(&1), "phase {phase}");
        }
    }

    /// Union the two lifetimes: every `$7DD0` value 0..`$C0` is exercised.
    #[test]
    fn every_countdown_phase_runs() {
        let Some(first) = Scene::tas(5810) else {
            return;
        };
        let Some(second) = Scene::tas(11014) else {
            return;
        };
        let mut phases: BTreeSet<u8> = assert_actor_phases(first, 286, &[0x32])
            .phases
            .into_keys()
            .collect();
        phases.extend(assert_actor_phases(second, 500, &[0x32]).phases.into_keys());
        assert_eq!(phases, (0..=COUNTDOWN).collect(), "{phases:?}");
    }

    /// `Scene::tas(5810)` with `$0472` bit 7 set and the `$7BD0,x` byte of the
    /// `$32`'s own slot set to `weapon_byte` for each actor loop.
    fn guarded(weapon_byte: u8) -> Option<Scene> {
        let mut scene = Scene::tas(5810)?;
        let slot = (0..24)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x32)
            .expect("the $32 slot");
        let byte = 0x7BD0 + u16::try_from(slot).expect("< 24");
        let pokes = &mut scene.machine.bus.trace.pokes;
        pokes.push((0xD450, 0x0472, 0x80));
        pokes.push((0xD450, byte, weapon_byte));
        pokes.push((0xD456, byte, 0));
        Some(scene)
    }

    /// `$8955`: with `$0472` bit 7 set and its own `$7BD0,x` byte negative, it
    /// returns before the countdown, so `$7DD0` never moves.
    #[test]
    fn a_negative_weapon_byte_holds_it() {
        let Some(scene) = guarded(0x80) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 286, &[0x32]);
        assert_eq!(coverage.occupied, coverage.frames);
        assert!(coverage.frames > 200, "{coverage:?}");
        assert_eq!(coverage.phases.keys().copied().collect::<Vec<_>>(), [0]);
    }

    /// `$8955` falls through on a clear byte: the countdown runs as unguarded.
    #[test]
    fn a_clear_weapon_byte_lets_it_count_down() {
        let Some(scene) = guarded(0) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 286, &[0x32]);
        assert!(coverage.phases.contains_key(&COUNTDOWN), "{coverage:?}");
    }

    /// Put the player `dx` right of the `$32` in slot 0 of `Scene::tas(8495)`,
    /// whose `$7D90` is 0.
    fn near(dx: u16) -> Option<Scene> {
        let mut scene = Scene::tas(8495)?;
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00], wram[0x1B20]]);
        let y = u16::from_be_bytes([wram[0x1B40], wram[0x1B60]]);
        scene.player(x.wrapping_add(dx), y);
        Some(scene)
    }

    /// The `$7D90 == 0` arm: in reach, the transition is armed through `$89B1`.
    #[test]
    fn matches_the_oracle_when_the_player_is_in_reach() {
        let Some(scene) = near(0) else {
            return;
        };
        let writes = oracle_writes(scene.clone(), 6, &[(0xBA, 0xBA)]);
        assert!(
            writes.iter().any(|&(_, a, v)| a == 0xBA && v == 3),
            "{writes:?}"
        );
        let coverage = assert_actor_phases(scene, 110, &[0x32]);
        assert_eq!(
            (coverage.frames, coverage.occupied, coverage.drawn),
            (1, 1, 0)
        );
    }

    /// The `$7D90 == 0` arm: far, the actor is freed (`$89FB`).
    #[test]
    fn despawns_when_the_player_is_far() {
        let Some(scene) = near(0x2000) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 110, &[0x32]);
        assert_eq!((coverage.occupied, coverage.drawn), (1, 0));
        assert!(coverage.frames > 1, "{coverage:?}");
    }

    /// The `$7D90 == 0` arm: clear, the actor does nothing and keeps its bytes.
    #[test]
    fn idles_while_the_player_is_clear() {
        let Some(scene) = near(0x600) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 110, &[0x32]);
        assert_eq!(
            (coverage.frames, coverage.occupied, coverage.drawn),
            (76, 76, 0)
        );
        assert_eq!(coverage.phases.keys().copied().collect::<Vec<_>>(), [0]);
    }
}
