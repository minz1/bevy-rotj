//! Type `$6E`, a stage 3/9 enemy (bank 0 `$891D`).

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The even/odd metasprite pair the handler reads at `$89AD` (the `$F1` bit 3
/// picks one); `$84CA` maps bank 9 at `$A000` for the draw.
const METASPRITES: u16 = 0x89AD;
const SPRITE_BANK: u8 = 9;
/// The left drift the handler re-arms in `$7E50:$7E60` every frame.
const DRIFT: i16 = -32;
/// The kind the dying path turns it into (`$8951`); its handler is bank 0 `$89B1`.
const CONTINUATION: u8 = 0x70;

/// Type `$6E` keeps its whole state in [`Body`]. `$7DD0` is only an "it was on
/// screen" flag (`$80`), read back by the shared off-screen tail at `$8908`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$891D`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        body.vx_drift = DRIFT;
        body.vy = 0;
        // `$A56E` with A = 0 (the hit flash, or dying and becoming item `$10`).
        if engine::dying(kind, body, work, 0, cx) {
            if *kind == 0x10 {
                body.y = body.y.wrapping_sub(0x04C0);
                *kind = CONTINUATION;
                body.vy = 0;
                body.hit = 0;
                body.health = 4;
            }
            return;
        }
        work.attr |= body.flags;
        engine::integrate(body, work);
        if engine::on_screen(work, cx.world) != Visibility::On {
            // `$8908`: once it has been drawn, leaving the screen frees it.
            if body.phase & engine::DRAWN != 0 {
                *kind = 0;
            } else {
                engine::store(body, work);
            }
            return;
        }
        body.phase = engine::DRAWN;
        let index = 2 * u16::from(cx.world.frame & 0x08 != 0);
        work.pointer = engine::rom_word(cx.prg, 0, METASPRITES + index);
        engine::tile_bits(work, cx.prg);
        engine::draw(work, SPRITE_BANK, cx);
        work.hitbox = 2;
        engine::contact(kind, body, work, 2, cx);
        engine::store(body, work);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    fn slot_of(scene: &Scene) -> usize {
        (0..32)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x6E)
            .expect("a live $6E")
    }

    fn camera_x(scene: &Scene) -> u16 {
        u16::from_be_bytes([scene.machine.bus.ram[0xCC], scene.machine.bus.ram[0xCD]])
    }

    /// Keeps every slot alive: a `$6E` death hands its slot to type `$70`, a
    /// different family (bank 0 `$89B1`) this task does not port.
    fn keep_alive(scene: &mut Scene) {
        for slot in 0..16u16 {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, 0x7BE0 + slot, 0x40));
        }
    }

    /// The stage 3 spawn at frame 8864: drawn while on screen, drifting left, then
    /// freed once it has left the view.
    #[test]
    fn matches_the_oracle() {
        let Some(mut scene) = Scene::tas(8863) else {
            return;
        };
        keep_alive(&mut scene);
        let coverage = assert_actor_phases(scene, 300, &[0x6E]);
        // The alive span covers phase `$80` (drawn) only; phase 0 is the off-screen
        // test below.
        let expected = Coverage {
            frames: 239,
            occupied: 34,
            drawn: 33,
            phases: BTreeMap::from([(0x80, 34)]),
        };
        assert_eq!(coverage, expected);
    }

    /// `$8908` phase 0: never on screen, it stores each frame and stays alive.
    #[test]
    fn off_screen_keeps_the_slot_undrawn() {
        let Some(mut scene) = Scene::tas(8863) else {
            return;
        };
        scene.isolate(&[0x6E]);
        let [hi, lo] = (camera_x(&scene) + 500 * 16).to_be_bytes();
        for slot in 0..16u16 {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, 0x7B00 + slot, hi));
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, 0x7B20 + slot, lo));
        }
        let coverage = assert_actor_phases(scene, 40, &[0x6E]);
        assert_eq!(coverage.drawn, 0, "{coverage:?}");
        assert_eq!(coverage.occupied, coverage.frames, "{coverage:?}");
        assert_eq!(coverage.phases, BTreeMap::from([(0, coverage.frames)]));
    }

    /// `$AB13` kills it, then `$891D`'s tail repurposes the slot as type `$70`
    /// with four hit points, 76 px lower. The frame it happens is diffed too.
    #[test]
    fn the_death_hands_the_slot_to_type_70() {
        let Some(mut scene) = Scene::tas(8863) else {
            return;
        };
        scene.isolate(&[0x6E]);
        let mut probe = scene.clone();
        let death = (1..=60)
            .find(|_| {
                probe.run(1, &[]);
                (0..32).any(|slot| probe.machine.bus.wram[0x1CC0 + slot] == 0x70)
            })
            .expect("the player never kills the $6E");
        let coverage = assert_actor_phases(scene, death, &[0x6E]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }

    /// It cannot touch the player: `$7EE0` (`$40`) gates the player check off in
    /// `$A9C4`, so only [`matches_the_oracle`]'s weapon hits exercise contact.
    #[test]
    fn contact_is_only_against_weapons() {
        let Some(scene) = Scene::tas(8864) else {
            return;
        };
        let slot = slot_of(&scene);
        assert_eq!(scene.machine.bus.wram[0x1EE0 + slot], 0x40);
    }
}
