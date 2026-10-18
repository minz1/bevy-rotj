//! Batman's projectiles: the bank-5 routine the gameplay loop calls at `$D444`.
//!
//! The routine dispatches on `$0476` through the table at `$801B`. [`step`] ports
//! handlers `$8029` (`$0476` 0), `$8170` (1), `$8891` (2), `$8D39` (3), `$91B2` (4), `$9647` (5)
//! and `$9D2E` (6, the power-up's pieces), the
//! bank-5 subroutines they call (`$9E13`, `$9E46`, `$9ED6`) and the draw (`$C247`,
//! [`engine::draw`]); `docs/ACTORS.md` §9g.

use super::collision::{self, Probe};
use super::engine::{self, Work, rom_byte, rom_word};
use super::oam::{Mark, attr};
use super::scroll::Scroll;
use super::slots::{self, hit};
use super::world::{
    Cx, Motion, Projectile, Weapon, attack, busy, power_up, set_first_colours, stage_script,
};
use crate::actor::button::Buttons;
use crate::actors::STATE_UNTARGETED;

/// A charged attack's first stage, every weapon's: the `kind` the player's charge writes.
pub const CHARGE_START: u8 = 0x80;

/// The homing weapon's charged attack, by projectile `kind`.
mod homing_stage {
    /// Gathering a ball while B is held.
    pub const GATHER: u8 = super::CHARGE_START;
    /// The ball thrown.
    pub const THROWN: u8 = 0x81;
    /// The three parts circling the ball (they do nothing on their own).
    pub const CIRCLING: u8 = 0x82;
}

/// The splitter's charged attack: `kind` `$81` and up fly the plain path, its high bit
/// picking the thrown sprites.
mod splitter_stage {
    /// The four parts orbiting the player.
    pub const ORBIT: u8 = super::CHARGE_START;
    /// A part let go.
    pub const THROWN: u8 = 0x81;
}

/// The wave's charged attack.
mod wave_stage {
    /// Gathering for 8 frames.
    pub const CHARGE: u8 = super::CHARGE_START;
    /// The 64-frame fan.
    pub const SPREAD: u8 = 0x81;
    /// The fan's shots: every `kind` from here up.
    pub const SPREAD_SHOT: u8 = 0x82;
}

/// The three-way weapon's charged attack.
mod three_way_stage {
    /// Bubbles every 8th frame.
    pub const BREW: u8 = super::CHARGE_START;
    /// A bubble.
    pub const BUBBLE: u8 = 0x81;
    /// The tether.
    pub const TETHER: u8 = 0x82;
}

/// The super weapon's attack.
mod super_stage {
    /// The charge that claims the other slots.
    pub const UNLEASH: u8 = super::CHARGE_START;
    /// A part orbiting the centre.
    pub const ORBITER: u8 = 0x81;
    /// The pair spawner.
    pub const PAIRS: u8 = 0x82;
    /// A slot the attack claimed.
    pub const CLAIMED: u8 = 0x83;
    /// The orbit's centre.
    pub const CENTRE: u8 = 0x84;
}

/// The weapons' constant tables (bank 5).
pub struct Tables {
    /// The homing charge's x launch velocity, by pad direction (0-3) (`$8881`, `$8885`).
    pub launch_vx: [i16; 4],
    /// The homing charge's y launch velocity, by pad direction (0-3) (`$8889`, `$888D`).
    pub launch_vy: [i16; 4],
    /// The homing charge's x offset in front, facing right, by `motion` (0-11) (`$8726`, `$8732`).
    pub front_right_dx: [u16; 12],
    /// The homing charge's x offset in front, facing left, by `motion` (0-11) (`$873E`, `$874A`).
    pub front_left_dx: [u16; 12],
    /// The homing charge's y offset in front, by `motion` (0-11) (`$8756`, `$8762`).
    pub front_dy: [u16; 12],
    /// The homing shot's draw attribute, by heading (0-19) (`$8437`).
    pub homing_attributes: [u8; 20],
    /// The circled parts' (x, y) offsets by `param & $3F`, past 31 the next tables' bytes (`$87FC`).
    pub circle_offsets: [(u16, u16); 64],
    /// The circled parts' starting angles, by spawn count (0-2) (`$887C`).
    pub circle_angles: [u8; 3],
    /// The power-up piece's x velocity step, by corner `param` (0-3) (`$9E00`, `$9E04`).
    pub piece_step_x: [i16; 4],
    /// The power-up piece's y velocity step, by corner `param` (0-3) (`$9E08`, `$9E0C`).
    pub piece_step_y: [i16; 4],
    /// The power-up piece's sprite frame, by `part` (0-7) (`$9DF0`).
    pub piece_frames: [u8; 8],
    /// The power-up's sprite palette (`$9E10`).
    pub piece_palette: [u8; 3],
    /// The super orbiter's sprite palette on odd frames (`$9D19`).
    pub orbiter_palette_odd: [u8; 4],
    /// The super orbiter's sprite palette on even frames (`$9D1D`).
    pub orbiter_palette_even: [u8; 4],
    /// The sprite palette restored when the super attack ends (`$9D21`).
    pub super_palette: [u8; 3],
    /// A split part's x velocity, by `part` (0-3) (`$8A2F`, `$8A33`).
    pub split_vx: [i16; 4],
    /// A split part's y velocity, by `part` (0-3) (`$8A37`, `$8A3B`).
    pub split_vy: [i16; 4],
    /// A split part's bounce byte, by `part` (0-3) (`$8A3F`).
    pub split_bounce: [u8; 4],
    /// A split part's x offset, facing right, by `motion` (0-11) (`$8CF1`, `$8CFD`).
    pub split_right_dx: [u16; 12],
    /// A split part's x offset, facing left, by `motion` (0-11) (`$8D09`, `$8D15`).
    pub split_left_dx: [u16; 12],
    /// A split part's y offset, by `motion` (0-11) (`$8D21`, `$8D2D`).
    pub split_dy: [u16; 12],
    /// The wave's x offset, facing right, by `motion` (0-11) (`$914A`, `$9156`).
    pub wave_right_dx: [u16; 12],
    /// The wave's x offset, facing left, by `motion` (0-11) (`$9162`, `$9168`).
    pub wave_left_dx: [u16; 12],
    /// The wave charge's y offset, by `motion` (0-11) (`$916E`, `$917A`).
    pub wave_charge_dy: [u16; 12],
    /// The wave spread's y offset, by `motion` (0-11) (`$916E`, `$9186`).
    pub wave_spread_dy: [u16; 12],
    /// The wave spread's shot speed, by `part` (0-15) (`$9192`, `$91A2`).
    pub spread_vx: [i16; 16],
    /// The three-way charge's x offset ahead, facing right, by `motion` (0-11) (`$9F07`, `$9F13`).
    pub ahead_right_dx: [u16; 12],
    /// The three-way charge's x offset ahead, facing left, by `motion` (0-11) (`$9F1F`, `$9F2B`).
    pub ahead_left_dx: [u16; 12],
    /// The three-way charge's y offset, by `motion` (0-11) (`$9F37`, `$9F43`).
    pub ahead_dy: [u16; 12],
    /// The three-way bubble's x velocity, by `pick` (0-5) (`$9EEF`, `$9EF5`).
    pub three_way_vx: [i16; 6],
    /// The three-way bubble's y velocity, by `pick` (0-5) (`$9EFB`, `$9F01`).
    pub three_way_vy: [i16; 6],
    /// The bubble's draw attribute, by `$F1` bit 3 (`$9637`).
    pub bubble_palette: [u8; 2],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        let circle = engine::rom_bytes::<256>(prg, 5, 0x87FC);
        Tables {
            launch_vx: engine::rom_split_words(prg, 5, 0x8885, 0x8881).map(u16::cast_signed),
            launch_vy: engine::rom_split_words(prg, 5, 0x888D, 0x8889).map(u16::cast_signed),
            front_right_dx: engine::rom_split_words(prg, 5, 0x8732, 0x8726),
            front_left_dx: engine::rom_split_words(prg, 5, 0x874A, 0x873E),
            front_dy: engine::rom_split_words(prg, 5, 0x8762, 0x8756),
            homing_attributes: engine::rom_bytes(prg, 5, 0x8437),
            circle_offsets: std::array::from_fn(|i| {
                (
                    u16::from_be_bytes([circle[4 * i], circle[4 * i + 1]]),
                    u16::from_be_bytes([circle[4 * i + 2], circle[4 * i + 3]]),
                )
            }),
            circle_angles: engine::rom_bytes(prg, 5, 0x887C),
            piece_step_x: engine::rom_split_words(prg, 5, 0x9E04, 0x9E00).map(u16::cast_signed),
            piece_step_y: engine::rom_split_words(prg, 5, 0x9E0C, 0x9E08).map(u16::cast_signed),
            piece_frames: engine::rom_bytes(prg, 5, 0x9DF0),
            piece_palette: engine::rom_bytes(prg, 5, 0x9E10),
            orbiter_palette_odd: engine::rom_bytes(prg, 5, 0x9D19),
            orbiter_palette_even: engine::rom_bytes(prg, 5, 0x9D1D),
            super_palette: engine::rom_bytes(prg, 5, 0x9D21),
            split_vx: engine::rom_split_words(prg, 5, 0x8A2F, 0x8A33).map(u16::cast_signed),
            split_vy: engine::rom_split_words(prg, 5, 0x8A37, 0x8A3B).map(u16::cast_signed),
            split_bounce: engine::rom_bytes(prg, 5, 0x8A3F),
            split_right_dx: engine::rom_split_words(prg, 5, 0x8CFD, 0x8CF1),
            split_left_dx: engine::rom_split_words(prg, 5, 0x8D15, 0x8D09),
            split_dy: engine::rom_split_words(prg, 5, 0x8D2D, 0x8D21),
            wave_right_dx: engine::rom_split_words(prg, 5, 0x9156, 0x914A),
            wave_left_dx: engine::rom_split_words(prg, 5, 0x9168, 0x9162),
            wave_charge_dy: engine::rom_split_words(prg, 5, 0x917A, 0x916E),
            wave_spread_dy: engine::rom_split_words(prg, 5, 0x9186, 0x916E),
            spread_vx: engine::rom_split_words(prg, 5, 0x91A2, 0x9192).map(u16::cast_signed),
            ahead_right_dx: engine::rom_split_words(prg, 5, 0x9F13, 0x9F07),
            ahead_left_dx: engine::rom_split_words(prg, 5, 0x9F2B, 0x9F1F),
            ahead_dy: engine::rom_split_words(prg, 5, 0x9F43, 0x9F37),
            three_way_vx: engine::rom_split_words(prg, 5, 0x9EF5, 0x9EEF).map(u16::cast_signed),
            three_way_vy: engine::rom_split_words(prg, 5, 0x9F01, 0x9EFB).map(u16::cast_signed),
            bubble_palette: engine::rom_bytes(prg, 5, 0x9637),
        }
    }
}

/// One call of the bank-5 weapons routine (`$D444` -> `$D447`), `overflow` the 6502
/// overflow flag the loop leaves before it (weapon 5's charge reads it).
pub fn step(work: &mut Work, cx: &mut Cx<'_>, overflow: bool) {
    // The handlers below take the scroll directly; lend it out of the context.
    let mut scroll = std::mem::take(cx.scroll);
    let marking = cx.oam.marking;
    match cx.effects.weapon {
        Weapon::PowerUpPieces => power_up_pieces(work, cx),
        weapon => each_projectile(work, cx, &mut scroll, weapon, overflow),
    }
    cx.oam.marking = marking;
    *cx.scroll = scroll;
}

