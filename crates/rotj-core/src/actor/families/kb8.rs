//! Type `$B8`, the last boss's fall (bank 12 `$93FC`), which type `$B4`'s death
//! (`$8F37`) hands a free slot: a fixed-screen metasprite that holds, flickers, then
//! sinks, and arms the ending (`$BA` 6).

use crate::actor::engine::{self, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The metasprite per `$7C30` frame (`$94B0`) and the CHR bank it puts in `$E7` (`$94B8`).
const FRAMES: u16 = 0x94B0;
const BANKS: u16 = 0x94B8;
/// `$81C6` maps bank 9 at `$A000` for the draw.
const DRAW_BANK: u8 = 9;
/// The `$BA` value that starts the ending (`$D967`).
const ENDING: u8 = 6;

/// Type `$B8` keeps its whole state in [`Body`]: the screen position in
/// `behaviour`/`subtype`, the frame in `sprite`, its timer in `anim`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$93FC`, phases at `$94AC`).
    pub fn step(&mut self, _kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        match body.phase {
            0 => {
                body.sprite = 0;
                body.behaviour = 0x80;
                body.subtype = 0x58;
                body.anim = body.anim.wrapping_add(1);
                if body.anim >= 0x80 {
                    body.phase = 1;
                    body.counter = 0;
                    body.anim = 0;
                }
            }
            1 if body.counter & 0x80 != 0 => {
                body.sprite = 3;
                if body.anim.is_multiple_of(4) {
                    body.subtype = body.subtype.wrapping_add(1);
                }
                body.anim = body.anim.wrapping_add(1);
                if body.anim >= 0x80 {
                    cx.effects.transition = ENDING;
                }
            }
            1 => {
                body.sprite = ((body.anim & 8) >> 3) + 1;
                body.anim = body.anim.wrapping_add(1);
                if body.anim >= 0x10 {
                    body.counter = 0x80;
                    body.anim = 0;
                }
            }
            phase => panic!("type $B8 phase {phase} is past the $94AC table"),
        }
        work.attr = 0;
        work.tile = 0;
        work.chr_slot = 0;
        work.screen_x = body.behaviour;
        work.screen_y = body.subtype;
        let frame = u16::from(body.sprite);
        cx.oam.chr[7] = engine::rom_byte(cx.prg, 12, BANKS + frame);
        work.pointer = engine::rom_word(cx.prg, 12, FRAMES + 2 * frame);
        engine::draw(work, DRAW_BANK, cx);
    }
}
