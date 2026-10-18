//! Type `$60` (bank 0 `$84F4`): a falling bomb. It hovers until the player is
//! within `$200`, then falls; when its feet find ground it frees its slot and
//! becomes a type `$18` burst of four parts.
//!
//! It calls only routines already in `docs/ACTORS.md` §9c: `$A000` (`load`),
//! `$A79B` (`integrate`), `$ABDF` (`player_dx`), `$A7DD` (`collision::feet`),
//! `$AC16` (`spawn_parts`), `$A852` (`on_screen`), `$C247` (`draw`, through
//! `$84CA`'s bank 9 map), `$AEE3` (`touch`) and `$A8E4` (`end_frame`).

use crate::actor::collision::{self, Probe};
use crate::actor::engine::{self, Visibility, Work};
use crate::actor::families::Brain;
use crate::actor::slots::Body;
use crate::actor::world::Cx;

const METASPRITE_PTR: u16 = 0x857E;
const DRAW_BANK: u8 = 9;
const HOVER: i16 = -6;
const WAKE_DISTANCE: u16 = 0x200;
const FALL_LIMIT_ROW: u8 = 0x0F;
const LANDING_SOUND: u8 = 0x22;
const PARTS_VARIANT: u8 = 5;
const PARTS_KIND: u8 = 0x18;
const PARTS_CONTACT: u8 = 0x20;
const PARTS_FLAGS: u8 = 2;
const DRAW_ATTR: u8 = 2;
const HITBOX: u8 = 6;
const DAMAGE: u8 = 2;

/// The phase (`$7DD0`) in which it hovers until the player comes near.
const HOVERING: u8 = 0;
/// The phase (`$7DD0`) in which it falls until it lands and explodes.
const FALLING: u8 = 1;

/// Type `$60` keeps its whole state in [`Body`] (`$7DD0`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$84F4`; it hovers, then falls).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        engine::integrate(body, work);
        if body.phase == HOVERING {
            work.vy = HOVER;
            if engine::player_dx(work, &cx.effects.player).unsigned_abs() < WAKE_DISTANCE {
                body.vy = 0;
                body.phase = FALLING;
            }
        } else {
            if work.y.to_be_bytes()[0] >= FALL_LIMIT_ROW {
                *kind = 0;
                return;
            }
            let mut probe = Probe::default();
            if collision::feet(work, &mut probe, cx) {
                explode(kind, body, work, cx);
                return;
            }
        }
        if engine::on_screen(work, cx.world) == Visibility::On {
            work.tile = 0;
            work.attr = DRAW_ATTR;
            work.pointer = engine::rom_word(cx.prg, 0, METASPRITE_PTR);
            engine::draw(work, DRAW_BANK, cx);
            work.hitbox = HITBOX;
            engine::touch(work, DAMAGE, cx);
        }
        engine::end_frame(body, work);
    }
}

/// `$8533`: frees the slot and spawns the `$18` burst.
fn explode(kind: &mut u8, body: &Body, work: &Work, cx: &mut Cx<'_>) {
    cx.effects.sound[0] = LANDING_SOUND;
    engine::free_own_slot(kind, body, work, cx);
    if let Some(index) = engine::spawn_parts(PARTS_VARIANT, work, cx) {
        let parts = &mut cx.slots.main[index];
        parts.kind = PARTS_KIND;
        parts.body.contact = PARTS_CONTACT;
        parts.body.flags = PARTS_FLAGS;
        parts.brain = Brain::new(PARTS_KIND);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, State, assert_actor_phases};

    fn count(scene: &Scene, kind: u8) -> usize {
        scene.machine.bus.wram[0x1CC0..0x1CD0]
            .iter()
            .map(|&slot| usize::from(slot == kind))
            .sum()
    }

    /// Area 0C:00: hovering bombs wake as the player nears, fall, and burst on ground.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(13374) else {
            return;
        };
        let mut probe = scene.clone();
        let mut bursts = 0;
        for _ in 0..300 {
            let before = count(&probe, 0x18);
            probe.run(1, &[]);
            bursts += count(&probe, 0x18).saturating_sub(before);
        }
        assert!(bursts > 0, "no $60 burst in the scene");
        let coverage = assert_actor_phases(scene, 300, &[0x60]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 300,
                occupied: 148,
                drawn: 180,
                phases: BTreeMap::from([(0, 122), (1, 26)]),
            },
            "{coverage:?}"
        );
    }

    /// `$8520`: a falling bomb past row `$0F` frees its slot without a burst.
    #[test]
    fn falling_out_of_the_level_frees_the_slot() {
        let Some(mut scene) = Scene::tas(13374) else {
            return;
        };
        let wram = &mut scene.machine.bus.wram;
        let slot = (0..16)
            .find(|&slot| wram[0x1CC0 + slot] == 0x60)
            .expect("a live $60");
        wram[0x1B40 + slot] = 0x10;
        wram[0x1DD0 + slot] = 1;
        let coverage = assert_actor_phases(scene, 5, &[0x60]);
        assert_eq!(coverage.phases.get(&1), Some(&1), "{coverage:?}");
    }

    /// `$AC16` searches slots 15 down to 0 after `$8533` freed the bomb's own
    /// slot: with the bomb moved to slot 15 the burst lands in that same slot.
    #[test]
    fn the_burst_reuses_the_bombs_own_slot() {
        let Some(mut scene) = Scene::tas(13374) else {
            return;
        };
        let bomb = loop {
            let mut next = scene.clone();
            next.run(1, &[]);
            let burst = (0..16).find(|&slot| {
                scene.machine.bus.wram[0x1CC0 + slot] == 0x60
                    && next.machine.bus.wram[0x1CC0 + slot] != 0x60
                    && count(&next, 0x18) > count(&scene, 0x18)
            });
            if let Some(slot) = burst {
                break slot;
            }
            assert!(next.machine.frame < 13674, "no $60 burst");
            scene = next;
        };
        let bus = &mut scene.machine.bus;
        let mut state = State::lift_memory(&bus.ram, &bus.wram);
        state.slots.main.swap(bomb, 15);
        state.project_into(&mut bus.ram, &mut bus.wram);
        scene.isolate(&[0x60]);
        let mut oracle = scene.clone();
        oracle.run(1, &[]);
        assert_eq!(
            oracle.machine.bus.wram[0x1CCF], 0x18,
            "the burst is not in slot 15"
        );
        let coverage = assert_actor_phases(scene, 3, &[0x60]);
        assert_eq!(coverage.phases.get(&1), Some(&1), "{coverage:?}");
    }
}
