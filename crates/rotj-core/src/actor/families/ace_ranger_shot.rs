//! Types `$3A` and `$3C`, the stage 1 boss's projectile parts (bank 11 `$9144`,
//! `$91DC`).
//!
//! `$3A` (`$8FB9`) drifts by a fixed per-`$7DD0` offset each frame and drains the
//! player's energy on `$AFD6` contact; `$3C` (`$909B`) homes on the player, drains
//! the boss's `$B2:$B3` pool when it reaches him, and detonates. Both draw bank-9
//! metasprites through the `$826F` helper. One module, as `breakable_block` covers
//! `$04`/`$06`.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::slots::hit;
use crate::actor::world::Cx;

/// The `$3A` metasprite pointers (`$91D8`), indexed by `$7C30`.
const PART_SPRITE: u16 = 0x91D8;
/// The `$3C` metasprite pointers (`$92B5`), indexed by `$F1` bit 1.
const SHELL_SPRITE: u16 = 0x92B5;
/// The `$B2:$B3` a `$3C` part drains on contact (`$927B`).
const SHELL_POOL: u16 = 0x0258;
/// The `$AFD6` amount a `$3A` part drains on contact (`$919A`) and its mode.
const PART_POOL: u16 = 0x01E0;
const PART_MODE: u8 = 0x80;
/// The `$3C` detonation (`$9288`): sound and the y step down.
const DETONATE_SOUND: u8 = 0x22;
const DETONATE_DROP: u16 = 0x04C0;
/// `$826F` maps bank 9 into `$A000` for the draws (`$C247`).
const DRAW_BANK: u8 = 9;

/// The `$3A` part's tables (bank 11), by its drift index `phase` (0-3).
pub struct Tables {
    /// The per-frame x offset, low/high (`$91C8`/`$91CC`).
    pub move_x: [u16; 4],
    /// The per-frame y offset, low/high (`$91D0`/`$91D4`).
    pub move_y: [u16; 4],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        let (x_lo, x_hi): ([u8; 4], [u8; 4]) = (
            engine::rom_bytes(prg, 11, 0x91C8),
            engine::rom_bytes(prg, 11, 0x91CC),
        );
        let (y_lo, y_hi): ([u8; 4], [u8; 4]) = (
            engine::rom_bytes(prg, 11, 0x91D0),
            engine::rom_bytes(prg, 11, 0x91D4),
        );
        Tables {
            move_x: std::array::from_fn(|i| u16::from_be_bytes([x_hi[i], x_lo[i]])),
            move_y: std::array::from_fn(|i| u16::from_be_bytes([y_hi[i], y_lo[i]])),
        }
    }
}

/// `$3A` keeps its drift's table index in `phase` and its metasprite in `sprite`; the `$3C`
/// shell counts its frames in the pair `[counter, timer]`, zeroed at spawn, and detonates
/// at `$0200` or on reaching the player.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler: `$3A` at `$9144`, `$3C` at `$91DC`.
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        match *kind {
            0x3A => part(kind, body, work, cx),
            0x3C => shell(kind, body, work, cx),
            other => panic!("type ${other:02X} is not a $3A/$3C part"),
        }
    }
}

/// `$9144`: add the `$7DD0` drift, draw from bank 9, and free on contact or off
/// screen.
fn part(kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    engine::load(body, work);
    work.attr = body.flags;
    drift(work, body.phase, cx);
    work.pointer = engine::rom_word(cx.prg, 11, PART_SPRITE + 2 * u16::from(body.sprite));
    if engine::on_screen(work, cx.world) != Visibility::On {
        *kind = 0;
        return;
    }
    work.tile = 0;
    if !cx.world.frame_odd {
        engine::draw(work, DRAW_BANK, cx);
    }
    work.hitbox = 1;
    if engine::boss_contact(PART_POOL, PART_MODE, body, work, cx) & 0x80 != 0 {
        *kind = 0;
        return;
    }
    body.x = work.x;
    body.y = work.y;
}

/// `$914F`: add the per-`$7DD0` x/y offset to the working position. The game
/// stores the moved position back, so the part travels by the offset each frame.
fn drift(work: &mut Work, phase: u8, cx: &Cx<'_>) {
    let tables = &cx.prg.families.ace_ranger_shot;
    let index = usize::from(phase);
    work.x = work.x.wrapping_add(tables.move_x[index]);
    work.y = work.y.wrapping_add(tables.move_y[index]);
}

