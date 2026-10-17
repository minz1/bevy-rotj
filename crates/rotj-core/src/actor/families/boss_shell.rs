//! Type `$4E` (bank 11 `$9B62`): the stage 4 boss's spent shell. Type `$4C`
//! releases one into the highest free slot each time its counter fills.
//!
//! The handler dispatches on neither `$7DD0` nor `$7D50`: its states are the
//! loaded/flash path (`engine::dying` with hit bit 7 clear), the dying path
//! (hit bit 7 set, which clears the parent's phase and timer and returns), the
//! `$7E10` counter wrap and the contact hit, both of which run the `$9BCB`
//! kill. It reads the boss's hit points `$A9:$AA` and, on touching the player,
//! drains `$0158` from the boss's displayed pool `$B2:$B3`.
//!
//! It calls only `docs/ACTORS.md` §9c routines: `$A000` (`load`), `$A56E`
//! (`dying`), `$A79B` (`integrate`), `$A852` (`on_screen`), `$C247` (`draw`,
//! which `$826F` maps bank 9 for) and `$A934` (`store`).

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

const HOME_BANK: u8 = 11;
const DRAW_BANK: u8 = 9;
/// The four metasprite pointers, low then high, indexed by `$F1 & 6` (`$9C0B`).
const METASPRITES: u16 = 0x9C0B;
/// The energy the handler sets before the contact probe and tests after (`$9BAB`).
const ENERGY_PROBE: u8 = 0x0E;
/// The hit box set before the contact call (`$9BB0`: `$A5` = 2).
const HITBOX: u8 = 2;
/// The damage the contact call passes in A (`$9BB2`: A = `$A5` = 2).
const DAMAGE: u8 = 2;
/// The boss pool the contact hit drains (`$9BC1`).
const POOL_DRAIN: u16 = 0x0158;
/// The kill drops the shell 76 px (`$9BD2`) and requests sound `$22` (`$9BCB`).
const KILL_DROP: u16 = 0x04C0;
const KILL_SOUND: u8 = 0x22;

/// Type `$4E` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9B62`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        if cx.effects.boss_hp == 0 {
            *kind = 0;
            return;
        }
        engine::load(body, work);
        if engine::dying(kind, body, work, 0, cx) {
            let parent = u16::from(body.anim);
            cx.set_byte(0x7DD0 + parent, 0);
            cx.set_byte(0x7E10 + parent, 0);
            work.tile = 0;
            work.attr = 0;
            return;
        }
        engine::integrate(body, work);
        body.counter = body.counter.wrapping_add(1);
        if body.counter == 0 {
            kill(body, work, cx);
        } else if engine::on_screen(work, cx.world) == Visibility::On {
            let index = cx.world.frame & 6;
            work.pointer = engine::rom_word(cx.prg, HOME_BANK, METASPRITES + u16::from(index));
            engine::draw(work, DRAW_BANK, cx);
            cx.effects.player.energy = ENERGY_PROBE;
            work.hitbox = HITBOX;
            engine::contact(kind, body, work, DAMAGE, cx);
            if cx.effects.player.energy != ENERGY_PROBE {
                let pool = u16::from_le_bytes(cx.effects.boss_pool).wrapping_sub(POOL_DRAIN);
                cx.effects.boss_pool = pool.to_le_bytes();
                kill(body, work, cx);
            }
        }
        engine::store(body, work);
    }
}

/// The kill arm (`$9BCB`): sound `$22`, the shell dropped 76 px, the dying flag set.
fn kill(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    cx.effects.sound[0] = KILL_SOUND;
    work.y = work.y.wrapping_add(KILL_DROP);
    body.hit = 0x80;
    body.dying_timer = 0;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// The shell's slot in the hinted scene (area 14:00).
    fn shell_slot(scene: &Scene) -> usize {
        (0..16)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x4E)
            .expect("a live $4E")
    }

    /// The boss's displayed pool `$B2:$B3`.
    fn pool(scene: &Scene) -> u16 {
        u16::from_le_bytes([scene.machine.bus.ram[0xB2], scene.machine.bus.ram[0xB3]])
    }

    /// `Scene::tas(26677)`: the shell's whole 16-frame life, on screen and drawn
    /// every frame. The handler dispatches on no `$7DD0`/`$7D50` byte, so its
    /// single phase is 0 throughout.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(26677) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 16, &[0x4E]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 16,
                occupied: 16,
                drawn: 16,
                phases: BTreeMap::from([(0, 16)]),
            }
        );
    }

    /// Placed on the player, the contact probe hurts him and the handler drains
    /// the boss pool by `$0158` and runs the `$9BCB` kill.
    #[test]
    fn touches_the_player() {
        let Some(mut scene) = Scene::tas(26677) else {
            return;
        };
        scene.isolate(&[0x4E]);
        let slot = shell_slot(&scene);
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.player(x, y);
        scene.script(16, |_| 0);
        scene.machine.bus.trace.pokes.push((0xD450, 0x047A, 0));
        let mut probe = scene.clone();
        let mut before = pool(&probe);
        let mut hits = 0;
        for _ in 0..16 {
            probe.run(1, &[]);
            let now = pool(&probe);
            hits += usize::from(now != before);
            before = now;
        }
        assert!(hits > 0, "the oracle never drains the pool on contact");
        let coverage = assert_actor_phases(scene, 16, &[0x4E]);
        assert!(coverage.occupied > 0 && coverage.drawn > 0, "{coverage:?}");
    }

    /// A dying shell (hit bit 7 set) clears its parent's `$7DD0`/`$7E10`. The
    /// parent is freed by isolation, so its live bytes are what the clear shows.
    #[test]
    fn the_death_clears_the_parents_timer() {
        let Some(mut scene) = Scene::tas(26677) else {
            return;
        };
        scene.isolate(&[0x4E]);
        let slot = shell_slot(&scene);
        let parent = usize::from(scene.machine.bus.wram[0x1E90 + slot]);
        assert_ne!(
            (
                scene.machine.bus.wram[0x1DD0 + parent],
                scene.machine.bus.wram[0x1E10 + parent],
            ),
            (0, 0),
            "the parent link is not live"
        );
        scene.machine.bus.trace.pokes.push((
            0xD450,
            0x7D70 + u16::try_from(slot).expect("slot < 16"),
            0x80,
        ));
        let coverage = assert_actor_phases(scene, 16, &[0x4E]);
        assert!(coverage.occupied < 16, "{coverage:?}");
    }

    /// With the boss dead (`$A9:$AA` zero) the shell frees its own slot.
    #[test]
    fn frees_itself_when_the_boss_is_dead() {
        let Some(mut scene) = Scene::tas(26677) else {
            return;
        };
        scene.isolate(&[0x4E]);
        for byte in [0xA9u16, 0xAA] {
            scene.machine.bus.trace.pokes.push((0xD450, byte, 0));
        }
        let coverage = assert_actor_phases(scene, 16, &[0x4E]);
        assert!(coverage.occupied < 16, "{coverage:?}");
    }

    /// The `$7E10` counter wrapping to zero runs the `$9BCB` kill arm.
    #[test]
    fn the_counter_wrap_kills_it() {
        let Some(mut scene) = Scene::tas(26677) else {
            return;
        };
        scene.isolate(&[0x4E]);
        let slot = shell_slot(&scene);
        scene.machine.bus.trace.pokes.push((
            0xD450,
            0x7E10 + u16::try_from(slot).expect("slot < 16"),
            0xFF,
        ));
        let coverage = assert_actor_phases(scene, 16, &[0x4E]);
        assert!(coverage.occupied < 16, "{coverage:?}");
    }
}
