//! Type `$5E` (bank 0 `$831D`): a swinging three-piece sprite whose pieces sit
//! at offsets from tables indexed by a `$7DD0` countdown.
//!
//! It calls only routines in `docs/ACTORS.md` §9c: `$A852` (`on_screen`), `$C247`
//! (`draw`, bank 9 as `$84CA` maps it), `$AEE3` (`touch`) and `$CD50` (`despawn`).
//! It never loads the slot into the working set: each piece's position is the
//! slot's own position plus a table offset.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

const POINTERS: u16 = 0x83C2;
const X_OFFSETS: u16 = 0x83CA;
const Y_OFFSETS: u16 = 0x844A;
const DRAW_BANK: u8 = 9;
const ATTR: u8 = 3;
const HITBOX: u8 = 5;
const DAMAGE: u8 = 2;
const PIECES: u8 = 3;
const COUNTDOWN_STEP: u8 = 4;
const COUNTDOWN_RESTART: u8 = 0x7C;

/// Type `$5E` keeps its countdown in [`Body`] (`$7DD0`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$831D`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        let frame = cx.world.frame;
        if frame.is_multiple_of(4) {
            let (countdown, borrow) = body.phase.overflowing_sub(COUNTDOWN_STEP);
            body.phase = if borrow { COUNTDOWN_RESTART } else { countdown };
        }
        work.pointer = engine::rom_word(cx.prg, 0, POINTERS + 2 * u16::from(frame & 3));
        work.tile = 0;
        work.attr = ATTR;
        for piece in (0..PIECES).rev() {
            let index = u16::from(piece | body.phase);
            let x_offset = u16::from(engine::rom_byte(cx.prg, 0, X_OFFSETS + index));
            let y_offset = u16::from(engine::rom_byte(cx.prg, 0, Y_OFFSETS + index));
            work.x = body.x.wrapping_add(x_offset << 4);
            work.y = body.y.wrapping_add(y_offset << 4);
            match engine::on_screen(work, cx.world) {
                Visibility::On => {
                    engine::draw(work, DRAW_BANK, cx);
                    work.hitbox = HITBOX;
                    engine::touch(work, DAMAGE, cx);
                }
                Visibility::Near => {}
                Visibility::Far => {
                    engine::despawn(kind, body, 0x80, cx.schedule);
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(13291) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 1100, &[0x5E]);
        let phases = (0..32)
            .map(|step| {
                let count = match step {
                    24 => 13,
                    25..=31 => 16,
                    _ => 12,
                };
                (step * 4, count)
            })
            .collect::<BTreeMap<u8, usize>>();
        assert_eq!(
            coverage,
            Coverage {
                frames: 972,
                occupied: 413,
                drawn: 360,
                phases,
            },
            "{coverage:?}"
        );
    }

    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(13291) else {
            return;
        };
        scene.isolate(&[0x5E]);
        let wram = &scene.machine.bus.wram;
        let slot = (0..24)
            .find(|&slot| wram[0x1CC0 + slot] == 0x5E)
            .expect("a live $5E");
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.player(x, y);
        let mut probe = scene.clone();
        let energy = probe.machine.bus.ram[0x478];
        probe.run(40, &[]);
        assert!(
            probe.machine.bus.ram[0x478] < energy,
            "the oracle takes no contact hit"
        );
        let coverage = assert_actor_phases(scene, 40, &[0x5E]);
        assert_eq!(coverage.occupied, coverage.frames, "{coverage:?}");
    }
}
