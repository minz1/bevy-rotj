//! Type `$38`, the stage 1 boss (bank 11 `$8D1F`).
//!
//! Phases through the table at `$910D` (`$7DD0` 0-4); every phase ends in the
//! shared draw/contact tail at `$8EDB`. It spawns type `$3A` parts into slots
//! 16-23 (`$8FB9`) and, from phase 2, type `$3C` parts into slots 0-15 (`$909B`).

use crate::actor::collision::{self, Probe};
use crate::actor::engine::{self, Visibility, Work};
use crate::actor::families::Brain;
use crate::actor::slots::Body;
use crate::actor::world::{Cx, Motion};

/// The phase -> next-phase table (`$911F`), indexed by the `$7E90` counter.
const NEXT_PHASE: u16 = 0x911F;
/// Metasprite pointers (`$90E7`), indexed by `$7C30`.
const METASPRITES: u16 = 0x90E7;
/// The CHR bank written to the `$E7` shadow (`$9129`), by `$7C30`.
const CHR_SHADOW: u16 = 0x9129;
/// Metasprite pointers for the `$7E10` bit-6 second draw (`$9136`).
const SECOND_DRAW: u16 = 0x9136;
/// Metasprite pointers for the `$7E10` bit-7 third draw (`$913A`), by phase.
const THIRD_DRAW: u16 = 0x913A;
/// `$3C` part x/y high bytes (`$913E`/`$9141`), indexed by the spawn count.
const SPAWN_X: u16 = 0x913E;
const SPAWN_Y: u16 = 0x9141;
/// `$C229` maps bank 9 into `$A000` for the draws, where the metasprites live.
const DRAW_BANK: u8 = 9;
/// The `$42` request when a part spawns.
const SPAWN_SOUND: u8 = 0x28;
/// The `$AFD6` amount the boss drains from `$B2` on contact.
const CONTACT: u16 = 0x00E0;

/// Type `$38` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8D1F`, phases at `$910D`).
    pub fn step(&mut self, _kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        let flash = engine::boss_flash(body, work, cx.world);
        if flash & 0x80 != 0 {
            work.attr = body.flags;
            if body.dying_timer == 0x01 {
                // `$B1C4` takes the dying slot's index in X as the part's timer.
                engine::boss_defeated(work.slot, cx);
            }
            if flash & 0x40 != 0 {
                finish(body, work, cx);
            }
            return;
        }
        work.attr |= body.flags;
        engine::integrate(body, work);
        let mut probe = Probe::default();
        let grounded = collision::feet(work, &mut probe, cx) & 0x80 == 0;
        let hi = work.x.to_be_bytes()[0];
        if hi < 0x02 {
            work.x = 0x0200;
        } else if hi >= 0x0E {
            work.x = 0x0E00;
        }
        if grounded {
            body.vx_drift = drift(probe);
        }
        match body.phase {
            0 => phase0(body, cx),
            1 => phase1(body, work, cx),
            2 => phase2(body, cx),
            3 => phase3(body, work, cx),
            4 => phase4(body, work, grounded, cx),
            phase => panic!("type $38 phase {phase} is past the $910D table"),
        }
        finish(body, work, cx);
    }
}

/// `$9067`: count `$7E90` on and take the next phase, clearing the timer.
fn advance(body: &mut Body, prg: &[u8]) {
    let anim = body.anim.wrapping_add(1);
    body.anim = if anim >= 0x0A { 0 } else { anim };
    body.phase = engine::rom_byte(prg, 11, NEXT_PHASE + u16::from(body.anim));
    body.timer = 0;
    body.subtype = 0;
}

/// `$9088`: face the player and keep only the low two attribute bits.
fn face(work: &mut Work, cx: &Cx<'_>) {
    let dx = engine::player_dx(work, &cx.effects.player);
    work.attr = (work.attr & 0x03) | engine::face(dx);
}

/// `$8D9F`: stand, then leave unless the level state is `$09`.
fn phase0(body: &mut Body, cx: &mut Cx<'_>) {
    body.sprite = 4;
    if cx.effects.player.motion != Motion::BossEntry {
        advance(body, cx.prg);
    }
}