/// Handler 6 (`$9D2E`), the power-up's four pieces: on even frames each flies (its
/// velocity stepped by `$9E00-$9E0F` for its corner `$7F40`) and is drawn kept on
/// the screen; after 8 steps it is gone and Batman is powered up: `$0472` and `$B8`
/// set, weapon 5, his motion back from `$040F`, `$C2` from `$0409`, and for the
/// pieces with `$7F60` bit 7 the sprite palette's second set from `$9E10`.
fn power_up_pieces(work: &mut Work, cx: &mut Cx<'_>) {
    if cx.world.frame_odd {
        return;
    }
    work.anchor = 0;
    work.tile = 0;
    work.attr = 3;
    work.chr_slot = 5;
    for slot in (0..16).rev() {
        if cx.effects.projectiles[slot].kind == 0 {
            continue;
        }
        advance(slot, work, cx);
        cx.oam.marking = Mark::Projectile(u8::try_from(slot).expect("16 projectile slots"));
        if project_keep(slot, work, cx) {
            let frame = cx.prg.weapons.piece_frames[usize::from(cx.effects.projectiles[slot].part)];
            work.pointer = rom_word(cx.prg, 5, 0x9DF8 + u16::from(frame));
            draw(work, cx);
        }
        let prg = cx.prg;
        let effects = &mut *cx.effects;
        let piece = &mut effects.projectiles[slot];
        let corner = usize::from(piece.param);
        piece.vx = piece.vx.wrapping_add(prg.weapons.piece_step_x[corner]);
        piece.vy = piece.vy.wrapping_add(prg.weapons.piece_step_y[corner]);
        piece.part = piece.part.wrapping_add(1);
        if piece.part < 8 {
            continue;
        }
        piece.kind = 0;
        let lit = piece.part_timer & 0x80 != 0;
        let player = &mut effects.player;
        player.power_up = power_up::ON;
        effects.attack = attack::CHARGEABLE;
        effects.weapon = Weapon::Super;
        player.facing &= !attr::PALETTE;
        player.stage_script = 0;
        player.hurt = 0;
        player.flash_timer = 0;
        effects.busy = 0;
        effects.scroll_lock = effects.stage_end.timer;
        player.motion =
            Motion::from_byte(effects.stage_args[1]).expect("the motion the power-up saved");
        if lit {
            effects.palette[17..20].copy_from_slice(&prg.weapons.piece_palette);
            effects.palette_dirty = 0x80;
        }
    }
}

/// The shared slot scan (`$8037`): `$9A`/`$A4`/`$98` reset, then slots 15 down to 0, the
/// overflow flag carried from slot to slot for weapon 5's charge.
fn each_projectile(
    work: &mut Work,
    cx: &mut Cx<'_>,
    scroll: &mut Scroll,
    weapon: Weapon,
    mut overflow: bool,
) {
    work.anchor = 0;
    work.tile = 0;
    work.chr_slot = 4;
    if weapon == Weapon::ThreeWay {
        work.attr = 0;
    }
    for slot in (0..16).rev() {
        if cx.effects.projectiles[slot].kind != 0 {
            cx.oam.marking = Mark::Projectile(u8::try_from(slot).expect("16 projectile slots"));
            match weapon {
                Weapon::Bolt => bolt(slot, work, cx),
                Weapon::Homing => homing(slot, work, cx),
                Weapon::Splitter => splitter(slot, work, cx),
                Weapon::ThreeWay => three_way(slot, work, cx),
                Weapon::Super => {
                    if !super_weapon(slot, work, cx, scroll, &mut overflow) {
                        return;
                    }
                }
                Weapon::Wave => {
                    if !wave(slot, work, cx) {
                        return;
                    }
                }
                Weapon::PowerUpPieces => unreachable!("the pieces fly in power_up_pieces"),
            }
        }
    }
}

/// Gravity's `$7EB0`/`$7D20` add: `$7EB0 += 6`, carrying into the high byte; returns
/// the overflow flag the `ADC #$06` leaves.
fn gravity(projectile: &mut Projectile) -> bool {
    let [hi, lo] = projectile.vy.to_be_bytes();
    let (lo_after, carry) = lo.overflowing_add(6);
    let hi = if carry { hi.wrapping_add(1) } else { hi };
    projectile.vy = i16::from_be_bytes([hi, lo_after]);
    engine::adc_overflows(lo, 6, false)
}

/// Integrates the projectile's level position by its velocity (`$9E13`).
fn advance(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    let projectile = &mut cx.effects.projectiles[slot];
    let (x_lo, carry) = projectile
        .level_x_lo
        .overflowing_add(projectile.vx.to_be_bytes()[1]);
    let x_hi = projectile
        .level_x_hi
        .wrapping_add(projectile.vx.to_be_bytes()[0])
        .wrapping_add(u8::from(carry));
    projectile.level_x_lo = x_lo;
    projectile.level_x_hi = x_hi;
    let (y_lo, carry) = projectile
        .level_y_lo
        .overflowing_add(projectile.vy.to_be_bytes()[1]);
    let y_hi = projectile
        .level_y_hi
        .wrapping_add(projectile.vy.to_be_bytes()[0])
        .wrapping_add(u8::from(carry));
    projectile.level_y_lo = y_lo;
    projectile.level_y_hi = y_hi;
    work.x = u16::from_be_bytes([x_hi, x_lo]);
    work.y = u16::from_be_bytes([y_hi, y_lo]);
}

/// Projects the working position onto the screen, freeing the slot when off it (`$9E46`, `A = 0`).
fn project_or_free(slot: usize, work: &mut Work, cx: &mut Cx<'_>) -> bool {
    project_to_screen(slot, work, cx, false)
}

/// `$9E46` with `A = $80`: off the screen keeps the slot, and a row just below it
/// sets the bottom anchor `$9A`.
fn project_keep(slot: usize, work: &mut Work, cx: &mut Cx<'_>) -> bool {
    project_to_screen(slot, work, cx, true)
}

/// `$9E46`: on the screen writes `$94/$95` and `$7B80/$7B90`.
fn project_to_screen(slot: usize, work: &mut Work, cx: &mut Cx<'_>, keep: bool) -> bool {
    let (camera_x, camera_y) = (cx.world.camera_x, cx.world.camera_y);
    let dx = work.x.wrapping_sub(camera_x);
    if work.x < camera_x || dx.to_be_bytes()[0] >= 0x10 {
        if !keep {
            cx.effects.projectiles[slot].kind = 0;
        }
        return false;
    }
    work.screen_x = (dx >> 4).to_le_bytes()[0];
    let dy = work.y.wrapping_sub(camera_y);
    if work.y < camera_y || dy.to_be_bytes()[0] >= 0x0F {
        if !keep {
            cx.effects.projectiles[slot].kind = 0;
        } else if dy.to_be_bytes()[0] < 0x15 {
            work.anchor = 0x40;
        }
        return false;
    }
    work.screen_y = (dy >> 4).to_le_bytes()[0];
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.screen_y = work.screen_y;
    projectile.screen_x = work.screen_x;
    true
}

/// Draws [`Work::pointer`] from the window its address falls in (`$C247`); a stale
/// timer can point past the sprite tables, into the loop's own bank 14 or bank 15.
fn draw(work: &mut Work, cx: &mut Cx<'_>) -> bool {
    let bank = match work.pointer {
        0x8000..=0x9FFF => 5,
        0xA000..=0xBFFF => 1,
        0xC000..=0xDFFF => 14,
        0xE000..=0xFFFF => 15,
        other => panic!("projectile metasprite ${other:04X} is below the PRG windows"),
    };
    engine::draw(work, bank, cx)
}

/// Is the metatile at the working position solid (`$C93B`)?
fn solid(work: &Work, flags: u8, cx: &Cx<'_>) -> bool {
    let mut probe = Probe {
        x: work.x,
        y: work.y,
        ..Probe::default()
    };
    collision::solid(&mut probe, flags, cx) & 0x80 != 0
}

/// Draws only on every other frame (`LDA $10; EOR $F1; LSR; BCS`); the overflow flag the
/// draw leaves, if it drew.
fn draw_alternate_frames(slot: usize, work: &mut Work, cx: &mut Cx<'_>) -> Option<bool> {
    ((u8::try_from(slot).expect("slot < 16") ^ cx.world.frame) & 1 == 0).then(|| draw(work, cx))
}

/// The overflow flag `$9E46`'s projection leaves: off the screen its last `BIT $30` (`A`
/// `$00` or `$80`) clears it, on the screen the y subtraction sets it.
fn projection_overflow(on_screen: bool, work: &Work, cx: &Cx<'_>) -> bool {
    on_screen && engine::sub_overflows16(work.y, cx.world.camera_y)
}

/// Handler `$8029` (`$0476` 0): the fired projectile.
fn bolt(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    flying_shot(
        slot,
        work,
        cx,
        sprite_by_timer(slot, [0x814C, 0x814A]),
        0x814E,
    );
}

/// Handler `$8170` (`$0476` 1): the homing shot; types `$80` and up are its
/// charged attack (`$847D`).
fn homing(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    let kind = cx.effects.projectiles[slot].kind;
    if kind < 0x80 {
        flying_shot(slot, work, cx, |cx| homing_steer(slot, cx), 0x8477);
        return;
    }
    if cx.world.frame.trailing_zeros() >= 5 {
        cx.effects.sound[0] = 0x2E;
    }
    match kind {
        homing_stage::GATHER => homing_gather(slot, work, cx),
        homing_stage::THROWN => homing_thrown(slot, work, cx),
        homing_stage::CIRCLING => {}
        other => panic!("handler 1's type ${other:02X} is past the $876E table"),
    }
}

/// `$8288`: picks the homing shot's sprite and `$7C20` by its heading, then turns
/// it 2 a frame toward its target slot `$7F40` (a new one from `$83FC` when that
/// one is gone).
fn homing_steer(slot: usize, cx: &mut Cx<'_>) -> u16 {
    let projectile = cx.effects.projectiles[slot];
    let lo = projectile.vy.to_be_bytes()[1];
    let base = match (projectile.vx < 0, projectile.vy < 0) {
        (false, true) => 0,
        (false, false) => 5,
        (true, true) => 10,
        (true, false) => 15,
    };
    let heading = if projectile.vx == 0 {
        base + 4
    } else if projectile.vy < 0 {
        base + [0xF8, 0xE8, 0xD8, 0xC8]
            .iter()
            .take_while(|&&edge| lo < edge)
            .count()
    } else {
        match [0x08, 0x18, 0x28]
            .iter()
            .take_while(|&&edge| lo >= edge)
            .count()
        {
            3 if lo < 0x38 => base + 4,
            steps => base + steps,
        }
    };
    let heading = u16::try_from(heading).expect("20 headings");
    cx.effects.projectiles[slot].attr = cx.prg.weapons.homing_attributes[usize::from(heading)];
    let pointer = rom_word(cx.prg, 5, 0x844B + heading * 2);
    let actor = |cx: &Cx<'_>, target: u8, array: u16| {
        slots::read(cx.slots, &cx.effects.projectiles, array + u16::from(target))
    };
    let mut target = projectile.param;
    if target & 0x80 != 0 || actor(cx, target, 0x7CC0) == 0 {
        target = homing_retarget(slot, cx);
    }
    if target & 0x80 == 0 && actor(cx, target, 0x7D50) & STATE_UNTARGETED != 0 {
        target = homing_retarget(slot, cx);
    }
    if target & 0x80 != 0 {
        return pointer;
    }
    #[cfg(test)]
    super::oracle::seen(0x831E);
    let position = |cx: &Cx<'_>, hi: u16, lo: u16| {
        u16::from_be_bytes([actor(cx, target, hi), actor(cx, target, lo)])
    };
    let projectile = &cx.effects.projectiles[slot];
    let dy = position(cx, 0x7B40, 0x7B60).wrapping_sub(u16::from_be_bytes([
        projectile.level_y_hi,
        projectile.level_y_lo,
    ]));
    let dx = position(cx, 0x7B00, 0x7B20).wrapping_sub(u16::from_be_bytes([
        projectile.level_x_hi,
        projectile.level_x_lo,
    ]));
    let (left, up) = (dx & 0x8000 != 0, dy & 0x8000 != 0);
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.vx = steer_axis(projectile.vx, left);
    if left && up {
        loop {
            projectile.vy = projectile.vy.wrapping_sub(2);
            let [hi, lo] = projectile.vy.to_be_bytes();
            if projectile.vy >= 0 {
                break;
            }
            if lo < 0xE0 {
                projectile.vy = i16::from_be_bytes([hi, 0xE0]);
                break;
            }
        }
    } else {
        projectile.vy = steer_axis(projectile.vy, up);
    }
    pointer
}

/// One axis of `$8288`'s turn: 2 down (`down`) or up, the low byte held to
/// `$E0` going negative and `$20` going positive.
fn steer_axis(velocity: i16, down: bool) -> i16 {
    let velocity = if down {
        velocity.wrapping_sub(2)
    } else {
        velocity.wrapping_add(2)
    };
    let [hi, lo] = velocity.to_be_bytes();
    match (velocity < 0, down) {
        (true, true) if lo < 0xE0 => i16::from_be_bytes([hi, 0xE0]),
        (false, false) if lo >= 0x20 => i16::from_be_bytes([hi, 0x20]),
        _ => velocity,
    }
}

/// `$830E`: a new target for the homing shot from `$83FC`, kept in `$7F40`.
fn homing_retarget(slot: usize, cx: &mut Cx<'_>) -> u8 {
    let hittable = |&target: &usize| {
        let actor = &cx.slots.main[target];
        actor.kind != 0
            && actor.body.contact & 0x20 == 0
            && actor.body.state & STATE_UNTARGETED == 0
    };
    let found = if slot & 1 == 0 {
        (0..16).find(hittable)
    } else {
        (0..16).rev().find(hittable)
    };
    let target = found.map_or(0xFF, |slot| u8::try_from(slot).expect("slot < 16"));
    cx.effects.projectiles[slot].param = target;
    target
}

