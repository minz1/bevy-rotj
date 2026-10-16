//! Type `$62` (bank 0 `$8580`): a falling, drifting piece that starts dying when its feet find ground.
//!
//! It calls only routines already in `docs/ACTORS.md` §9c: `$A000` (`load`), `$A56E`
//! (`dying`), `$A79B` (`integrate`), `$A7DD` (`collision::feet`), `$A852` (`on_screen`),
//! `$84CA` -> `$C247` (`draw`, bank 9), `$AEE3` (`touch`) and `$A8E4` (`end_frame`).

use crate::actor::collision::{self, Probe};
use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The metasprite pointer the handler reads from bank 0 `$8604`.
const METASPRITE_PTR: u16 = 0x8604;
/// The bank `$84CA` maps at `$A000` for the draw.
const DRAW_BANK: u8 = 9;
/// The x drift added to each step (`$7E50:$7E60`).
const DRIFT: i16 = -0x20;
/// The level y high byte from which the piece is freed.
const FLOOR_PAGE: u8 = 0x0F;
/// What the landing adds to the stored y (1/16 px).
const LANDING_DROP: u16 = 0x04C0;
/// The sound request on landing.
const LANDING_SOUND: u8 = 0x22;
/// Contact damage.
const DAMAGE: u8 = 2;

/// Type `$62` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8580`; no `$7DD0` dispatch).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if engine::dying(kind, body, work, 0, cx) {
            return;
        }
        body.vx_drift = DRIFT;
        engine::integrate(body, work);
        work.vy = work.vy.wrapping_sub(3);
        if work.y.to_be_bytes()[0] >= FLOOR_PAGE {
            *kind = 0;
            return;
        }
        let mut probe = Probe::default();
        if collision::feet(work, &mut probe, cx) & 0x80 == 0 {
            cx.effects.sound[0] = LANDING_SOUND;
            body.y = body.y.wrapping_add(LANDING_DROP);
            body.hit = 0x80;
            body.dying_timer = 0;
            return;
        }
        if engine::on_screen(work, cx.world) == Visibility::On {
            work.tile = 0;
            work.attr = 3;
            work.pointer = engine::rom_word(cx.prg, 0, METASPRITE_PTR);
            engine::draw(work, DRAW_BANK, cx);
            work.hitbox = 0;
            engine::touch(work, DAMAGE, cx);
        }
        engine::end_frame(body, work);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    fn slots(scene: &Scene) -> Vec<usize> {
        (0..16)
            .filter(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x62)
            .collect()
    }

    /// Slots 8-11 fall and drift, land (the `$7D70` dying flag, sound `$22`), die and free.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(12022) else {
            return;
        };
        let mut probe = scene.clone();
        let (mut landed, mut freed) = (0, 0);
        let mut live = slots(&probe).len();
        for _ in 0..400 {
            probe.run(1, &[]);
            landed += slots(&probe)
                .iter()
                .filter(|&&slot| probe.machine.bus.wram[0x1D70 + slot] & 0x80 != 0)
                .count();
            let now = slots(&probe).len();
            freed += live.saturating_sub(now);
            live = now;
        }
        assert!(landed > 0 && freed > 0, "landed {landed}, freed {freed}");
        let coverage = assert_actor_phases(scene, 400, &[0x62]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 400,
                occupied: 100,
                drawn: 78,
                phases: BTreeMap::from([(0, 100)]),
            },
            "{coverage:?}"
        );
    }

    /// A piece below the last map page is freed (`$85A7`).
    #[test]
    fn below_the_map_it_is_freed() {
        let Some(mut scene) = Scene::tas(12022) else {
            return;
        };
        scene.isolate(&[0x62]);
        let slot = slots(&scene)[0];
        scene.machine.bus.wram[0x1B40 + slot] = 0x0F;
        let coverage = assert_actor_phases(scene, 2, &[0x62]);
        assert_eq!(coverage.occupied, 1, "{coverage:?}");
        assert_eq!(coverage.drawn, 0, "{coverage:?}");
    }

    /// The `$AEE3` touch costs the player energy up close.
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(12022) else {
            return;
        };
        scene.isolate(&[0x62]);
        let slot = slots(&scene)[0];
        scene.run(30, &[]);
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.player(x, y);
        let mut probe = scene.clone();
        let energy = probe.machine.bus.ram[0x478];
        probe.run(5, &[]);
        assert!(
            probe.machine.bus.ram[0x478] < energy,
            "the oracle takes no contact hit"
        );
        let coverage = assert_actor_phases(scene, 5, &[0x62]);
        assert_eq!(coverage.occupied, coverage.frames, "{coverage:?}");
        assert!(coverage.drawn > 0, "{coverage:?}");
    }
}
