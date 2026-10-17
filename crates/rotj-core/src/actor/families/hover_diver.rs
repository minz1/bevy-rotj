//! Type `$0A`, the green enemy of stage 1-1 (bank 8 `$85FD`).

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

const METASPRITES: u16 = 0x8763;
const LAST_FRAME: u8 = 6;
const WAKE_DISTANCE: u16 = 0x300;
const HOVER: i16 = -6;
const DROP: u16 = 0x04C0;
const FLOOR_ROW: u8 = 0x0C;

phases! {
    /// The diver's phase (`$7DD0`), which picks its handler (`$875B`).
    Phase {
        /// Hovering until its timer runs out and the player comes close.
        Hovering = 0,
        /// Diving to the floor.
        Diving = 1,
        /// Landed, hovering in place for `$20` frames.
        Landed = 2,
        /// Rising back to its row.
        Rising = 3,
    }
}

/// Type `$0A` keeps its phase, timer and animation counter in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$85FD`, phases at `$875B`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        engine::integrate(body, work);
        match Phase::from_byte(body.phase) {
            Some(Phase::Hovering) => {
                work.vy = HOVER;
                if body.timer == 0 {
                    if engine::player_dx(work, &cx.effects.player).unsigned_abs() < WAKE_DISTANCE {
                        body.phase = Phase::Diving.byte();
                        body.counter = 0;
                        body.anim = 0;
                    }
                } else {
                    body.timer -= 1;
                }
            }
            Some(Phase::Diving) => {
                if work.y.to_be_bytes()[0] >= FLOOR_ROW {
                    work.y = u16::from_be_bytes([FLOOR_ROW - 1, 0xFF]);
                    cx.effects.sound[0] = 0x34;
                    work.vy = HOVER;
                    work.y = work.y.wrapping_add(DROP);
                    engine::spawn_parts(2, work, cx);
                    work.y = work.y.wrapping_sub(DROP);
                    body.phase = Phase::Landed.byte();
                    body.timer = 0x20;
                } else {
                    let swing = u16::from_be_bytes([body.anim, body.counter]);
                    [body.anim, body.counter] =
                        swing.wrapping_add(work.vy.cast_unsigned()).to_be_bytes();
                }
            }
            Some(Phase::Landed) => {
                work.vy = HOVER;
                body.timer = body.timer.wrapping_sub(1);
                if body.timer & 0x80 != 0 {
                    body.phase = Phase::Rising.byte();
                    work.vy = 0;
                }
            }
            Some(Phase::Rising) => {
                if work.y.to_be_bytes()[0] == body.subtype {
                    body.phase = Phase::Hovering.byte();
                    body.anim = 0;
                    work.vy = 0;
                    body.timer = 0x10;
                } else {
                    let rise = work.vy.to_be_bytes()[1].wrapping_sub(7).max(0xE8);
                    work.vy = i16::from_be_bytes([0xFF, rise]);
                    let (counter, carry) = body.counter.overflowing_add(rise);
                    body.counter = counter;
                    let anim = body.anim.wrapping_add(0xFF).wrapping_add(u8::from(carry));
                    body.anim = if anim & 0x80 == 0 { anim } else { 0 };
                }
            }
            None => panic!("type $0A phase {} is past the $875B table", body.phase),
        }
        if engine::on_screen(work, cx.world) == Visibility::On {
            let mut frame = (body.anim & 0x0F).min(LAST_FRAME);
            if body.phase == Phase::Rising.byte() {
                frame += 1;
            }
            work.pointer = engine::rom_word(cx.prg, 8, METASPRITES + 2 * u16::from(frame));
            work.tile = 0;
            work.attr = 1;
            engine::draw(work, 1, cx);
            work.hitbox = frame + 2;
            engine::contact(kind, body, work, 2, cx);
        }
        work.age = work.age.wrapping_add(1);
        engine::end_frame(body, work);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::route(893) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 102, &[0x0A]);
        let expected = Coverage {
            frames: 102,
            occupied: 102,
            drawn: 102,
            phases: BTreeMap::from([(0, 102)]),
        };
        assert_eq!(coverage, expected);
    }

    #[test]
    fn matches_the_oracle_up_close() {
        let Some(mut scene) = Scene::route(953) else {
            return;
        };
        let wram = &scene.machine.bus.wram;
        assert_eq!(wram[0x1CC3], 0x0A);
        let x = u16::from_be_bytes([wram[0x1B03], wram[0x1B23]]);
        scene.player(x - 0x200, 0x0BFF);
        scene.machine.bus.trace.pokes.push((0xD450, 0x7CCF, 0));
        let coverage = assert_actor_phases(scene, 200, &[0x0A]);
        assert_eq!((coverage.frames, coverage.occupied), (200, 200));
        assert!(
            (0..4).all(|phase| coverage.phases.get(&phase).is_some_and(|&n| n > 0)),
            "{:?}",
            coverage.phases
        );
    }

    #[test]
    fn off_screen_merges_the_last_drawn_attr() {
        let Some(mut scene) = Scene::route(960) else {
            return;
        };
        let wram = &mut scene.machine.bus.wram;
        let slot = (0..16).find(|&s| wram[0x1CC0 + s] == 0x0A).expect("a $0A");
        let camera = u16::from_be_bytes([scene.machine.bus.ram[0xCC], scene.machine.bus.ram[0xCD]]);
        let [hi, lo] = (camera + 300 * 16).to_be_bytes();
        (wram[0x1B00 + slot], wram[0x1B20 + slot]) = (hi, lo);
        scene.machine.bus.trace.pokes.push((0xD450, 0x99, 0xE0));
        let coverage = assert_actor_phases(scene, 30, &[0x0A]);
        assert_eq!((coverage.occupied, coverage.drawn), (30, 0));
    }
}