/// Type `$80` (`$8496`): the charge gathers in front of the player while B is
/// held, three type `$82` parts circling it once its seven frames have run
/// (`$7D00` counting them); letting go before that clears the charge, and after it,
/// or A or a direction pressed, throws it as type `$81`.
fn homing_gather(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    let held = cx.effects.attack & attack::CHARGING != 0;
    if cx.effects.projectiles[slot].param & 0x80 == 0 {
        if !held {
            #[cfg(test)]
            super::oracle::seen(0x8712);
            homing_clear_charge(cx);
            return;
        }
    } else if cx
        .world
        .pad
        .intersects(Buttons::A | Buttons::UP_OR_DOWN | Buttons::SIDEWAYS)
        || !held
    {
        homing_launch(slot, work, cx);
        homing_thrown(slot, work, cx);
        return;
    }
    #[cfg(test)]
    super::oracle::seen(0x84B6);
    homing_in_front(slot, work, cx);
    let step = cx.effects.projectiles[slot].vx.to_be_bytes()[0];
    work.pointer = rom_word(cx.prg, 5, 0x8774 + u16::from(step) * 2);
    if !cx.world.frame_odd {
        let step = step.wrapping_add(1);
        let projectile = &mut cx.effects.projectiles[slot];
        projectile.vx = i16::from_be_bytes([step, projectile.vx.to_be_bytes()[1]]);
        if step >= 7 {
            let mut reset = slot;
            if cx.effects.projectiles[slot].param & 0x80 == 0 {
                cx.effects.projectiles[slot].param = 0x80;
                if !homing_spawn_circlers(slot, cx) {
                    reset = 0xFF;
                }
            }
            let at = 0x7D00 + u16::try_from(reset).expect("a byte");
            slots::write(cx.slots, &mut cx.effects.projectiles, at, 0);
        }
    }
    work.y = work.y.wrapping_add(0x0380);
    if project_keep(slot, work, cx) || work.anchor & 0x40 != 0 {
        work.attr = 0;
        draw(work, cx);
        if cx.effects.projectiles[slot].param != 0 {
            homing_draw_circlers(slot, work, cx);
        }
    }
}

/// `$8712`: frees every slot of type `$80` and up.
fn homing_clear_charge(cx: &mut Cx<'_>) {
    for projectile in &mut cx.effects.projectiles {
        if projectile.kind & 0x80 != 0 {
            projectile.kind = 0;
        }
    }
}

/// `$85CF`: three type `$82` parts, in free slots first, then over the plain shots;
/// their slots kept in the charge's `$7F50` (two nibbles) and `$7F60`. False when
/// even that finds too few and every slot is freed (`$8617`, which leaves X = `$FF`).
fn homing_spawn_circlers(source: usize, cx: &mut Cx<'_>) -> bool {
    #[cfg(test)]
    super::oracle::seen(0x85CF);
    let mut parts = [0u8; 3];
    let mut count = 0;
    let mut at = 15i8;
    let mut over_shots = false;
    loop {
        if at < 0 {
            if over_shots {
                for projectile in &mut cx.effects.projectiles {
                    projectile.kind = 0;
                }
                return false;
            }
            over_shots = true;
            at = 15;
            continue;
        }
        let index = usize::try_from(at).expect("non-negative");
        let kind = cx.effects.projectiles[index].kind;
        if (over_shots && kind < 0x80) || kind == 0 {
            let angle = cx.prg.weapons.circle_angles[count];
            let projectile = &mut cx.effects.projectiles[index];
            projectile.kind = homing_stage::CIRCLING;
            projectile.damage = 1;
            projectile.flags = 0xE0;
            projectile.state = 0;
            projectile.param = angle;
            parts[count] = at.cast_unsigned();
            count += 1;
            if count == 3 {
                let projectile = &mut cx.effects.projectiles[source];
                projectile.part = parts[0] << 4 | parts[1];
                projectile.part_timer = parts[2];
                return true;
            }
            over_shots = false;
        }
        at -= 1;
    }
}

/// `$8525`: draws the three parts circling the charge at `slot`, each a step
/// further round its 32 angles (`$87FC`).
fn homing_draw_circlers(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::oracle::seen(0x8525);
    work.pointer = rom_word(cx.prg, 5, 0x887F);
    work.attr = 0;
    let centre = cx.effects.projectiles[slot];
    for part in [
        centre.part >> 4,
        centre.part & 0x0F,
        centre.part_timer & 0x0F,
    ] {
        let part = usize::from(part);
        cx.effects.projectiles[part].flags = 0x60;
        let (dx, dy) =
            cx.prg.weapons.circle_offsets[usize::from(cx.effects.projectiles[part].param & 0x3F)];
        work.x = u16::from_be_bytes([centre.level_x_hi, centre.level_x_lo]).wrapping_add(dx);
        work.y = u16::from_be_bytes([centre.level_y_hi, centre.level_y_lo]).wrapping_add(dy);
        let projectile = &mut cx.effects.projectiles[part];
        projectile.param = projectile.param.wrapping_add(1);
        if projectile.param >= 0x20 {
            projectile.param = 0;
        }
        if project_keep(slot, work, cx) || work.anchor & 0x40 != 0 {
            draw(work, cx);
            let projectile = &mut cx.effects.projectiles[part];
            projectile.screen_x = work.screen_x;
            projectile.screen_y = work.screen_y;
        } else {
            cx.effects.projectiles[part].flags = 0xE0;
        }
    }
}

/// `$864A`: the charge becomes type `$81`, thrown by the pad's direction (none:
/// down when aiming up, else the way he faces) from in front of the player.
fn homing_launch(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::oracle::seen(0x864A);
    cx.effects.attack &= attack::CHARGEABLE;
    let pad = cx.world.pad;
    let direction = if !pad.intersects(Buttons::DPAD) {
        if cx.effects.player.motion == Motion::AimingUp {
            3
        } else {
            u16::from(cx.effects.player.faces_left())
        }
    } else if !pad.intersects(Buttons::SIDEWAYS) {
        #[cfg(test)]
        super::oracle::seen(0x8668);
        if pad.contains(Buttons::UP) { 2 } else { 3 }
    } else {
        u16::from(!pad.contains(Buttons::RIGHT))
    };
    let (vx, vy) = (
        cx.prg.weapons.launch_vx[usize::from(direction)],
        cx.prg.weapons.launch_vy[usize::from(direction)],
    );
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.vx = vx;
    projectile.vy = vy;
    projectile.kind = homing_stage::THROWN;
    homing_in_front(slot, work, cx);
}

/// Type `$81` (`$8515`): the thrown charge flies, its parts circling; off the
/// screen it is freed with every other charged slot.
fn homing_thrown(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::oracle::seen(0x8515);
    advance(slot, work, cx);
    if project_or_free(slot, work, cx) {
        homing_draw_circlers(slot, work, cx);
    } else {
        homing_clear_charge(cx);
    }
}

/// `$86A5`: puts the charge in front of the player (`$9EA7` when aiming up, else
/// the `$8726`-`$8762` offsets by motion), in its slot and the working position.
fn homing_in_front(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    let motion = usize::from(cx.effects.player.motion.byte());
    let (x, y) = if cx.effects.player.motion == Motion::AimingUp {
        above_player(cx)
    } else {
        let dx = if cx.effects.player.faces_left() {
            cx.prg.weapons.front_left_dx[motion]
        } else {
            cx.prg.weapons.front_right_dx[motion]
        };
        beside_player(cx, dx, cx.prg.weapons.front_dy[motion])
    };
    let projectile = &mut cx.effects.projectiles[slot];
    [projectile.level_x_hi, projectile.level_x_lo] = x.to_be_bytes();
    [projectile.level_y_hi, projectile.level_y_lo] = y.to_be_bytes();
    (work.x, work.y) = (x, y);
}

/// `$9EA7`'s spot: 2 px ahead of the player and `$410` up.
fn above_player(cx: &Cx<'_>) -> (u16, u16) {
    #[cfg(test)]
    super::oracle::seen(0x9EA7);
    beside_player(
        cx,
        if cx.effects.player.faces_left() {
            0xFFE0
        } else {
            0x0020
        },
        0xFBF0,
    )
}

/// The in-flight sprite of handlers 0 and 5: the pointer at `sprites[$7CB0 >> 7]`.
fn sprite_by_timer(slot: usize, sprites: [u16; 2]) -> impl FnOnce(&mut Cx<'_>) -> u16 {
    move |cx| {
        let timer = cx.effects.projectiles[slot].timer;
        rom_word(cx.prg, 5, sprites[usize::from(timer >> 7)])
    }
}

/// Handler `$9647` (`$0476` 5, the power-up's weapon): handler 0's flight and wall
/// bounce with its own sprites (`$9771`, `$9779`, the spent `$977B`); types `$80`
/// and up are its super attack. False when the handler returns before the other slots.
fn super_weapon(
    slot: usize,
    work: &mut Work,
    cx: &mut Cx<'_>,
    scroll: &mut Scroll,
    overflow: &mut bool,
) -> bool {
    if cx.effects.projectiles[slot].kind >= 0x80 {
        return super_attack(slot, work, cx, scroll, overflow);
    }
    #[cfg(test)]
    super::oracle::seen(0x9668);
    *overflow = flying_shot(
        slot,
        work,
        cx,
        sprite_by_timer(slot, [0x9771, 0x9779]),
        0x977B,
    );
    true
}

/// Handler 5's types `$80` and up (`$9781`, dispatch `$9D24`), the power-up's
/// super attack; false when the handler returns before the other slots.
fn super_attack(
    slot: usize,
    work: &mut Work,
    cx: &mut Cx<'_>,
    scroll: &mut Scroll,
    overflow: &mut bool,
) -> bool {
    match cx.effects.projectiles[slot].kind {
        super_stage::UNLEASH => super_unleash(slot, cx, scroll, *overflow),
        super_stage::ORBITER => {
            *overflow = false;
            super_orbiter(slot, work, cx, scroll)
        }
        super_stage::PAIRS => {
            super_pairs(slot, cx);
            true
        }
        super_stage::CLAIMED | super_stage::CENTRE => true,
        other => panic!("handler 5's type ${other:02X} is past the $9D24 table"),
    }
}

/// Type `$80` (`$9790`): with the meter at 2 or more, the charge claims every
/// other slot (type `$83`), stops the scroll and the player (`$0481` bit 7) and
/// becomes the type `$82` that releases the orbiting pairs; with less it is
/// dropped and the handler returns.
///
/// Classic's rule: the charge waits while `overflow`, the 6502's overflow flag, is set.
/// The game tests `LDA $B8; BVC`, and `LDA` leaves V as the code before left it
/// (`docs/ACTORS.md` §9g): the last writer among Batman's frame (`$84AB`: set while B is
/// held, while he stands invulnerable, or turning mid-jump or rising from a crouch), the
/// last live second-class actor's projection and draw (which usually clear it), and a shot
/// in a higher slot.
fn super_unleash(slot: usize, cx: &mut Cx<'_>, scroll: &mut Scroll, overflow: bool) -> bool {
    #[cfg(test)]
    super::oracle::overflow_at(0x9790, overflow);
    if overflow {
        return true;
    }
    if cx.effects.meter < 2 {
        #[cfg(test)]
        super::oracle::seen(0x979E);
        cx.effects.attack &= attack::CHARGEABLE;
        cx.effects.projectiles[slot].kind = 0;
        return false;
    }
    #[cfg(test)]
    super::oracle::seen(0x97AC);
    cx.effects.busy = busy::SCRIPTED;
    cx.effects.sound[0] = 0x47;
    cx.effects.player.stage_script |= stage_script::FROZEN;
    scroll.gate = 0;
    scroll.speed_x = 0;
    scroll.speed_y = 0;
    let kept = cx.effects.scroll_lock;
    cx.effects.scroll_lock = 0xFF;
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.kind = super_stage::PAIRS;
    projectile.part = 0;
    projectile.part_timer = 0;
    projectile.timer = kept;
    for projectile in &mut cx.effects.projectiles {
        if projectile.kind & 0x80 == 0 {
            projectile.kind = super_stage::CLAIMED;
            projectile.flags = 0xE0;
        }
    }
    let (x, y) = if cx.effects.player.motion == Motion::AimingUp {
        #[cfg(test)]
        super::oracle::seen(0x97F9);
        above_player(cx)
    } else {
        let motion = usize::from(cx.effects.player.motion.byte());
        beside_player(cx, ahead(cx, motion), cx.prg.weapons.ahead_dy[motion])
    };
    let projectile = &mut cx.effects.projectiles[slot];
    [projectile.level_x_hi, projectile.level_x_lo] = x.to_be_bytes();
    [projectile.level_y_hi, projectile.level_y_lo] = y.to_be_bytes();
    true
}

