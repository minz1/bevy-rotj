//! Type `$0C` (bank 8 `$8771`): a stomping hazard that walks a step counter
//! through three phases (the dispatch table at bank 8 `$8A68`).

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::slots::hit;
use crate::actor::world::Cx;

/// The overlay metasprites (bank 8 `$8A64`, by Y = 0 or 2).
const OVERLAYS: u16 = 0x8A64;
/// The impact metasprites (bank 8 `$8A76`, by `$F1 & 6`).
const IMPACTS: u16 = 0x8A76;
/// The stomping's frame-pointer table (bank 8 `$8A89`, by `$7E90`).
const FRAMES: u16 = 0x8A89;
/// The stomping's touch-enable table (bank 8 `$8AB8`, by `$7E90`).
const TOUCHES: u16 = 0x8AB8;
/// The hit-flash metasprite (bank 8 `$8ACD`).
const FLASH: u16 = 0x8ACD;
/// The pointer to the 3x3 metatiles a death leaves (bank 8 `$8AE9`).
const RUBBLE: u16 = 0x8AE9;
/// The pointer to the 6x6 tiles a death queues (bank 8 `$8AF4`).
const RUBBLE_TILES: u16 = 0x8AF4;

phases! {
    /// The stomper's phase (`$7DD0`), which picks its handler (`$8A68`).
    Phase {
        /// Hovering until the player comes close.
        Hovering = 0,
        /// The landing impact, again after the fifth stomp.
        Landing = 1,
        /// Stepping across the floor, stomping.
        Stomping = 2,
    }
}

/// The stomper's tables (bank 8).
pub struct Tables {
    /// The landing's counter, by the stomp number `timer` (0-7) (`$8A6E`).
    pub landing_counters: [u8; 8],
    /// The stomping's `anim` (0-2), by `timer` (`$8A7E`).
    pub anims: [u8; 8],
    /// How far left of its x the stomping is drawn, by `anim` (`$8A86`).
    pub draw_offsets: [u8; 3],
    /// The stomping's step count, by `anim` (`$8AAD`).
    pub step_counts: [u8; 3],
    /// The step the stomp lands on, by `anim` (`$8AB1`).
    pub stomp_steps: [u8; 3],
    /// How far a stomp moves it left, by `anim`; entries overlap a byte (`$8AB4`).
    pub stomp_shifts: [u16; 3],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        let shifts: [u8; 4] = engine::rom_bytes(prg, 8, 0x8AB4);
        Tables {
            landing_counters: engine::rom_bytes(prg, 8, 0x8A6E),
            anims: engine::rom_bytes(prg, 8, 0x8A7E),
            draw_offsets: engine::rom_bytes(prg, 8, 0x8A86),
            step_counts: engine::rom_bytes(prg, 8, 0x8AAD),
            stomp_steps: engine::rom_bytes(prg, 8, 0x8AB1),
            stomp_shifts: std::array::from_fn(|i| u16::from_le_bytes([shifts[i], shifts[i + 1]])),
        }
    }
}

/// Type `$0C` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8771`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        let visibility = engine::on_screen(work, cx.world);
        if visibility == Visibility::On {
            if body.hit & hit::DYING != 0 {
                if body.dying_timer == 0 {
                    if cx.effects.vram.top < 0x60 {
                        return;
                    }
                    explode(body, work, cx);
                }
                engine::dying(kind, body, work, 0, cx);
                return;
            }
            if body.hit & hit::FLASHING != 0 {
                work.pointer = engine::rom_word(cx.prg, 8, FLASH);
                work.chr_slot = 6;
                work.attr = 0;
                work.tile = 0;
                engine::draw(work, 8, cx);
                body.hit = 0;
            }
        }
        match Phase::from_byte(body.phase) {
            Some(Phase::Hovering) => hovering(kind, body, work, visibility, cx),
            Some(Phase::Landing) => landing(kind, body, work, visibility, cx),
            Some(Phase::Stomping) => stomping(kind, body, work, visibility, cx),
            None => panic!("type $0C phase {} is past the $8A68 table", body.phase),
        }
    }
}

/// The death (`$89B7`): 76 px lower, two part groups, the 3x3 metatiles at the
/// actor's map cell cleared and the 6x6 tile block queued.
fn explode(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    body.y = body.y.wrapping_add(0x04C0);
    work.y = body.y;
    _ = engine::spawn_parts(0, work, cx);
    _ = engine::spawn_parts(1, work, cx);
    let source = engine::rom_word(cx.prg, 8, RUBBLE);
    let metatiles: Vec<u8> = (0..9)
        .map(|i| engine::rom_byte(cx.prg, 8, source + i))
        .collect();
    let cell = u16::from_be_bytes([body.subtype, body.behaviour]);
    engine::copy_metatiles(cell, 3, &metatiles, &mut cx.effects.level);
    let source = engine::rom_word(cx.prg, 8, RUBBLE_TILES);
    let tiles: Vec<u8> = (0..36)
        .map(|i| engine::rom_byte(cx.prg, 8, source + i))
        .collect();
    let block = engine::TileBlock {
        column: work.x.to_be_bytes()[0].wrapping_sub(1),
        row: 7,
        width: 6,
        height: 6,
        tiles: &tiles,
        fields: None,
    };
    engine::queue_block(&block, cx);
}

