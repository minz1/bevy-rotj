//! Type `$10` (bank 8 `$8B55`): the pickup item the dying animation (`$A56E`)
//! turns a killed actor into. It moves, rides second-class platforms and frees
//! its slot after `$F0` phases.

use crate::actor::collision::{self, Probe};
use crate::actor::engine::{self, Visibility, Work, anchor};
use crate::actor::slots::Body;
use crate::actor::slots::hit;
use crate::actor::world::Cx;

/// Metasprite pointers for the states other than `$04`/`$84` (bank 8 `$8C44`).
const FRAMES: u16 = 0x8C44;
/// Metasprite pointers for state `$04`/`$84` (bank 8 `$8C4E`).
const HIT_FRAMES: u16 = 0x8C4E;
/// The dying animation counts the state up to this value, then wraps to `$80`.
const LAST_STATE: u8 = 0x84;
/// The phase counter at which the actor stops drawing every frame.
const BLINK: u8 = 0xC0;
/// The phase counter at which the actor frees its slot.
const LIFETIME: u8 = 0xF0;
/// The player contact's hit-box index and damage (`$8C2C`: A = 9, stored to `$A5`).
const CONTACT: u8 = 9;

/// Type `$10` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8B55`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if body.hit & hit::DYING != 0 {
            work.y = work.y.wrapping_sub(0x04C0);
            if body.state != LAST_STATE {
                let next = body.state.wrapping_add(1);
                body.state = if next == LAST_STATE { 0x80 } else { next };
            }
            body.hit = 0;
            body.health = 3;
        }
        engine::integrate(body, work);
        let mut probe = Probe::default();
        if collision::feet(work, &mut probe, cx) {
            body.vx_drift = drift(probe);
        } else {
            probe.x = work.x;
            probe.y = work.y;
            if collision::ride(work, &mut probe, cx) {
                body.vx_drift = drift(probe);
            }
        }

        if engine::on_screen(work, cx.world) != Visibility::On {
            *kind = 0;
            return;
        }
        body.phase = body.phase.wrapping_add(1);
        if body.phase >= LIFETIME {
            *kind = 0;
            return;
        }
        if body.phase < BLINK || body.phase & 0x02 != 0 {
            work.tile = 0;
            let (pointer, attr) = frame(cx.prg, body.state, body.phase);
            work.pointer = pointer;
            work.attr = attr;
            engine::draw(work, 1, cx);
        }
        if work.anchor & anchor::BOTTOM == 0 {
            work.hitbox = CONTACT;
            engine::contact(kind, body, work, CONTACT, cx);
        }
        work.age = work.age.wrapping_add(1);
        engine::end_frame(body, work);
    }
}

/// The `$8C44`/`$8C4E` table lookup: the metasprite pointer and attribute bits.
fn frame(prg: &[u8], state: u8, phase: u8) -> (u16, u8) {
    let doubled = state.wrapping_shl(1);
    let index = (phase >> 1) & 0x06;
    if doubled == 0x08 {
        let pointer = engine::rom_word(prg, 8, HIT_FRAMES + u16::from(index));
        (pointer, if index == 2 { 3 } else { 0 })
    } else {
        let y = if index < 4 { 8 } else { doubled };
        let attr = u8::from(index == 6) * 3;
        (engine::rom_word(prg, 8, FRAMES + u16::from(y)), attr)
    }
}

/// The `$27/$28` landing bytes the game stores into `$7E50:$7E60`.
fn drift(probe: Probe) -> i16 {
    i16::from_be_bytes([probe.landing[1], probe.landing[0]])
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::actor::oracle::{Scene, assert_actor_phases};

    /// The hinted scene: the item is on screen for its whole 240-frame life
    /// (`$7DD0` 0 to `$EF`). Area 2 arms the FME-7 IRQ, which fires inside the
    /// actor loop on 162 of those frames; `brackets` drops those, so only 123
    /// loops are compared.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(3968) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 240, &[0x10]);
        assert_eq!(
            (coverage.frames, coverage.occupied, coverage.drawn),
            (123, 123, 115),
            "{coverage:?}"
        );
        assert_eq!(coverage.phases.len(), 123);
        assert_eq!(coverage.phases.keys().next(), Some(&0));
        assert_eq!(coverage.phases.keys().next_back(), Some(&0xEF));
    }

    /// Three lifetimes (area 2 twice, area 0 once). Each diff proves the port;
    /// their non-IRQ brackets union to every value of the `$7DD0` counter, so
    /// every phase the handler can run in is exercised.
    #[test]
    fn every_phase_of_the_counter_runs() {
        let mut phases = BTreeSet::new();
        let mut drawn = 0usize;
        for (frame, frames) in [(3968u64, 240u64), (5254, 230), (1044, 200)] {
            let Some(scene) = Scene::tas(frame) else {
                return;
            };
            let coverage = assert_actor_phases(scene, frames, &[0x10]);
            assert!(coverage.occupied > 0, "scene {frame}: {coverage:?}");
            drawn += coverage.drawn;
            phases.extend(coverage.phases.keys().copied());
        }
        assert!(drawn > 0, "no scene drew the actor");
        let missing: Vec<u8> = (0..=0xEF).filter(|phase| !phases.contains(phase)).collect();
        assert!(missing.is_empty(), "phases not reached: {missing:?}");
    }

    /// `$8B6A`: a hit counts the state up, and `$84` wraps back to `$80` instead
    /// of being stored. Forcing the state each loop is the only way to reach it.
    #[test]
    fn the_dying_state_wraps_from_83_to_80() {
        let Some(mut scene) = Scene::tas(3968) else {
            return;
        };
        let slot = (0..24)
            .find(|&s| scene.machine.bus.wram[0x1CC0 + s] == 0x10)
            .expect("a $10");
        let state = 0x7D50 + u16::try_from(slot).expect("slot fits");
        // Only the state is forced; the item's own hit flag drives the wrap.
        scene.machine.bus.trace.pokes.push((0xD450, state, 0x83));
        let coverage = assert_actor_phases(scene, 4, &[0x10]);
        assert_eq!((coverage.frames, coverage.occupied), (4, 4), "{coverage:?}");
    }

    /// The item's `$7EE0` is `$49`: bit 6 makes `$A9C4` collect it on contact.
    /// A probe run with the player on it frees the slot, proving the path runs.
    #[test]
    fn the_player_collects_the_item() {
        let Some(mut scene) = Scene::tas(3968) else {
            return;
        };
        scene.isolate(&[0x10]);
        // Let the item's own dying frame settle, then put the player on it.
        scene.run(1, &[]);
        let wram = &scene.machine.bus.wram;
        let slot = (0..24).find(|&s| wram[0x1CC0 + s] == 0x10).expect("a $10");
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.player(x, y);
        let mut probe = scene.clone();
        let mut freed = 0;
        for _ in 0..30 {
            probe.run(1, &[]);
            freed += usize::from(probe.machine.bus.wram[0x1CC0 + slot] != 0x10);
        }
        assert!(freed > 0, "the oracle never collected the item");
        let coverage = assert_actor_phases(scene, 30, &[0x10]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }
}
