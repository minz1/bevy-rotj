//! Type `$A6`, the stage 6 boss (bank 12 `$84EB`).
//!
//! Phases through the table at `$87C5` (`$7DD0` 0-2; any larger value indexes
//! past the table). The draw tail at `$86CB` maps bank 9 into `$A000` for its two
//! metasprites and runs the boss contact `$AFD6`. Phase 1 spawns two `$A8` parts
//! into slots 16-23 (`$8737`) every 32 frames; the dying path spawns a `$50`
//! (`$B1C4`) and an `$AA` (`$8785`) once, when `$7F00` reaches 1.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::{Cx, Motion};

/// Primary metasprite pointers (`$87C1`), by `$F1` bit 2.
const METASPRITES: u16 = 0x87C1;
/// Second-draw metasprite pointers (`$87E0`), by `$7C30 * 2` (`0` when bit 7 set).
const SECOND_METASPRITES: u16 = 0x87E0;
/// The phase-2 animation table (`$87CB`), by `$7DB0 >> 3`.
const ANIMATION: u16 = 0x87CB;
/// The `$A8` parts' phases (`$87E8`) and timers (`$87EA`), by the spawn index.
const CHILD_PHASE: u16 = 0x87E8;
const CHILD_TIMER: u16 = 0x87EA;
/// `$C229` maps bank 9 into `$A000` for the draws.
const DRAW_BANK: u8 = 9;
/// The CHR shadows the tail sets before the draws (`$E6`/`$E7`).
const CHR_6: u8 = 0xC6;
const CHR_7: u8 = 0xC7;
/// The `$AFD6` amount the boss drains from `$B2` on contact.
const CONTACT: u16 = 0x0190;
/// The `$42` request when an `$A8` part spawns (`$8748`).
const SPAWN_SOUND: u8 = 0x2B;

/// Type `$A6` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$84EB`, phases at `$87C5`).
    pub fn step(&mut self, _kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        let flash = engine::boss_flash(body, work, cx.world);
        work.attr |= body.flags;
        if flash & 0x80 != 0 {
            if body.dying_timer == 0x01 {
                // `$B1C4` takes the dying slot's index in X as the part's timer.
                engine::boss_defeated(work.slot, cx);
                spawn_aa(body, work, cx);
            }
            if flash & 0x40 != 0 {
                tail(body, work, cx);
            }
            return;
        }
        match body.phase {
            0 => phase0(body, cx),
            1 => phase1(body, work, cx),
            2 => phase2(body, cx),
            phase => panic!("type $A6 phase {phase} is past the $87C5 table"),
        }
        tail(body, work, cx);
    }
}

/// `$8530`: stand still, then leave for phase 1 unless the level state is `$09`.
fn phase0(body: &mut Body, cx: &mut Cx<'_>) {
    body.sprite = 0;
    if cx.effects.player.motion != Motion::BossEntry {
        body.vx_drift = 0x10;
        body.phase = 1;
    }
}

/// `$854E`: drift, steer, and arm the dive; hand off to `$8633` when diving.
fn phase1(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    body.sprite = 0;
    work.x = work.x.wrapping_add_signed(body.vx_drift);
    work.y = work.y.wrapping_add_signed(body.vy);
    if body.timer != 0 {
        descent(body, work);
        return;
    }
    // `$8737`'s spawn loop ends with `X = $1F`, 0 after a full pair, so the
    // frame's steering (`$8582`-`$8630`) runs on slot 0, not the actor's slot.
    let x = if cx.world.frame.is_multiple_of(0x20) {
        spawn_a8(work, cx)
    } else {
        work.slot
    };
    if x == work.slot {
        steer(body, work);
    } else {
        steer(&mut cx.slots.main[usize::from(x)].body, work);
    }
}

/// `$8582`: bounce vy between rows 6-8 and pull x back into columns 3-11.
fn steer(body: &mut Body, work: &mut Work) {
    let y_hi = work.y.to_be_bytes()[0];
    body.vy = if body.vy < 0 {
        if y_hi >= 0x06 { -8 } else { 8 }
    } else if y_hi < 0x08 {
        8
    } else {
        -8
    };
    let x_hi = work.x.to_be_bytes()[0];
    if x_hi < 0x03 {
        work.x = 0x0300;
        body.vx_drift = 0x10;
    } else if x_hi >= 0x0C {
        work.x = 0x0C00;
        body.vx_drift = -0x10;
    } else {
        steer_end(body, work);
        return;
    }
    dive(body, work);
}

