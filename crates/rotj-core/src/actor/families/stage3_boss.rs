//! Type `$8E`, a stage controller (bank 0 `$993A`).
//!
//! Phases through the table at `$9D91` (`$7DD0` 0-5): it turns for 12 frames
//! (phase 0, `$9D7D`), drops and dashes across the room (phases 1/2), fires a
//! burst that spawns type `$90` shots into slots 16-23 (phase 3, `$9D13`), then
//! bounces between phases 4 and 5 (`$7DB0` counting, flipping its facing every
//! fourth count). Every phase ends in the shared draw tail at `$9C24`: three
//! bank-9 metasprites (`$9D85`, `$9DA7`, `$9DAB`) with `$E6`/`$E7` set from
//! `$9DA1`, then the `$AFD6` boss contact. The death path is the `$AF97`/`$B1C4`
//! boss pair.

use crate::actor::collision::{self, Probe};
use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::{Cx, Motion};

/// Phase 0's `$7C30` values (`$9D7D`), indexed by `($7DF0 & $0C) >> 2`.
const TURN_FRAMES: u16 = 0x9D7D;
/// Phase 3's `$7C30` values (`$9D80`), indexed by `($7DF0 & $1C) >> 2`.
const COUNTER_FRAMES: u16 = 0x9D80;
/// Metasprite pointers (`$9D85`), indexed by `$7C30`.
const METASPRITES: u16 = 0x9D85;
/// The `$E7` CHR shadow (`$9DA1`), indexed by `$7C30`; `$E6` is the fixed `$C3`.
const CHR_HIGH: u16 = 0x9DA1;
const CHR_LOW: u8 = 0xC3;
/// The `$7E10` bit-6 second draw (`$9DA7`), indexed by the counter's low bit.
const SECOND_DRAW: u16 = 0x9DA7;
/// The `$7E10` bit-7 third draw (`$9DAB`).
const THIRD_DRAW: u16 = 0x9DAB;
/// The draws select bank 9 at `$A000`.
const DRAW_BANK: u8 = 9;
/// The shots `$9D13` spawn into slots 16-23 are type `$90`.
const SHOT_KIND: u8 = 0x90;
/// The `$AFD6` amount the tail drains on contact.
const CONTACT: u16 = 0x00E0;

/// Type `$8E` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$993A`, phases at `$9D91`).
    pub fn step(&mut self, _kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        let flash = engine::boss_flash(body, work, cx.world);
        if flash & 0x80 != 0 {
            work.attr = body.flags & 0xFC;
            if body.dying_timer == 0x01 {
                engine::boss_defeated(work.slot, cx);
            }
            if flash & 0x40 != 0 {
                tail(body, work, cx);
            }
            return;
        }
        engine::integrate(body, work);
        let mut probe = Probe::default();
        let grounded = if collision::feet(work, &mut probe, cx) & 0x80 == 0 {
            true
        } else {
            side_ground(work, &mut probe, cx) & 0x80 == 0
        };
        let hi = work.x.to_be_bytes()[0];
        if hi < 0x02 {
            work.x = 0x0200;
        } else if hi >= 0x0E {
            work.x = 0x0E00;
        }
        if grounded {
            body.vx_drift = i16::from_be_bytes([probe.landing[1], probe.landing[0]]);
        }
        match body.phase {
            0 => phase0(body, cx),
            1 => phase1(body, work, cx),
            2 => phase2(body, work, cx),
            3 => phase3(body, work, cx),
            4 => phase4(body, work, grounded, cx),
            5 => phase5(body, work),
            other => panic!("type $8E phase {other} is past the $9D91 table"),
        }
        tail(body, work, cx);
    }
}

/// `$9CE0`: probe the floor at both sides, one `$C0` away.
fn side_ground(work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) -> u8 {
    probe.x = work.x.wrapping_add(0xC0);
    probe.y = work.y;
    let floor = collision::floor(work, probe, cx);
    if floor & 0x80 == 0 {
        return floor;
    }
    probe.x = work.x.wrapping_sub(0xC0);
    probe.y = work.y;
    collision::floor(work, probe, cx)
}

/// `$9CB6`: face the player, choosing phase 2 (keep turning) or 5 (flip).
fn face(body: &mut Body, work: &Work, cx: &Cx<'_>) {
    let dx = work.x.wrapping_sub(cx.effects.player.x).to_be_bytes();
    body.timer = dx[1];
    body.anim = dx[0];
    if (work.attr.wrapping_shl(1) ^ body.anim) & 0x80 != 0 {
        body.phase = 2;
    } else {
        body.subtype = 0x80;
        body.phase = 5;
    }
}

