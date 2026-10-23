//! Type `$72` (bank 0 `$8A7D`): a sprite that drifts in, drops four `$74` parts (at timer $40 and $60)
//! and dives at the player. Its [`Phase`]s run through `$8C74`.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::{Body, Counter};
use crate::actor::world::Cx;

const SPRITES: u16 = 0x8C7C;
const SPRITE_BANK: u8 = 9;
const HITBOX: u8 = 0x0B;
const DAMAGE: u8 = 4;
const PART: u8 = 0x74;
const PART_DX: [i16; 4] = [-24, -24, 0, 0];
const PART_TIMER: [u8; 4] = [0x90, 0xC0, 0x40, 0x70];
const PART_COUNTER: [u8; 4] = [0xFF, 0xFF, 0x00, 0x00];
const PART_PHASE: [u8; 4] = [0x00, 0x00, 0x80, 0x80];
const PART_DY: i16 = -24;
/// `anim` bit 7 once the sprite has been drawn.
const DRAWN: u8 = 0x80;

phases! {
    /// The sprite's phase (`$7DD0`), which picks its handler (`$8C74`).
    Phase {
        /// Drifting in from the right.
        Drifting = 0,
        /// Waiting and shedding the parts.
        Shedding = 1,
        /// Diving at the player.
        Diving = 2,
        /// Slowing its drift until it reverses.
        Leaving = 3,
    }
}

/// Type `$72` keeps its whole state in [`Body`]; `anim` bit 7 is "has been drawn" and
/// `subtype` counts the dives.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8A7D`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if engine::dying(kind, body, work, 0x80, cx) {
            return;
        }
        engine::integrate(body, work);
        work.attr |= body.flags;
        let visibility = engine::on_screen(work, cx.world);
        match Phase::from_byte(body.phase) {
            Some(Phase::Drifting) => drifting(body, work),
            Some(Phase::Shedding) => shedding(body, work, cx),
            Some(Phase::Diving) => diving(body, work),
            Some(Phase::Leaving) => leaving(body),
            None => panic!("type $72 phase {} is past the $8C74 table", body.phase),
        }
        if visibility != Visibility::On {
            *kind = 0;
            return;
        }
        body.anim = DRAWN;
        let index = 2 * u16::from(cx.world.frame & 0x08 != 0);
        work.pointer = engine::rom_word(cx.prg, 0, SPRITES + index);
        engine::tile_bits(work, cx.prg);
        engine::draw(work, SPRITE_BANK, cx);
        work.hitbox = HITBOX;
        engine::contact(kind, body, work, DAMAGE, cx);
        engine::store(body, work);
    }
}

/// Drifting (`$8AAD`).
fn drifting(body: &mut Body, work: &Work) {
    body.vy = 0;
    if body.timer & 0x80 == 0 {
        body.vx_drift = -0x40;
        body.timer = 0xFF;
    }
    if body.anim & DRAWN != 0 {
        if work.screen_x < 0xC0 {
            body.vx_drift = 0x0A;
            body.timer = 0;
            body.phase = Phase::Shedding.byte();
        } else {
            body.vx_drift = body.vx_drift.wrapping_add(3);
        }
    }
}

/// Shedding (`$8B00`): the parts leave at timer `$40` and `$60`, the dive is picked at `$80`.
fn shedding(body: &mut Body, work: &Work, cx: &mut Cx<'_>) {
    body.vy = 0;
    body.timer.tick_up();
    let mut result = body.timer;
    if result == 0x40 || result == 0x60 {
        result = spawn_parts(body, work, cx);
    }
    if result == 0x80 {
        body.phase = Phase::Diving.byte();
        body.counter = 0x30;
        if engine::player_dy(work, &cx.effects.player) < 0 {
            body.timer = 0;
            body.vy = -0x20;
        } else {
            body.timer = 0x80;
            body.vy = 0x20;
        }
    }
}

