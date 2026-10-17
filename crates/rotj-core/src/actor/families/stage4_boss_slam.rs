//! Type `$46` (bank 11 `$97FF`): the stage-4 boss's charged attack. The `$42`
//! spawns it into slot 23 when its counter reaches `$C0` (`$957E`), with the
//! parent's slot in `$7E90`. It falls under its own gravity (`$7DB0:$7D90`) to
//! row `$0B` (`$7DF0` clear), plays a three-frame landing (`$7DF0` = `$80`),
//! then flashes the screen white for 12 frames (`$7DF0` = `$C0`, the `$04A0`
//! palette through `$C33C`). On the last frame it restores the palette
//! (`$C34B`), hurts the player and drains `$03E8` from `$B2:$B3` while the boss
//! has hit points (`$B17F`), frees itself and sets the boss back to waiting.
//! It calls `$A000` (`load`), `$A852` (`on_screen`), `$C247` (`draw`, bank 9
//! through `$826F`) and `$A934` (`store`).

use crate::actor::engine::{self, Work};
use crate::actor::slots::{Body, Field};
use crate::actor::world::Cx;

const HOME_BANK: u8 = 11;
const DRAW_BANK: u8 = 9;
/// The falling frames (`$98F0`), by `($7DD0 >> 2) & $FE`.
const FALLING_SPRITES: u16 = 0x98F0;
/// The landing frames (`$98FC`), by `$7DD0 & $FE`.
const LANDING_SPRITES: u16 = 0x98FC;
const GRAVITY: u8 = 3;
const FLOOR_ROW: u8 = 0x0B;
const LANDED: u8 = 0x80;
const FLASHING: u8 = 0xC0;
const LANDING_SOUND: u8 = 0x35;
const FLASH_SOUND: u8 = 0x23;
const LANDING_FRAMES: u8 = 6;
const FLASH_FRAMES: u8 = 0x0C;
/// The flash palette: every colour `$30`, sprite palette 0's colours 1-3 `$0F` (`$983D`).
const WHITE: u8 = 0x30;
const BLACK: u8 = 0x0F;
/// What the attack drains from `$B2:$B3` (`$981D`).
const DRAIN: u16 = 0x03E8;

/// The frame counter (`$7DD0`) at the start of the landing frames.
const LANDING_START: u8 = 0;
/// The frame counter at the start of the flash.
const FLASH_START: u8 = 0;
/// The frame counter the fall animation rewinds to once it passes its last frame.
const FALL_LOOP: u8 = 0x18;
/// Type `$46` keeps its parent's slot in `anim`, its fall speed in the pair `[subtype,
/// behaviour]` and its frame counter in `phase`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$97FF`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if body.timer & LANDED == 0 {
            if !fall(body, work, cx) {
                return finish(body, work, cx);
            }
        } else if body.timer & 0x40 != 0 {
            return flash(kind, body, cx);
        }
        land(body, work, cx);
        finish(body, work, cx);
    }
}

/// `$9882`: accelerate down; true once it reaches the floor row.
fn fall(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) -> bool {
    let (low, carry) = body.behaviour.overflowing_add(GRAVITY);
    body.behaviour = low;
    if carry {
        body.subtype = body.subtype.wrapping_add(1);
    }
    let [y_high, y_low] = work.y.to_be_bytes();
    let (y_low, carry) = y_low.overflowing_add(body.behaviour);
    let y_high = y_high
        .wrapping_add(body.subtype)
        .wrapping_add(u8::from(carry));
    work.y = u16::from_be_bytes([y_high, y_low]);
    if y_high >= FLOOR_ROW {
        cx.effects.sound[1] = LANDING_SOUND;
        work.y = u16::from_be_bytes([FLOOR_ROW, 0]);
        body.phase = LANDING_START;
        body.timer = LANDED;
        return true;
    }
    let mut index = (body.phase >> 2) & 0xFE;
    if index >= 6 {
        body.phase = FALL_LOOP;
        index = 4;
    }
    work.pointer = engine::rom_word(cx.prg, HOME_BANK, FALLING_SPRITES + u16::from(index));
    body.phase = body.phase.wrapping_add(1);
    false
}

/// `$9857`: the landing frames, then the flash.
fn land(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    work.pointer = engine::rom_word(
        cx.prg,
        HOME_BANK,
        LANDING_SPRITES + u16::from(body.phase & 0xFE),
    );
    body.phase = body.phase.wrapping_add(1);
    if body.phase >= LANDING_FRAMES {
        cx.effects.sound[1] = FLASH_SOUND;
        body.timer = FLASHING;
        body.phase = FLASH_START;
    }
}