/// `$99BA`: hold for a beat, then turn for 12 frames.
fn phase0(body: &mut Body, cx: &Cx<'_>) {
    if body.anim & 0x80 == 0 {
        body.counter = 0x40;
        body.sprite = 0;
        if cx.effects.player.motion != Motion::BossEntry {
            body.anim = 0x80;
        }
        return;
    }
    body.counter = 0;
    let idx = (body.timer & 0x0C) >> 2;
    body.sprite = engine::rom_byte(cx.prg, 0, TURN_FRAMES + u16::from(idx));
    if body.sprite == 0 {
        body.counter = 0x40;
    }
    body.timer = body.timer.wrapping_add(1);
    if body.timer >= 0x0C {
        body.phase = 1;
        body.timer = 0;
        body.anim = 0x90;
    }
}

/// `$9A0E`: rear back and drop.
fn phase1(body: &mut Body, work: &mut Work, cx: &Cx<'_>) {
    if body.anim & 0x80 != 0 {
        body.sprite = 5;
        body.anim = body.anim.wrapping_sub(1);
        return;
    }
    if work.y.to_be_bytes()[0] < 0x08 {
        face(body, work, cx);
    }
    let step = if cx.world.frame.is_multiple_of(4) {
        8
    } else {
        6
    };
    let lo = work.vy.to_be_bytes()[1].wrapping_sub(step);
    work.vy = i16::from_be_bytes([0xFF, if lo >= 0xE0 { lo } else { 0xF0 }]);
    body.sprite = 3;
    body.counter = if cx.world.frame & 0x04 != 0 { 0x80 } else { 0 };
}

/// `$9A59`: dash across the room, accelerating until it reaches the far wall.
fn phase2(body: &mut Body, work: &mut Work, cx: &Cx<'_>) {
    body.sprite = 3;
    work.vy = i16::from_be_bytes([0xFF, 0xFA]);
    work.ax = 0;
    if work.attr & 0x40 == 0 {
        if work.x.to_be_bytes()[0] >= 0x0C {
            return end_dash(body, work, cx);
        }
        let acc =
            u16::from_be_bytes([body.anim, body.timer]).wrapping_add(body.vx_drift.cast_unsigned());
        [body.anim, body.timer] = acc.to_be_bytes();
        let lo = body.vx_drift.to_be_bytes()[1].wrapping_add(1).min(0x20);
        body.vx_drift = i16::from(lo);
    } else {
        if work.x.to_be_bytes()[0] < 0x05 {
            return end_dash(body, work, cx);
        }
        let total = u16::from_be_bytes([body.anim, body.timer]);
        let acc = body.vx_drift.cast_unsigned().wrapping_sub(total);
        [body.anim, body.timer] = acc.to_be_bytes();
        let lo = body.vx_drift.to_be_bytes()[1].wrapping_sub(1).max(0xE0);
        body.vx_drift = i16::from_be_bytes([0xFF, lo]);
    }
    if body.anim == 0 {
        return end_dash(body, work, cx);
    }
    body.counter = if cx.world.frame & 0x04 != 0 { 0x80 } else { 0 };
}

/// `$9AE0`: stop, then pick phase 4 or 5 by facing against the player.
fn end_dash(body: &mut Body, work: &Work, cx: &Cx<'_>) {
    body.vy = 0;
    let sign = if engine::player_dx(work, &cx.effects.player) < 0 {
        0x80u8
    } else {
        0
    };
    if (work.attr.wrapping_shl(1) ^ sign) & 0x80 != 0 {
        body.timer = 0;
        body.anim = 0;
        body.vx_drift = 0;
        body.phase = 4;
    } else {
        body.subtype = 0;
        body.phase = 5;
    }
}

/// `$9B11`: wind up, fire the `$90` burst at `$7E90 == $50`, then reset.
fn phase3(body: &mut Body, work: &Work, cx: &mut Cx<'_>) {
    body.counter = 0;
    if body.timer & 0x80 == 0 {
        let idx = (body.timer & 0x1C) >> 2;
        body.sprite = engine::rom_byte(cx.prg, 0, COUNTER_FRAMES + u16::from(idx));
        if body.sprite == 0 {
            body.counter = 0x40;
        }
        body.timer = body.timer.wrapping_add(1);
        if body.timer >= 0x14 {
            body.timer = 0x80;
            body.anim = 0;
        }
        return;
    }
    body.counter = 0x40;
    body.anim = body.anim.wrapping_add(1);
    if body.anim < 0x20 {
        if body.anim.is_multiple_of(8) {
            body.counter = 0x41;
        }
    } else if body.anim < 0x40 {
        if body.anim.is_multiple_of(4) {
            body.counter = 0x41;
        }
    } else if body.anim < 0x50 {
        body.counter = 0x41;
    } else if body.anim == 0x50 {
        spawn_shots(body, work, cx);
    } else if body.anim >= 0x80 {
        body.phase = 1;
        body.timer = 0;
        body.anim = 0x90;
    }
}

