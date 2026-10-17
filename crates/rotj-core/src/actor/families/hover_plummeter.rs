//! Type `$9E` (bank 12 `$8309`): the green enemy of stage 6 (area 1C:00) that
//! hovers, wakes when the player comes within `$300`, then falls and bursts into
//! four type-`$08` parts the first time its feet find ground.
//!
//! It calls only routines already in `docs/ACTORS.md` §9c: `$A000` (`load`),
//! `$A79B` (`integrate`), `$ABDF` (`player_dx`), `$A7DD` (`collision::feet`),
//! `$AC16` (`spawn_parts`), `$A852` (`on_screen`), `$C247` (`draw`, which `$81C6`
//! maps bank 9 at `$A000`), `$A9C4` (`contact`) and `$A8E4` (`end_frame`).

use crate::actor::collision::{self, Probe};
use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The metasprite pointer (`$83CB`): bank 9's `$A946`.
const METASPRITE: u16 = 0xA946;
/// `$81C6` maps bank 9 at `$A000` around the draw.
const DRAW_BANK: u8 = 9;
/// Hover velocity (`$8321`): `$FFFA`.
const HOVER: i16 = -6;
/// Wake when the player is within `$0300` (`$832C`).
const WAKE_DISTANCE: u16 = 0x300;
/// Row past which a falling actor is freed (`$8341`).
const FALL_LIMIT_ROW: u8 = 0x0F;
/// Row from which the fall accelerates (`$837B`).
const ACCEL_ROW: u8 = 0x0C;
/// The burst's sound (`$8353`).
const BURST_SOUND: u8 = 0x34;
/// The `$04C0` offset the burst spawns at, restored after (`$835C`/`$836E`).
const BURST_OFFSET: u16 = 0x04C0;
/// The burst's part variant (`$8369`).
const BURST_VARIANT: u8 = 8;
/// The fall's per-frame velocity step once past [`ACCEL_ROW`] (`$8381`): `$FFFB`.
const FALL_STEP: i16 = -5;
/// Tile bits for the draw (`$83A0`).
const DRAW_TILE: u8 = 0x80;
/// Attribute bits for the draw (`$83A4`).
const DRAW_ATTR: u8 = 6;
/// Contact hit box and damage (`$83AB`/`$83AF`).
const HITBOX: u8 = 9;
const DAMAGE: u8 = 2;

/// Type `$9E` keeps its whole state in [`Body`] (`$7DD0`, `$7DF0`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8309`, phases 0 then 1).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        engine::integrate(body, work);
        match body.phase {
            0 => {
                work.vy = HOVER;
                if engine::player_dx(work, &cx.effects.player).unsigned_abs() < WAKE_DISTANCE {
                    body.phase = 1;
                    body.counter = 0;
                    body.anim = 0;
                }
            }
            1 => {
                if work.y.to_be_bytes()[0] >= FALL_LIMIT_ROW {
                    *kind = 0;
                    return;
                }
                let mut probe = Probe::default();
                if collision::feet(work, &mut probe, cx) & 0x80 == 0 && body.timer & 0x80 == 0 {
                    cx.effects.sound[0] = BURST_SOUND;
                    body.timer = 0x80;
                    work.y = work.y.wrapping_add(BURST_OFFSET);
                    engine::spawn_parts(BURST_VARIANT, work, cx);
                    work.y = work.y.wrapping_sub(BURST_OFFSET);
                }
                if work.y.to_be_bytes()[0] >= ACCEL_ROW {
                    work.vy = work.vy.wrapping_add(FALL_STEP);
                }
            }
            phase => panic!("type $9E phase {phase} is past the $83C7 table"),
        }
        match engine::on_screen(work, cx.world) {
            Visibility::On => {
                work.pointer = METASPRITE;
                work.tile = DRAW_TILE;
                work.attr = DRAW_ATTR;
                engine::draw(work, DRAW_BANK, cx);
                work.hitbox = HITBOX;
                engine::contact(kind, body, work, DAMAGE, cx);
                engine::end_frame(body, work);
            }
            Visibility::Near => engine::end_frame(body, work),
            Visibility::Far => *kind = 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Area 1C:00 (level 11). With `isolate` the `$9E` hovers at a fixed x and the
    /// player never comes within its `$300` wake range, so the player is placed
    /// next to it (as `hover_diver::matches_the_oracle_up_close` does): the `$9E` wakes
    /// into phase 1, falls and bursts into `$08` parts. Both slots 3-4 run, so the
    /// phase-0 hover (85 slot-frames) and the phase-1 fall (199) both appear.
    #[test]
    fn matches_the_oracle() {
        let Some(mut scene) = Scene::tas(33794) else {
            return;
        };
        let wram = &scene.machine.bus.wram;
        let slot = (0..24)
            .find(|&s| wram[0x1CC0 + s] == 0x9E)
            .expect("a live $9E");
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.player(x.wrapping_sub(0x200), y);
        let coverage = assert_actor_phases(scene, 200, &[0x9E]);
        let expected = Coverage {
            frames: 200,
            occupied: 200,
            drawn: 200,
            phases: BTreeMap::from([(0, 85), (1, 199)]),
        };
        assert_eq!(coverage, expected);
    }

    /// The player on the `$9E`: `$A9C4` (`contact`, damage 2) hurts him.
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(33794) else {
            return;
        };
        scene.isolate(&[0x9E]);
        let slot = (0..24)
            .find(|&s| scene.machine.bus.wram[0x1CC0 + s] == 0x9E)
            .expect("a live $9E");
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.script(60, |_| 0);
        scene.player(x, y);
        let mut probe = scene.clone();
        let mut energy = probe.machine.bus.ram[0x478];
        let mut hits = 0;
        for _ in 0..20 {
            probe.run(1, &[]);
            let now = probe.machine.bus.ram[0x478];
            if now < energy {
                hits += 1;
                energy = now;
            }
        }
        assert!(hits > 0, "the oracle takes no contact hit");
        let coverage = assert_actor_phases(scene, 20, &[0x9E]);
        assert!(coverage.occupied > 0, "{coverage:?}");
        assert!(
            coverage.phases.get(&1).is_some_and(|&n| n > 0),
            "{coverage:?}"
        );
    }
}
