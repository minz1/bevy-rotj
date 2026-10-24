//! Type `$18` (bank 8 `$8FD9`): four records of six pointers into the slot
//! arrays, integrated, drawn and touched as they pass on screen.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::{Body, Field};
use crate::actor::stage;
use crate::actor::world::Cx;

/// Bank 8 `$9091`: the metasprite pointer for an area, and for area `$0A`.
const METASPRITES: u16 = 0x9091;
/// Bank 8 `$9095`: a word holding the first record's address (`$9097`).
const RECORDS: u16 = 0x9095;
/// Bytes in one record: six little-endian pointers (x lo/hi, y lo/hi, dx, dy).
const RECORD_BYTES: u16 = 0x0C;
/// The handler walks records `$12 = 3..0` (`$907E`).
const RECORD_COUNT: u8 = 4;
/// `$906D`: added to a record's dy after it moves the record.
const GRAVITY: u8 = 0x0C;
/// `$8FF9`: the contact hit-box index (`$A5`).
const HITBOX: u8 = 3;
/// `$9079`: the contact damage (`$AEE3`'s argument).
const DAMAGE: u8 = 2;

/// Type `$18` keeps no state outside [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8FD9`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        let entry = if stage::at(&cx.prg.levels, cx.world.area).area == 0x0A {
            2
        } else {
            0
        };
        work.pointer = engine::rom_word(cx.prg, 8, METASPRITES + entry);
        work.tile = 0;
        work.attr = body.flags;
        work.hitbox = HITBOX;
        let records = engine::rom_word(cx.prg, 8, RECORDS);
        // `$9009` XORs the record counter with `$F1`, the frame counter whose low
        // bit is the `$F7` bit 7 the port keeps.
        let parity = u8::from(cx.world.frame_odd);
        for (index, counter) in (0..RECORD_COUNT).rev().enumerate() {
            if (counter ^ parity) & 1 == 0 {
                continue;
            }
            let record = records + RECORD_BYTES * u16::try_from(index).expect("record index");
            let pointer = |offset: u16| engine::rom_word(cx.prg, 8, record + offset);
            let field = |offset: u16| {
                Field::from_array(pointer(offset)).expect("$9009 record names a Body array")
            };
            let (x_lo, x_hi) = (field(0), field(2));
            let (y_lo, y_hi) = (field(4), field(6));
            let (dx, dy) = (field(8), field(10));
            let x = u16::from_be_bytes([body.get(x_hi), body.get(x_lo)])
                .wrapping_add_signed(i16::from(body.get(dx).cast_signed()));
            let [hi, lo] = x.to_be_bytes();
            body.set(x_lo, lo);
            body.set(x_hi, hi);
            work.x = x;
            let dy_value = body.get(dy);
            let y = u16::from_be_bytes([body.get(y_hi), body.get(y_lo)])
                .wrapping_add_signed(i16::from(dy_value.cast_signed()));
            let [hi, lo] = y.to_be_bytes();
            body.set(y_lo, lo);
            body.set(y_hi, hi);
            work.y = y;
            body.set(dy, dy_value.wrapping_add(GRAVITY));
            if engine::on_screen(work, cx.world) == Visibility::On {
                engine::draw(work, 1, cx);
                engine::touch(work, DAMAGE, cx);
            }
        }
        // `$9084`: the record's own lifetime countdown; wraps the slot free.
        let [hi, lo] = body.ref_vx.to_be_bytes();
        let hi = hi.wrapping_sub(1);
        body.ref_vx = i16::from_be_bytes([hi, lo]);
        if hi & 0x80 != 0 {
            *kind = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Stage 1-2, area 2: the `$18` spawns at 2392 in slot 15 and lives 31
    /// frames each time, drawn every frame it is on screen, phase `$F0` (§8b).
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(2392) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 300, &[0x18]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 298,
                occupied: 30,
                drawn: 30,
                phases: BTreeMap::from([(0xF0, 30)]),
            }
        );
    }

    /// Stage 5, area `$0C`: the same handler with the other spawn velocity,
    /// phase `$E0` (frames 13419 and 13901, §8b). The `$18` there is spawned by a
    /// parent the isolation frees, so the scene starts with it already in its slot.
    #[test]
    fn matches_the_oracle_in_level_5() {
        let Some(scene) = Scene::tas(13419) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 100, &[0x18]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 100,
                occupied: 33,
                drawn: 33,
                phases: BTreeMap::from([(0xE0, 33)]),
            }
        );
    }

    /// `$AEE3` touches the player: put him on record 0's position and confirm the
    /// oracle loses energy, then that the port reproduces every byte of that.
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(2392) else {
            return;
        };
        scene.isolate(&[0x18]);
        let slot = (0..16)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x18)
            .expect("a live $18");
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1D70 + slot], wram[0x1D50 + slot]]);
        let y = u16::from_be_bytes([wram[0x1DB0 + slot], wram[0x1D90 + slot]]);
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
        let coverage = assert_actor_phases(scene, 40, &[0x18]);
        assert!(coverage.drawn > 0, "{coverage:?}");
    }
}
