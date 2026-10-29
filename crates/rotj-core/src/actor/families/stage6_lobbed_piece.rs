//! Type `$9A`, an area-1C falling actor spawned by type `$96` (bank 12 `$8208`).
//!
//! Its `$7D90` behaviour byte is the phase switch: bit 7 clear is the moving
//! phase (x steps by `$7E90:$7E10`, y by a `$7DF0:$7DD0` offset that rises 3 a
//! frame from `-$18` to `$30`), bit 7 set is the twelve-frame dying animation
//! that frees the slot. It calls only routines already in `docs/ACTORS.md` §9c:
//! `$A000` (`load`), `$A852` (`on_screen`), `$AC06` (`tile_bits`), `$C247`
//! (`draw`) and `$AEE3` (`touch`); `$81C6` maps bank 9 at `$A000` around the
//! draw, so the port passes bank 9 to `engine::draw`.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::{Body, Counter, Metatile};
use crate::actor::world::{Cx, sound};

/// The `$96:$97` metasprite table the moving phase selects (bank 12 `$82E5`).
const MOVING_METASPRITES: u16 = 0x82E5;
/// The `$96:$97` metasprite table the dying animation selects (bank 12 `$82F1`).
const DYING_METASPRITES: u16 = 0x82F1;
/// The bank `$81C6` maps at `$A000` for the draw.
const DRAW_BANK: u8 = 9;
/// The `$7DD0` offset clamps here once its high byte is non-negative.
const OFFSET_LIMIT: u8 = 0x30;
/// The moving subtype animation wraps to zero at this `$7DB0`.
const SUBTYPE_LIMIT: u8 = 0x30;
/// The dying animation frees the slot once `$7E90` reaches this.
const DYING_FRAMES: u8 = 0x0C;
/// At this level y high byte or below the actor is still alive.
const FLOOR_ROW: u8 = 0x0C;
/// The y the dying tail parks the slot at (`$0C20`).
const DEATH_Y: u16 = 0x0C20;
const DYING: u8 = 0x80;

/// Type `$9A` keeps its whole state in [`Body`] (`$7DD0`/`$7DF0` offset,
/// `$7E10`/`$7E90` x velocity then dying counter, `$7DB0` animation, `$7D90` phase).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8208`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if body.behaviour & DYING != 0 {
            work.attr = 3;
            let frame = (body.anim >> 1) & 0x06;
            work.pointer = engine::rom_word(cx.prg, 12, DYING_METASPRITES + u16::from(frame));
            body.anim.tick_up();
            if body.anim >= DYING_FRAMES {
                *kind = 0;
                return;
            }
        } else {
            work.attr = 1;
            work.x = work
                .x
                .wrapping_add(u16::from_be_bytes([body.anim, body.counter]));
            [body.timer, body.phase] = u16::from_be_bytes([body.timer, body.phase])
                .wrapping_add(3)
                .to_be_bytes();
            if body.timer & 0x80 == 0 && body.phase >= OFFSET_LIMIT {
                body.phase = OFFSET_LIMIT;
            }
            work.y = work
                .y
                .wrapping_add(u16::from_be_bytes([body.timer, body.phase]));
            if work.y.row() >= FLOOR_ROW {
                die(body, cx);
                return;
            }
            let frame = (body.subtype >> 2) & 0x0E;
            work.pointer = engine::rom_word(cx.prg, 12, MOVING_METASPRITES + u16::from(frame));
            body.subtype = body.subtype.wrapping_add(1);
            if body.subtype == SUBTYPE_LIMIT {
                body.subtype = 0;
            }
        }
        if engine::on_screen(work, cx.world) != Visibility::On {
            die(body, cx);
            return;
        }
        work.chr_slot = 6;
        engine::tile_bits(work, cx.prg);
        engine::draw(work, DRAW_BANK, cx);
        work.hitbox = 3;
        if engine::touch(work, 2, cx) {
            die(body, cx);
            return;
        }
        engine::store(body, work);
    }
}