/// Type `$82` (`$9B4A`): every 17th frame, two claimed slots become a type `$81`
/// orbiter and the type `$84` centre it circles, at the charge's position; after
/// five pairs the charge is claimed itself.
fn super_pairs(slot: usize, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::oracle::seen(0x9B4A);
    let projectile = &mut cx.effects.projectiles[slot];
    if projectile.part != 0x10 {
        projectile.part += 1;
        return;
    }
    projectile.part = 0;
    let claimed = |cx: &Cx<'_>, below: usize| {
        (0..below)
            .rev()
            .find(|&i| cx.effects.projectiles[i].kind == super_stage::CLAIMED)
            .expect("the charge claimed every other slot, enough for five pairs")
    };
    let orbiter = claimed(cx, 16);
    cx.effects.projectiles[orbiter].kind = super_stage::ORBITER;
    let centre = claimed(cx, orbiter);
    cx.effects.projectiles[centre].kind = super_stage::CENTRE;
    let source = cx.effects.projectiles[slot];
    let orbiter_weapon = &mut cx.effects.projectiles[orbiter];
    [
        orbiter_weapon.level_x_hi,
        orbiter_weapon.level_x_lo,
        orbiter_weapon.level_y_hi,
        orbiter_weapon.level_y_lo,
    ] = [1, 0, 0, 0];
    orbiter_weapon.flags = 0xE0;
    orbiter_weapon.param = 0;
    orbiter_weapon.part = if source.part_timer == 0 { 0x80 } else { 0 };
    orbiter_weapon.vx = -8;
    orbiter_weapon.vy = 0x0080;
    orbiter_weapon.timer = source.timer;
    orbiter_weapon.state = source.state;
    orbiter_weapon.part_timer = u8::try_from(centre).expect("slot < 16");
    let centre_weapon = &mut cx.effects.projectiles[centre];
    [centre_weapon.level_x_hi, centre_weapon.level_x_lo] = [source.level_x_hi, source.level_x_lo];
    [centre_weapon.level_y_hi, centre_weapon.level_y_lo] = [source.level_y_hi, source.level_y_lo];
    centre_weapon.vy = 0x0080;
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.part_timer += 1;
    if projectile.part_timer >= 5 {
        projectile.kind = super_stage::CLAIMED;
    }
}

/// Type `$81` (`$9843`): an orbiter. Each frame it strikes every actor on the
/// screen (`$9F4F`), springs about its centre (its position an offset from it,
/// drawn turned 45 degrees and mirrored on odd frames) and, for the first pair,
/// flashes the palette; after `$E0` frames the attack ends and the handler returns.
fn super_orbiter(slot: usize, work: &mut Work, cx: &mut Cx<'_>, scroll: &mut Scroll) -> bool {
    #[cfg(test)]
    super::oracle::seen(0x9843);
    if cx.effects.meter < 2 {
        cx.effects.meter = 2;
    }
    super_smite(cx);
    advance(slot, work, cx);
    let across = work.x.cast_signed() >> 2;
    let down = work.y.cast_signed() >> 2;
    let mut dx = across.wrapping_add(down);
    let dy = down.wrapping_sub(across);
    let mirror = if cx.world.frame_odd {
        dx = dx.wrapping_neg();
        0x40
    } else {
        0
    };
    work.attr = 0;
    cx.effects.palette_dirty = 0x80;
    let centre = usize::from(cx.effects.projectiles[slot].part_timer);
    let centre_weapon = cx.effects.projectiles[centre];
    work.x = u16::from_be_bytes([centre_weapon.level_x_hi, centre_weapon.level_x_lo])
        .wrapping_add(dx.cast_unsigned());
    work.y = u16::from_be_bytes([centre_weapon.level_y_hi, centre_weapon.level_y_lo])
        .wrapping_add(dy.cast_unsigned());
    if project_keep(slot, work, cx) {
        let hi = cx.effects.projectiles[slot].level_x_hi;
        let (table, step) = if hi & 0x80 != 0 {
            (0x9C99, !hi & 0x0F)
        } else {
            (0x9C19, hi & 0x0F)
        };
        work.pointer = rom_word(cx.prg, 5, table + u16::from(step << 1 | mirror));
        draw(work, cx);
    }
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.param = projectile.param.wrapping_add(1);
    if projectile.param.trailing_zeros() >= 5 {
        cx.effects.sound[0] = 0x48;
    }
    if cx.effects.projectiles[slot].param >= 0xE0 {
        super_end_attack(slot, cx, scroll);
        return false;
    }
    super_spring(slot, centre, cx);
    if cx.effects.projectiles[slot].part & 0x80 != 0 {
        super_flash(slot, cx);
    }
    true
}

/// The orbiter's spring (`$99F7`-`$9AAA`): its y velocity pulled `$3C` back
/// toward the centre; when the pull, with the centre's `$7EB0` added, crosses
/// zero, the x velocity turns, the offset snaps to the centre and `$30` more goes
/// on, kept in the centre's y velocity too.
fn super_spring(slot: usize, centre: usize, cx: &mut Cx<'_>) {
    let kept = cx.effects.projectiles[centre].vy;
    let projectile = &mut cx.effects.projectiles[slot];
    let above = projectile.level_y_hi & 0x80 != 0;
    projectile.vy = if above {
        projectile.vy.wrapping_add(0x3C)
    } else {
        projectile.vy.wrapping_sub(0x3C)
    };
    if (kept.wrapping_add(projectile.vy) < 0) == above {
        return;
    }
    if above {
        projectile.vx = (!projectile.vx).wrapping_sub(0x0C);
        [projectile.level_y_hi, projectile.level_y_lo] = [0, 0];
        projectile.vy = projectile.vy.wrapping_add(0x30);
    } else {
        projectile.vx = (!projectile.vx).wrapping_add(0x0C);
        [projectile.level_y_hi, projectile.level_y_lo] = [0xFF, 0xFF];
        projectile.vy = projectile.vy.wrapping_sub(0x30);
    }
    let vy = projectile.vy;
    cx.effects.projectiles[centre].vy = vy;
}

/// `$9AAD`: the first orbiter's flash, the sprite set `$9D19` on odd frames and
/// `$9D1D` on even ones, every palette's first colour `$30` each eighth frame and
/// `$0F` otherwise.
fn super_flash(slot: usize, cx: &mut Cx<'_>) {
    let table = if cx.world.frame_odd {
        cx.prg.weapons.orbiter_palette_odd
    } else {
        cx.prg.weapons.orbiter_palette_even
    };
    cx.effects.palette[16..20].copy_from_slice(&table);
    let first = if cx.effects.projectiles[slot].param.trailing_zeros() >= 3 {
        0x30
    } else {
        0x0F
    };
    set_first_colours(&mut cx.effects.palette, first);
    cx.effects.palette_dirty = 0x80;
}

/// `$9AF7`: the super attack ends: the power-up gone (`$0472` bit 7, the meter
/// `$FE`), the stored weapon back, the scroll and `$C2` restored, the palette's
/// sprite set from `$9D21` and every slot freed.
fn super_end_attack(slot: usize, cx: &mut Cx<'_>, scroll: &mut Scroll) {
    #[cfg(test)]
    super::oracle::seen(0x9AF7);
    let effects = &mut *cx.effects;
    effects.player.power_up &= !power_up::ON;
    effects.player.stage_script = 0;
    effects.meter = 0xFE;
    effects.weapon = effects.stored_weapon;
    effects.attack = if effects.weapon == Weapon::Bolt {
        0
    } else {
        0x80
    };
    effects.palette[17..20].copy_from_slice(&cx.prg.weapons.super_palette);
    effects.scroll_lock = effects.projectiles[slot].timer;
    set_first_colours(&mut effects.palette, 0x0F);
    effects.palette_dirty = 0x80;
    effects.busy = 0;
    for projectile in &mut effects.projectiles {
        projectile.kind = 0;
    }
    scroll.gate = 0xFF;
}

/// `$9F4F`: takes `$C0` hit points from every hittable actor in slots 0-15 on
/// the screen; one it kills drops `$4C` px and starts dying (`$7D70` = `$80`).
fn super_smite(cx: &mut Cx<'_>) {
    let [camera_x, _] = cx.world.camera_x.to_be_bytes();
    let camera_y = cx.world.camera_y;
    for actor in cx.slots.main[..16].iter_mut().rev() {
        let body = &mut actor.body;
        if actor.kind == 0 || body.contact & 0x08 != 0 || body.hit & hit::DYING != 0 {
            continue;
        }
        let dx = body.x.to_be_bytes()[0].wrapping_sub(camera_x);
        let dy = body.y.wrapping_sub(camera_y).to_be_bytes()[0];
        if (dx | dy) & 0xF0 != 0 {
            continue;
        }
        body.health = body.health.wrapping_sub(0xC0);
        if body.health & 0x8000 == 0 {
            continue;
        }
        body.y = body.y.wrapping_add(0x04C0);
        body.hit = hit::DYING;
        body.dying_timer = 0;
    }
}