/// `$8633`: the armed dive, until the boss rises back under row 8.
fn descent(body: &mut Body, work: &mut Work) {
    body.vy = body.vy.wrapping_sub(1);
    if body.vy < 0 && body.vy.to_be_bytes()[1] < 0xE0 {
        body.vy = i16::from_be_bytes([0xFF, 0xE0]);
    }
    if work.y.to_be_bytes()[0] >= 0x08 || body.vy >= 0 {
        clamp_x(body, work);
        return;
    }
    body.timer = 0;
    body.vx_drift = if body.vx_drift < 0 { -0x10 } else { 0x10 };
}

/// `$867B`: pull the boss back into x columns 3-11, clearing the drift when clamped.
fn clamp_x(body: &mut Body, work: &mut Work) {
    let hi = work.x.to_be_bytes()[0];
    if hi < 0x03 {
        work.x = 0x0300;
        body.vx_drift = 0;
    } else if hi >= 0x0C {
        work.x = 0x0C00;
        body.vx_drift = 0;
    }
}

/// `$85D0`: count and re-arm the dive, then check for the phase-2 hand-off.
fn dive(body: &mut Body, work: &mut Work) {
    work.x = u16::from_be_bytes([work.x.to_be_bytes()[0], 0]);
    body.counter = body.counter.wrapping_add(1);
    if body.counter & 0x02 == 0 {
        body.timer = 1;
        body.vy = 0x28;
        body.vx_drift = if body.vx_drift < 0 { -0x20 } else { 0x20 };
    }
    steer_end(body, work);
}

/// `$8603`: at exactly x `$0800` on an even counter step, enter phase 2.
fn steer_end(body: &mut Body, work: &mut Work) {
    if work.x == 0x0800 && body.counter & 0x04 == 0 {
        body.phase = 2;
        body.anim = body.vx_drift.to_be_bytes()[0];
        body.vx_drift = 0;
        body.vy = 0;
        body.subtype = 0;
    }
}

/// `$869E`: run the `$87CB` animation until `$7DB0` reaches `$A8`, then re-dive.
fn phase2(body: &mut Body, cx: &mut Cx<'_>) {
    body.sprite = engine::rom_byte(cx.prg, 12, ANIMATION + u16::from(body.subtype >> 3));
    body.subtype = body.subtype.wrapping_add(1);
    if body.subtype >= 0xA8 {
        let lo = if body.anim & 0x80 == 0 { 0x10 } else { 0xF0 };
        body.vx_drift = i16::from_be_bytes([body.anim, lo]);
        body.phase = 1;
    }
}

/// `$8737`: drop two `$A8` parts into the highest free slots 16-23. Returns the
/// X the routine leaves behind (its spawn index `$1F`, 0 after a full pair).
fn spawn_a8(work: &Work, cx: &mut Cx<'_>) -> u8 {
    let mut part = 1u8;
    let mut spawn_index = work.slot;
    loop {
        let Some(slot) = engine::free_child_slot(cx.slots) else {
            return spawn_index;
        };
        let child = u16::try_from(slot).expect("slots 16-23");
        spawn_index = part;
        cx.effects.sound[0] = SPAWN_SOUND;
        let [x_hi, x_lo] = work.x.to_be_bytes();
        let [y_hi, y_lo] = work.y.to_be_bytes();
        for (array, value) in [
            (
                0x7DD0,
                engine::rom_byte(cx.prg, 12, CHILD_PHASE + u16::from(part)),
            ),
            (
                0x7DF0,
                engine::rom_byte(cx.prg, 12, CHILD_TIMER + u16::from(part)),
            ),
            (0x7E10, 0xC0),
            (0x7E90, 0xFF),
            (0x7B20, x_lo),
            (0x7B00, x_hi),
            (0x7B60, y_lo),
            (0x7B40, y_hi.wrapping_sub(1)),
            (0x7D50, 0x00),
        ] {
            cx.set_byte(array + child, value);
        }
        cx.set_byte(0x7CC0 + child, 0xA8);
        part = part.wrapping_sub(1);
        if part & 0x80 != 0 {
            return spawn_index;
        }
    }
}

