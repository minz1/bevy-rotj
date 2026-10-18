//! Type `$70` (bank 0 `$89B1`): the continuation the stage 3/9 `$6E` turns its
//! slot into when it dies (`$8951`), drifting left on the `$6E`'s stale
//! `vx_drift`. A lethal hit does not kill it: `$89B4` restores its hit points,
//! steps `$7D50` (which picks the metasprite) and it lives on.
//!
//! It calls only routines already in `docs/ACTORS.md` §9c: `$A000` (`load`),
//! `$A79B` (`integrate`), `$A852` (`on_screen`), `$A9C4` (`contact`) and `$A934`
//! (`store`). `$84DF` maps bank 1 at `$A000` around the draw, so the port passes
//! bank 1 to `engine::draw`. The handler ends a frame with `$A934`, never
//! `$A8E4`: it has no gravity, and `$7DD0` is a lifetime counter, not a phase
//! flag, so the slot frees after `$B0` frames whether drawn or not.

use crate::actor::engine::{self, Visibility, Work, anchor};
use crate::actor::slots::Body;
use crate::actor::slots::hit;
use crate::actor::world::Cx;

/// The metasprite pointer table the handler indexes by `$7D50` (`$8A6B`).
const METASPRITES: u16 = 0x8A6B;
/// The bank `$84DF` maps at `$A000` for the draw.
const DRAW_BANK: u8 = 1;
/// The hit box the contact uses (`$8A51`).
const HITBOX: u8 = 9;
/// The `$A9C4` argument, passed in A as well as to `$A5` (`$8A51`).
const CONTACT: u8 = 9;
/// `$7DD0` counts to this before the slot frees (`$8A07`).
const LIFETIME: u8 = 0xB0;
/// The hit reaction's restored hit points (`$89DD`: `$7EF0`=0, `$7BE0`=3).
const RESTORED_HEALTH: u16 = 3;
/// The hit reaction's drop-back on y (`$89B9`).
const HIT_DROP: u16 = 0x04C0;

/// The lifetime count from which it draws only on frames where bit 1 is set.
const FLICKER: u8 = 0x80;
/// Type `$70` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$89B1`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        // `$89B4`: a lethal hit drops the actor 76 px, advances `$7D50` (wrapping
        // `$84` to `$80`) and restores its hit points instead of killing it.
        if body.hit & hit::DYING != 0 {
            work.y = work.y.wrapping_sub(HIT_DROP);
            let next = body.state.wrapping_add(1);
            body.state = if next == 0x84 { 0x80 } else { next };
            body.hit = 0;
            body.health = RESTORED_HEALTH;
        }
        engine::integrate(body, work);
        body.phase = body.phase.wrapping_add(1);
        if engine::on_screen(work, cx.world) != Visibility::On {
            // `$89EA`: off screen it only counts down and stores, never draws.
            if body.phase >= LIFETIME {
                *kind = 0;
                return;
            }
            store(body, work);
            return;
        }
        if body.phase >= LIFETIME {
            *kind = 0;
            return;
        }
        // `$8A07`: below `$80` it draws every frame; in `$80-$AF` only when the
        // counter's bit 1 is set, storing (and contacting) otherwise.
        if body.phase < FLICKER || body.phase & 0x02 != 0 {
            draw(body, work, cx);
        }
        if work.anchor & anchor::BOTTOM == 0 {
            work.hitbox = HITBOX;
            engine::contact(kind, body, work, CONTACT, cx);
        }
        store(body, work);
    }
}

/// `$8A1B`: picks the metasprite by `$7D50` and, on the draw frames the counter
/// selects, by `$7DD0`'s bits 1-3; then draws it from bank 1.
fn draw(body: &Body, work: &mut Work, cx: &mut Cx<'_>) {
    work.tile = 0;
    let mut index = body.state.wrapping_shl(1);
    let attr = match (body.phase >> 1) & 0x06 {
        0 | 2 => {
            index = 8;
            0
        }
        6 => 3,
        _ => 0,
    };
    work.attr = attr;
    work.pointer = engine::rom_word(cx.prg, 0, METASPRITES + u16::from(index));
    engine::draw(work, DRAW_BANK, cx);
}

