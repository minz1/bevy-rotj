//! Type `$52` (bank 11 `$9CFC`): a short-lived effect that other handlers spawn.
//! It calls only routines in `docs/ACTORS.md` §9c: `$A000` (`load`), `$A852`
//! (`on_screen`), `$AC06` (`tile_bits`), `$C247` (`draw`, bank 1 as `$825A` maps
//! it) and `$A934` (`store`). It never integrates, contacts or dispatches on
//! `$7DD0`; `$7E90` counts two per frame from 0, so it draws four metasprites and
//! frees its slot after eight loops.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::{Body, Counter};
use crate::actor::world::Cx;

const METASPRITES: u16 = 0x9D41;
const HOME_BANK: u8 = 11;
const DRAW_BANK: u8 = 1;
const LIFETIME: u8 = 0x10;

/// Type `$52` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9CFC`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        body.anim.tick_up();
        let index = (body.anim >> 1) & 0xFE;
        work.pointer = engine::rom_word(cx.prg, HOME_BANK, METASPRITES + u16::from(index));
        work.chr_slot = body.chr_slot;
        work.attr = 0;
        if engine::on_screen(work, cx.world) == Visibility::On {
            engine::tile_bits(work, cx.prg);
            engine::draw(work, DRAW_BANK, cx);
        }
        body.anim.tick_up();
        if body.anim >= LIFETIME {
            *kind = 0;
        } else {
            engine::store(body, work);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// The one live `$52` of a scene, before its slot is isolated.
    fn live_slot(scene: &Scene) -> usize {
        (0..24)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x52)
            .expect("a live $52")
    }

    /// Isolates `$52` and diffs the actor loop for `frames`.
    fn checked(scene: Scene, frames: u64) -> Coverage {
        assert_actor_phases(scene, frames, &[0x52])
    }

    /// Area 04:00 (the stage 1 boss room): the boss's parts spawn an 8-frame `$52`.
    /// The scene is seeded on the first frame one is alive and `isolate` drops the
    /// other kinds, so only that `$52` is diffed; it is drawn every one of its
    /// seven remaining loops.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(6580) else {
            return;
        };
        assert_eq!(
            checked(scene, 30),
            Coverage {
                frames: 30,
                occupied: 7,
                drawn: 7,
                phases: BTreeMap::from([(0, 7)]),
            }
        );
    }

    /// The inventory's scene frame (`Scene::tas(37460)`, area 1E:00) matches too.
    #[test]
    fn matches_the_oracle_in_the_last_room() {
        let Some(scene) = Scene::tas(37460) else {
            return;
        };
        assert_eq!(
            checked(scene, 40),
            Coverage {
                frames: 40,
                occupied: 7,
                drawn: 7,
                phases: BTreeMap::from([(0, 7)]),
            }
        );
    }

    /// `$7DD0` is residue: the handler never reads it. Forcing every value the
    /// inventory reports for type `$52` (`00`/`01`/`5D`/`5E`/`FF`) still matches
    /// the oracle and still draws seven loops, so the port ignores it too.
    #[test]
    fn every_hinted_phase_is_ignored() {
        for phase in [0x00, 0x01, 0x5D, 0x5E, 0xFF] {
            let Some(mut scene) = Scene::tas(6580) else {
                return;
            };
            let slot = live_slot(&scene);
            scene.machine.bus.trace.pokes.push((
                0xD450,
                0x7DD0 + u16::try_from(slot).expect("slot < 24"),
                phase,
            ));
            let coverage = checked(scene, 30);
            assert_eq!(coverage.occupied, 7, "phase {phase:02X}: {coverage:?}");
            assert_eq!(coverage.drawn, 7, "phase {phase:02X}: {coverage:?}");
            assert_eq!(
                coverage.phases,
                BTreeMap::from([(phase, 7)]),
                "phase {phase:02X}"
            );
        }
    }

    /// `$7D50`, the spawn state, is residue too: forcing each reported value still
    /// matches.
    #[test]
    fn every_hinted_state_is_ignored() {
        for state in [0x00, 0x5D, 0xA0, 0xA1, 0xFF] {
            let Some(mut scene) = Scene::tas(6580) else {
                return;
            };
            let slot = live_slot(&scene);
            scene.machine.bus.trace.pokes.push((
                0xD450,
                0x7D50 + u16::try_from(slot).expect("slot < 24"),
                state,
            ));
            let coverage = checked(scene, 30);
            assert_eq!(coverage.occupied, 7, "state {state:02X}: {coverage:?}");
            assert_eq!(coverage.drawn, 7, "state {state:02X}: {coverage:?}");
        }
    }

    /// Pushed past the right margin, the actor is skipped by the draw: no OAM
    /// change, and `$A934` still stores the unchanged position.
    #[test]
    fn off_screen_skips_the_draw() {
        let Some(mut scene) = Scene::tas(6580) else {
            return;
        };
        let slot = live_slot(&scene);
        let camera = u16::from_be_bytes([scene.machine.bus.ram[0xCC], scene.machine.bus.ram[0xCD]]);
        let [hi, lo] = camera.wrapping_add(0x7000).to_be_bytes();
        scene.machine.bus.trace.pokes.push((
            0xD450,
            0x7B00 + u16::try_from(slot).expect("slot < 24"),
            hi,
        ));
        scene.machine.bus.trace.pokes.push((
            0xD450,
            0x7B20 + u16::try_from(slot).expect("slot < 24"),
            lo,
        ));
        let coverage = checked(scene, 30);
        assert_eq!((coverage.occupied, coverage.drawn), (7, 0), "{coverage:?}");
    }
}