/// `$8785`: drop one `$AA` part into the highest free main slot 0-15 and mark
/// the dying boss's `$7C30` so its second draw is skipped.
fn spawn_aa(body: &mut Body, work: &Work, cx: &mut Cx<'_>) {
    let Some(slot) = (0..16).rev().find(|&i| cx.slots.main[i].kind == 0) else {
        return;
    };
    let child = u16::try_from(slot).expect("slots 0-15");
    body.sprite = 0x80;
    let [x_hi, x_lo] = work.x.to_be_bytes();
    let [y_hi, y_lo] = work.y.to_be_bytes();
    for (array, value) in [
        (0x7C30, 0x00),
        (0x7DD0, 0x00),
        (0x7E10, 0x00),
        (0x7E90, 0x00),
        (0x7B20, x_lo),
        (0x7B00, x_hi),
        (0x7B60, y_lo),
        (0x7B40, y_hi),
    ] {
        cx.set_byte(array + child, value);
    }
    cx.set_byte(0x7CC0 + child, 0xAA);
}

/// `$86CB`: project, draw the flash and both metasprites, and contact the player.
fn tail(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    if engine::on_screen(work, cx.world) != Visibility::On {
        engine::store(body, work);
        return;
    }
    cx.oam.chr[6] = CHR_6;
    cx.oam.chr[7] = CHR_7;
    let index = (cx.world.frame & 0x04) >> 1;
    work.pointer = engine::rom_word(cx.prg, 12, METASPRITES + u16::from(index));
    work.tile = 0;
    engine::draw(work, DRAW_BANK, cx);
    if body.sprite & 0x80 == 0 {
        let second = SECOND_METASPRITES + 2 * u16::from(body.sprite);
        work.pointer = engine::rom_word(cx.prg, 12, second);
        engine::draw(work, DRAW_BANK, cx);
    }
    work.hitbox = 2;
    engine::boss_contact(CONTACT, 0, body, work, cx);
    engine::store(body, work);
}

#[cfg(test)]
mod tests {
    use crate::actor::oracle::{Scene, assert_actor_phases};

    /// The A6's slot in the whole-game TAS's stage 6 boss room.
    const SLOT: usize = 1;

    /// `Scene::tas(frame)` with every other slot freed each loop, so the `$A8`,
    /// `$AA` and `$50` parts the boss spawns never run unported.
    fn fight(frame: u64) -> Option<Scene> {
        let mut scene = Scene::tas(frame)?;
        assert_eq!(scene.machine.bus.wram[0x1CC0 + SLOT], 0xA6);
        for slot in 0..24 {
            if slot == SLOT {
                continue;
            }
            let addr = 0x7CC0 + u16::try_from(slot).expect("slot < 24");
            scene.machine.bus.trace.pokes.push((0xD450, addr, 0));
        }
        Some(scene)
    }

    /// The TAS boss fight (area 1E:00, frames 36863-37663): phases 0-2, the
    /// `$A8` part spawns, the death and its `$50`/`$AA` spawns.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = fight(36863) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 800, &[0xA6]);
        assert!(coverage.occupied > 0, "{coverage:?}");
        assert!(coverage.drawn > 0, "{coverage:?}");
        assert_eq!(
            coverage.phases.keys().copied().collect::<Vec<_>>(),
            vec![0, 1, 2],
            "{coverage:?}"
        );
    }

    /// The boss's `$AFD6` touch, forced by pinning it onto the player each loop
    /// with `$0473` clear so he is not mid-attack: standing in it hurts him.
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = fight(36863) else {
            return;
        };
        scene.run(1, &[]);
        let player = &scene.machine.bus.ram[0x460..0x464];
        let (x_hi, x_lo, y_hi, y_lo) = (player[0], player[1], player[2], player[3]);
        for (array, value) in [
            (0x7B00, x_hi),
            (0x7B20, x_lo),
            (0x7B40, y_hi),
            (0x7B60, y_lo),
        ] {
            let addr = array + u16::try_from(SLOT).expect("slot < 24");
            scene.machine.bus.trace.pokes.push((0xD450, addr, value));
        }
        scene.machine.bus.trace.pokes.push((0xD450, 0x0473, 0));
        let mut probe = scene.clone();
        let mut hurt = 0u8;
        for _ in 0..90 {
            probe.run(1, &[]);
            hurt = hurt.max(probe.machine.bus.ram[0x479]);
        }
        assert!(hurt > 0, "the oracle never took boss contact");
        let coverage = assert_actor_phases(scene, 90, &[0xA6]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }
}