/// Hovering (`$87BA`), then landing when the player comes close.
fn hovering(
    kind: &mut u8,
    body: &mut Body,
    work: &mut Work,
    visibility: Visibility,
    cx: &mut Cx<'_>,
) {
    if visibility == Visibility::On {
        body.contact = 0x80;
        work.hitbox = 0;
        engine::contact(kind, body, work, 4, cx);
    }
    let mut wake = true;
    if body.anim & 0x80 != 0 {
        body.counter = body.counter.wrapping_sub(1);
        if body.counter != 0 {
            wake = false;
        } else {
            body.anim = 0;
        }
    }
    if wake {
        let dx = engine::player_dx(work, &cx.effects.player);
        let [hi, lo] = dx.unsigned_abs().to_be_bytes();
        if hi & 0xF0 == 0 && hi & 0x0F < 5 && (hi & 0x0F < 4 || lo < 0x80) {
            body.phase = Phase::Landing.byte();
            body.counter = cx.prg.families.stomper.landing_counters[usize::from(body.timer)];
        }
    }
    overlay(work, 0, cx);
}

/// Landing (`$880F`): the impact, then the stomping when its counter runs out.
fn landing(
    kind: &mut u8,
    body: &mut Body,
    work: &mut Work,
    visibility: Visibility,
    cx: &mut Cx<'_>,
) {
    body.contact = 0;
    if visibility == Visibility::On {
        work.hitbox = 0;
        engine::contact(kind, body, work, 2, cx);
        if body.hit & hit::DYING != 0 {
            return;
        }
        let (screen_y, _) = work.screen_y.overflowing_sub(0x1A);
        let (screen_x, borrow) = work.screen_x.overflowing_sub(0x10);
        work.screen_y = screen_y;
        work.screen_x = screen_x;
        if !borrow {
            engine::touch(work, 4, cx);
            let [seed, noise] = cx.world.rng;
            work.screen_x = work.screen_x.wrapping_add(seed & 3);
            work.screen_y = work.screen_y.wrapping_add(noise & 3);
            work.pointer = engine::rom_word(cx.prg, 8, IMPACTS + u16::from(cx.world.frame & 6));
            work.chr_slot = 7;
            work.tile = 0xC0;
            work.attr = 3;
            engine::draw(work, 1, cx);
            overlay(work, 2, cx);
        }
    }
    body.counter = body.counter.wrapping_sub(1);
    if body.counter == 0 {
        cx.effects.sound[0] = 0x35;
        body.phase = Phase::Stomping.byte();
        body.counter = 0;
        body.anim = cx.prg.families.stomper.anims[usize::from(body.timer)];
    }
}

/// The stepping attack (`$88A4`); each stomp shifts x and bursts parts.
fn stomping(
    kind: &mut u8,
    body: &mut Body,
    work: &mut Work,
    visibility: Visibility,
    cx: &mut Cx<'_>,
) {
    let tables = &cx.prg.families.stomper;
    let anim = usize::from(body.anim);
    body.contact = 0;
    if visibility == Visibility::On {
        work.hitbox = 0;
        engine::contact(kind, body, work, 2, cx);
        if body.hit & hit::DYING != 0 {
            return;
        }
        let (screen_x, borrow) = work.screen_x.overflowing_sub(tables.draw_offsets[anim]);
        work.screen_x = screen_x;
        if borrow {
            return;
        }
        work.screen_y = 0xAF;
        let frames = table_word(cx.prg, FRAMES, u16::from(body.anim));
        let touches = table_word(cx.prg, TOUCHES, u16::from(body.anim));
        let index = u16::from(body.counter & 0xFE);
        work.pointer = engine::rom_word(cx.prg, 8, frames + index);
        if engine::rom_byte(cx.prg, 8, touches + ((index + 1) >> 1)) != 0 {
            work.hitbox = 1;
            engine::touch(work, 4, cx);
        }
        work.chr_slot = 7;
        work.tile = 0xC0;
        work.attr = if cx.world.frame_odd { 3 } else { 0 };
        engine::draw(work, 1, cx);
        overlay(work, 2, cx);
    }
    body.counter = body.counter.wrapping_add(1);
    if body.counter < tables.step_counts[anim] {
        if body.counter != tables.stomp_steps[anim] || visibility != Visibility::On {
            return;
        }
        work.x = work.x.wrapping_sub(tables.stomp_shifts[anim]);
        engine::spawn_parts(3, work, cx);
        return;
    }
    if body.timer == 5 {
        body.timer = 6;
        body.phase = Phase::Landing.byte();
        body.counter = 8;
        return;
    }
    body.anim = 0x80;
    body.phase = Phase::Hovering.byte();
    body.counter = 0x20;
    body.timer = body.timer.wrapping_add(1);
    if body.timer >= 8 {
        body.timer = 0;
    }
}