/// `$8DB1`: wait, spawn `$3A` parts on the timer, then jump out at `$A0`.
fn phase1(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    body.counter = 0;
    face(work, cx);
    if body.timer & 0x80 == 0 {
        let old = body.timer;
        body.timer = body.timer.wrapping_add(1);
        if old.is_multiple_of(8) {
            spawn_3a(body, work, cx);
            if body.timer & 0x40 != 0 {
                body.timer = 0x80;
            }
        }
        if body.timer.is_multiple_of(4) {
            body.counter = 0x80;
        }
        body.sprite = 4;
    } else {
        body.sprite = if body.timer & 0x08 != 0 { 5 } else { 4 };
        body.timer = body.timer.wrapping_add(1);
        if body.timer >= 0xA0 {
            advance(body, cx.prg);
        }
    }
}

/// `$8E09`: hover for `$40` frames, then spawn three `$3C` parts and leave.
fn phase2(body: &mut Body, cx: &mut Cx<'_>) {
    body.counter = 0x40 | ((body.timer & 0x04) >> 2);
    body.sprite = 0x0A;
    body.timer = body.timer.wrapping_add(1);
    if body.timer >= 0x40 {
        advance(body, cx.prg);
        spawn_3c(cx);
    }
}

/// `$8E2D`: a short wait that spawns `$3A` parts, then leaves at `$50`.
fn phase3(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    body.counter = 0;
    body.sprite = 9;
    if body.timer >= 0x20 && body.timer.is_multiple_of(8) {
        spawn_3a(body, work, cx);
    }
    body.timer = body.timer.wrapping_add(1);
    if body.timer >= 0x50 {
        advance(body, cx.prg);
    } else if body.timer >= 0x20 && body.timer.is_multiple_of(4) {
        body.counter = 0x80;
    }
}

/// `$8E67`: wind up for `$20` frames into a jump (`vy` -160), land, then leave.
fn phase4(body: &mut Body, work: &mut Work, grounded: bool, cx: &mut Cx<'_>) {
    body.counter = 0;
    if body.timer & 0x80 != 0 {
        if grounded {
            body.timer = 0;
            body.subtype = 0x80;
            body.sprite = 6;
        } else {
            body.sprite = 7;
        }
        return;
    }
    body.sprite = 6;
    if body.subtype & 0x80 != 0 {
        body.timer = body.timer.wrapping_add(1);
        if body.timer >= 0x10 {
            advance(body, cx.prg);
        }
        return;
    }
    if body.timer == 0 {
        face(work, cx);
    }
    body.timer = body.timer.wrapping_add(1);
    if body.timer >= 0x20 {
        work.vy = -0xA0;
        body.vx_drift = if work.attr & 0x40 != 0 { -0x20 } else { 0x20 };
        body.timer = 0x80;
    }
}

/// `$8EDB`: draw the metasprites and run the boss contact, or just store off screen.
fn finish(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    if engine::on_screen(work, cx.world) != Visibility::On {
        work.age = work.age.wrapping_add(1);
        engine::end_frame(body, work);
        return;
    }
    let chr7 = engine::rom_byte(cx.prg, 11, CHR_SHADOW + u16::from(body.sprite));
    cx.oam.chr[6] = 0xB8;
    cx.oam.chr[7] = chr7;
    work.pointer = engine::rom_word(cx.prg, 11, METASPRITES + 2 * u16::from(body.sprite));
    work.tile = 0;
    engine::draw(work, DRAW_BANK, cx);
    if body.counter & 0x40 != 0 {
        let y = (body.counter << 1) & 0x03;
        work.pointer = engine::rom_word(cx.prg, 11, SECOND_DRAW + u16::from(y));
        engine::draw(work, DRAW_BANK, cx);
    }
    if body.counter & 0x80 != 0 {
        third_draw(body, work, cx);
    }
    work.hitbox = 0;
    engine::boss_contact(CONTACT, 0, body, work, cx);
    work.age = work.age.wrapping_add(1);
    engine::end_frame(body, work);
}

