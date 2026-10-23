//! Type `$B4`, the stage 7 boss controller (bank 12 `$8C59`).
//!
//! It never calls `$A000`'s siblings `$A79B`/`$A852`/`$A8E4`: it draws fixed-screen
//! metasprites through `$81C6` (bank 9 at `$A000`) and hands the visible parts to
//! free slots — type `$52` into slots 0-15, `$B8`/`$BC`/`$BE`/`$C0` into 16-23,
//! type `$50` through `$B1C4`. Its `$7DD0` phases come from the table at `$9100`;
//! `$8DB8` leaves holding/dropping for shedding once the boss's hit points reach `$4650`.
//!
//! Its only byte outside `Body`/`Work`/`Effects` is `$FD` (`$8EF1`), the camera's
//! scroll x; see `REPORT.md` and the tests.

use crate::actor::engine::{self, Work};
use crate::actor::slots::Body;
use crate::actor::world::{Cx, Motion, set_first_colours, sound};

/// The `final_boss_defeat` kind (`$8F37`).
const DEFEAT: u8 = 0xB8;
/// The `final_boss_falling_shot` kind (`$8F55`).
const FALLING_SHOT: u8 = 0xBC;
/// The `final_boss_falling_part` kind.
const FALLING_PART: u8 = 0xBE;
/// The `final_boss_bouncer` kind.
const BOUNCER: u8 = 0xC0;
/// The `explosion` family's kind.
const EXPLOSION: u8 = 0x52;

/// The single body draw's metasprite pointers (`$910E`), by `$F1` bits 2-3.
const BODY_DRAW: u16 = 0x910E;
/// The `$E2 == $68` draw's pointers (`$911E`), by `$7C30`.
const SHAPE_DRAW: u16 = 0x911E;
/// The five-piece draw's pointers (`$912F`).
const PIECES: u16 = 0x912F;
/// The five-piece draw's screen positions (`$9139`), x and y interleaved through `$9142`.
const PIECE_X: u16 = 0x9139;
/// `$8DB8`'s `$7C30` values (`$9126`), by `$F1` bits 3-4.
const TURN_FRAMES: u16 = 0x9126;
/// Phase 3's `$E2` values (`$912A`), by `$7E90 >> 3`; `$912D` is its `$7E90 >= $40` arm.
const CHR_RAMP: u16 = 0x912A;
/// The `$BE` children's x low (`$9143`), x high (`$9147`), y low (`$914B`) and
/// y high (`$914F`), by spawn index.
const BE_X_LO: u16 = 0x9143;
const BE_X_HI: u16 = 0x9147;
const BE_Y_LO: u16 = 0x914B;
const BE_Y_HI: u16 = 0x914F;
/// `$907A`'s `$52` offsets (`$90E8`, `$90EE`, `$90F4`, `$90FA`), by `$7E10`.
const SPOT_X_LO: u16 = 0x90E8;
const SPOT_X_HI: u16 = 0x90EE;
const SPOT_Y_LO: u16 = 0x90F4;
const SPOT_Y_HI: u16 = 0x90FA;
/// `$81C6` maps bank 9 at `$A000` for every draw.
const DRAW_BANK: u8 = 9;
/// The `$AFD6` amount and mode the tail passes (`$8E66`).
const CONTACT: u16 = 0x00E0;
const CONTACT_MODE: u8 = 0xC0;
/// `$8DB8`'s hit-point gate: above it the boss stays in its phase.
const PHASE_GATE: u16 = 0x4650;

phases! {
    /// The boss's phase (`$7DD0`), which picks its handler (`$9100`). Outside shedding
    /// `counter` is the hit flash's countdown (`$8E04`).
    Phase {
        /// Holding until the player's boss-entry motion ends.
        Waiting = 0,
        /// Stepping the body sprite through 0/2/4/6 while `anim` counts 16 frames.
        Animating = 1,
        /// Counting `anim` down to 0 on the plain `$E2` bank.
        Holding = 2,
        /// Ramping `$E2` by `anim`, counting up to `$32`, and dropping a `$BC` every 16;
        /// leaves `$80` in `anim` for holding.
        Dropping = 3,
        /// Shedding `$52` parts while `anim` counts up to `$C0`; `counter` picks the
        /// next part's spot (0-5).
        Shedding = 4,
        /// Counting `anim` to `$C0` between spawns.
        Resting = 5,
        /// Spawning `$BE` or `$C0` parts as `scratch` bit 7 flips, each time `anim` reaches
        /// `$3F`.
        Summoning = 6,
    }
}