/// The bank-8 `$8A23` overlay: a metasprite projected from `x - 1` (`$90` is the
/// high byte, so `dec $90`/`inc $90` shift the level x by `$100`).
fn overlay(work: &mut Work, y: u8, cx: &mut Cx<'_>) {
    work.attr = if y != 0 && cx.world.frame_odd {
        0x03
    } else {
        0x01
    };
    work.pointer = engine::rom_word(cx.prg, 8, OVERLAYS + u16::from(y));
    work.x = work.x.wrapping_sub(0x100);
    if engine::on_screen(work, cx.world) == Visibility::On {
        work.tile = 0x80;
        work.chr_slot = 6;
        engine::draw(work, 1, cx);
    }
    work.x = work.x.wrapping_add(0x100);
}

/// The low-byte-at-`base`/high-byte-at-`base + 1` pointer table entry (bank 8
/// `$8A89`/`$8AB8`).
fn table_word(prg: &[u8], base: u16, index: u16) -> u16 {
    u16::from_le_bytes([
        engine::rom_byte(prg, 8, base + index),
        engine::rom_byte(prg, 8, base + index + 1),
    ])
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    /// Every `anim` the stomping can take indexes the tables sized by it.
    #[test]
    fn every_anim_is_in_the_tables() {
        let Some(prg) = crate::prg::for_test() else {
            return;
        };
        let tables = &prg.families.stomper;
        assert!(
            tables
                .anims
                .iter()
                .all(|&anim| usize::from(anim) < tables.draw_offsets.len())
        );
    }

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Stage 1-1 room 4 (`$0400=0`, `$0402=4`): the `$0C` spawns at frame 1306
    /// in slot 0. The player's weapon hits would kill it at 1361 into the
    /// unported `$89A6` path (§9c: `$0181`, `$AE0A`, `$A0CE`), so this keeps its
    /// health up; the hits still set `$7D70` bit 6, so the flash draw is diffed.
    fn alive(frame: u64) -> Option<Scene> {
        let mut scene = Scene::tas(frame)?;
        scene.machine.bus.trace.pokes.push((0xD450, 0x7EF0, 0xFF));
        scene.machine.bus.trace.pokes.push((0xD450, 0x7BE0, 0xFF));
        Some(scene)
    }

    /// `$7DF0` (`Body.timer`) pinned at 5, the branch at `$893C` that sends the
    /// step counter to 6 and back to phase 1 with a short counter. The poke is
    /// applied at `$D450`, so both sides see it and the branch's phase/counter
    /// writes are still diffed.

    #[test]
    fn matches_the_oracle_at_step_five() {
        let Some(mut scene) = alive(1306) else {
            return;
        };
        scene.machine.bus.trace.pokes.push((0xD450, 0x7DF0, 5));
        let coverage = assert_actor_phases(scene, 200, &[0x0C]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 184,
                occupied: 184,
                drawn: 184,
                phases: BTreeMap::from([(0, 16), (1, 80), (2, 88)]),
            }
        );
    }

    /// The whole live handler: phase 0 hovers, phase 1 impacts, phase 2 stomps
    /// (which bursts `$08` parts), the overlay drawn every frame.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = alive(1306) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 220, &[0x0C]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 204,
                occupied: 204,
                drawn: 204,
                phases: BTreeMap::from([(0, 80), (1, 80), (2, 44)]),
            }
        );
    }

    /// The natural scene, no poke: phases 0 and 1 and the `$7D70` bit-6 flash
    /// draw, through the first dying frame (1361).
    #[test]
    fn matches_the_oracle_before_the_death() {
        let Some(scene) = Scene::tas(1306) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 55, &[0x0C]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 39,
                occupied: 39,
                drawn: 39,
                phases: BTreeMap::from([(0, 16), (1, 23)]),
            }
        );
    }

    /// `$A9C4` and `$AEE3` reach the player: put him on the actor and confirm the
    /// oracle loses energy, then that the port reproduces every byte of that.
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = alive(1306) else {
            return;
        };
        scene.isolate(&[0x0C]);
        let slot = (0..16)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x0C)
            .expect("a live $0C");
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.player(x.wrapping_sub(0x100), y);
        let mut probe = scene.clone();
        let mut energy = probe.machine.bus.ram[0x478];
        let mut hits = 0;
        for _ in 0..120 {
            probe.run(1, &[]);
            let now = probe.machine.bus.ram[0x478];
            if now < energy {
                hits += 1;
                energy = now;
            }
        }
        assert!(hits > 0, "the oracle takes no contact hit");
        let coverage = assert_actor_phases(scene, 120, &[0x0C]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 104,
                occupied: 104,
                drawn: 104,
                phases: BTreeMap::from([(0, 132), (1, 49), (2, 22)]),
            }
        );
    }

    /// The natural death: the explosion (`$89B7`) and the dying animation, TAS 1355-1380.
    #[test]
    fn matches_the_oracle_through_the_death() {
        let Some(scene) = Scene::tas(1354) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 26, &[0x0C, 0x08]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 26,
                occupied: 26,
                drawn: 26,
                phases: BTreeMap::from([(0, 1), (1, 21), (0xE0, 36)]),
            }
        );
    }
}