/// `$8BEA`: with no free slot the remaining parts are not spawned; A is slot 16's type.
fn spawn_parts(parent: &Body, work: &Work, cx: &mut Cx<'_>) -> u8 {
    let mut result = 0;
    for part in (0..4).rev() {
        let Some(slot) = engine::free_child_slot(cx.slots) else {
            return cx.slots.main[16].kind;
        };
        let child = cx.slots.main[slot].spawn(
            PART,
            work.x.wrapping_add_signed(PART_DX[part]),
            work.y.wrapping_add_signed(PART_DY),
        );
        child.timer = PART_TIMER[part];
        child.counter = PART_COUNTER[part];
        child.phase = PART_PHASE[part];
        child.state = 0;
        child.anim = 0;
        child.chr_slot = parent.chr_slot;
        result = parent.chr_slot;
    }
    result
}

/// Diving (`$8B25`): turns at the screen's top and bottom edges, `$7E10` counts the dive.
fn diving(body: &mut Body, work: &Work) {
    let turn = if body.timer & 0x80 == 0 {
        work.screen_y < 0x40
    } else {
        work.screen_y >= 0xC0
    };
    if turn {
        body.timer ^= 0x80;
        body.vy = body.vy.wrapping_neg();
    }
    body.counter.tick_down();
    if body.counter == 0 {
        body.subtype = body.subtype.wrapping_add(1);
        if body.subtype < 3 {
            body.phase = Phase::Shedding.byte();
            body.timer = 0;
            body.vy = 0;
        } else {
            body.phase = Phase::Leaving.byte();
            body.timer = 3;
        }
    }
}