/// The boss's constant tables (bank 12).
pub struct Tables {
    /// The `$E2` ramp, by `anim >> 3` or `3 + (anim - $40) >> 3`, 0-26 over all `anim` (`$912A`).
    pub chr_ramp: [u8; 27],
    /// `$7C30`, by `$F1` bits 3-4 (0-3) (`$9126`).
    pub turn_frames: [u8; 4],
    /// The five-piece draw's screen x, by the piece 0-4 (`$9139`).
    pub piece_x: [u8; 5],
    /// The five-piece draw's screen y, by the piece 0-4 (`$913A`).
    pub piece_y: [u8; 5],
    /// The `$BE` children's x, by the spawn count 0-3 (`$9147` high, `$9143` low).
    pub be_x: [u16; 4],
    /// The `$BE` children's y, by the spawn count 0-3 (`$914F` high, `$914B` low).
    pub be_y: [u16; 4],
    /// The `$52` parts' x offset, by `counter` 0-5 (`$90EE` high, `$90E8` low).
    pub spot_x: [u16; 6],
    /// The `$52` parts' y offset, by `counter` 0-5 (`$90FA` high, `$90F4` low).
    pub spot_y: [u16; 6],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        let positions: [u8; 10] = engine::rom_bytes(prg, 12, PIECE_X);
        Tables {
            chr_ramp: engine::rom_bytes(prg, 12, CHR_RAMP),
            turn_frames: engine::rom_bytes(prg, 12, TURN_FRAMES),
            piece_x: std::array::from_fn(|i| positions[2 * i]),
            piece_y: std::array::from_fn(|i| positions[2 * i + 1]),
            be_x: engine::rom_split_words(prg, 12, BE_X_HI, BE_X_LO),
            be_y: engine::rom_split_words(prg, 12, BE_Y_HI, BE_Y_LO),
            spot_x: engine::rom_split_words(prg, 12, SPOT_X_HI, SPOT_X_LO),
            spot_y: engine::rom_split_words(prg, 12, SPOT_Y_HI, SPOT_Y_LO),
        }
    }
}

/// Type `$B4` keeps its whole state in [`Body`]; `scratch` bit 7 is summoning's toggle.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8C59`, phases through `$9100`).
    pub fn step(&mut self, _kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        cx.oam.chr[6] = 0xD0;
        engine::load(body, work);
        let flash = engine::boss_flash(body, work, cx.world);
        if flash & 0x80 != 0 {
            if body.dying_timer == 0x01 {
                spawn_b8(body, cx);
                engine::boss_defeated(work.slot, cx);
            }
            return;
        }
        match Phase::from_byte(body.phase) {
            Some(Phase::Waiting) => waiting(body, work, cx),
            Some(Phase::Animating) => animating(body, work, cx),
            Some(Phase::Holding) => holding(body, work, cx),
            Some(Phase::Dropping) => dropping(body, work, cx),
            Some(Phase::Shedding) => shedding(body, work, cx),
            Some(Phase::Resting) => resting(body, work, cx),
            Some(Phase::Summoning) => summoning(body, work, cx),
            None => panic!("type $B4 phase {} is past the $9100 table", body.phase),
        }
    }
}

/// Waiting (`$8C8B`): hold until the player reaches motion 9, then animate.
fn waiting(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    cx.oam.chr[7] = 0xD1;
    if cx.effects.player.motion != Motion::BossEntry {
        body.phase = Phase::Animating.byte();
        body.anim = 0;
    }
    tail(body, work, cx);
}

/// Animating (`$8CA3`): step `$7C30` through 0/2/4/6 for 16 frames, then hold.
fn animating(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    cx.oam.chr[7] = 0xD1;
    body.sprite = (body.anim >> 2) & 0x06;
    body.anim = body.anim.wrapping_add(1);
    if body.anim >= 0x10 {
        body.phase = Phase::Holding.byte();
        body.anim = 0;
    }
    tail(body, work, cx);
}

