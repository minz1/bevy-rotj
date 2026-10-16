//! Second-class type `$0A`, the platform that follows a path once stood on (bank 8 `$90C7`).

use crate::actor::engine::{rom_byte, rom_word};
use crate::actor::slots::{Body, PRESSED};
use crate::actor::world::Cx;

/// The direction it moves in, 0-3 right, left, down, up, 4 stopped (`$7BF0,x`).
const DIRECTION: u16 = 0x7BF0;
const STEP: u16 = 0x20;

/// The path is chosen by `state` and walked by `health`'s low byte (`$7BF8,x`); the
/// carry the player rides with is `subtype:phase` across and `hit:behaviour` down.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame (`$90C7`): still until the player first stands on it (`dying_timer`
    /// bit 7 latches `$7F10,x`), then 2 px a frame toward the path's next corner.
    pub fn step(&mut self, index: usize, kind: &mut u8, body: &mut Body, cx: &mut Cx<'_>) {
        let index = u16::try_from(index).expect("< 8");
        body.scratch = 0;
        body.anim = 0;
        if body.state & 0x80 != 0 && body.dying_timer & 0x80 == 0 {
            let pressed = cx.byte(PRESSED + index);
            if pressed & 0x80 == 0 {
                (body.hit, body.behaviour, body.subtype, body.phase) = (0, 0, 0, 0);
                return;
            }
            body.dying_timer = pressed;
        }
        let path = u16::from(body.state << 1);
        let corners = rom_word(cx.prg, 8, 0x9287 + path);
        let turns = rom_word(cx.prg, 8, 0x9297 + path);
        let direction = cx.byte(DIRECTION + index);
        if direction >= 4 {
            (body.hit, body.behaviour, body.subtype, body.phase) = (0, 0, 0, 0);
            return;
        }
        let carry = [
            (0, 0, 0x20, 0),
            (0, 0, 0xE0, 0xFF),
            (0x20, 0, 0, 0),
            (0x20, 0, 0, 0),
        ];
        (body.hit, body.behaviour, body.subtype, body.phase) = carry[usize::from(direction)];
        let position = match direction {
            0 => {
                body.x = body.x.wrapping_add(STEP);
                body.x
            }
            1 => {
                body.x = body.x.wrapping_sub(STEP);
                body.x
            }
            2 => {
                body.y = body.y.wrapping_add(STEP);
                body.y
            }
            _ => {
                body.y = body.y.wrapping_sub(STEP);
                body.y
            }
        };
        let corner = rom_word(cx.prg, 8, corners + 2 * u16::from(body.byte(0x7BE0)));
        if corner == position {
            turn(index, kind, body, turns, cx);
        }
    }
}

/// The next corner (`$920E`): the step wraps at the path's length `$92A7`; its turn
/// is a direction, `$FF` (the platform is freed), or a jump to a fixed position.
fn turn(index: u16, kind: &mut u8, body: &mut Body, turns: u16, cx: &mut Cx<'_>) {
    let mut step = body.byte(0x7BE0).wrapping_add(1);
    if step == rom_byte(cx.prg, 8, 0x92A7 + u16::from(body.state & 0x7F)) {
        step = 0;
    }
    body.set_byte(0x7BE0, step);
    let next = rom_byte(cx.prg, 8, turns + u16::from(step));
    let direction = if next & 0x80 == 0 {
        next
    } else if next == 0xFF {
        *kind = 0;
        return;
    } else {
        let jump = u16::from(next & 0x7F);
        let at = 0x92C4 + 4 * jump;
        let byte = |i: u16| rom_byte(cx.prg, 8, at + i);
        body.x = u16::from_be_bytes([byte(0), byte(1)]);
        body.y = u16::from_be_bytes([byte(2), byte(3)]);
        rom_byte(cx.prg, 8, 0x92C3 + jump)
    };
    cx.set_byte(DIRECTION + index, direction);
    let at = 0x92AF + 4 * u16::from(direction);
    let byte = |i: u16| rom_byte(cx.prg, 8, at + i);
    (body.hit, body.behaviour, body.subtype, body.phase) = (byte(0), byte(1), byte(2), byte(3));
}

#[cfg(test)]
mod tests {
    use crate::actor::oracle::{Scene, assert_second_phases};

    #[test]
    fn matches_the_oracle_in_stage_2_1() {
        let Some(scene) = Scene::tas(7250) else {
            return;
        };
        let coverage = assert_second_phases(scene, 320, &[0x0A]);
        assert!(coverage.occupied > 300, "{coverage:?}");
        assert!(coverage.drawn > 0, "{coverage:?}");
    }
}