/// The `$82C9` tail: sound `$43`, the slot parked at `$0C20` and the dying phase armed.
fn die(body: &mut Body, cx: &mut Cx<'_>) {
    cx.effects.sound[1] = sound::IMPACT;
    body.y = DEATH_Y;
    body.behaviour = DYING;
    body.anim = 0;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Area 1C:00, the frame the type `$96` parent spawns the `$9A` into slot 23
    /// (its first loop head, `$7DD0` `$E8`). `isolate` keeps only the `$9A`, so
    /// the `$96` parent is freed; the `$9A` then runs its whole life: the
    /// `$7DD0` offset ramps `$E8`..`$FD`, `$00`..`$2D`, clamps at `$30`, the y
    /// reaches row `$0C` and the twelve-frame `$7D90` bit-7 dying animation frees
    /// the slot. `$FE`/`$FF` never occur: the offset steps by 3 from `$FD`.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(33956) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 72, &[0x9A]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 72,
                occupied: 65,
                drawn: 63,
                phases: BTreeMap::from([
                    (0x00, 1),
                    (0x03, 1),
                    (0x06, 1),
                    (0x09, 1),
                    (0x0C, 1),
                    (0x0F, 1),
                    (0x12, 1),
                    (0x15, 1),
                    (0x18, 1),
                    (0x1B, 1),
                    (0x1E, 1),
                    (0x21, 1),
                    (0x24, 1),
                    (0x27, 1),
                    (0x2A, 1),
                    (0x2D, 1),
                    (0x30, 41),
                    (0xE8, 1),
                    (0xEB, 1),
                    (0xEE, 1),
                    (0xF1, 1),
                    (0xF4, 1),
                    (0xF7, 1),
                    (0xFA, 1),
                    (0xFD, 1),
                ]),
            }
        );
    }

    /// The player standing on the actor: `$AEE3` (`touch`, damage 2) hurts him
    /// and takes the `$82C9` dying tail on the same frame, so the offset freezes
    /// at `$EB` through the twelve dying frames.
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(33956) else {
            return;
        };
        scene.isolate(&[0x9A]);
        let slot = (0..24)
            .find(|&s| scene.machine.bus.wram[0x1CC0 + s] == 0x9A)
            .expect("a live $9A");
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.script(120, |_| 0);
        scene.player(x, y);
        let mut probe = scene.clone();
        let mut energy = probe.machine.bus.ram[0x478];
        let mut hits = 0;
        for _ in 0..20 {
            probe.run(1, &[]);
            let now = probe.machine.bus.ram[0x478];
            if now < energy {
                hits += 1;
                energy = now;
            }
        }
        assert!(hits > 0, "the oracle takes no contact hit");
        let coverage = assert_actor_phases(scene, 20, &[0x9A]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 20,
                occupied: 13,
                drawn: 12,
                phases: BTreeMap::from([(0xE8, 1), (0xEB, 12)]),
            }
        );
    }

    /// The x and y steps and the `[timer, phase]` gain are the byte pairs they replaced.
    #[test]
    fn the_steps_are_word_maths() {
        for (position, step) in crate::actor::families::word_pairs() {
            let [hi, lo] = position.to_be_bytes();
            let [step_hi, step_lo] = step.to_be_bytes();
            let (lo, carry) = lo.overflowing_add(step_lo);
            let bytes =
                u16::from_be_bytes([hi.wrapping_add(step_hi).wrapping_add(u8::from(carry)), lo]);
            let word = position.wrapping_add(u16::from_be_bytes([step_hi, step_lo]));
            assert_eq!(word, bytes, "{position:#06X} {step:#06X}");
        }
        for pair in 0..=u16::MAX {
            let [timer, phase] = pair.to_be_bytes();
            let (phase_bytes, carry) = phase.overflowing_add(3);
            let timer_bytes = timer.wrapping_add(u8::from(carry));
            let word = u16::from_be_bytes([timer, phase])
                .wrapping_add(3)
                .to_be_bytes();
            assert_eq!(word, [timer_bytes, phase_bytes], "{pair:#06X}");
        }
    }
}
