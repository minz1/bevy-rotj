//! Type `$2A` (bank 11 `$8284`), a three-phase flyer that drops `$2C` bombs.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::families::Brain;
use crate::actor::slots::Body;
use crate::actor::world::{Cx, sound};
use crate::actors::STATE_UNTARGETED;

const BODY_SPRITES: u16 = 0x8506;
const PART_SPRITES: u16 = 0x850A;
const BLAST_SPRITES: u16 = 0x850E;
const CHILD: u8 = 0x2C;
const DRAW_BANK: u8 = 1;
const SOURCE_BANK: u8 = 11;
const BLAST_DROP: u8 = 0x2B;
/// `subtype` bit 7: the blast flash draws; the bomb's drop sets it, `counter` reaching `$86`
/// clears it.
const BLASTING: u8 = 0x80;

phases! {
    /// The bomber's phase (`$7DD0`): the game stores its doubled index into `$8514`.
    Phase {
        /// Starting the first dive.
        Starting = 0,
        /// Sweeping sideways and flapping, then diving.
        Sweeping = 2,
        /// Dropping a bomb, hovering, then rising.
        Bombing = 4,
    }
}

/// Type `$2A` keeps its whole state in [`Body`]; `subtype` says whether the blast flash draws.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8284`; phases through the table at `$8514`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if engine::dying(kind, body, work, 0x80, cx) {
            return;
        }
        work.attr |= body.flags;
        engine::integrate(body, work);
        match Phase::from_byte(body.phase) {
            Some(Phase::Starting) => starting(body, work),
            Some(Phase::Sweeping) => sweeping(body, work, cx),
            Some(Phase::Bombing) => bombing(body, work, cx),
            None => panic!("type $2A phase {} is past the $8514 table", body.phase),
        }
        tail(kind, body, work, cx);
    }
}

fn camera_row(cx: &Cx<'_>) -> u8 {
    cx.world.camera_y.to_be_bytes()[0]
}

/// Starting (`$82AB`): set the first dive going.
fn starting(body: &mut Body, work: &mut Work) {
    body.sprite = 0;
    body.anim = 0;
    body.phase = Phase::Sweeping.byte();
    work.step_x = 4;
    work.vy = i16::from_be_bytes([0xFF, 0xF0]);
    body.counter = 0xFA;
}

/// Sweeping (`$82CD`): steer the lift on odd frames, sweep sideways, then flap.
fn sweeping(body: &mut Body, work: &mut Work, cx: &Cx<'_>) {
    if cx.world.frame_odd {
        let rows = work.y.to_be_bytes()[0].checked_sub(camera_row(cx));
        body.counter = match rows {
            Some(5..=6) => {
                if cx.world.rng[0] & 0x80 == 0 {
                    0xF7
                } else {
                    0xFC
                }
            }
            Some(7..) => {
                work.vy = 0;
                0xF7
            }
            _ => {
                work.vy = 0;
                0xFA
            }
        };
    }
    let [hi, lo] = work.vy.to_be_bytes();
    let (lo, carry) = lo.overflowing_add(body.counter);
    work.vy = i16::from_be_bytes([hi.wrapping_add(0xFF).wrapping_add(u8::from(carry)), lo]);
    if body.timer & 0x80 == 0 {
        let column = work.x.to_be_bytes()[0];
        if work.step_x >= 0 {
            if column >= 0x0C {
                work.step_x = -4;
            }
        } else if column < 0x04 {
            work.step_x = 4;
        }
        body.anim = body.anim.wrapping_add(1);
        if body.anim < 0xC0 {
            return;
        }
        work.step_x = 0;
        body.timer = 0x80;
        body.anim = 0;
    }
    if body.anim >= 0x40 {
        body.phase = Phase::Bombing.byte();
        body.sprite = 0;
        body.anim = 0;
        body.timer = 0;
        work.vy = i16::from_be_bytes([0xFF, 0xF0]);
        work.step_x = if work.x.to_be_bytes()[0] >= 8 { -8 } else { 8 };
    } else {
        body.sprite = u8::from(body.anim & 8 == 0);
        body.anim = body.anim.wrapping_add(1);
    }
}

/// Bombing (`$8395`): drop a bomb, hover for 64 frames, then rise and return to sweeping.
fn bombing(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    if body.timer & 0x80 == 0 {
        if body.counter & 0x80 == 0 {
            cx.effects.sound[0] = sound::IMPACT;
            spawn_bomb(body, work, cx);
            body.counter = 0x80;
            body.subtype = BLASTING;
        } else {
            advance_blast(body);
        }
        body.anim = body.anim.wrapping_add(1);
        if body.anim >= 0x40 {
            body.timer = 0x80;
        }
        return;
    }
    work.vy = work.vy.wrapping_sub(8);
    if body.counter & 0x80 != 0 {
        body.counter = body.counter.wrapping_add(1);
        if body.counter < 0x86 {
            body.anim = body.anim.wrapping_add(1);
            if body.anim >= 0x40 {
                body.timer = 0x80;
            }
            return;
        }
        body.counter = 0;
        body.subtype = 0;
    }
    if work.y.to_be_bytes()[0].wrapping_sub(camera_row(cx)) < 4 {
        body.phase = Phase::Sweeping.byte();
        body.timer = 0;
        body.anim = 0;
        body.counter = 0xF0;
    }
}

/// `$83B6`: step the blast flash, ending it at `$86`.
fn advance_blast(body: &mut Body) {
    body.counter = body.counter.wrapping_add(1);
    if body.counter >= 0x86 {
        body.counter = 0;
        body.subtype = 0;
    }
}