/// `$91DC`: home on the player, draw from bank 9, and drain him and the boss on
/// contact.
fn shell(kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    if cx.effects.boss_hp == 0 {
        *kind = 0;
        return;
    }
    engine::load(body, work);
    if engine::dying(kind, body, work, 0, cx) {
        return;
    }
    work.attr = 0x02;
    engine::integrate(body, work);
    // `$91FE`/`$9222`: home on the player, four 1/16 px a frame.
    let dy = engine::player_dy(work, &cx.effects.player);
    body.vy = if dy < 0 {
        if dy.unsigned_abs().to_be_bytes()[0] >= 2 {
            4
        } else {
            0
        }
    } else {
        -4
    };
    let dx = engine::player_dx(work, &cx.effects.player);
    body.vx_drift = if dx < 0 { 4 } else { -4 };
    let life = u16::from_be_bytes([body.counter, body.timer]).wrapping_add(1);
    [body.counter, body.timer] = life.to_be_bytes();
    let mut detonate = life.to_be_bytes()[1] == 0 && life >= 0x0200;
    if !detonate && engine::on_screen(work, cx.world) == Visibility::On {
        work.pointer = engine::rom_word(cx.prg, 11, SHELL_SPRITE + u16::from(cx.world.frame & 2));
        work.tile = 0;
        engine::draw(work, DRAW_BANK, cx);
        cx.effects.player.energy = 0x0E;
        work.hitbox = 2;
        engine::contact(kind, body, work, 2, cx);
        detonate = cx.effects.player.energy != 0x0E;
        if detonate {
            let pool = u16::from_le_bytes(cx.effects.boss_pool).wrapping_sub(SHELL_POOL);
            cx.effects.boss_pool = pool.to_le_bytes();
        }
    }
    if detonate {
        // `$9288`: sound, 76 px down, and the `$7D70` that starts the death.
        cx.effects.sound[0] = DETONATE_SOUND;
        work.y = work.y.wrapping_add(DETONATE_DROP);
        body.hit = hit::DYING;
        body.dying_timer = 0;
    }
    engine::store(body, work);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Scene, assert_actor_phases};

    fn slots_of(scene: &Scene, kind: u8, range: std::ops::Range<usize>) -> Vec<usize> {
        range
            .filter(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == kind)
            .collect()
    }

    /// The stage 1 boss room (`Scene::tas(6282)`, area 4): the boss is isolated
    /// and its `$7DD0` held at `phase`, so it spawns its parts, then `frames`
    /// frames let them appear.
    fn boss_with_parts(frame: u64, phase: u8, frames: u64) -> Option<Scene> {
        let mut scene = Scene::tas(frame)?;
        scene.isolate(&[0x38]);
        let boss = slots_of(&scene, 0x38, 0..24)[0];
        scene.machine.bus.trace.pokes.push((
            0xD450,
            0x7DD0 + u16::try_from(boss).expect("slot < 24"),
            phase,
        ));
        scene.run(frames, &[]);
        Some(scene)
    }

    /// Pins `slot` onto the player's level position each loop, so a handler's
    /// contact test reaches its overlap branch.
    fn pin_to_player(scene: &mut Scene, slot: usize) {
        let ram = &scene.machine.bus.ram;
        let x = u16::from_be_bytes([ram[0x460], ram[0x461]]);
        let y = u16::from_be_bytes([ram[0x462], ram[0x463]]);
        let [x_hi, x_lo] = x.to_be_bytes();
        let [y_hi, y_lo] = y.to_be_bytes();
        let at = u16::try_from(slot).expect("slot < 24");
        for (base, value) in [
            (0x7B00, x_hi),
            (0x7B20, x_lo),
            (0x7B40, y_hi),
            (0x7B60, y_lo),
        ] {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, base + at, value));
        }
    }

    /// The `$3A` phase-1 spawn (`$8FB9`) into slots 16-23, then each of the four
    /// `$7DD0` drift offsets held in turn. The boss's own facing only picks
    /// `$7DD0` 2 or 3, so the pokes cover 0 and 1.
    #[test]
    fn matches_the_oracle() {
        let mut phases: BTreeMap<u8, usize> = BTreeMap::new();
        let mut drawn = 0;
        for phase in 0..4u8 {
            let Some(mut scene) = boss_with_parts(6282, 1, 40) else {
                return;
            };
            assert!(
                !slots_of(&scene, 0x3A, 16..24).is_empty(),
                "phase {phase}: no $3A spawned"
            );
            for slot in 16..24 {
                scene.machine.bus.trace.pokes.push((
                    0xD450,
                    0x7DD0 + u16::try_from(slot).expect("slot < 24"),
                    phase,
                ));
            }
            let coverage = assert_actor_phases(scene, 60, &[0x3A]);
            assert!(coverage.occupied > 0, "phase {phase}: {coverage:?}");
            drawn += coverage.drawn;
            for (phase, frames) in coverage.phases {
                *phases.entry(phase).or_default() += frames;
            }
        }
        assert!(drawn > 0, "the parts never drew: {phases:?}");
        for phase in 0..4u8 {
            assert!(
                phases.get(&phase).is_some_and(|&frames| frames > 0),
                "phase {phase}: {phases:?}"
            );
        }
    }

    /// `$AFD6` on the player: a `$3A` pinned onto him drains his energy and the
    /// part frees its own slot.
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = boss_with_parts(6282, 1, 40) else {
            return;
        };
        scene.isolate(&[0x3A]);
        let slot = slots_of(&scene, 0x3A, 16..24)[0];
        pin_to_player(&mut scene, slot);
        let mut probe = scene.clone();
        let mut hurt = 0u8;
        for _ in 0..60 {
            probe.run(1, &[]);
            hurt = hurt.max(probe.machine.bus.ram[0x479]);
        }
        assert!(hurt > 0, "the oracle never took the $3A contact");
        let coverage = assert_actor_phases(scene, 60, &[0x3A]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }

    /// The `$3C` phase-2 spawn (`$909B`) into slots 0-15. All but one shell are
    /// dropped: with three alive the boss room's FME-7 IRQ lands inside every
    /// actor loop and the harness finds none to diff.
    fn shell_scene(frame: u64) -> Option<Scene> {
        let mut scene = boss_with_parts(frame, 2, 70)?;
        let shells = slots_of(&scene, 0x3C, 0..16);
        let first = *shells.first()?;
        scene.isolate(&[0x3C]);
        for &slot in shells.iter().filter(|&&slot| slot != first) {
            scene.machine.bus.wram[0x1CC0 + slot] = 0;
        }
        Some(scene)
    }

    /// The `$3C`'s homing run: bank-9 draw from bank 9, and its only `$7DD0`,
    /// zero, in every compared loop (the handler never reads it).
    #[test]
    fn matches_the_oracle_shell() {
        let Some(scene) = shell_scene(6282) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 80, &[0x3C]);
        assert!(coverage.occupied > 0, "{coverage:?}");
        assert!(coverage.drawn > 0, "{coverage:?}");
        assert_eq!(
            coverage.phases,
            BTreeMap::from([(0, coverage.occupied)]),
            "{coverage:?}"
        );
    }

    /// A `$3C` pinned onto an idle player: `$A9C4` drains his energy, the handler
    /// branches at `$9274`, and `$0258` leaves the boss's `$B2:$B3` pool as it
    /// detonates.
    #[test]
    fn the_shell_detonates_on_the_player() {
        let Some(mut scene) = shell_scene(6282) else {
            return;
        };
        let slot = slots_of(&scene, 0x3C, 0..16)[0];
        scene.script(60, |_| 0);
        pin_to_player(&mut scene, slot);
        let pool = |scene: &Scene| {
            u16::from_le_bytes(
                scene.machine.bus.ram[0xB2..0xB4]
                    .try_into()
                    .expect("2 bytes"),
            )
        };
        let before = pool(&scene);
        let mut probe = scene.clone();
        let mut after = before;
        for _ in 0..60 {
            probe.run(1, &[]);
            after = after.min(pool(&probe));
        }
        assert!(after < before, "the $3C never drained the boss's $B2");
        let coverage = assert_actor_phases(scene, 60, &[0x3C]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }

    /// `$91DC`: with the boss's `$A9:$AA` zeroed, the shell frees its slot on its
    /// first loop.
    #[test]
    fn a_dead_boss_frees_the_shell() {
        let Some(mut scene) = shell_scene(6282) else {
            return;
        };
        for addr in [0x00A9, 0x00AA] {
            scene.machine.bus.trace.pokes.push((0xD450, addr, 0));
        }
        let mut probe = scene.clone();
        probe.run(1, &[]);
        assert!(
            slots_of(&probe, 0x3C, 0..16).is_empty(),
            "the shell survived a dead boss"
        );
        let coverage = assert_actor_phases(scene, 8, &[0x3C]);
        assert!(coverage.frames > 0, "{coverage:?}");
    }

    /// The boss and the `$3A` parts it keeps spawning run together in one actor
    /// loop: the `$8FB9` spawn, the slots-16-23 aliasing and both draws match,
    /// which the `ace_ranger` fight test cannot reach (it clears every part).
    #[test]
    fn the_boss_runs_with_its_parts() {
        let Some(scene) = boss_with_parts(6282, 1, 24) else {
            return;
        };
        assert!(
            !slots_of(&scene, 0x3A, 16..24).is_empty(),
            "no $3A to run with the boss"
        );
        let coverage = assert_actor_phases(scene, 120, &[0x38, 0x3A]);
        assert!(coverage.occupied > 0, "{coverage:?}");
        assert!(coverage.drawn > 0, "{coverage:?}");
    }
}
