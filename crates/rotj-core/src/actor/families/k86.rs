//! Type `$86` (bank 0 `$9516`): four drifting parts drawn two per frame.
//!
//! It is the child of type `$84` (bank 0 `$9361`, `$93CF`), which spawns it with
//! `$AC16` variant 6; that variant's 24 arrays are exactly the descriptors this
//! handler reads (`$95C6`), so each part keeps a 16-bit position and a signed
//! velocity in the slot arrays and falls 12 a frame.
//!
//! It calls only routines in `docs/ACTORS.md` §9c: `$A852` (`on_screen`) and
//! `$C247` (`draw`, bank 0 as the handler itself is mapped there). No `$A000`
//! load, no physics, no contact.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The metasprite pointer word (`$95BC` -> `$95BE`).
const SPRITE_WORD: u16 = 0x95BC;
/// The part descriptor table word (`$95C4` -> `$95C6`, four 12-byte records).
const PARTS_WORD: u16 = 0x95C4;
/// The handler runs with bank 0 at `$8000`; its metasprites live there.
const DRAW_BANK: u8 = 0;
/// The parts per actor.
const PARTS: u8 = 4;
/// One part record's size in the descriptor table.
const PART_SIZE: u16 = 0x0C;
/// Gravity added to each part's y velocity at the end of its update.
const GRAVITY: u8 = 0x0C;
/// The countdown decremented once a frame; below 0 the slot frees.
const LIFE: u16 = 0x7ED0;

/// Type `$86` keeps its whole state in [`Body`] (each part's arrays).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9516`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        work.pointer = engine::rom_word(cx.prg, DRAW_BANK, SPRITE_WORD);
        work.tile = 0;
        work.attr = body.flags;
        let mut parts = engine::rom_word(cx.prg, DRAW_BANK, PARTS_WORD);
        for part in (0..PARTS).rev() {
            if (part ^ cx.world.frame) & 1 != 0 {
                update(cx.prg, parts, body, work, cx);
            }
            parts = parts.wrapping_add(PART_SIZE);
        }
        let life = body.byte(LIFE).wrapping_sub(1);
        body.set_byte(LIFE, life);
        if life & 0x80 != 0 {
            *kind = 0;
        }
    }
}

/// One part: integrate its velocity, then draw it (`$954B`).
fn update(prg: &[u8], descriptor: u16, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    let array = |i: u16| engine::rom_word(prg, DRAW_BANK, descriptor + 2 * i);
    let (x_lo, x_hi, y_lo, y_hi, vx, vy) =
        (array(0), array(1), array(2), array(3), array(4), array(5));
    let x = u16::from_be_bytes([body.byte(x_hi), body.byte(x_lo)])
        .wrapping_add_signed(i16::from(body.byte(vx).cast_signed()));
    body.set_byte(x_hi, x.to_be_bytes()[0]);
    body.set_byte(x_lo, x.to_be_bytes()[1]);
    work.x = x;
    let y = u16::from_be_bytes([body.byte(y_hi), body.byte(y_lo)])
        .wrapping_add_signed(i16::from(body.byte(vy).cast_signed()));
    body.set_byte(y_hi, y.to_be_bytes()[0]);
    body.set_byte(y_lo, y.to_be_bytes()[1]);
    work.y = y;
    body.set_byte(vy, body.byte(vy).wrapping_add(GRAVITY));
    if engine::on_screen(work, cx.world) == Visibility::On {
        engine::draw(work, DRAW_BANK, cx);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(33640) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 300, &[0x86]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 256,
                occupied: 31,
                drawn: 31,
                phases: BTreeMap::from([(0x10, 31)]),
            },
            "{coverage:?}"
        );
    }

    #[test]
    fn matches_the_oracle_with_three_alive() {
        let Some(scene) = Scene::tas(33671) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 100, &[0x86]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 100,
                occupied: 33,
                drawn: 33,
                phases: BTreeMap::from([(0x10, 51)]),
            },
            "{coverage:?}"
        );
    }
}