/// `$980A`: the white screen, then the hurt and the boss's reset.
fn flash(kind: &mut u8, body: &mut Body, cx: &mut Cx<'_>) {
    body.phase = body.phase.wrapping_add(1);
    if body.phase < FLASH_FRAMES {
        let mut white = [WHITE; 32];
        white[0x11..0x14].fill(BLACK);
        cx.effects.vram.push(0x3F00, 0, &white);
        return;
    }
    let palette = cx.effects.palette;
    cx.effects.vram.push(0x3F00, 0, &palette);
    if cx.effects.boss_hp != 0 {
        let pool = u16::from_le_bytes(cx.effects.boss_pool).wrapping_sub(DRAIN);
        cx.effects.boss_pool = pool.to_le_bytes();
        engine::hurt_player(cx.effects);
    }
    *kind = 0;
    let parent = usize::from(body.anim);
    cx.set(
        parent,
        Field::Phase,
        super::stage4_boss::Phase::Waiting.byte(),
    );
}

/// `$98DC`: draw and store.
fn finish(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    engine::on_screen(work, cx.world);
    work.attr = 0;
    work.tile = 0;
    engine::draw(work, DRAW_BANK, cx);
    engine::store(body, work);
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::DRAIN;
    use crate::actor::oracle::{Scene, assert_actor_phases};

    const CHILD_SLOT: usize = 23;

    /// TAS 26808 (area 14:00) with the pad released from there: unhit, the `$42`
    /// charges `$7E10` to `$C0` and spawns the `$46` at 27208, with no poke. The
    /// scene stops the frame before.
    fn before_the_charge() -> Option<Scene> {
        static SCENE: Mutex<Option<Scene>> = Mutex::new(None);
        let mut guard = SCENE.lock().expect("the scene lock");
        if guard.is_none() {
            *guard = Scene::tas(26808).map(|mut scene| {
                scene.script(600, |_| 0);
                scene.run(27207 - 26808, &[]);
                scene
            });
        }
        guard.clone()
    }

    const KINDS: [u8; 3] = [0x42, 0x44, 0x46];

    /// What the oracle's `$46` does over `frames`: the `$7DF0` states it passes
    /// through, and on the loop it frees itself whether it hurt the player (`$0479`
    /// set to `$20` and `$B2:$B3` down by `$03E8`) and the parent's `$7DD0` is 0.
    fn life(scene: &Scene, frames: u64) -> (Vec<u8>, Option<(bool, bool)>) {
        let pool = |ram: &[u8; 2048]| u16::from_le_bytes([ram[0xB2], ram[0xB3]]);
        let mut probe = scene.clone();
        probe.isolate(&KINDS);
        let (mut states, mut end) = (Vec::new(), None);
        for _ in 0..frames {
            let alive = probe.machine.bus.wram[0x1CC0 + CHILD_SLOT] == 0x46;
            let before = pool(&probe.machine.bus.ram);
            probe.run(1, &[]);
            let (ram, wram) = (&probe.machine.bus.ram, &probe.machine.bus.wram);
            if wram[0x1CC0 + CHILD_SLOT] == 0x46 {
                let state = wram[0x1DF0 + CHILD_SLOT];
                if !states.contains(&state) {
                    states.push(state);
                }
            } else if alive && end.is_none() {
                let parent = usize::from(wram[0x1E90 + CHILD_SLOT]);
                let hurt = ram[0x479] == 0x20 && before.wrapping_sub(pool(ram)) == DRAIN;
                end = Some((hurt, wram[0x1DD0 + parent] == 0));
            }
        }
        (states, end)
    }

    /// Its whole life: the fall, the landing, the 12-frame flash, then the
    /// hurt, the free and the parent back in phase 0; drawn every loop.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = before_the_charge() else {
            return;
        };
        assert_eq!(life(&scene, 120), (vec![0, 0x80, 0xC0], Some((true, true))));
        let coverage = assert_actor_phases(scene, 120, &KINDS);
        assert_eq!(
            (coverage.frames, coverage.occupied, coverage.drawn),
            (120, 120, 120)
        );
    }

    /// The `$9817` branch with the boss's hit points (`$A9:$AA`) at 0, as when it
    /// dies mid-attack: no hurt and no drain. Both are held at 0 each loop.
    #[test]
    fn spares_the_player_once_the_boss_is_beaten() {
        let Some(mut scene) = before_the_charge() else {
            return;
        };
        for addr in [0xA9, 0xAA] {
            scene.machine.bus.trace.pokes.push((0xD450, addr, 0));
        }
        assert_eq!(
            life(&scene, 120),
            (vec![0, 0x80, 0xC0], Some((false, true)))
        );
        let coverage = assert_actor_phases(scene, 120, &KINDS);
        assert_eq!(coverage.occupied, 120, "{coverage:?}");
    }
}