/// Leaving (`$8B81`): slows the drift until it reverses.
fn leaving(body: &mut Body) {
    if body.timer & 0x80 == 0 {
        body.vy = 0;
        body.vx_drift = body.vx_drift.wrapping_sub(3);
        if body.vx_drift < 0 && body.vx_drift.to_be_bytes()[1] < 0x80 {
            body.timer = 0x80;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    const SLOT: usize = 2;
    const PHASE: usize = 0x1DD0;
    const SUBTYPE: usize = 0x1DB0;
    const COUNTER: usize = 0x1E10;
    const TIMER: usize = 0x1DF0;
    const HIT: usize = 0x1D70;

    /// A TAS scene with the `$74` parts cleared at the start of each actor loop.
    fn tas_at(frame: u64) -> Option<Scene> {
        let mut scene = Scene::tas(frame)?;
        for slot in 16..24 {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, 0x7CC0 + slot, 0));
        }
        Some(scene)
    }

    fn poked(pokes: &[(usize, u8)]) -> Option<Scene> {
        let mut scene = tas_at(10300)?;
        for &(base, value) in pokes {
            scene.machine.bus.wram[base + SLOT] = value;
        }
        Some(scene)
    }

    fn player_at(scene: &mut Scene, dy: i32) {
        let wram = &scene.machine.bus.wram;
        let y = i32::from(u16::from_be_bytes([
            wram[0x1B40 + SLOT],
            wram[0x1B60 + SLOT],
        ])) + dy;
        let x = u16::from_be_bytes([wram[0x1B00 + SLOT], wram[0x1B20 + SLOT]]);
        scene.player(x, u16::try_from(y).expect("on the map"));
    }

    fn phases(coverage: &Coverage) -> Vec<u8> {
        coverage.phases.keys().copied().collect()
    }

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = tas_at(10246) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 400, &[0x72]);
        let expected = Coverage {
            frames: 374,
            occupied: 86,
            drawn: 86,
            phases: BTreeMap::from([(0, 1), (1, 85)]),
        };
        assert_eq!(coverage, expected);
    }

    /// Area 18:00 (level 9): the same handler in slot 1, alive frames 29409-29488. The
    /// area's FME-7 IRQ drops most brackets, so only 37 of the 100 loops are compared.
    #[test]
    fn matches_the_oracle_in_level_9() {
        let Some(scene) = tas_at(29408) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 100, &[0x72]);
        let expected = Coverage {
            frames: 37,
            occupied: 37,
            drawn: 37,
            phases: BTreeMap::from([(0, 1), (1, 36)]),
        };
        assert_eq!(coverage, expected);
    }

    #[test]
    fn sheds_its_parts() {
        let Some(scene) = poked(&[(TIMER, 0x3F)]) else {
            return;
        };
        let mut probe = scene.clone();
        probe.run(1, &[]);
        let wram = &probe.machine.bus.wram;
        let parts = (16..24).filter(|&s| wram[0x1CC0 + s] == 0x74).count();
        assert_eq!(parts, 4, "the oracle does not shed its parts");
        let coverage = assert_actor_phases(scene, 120, &[0x72]);
        assert_eq!(phases(&coverage), [1], "{coverage:?}");
        assert!(coverage.occupied > 60 && coverage.drawn == coverage.occupied);
    }

    #[test]
    fn dives_toward_the_player_above_and_below() {
        for dy in [-0x300, 0x300] {
            let Some(mut scene) = poked(&[(TIMER, 0x7F)]) else {
                return;
            };
            player_at(&mut scene, dy);
            let coverage = assert_actor_phases(scene, 150, &[0x72]);
            assert_eq!(phases(&coverage), [1, 2], "{coverage:?}");
            assert!(coverage.phases[&2] > 30 && coverage.drawn == coverage.occupied);
        }
    }

    #[test]
    fn a_dive_ends_in_another_wait_or_the_leave() {
        let Some(scene) = poked(&[(PHASE, 2), (COUNTER, 2), (TIMER, 0x80)]) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 40, &[0x72]);
        assert_eq!(phases(&coverage), [1, 2], "{coverage:?}");
        let Some(scene) = poked(&[(PHASE, 2), (COUNTER, 2), (SUBTYPE, 2), (TIMER, 0x80)]) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 40, &[0x72]);
        assert_eq!(phases(&coverage), [2, 3], "{coverage:?}");
    }

    #[test]
    fn the_leave_slows_its_drift() {
        let Some(scene) = poked(&[(PHASE, 3), (TIMER, 3)]) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 200, &[0x72]);
        assert_eq!(phases(&coverage), [3], "{coverage:?}");
        assert!(coverage.phases[&3] > 40 && coverage.drawn >= coverage.occupied - 1);
    }

    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = tas_at(10300) else {
            return;
        };
        player_at(&mut scene, 0);
        let mut probe = scene.clone();
        let energy = probe.machine.bus.ram[0x478];
        probe.run(17, &[]);
        assert!(
            probe.machine.bus.ram[0x478] < energy,
            "the oracle takes no contact hit"
        );
        let coverage = assert_actor_phases(scene, 60, &[0x72]);
        assert!(coverage.occupied > 10, "{coverage:?}");
    }

    #[test]
    fn flashes_and_dies_when_hit() {
        let Some(scene) = poked(&[(HIT, 0x45)]) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 30, &[0x72]);
        assert_eq!(
            (coverage.occupied, coverage.drawn),
            (28, 28),
            "{coverage:?}"
        );
        let Some(scene) = poked(&[(HIT, 0x80)]) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 30, &[0x72]);
        assert_eq!(
            (coverage.occupied, coverage.drawn),
            (16, 16),
            "{coverage:?}"
        );
    }

    /// `$8BEA` with no free child slot (`$AF38` scans 16-23): the remaining parts are
    /// not spawned and the handler leaves slot 16's type in A. The child bank is held
    /// busy by parked `$4C` slots, whose handler returns before touching anything.
    #[test]
    fn a_full_child_bank_spawns_no_parts() {
        let Some(mut scene) = Scene::tas(10300) else {
            return;
        };
        for slot in 16..24 {
            scene.machine.bus.wram[0x1CC0 + slot] = 0x4C;
            scene.machine.bus.wram[0x1DD0 + slot] = 0x80;
        }
        scene.machine.bus.wram[TIMER + SLOT] = 0x3F;
        let mut probe = scene.clone();
        probe.run(2, &[]);
        let parts = (16..24)
            .filter(|&slot| probe.machine.bus.wram[0x1CC0 + slot] == 0x74)
            .count();
        assert_eq!(parts, 0, "the oracle shed parts into a full child bank");
        let coverage = assert_actor_phases(scene, 60, &[0x72, 0x4C]);
        assert!(coverage.occupied > 10, "{coverage:?}");
        assert_eq!(coverage.drawn, coverage.occupied, "{coverage:?}");
    }
}