/// `$84BD`: writes the `$2C` bomb into the highest free slot 0-15.
fn spawn_bomb(parent: &mut Body, work: &Work, cx: &mut Cx<'_>) {
    let Some(slot) = (0..16).rev().find(|&slot| cx.slots.main[slot].kind == 0) else {
        return;
    };
    let child = &mut cx.slots.main[slot];
    child.body.x = work.x;
    child.body.y = work.y.wrapping_add(0x02B0);
    child.body.vy = 0x40;
    child.body.state = STATE_UNTARGETED;
    child.body.phase = 0;
    parent.vx = 0;
    child.body.chr_slot = parent.chr_slot;
    child.kind = CHILD;
    child.body.contact = CHILD;
    child.brain = Brain::new(CHILD);
}

/// `$841C`: draw the body, its part and the blast flash, test contact, then
/// clamp the y velocity and store.
fn tail(kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    if engine::on_screen(work, cx.world) == Visibility::On {
        let pick = u16::from(cx.world.frame & 2);
        work.pointer = engine::rom_word(cx.prg, SOURCE_BANK, BODY_SPRITES + pick);
        engine::tile_bits(work, cx.prg);
        engine::draw(work, DRAW_BANK, cx);
        let part = 2 * u16::from(body.sprite);
        work.pointer = engine::rom_word(cx.prg, SOURCE_BANK, PART_SPRITES + part);
        engine::draw(work, DRAW_BANK, cx);
        if body.subtype & BLASTING != 0 {
            let frame = u16::from(body.counter & 0x0E);
            work.pointer = engine::rom_word(cx.prg, SOURCE_BANK, BLAST_SPRITES + frame);
            if let Some(low) = work.screen_y.checked_add(BLAST_DROP) {
                let top = work.screen_y;
                work.screen_y = low;
                engine::draw(work, DRAW_BANK, cx);
                work.screen_y = top;
            }
        }
        work.hitbox = 1;
        engine::contact(kind, body, work, 2, cx);
    }
    let [hi, lo] = work.vy.to_be_bytes();
    work.vy = if hi & 0x80 != 0 {
        if lo >= 0xF5 {
            work.vy
        } else {
            i16::from_be_bytes([0xF0, lo])
        }
    } else if lo >= 3 {
        3
    } else {
        work.vy
    };
    engine::end_frame(body, work);
    body.vx_drift = work.step_x;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    const BOMB_SLOT: u16 = 0x7CCF;

    /// The TAS flyer (slot 0, area 12:02) kept alive, its `$2C` bombs removed each loop.
    fn flyer(frame: u64) -> Option<Scene> {
        let mut scene = Scene::tas(frame)?;
        for (at, value) in [(BOMB_SLOT, 0), (0x7EF0, 0x7F), (0x7BE0, 0xFF)] {
            scene.machine.bus.trace.pokes.push((0xD450, at, value));
        }
        Some(scene)
    }

    fn expect(coverage: &Coverage, frames: usize, drawn: usize, phases: &[(u8, usize)]) {
        let expected = Coverage {
            frames,
            occupied: frames,
            drawn,
            phases: BTreeMap::from_iter(phases.iter().copied()),
        };
        assert_eq!(coverage, &expected);
    }

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = flyer(21030) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 200, &[0x2A]);
        expect(&coverage, 200, 197, &[(2, 190), (4, 10)]);
    }

    #[test]
    fn spawns_into_the_dive_off_screen() {
        let Some(scene) = flyer(20960) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 60, &[0x2A]);
        assert_eq!(coverage.drawn, 0);
        assert_eq!(coverage.phases, BTreeMap::from([(0, 1), (2, 56)]));
    }

    #[test]
    fn drops_a_bomb_hovers_and_rises() {
        let Some(scene) = flyer(21215) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 130, &[0x2A]);
        expect(&coverage, 130, 130, &[(2, 37), (4, 93)]);
    }

    #[test]
    fn steers_the_lift() {
        let Some(scene) = flyer(21400) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 80, &[0x2A]);
        expect(&coverage, 80, 80, &[(2, 80)]);
    }

    #[test]
    fn turns_at_both_edges() {
        for (frame, frames, column) in [(21030, 40, 0x0D), (21160, 80, 0x05)] {
            let Some(mut scene) = flyer(frame) else {
                return;
            };
            scene.machine.bus.trace.pokes.push((0xD450, 0x7B00, column));
            let coverage = assert_actor_phases(scene, frames, &[0x2A]);
            assert_eq!(coverage.occupied, usize::try_from(frames).unwrap());
        }
    }

    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = flyer(21030) else {
            return;
        };
        scene.isolate(&[0x2A]);
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00], wram[0x1B20]]);
        let y = u16::from_be_bytes([wram[0x1B40], wram[0x1B60]]);
        scene.script(200, |_| 0);
        scene.player(x, y);
        let mut probe = scene.clone();
        let mut energy = probe.machine.bus.ram[0x478];
        let mut hits = 0;
        for _ in 0..40 {
            probe.run(1, &[]);
            let now = probe.machine.bus.ram[0x478];
            if now < energy {
                hits += 1;
                energy = now;
            }
        }
        assert!(hits > 0, "the oracle takes no contact hit");
        let coverage = assert_actor_phases(scene, 40, &[0x2A]);
        assert_eq!(coverage.occupied, 40);
    }
}