/// Holding (`$8CC8`): pick the `$E2` CHR bank and wait `$7E90` out into dropping.
fn holding(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    cx.oam.chr[7] = 0xD1;
    cx.oam.chr[2] = 0x71;
    if body.anim == 0 {
        body.phase = Phase::Dropping.byte();
    } else {
        body.anim = body.anim.wrapping_sub(1);
    }
    gate(body, work, cx);
}

/// Dropping (`$8CE3`): ramp `$E2` on `$7E90`, drop a `$BC` every 16 frames, then back to holding.
fn dropping(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    cx.oam.chr[7] = 0xD1;
    let anim = body.anim;
    let ramp = &cx.prg.families.final_boss.chr_ramp;
    if anim < 0x18 {
        let chr = ramp[usize::from(anim >> 3)];
        if chr != 0 {
            cx.oam.chr[2] = chr;
        }
    } else if anim >= 0x40 {
        cx.oam.chr[2] = ramp[3 + usize::from((anim - 0x40) >> 3)];
    }
    body.anim = body.anim.wrapping_add(1);
    if body.anim >= 0x10 && body.anim.is_multiple_of(0x10) {
        spawn_bc(cx);
    }
    if body.anim >= 0x32 {
        body.phase = Phase::Holding.byte();
        body.anim = 0x80;
    }
    gate(body, work, cx);
}

/// Shedding (`$8D2D`): count `$7E90` up through `$8EDB`, drawing the shape from `$68`.
fn shedding(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    cx.oam.chr[6] = 0xB3;
    count_up(body, work, cx);
    if body.anim >= 0x68 {
        cx.oam.chr[7] = 0xD4;
        work.attr = 0;
        work.tile = 0;
        body.sprite = 0;
        shape_draw(body, work, cx);
    }
}

/// Resting (`$8D50`): count `$7E90` to `$C0`, then hand over to summoning with the y offset.
fn resting(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    cx.oam.chr[7] = 0xD4;
    body.anim = body.anim.wrapping_add(1);
    if body.anim == 0xC0 {
        body.phase = Phase::Summoning.byte();
        body.anim = 0;
        body.timer = cx.effects.player.x.to_be_bytes()[0].wrapping_shl(4);
    }
    body.sprite = 0;
    tail(body, work, cx);
}

/// Summoning (`$8D7A`): alternate the `7D30` bit as `anim` reaches `$3F` and drop `$C0` or `$BE` parts.
fn summoning(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    cx.oam.chr[7] = 0xD4;
    body.sprite = ((body.anim & 0x08) >> 2).wrapping_add(2);
    body.anim = body.anim.wrapping_add(1);
    if body.anim & 0x3F == 0x3F {
        body.scratch ^= 0x80;
        if body.scratch & 0x80 != 0 {
            spawn_be(body, cx);
        } else {
            spawn_c0(body, work, cx);
        }
        body.anim = 0;
        body.phase = Phase::Resting.byte();
    }
    tail(body, work, cx);
}

/// `$8EDB`: the shedding count, its `$FD` write and the `$E2 = $68` swap at `$60`.
fn count_up(body: &mut Body, work: &Work, cx: &mut Cx<'_>) {
    if body.anim < 0xAF && body.anim.is_multiple_of(0x04) {
        spawn_52(body, work, cx);
    }
    cx.scroll.scroll_x = u8::from(cx.world.frame & 0x04 != 0);
    body.anim = body.anim.wrapping_add(1);
    if body.anim >= 0xC0 {
        body.phase = Phase::Resting.byte();
        body.counter = 0;
        body.anim = 0;
        body.sprite = 0;
        cx.scroll.scroll_x = 0;
        body.scratch = 0;
    } else if body.anim >= 0x60 {
        cx.oam.chr[2] = 0x68;
    }
}

/// `$8DB8`: pick `$7C30`, then leave for shedding above the hit-point gate or draw on.
fn gate(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    body.sprite = cx.prg.families.final_boss.turn_frames[usize::from((cx.world.frame >> 3) & 0x03)];
    if cx.effects.boss_hp > PHASE_GATE {
        tail(body, work, cx);
        return;
    }
    body.phase = Phase::Shedding.byte();
    body.timer = 0;
    body.counter = 0;
    body.anim = 0;
    for slot in 16..32 {
        cx.slots.set_kind(slot, 0);
    }
    for slot in 0..16 {
        if cx.slots.kind(slot) == 0xB6 {
            cx.slots.set_kind(slot, 0);
        }
    }
    set_first_colours(&mut cx.effects.palette, 0x0F);
}