/// `$9B8B`: hold facing the wall, then flip back to phase 3.
fn phase4(body: &mut Body, work: &mut Work, grounded: bool, cx: &Cx<'_>) {
    if body.timer & 0x80 != 0 {
        body.counter = 0;
        body.sprite = 5;
        body.anim = body.anim.wrapping_add(1);
        if body.anim >= 0x10 {
            body.phase = 3;
            body.timer = 0;
            body.anim = 0;
        }
        return;
    }
    body.sprite = 3;
    body.counter = if cx.world.frame & 0x04 != 0 { 0x80 } else { 0 };
    if grounded {
        body.timer = 0x80;
    } else {
        work.vy = 5;
    }
}

/// `$9BDB`: count `$7DB0` on, flipping the facing every fourth value.
fn phase5(body: &mut Body, work: &mut Work) {
    body.counter = 0;
    body.vy = i16::from_be_bytes([0xFF, 0xFA]);
    body.sprite = 4;
    body.subtype = body.subtype.wrapping_add(1);
    let low = body.subtype & 0x7F;
    if low == 4 {
        work.attr ^= 0x40;
    } else if low >= 8 {
        if body.subtype & 0x80 != 0 {
            body.phase = 2;
        } else {
            body.timer = 0;
            body.anim = 0;
            body.vx_drift = 0;
            body.phase = 4;
        }
    }
}

/// `$9D13`: spawn up to five type-`$90` shots into the free slots 16-23.
fn spawn_shots(body: &Body, work: &Work, cx: &mut Cx<'_>) {
    let mut count = 4u8;
    loop {
        let Some(slot) = engine::free_child_slot(cx.slots) else {
            return;
        };
        let at = u16::try_from(slot).expect("slot < 32");
        cx.set_byte(0x7C00 + at, (work.attr & 0x40) | 0x03);
        cx.set_byte(0x7DD0 + at, count);
        let [x_hi, x_lo] = if work.attr & 0x40 == 0 {
            work.x.wrapping_add(0x80).to_be_bytes()
        } else {
            let phase = cx.byte(0x7DD0 + at).wrapping_add(5);
            cx.set_byte(0x7DD0 + at, phase);
            work.x.wrapping_sub(0x80).to_be_bytes()
        };
        cx.set_byte(0x7B00 + at, x_hi);
        cx.set_byte(0x7B20 + at, x_lo);
        let [y_hi, y_lo] = work.y.wrapping_sub(0x02C0).to_be_bytes();
        cx.set_byte(0x7B40 + at, y_hi);
        cx.set_byte(0x7B60 + at, y_lo);
        cx.set_byte(0x7C90 + at, body.chr_slot);
        cx.set_byte(0x7CC0 + at, SHOT_KIND);
        cx.set_byte(0x7D50 + at, 0);
        if count == 0 {
            return;
        }
        count -= 1;
    }
}

/// `$9C24`: the shared draw, contact and store tail.
fn tail(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    if engine::on_screen(work, cx.world) == Visibility::On {
        let index = body.sprite;
        cx.oam.chr[6] = CHR_LOW;
        cx.oam.chr[7] = engine::rom_byte(cx.prg, 0, CHR_HIGH + u16::from(index));
        work.pointer = engine::rom_word(cx.prg, 0, METASPRITES + 2 * u16::from(index));
        work.tile = 0;
        engine::draw(work, DRAW_BANK, cx);
        if body.counter & 0x40 != 0 {
            let at = if body.counter & 0x01 != 0 { 0 } else { 2 };
            work.pointer = engine::rom_word(cx.prg, 0, SECOND_DRAW + at);
            engine::draw(work, DRAW_BANK, cx);
        }
        if body.counter & 0x80 != 0 {
            work.attr |= 0x03;
            work.pointer = engine::rom_word(cx.prg, 0, THIRD_DRAW);
            engine::draw(work, DRAW_BANK, cx);
        }
        work.hitbox = 0;
        engine::boss_contact(CONTACT, 0, body, work, cx);
    }
    work.age = work.age.wrapping_add(1);
    engine::end_frame(body, work);
}

#[cfg(test)]
mod tests {
    use crate::actor::oracle::{Scene, assert_actor_phases};
    use crate::actor::slots::Slots;