/// `$8A58`: bumps the age then stores (`$A934`).
fn store(body: &mut Body, work: &mut Work) {
    work.age = work.age.wrapping_add(1);
    engine::store(body, work);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::actor::oracle::{Scene, assert_actor_phases};

    /// The slot `$70` occupies after isolation.
    fn slot_of(scene: &Scene) -> usize {
        (0..24)
            .find(|&s| scene.machine.bus.wram[0x1CC0 + s] == 0x70)
            .expect("a $70")
    }

    /// The stage 3 spawn: the `$6E` dies and hands its slot to `$70`, which drifts
    /// left through its whole `$7DD0` counter (0 to `$AF`), drawn both every frame
    /// and on the counter's odd frames, and killed once by the player's weapon
    /// (which the `$89B4` reaction turns into a `$7D50` step, not a despawn).
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(8884) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 300, &[0x70]);
        assert_eq!(
            (coverage.frames, coverage.occupied, coverage.drawn),
            (260, 138, 113),
            "{coverage:?}"
        );
        assert_eq!(coverage.phases.keys().next(), Some(&0), "{coverage:?}");
        assert_eq!(coverage.phases.keys().next_back(), Some(&0xAF));
    }

    /// The level 9 spawn: the same family, reached from the `$6E` of area 18.
    #[test]
    fn matches_the_oracle_in_level_9() {
        let Some(scene) = Scene::tas(28978) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 300, &[0x70]);
        assert_eq!(
            (coverage.frames, coverage.occupied, coverage.drawn),
            (123, 41, 37),
            "{coverage:?}"
        );
        assert!(coverage.drawn > 0, "{coverage:?}");
    }

    /// The two scenes' non-interrupted brackets leave 34 counter values unseen
    /// (an IRQ or NMI drops the bracket on those frames). Forcing `$7DD0` to each
    /// value in turn diffs every phase the handler can run in, including the
    /// free at `$B0`.
    #[test]
    fn every_phase_of_the_counter_runs() {
        let Some(mut base) = Scene::tas(8884) else {
            return;
        };
        base.isolate(&[0x70]);
        let slot = slot_of(&base);
        let phase = 0x7DD0 + u16::try_from(slot).expect("slot fits");
        let mut covered = BTreeSet::new();
        for value in 0..=0xAF {
            let mut probe = base.clone();
            probe.machine.bus.trace.pokes.push((0xD450, phase, value));
            let coverage = assert_actor_phases(probe, 1, &[0x70]);
            covered.extend(coverage.phases.keys().copied());
        }
        let missing: Vec<u8> = (0..=0xAF)
            .filter(|value| !covered.contains(value))
            .collect();
        assert!(missing.is_empty(), "phases not reached: {missing:?}");
    }

    /// `$89C6`: a hit counts `$7D50` up and wraps `$83` to `$80` instead of
    /// storing `$84`. The counter's own path only reaches `$81`/`$82`, so the
    /// wrap is forced. The diff also proves the four-hit reaction itself.
    #[test]
    fn the_hit_reaction_wraps_the_state_from_83_to_80() {
        let Some(mut scene) = Scene::tas(8884) else {
            return;
        };
        scene.isolate(&[0x70]);
        let slot = slot_of(&scene);
        scene.machine.bus.trace.pokes.push((
            0xD450,
            0x7D50 + u16::try_from(slot).expect("slot"),
            0x83,
        ));
        scene.machine.bus.trace.pokes.push((
            0xD450,
            0x7D70 + u16::try_from(slot).expect("slot"),
            0x80,
        ));
        let mut probe = scene.clone();
        probe.run(1, &[]);
        assert_eq!(
            probe.machine.bus.wram[0x1D50 + slot],
            0x80,
            "the oracle did not wrap the state"
        );
        let coverage = assert_actor_phases(scene, 2, &[0x70]);
        assert_eq!(coverage.frames, coverage.occupied, "{coverage:?}");
    }

    /// `$7EE0` is `$49`: `$A9C4` collects it on contact rather than damaging the
    /// player. A probe with the player on it frees the slot.
    #[test]
    fn the_player_collects_it_on_contact() {
        let Some(mut scene) = Scene::tas(8884) else {
            return;
        };
        scene.isolate(&[0x70]);
        scene.run(1, &[]);
        let slot = slot_of(&scene);
        assert_eq!(scene.machine.bus.wram[0x1EE0 + slot], 0x49);
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.player(x, y);
        let mut probe = scene.clone();
        let mut freed = 0;
        for _ in 0..4 {
            probe.run(1, &[]);
            freed += usize::from(probe.machine.bus.wram[0x1CC0 + slot] != 0x70);
        }
        assert!(freed > 0, "the oracle never collected the $70");
        let coverage = assert_actor_phases(scene, 4, &[0x70]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }
}