/// A shot's flight, wall bounce and draw: `flight` picks the sprite in flight
/// (and may change `$7C20`), `spent` is the spent sprites' pointer table. The overflow
/// flag it leaves: gravity's, the projection's or the draw's.
fn flying_shot(
    slot: usize,
    work: &mut Work,
    cx: &mut Cx<'_>,
    flight: impl FnOnce(&mut Cx<'_>) -> u16,
    spent: u16,
) -> bool {
    advance(slot, work, cx);
    let phase = cx.effects.projectiles[slot].state;
    if phase & 0x80 == 0 {
        if phase & 0x40 != 0 {
            let projectile = &mut cx.effects.projectiles[slot];
            let overflow = gravity(projectile);
            projectile.timer = projectile.timer.wrapping_sub(1);
            if projectile.timer & 0x80 != 0 {
                projectile.kind = 0;
                return overflow;
            }
        } else if solid(work, phase << 1, cx) {
            let timer = cx.effects.projectiles[slot].timer;
            if timer & 1 == 0 {
                #[cfg(test)]
                super::oracle::seen(0x80CD);
                cx.effects.projectiles[slot].timer = timer | 1;
                let on_screen = project_or_free(slot, work, cx);
                return projection_overflow(on_screen, work, cx);
            }
            #[cfg(test)]
            super::oracle::seen(0x80DA);
            let positive = cx.world.rng[0] & 0x01 != 0;
            bounce_off_wall(slot, cx, positive);
        }
    }
    let on_screen = project_or_free(slot, work, cx);
    let projected = projection_overflow(on_screen, work, cx);
    if !on_screen {
        return projected;
    }
    let (phase, timer) = {
        let projectile = &cx.effects.projectiles[slot];
        (projectile.state, projectile.timer)
    };
    if phase & 0x80 == 0 {
        work.pointer = flight(cx);
    } else {
        work.pointer = rom_word(cx.prg, 5, spent + u16::from(timer & 0xFE));
        let projectile = &mut cx.effects.projectiles[slot];
        projectile.timer = projectile.timer.wrapping_sub(1);
        if projectile.timer & 0x80 != 0 {
            projectile.kind = 0;
        }
    }
    work.attr = cx.effects.projectiles[slot].attr;
    draw_alternate_frames(slot, work, cx).unwrap_or(projected)
}

/// Handler `$8891` (`$0476` 2): the splitter, which flies on, pushing its speed toward a
/// cruise, and splits at a wall or when spent.
fn splitter(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    if cx.effects.projectiles[slot].kind == splitter_stage::ORBIT {
        splitter_orbit(slot, work, cx);
        return;
    }
    advance(slot, work, cx);
    let phase = cx.effects.projectiles[slot].state;
    if phase & 0x80 == 0 {
        if phase & 0x40 != 0 {
            let projectile = &mut cx.effects.projectiles[slot];
            gravity(projectile);
            projectile.timer = projectile.timer.wrapping_sub(1);
            if projectile.timer & 0x80 != 0 {
                projectile.kind = 0;
                return;
            }
        } else if solid(work, phase << 1, cx) {
            cx.effects.sound[0] = 0x22;
            let timer = cx.effects.projectiles[slot].timer;
            if timer & 1 != 0 {
                splitter_split(slot, cx);
                return;
            }
            cx.effects.projectiles[slot].timer = timer | 1;
            project_or_free(slot, work, cx);
            return;
        }
    }
    if !project_or_free(slot, work, cx) {
        return;
    }
    let (attr, phase, timer, bounce) = {
        let projectile = &cx.effects.projectiles[slot];
        (
            projectile.attr,
            projectile.state,
            projectile.timer,
            projectile.param,
        )
    };
    work.attr = attr;
    if phase & 0x80 != 0 {
        if bounce & 0x80 == 0 {
            splitter_split(slot, cx);
            return;
        }
        work.pointer = rom_word(cx.prg, 5, 0x8A4B + u16::from(timer < 8) * 2);
        let projectile = &mut cx.effects.projectiles[slot];
        projectile.timer = projectile.timer.wrapping_add(1);
        if projectile.timer >= 0x20 {
            projectile.kind = 0;
            return;
        }
    } else {
        let kind = cx.effects.projectiles[slot].kind;
        if timer & 0x80 != 0 {
            if phase & 0x40 == 0 {
                let projectile = &mut cx.effects.projectiles[slot];
                let [hi, lo] = projectile.vy.to_be_bytes();
                let (sub, borrow) = lo.overflowing_sub(2);
                let hi = if borrow { hi.wrapping_sub(1) } else { hi };
                projectile.vy = i16::from_be_bytes([hi, if sub >= 0xD0 { sub } else { 0xC0 }]);
            }
            work.pointer = rom_word(cx.prg, 5, 0x8A43 + u16::from(kind & 0x80 != 0) * 2);
        } else {
            if phase & 0x40 == 0 {
                let projectile = &mut cx.effects.projectiles[slot];
                let [hi, lo] = projectile.vx.to_be_bytes();
                if attr & attr::FLIP_X != 0 {
                    let (sub, borrow) = lo.overflowing_sub(2);
                    let hi = if borrow { hi.wrapping_sub(1) } else { hi };
                    projectile.vx = i16::from_be_bytes([hi, if sub >= 0xD0 { sub } else { 0xC0 }]);
                } else {
                    let (add, carry) = lo.overflowing_add(2);
                    let hi = if carry { hi.wrapping_add(1) } else { hi };
                    projectile.vx = i16::from_be_bytes([hi, if add < 0x30 { add } else { 0x40 }]);
                }
            }
            work.pointer = rom_word(cx.prg, 5, 0x8A47 + u16::from(kind & 0x80 != 0) * 2);
        }
    }
    draw_alternate_frames(slot, work, cx);
}

/// Splits the shot into three parts, then one more (`$89AC`).
fn splitter_split(source: usize, cx: &mut Cx<'_>) {
    let mut part = 0usize;
    let mut slot = 0usize;
    while slot < 16 {
        if cx.effects.projectiles[slot].kind == 0 {
            splitter_part(slot, part, source, cx);
            part += 1;
            if part >= 3 {
                break;
            }
        }
        slot += 1;
    }
    splitter_part(source, part, source, cx);
}

/// Writes one split part at `source`'s position (`$89D7`).
fn splitter_part(slot: usize, part: usize, source: usize, cx: &mut Cx<'_>) {
    let src = cx.effects.projectiles[source];
    let tables = &cx.prg.weapons;
    let (vx, vy, bounce) = (
        tables.split_vx[part],
        tables.split_vy[part],
        tables.split_bounce[part],
    );
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.vx = vx;
    projectile.vy = vy;
    projectile.param = bounce;
    projectile.kind = 0x06;
    projectile.state = 0x80;
    projectile.attr = 0;
    projectile.timer = 0;
    projectile.level_x_hi = src.level_x_hi;
    projectile.level_x_lo = src.level_x_lo;
    projectile.level_y_hi = src.level_y_hi;
    projectile.level_y_lo = src.level_y_lo;
    projectile.flags = 0x60;
    projectile.damage = 0x02;
}

/// Handler `$8D39` (`$0476` 3): the wave, two parts that weave. False when the handler
/// returns before the other slots (`$8FD6`).
fn wave(slot: usize, work: &mut Work, cx: &mut Cx<'_>) -> bool {
    match cx.effects.projectiles[slot].kind {
        wave_stage::CHARGE => return wave_charge(slot, work, cx),
        wave_stage::SPREAD => return wave_spread(slot, cx),
        wave_stage::SPREAD_SHOT.. => {
            wave_spread_shot(slot, work, cx);
            return true;
        }
        _ => {}
    }
    advance(slot, work, cx);
    let phase = cx.effects.projectiles[slot].state;
    if phase & 0x80 == 0 {
        if phase & 0x40 != 0 {
            let projectile = &mut cx.effects.projectiles[slot];
            gravity(projectile);
            projectile.timer = projectile.timer.wrapping_sub(1);
            if projectile.timer & 0x80 != 0 {
                projectile.kind = 0;
                return true;
            }
        } else if solid(work, phase << 1, cx) {
            let timer = cx.effects.projectiles[slot].timer;
            if timer & 1 == 0 {
                cx.effects.projectiles[slot].timer = timer | 1;
                project_or_free(slot, work, cx);
                return true;
            }
            let positive = cx.world.rng[0] & 0x80 == 0;
            bounce_off_wall(slot, cx, positive);
        }
    }
    wave_flight(slot, work, cx);
    true
}

/// Handler `$91B2` (`$0476` 4): the three-way shot, drawn with `$99 = 0`; types
/// `$80` and up are its charged attack (`$92C4`).
fn three_way(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    match cx.effects.projectiles[slot].kind {
        three_way_stage::BREW => return three_way_brew(slot, cx),
        three_way_stage::BUBBLE => return three_way_bubble(slot, work, cx),
        three_way_stage::TETHER => return three_way_tether(slot, work, cx),
        other @ 0x83.. => panic!("handler 4's type ${other:02X} is past the $95EF table"),
        _ => {}
    }
    advance(slot, work, cx);
    let phase = cx.effects.projectiles[slot].state;
    if phase & 0x80 == 0 {
        if phase & 0x40 != 0 {
            let projectile = &mut cx.effects.projectiles[slot];
            gravity(projectile);
            projectile.timer = projectile.timer.wrapping_sub(1);
            if projectile.timer & 0x80 != 0 {
                projectile.kind = 0;
                return;
            }
        } else if solid(work, phase << 1, cx) {
            let timer = cx.effects.projectiles[slot].timer;
            if timer & 1 == 0 {
                cx.effects.projectiles[slot].timer = timer | 1;
                project_or_free(slot, work, cx);
                return;
            }
            if timer & 0x80 != 0 {
                #[cfg(test)]
                super::oracle::seen(0x9268);
                let pick = usize::from(cx.world.rng[0] & 0x02 | 0x01);
                let (vx, vy) = (
                    cx.prg.weapons.three_way_vx[pick],
                    cx.prg.weapons.three_way_vy[pick],
                );
                let projectile = &mut cx.effects.projectiles[slot];
                projectile.vx = vx;
                projectile.vy = vy;
                projectile.state = 0x80;
                projectile.attr = 0;
                projectile.timer = 0x04;
                projectile.flags = 0xE0;
            } else {
                #[cfg(test)]
                super::oracle::seen(0x9275);
                let positive = cx.world.rng[0] & 0x80 == 0;
                bounce_off_wall(slot, cx, positive);
            }
        }
    }
    if !project_or_free(slot, work, cx) {
        return;
    }
    let projectile = &mut cx.effects.projectiles[slot];
    if projectile.state & 0x80 == 0 {
        work.pointer = rom_word(cx.prg, 5, 0x92BA + u16::from((cx.world.frame & 0x04) >> 1));
    } else {
        work.pointer = rom_word(cx.prg, 5, 0x92BE + u16::from(projectile.timer & 0xFE));
        projectile.timer = projectile.timer.wrapping_sub(1);
        if projectile.timer & 0x80 != 0 {
            projectile.kind = 0;
        }
    }
    draw_alternate_frames(slot, work, cx);
}

/// Type `$80` (`$92D3`): while B is held, a type `$81` bubble from the player
/// every eighth frame (effect `$3F`); let go, it becomes the type `$82` tether
/// (effect `$2A`).
fn three_way_brew(slot: usize, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::oracle::seen(0x92D3);
    if cx.effects.attack & attack::CHARGING == 0 {
        cx.effects.sound[1] = 0x2A;
        three_way_let_go(slot, cx);
        return;
    }
    let projectile = &mut cx.effects.projectiles[slot];
    let count = projectile.part_timer;
    projectile.part_timer = count.wrapping_add(1);
    if count & 0x07 != 0 {
        return;
    }
    let Some(free) = free_slot(cx) else {
        return;
    };
    cx.effects.sound[0] = 0x3F;
    let (x, y) = if cx.effects.player.motion == Motion::AimingUp {
        above_player(cx)
    } else {
        let motion = usize::from(cx.effects.player.motion.byte());
        beside_player(cx, ahead(cx, motion), cx.prg.weapons.ahead_dy[motion])
    };
    let [random, sign] = cx.world.rng;
    let drift = random >> 4;
    let vx = if sign & 0x80 != 0 {
        i16::from_be_bytes([0xFF, drift | 0xFC])
    } else {
        i16::from_be_bytes([0, drift & 0x03])
    };
    let projectile = &mut cx.effects.projectiles[free];
    [projectile.level_x_hi, projectile.level_x_lo] = x.to_be_bytes();
    [projectile.level_y_hi, projectile.level_y_lo] = y.to_be_bytes();
    three_way_blow(free, vx, cx);
}

/// The `$9F07`-`$9F2B` x offset ahead of the player for `motion`.
fn ahead(cx: &Cx<'_>, motion: usize) -> u16 {
    if cx.effects.player.faces_left() {
        cx.prg.weapons.ahead_left_dx[motion]
    } else {
        cx.prg.weapons.ahead_right_dx[motion]
    }
}

/// `$9489`/`$959F`'s shared part: `slot` becomes a type `$81` bubble with x
/// velocity `vx`, rising at `$FF:($F9 | $E0)`, its palette `$9637` by `$F1` bit 3.
fn three_way_blow(slot: usize, vx: i16, cx: &mut Cx<'_>) {
    let attr = cx.prg.weapons.bubble_palette[usize::from(cx.world.frame & 0x08 != 0)];
    let rise = cx.world.rng[0] | 0xE0;
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.kind = three_way_stage::BUBBLE;
    projectile.state = 0;
    projectile.part_timer = 0;
    projectile.flags = 0x20;
    projectile.vx = vx;
    projectile.vy = i16::from_be_bytes([0xFF, rise]);
    projectile.attr = attr;
}

/// `$9521`: B let go, the charge becomes the type `$82` tether, its position an
/// offset from the player: ahead of him (velocity `$9ED6[4/5]`) or, aiming up,
/// `$FD70` above him (`$9ED6[3]`, `$7F60` bit 7 marking it vertical).
fn three_way_let_go(slot: usize, cx: &mut Cx<'_>) {
    let motion = usize::from(cx.effects.player.motion.byte());
    let player = &cx.effects.player;
    let (aim_up, left) = (player.motion == Motion::AimingUp, player.faces_left());
    let (x, y, pick) = if aim_up {
        (0, 0xFD70, 3)
    } else {
        let y = cx.prg.weapons.ahead_dy[motion].wrapping_add(0x0180);
        (ahead(cx, motion), y, if left { 4 } else { 5 })
    };
    let (vx, vy) = (
        cx.prg.weapons.three_way_vx[pick],
        cx.prg.weapons.three_way_vy[pick],
    );
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.kind = three_way_stage::TETHER;
    projectile.damage = 1;
    projectile.state = 0;
    projectile.param = 0;
    projectile.part = 0;
    projectile.flags = 0x60;
    projectile.part_timer = if aim_up { 0x80 } else { 0 };
    projectile.vx = vx;
    projectile.vy = vy;
    [projectile.level_x_hi, projectile.level_x_lo] = x.to_be_bytes();
    [projectile.level_y_hi, projectile.level_y_lo] = y.to_be_bytes();
}

/// Type `$81` (`$930B`): a bubble rising for 48 frames, slowing by 1 a frame.
fn three_way_bubble(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::oracle::seen(0x930B);
    let projectile = &mut cx.effects.projectiles[slot];
    if projectile.state & 0x80 != 0 {
        projectile.kind = 0;
        return;
    }
    projectile.part_timer = projectile.part_timer.wrapping_add(1);
    if projectile.part_timer >= 0x30 {
        projectile.kind = 0;
        return;
    }
    work.anchor = 0;
    work.tile = 0;
    work.attr = projectile.attr;
    work.chr_slot = 4;
    advance(slot, work, cx);
    if !project_or_free(slot, work, cx) {
        return;
    }
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.vy = projectile.vy.wrapping_add(1);
    work.pointer = rom_word(cx.prg, 5, 0x9635);
    draw(work, cx);
}

/// Type `$82` (`$935E`): for `$C0` frames the tether springs about the player (its
/// position an offset from him, its velocity pulled back 4 a frame), a bubble
/// leaving it every eighth frame.
fn three_way_tether(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::oracle::seen(0x935E);
    work.anchor = 0;
    work.attr = 0;
    work.tile = 0;
    work.chr_slot = 4;
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.part = projectile.part.wrapping_add(1);
    if projectile.part >= 0xC0 {
        projectile.kind = 0;
        return;
    }
    if projectile.part_timer & 0x80 != 0 {
        #[cfg(test)]
        super::oracle::seen(0x93BA);
        projectile.vy =
            three_way_pull(projectile.vy, projectile.level_y_hi & 0x80 != 0, 0x50, 0xB0);
    } else {
        projectile.vx =
            three_way_pull(projectile.vx, projectile.level_x_hi & 0x80 != 0, 0x40, 0xC0);
    }
    let x = u16::from_be_bytes([projectile.level_x_hi, projectile.level_x_lo])
        .wrapping_add(projectile.vx.cast_unsigned());
    let y = u16::from_be_bytes([projectile.level_y_hi, projectile.level_y_lo])
        .wrapping_add(projectile.vy.cast_unsigned());
    [projectile.level_x_hi, projectile.level_x_lo] = x.to_be_bytes();
    [projectile.level_y_hi, projectile.level_y_lo] = y.to_be_bytes();
    let count = projectile.part;
    (work.x, work.y) = beside_player(cx, x, y.wrapping_sub(0x0180));
    if count.trailing_zeros() >= 3
        && let Some(free) = free_slot(cx)
    {
        cx.effects.sound[0] = 0x3F;
        let projectile = &mut cx.effects.projectiles[free];
        [projectile.level_x_hi, projectile.level_x_lo] = work.x.to_be_bytes();
        [projectile.level_y_hi, projectile.level_y_lo] = work.y.to_be_bytes();
        let vx = cx.effects.projectiles[slot].vx;
        three_way_blow(free, vx, cx);
    }
    if project_keep(slot, work, cx) {
        let frame = (cx.effects.projectiles[slot].part >> 1) & 0x06;
        work.pointer = rom_word(cx.prg, 5, 0x963F + u16::from(frame));
        draw(work, cx);
    }
}

/// One axis of the tether's spring: the low byte up 4 when the offset is negative
/// (`behind`) and down 4 otherwise, held to `toward`/`away` in `$xx-$7F`/`$80-$xx`.
fn three_way_pull(velocity: i16, behind: bool, toward: u8, away: u8) -> i16 {
    let [hi, lo] = velocity.to_be_bytes();
    if behind {
        let (lo, carry) = lo.overflowing_add(4);
        let hi = hi.wrapping_add(u8::from(carry));
        let lo = if (toward..0x80).contains(&lo) {
            toward
        } else {
            lo
        };
        i16::from_be_bytes([hi, lo])
    } else {
        let (lo, borrow) = lo.overflowing_sub(4);
        let hi = hi.wrapping_sub(u8::from(borrow));
        let lo = if (0x80..away).contains(&lo) { away } else { lo };
        i16::from_be_bytes([hi, lo])
    }
}

/// The highest free weapon slot.
fn free_slot(cx: &Cx<'_>) -> Option<usize> {
    (0..16).rev().find(|&i| cx.effects.projectiles[i].kind == 0)
}

/// The big-endian word from bank 5 bytes `hi` and `lo`.
fn word(cx: &Cx<'_>, hi: u16, lo: u16) -> u16 {
    u16::from_be_bytes([rom_byte(cx.prg, 5, hi), rom_byte(cx.prg, 5, lo)])
}

/// The player's level position plus `(dx, dy)`.
fn beside_player(cx: &Cx<'_>, dx: u16, dy: u16) -> (u16, u16) {
    let player = &cx.effects.player;
    (player.x.wrapping_add(dx), player.y.wrapping_add(dy))
}

/// Handler 2's charged attack, type `$80` (`$8A4F`): four parts circle the player
/// while B is held, then fly off as type `$81` parts.
fn splitter_orbit(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::oracle::seen(0x8A4F);
    let projectile = cx.effects.projectiles[slot];
    let turn = u16::from(projectile.part) + if projectile.param & 0x80 != 0 { 4 } else { 0 };
    let vx = projectile
        .vx
        .cast_unsigned()
        .wrapping_add(word(cx, 0x8C79 + turn, 0x8C71 + turn));
    let vy = projectile
        .vy
        .cast_unsigned()
        .wrapping_add(word(cx, 0x8C89 + turn, 0x8C81 + turn));
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.vx = vx.cast_signed();
    projectile.vy = vy.cast_signed();
    projectile.part_timer = projectile.part_timer.wrapping_add(1);
    if projectile.part_timer >= 0x40 {
        projectile.param ^= 0x80;
        projectile.part_timer = 0;
        cx.effects.sound[1] = 0x30;
    }
    let [hi, lo] = vx.to_be_bytes();
    let (back_lo, carry) = (!lo).overflowing_add(1);
    let (back_hi, round) = (!hi).overflowing_add(u8::from(carry));
    let back = u16::from_be_bytes([back_hi, back_lo]);
    let motion = usize::from(cx.effects.player.motion.byte());
    let left = cx.effects.player.faces_left();
    (work.x, work.y) = if cx.effects.player.motion == Motion::AimingUp {
        #[cfg(test)]
        super::oracle::seen(0x8AFF);
        let dx = if left { 0xFFE0 } else { 0x0020 };
        let (x, y) = beside_player(cx, dx, 0xFBF0);
        (x.wrapping_add(vy), y.wrapping_add(back))
    } else if left {
        let (x, y) = beside_player(
            cx,
            cx.prg.weapons.split_left_dx[motion],
            cx.prg.weapons.split_dy[motion],
        );
        let x = x.wrapping_add(back).wrapping_add(u16::from(round));
        (x, y.wrapping_add(vy))
    } else {
        let (x, y) = beside_player(
            cx,
            cx.prg.weapons.split_right_dx[motion],
            cx.prg.weapons.split_dy[motion],
        );
        (x.wrapping_add(vx), y.wrapping_add(vy))
    };
    if project_keep(slot, work, cx) {
        work.anchor = 0;
        work.attr = 0;
        work.pointer = rom_word(cx.prg, 5, 0x8C6D + u16::from((cx.world.frame & 4) >> 1));
        draw(work, cx);
    }
    if cx.effects.attack & attack::CHARGING != 0 {
        return;
    }
    #[cfg(test)]
    super::oracle::seen(0x8BCB);
    cx.effects.sound[0] = 0x2F;
    let player = &cx.effects.player;
    let (aim_up, left) = (player.motion == Motion::AimingUp, player.faces_left());
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.kind = splitter_stage::THROWN;
    [projectile.level_x_hi, projectile.level_x_lo] = work.x.to_be_bytes();
    [projectile.level_y_hi, projectile.level_y_lo] = work.y.to_be_bytes();
    projectile.flags = 0;
    projectile.attr = 0;
    projectile.damage = 2;
    projectile.state = 0;
    let pose = if aim_up {
        #[cfg(test)]
        super::oracle::seen(0x8C18);
        projectile.timer = 0x80;
        4
    } else {
        projectile.timer = 0;
        if left {
            projectile.attr = attr::FLIP_X;
            2
        } else {
            0
        }
    };
    let tables = if projectile.param & 0x80 != 0 {
        0x8C67
    } else {
        0x8C61
    };
    let (table, part) = (
        rom_word(cx.prg, 5, tables + pose),
        u16::from(projectile.part),
    );
    let vx = word(cx, table + part + 4, table + part);
    let vy = word(cx, table + part + 12, table + part + 8);
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.vx = vx.cast_signed();
    projectile.vy = vy.cast_signed();
}

/// Handler 3's charged attack, type `$80` (`$8F03`): it gathers in front of the
/// player for eight frames while B is held, then becomes the `$81` spread.
fn wave_charge(slot: usize, work: &mut Work, cx: &mut Cx<'_>) -> bool {
    #[cfg(test)]
    super::oracle::seen(0x8F03);
    if cx.effects.attack & attack::CHARGING == 0 {
        cx.effects.projectiles[slot].kind = 0;
        return true;
    }
    let motion = usize::from(cx.effects.player.motion.byte());
    (work.x, work.y) = if cx.effects.player.motion == Motion::AimingUp {
        #[cfg(test)]
        super::oracle::seen(0x8F16);
        let dx = if cx.effects.player.faces_left() {
            0xFFE0
        } else {
            0x0020
        };
        beside_player(cx, dx, 0xFDF0)
    } else {
        let dx = if cx.effects.player.faces_left() {
            cx.prg.weapons.wave_left_dx[motion]
        } else {
            cx.prg.weapons.wave_right_dx[motion]
        };
        beside_player(cx, dx, cx.prg.weapons.wave_charge_dy[motion])
    };
    if project_keep(slot, work, cx) && !cx.world.frame_odd {
        let frame = cx.effects.projectiles[slot].part_timer & 0x02;
        work.pointer = rom_word(cx.prg, 5, 0x9142 + u16::from(frame));
        draw(work, cx);
    }
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.part_timer = projectile.part_timer.wrapping_add(1);
    if projectile.part_timer != 8 {
        return true;
    }
    projectile.kind = wave_stage::SPREAD;
    projectile.part_timer = 0;
    projectile.part = 0;
    wave_spread(slot, cx)
}

/// Type `$81` (`$8FC2`): while B is held, a `$82` shot every fourth frame for 64
/// frames, its angle stepping through `$9192`; then every shot is cleared. With no
/// free slot the game's `INC $7F60,X` (X = `$FF`) writes only the FME-7 command
/// latch `$805F`, which every parameter write sets again first.
fn wave_spread(slot: usize, cx: &mut Cx<'_>) -> bool {
    #[cfg(test)]
    super::oracle::seen(0x8FC2);
    if cx.effects.attack & attack::CHARGING != 0 {
        cx.effects.sound[0] = 0x2C;
    }
    let count = cx.effects.projectiles[slot].part_timer;
    if cx.effects.attack & attack::CHARGING == 0 || count >= 0x40 {
        for projectile in &mut cx.effects.projectiles {
            projectile.kind = 0;
        }
        cx.effects.attack &= attack::CHARGEABLE;
        return false;
    }
    cx.effects.projectiles[slot].part_timer = count.wrapping_add(1);
    if count & 3 != 0 {
        return true;
    }
    let Some(free) = free_slot(cx) else {
        #[cfg(test)]
        super::oracle::seen(0x8FE8);
        return true;
    };
    let source = cx.effects.projectiles[slot];
    let angle = usize::from(source.part);
    let along = cx.prg.weapons.spread_vx[angle];
    let motion = usize::from(cx.effects.player.motion.byte());
    let left = cx.effects.player.faces_left();
    let (x, y) = if cx.effects.player.motion == Motion::AimingUp {
        #[cfg(test)]
        super::oracle::seen(0x9023);
        beside_player(cx, if left { 0xFFE0 } else { 0x0020 }, 0xFBF0)
    } else {
        let dx = if left {
            cx.prg.weapons.wave_left_dx[motion]
        } else {
            cx.prg.weapons.wave_right_dx[motion]
        };
        beside_player(cx, dx, cx.prg.weapons.wave_spread_dy[motion])
    };
    let projectile = &mut cx.effects.projectiles[free];
    projectile.part = (source.part_timer & 0x04) >> 1;
    projectile.kind = wave_stage::SPREAD_SHOT;
    projectile.part_timer = 0;
    projectile.damage = 1;
    projectile.flags = 0xE0;
    [projectile.level_x_hi, projectile.level_x_lo] = x.to_be_bytes();
    [projectile.level_y_hi, projectile.level_y_lo] = y.to_be_bytes();
    if cx.effects.player.motion == Motion::AimingUp {
        projectile.vx = along;
        projectile.vy = -0x50;
        projectile.timer = 0x80;
        projectile.attr = 0;
    } else {
        projectile.attr = cx.effects.player.facing & attr::FLIP_X;
        projectile.vx = if left { -0x50 } else { 0x50 };
        projectile.vy = along;
        projectile.timer = 0;
    }
    let next = source.part.wrapping_add(1);
    cx.effects.projectiles[slot].part = if next < 0x10 { next } else { 0 };
    true
}

/// One spread shot, type `$82` and up (`$90E1`): 24 frames of flight.
fn wave_spread_shot(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::oracle::seen(0x90E1);
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.flags = 0x60;
    projectile.part_timer = projectile.part_timer.wrapping_add(1);
    if projectile.part_timer >= 0x18 {
        projectile.kind = 0;
        return;
    }
    work.attr = projectile.attr;
    work.anchor = 0;
    work.tile = 0;
    work.chr_slot = 4;
    advance(slot, work, cx);
    if !project_or_free(slot, work, cx) || cx.world.frame_odd {
        return;
    }
    let projectile = &cx.effects.projectiles[slot];
    let table = if projectile.timer & 0x80 != 0 {
        0x913A
    } else {
        0x913E
    };
    work.pointer = rom_word(cx.prg, 5, table + u16::from(projectile.part));
    draw(work, cx);
}

/// A shot bounces off a wall and resets its velocity (`$80DA`, `$8E7A`): a falling
/// one to a random side, a level one back, both with a random vertical sign;
/// `positive` is the random pick for `+$20`.
fn bounce_off_wall(slot: usize, cx: &mut Cx<'_>, positive: bool) {
    let random = if positive { 0x0020 } else { -0x0020 };
    let projectile = &mut cx.effects.projectiles[slot];
    if projectile.timer & 0x80 != 0 {
        projectile.vy = 0x0020;
        projectile.vx = random;
    } else {
        projectile.vx = if projectile.vx < 0 { 0x0020 } else { -0x0020 };
        projectile.vy = random;
    }
    projectile.state = 0x80;
    projectile.attr = 0;
    projectile.timer = 0x04;
    projectile.flags = 0xE0;
}

/// Projects, weaves and draws the wave's part (`$8D7E`-`$8E62`).
fn wave_flight(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    if !project_or_free(slot, work, cx) {
        return;
    }
    let (phase, timer) = {
        let projectile = &cx.effects.projectiles[slot];
        (projectile.state, projectile.timer)
    };
    if phase & 0x80 != 0 {
        if cx.world.frame_odd {
            let projectile = &mut cx.effects.projectiles[slot];
            projectile.timer = projectile.timer.wrapping_sub(1);
            if projectile.timer & 0x80 != 0 {
                projectile.kind = 0;
            }
            return;
        }
        work.pointer = rom_word(cx.prg, 5, 0x8EEF + u16::from(timer & 0xFE));
        let projectile = &mut cx.effects.projectiles[slot];
        projectile.timer = projectile.timer.wrapping_sub(1);
        if projectile.timer & 0x80 != 0 {
            projectile.kind = 0;
            return;
        }
        draw(work, cx);
        return;
    }
    if timer & 0x80 != 0 {
        work.attr = 0;
        work.pointer = rom_word(cx.prg, 5, 0x8EEB + u16::from(cx.world.frame & 2));
        if weave(slot, cx, true) {
            draw_on_odd_frames(work, cx);
            return;
        }
    }
    work.attr = cx.effects.projectiles[slot].attr;
    work.pointer = rom_word(cx.prg, 5, 0x8EE7 + u16::from(cx.world.frame & 2));
    weave(slot, cx, false);
    draw_on_odd_frames(work, cx);
}

/// One step of the weave (`$8DD8`-`$8E58`), the velocity's low byte swung between `$40`
/// and `$C0` by `$7F40`'s direction: `horizontal` picks x (`$7D00`/`$7D10`)
/// over y (`$7D20`/`$7EB0`). True when the caller draws without the other axis.
/// The `BMI`/`BEQ` after each `STA $7F40,X` tests the `CMP` before it, so a step
/// down always ends there and a step up across goes on to y unless it reached `$40`.
fn weave(slot: usize, cx: &mut Cx<'_>, horizontal: bool) -> bool {
    if cx.effects.projectiles[slot].param & 0x80 == 0 {
        let projectile = &mut cx.effects.projectiles[slot];
        let (hi, lo) = velocity(projectile, horizontal);
        let (sub, borrow) = lo.overflowing_sub(4);
        let hi = if borrow { hi.wrapping_sub(1) } else { hi };
        set_velocity(projectile, horizontal, hi, sub);
        if (0x40..0xC0).contains(&sub) {
            #[cfg(test)]
            super::oracle::seen(if horizontal { 0x8DEE } else { 0x8E3D });
            projectile.param = sub;
        }
        return true;
    }
    let projectile = &mut cx.effects.projectiles[slot];
    let (hi, lo) = velocity(projectile, horizontal);
    let (add, carry) = lo.overflowing_add(4);
    let hi = if carry { hi.wrapping_add(1) } else { hi };
    set_velocity(projectile, horizontal, hi, add);
    if !(0x40..0xC0).contains(&add) {
        return true;
    }
    projectile.param = add;
    !horizontal || add == 0x40
}

/// The high and low bytes of the velocity `edge` adjusts.
fn velocity(projectile: &Projectile, horizontal: bool) -> (u8, u8) {
    let [hi, lo] = if horizontal {
        projectile.vx.to_be_bytes()
    } else {
        projectile.vy.to_be_bytes()
    };
    (hi, lo)
}

/// Writes the two velocity bytes `edge` adjusted.
fn set_velocity(projectile: &mut Projectile, horizontal: bool, hi: u8, lo: u8) {
    let value = i16::from_be_bytes([hi, lo]);
    if horizontal {
        projectile.vx = value;
    } else {
        projectile.vy = value;
    }
}

/// Draws only on the odd frames the wave's flight draws (`$8E5B`).
fn draw_on_odd_frames(work: &mut Work, cx: &mut Cx<'_>) {
    if cx.world.frame_odd {
        draw(work, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::super::oracle::{Scene, assert_game, assert_routine, was_seen};
    use super::super::world::Cx;

    /// Stage 1-1 with the power-up's weapon and its charge (type `$80`) in slot 14, the
    /// meter at 2, after one weapons call with `overflow` coming in and, if `shot_above`, a
    /// shot flying off the screen in slot 15; the charge's type after it.
    fn charge_after(overflow: bool, shot_above: bool) -> Option<u8> {
        let rom = rotj_rom::for_test()?;
        let prg = crate::prg::for_test()?;
        let mut game = super::super::room::start(prg, &rom.chr, 0);
        game.effects.weapon = super::Weapon::Super;
        game.effects.meter = 2;
        game.effects.projectiles[14].kind = super::super_stage::UNLEASH;
        if shot_above {
            let shot = &mut game.effects.projectiles[15];
            shot.kind = 1;
            shot.state = 0x40;
            shot.timer = 0x10;
            shot.level_x_hi = game.world.camera_x.to_be_bytes()[0].wrapping_add(0x20);
        }
        let (mut cx, work) = game.parts(prg);
        super::step(work, &mut cx, overflow);
        Some(game.effects.projectiles[14].kind)
    }

    /// Classic's rule: the charge waits while the overflow flag coming in is set, unless a
    /// shot in a higher slot leaves it clear (an off-screen projection clears it).
    #[test]
    fn the_charge_waits_on_the_overflow_flag_the_slots_above_leave() {
        let Some(waiting) = charge_after(true, false) else {
            return;
        };
        assert_eq!(waiting, super::super_stage::UNLEASH);
        assert_eq!(charge_after(false, false), Some(super::super_stage::PAIRS));
        assert_eq!(charge_after(true, true), Some(super::super_stage::PAIRS));
    }

    /// Diffs the port against every `$D444` call of `scene`; returns `(calls, live)`.
    fn diff(scene: Option<Scene>, frames: u64) -> (usize, usize) {
        let Some(mut scene) = scene else {
            return (0, 0);
        };
        let mut live = 0;
        let checked = assert_routine(&mut scene, frames, (0xD444, 14), &[0xD447], |r| {
            let prg = r.prg;
            live += usize::from(
                r.state
                    .effects
                    .projectiles
                    .iter()
                    .any(|projectile| projectile.kind != 0),
            );
            let mut cx = Cx {
                prg,
                world: &r.state.world,
                slots: &mut r.state.slots,
                schedule: &mut r.state.schedule,
                effects: &mut r.state.effects,
                scroll: &mut r.state.scroll,
                oam: &mut r.state.oam,
            };
            super::step(&mut r.state.work, &mut cx, r.begin.cpu.p & 0x40 != 0);
        });
        (checked, live)
    }

    #[test]
    fn matches_the_oracle_on_the_tas() {
        if rotj_rom::for_test().is_none() {
            return;
        }
        let (checked, live) = diff(Scene::tas(351), 1650);
        assert!(checked >= 1000, "{checked} calls compared");
        assert!(live >= 100, "{live} frames with a live projectile");
    }

    /// TAS 370 with `$0476` poked to `weapon` and one shot of type `kind` per
    /// timer (`$7CB0`) placed in the ground under the player, moving right;
    /// returns the `$D444` calls compared over 40 frames.
    fn grounded(weapon: u8, kind: u8, timers: &[u8]) -> usize {
        let Some(mut scene) = Scene::tas(370) else {
            return 0;
        };
        scene.machine.bus.ram[0x476] = weapon;
        let ram = &scene.machine.bus.ram;
        let (x, y) = (
            u16::from_be_bytes([ram[0x460], ram[0x461]]),
            u16::from_be_bytes([ram[0x462], ram[0x463]]).wrapping_add(0x0200),
        );
        for (slot, &timer) in timers.iter().enumerate() {
            let shot: [(u16, u8); 11] = [
                (0x7BD0, kind),
                (0x7BB0, x.to_be_bytes()[0]),
                (0x7BC0, x.to_be_bytes()[1]),
                (0x7C60, y.to_be_bytes()[0]),
                (0x7C70, y.to_be_bytes()[1]),
                (0x7D00, 0x00),
                (0x7D10, 0x40),
                (0x7D20, 0x00),
                (0x7EB0, 0x00),
                (0x7C80, 0x00),
                (0x7CB0, timer),
            ];
            for (array, value) in shot {
                scene.machine.bus.wram[usize::from(array - 0x6000) + 8 + slot] = value;
            }
        }
        let (checked, live) = diff(Some(scene), 40);
        assert!(live >= 3, "{live} frames with a live weapon");
        checked
    }

    /// `$80C5`: a weapon 0 shot in a wall marks the hit, then bounces out the
    /// next frame.
    #[test]
    fn a_shot_bounces_off_a_wall() {
        if grounded(0, 0x02, &[0]) == 0 {
            return;
        }
        assert!(was_seen(0x80CD), "the shot did not reach the wall");
        assert!(was_seen(0x80DA), "the shot did not bounce");
    }

    /// `$924E`: weapon 4's wall bounce, for a level shot and a falling one (`$9ED6`).
    #[test]
    fn a_weapon_4_shot_bounces_off_a_wall() {
        if grounded(4, 0x0A, &[0x00, 0x80]) == 0 {
            return;
        }
        assert!(was_seen(0x9275), "the level shot did not bounce");
        assert!(was_seen(0x9268), "the falling shot did not bounce");
    }

    /// `$91B2`: weapon 4's three-way shots (`$9E09`), fired walking right from TAS
    /// 370 with `$0476` poked to 4; the whole loop chained.
    #[test]
    fn weapon_4_fires_three_ways() {
        let Some(mut scene) = Scene::tas(370) else {
            return;
        };
        scene.machine.bus.ram[0x476] = 4;
        scene.script(200, |i| if i % 8 < 2 { 0x82 } else { 0x80 });
        let frames = assert_game(scene, 200).frames;
        assert!(frames >= 190, "{frames} loop frames compared");
        assert!(was_seen(0x9E09), "no three-way shot fired");
    }

    /// `$8170`: weapon 1's shots, fired walking right from TAS 370 with `$0476`
    /// poked to 1, home in on the actors about (`$8288`); the whole loop chained.
    #[test]
    fn weapon_1_shots_home_in() {
        let Some(mut scene) = Scene::tas(370) else {
            return;
        };
        scene.machine.bus.ram[0x476] = 1;
        scene.script(200, |i| if i % 8 < 2 { 0x82 } else { 0x80 });
        let frames = assert_game(scene, 200).frames;
        assert!(frames >= 190, "{frames} loop frames compared");
        assert!(was_seen(0x831E), "no shot turned toward a target");
    }

    /// `$847D`: weapon 1's charge gathers (`$8496`), three parts circle it
    /// (`$85CF`, `$8525`) and letting go throws it (`$864A`, `$8515`).
    #[test]
    fn weapon_1_charges_and_throws_a_circle() {
        let frames = charged(1, 120);
        if frames == 0 {
            return;
        }
        assert!(frames >= 190, "{frames} loop frames compared");
        for path in [0x84B6, 0x85CF, 0x8525, 0x864A, 0x8515] {
            assert!(was_seen(path), "${path:04X} did not run");
        }
    }

    /// Weapon 1's charge aimed up (`$9EA7`) and thrown up with Up held (`$864A`).
    #[test]
    fn weapon_1_charges_aiming_up() {
        let frames = charged_with(1, 120, 0x12, 0x10);
        if frames == 0 {
            return;
        }
        assert!(frames >= 190, "{frames} loop frames compared");
        for path in [0x9EA7, 0x8668] {
            assert!(was_seen(path), "${path:04X} did not run");
        }
    }

    /// Weapon 2's four parts circling (`$8AFF`) and thrown (`$8C18`) aiming up.
    #[test]
    fn weapon_2_charges_aiming_up() {
        let frames = charged_with(2, 120, 0x12, 0x10);
        if frames == 0 {
            return;
        }
        assert!(frames >= 190, "{frames} loop frames compared");
        for path in [0x8AFF, 0x8C18] {
            assert!(was_seen(path), "${path:04X} did not run");
        }
    }

    /// Weapon 3's charge gathering (`$8F16`) and its spread (`$9023`) aiming up.
    #[test]
    fn weapon_3_charges_aiming_up() {
        let frames = charged_with(3, 120, 0x12, 0x10);
        if frames == 0 {
            return;
        }
        assert!(frames >= 190, "{frames} loop frames compared");
        for path in [0x8F16, 0x9023] {
            assert!(was_seen(path), "${path:04X} did not run");
        }
    }

    /// `$92C4`: weapon 4's charge blows bubbles (`$92D3`, `$930B`) and, let go,
    /// springs about the player as a tether blowing more (`$935E`).
    #[test]
    fn weapon_4_charges_and_blows_bubbles() {
        let frames = charged(4, 120);
        if frames == 0 {
            return;
        }
        assert!(frames >= 190, "{frames} loop frames compared");
        for path in [0x92D3, 0x930B, 0x935E] {
            assert!(was_seen(path), "${path:04X} did not run");
        }
    }

    /// Weapon 4's charge aimed up (`$9EA7`), its tether springing vertically.
    #[test]
    fn weapon_4_charges_aiming_up() {
        let frames = charged_with(4, 120, 0x12, 0x10);
        if frames == 0 {
            return;
        }
        assert!(frames >= 190, "{frames} loop frames compared");
        assert!(was_seen(0x93BA), "the tether did not spring vertically");
    }

    /// `$8FE8` with no free slot: once weapon 3's charge is out, slots 0-13 get
    /// type `$82` spread shots at the player, so its spread finds none and `INC
    /// $7F60,X` writes the FME-7 command latch `$805F`; the loop chained through it
    /// and on still matches.
    #[test]
    fn weapon_3_spread_with_no_free_slot() {
        let Some(mut scene) = charge_scene(3) else {
            return;
        };
        scene.script(200, |i| if (15..135).contains(&i) { 0x02 } else { 0 });
        scene.run(66, &[]);
        let ram = &scene.machine.bus.ram;
        let (x, y) = (
            u16::from_be_bytes([ram[0x460], ram[0x461]]),
            u16::from_be_bytes([ram[0x462], ram[0x463]]).wrapping_sub(0x0100),
        );
        let wram = &mut scene.machine.bus.wram;
        assert_eq!(wram[0x1BDF], 0x80, "the charge is not in slot 15");
        for slot in 0..14 {
            let shot: [(u16, u8); 10] = [
                (0x7BD0, 0x82),
                (0x7BB0, x.to_be_bytes()[0]),
                (0x7BC0, x.to_be_bytes()[1]),
                (0x7C60, y.to_be_bytes()[0]),
                (0x7C70, y.to_be_bytes()[1]),
                (0x7D00, 0),
                (0x7D10, 0),
                (0x7D20, 0),
                (0x7EB0, 0),
                (0x7F60, 0),
            ];
            for (array, value) in shot {
                wram[usize::from(array - 0x6000) + slot] = value;
            }
        }
        let frames = assert_game(scene, 134).frames;
        assert!(frames >= 125, "{frames} loop frames compared");
        assert!(was_seen(0x8FE8), "the spread never found the slots full");
    }

    /// `$8712`: letting go of weapon 1's charge while it gathers clears it.
    #[test]
    fn weapon_1_charge_let_go_early_is_cleared() {
        let frames = charged(1, 0x36);
        if frames == 0 {
            return;
        }
        assert!(frames >= 110, "{frames} loop frames compared");
        assert!(was_seen(0x8712), "$8712 did not run");
    }

    /// The upward shots `$9EB5[$0476]` of weapons 0-5, fired standing at TAS 470
    /// with Up held, and their flight; the whole loop chained.
    #[test]
    fn every_weapon_fires_up() {
        for (weapon, handler) in [0x9BCE, 0x9BF5, 0x9C06, 0x9C2E, 0x9C7E, 0x9CD9]
            .into_iter()
            .enumerate()
        {
            let Some(mut scene) = Scene::tas(470) else {
                return;
            };
            scene.machine.bus.ram[0x476] = u8::try_from(weapon).expect("a weapon");
            scene.machine.bus.ram[0xB8] = 0x80;
            scene.machine.bus.wram[0x1BD0..0x1BE0].fill(0);
            scene.isolate(&[]);
            scene.script(120, |i| if i % 16 < 2 { 0x12 } else { 0x10 });
            let frames = assert_game(scene, 120).frames;
            assert!(
                frames >= 110,
                "weapon {weapon}: {frames} loop frames compared"
            );
            assert!(was_seen(handler), "${handler:04X} did not run");
        }
    }

    /// Stands at TAS 470 with no actors about, holding B for `hold` frames after a
    /// press, then lets go, with weapon `weapon` and no shots out; the whole loop
    /// chained from there.
    fn charged(weapon: u8, hold: u64) -> usize {
        charged_with(weapon, hold, 0x02, 0)
    }

    /// [`charged`] pressing B, then holding `buttons` (B among them), then `after`.
    fn charged_with(weapon: u8, hold: u64, buttons: u8, after: u8) -> usize {
        hold_b(charge_scene(weapon), hold, buttons, after, 60)
    }

    /// TAS 470 with weapon `weapon`, `$B8` bit 7 set, no shots out and no actors.
    fn charge_scene(weapon: u8) -> Option<Scene> {
        let mut scene = Scene::tas(470)?;
        scene.machine.bus.ram[0x476] = weapon;
        scene.machine.bus.ram[0xB8] = 0x80;
        scene.machine.bus.wram[0x1BD0..0x1BE0].fill(0);
        scene.isolate(&[]);
        Some(scene)
    }

    /// Chains the loop through `scene`: B pressed on frame 15, `buttons` held to
    /// `15 + hold`, then `after` for `tail` frames; returns the loop frames compared.
    fn hold_b(scene: Option<Scene>, hold: u64, buttons: u8, after: u8, tail: u64) -> usize {
        let Some(mut scene) = scene else {
            return 0;
        };
        scene.script(15 + hold + tail, |i| {
            if i == 15 {
                0x02
            } else if (16..15 + hold).contains(&i) {
                buttons
            } else if i >= 15 + hold {
                after
            } else {
                0
            }
        });
        assert_game(scene, 15 + hold + tail).frames
    }

    /// `$9781`: weapon 5's super attack, let go with the meter full: every slot
    /// claimed (`$9790`), five orbiting pairs released (`$9B4A`, `$9843`) and,
    /// `$E0` frames on, the power-up's end (`$9AF7`).
    #[test]
    fn weapon_5_unleashes_its_super_attack() {
        let scene = charge_scene(5).map(|mut scene| {
            scene.machine.bus.ram[0x477] = 0x0E;
            scene
        });
        let frames = hold_b(scene, 120, 0x02, 0, 300);
        if frames == 0 {
            return;
        }
        assert!(frames >= 420, "{frames} loop frames compared");
        for path in [0x97AC, 0x9B4A, 0x9843, 0x9AF7] {
            assert!(was_seen(path), "${path:04X} did not run");
        }
    }

    /// Weapon 5's super attack let go aiming up (`$97F9`).
    #[test]
    fn weapon_5_unleashes_aiming_up() {
        let scene = charge_scene(5).map(|mut scene| {
            scene.machine.bus.ram[0x477] = 0x0E;
            scene
        });
        let frames = hold_b(scene, 120, 0x12, 0x10, 300);
        if frames == 0 {
            return;
        }
        assert!(frames >= 420, "{frames} loop frames compared");
        assert!(was_seen(0x97F9), "$97F9 did not run");
    }

    /// `$9797`: with the meter below 2, weapon 5's charge is dropped.
    #[test]
    fn weapon_5_charge_needs_the_meter() {
        let scene = charge_scene(5).map(|mut scene| {
            scene.machine.bus.ram[0x477] = 0;
            scene
        });
        let frames = hold_b(scene, 120, 0x02, 0, 60);
        if frames == 0 {
            return;
        }
        assert!(frames >= 190, "{frames} loop frames compared");
        assert!(was_seen(0x979E), "$979E did not run");
    }

    /// `$9A56`: weapon 3's charged attack (`$9AE3`) gathers (`$8F03`), fires its
    /// spread (`$8FC2`) and the spread shots fly (`$90E1`).
    #[test]
    fn weapon_3_charges_and_fires_a_spread() {
        let frames = charged(3, 120);
        if frames == 0 {
            return;
        }
        assert!(frames >= 190, "{frames} loop frames compared");
        for path in [0x9AE3, 0x8F03, 0x8FC2, 0x90E1] {
            assert!(was_seen(path), "${path:04X} did not run");
        }
    }

    /// `$9AA0`: weapon 2's four parts circle the player (`$8A4F`) and fly off when
    /// B is let go (`$8BCB`).
    #[test]
    fn weapon_2_charges_and_throws_four_parts() {
        let frames = charged(2, 120);
        if frames == 0 {
            return;
        }
        assert!(frames >= 190, "{frames} loop frames compared");
        for path in [0x9AA0, 0x8A4F, 0x8BCB] {
            assert!(was_seen(path), "${path:04X} did not run");
        }
    }

    /// `$9A81`: weapon 0's charged attack is a plain type `$80` shot.
    #[test]
    fn weapon_0_charges() {
        let frames = charged(0, 80);
        if frames == 0 {
            return;
        }
        assert!(frames >= 150, "{frames} loop frames compared");
        assert!(was_seen(0x9A81), "$9A81 did not run");
    }

    /// Pads that fire: walking right tapping B, walking right or left firing every
    /// third frame, jumping (A, then B twice in the air), crouched and aiming up.
    const FIRING: [fn(u64) -> u8; 6] = [
        |i| if i % 12 < 2 { 0x82 } else { 0x80 },
        |i| if i % 3 == 0 { 0x82 } else { 0x80 },
        |i| if i % 3 == 0 { 0x42 } else { 0x40 },
        |i| match i % 40 {
            0..2 => 0x81,
            2..12 => 0x01,
            12..14 | 20..22 => 0x02,
            _ => 0,
        },
        |i| if i % 6 < 2 { 0x22 } else { 0x20 },
        |i| if i % 6 < 2 { 0x12 } else { 0x10 },
    ];

    /// From TAS 470 with the TAS's weapon 3 and the actors about, each [`FIRING`]
    /// pattern chains 300 frames of the whole loop; firing walking right, a shield
    /// deflects a wave part that falls into the weave's window (`$8E3D`).
    #[test]
    fn firing_scenes_match_the_oracle() {
        for pads in FIRING {
            let Some(mut scene) = Scene::tas(470) else {
                return;
            };
            scene.script(300, pads);
            let frames = assert_game(scene, 300).frames;
            assert!(frames >= 290, "{frames} loop frames compared");
        }
        assert!(
            was_seen(0x8E3D),
            "no deflected wave part reached the window"
        );
    }

    /// From TAS 600, past the stage's weapon pickup, `$0476` poked to `weapon`: each
    /// [`FIRING`] pattern chains 200 frames among the actors, the room change included.
    fn fires_in_every_pose(weapon: u8) {
        for pads in FIRING {
            let Some(mut scene) = Scene::tas(600) else {
                return;
            };
            scene.machine.bus.ram[0x476] = weapon;
            scene.script(200, pads);
            let frames = assert_game(scene, 200).frames;
            assert!(
                frames >= 140,
                "weapon {weapon}: {frames} loop frames compared"
            );
        }
    }

    /// The TAS's weapon 4 in stage 4: a bubble hit at 17353 is dropped before it counts.
    #[test]
    fn weapon_4_bubbles_match_the_tas_in_stage_4() {
        let Some(scene) = Scene::tas(17300) else {
            return;
        };
        assert_eq!(assert_game(scene, 100).frames, 99);
    }

    #[test]
    fn weapon_0_fires_in_every_pose() {
        fires_in_every_pose(0);
    }

    #[test]
    fn weapon_1_fires_in_every_pose() {
        fires_in_every_pose(1);
    }

    #[test]
    fn weapon_2_fires_in_every_pose() {
        fires_in_every_pose(2);
    }

    #[test]
    fn weapon_3_fires_in_every_pose() {
        fires_in_every_pose(3);
    }

    #[test]
    fn weapon_4_fires_in_every_pose() {
        fires_in_every_pose(4);
    }

    #[test]
    fn weapon_5_fires_in_every_pose() {
        fires_in_every_pose(5);
    }

    #[test]
    fn matches_the_oracle_on_the_route() {
        if rotj_rom::for_test().is_none() {
            return;
        }
        let (checked, _) = diff(Scene::route(600), 200);
        assert!(checked >= 150, "{checked} calls compared");
        let (checked, _) = diff(Scene::route(840), 200);
        assert!(checked >= 150, "{checked} calls compared");
    }
}
