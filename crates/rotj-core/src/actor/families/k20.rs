//! Type `$20`, the projectile the `$1E` handler fires (`$93F6 JSR $9457`, bank 8
//! `$94B2`).

use crate::actor::collision::{self, Probe};
use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The word at `$9551`, the moving metasprite (bank 1).
const MOVING: u16 = 0xB2C6;
/// The word at `$9553`, the flash after it hits a wall (bank 1).
const FLASH: u16 = 0xB2D0;
/// The x step per frame (`$20`, 1/16 px); `$7C00` bit 6 picks the direction.
const STEP: u16 = 0x20;

/// Type `$20` keeps its whole state in [`Body`]: `$7D50` says moving or flashing,
/// `$7D70` counts the flash. It has no `$7DD0` phase dispatch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$94B2`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if body.state != 0 {
            work.attr = 0;
            body.hit = body.hit.wrapping_add(1);
            if body.hit & 0x08 != 0 {
                *kind = 0;
                return;
            }
            work.pointer = FLASH;
        } else {
            work.attr = body.flags;
            work.x = if body.flags & 0x40 == 0 {
                work.x.wrapping_add(STEP)
            } else {
                work.x.wrapping_sub(STEP)
            };
            let mut probe = Probe {
                x: work.x,
                y: work.y,
                ..Probe::default()
            };
            if collision::solid(&mut probe, 0x84, cx) & 0x80 != 0 {
                body.state = 1;
                body.hit = 0;
                return;
            }
            work.pointer = MOVING;
        }
        if engine::on_screen(work, cx.world) != Visibility::On {
            *kind = 0;
            return;
        }
        engine::tile_bits(work, cx.prg);
        engine::draw(work, 1, cx);
        work.hitbox = 3;
        if engine::touch(work, 2, cx) {
            *kind = 0;
            return;
        }
        engine::store(body, work);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    fn slot_of(scene: &Scene) -> usize {
        (0..32)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x20)
            .expect("a live $20")
    }

    /// Stage 1-2 frames 3844-3963: the `$1E` parent's projectile in slot 22, moving
    /// until it leaves the screen. Its `$7DD0` phase is residue (the handler never
    /// writes it); `$7D50` stays 0, so only the moving path runs.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(3844) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 120, &[0x20]);
        let expected = Coverage {
            frames: 119,
            occupied: 79,
            drawn: 78,
            phases: BTreeMap::from([(3, 79)]),
        };
        assert_eq!(coverage, expected);
    }

    /// `$94F3`: its next step lands in the ground, so it flips to state 1 (`$7D50`
    /// = 1), flashes the `$B2D0` metasprite for seven `$7D70` counts, then despawns.
    #[test]
    fn matches_the_oracle_when_it_hits_a_wall() {
        let Some(mut scene) = Scene::tas(3844) else {
            return;
        };
        let slot = slot_of(&scene);
        let wram = &mut scene.machine.bus.wram;
        (wram[0x1B40 + slot], wram[0x1B60 + slot]) = (0x09, 0x00);
        let mut probe = scene.clone();
        probe.run(1, &[]);
        assert_eq!(
            probe.machine.bus.wram[0x1D50 + slot],
            1,
            "the oracle never flipped to state 1"
        );
        let coverage = assert_actor_phases(scene, 12, &[0x20]);
        assert_eq!(
            (coverage.frames, coverage.occupied, coverage.drawn),
            (12, 9, 7),
            "{coverage:?}"
        );
    }

    /// `$951D`: the projectile touches the player (`$AEE3`) and is freed on contact.
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(3844) else {
            return;
        };
        let slot = slot_of(&scene);
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.player(x, y);
        let mut probe = scene.clone();
        let mut energy = probe.machine.bus.ram[0x478];
        let mut hits = 0;
        for _ in 0..6 {
            probe.run(1, &[]);
            let now = probe.machine.bus.ram[0x478];
            if now < energy {
                hits += 1;
                energy = now;
            }
        }
        assert!(hits > 0, "the oracle takes no contact hit");
        let coverage = assert_actor_phases(scene, 12, &[0x20]);
        assert!(coverage.occupied < 12, "{coverage:?}");
    }
}