/// `$8F2E`: the `$7E10` bit-7 draw, offset off the working position and put back.
fn third_draw(body: &Body, work: &mut Work, cx: &mut Cx<'_>) {
    let saved_y = work.screen_y;
    let saved_x = work.screen_x;
    let index = if body.phase == 1 {
        let Some(y) = saved_y.checked_sub(0x3B) else {
            return;
        };
        work.screen_y = y;
        let moved = if work.attr & 0x40 == 0 {
            work.screen_x.checked_add(0x1B)
        } else {
            work.screen_x.checked_sub(0x1B)
        };
        let Some(x) = moved else {
            return;
        };
        work.screen_x = x;
        0u8
    } else {
        let Some(y) = saved_y.checked_sub(0x1E) else {
            return;
        };
        work.screen_y = y;
        let moved = if work.attr & 0x40 == 0 {
            work.screen_x.checked_add(0x27)
        } else {
            work.screen_x.checked_sub(0x27)
        };
        let Some(x) = moved else {
            work.screen_x = saved_x;
            work.screen_y = saved_y;
            return;
        };
        work.screen_x = x;
        2u8
    };
    work.pointer = engine::rom_word(cx.prg, 11, THIRD_DRAW + u16::from(index));
    engine::draw(work, DRAW_BANK, cx);
    work.screen_y = saved_y;
    work.screen_x = saved_x;
}

/// `$8FB9`: spawn a type `$3A` part into the highest free slot 16-23.
fn spawn_3a(body: &Body, work: &Work, cx: &mut Cx<'_>) {
    let Some(slot) = (16..24).rev().find(|&i| cx.slots.main[i].kind == 0) else {
        return;
    };
    cx.effects.sound[0] = SPAWN_SOUND;
    put(cx, 0x7C00, slot, work.attr);
    let left = work.attr & 0x40 != 0;
    let (dx, dy, phase, frame) = if body.phase == 1 {
        (0x01D0, 0x03B0, if left { 2 } else { 3 }, 1)
    } else {
        (0x0270, 0x01E0, u8::from(left), 0)
    };
    let x = if left {
        work.x.wrapping_sub(dx)
    } else {
        work.x.wrapping_add(dx)
    };
    let y = work.y.wrapping_sub(dy);
    put(cx, 0x7B20, slot, x.to_be_bytes()[1]);
    put(cx, 0x7B00, slot, x.to_be_bytes()[0]);
    put(cx, 0x7B60, slot, y.to_be_bytes()[1]);
    put(cx, 0x7B40, slot, y.to_be_bytes()[0]);
    put(cx, 0x7DD0, slot, phase);
    put(cx, 0x7C30, slot, frame);
    put(cx, 0x7C90, slot, body.chr_slot);
    put(cx, 0x7CC0, slot, 0x3A);
    put(cx, 0x7D50, slot, 0);
    cx.slots.main[slot].brain = Brain::new(0x3A);
}

/// `$909B`: spawn up to three type `$3C` parts into the highest free slots 0-15.
fn spawn_3c(cx: &mut Cx<'_>) {
    let mut count = 2u8;
    loop {
        let Some(slot) = (0..16).rev().find(|&i| cx.slots.main[i].kind == 0) else {
            return;
        };
        let x = engine::rom_byte(cx.prg, 11, SPAWN_X + u16::from(count));
        let y = engine::rom_byte(cx.prg, 11, SPAWN_Y + u16::from(count));
        put(cx, 0x7B00, slot, x);
        put(cx, 0x7B40, slot, y);
        put(cx, 0x7B60, slot, 0);
        put(cx, 0x7B20, slot, 0);
        put(cx, 0x7D50, slot, 0);
        put(cx, 0x7D70, slot, 0);
        put(cx, 0x7EF0, slot, 0);
        put(cx, 0x7DF0, slot, 0);
        put(cx, 0x7E10, slot, 0);
        put(cx, 0x7EE0, slot, 0x10);
        put(cx, 0x7BE0, slot, 0x08);
        put(cx, 0x7CC0, slot, 0x3C);
        cx.slots.main[slot].brain = Brain::new(0x3C);
        if count == 0 {
            break;
        }
        count -= 1;
    }
}

/// Writes one byte of a child slot's array.
fn put(cx: &mut Cx<'_>, base: u16, slot: usize, value: u8) {
    cx.set_byte(base + u16::try_from(slot).expect("slot < 32"), value);
}

