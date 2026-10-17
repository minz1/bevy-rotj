//! Second-class type `$0A`, the platform that follows a path once stood on (bank 8 `$90C7`).

use crate::actor::engine::{self, rom_byte, rom_word};
use crate::actor::slots::{Body, Field};
use crate::actor::world::Cx;

/// The path's corner index, which the game keeps in health's low byte.
const CORNER: Field = Field::HealthLow;
const STEP: u16 = 0x20;

/// Second-class type `$0A`'s tables (bank 8).
pub struct Tables {
    /// The path's length in steps, by `state & $7F` (`$92A7`).
    pub lengths: [u8; 128],
    /// A jump's direction, by `next & $7F` (`$92C3`).
    pub turn_directions: [u8; 128],
    /// A jump's `(x, y)`, by `next & $7F` (`$92C4`).
    pub jump_positions: [(u16, u16); 128],
    /// The `(hit, behaviour, subtype, phase)` a turn sets, by direction (any byte) (`$92AF`).
    pub carries: [(u8, u8, u8, u8); 256],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        let jumps: [u8; 512] = engine::rom_bytes(prg, 8, 0x92C4);
        let carries: [u8; 1024] = engine::rom_bytes(prg, 8, 0x92AF);
        Tables {
            lengths: engine::rom_bytes(prg, 8, 0x92A7),
            turn_directions: engine::rom_bytes(prg, 8, 0x92C3),
            jump_positions: std::array::from_fn(|i| {
                (
                    u16::from_be_bytes([jumps[4 * i], jumps[4 * i + 1]]),
                    u16::from_be_bytes([jumps[4 * i + 2], jumps[4 * i + 3]]),
                )
            }),
            carries: std::array::from_fn(|i| {
                (
                    carries[4 * i],
                    carries[4 * i + 1],
                    carries[4 * i + 2],
                    carries[4 * i + 3],
                )
            }),
        }
    }
}

/// The path is chosen by `state` and walked by `health`'s low byte (`$7BF8,x`); the
/// carry the player rides with is `subtype:phase` across and `hit:behaviour` down.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame (`$90C7`): still until the player first stands on it (`dying_timer`
    /// bit 7 latches `$7F10,x`), then 2 px a frame toward the path's next corner.
    pub fn step(&mut self, index: usize, kind: &mut u8, body: &mut Body, cx: &mut Cx<'_>) {
        body.scratch = 0;
        body.anim = 0;
        if body.state & 0x80 != 0 && body.dying_timer & 0x80 == 0 {
            let pressed = cx.slots.pressed(index);
            if pressed & 0x80 == 0 {
                (body.hit, body.behaviour, body.subtype, body.phase) = (0, 0, 0, 0);
                return;
            }
            body.dying_timer = pressed;
        }
        let path = u16::from(body.state << 1);
        let corners = rom_word(cx.prg, 8, 0x9287 + path);
        let turns = rom_word(cx.prg, 8, 0x9297 + path);
        // The direction: 0-3 right, left, down, up; 4 stopped.
        let direction = cx.slots.platform_parameter(index);
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
        let corner = rom_word(cx.prg, 8, corners + 2 * u16::from(body.get(CORNER)));
        if corner == position {
            turn(index, kind, body, turns, cx);
        }
    }
}

/// The next corner (`$920E`): the step wraps at the path's length `$92A7`; its turn
/// is a direction, `$FF` (the platform is freed), or a jump to a fixed position.
fn turn(index: usize, kind: &mut u8, body: &mut Body, turns: u16, cx: &mut Cx<'_>) {
    let tables = &cx.prg.families.path_platform;
    let mut step = body.get(CORNER).wrapping_add(1);
    if step == tables.lengths[usize::from(body.state & 0x7F)] {
        step = 0;
    }
    body.set(CORNER, step);
    let next = rom_byte(cx.prg, 8, turns + u16::from(step));
    let direction = if next & 0x80 == 0 {
        next
    } else if next == 0xFF {
        *kind = 0;
        return;
    } else {
        let jump = usize::from(next & 0x7F);
        (body.x, body.y) = tables.jump_positions[jump];
        tables.turn_directions[jump]
    };
    cx.slots.set_platform_parameter(index, direction);
    (body.hit, body.behaviour, body.subtype, body.phase) = tables.carries[usize::from(direction)];
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
