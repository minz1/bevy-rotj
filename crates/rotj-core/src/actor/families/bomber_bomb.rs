//! Type `$2C` (bank 11 `$851A`): the bomb the `$2A` bomber drops. It falls until
//! it reaches ground or a platform, then plays a three-step blast animation and
//! frees its slot.
//!
//! It calls only routines already in `docs/ACTORS.md` §9c: `$A000` (`load`),
//! `$A79B` (`integrate`), `$AF7D` (`collision::below`), `$AF6A` (`collision::ride_here`),
//! `$A852` (`on_screen`), `$AC06` (`tile_bits`), `$C247` (`draw`, through `$825A`'s
//! bank 1 map) and `$AEE3` (`touch`).

use crate::actor::collision::{self, Probe};
use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

const SOURCE_BANK: u8 = 11;
const DRAW_BANK: u8 = 1;
const FALLING_SPRITES: u16 = 0x859F;
const BLAST_SPRITES: u16 = 0x85A1;
const EXPLODING: u8 = 0x80;
const BLAST_STEPS: u8 = 6;
const HITBOX: u8 = 3;
const DAMAGE: u8 = 4;

/// Type `$2C` keeps its whole state in [`Body`]: `phase` (`$7DD0`) is `$80` once it
/// blasts and `timer` (`$7DF0`) counts the blast steps.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$851A`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        work.attr = 0;
        engine::load(body, work);
        engine::integrate(body, work);
        if body.phase & EXPLODING == 0 && landed(work, cx) {
            body.phase = EXPLODING;
            body.timer = 0;
            body.vy = 0;
        }
        if engine::on_screen(work, cx.world) != Visibility::On {
            *kind = 0;
            return;
        }
        let sprites = if body.phase & EXPLODING == 0 {
            FALLING_SPRITES
        } else if body.timer < BLAST_STEPS {
            let step = BLAST_SPRITES + u16::from(body.timer & 0x0E);
            body.timer += 1;
            step
        } else {
            *kind = 0;
            return;
        };
        work.pointer = engine::rom_word(cx.prg, SOURCE_BANK, sprites);
        engine::tile_bits(work, cx.prg);
        engine::draw(work, DRAW_BANK, cx);
        work.hitbox = HITBOX;
        if engine::touch(work, DAMAGE, cx) {
            *kind = 0;
            return;
        }
        body.x = work.x;
        body.y = work.y;
    }
}

/// `$8529`: solid ground just below, or a second-class platform to stand on.
fn landed(work: &mut Work, cx: &mut Cx<'_>) -> bool {
    let mut probe = Probe::default();
    collision::below(work, &mut probe, cx) || collision::ride_here(work, &mut probe, cx)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};
    use crate::actors::SpawnRecord;

    /// A `$2C` written the way `$2A`'s `$84BD` does: byte 5 `$16` -> kind `$2C`.
    fn bomb(x: u16, y: u16) -> SpawnRecord {
        SpawnRecord {
            x,
            y,
            kind: 0x2C,
            state: 0x40,
            ..SpawnRecord::default()
        }
    }

    /// The published TAS's bomber (`$2A`, area 12:02) drops a bomb whose real
    /// landing is a *ride* on a `$0C` second-class platform, whose `$CE3E` ride
    /// handler is unported (§9f), so `isolate` removes both; the falling phase,
    /// the `ride_here` miss and the off-screen free are still compared.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(21223) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 200, &[0x2C]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 200,
                occupied: 54,
                drawn: 42,
                phases: BTreeMap::from([(0, 54)]),
            }
        );
    }

    /// A `$2C` set down over stage 1-1's solid floor: `$8529`/`$852C` land it
    /// (`below`, the `$8533` branch), then the whole six-step blast runs and
    /// `$859C` frees the slot.
    #[test]
    fn lands_and_blasts_on_solid_ground() {
        let Some(scene) = ground_bomb(0x400) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 40, &[0x2C]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 40,
                occupied: 16,
                drawn: 15,
                phases: BTreeMap::from([(0, 10), (0x80, 6)]),
            }
        );
    }

    /// `$AEE3`: standing on the bomb costs the player health (and frees it).
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = ground_bomb(0) else {
            return;
        };
        let ram = &scene.machine.bus.ram;
        let x = u16::from_be_bytes([ram[0x460], ram[0x461]]);
        let y = u16::from_be_bytes([ram[0x462], ram[0x463]]);
        scene.player(x, y.wrapping_sub(0x40));
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
        let coverage = assert_actor_phases(scene, 6, &[0x2C]);
        assert_eq!(coverage.occupied, 1);
    }

    /// The stage-1-1 scene with a `$2C` set down `dx`/16 px right of the player,
    /// four px above his feet.
    fn ground_bomb(dx: u16) -> Option<Scene> {
        let mut scene = Scene::route(700)?;
        let ram = &scene.machine.bus.ram;
        let x = u16::from_be_bytes([ram[0x460], ram[0x461]]);
        let y = u16::from_be_bytes([ram[0x462], ram[0x463]]);
        scene.place(1, &bomb(x.wrapping_add(dx), y.wrapping_sub(0x40)), 0x1E);
        Some(scene)
    }
}