/// `$8E04`: the shared draw, boss contact and hit-flash tail.
fn tail(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    work.tile = 0;
    work.attr = 0;
    if cx.oam.chr[2] == 0x68 {
        shape_draw(body, work, cx);
        if cx.world.frame & 0x08 == 0 {
            let tables = &cx.prg.families.final_boss;
            for index in (0..=4u8).rev() {
                let y = u16::from(index) * 2;
                work.pointer = engine::rom_word(cx.prg, 12, PIECES + y);
                work.screen_x = tables.piece_x[usize::from(index)];
                work.screen_y = tables.piece_y[usize::from(index)];
                engine::draw(work, DRAW_BANK, cx);
            }
        }
    } else {
        let y = u16::from((cx.world.frame & 0x0C) >> 1);
        work.pointer = engine::rom_word(cx.prg, 12, BODY_DRAW + y);
        work.screen_x = 0x80;
        work.screen_y = 0x40;
        engine::draw(work, DRAW_BANK, cx);
    }
    let before = cx.effects.boss_hp;
    work.hitbox = 4;
    engine::boss_contact(CONTACT, CONTACT_MODE, body, work, cx);
    if cx.effects.boss_hp.wrapping_sub(before).to_be_bytes()[0] != 0 && body.counter == 0 {
        body.counter = 0x80;
    }
    match body.counter {
        0 => {}
        0x80.. => {
            set_first_colours(&mut cx.effects.palette, 0x32);
            queue_palette(cx);
            body.counter = 0x40;
        }
        0x40..=0x7F => {
            set_first_colours(&mut cx.effects.palette, 0x0F);
            queue_palette(cx);
            body.counter = 0x07;
        }
        counter => body.counter = counter - 1,
    }
}

/// `$8F1D`: draw the `$911E` metasprite at `$7C30`, fixed at screen (128, 88).
fn shape_draw(body: &Body, work: &mut Work, cx: &mut Cx<'_>) {
    work.pointer = engine::rom_word(cx.prg, 12, SHAPE_DRAW + u16::from(body.sprite));
    work.screen_x = 0x80;
    work.screen_y = 0x58;
    engine::draw(work, DRAW_BANK, cx);
}

/// `$C34B`: queue the palette shadow, but only while the VRAM queue has room.
fn queue_palette(cx: &mut Cx<'_>) {
    if cx.effects.vram.top < 0x70 {
        return;
    }
    let palette = cx.effects.palette;
    cx.effects.vram.push_palette(&palette);
}

/// `$8F37`: palette `$0F`, then a type `$B8` into the highest free slot 0-15.
fn spawn_b8(body: &mut Body, cx: &mut Cx<'_>) {
    set_first_colours(&mut cx.effects.palette, 0x0F);
    let Some(slot) = (0..16).rev().find(|&slot| cx.slots.main[slot].kind == 0) else {
        return;
    };
    cx.slots.main[slot].kind = DEFEAT;
    body.counter = 0;
    body.anim = 0;
}

/// `$8F55`: a type `$BC` at (0x0800, 0x0580) into the highest free slot 16-23.
fn spawn_bc(cx: &mut Cx<'_>) {
    let Some(slot) = engine::free_child_slot(cx.slots) else {
        return;
    };
    let child = &mut cx.slots.main[slot];
    child.kind = FALLING_SHOT;
    let body = &mut child.body;
    (body.x, body.y) = (0x0800, 0x0580);
    (body.state, body.subtype, body.phase) = (0, 0, 0);
    (body.timer, body.counter, body.anim) = (0, 0, 0);
}

/// `$8F89`: up to four type `$BE` children at the `$9143` offsets.
fn spawn_be(body: &Body, cx: &mut Cx<'_>) {
    let tables = &cx.prg.families.final_boss;
    for count in 0..4u8 {
        let Some(slot) = engine::free_child_slot(cx.slots) else {
            return;
        };
        let n = usize::from(count);
        let (x, y) = (tables.be_x[n], tables.be_y[n]);
        let (subtype, phase) = if body.timer & 0x80 == 0 {
            (if count < 2 { 0xF0 } else { 0xD8 }, 0xFF)
        } else {
            (if count < 2 { 0x28 } else { 0x10 }, 0)
        };
        let child = &mut cx.slots.main[slot];
        child.kind = FALLING_PART;
        let part = &mut child.body;
        (part.x, part.y) = (x, y);
        (part.state, part.timer, part.counter, part.anim) = (0, 0, 0, 0);
        (part.subtype, part.phase) = (subtype, phase);
    }
}