/// The `$27/$28` landing bytes the game stores into `$7E50:$7E60`.
fn drift(probe: Probe) -> i16 {
    i16::from_be_bytes([probe.landing[1], probe.landing[0]])
}

#[cfg(test)]
mod tests {
    use crate::actor::oracle::{Scene, assert_actor_phases};

    /// The TAS boss fight (area 4, frames 6281-6771). Every non-boss slot is
    /// cleared each loop so the `$3A`/`$3C` parts it spawns never run unported.
    fn fight(frame: u64) -> Option<Scene> {
        let mut scene = Scene::tas(frame)?;
        let boss = (0..24)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x38)
            .expect("a $38");
        for slot in 0..24 {
            if slot == boss {
                continue;
            }
            let addr = 0x7CC0 + u16::try_from(slot).expect("slot < 24");
            scene.machine.bus.trace.pokes.push((0xD450, addr, 0));
        }
        Some(scene)
    }

    /// The TAS fight's natural run: phases 0, 3 and 4, the death and its
    /// `$B1C4` spawn. The fight lasts 490 frames; 285 actor loops are compared
    /// (area 4 arms the FME-7 IRQ, whose loops `brackets` drops).
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = fight(6282) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 490, &[0x38]);
        assert!(coverage.occupied > 0, "{coverage:?}");
        assert!(coverage.drawn > 0, "{coverage:?}");
        for phase in [0u8, 3, 4] {
            assert!(
                coverage.phases.get(&phase).is_some_and(|&n| n > 0),
                "phase {phase}: {:?}",
                coverage.phases
            );
        }
    }

    /// The published TAS never reaches phases 1 and 2: `$046E` pins phase 0 for
    /// the fight's first ~98 frames and the boss dies at frame ~6574, before the
    /// `$7E90` counter cycles to the `1`/`2` entries. Forcing `$7DD0` each loop
    /// runs the same 6502 path in both machines, so the diff proves those phases.
    #[test]
    fn every_phase_matches_the_oracle() {
        for phase in [1u8, 2] {
            let Some(mut scene) = fight(6282) else {
                return;
            };
            let boss = (0..24)
                .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x38)
                .expect("a $38");
            let at = 0x7DD0 + u16::try_from(boss).expect("slot < 24");
            scene.machine.bus.trace.pokes.push((0xD450, at, phase));
            let coverage = assert_actor_phases(scene, 200, &[0x38]);
            assert!(
                coverage.phases.get(&phase).is_some_and(|&n| n > 0),
                "phase {phase}: {:?}",
                coverage.phases
            );
        }
    }

    /// `$AFD6` on the player: standing in the boss is contact, and the oracle
    /// takes the hurt (`$0479` is set to `$20`).
    #[test]
    fn the_boss_hurts_the_player_up_close() {
        let Some(mut scene) = fight(6282) else {
            return;
        };
        scene.isolate(&[0x38]);
        scene.run(1, &[]);
        let boss = (0..24)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x38)
            .expect("a $38");
        // The player is pinned in this arena; bring the boss onto his screen
        // position each loop so `$AFD6` reaches its hurt branch. The camera
        // `$CC` reads zero here, so level x is screen x times 16.
        let (px, py) = (scene.machine.bus.ram[0xB0], scene.machine.bus.ram[0xB1]);
        let (bx, by) = (u16::from(px) * 16, u16::from(py) * 16);
        for (addr, value) in [
            (0x7B00, bx.to_be_bytes()[0]),
            (0x7B20, bx.to_be_bytes()[1]),
            (0x7B40, by.to_be_bytes()[0]),
            (0x7B60, by.to_be_bytes()[1]),
        ] {
            scene.machine.bus.trace.pokes.push((
                0xD450,
                addr + u16::try_from(boss).expect("slot < 24"),
                value,
            ));
        }
        let mut probe = scene.clone();
        let mut hurt = 0u8;
        for _ in 0..90 {
            probe.run(1, &[]);
            hurt = hurt.max(probe.machine.bus.ram[0x479]);
        }
        assert!(hurt > 0, "the oracle never took boss contact");
        let coverage = assert_actor_phases(scene, 90, &[0x38]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }
}
