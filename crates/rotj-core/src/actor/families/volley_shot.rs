//! Type `$4A` (bank 11 `$9A4D`): a stage-4 boss part. It drifts one pixel a
//! frame — `+/-1` px sideways from `$7DD0`'s phase, always 1 px down — draws the
//! bank-9 metasprite its `$9AD4` table picks, and is freed once its y reaches
//! `$0B00`. It calls only routines in `docs/ACTORS.md` §9c: `$A000` (`load`),
//! `$A852` (`on_screen`), `$C247` (`draw`, bank 9 as the `$826F` wrapper maps it),
//! `$AFD6` (`boss_contact`) and `$A934` (`store`).
//!
//! `$7DD0` is the phase (`$9A13`'s spawner sets it from the parent's attr bit 6);
//! `$7DF0` bit 7 is the dying flag the floor clamp sets and `$7E10` is its
//! metasprite index.

use crate::actor::engine::{self, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

const BANK: u8 = 11;
/// Per-phase x offset, low bytes (`$9AD0`) and high bytes (`$9AD2`).
const X_OFFSET: u16 = 0x9AD0;
const X_OFFSET_HIGH: u16 = 0x9AD2;
/// Metasprite pointers (`$9AD4`), indexed by `2 * $7E10`.
const METASPRITES: u16 = 0x9AD4;
/// The draw's bank: `$826F` maps bank 9 into `$A000` around `$C247`.
const DRAW_BANK: u8 = 9;
/// `$AFD6`'s amount (X:A = `$012C`) and mode (Y = `$80`) at the `$9AC5` call.
const CONTACT: u16 = 0x012C;
const CONTACT_MODE: u8 = 0x80;
/// The y high byte that ends the descent, and the timer that frees the slot.
const FLOOR: u8 = 0x0B;
const DEAD: u8 = 0x0A;
const DYING: u8 = 0x80;

/// The part's tables (bank 11), by `phase` (0 or 1).
pub struct Tables {
    /// The x offset added each frame, by `phase` (`$9AD0`, `$9AD2`).
    pub x_offset: [i16; 2],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        Tables {
            x_offset: engine::rom_split_words(prg, BANK, X_OFFSET_HIGH, X_OFFSET)
                .map(u16::cast_signed),
        }
    }
}

/// Type `$4A` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9A4D`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if body.timer & 0x80 != 0 {
            let frame = body.counter & 0xFE;
            body.timer = body.timer.wrapping_add(1);
            if body.timer >= DEAD {
                *kind = 0;
                return;
            }
            paint(body, work, cx, frame);
        } else {
            work.attr = body.flags;
            let offset = cx.prg.families.volley_shot.x_offset[usize::from(body.phase)];
            work.x = work.x.wrapping_add_signed(offset);
            work.y = work.y.wrapping_add(0x0010);
            if work.y.to_be_bytes()[0] >= FLOOR {
                work.y = u16::from_be_bytes([FLOOR, 0]);
                body.timer = DYING;
                body.counter = 0x02;
            }
            paint(body, work, cx, 0);
        }
        engine::store(body, work);
    }
}

/// `$9AA9`: point at the frame's metasprite, project, draw, contact and store.
fn paint(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>, frame: u8) {
    work.pointer = engine::rom_word(cx.prg, BANK, METASPRITES + u16::from(frame));
    engine::on_screen(work, cx.world);
    work.tile = 0;
    engine::draw(work, DRAW_BANK, cx);
    work.hitbox = 1;
    engine::boss_contact(CONTACT, CONTACT_MODE, body, work, cx);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// The whole life of the `$4A` parts on screen at area 14:00, TAS 26500: both
    /// `$7DD0` phases (the `$9A13` spawner takes bit 6 of the parent's attr), the
    /// drift, the draw, then the y `$0B00` clamp and the free.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(26500) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 150, &[0x4A]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 150,
                occupied: 73,
                drawn: 72,
                phases: BTreeMap::from([(0, 138), (1, 96)]),
            }
        );
    }

    /// `$AFD6` with mode `$80`: standing inside a part drains the player's energy.
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(26500) else {
            return;
        };
        scene.isolate(&[0x4A]);
        scene.run(1, &[]);
        let player = (
            u16::from_be_bytes([scene.machine.bus.ram[0x460], scene.machine.bus.ram[0x461]]),
            u16::from_be_bytes([scene.machine.bus.ram[0x462], scene.machine.bus.ram[0x463]]),
        );
        let slot = (16..24)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x4A)
            .expect("a $4A part");
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
        let coverage = assert_actor_phases(scene, 90, &[0x4A]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }
}