    /// The `$8E`'s whole life from the scene hint (`Scene::tas(14672)`): phases
    /// 0-5, on screen and drawn. The area-`0E` FME-7 IRQ drops some loops, so 570
    /// of 1000 frames are compared; all of them hold the `$8E`.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(14672) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 1000, &[0x8E]);
        assert!(coverage.frames > 500, "{coverage:?}");
        assert_eq!(coverage.occupied, coverage.frames, "{coverage:?}");
        assert!(coverage.drawn > 0, "{coverage:?}");
        for phase in 0..6u8 {
            assert!(
                coverage.phases.get(&phase).is_some_and(|&n| n > 0),
                "phase {phase}: {:?}",
                coverage.phases
            );
        }
    }

    /// The `$AF97`/`$B1C4` death pair. The `$8E` is never hit in the whole-game
    /// TAS (§8b weapon 0), so `$7D70` = `$80` is held: the first loop spawns the
    /// `$B1C4` type `$50` (freed each loop), then the flash draws every other
    /// frame.
    #[test]
    fn the_death_matches_the_oracle() {
        let Some(mut scene) = Scene::tas(14672) else {
            return;
        };
        scene.isolate(&[0x8E]);
        let slot = (0..24)
            .find(|&s| scene.machine.bus.wram[0x1CC0 + s] == 0x8E)
            .expect("a $8E");
        let at = u16::try_from(slot).expect("slot < 24");
        scene
            .machine
            .bus
            .trace
            .pokes
            .push((0xD450, 0x7D70 + at, 0x80));
        for other in 0..24u16 {
            if other != at {
                scene
                    .machine
                    .bus
                    .trace
                    .pokes
                    .push((0xD450, 0x7CC0 + other, 0));
            }
        }
        let coverage = assert_actor_phases(scene, 80, &[0x8E]);
        assert!(coverage.frames > 0, "{coverage:?}");
        assert!(coverage.occupied > 0, "{coverage:?}");
        assert!(coverage.drawn > 0, "{coverage:?}");
    }

    /// `$9D13`: the `$90` burst of phase 3. The natural scene only reaches it on
    /// IRQ-dropped loops, so `$7DD0` = 3, `$7DF0` = `$80`, `$7E90` = `$4F` are
    /// held at `$D450`: the handler advances `$7E90` to `$50` and spawns five
    /// shots. The shots are freed each loop, so the unported `$90` never runs.
    #[test]
    fn the_burst_spawns_shots() {
        let Some(mut scene) = Scene::tas(14672) else {
            return;
        };
        scene.isolate(&[0x8E]);
        let slot = (0..24)
            .find(|&s| scene.machine.bus.wram[0x1CC0 + s] == 0x8E)
            .expect("a $8E");
        let at = u16::try_from(slot).expect("slot < 24");
        for child in 16..24u16 {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, 0x7CC0 + child, 0));
        }
        for (base, value) in [(0x7DD0, 3u8), (0x7DF0, 0x80), (0x7E90, 0x4F)] {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, base + at, value));
        }
        let mut probe = scene.clone();
        let event = probe.run(1, &[0xD453]).remove(0);
        let shots = Slots::lift(&event.wram).main[16..24]
            .iter()
            .filter(|slot| slot.kind == 0x90)
            .count();
        assert_eq!(shots, 5, "the $9D13 burst spawned {shots} shots");
        let coverage = assert_actor_phases(scene, 30, &[0x8E]);
        assert!(coverage.frames > 0, "{coverage:?}");
        assert!(
            coverage.phases.get(&3).is_some_and(|&n| n > 0),
            "{coverage:?}"
        );
    }

    /// The `$AFD6` tail contact. The whole-game TAS never overlaps the player
    /// (`docs/ACTORS.md` §8b contact 0), so both are pinned to one point: the
    /// pin makes `$AFD6` drain the player's energy (`$0479` is set).
    #[test]
    fn the_boss_hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(14672) else {
            return;
        };
        scene.isolate(&[0x8E]);
        let slot = (0..24)
            .find(|&s| scene.machine.bus.wram[0x1CC0 + s] == 0x8E)
            .expect("a $8E");
        let at = u16::try_from(slot).expect("slot < 24");
        for child in 16..24u16 {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, 0x7CC0 + child, 0));
        }
        let ram = &scene.machine.bus.ram;
        let x = u16::from_be_bytes([ram[0x460], ram[0x461]]);
        let y = u16::from_be_bytes([ram[0x462], ram[0x463]]);
        let [xh, xl] = x.to_be_bytes();
        let [yh, yl] = y.to_be_bytes();
        for (addr, value) in [(0x460, xh), (0x461, xl), (0x462, yh), (0x463, yl)] {
            scene.machine.bus.trace.pokes.push((0xD450, addr, value));
        }
        for (base, value) in [(0x7B00, xh), (0x7B20, xl), (0x7B40, yh), (0x7B60, yl)] {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, base + at, value));
        }
        scene.script(60, |_| 0);
        let mut probe = scene.clone();
        let mut hurt = 0u8;
        for _ in 0..60 {
            probe.run(1, &[]);
            hurt = hurt.max(probe.machine.bus.ram[0x479]);
        }
        assert!(hurt > 0, "the oracle never took the boss contact");
        let coverage = assert_actor_phases(scene, 60, &[0x8E]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }
}
