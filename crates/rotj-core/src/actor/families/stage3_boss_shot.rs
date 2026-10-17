//! Type `$90` (bank 0 `$9DAD`): one shot of the `$8E` boss's five-shot burst
//! (`$9D13`), living in slots 16-23. It moves by a fixed step per direction
//! (`$7DD0` 0-4 facing right, 5-9 left), dies on a solid metatile (`$C93B`,
//! flags `$84`) or off screen, and the two lowest directions burst into a type
//! `$92` on the wall (`$AC16` variant 7). On screen it draws through `$84CA`
//! (bank 9) and runs the `$AFD6` boss contact in mode `$80`.

use crate::actor::collision::{self, Probe};
use crate::actor::engine::{self, Visibility, Work};
use crate::actor::families::Brain;
use crate::actor::slots::Body;
use crate::actor::world::Cx;

const HOME_BANK: u8 = 0;
const DRAW_BANK: u8 = 9;
/// The step per direction: x low (`$9E59`), x high (`$9E63`), y low (`$9E6D`), y high (`$9E77`).
const STEP_X_LOW: u16 = 0x9E59;
const STEP_X_HIGH: u16 = 0x9E63;
const STEP_Y_LOW: u16 = 0x9E6D;
const STEP_Y_HIGH: u16 = 0x9E77;
/// One metasprite per direction mod 5 (`$9E81`).
const METASPRITES: u16 = 0x9E81;
const DIRECTIONS: u8 = 5;
const SOLID_FLAGS: u8 = 0x84;
const WALL_SOUND: u8 = 0x23;
/// Directions below this burst on a wall (`$9E35`).
const BURSTING: u8 = 2;
const BURST_VARIANT: u8 = 7;
const BURST_KIND: u8 = 0x92;
const BURST_CONTACT: u8 = 0x20;
const HITBOX: u8 = 1;
/// `$AFD6`'s amount (X:A) and mode (Y).
const CONTACT: u16 = 0x03E0;
const CONTACT_MODE: u8 = 0x80;

/// Type `$90` keeps its direction (`phase`) in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9DAD`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        work.attr = body.flags;
        let direction = u16::from(body.phase);
        let byte = |table: u16| engine::rom_byte(cx.prg, HOME_BANK, table + direction);
        let [x_high, x_low] = work.x.to_be_bytes();
        let (x_low, carry) = x_low.overflowing_add(byte(STEP_X_LOW));
        let x_high = x_high
            .wrapping_add(byte(STEP_X_HIGH))
            .wrapping_add(u8::from(carry));
        let [y_high, y_low] = work.y.to_be_bytes();
        let (y_low, carry) = y_low.overflowing_add(byte(STEP_Y_LOW));
        let y_high = y_high
            .wrapping_add(byte(STEP_Y_HIGH))
            .wrapping_add(u8::from(carry));
        work.x = u16::from_be_bytes([x_high, x_low]);
        work.y = u16::from_be_bytes([y_high, y_low]);
        let mut probe = Probe {
            x: work.x,
            y: work.y,
            ..Probe::default()
        };
        let frame = body.phase % DIRECTIONS;
        if collision::solid(&mut probe, SOLID_FLAGS, cx) & 0x80 != 0 {
            cx.effects.sound[1] = WALL_SOUND;
            if frame < BURSTING {
                burst(work, cx);
            }
            *kind = 0;
            return;
        }
        work.pointer = engine::rom_word(cx.prg, HOME_BANK, METASPRITES + 2 * u16::from(frame));
        if engine::on_screen(work, cx.world) != Visibility::On {
            *kind = 0;
            return;
        }
        engine::tile_bits(work, cx.prg);
        engine::draw(work, DRAW_BANK, cx);
        work.hitbox = HITBOX;
        engine::boss_contact(CONTACT, CONTACT_MODE, body, work, cx);
        engine::store(body, work);
    }
}

/// `$9E40`: the `$AC16` parts, retyped as a `$92` burst.
fn burst(work: &Work, cx: &mut Cx<'_>) {
    // shortcut: no free slot leaves $AC16's stale Y written with $92, which no scene reaches.
    let Some(index) = engine::spawn_parts(BURST_VARIANT, work, cx) else {
        return;
    };
    let slot = &mut cx.slots.main[index];
    slot.kind = BURST_KIND;
    slot.body.contact = BURST_CONTACT;
    slot.body.sprite = 0;
    slot.brain = Brain::new(BURST_KIND);
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// TAS 14672 (area 0E:00) with the pad released from there, one frame before
    /// the `$8E`'s first `$9D13` burst (15083). The TAS kills the boss at 15050,
    /// its phase-3 count `$7E90` at `$3B`; unhit, it reaches `$50` with no poke.
    pub(crate) fn before_the_burst() -> Option<Scene> {
        static SCENE: Mutex<Option<Scene>> = Mutex::new(None);
        let mut guard = SCENE.lock().expect("the scene lock");
        if guard.is_none() {
            *guard = Scene::tas(14672).map(|mut scene| {
                scene.script(1000, |_| 0);
                scene.run(15082 - 14672, &[]);
                scene
            });
        }
        guard.clone()
    }

    /// Runs the burst's frame, then diffs its shots alone for `frames` loops; also
    /// whether the oracle spawned a `$92` and whether the player's `$0479` rose.
    fn shots(mut scene: Scene, frames: u64) -> (Coverage, bool, bool) {
        scene.run(1, &[]);
        let mut probe = scene.clone();
        probe.isolate(&[0x90, 0x92]);
        let (mut burst, mut hurt) = (false, false);
        for _ in 0..frames {
            let before = probe.machine.bus.ram[0x479];
            probe.run(1, &[]);
            burst |= probe.machine.bus.wram[0x1CC0..0x1CD0].contains(&0x92);
            hurt |= probe.machine.bus.ram[0x479] > before;
        }
        (
            assert_actor_phases(scene, frames, &[0x90, 0x92]),
            burst,
            hurt,
        )
    }

    /// The first burst faces left: all five directions 5-9 fly, the player takes
    /// the `$AFD6` contact, and the lowest ones burst into a `$92` on the wall.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = before_the_burst() else {
            return;
        };
        let (coverage, burst, hurt) = shots(scene, 120);
        assert!(burst && hurt, "burst {burst}, hurt {hurt}");
        for direction in 5..10 {
            assert!(
                coverage.phases.get(&direction).is_some_and(|&n| n > 0),
                "{coverage:?}"
            );
        }
    }

    /// The same burst facing right (`$7C00` bit 6 cleared on the `$8E` the frame
    /// before): directions 0-4.
    #[test]
    fn the_right_facing_burst_matches_the_oracle() {
        let Some(mut scene) = before_the_burst() else {
            return;
        };
        let wram = &mut scene.machine.bus.wram;
        let boss = (0..16)
            .find(|&s| wram[0x1CC0 + s] == 0x8E)
            .expect("the $8E");
        wram[0x1C00 + boss] &= !0x40;
        let (coverage, burst, _) = shots(scene, 120);
        assert!(burst, "no $92");
        for direction in 0..5 {
            assert!(
                coverage.phases.get(&direction).is_some_and(|&n| n > 0),
                "{coverage:?}"
            );
        }
    }
}