/// `$8FFA`: up to two type `$C0` children at (0x0800, 0x0580), facing off the boss.
fn spawn_c0(body: &mut Body, work: &Work, cx: &mut Cx<'_>) {
    if body.behaviour != 0 {
        return;
    }
    for count in 0..2u8 {
        let Some(slot) = engine::free_child_slot(cx.slots) else {
            return;
        };
        let (behaviour, subtype, phase) = if count == 0 {
            body.behaviour |= 0x80;
            (0, 0x20, 0)
        } else {
            body.behaviour |= 0x40;
            (count, 0xE0, 0xFF)
        };
        let child = &mut cx.slots.main[slot];
        child.kind = BOUNCER;
        let part = &mut child.body;
        (part.x, part.y) = (0x0800, 0x0580);
        (part.state, part.anim, part.chr_slot, part.cell) = (0, 0, 0, 0);
        (part.timer, part.counter) = (0x14, 0);
        (part.behaviour, part.subtype, part.phase) = (behaviour, subtype, phase);
        part.sprite = work.slot;
    }
}

/// `$907A`: a type `$52` part at a `$7E10`-indexed offset, once every fourth count.
fn spawn_52(body: &mut Body, work: &Work, cx: &mut Cx<'_>) {
    cx.effects.sound[0] = sound::EXPLOSION;
    let n = usize::from(body.counter);
    let tables = &cx.prg.families.final_boss;
    let x = work.x.wrapping_add(tables.spot_x[n]);
    let y = work.y.wrapping_add(tables.spot_y[n]);
    if x & 0xF000 == 0
        && y & 0xF000 == 0
        && let Some(slot) = (0..16).rev().find(|&slot| cx.slots.main[slot].kind == 0)
    {
        let part = &mut cx.slots.main[slot];
        part.kind = EXPLOSION;
        part.body.chr_slot = 0x06;
        part.body.x = x;
        part.body.y = y;
        part.body.anim = 0;
    }
    body.counter = body.counter.wrapping_add(1);
    if body.counter >= 6 {
        body.counter = 0;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// The slot the level-start record puts the boss in (§8b, checked by `occupied`).
    const BOSS_SLOT: u16 = 1;

    /// Frees every slot but the boss's own at the loop head, so the parts it spawns
    /// never run; their spawn writes are still compared.
    fn isolate_children(mut scene: Scene) -> Scene {
        let pokes = &mut scene.machine.bus.trace.pokes;
        for slot in (0..=15u16).filter(|&slot| slot != BOSS_SLOT) {
            pokes.push((0xD450, 0x7CC0 + slot, 0));
        }
        for slot in 0..8u16 {
            pokes.push((0xD450, 0x7CD0 + slot, 0));
        }
        scene
    }

    /// Frames 39680-40087: the boss's phases 0-4, 387 actor loops (the rest drop on
    /// an NMI), every one occupied, 384 drawn, all compared byte for byte — the short
    /// window beside the whole-life test below.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(39679) else {
            return;
        };
        let coverage = assert_actor_phases(isolate_children(scene), 408, &[0xB4]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 387,
                occupied: 387,
                drawn: 384,
                phases: BTreeMap::from([(0, 100), (1, 16), (2, 169), (3, 100), (4, 2)]),
            }
        );
    }

    /// The whole life (frames 39680-40894), every phase 0-6.
    ///
    /// The count-up writes `$FD` from `$F1` (`$8EF1`); now that the handler reaches the
    /// scroll through `Cx`, the port reproduces those writes and the comparison covers
    /// them natively, so neither `$F1` nor `$FD` is held.
    #[test]
    fn matches_the_oracle_through_every_phase() {
        let Some(scene) = Scene::tas(39679) else {
            return;
        };
        let coverage = assert_actor_phases(isolate_children(scene), 1215, &[0xB4]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 1194,
                occupied: 1194,
                drawn: 1089,
                phases: BTreeMap::from([
                    (0, 100),
                    (1, 16),
                    (2, 169),
                    (3, 100),
                    (4, 192),
                    (5, 491),
                    (6, 126),
                ]),
            }
        );
    }
}
