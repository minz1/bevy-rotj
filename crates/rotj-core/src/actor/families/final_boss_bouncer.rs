//! Type `$C0`, a stage 7 boss part (bank 12 `$96B1`).
//!
//! It is a bouncing body: `$7DD0:$7DB0` is its x velocity and `$7E10:$7DF0`
//! its y velocity, added to the position each frame. It bounces off the level's
//! left or right wall (whichever its x velocity points at) and off floor row
//! `$0B`, flipping the matching velocity, and is freed when its y leaves the
//! level (`$92 & $F0`) or the projection leaves the screen. While alive it draws
//! one of six metasprites (bank 9, picked by `$7E90 << 2 | $F1 & 2`) and drains
//! the player on touch (`engine::boss_contact`, mode `$80`). Freeing it also
//! clears a bit of the behaviour of the slot named in `$7C30` (its parent).

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The bank-12 table of six bank-9 metasprite pointers (`$97AA`), indexed by
/// the always-even `$7E90 << 2 | $F1 & 2`.
const METASPRITES: u16 = 0x97AA;
/// The draws map bank 9 at `$A000` (`$81C6`).
const DRAW_BANK: u8 = 9;
/// The y row the floor bounce clamps to (`$96EA`).
const FLOOR_ROW: u8 = 0x0B;
/// The x the leftward bounce clamps to (`$9712`).
const LEFT_WALL: u16 = 0x0200;
/// The x the rightward bounce clamps to (`$971C`).
const RIGHT_WALL: u16 = 0x0E00;
/// The amount `$AFD6` drains from the player on touch (`X:A` = `$0320`).
const CONTACT: u16 = 0x0320;
/// The `$AFD6` mode (`Y` = `$80`).
const MODE: u8 = 0x80;

/// Type `$C0` keeps its whole state in [`Body`]: `$7DD0:$7DB0` its x velocity,
/// `$7E10:$7DF0` its y velocity, `$7E90`/`$7CE0` the animation and `$7C90` the
/// CHR shadow; it has none of its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$96B1`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        work.attr = 0;
        work.x = work
            .x
            .wrapping_add(u16::from_be_bytes([body.phase, body.subtype]));
        work.y = work
            .y
            .wrapping_add(u16::from_be_bytes([body.counter, body.timer]));
        if work.y.to_be_bytes()[0] & 0xF0 != 0 {
            free(kind, body, cx);
            return;
        }
        if body.counter & 0x80 == 0 && work.y.to_be_bytes()[0] >= FLOOR_ROW {
            work.y = u16::from(FLOOR_ROW) << 8;
            negate(&mut body.counter, &mut body.timer);
        }
        if body.phase & 0x80 != 0 {
            if work.x.to_be_bytes()[0] < 2 {
                work.x = LEFT_WALL;
                negate(&mut body.phase, &mut body.subtype);
            }
        } else if work.x.to_be_bytes()[0] >= 0x0E {
            work.x = RIGHT_WALL;
            negate(&mut body.phase, &mut body.subtype);
        }
        let index = (u16::from(body.anim) << 2) | u16::from(cx.world.frame & 2);
        work.pointer = engine::rom_word(cx.prg, 12, METASPRITES + index);
        if engine::on_screen(work, cx.world) != Visibility::On {
            free(kind, body, cx);
            return;
        }
        work.tile = 0;
        engine::draw(work, DRAW_BANK, cx);
        work.hitbox = 1;
        engine::boss_contact(CONTACT, MODE, body, work, cx);
        let (chr_slot, carry) = body.chr_slot.overflowing_add(4);
        body.chr_slot = chr_slot;
        let (cell, _) = body.cell.overflowing_add(u8::from(carry));
        body.cell = cell;
        body.anim = cell.min(2);
        engine::store(body, work);
    }
}

/// Frees the slot and clears the parent's behaviour bit (`$9788`): bit 7 when
/// this actor moves right, bit 6 when it moves left.
fn free(kind: &mut u8, body: &Body, cx: &mut Cx<'_>) {
    let addr = 0x7D90 + u16::from(body.sprite);
    let value = cx.byte(addr);
    cx.set_byte(addr, value & if body.behaviour == 0 { 0x7F } else { 0xBF });
    *kind = 0;
}

/// Two's-complements the 16-bit [`Body`] velocity split as `(hi, lo)`.
fn negate(hi: &mut u8, lo: &mut u8) {
    let [h, l] = u16::from_be_bytes([*hi, *lo]).wrapping_neg().to_be_bytes();
    *hi = h;
    *lo = l;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Area 22:00 (the stage 7 boss room), where $C0's two parts live in slots
    /// 22 and 23 from frame 40787. They bounce left/right, so both `$7DD0`
    /// x-velocity phases (`$00` and `$FF`) run; `drawn` counts the loops that
    /// changed the OAM page.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(40787) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 110, &[0xC0]);
        let expected = Coverage {
            frames: 110,
            occupied: 110,
            drawn: 110,
            phases: BTreeMap::from([(0x00, 109), (0xFF, 111)]),
        };
        assert_eq!(coverage, expected);
    }

    /// `$AFD6` with mode `$80`: a part pinned onto the player drains his life.
    /// The oracle is probed first so the poke is known to land the contact.
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(40787) else {
            return;
        };
        scene.isolate(&[0xC0]);
        scene.run(1, &[]);
        let player = (
            u16::from_be_bytes([scene.machine.bus.ram[0x460], scene.machine.bus.ram[0x461]]),
            u16::from_be_bytes([scene.machine.bus.ram[0x462], scene.machine.bus.ram[0x463]]),
        );
        let slot = (16..24)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0xC0)
            .expect("a $C0 part");
        let at = u16::try_from(slot).expect("slot < 24");
        for (addr, value) in [
            (0x7B00, player.0.to_be_bytes()[0]),
            (0x7B20, player.0.to_be_bytes()[1]),
            (0x7B40, player.1.to_be_bytes()[0]),
            (0x7B60, player.1.to_be_bytes()[1]),
        ] {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, addr + at, value));
        }
        let mut probe = scene.clone();
        let mut hurt = 0u8;
        for _ in 0..90 {
            probe.run(1, &[]);
            hurt = hurt.max(probe.machine.bus.ram[0x479]);
        }
        assert!(hurt > 0, "the oracle never took boss contact");
        let coverage = assert_actor_phases(scene, 90, &[0xC0]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }
}
