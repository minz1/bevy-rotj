//! Type `$92` (bank 0 `$9E8B`): the four-part burst a `$90` shot leaves on a
//! wall, `$AC16` variant 7 retyped by `$9E47`. Like type `$08` it moves two of
//! its four parts a frame through the array pointers at `$9F56`, but with one
//! metasprite (`$9F52`, bank 9 through `$84CA`), a gravity clamp when `$7C30`
//! bit 7 is set, and the `$AFD6` boss contact (mode `$80`) per drawn part. It
//! frees itself when the `$7ED0` lifetime runs out.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::{Body, Field};
use crate::actor::world::Cx;

const HOME_BANK: u8 = 0;
const DRAW_BANK: u8 = 9;
/// The metasprite pointer (`$9F52`).
const SPRITE: u16 = 0x9F52;
/// The pointer to the four parts' six array pointers each (`$9F54`): x low/high,
/// y low/high, x velocity, y velocity.
const PARTS: u16 = 0x9F54;
/// The lifetime counter, the high byte of `ref_vx` (`$7ED0`).
const LIFETIME: Field = Field::RefVxHigh;
const ATTR: u8 = 0x02;
const GRAVITY: u8 = 0x0C;
const HITBOX: u8 = 1;
/// `$AFD6`'s amount (X:A) and mode (Y).
const CONTACT: u16 = 0x0100;
const CONTACT_MODE: u8 = 0x80;

/// Type `$92` has no handler phase: its `$7DD0` byte is part 0's x velocity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9E8B`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        work.pointer = engine::rom_word(cx.prg, HOME_BANK, SPRITE);
        work.tile = 0;
        work.attr = ATTR;
        let parts = engine::rom_word(cx.prg, HOME_BANK, PARTS);
        for part in (0..4u8).rev() {
            if (part ^ cx.world.frame) & 1 == 0 {
                continue;
            }
            let record = parts + 12 * u16::from(3 - part);
            let at = |i: u16| engine::rom_word(cx.prg, HOME_BANK, record + 2 * i);
            let (x_lo, x_hi, y_lo, y_hi, vx, vy) = (at(0), at(1), at(2), at(3), at(4), at(5));
            work.x = engine::move_by_velocity(body, x_lo, x_hi, vx);
            work.y = engine::move_by_velocity(body, y_lo, y_hi, vy);
            let vy = Field::from_array(vy).expect("$9F54 table names a Body array");
            let fallen = body.get(vy).wrapping_add(GRAVITY);
            let clamped = body.sprite & 0x80 != 0 && fallen & 0x80 != 0;
            body.set(vy, if clamped { 0x7F } else { fallen });
            if engine::on_screen(work, cx.world) == Visibility::On {
                engine::draw(work, DRAW_BANK, cx);
                work.hitbox = HITBOX;
                engine::boss_contact(CONTACT, CONTACT_MODE, body, work, cx);
            }
        }
        let lifetime = body.get(LIFETIME).wrapping_sub(1);
        body.set(LIFETIME, lifetime);
        if lifetime & 0x80 != 0 {
            *kind = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::actor::families::stage3_boss_shot;
    use crate::actor::oracle::{Scene, assert_actor_phases};

    /// The `$8E`'s first burst with the pad released (`stage3_boss_shot`), run to the frame its
    /// first `$92` lands (15093), and that `$92`'s slot.
    fn first_burst() -> Option<(Scene, usize)> {
        let mut scene = stage3_boss_shot::tests::before_the_burst()?;
        for _ in 0..40 {
            scene.run(1, &[]);
            let wram = &scene.machine.bus.wram;
            if let Some(slot) = (0..16).find(|&s| wram[0x1CC0 + s] == 0x92) {
                return Some((scene, slot));
            }
        }
        panic!("no $92 within 40 frames of the burst");
    }

    /// Its life alone from its second loop: `$7ED0` counts `$20` down past 0, so
    /// 32 more loops, drawn every one.
    #[test]
    fn matches_the_oracle() {
        let Some((scene, _)) = first_burst() else {
            return;
        };
        let coverage = assert_actor_phases(scene, 60, &[0x92]);
        assert_eq!(
            (coverage.occupied, coverage.drawn),
            (32, 32),
            "{coverage:?}"
        );
    }

    /// `$7C30` bit 7 (`$9F15`): a fall speed that turns negative is clamped to
    /// `$7F`. No spawner sets it (`$9E51` stores 0), so it is held at `$D450`.
    #[test]
    fn the_clamped_fall_matches_the_oracle() {
        let Some((mut scene, slot)) = first_burst() else {
            return;
        };
        let at = u16::try_from(slot).expect("slot < 16");
        scene
            .machine
            .bus
            .trace
            .pokes
            .push((0xD450, 0x7C30 + at, 0x80));
        let coverage = assert_actor_phases(scene, 60, &[0x92]);
        assert_eq!(coverage.occupied, 32, "{coverage:?}");
    }

    /// The `$AFD6` contact: part 3 (x `$7B20:$7B00`, y `$7B60:$7B40`) is held
    /// on the standing player, so the oracle takes the hurt (`$0479`).
    #[test]
    fn hurts_the_player_up_close() {
        let Some((mut scene, slot)) = first_burst() else {
            return;
        };
        let at = u16::try_from(slot).expect("slot < 16");
        let ram = &scene.machine.bus.ram;
        let (x_high, x_low, y_high, y_low) = (ram[0x460], ram[0x461], ram[0x462], ram[0x463]);
        for (array, value) in [
            (0x7B00, x_low),
            (0x7B20, x_high),
            (0x7B40, y_low),
            (0x7B60, y_high),
        ] {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, array + at, value));
        }
        let mut probe = scene.clone();
        probe.isolate(&[0x92]);
        let mut hurt = false;
        for _ in 0..40 {
            let before = probe.machine.bus.ram[0x479];
            probe.run(1, &[]);
            hurt |= probe.machine.bus.ram[0x479] > before;
        }
        assert!(hurt, "the oracle never took the contact");
        let coverage = assert_actor_phases(scene, 40, &[0x92]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }
}
