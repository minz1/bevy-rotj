//! Batman's projectiles: the bank-5 routine the gameplay loop calls at `$D444`.
//!
//! The routine dispatches on `$0476` through the table at `$801B`. [`step`] ports
//! handlers `$8029` (`$0476` 0), `$8170` (1), `$8891` (2), `$8D39` (3), `$91B2` (4), `$9647` (5)
//! and `$9D2E` (6, the power-up's pieces), the
//! bank-5 subroutines they call (`$9E13`, `$9E46`, `$9ED6`) and the draw (`$C247`,
//! [`engine::draw`]); `docs/re/ACTORS.md` §9g.

use super::collision::{self, Probe};
use super::engine::{self, Work, anchor, rom_word};
use super::oam::{Mark, attr};
use super::scroll::Scroll;
use super::slots::Counter;
use super::world::{Cx, Motion, Projectile, Weapon, attack, power_up, shot_flags, shot_state};

/// A charged attack's first stage, every weapon's: the `kind` the player's charge writes.
pub const CHARGE_START: u8 = Projectile::CHARGED;

/// A shot's `timer` bit 7 while in flight: fired upward (while spent/falling, `timer` counts down).
const AIMED_UP: u8 = 0x80;

/// The homing charge's `param` once its circling parts are out.
const CIRCLED: u8 = 0x80;

/// A power-up piece's `part_timer` or super orbiter's `part`: it lights the sprite palette.
const LIT: u8 = 0x80;

/// A splitter shot's `param`: spent, it runs out its timer instead of splitting. A split part
/// sets it from its bounce byte (`$8A3F`); a fired shot keeps whatever its slot last held.
const NO_SPLIT: u8 = 0x80;

/// The splitter orbit's `param`, toggled every 64 frames: the turn tables 4 on, the `$8C67` throw.
const SECOND_HALF: u8 = 0x80;

/// A wave part's `param`: its weave adds 4 to the velocity instead of taking 4 away.
const WEAVE_INCREASING: u8 = 0x80;

/// The three-way tether's `part_timer`, let go aiming up: it springs vertically.
const VERTICAL: u8 = 0x80;

/// The steps a power-up piece's `part` counts before it lands.
const PIECE_STEPS: u8 = 8;

/// A plain shot's flight mode, decoded from [`Projectile::state`] (`$7C80`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShotMode {
    /// Flying normally, probing walls (`SPENT` and `FALLING` clear).
    Flying,
    /// Falling under gravity and counting `timer` down (`FALLING` set, `SPENT` clear).
    Falling,
    /// Spent impact animation (`SPENT` set).
    Spent,
}

impl ShotMode {
    const fn of(state: u8) -> ShotMode {
        if state & shot_state::SPENT != 0 {
            ShotMode::Spent
        } else if state & shot_state::FALLING != 0 {
            ShotMode::Falling
        } else {
            ShotMode::Flying
        }
    }
}

mod homing;
mod splitter;
mod super_weapon;
mod three_way;
mod wave;

use homing::homing;
use splitter::splitter;
use super_weapon::super_weapon;
use three_way::three_way;
use wave::wave;

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
        if cx.effects.projectiles[slot].is_free() {
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
        piece.part.tick_up();
        if piece.part < PIECE_STEPS {
            continue;
        }
        piece.free();
        let lit = piece.part_timer & LIT != 0;
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
        if !cx.effects.projectiles[slot].is_free() {
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
    let overflow = engine::adc_overflows(projectile.vy.to_be_bytes()[1], 6, false);
    projectile.vy = projectile.vy.wrapping_add(6);
    overflow
}

/// Integrates the projectile's level position by its velocity (`$9E13`).
fn advance(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.x = projectile.x.wrapping_add(projectile.vx.cast_unsigned());
    projectile.y = projectile.y.wrapping_add(projectile.vy.cast_unsigned());
    (work.x, work.y) = (projectile.x, projectile.y);
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
            cx.effects.projectiles[slot].free();
        }
        return false;
    }
    work.screen_x = (dx >> 4).to_le_bytes()[0];
    let dy = work.y.wrapping_sub(camera_y);
    if work.y < camera_y || dy.to_be_bytes()[0] >= 0x0F {
        if !keep {
            cx.effects.projectiles[slot].free();
        } else if dy.to_be_bytes()[0] < 0x15 {
            work.anchor = anchor::BOTTOM;
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

/// Is the metatile at the working position solid (`$C93B`)? Callers pass `phase << 1`, so
/// the phase's bit 1 lands on [`collision::probe::FLOORS_SOLID`].
fn solid(work: &Work, flags: u8, cx: &Cx<'_>) -> bool {
    let mut probe = Probe {
        x: work.x,
        y: work.y,
        ..Probe::default()
    };
    collision::solid(&mut probe, flags, cx)
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

/// The in-flight sprite of handlers 0 and 5: the pointer at `sprites[aimed_up]`.
fn sprite_by_timer(slot: usize, sprites: [u16; 2]) -> impl FnOnce(&mut Cx<'_>) -> u16 {
    move |cx| {
        let aimed_up = cx.effects.projectiles[slot].timer & AIMED_UP != 0;
        rom_word(cx.prg, 5, sprites[usize::from(aimed_up)])
    }
}

/// What a shot's flight did this frame.
enum Flight {
    /// Flying on, or spent; gravity has run when it falls.
    On,
    /// The falling shot's timer ran out and the slot is free; `overflow` is gravity's flag.
    Freed { overflow: bool },
    /// The first wall touch (timer bit 0 clear); nothing is written yet.
    Touched,
    /// A later wall touch (timer bit 0 set); nothing is written yet.
    TouchedAgain,
}

/// Advances a shot, then lets a falling one fall and count down, or probes the wall for the others.
fn fly(slot: usize, work: &mut Work, cx: &mut Cx<'_>) -> Flight {
    advance(slot, work, cx);
    let phase = cx.effects.projectiles[slot].state;
    match ShotMode::of(phase) {
        ShotMode::Spent => return Flight::On,
        ShotMode::Falling => {
            let projectile = &mut cx.effects.projectiles[slot];
            let overflow = gravity(projectile);
            return if projectile.count_down() {
                Flight::Freed { overflow }
            } else {
                Flight::On
            };
        }
        ShotMode::Flying => {}
    }
    if !solid(work, phase << 1, cx) {
        return Flight::On;
    }
    if cx.effects.projectiles[slot].timer & 1 == 0 {
        Flight::Touched
    } else {
        Flight::TouchedAgain
    }
}

/// A shot's flight, wall bounce and draw: `sprite` picks the sprite in flight
/// (and may change `$7C20`), `spent` is the spent sprites' pointer table. The overflow
/// flag it leaves: gravity's, the projection's or the draw's.
fn flying_shot(
    slot: usize,
    work: &mut Work,
    cx: &mut Cx<'_>,
    sprite: impl FnOnce(&mut Cx<'_>) -> u16,
    spent: u16,
) -> bool {
    match fly(slot, work, cx) {
        Flight::Freed { overflow } => return overflow,
        Flight::Touched => {
            #[cfg(test)]
            super::oracle::seen(0x80CD);
            cx.effects.projectiles[slot].timer |= 1;
            let on_screen = project_or_free(slot, work, cx);
            return projection_overflow(on_screen, work, cx);
        }
        Flight::TouchedAgain => {
            #[cfg(test)]
            super::oracle::seen(0x80DA);
            let positive = cx.world.rng[0] & 0x01 != 0;
            bounce_off_wall(slot, cx, positive);
        }
        Flight::On => {}
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
    if phase & shot_state::SPENT == 0 {
        work.pointer = sprite(cx);
    } else {
        work.pointer = rom_word(cx.prg, 5, spent + u16::from(timer & 0xFE));
        let projectile = &mut cx.effects.projectiles[slot];
        projectile.count_down();
    }
    work.attr = cx.effects.projectiles[slot].attr;
    draw_alternate_frames(slot, work, cx).unwrap_or(projected)
}

/// The highest free weapon slot.
fn free_slot(cx: &Cx<'_>) -> Option<usize> {
    (0..16).rev().find(|&i| cx.effects.projectiles[i].is_free())
}

/// The player's level position plus `(dx, dy)`.
fn beside_player(cx: &Cx<'_>, dx: u16, dy: u16) -> (u16, u16) {
    let player = &cx.effects.player;
    (player.x.wrapping_add(dx), player.y.wrapping_add(dy))
}

/// Draws only on the odd frames the wave's flight draws (`$8E5B`).
fn draw_on_odd_frames(work: &mut Work, cx: &mut Cx<'_>) {
    if cx.world.frame_odd {
        draw(work, cx);
    }
}

/// The `$9F07`-`$9F2B` x offset ahead of the player for `motion`.
fn ahead(cx: &Cx<'_>, motion: usize) -> u16 {
    if cx.effects.player.faces_left() {
        cx.prg.weapons.ahead_left_dx[motion]
    } else {
        cx.prg.weapons.ahead_right_dx[motion]
    }
}

/// A shot bounces off a wall and resets its velocity (`$80DA`, `$8E7A`): a falling
/// one to a random side, a level one back, both with a random vertical sign;
/// `positive` is the random pick for `+$20`.
fn bounce_off_wall(slot: usize, cx: &mut Cx<'_>, positive: bool) {
    let random = if positive { 0x0020 } else { -0x0020 };
    let projectile = &mut cx.effects.projectiles[slot];
    if projectile.timer & AIMED_UP != 0 {
        projectile.vy = 0x0020;
        projectile.vx = random;
    } else {
        projectile.vx = if projectile.vx < 0 { 0x0020 } else { -0x0020 };
        projectile.vy = random;
    }
    projectile.state = shot_state::SPENT;
    projectile.attr = 0;
    projectile.timer = 0x04;
    projectile.flags = shot_flags::INERT;
}

#[cfg(test)]
mod tests {
    use super::super::oracle::{Scene, assert_game, assert_routine, was_seen};
    use super::super::world::{Cx, shot_state};

    #[test]
    fn splitter_negate_is_word_maths() {
        for vx in 0..=0xFFFFu16 {
            let [hi, lo] = vx.to_be_bytes();
            let (back_lo, carry) = (!lo).overflowing_add(1);
            let (back_hi, round) = (!hi).overflowing_add(u8::from(carry));
            let back = u16::from_be_bytes([back_hi, back_lo]);
            assert_eq!((vx.wrapping_neg(), vx == 0), (back, round));
        }
    }

    /// Stage 1-1 with the power-up's weapon and its charge (type `$80`) in slot 14, the
    /// meter at 2, after one weapons call with `overflow` coming in and, if `shot_above`, a
    /// shot flying off the screen in slot 15; the charge's type after it.
    fn charge_after(overflow: bool, shot_above: bool) -> Option<u8> {
        let rom = rotj_rom::for_test()?;
        let prg = crate::prg::for_test()?;
        let mut game = super::super::room::start(prg, &rom.chr, 0);
        game.effects.weapon = super::Weapon::Super;
        game.effects.meter = 2;
        game.effects.projectiles[14].kind = super::super_weapon::SuperStage::Unleash.byte();
        if shot_above {
            let shot = &mut game.effects.projectiles[15];
            shot.kind = 1;
            shot.state = shot_state::FALLING;
            shot.timer = 0x10;
            let [_, low] = shot.x.to_be_bytes();
            let high = game.world.camera_x.to_be_bytes()[0].wrapping_add(0x20);
            shot.x = u16::from_be_bytes([high, low]);
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
        assert_eq!(waiting, super::super_weapon::SuperStage::Unleash.byte());
        assert_eq!(
            charge_after(false, false),
            Some(super::super_weapon::SuperStage::Pairs.byte())
        );
        assert_eq!(
            charge_after(true, true),
            Some(super::super_weapon::SuperStage::Pairs.byte())
        );
    }

    /// A finished gather with no free slots frees them all and, with X = `$FF` after
    /// `$8617`, the ROM's reset write lands on main slot 15's timer.
    #[test]
    fn a_gather_that_finds_no_slots_clears_main_slot_15s_timer() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let Some(prg) = crate::prg::for_test() else {
            return;
        };
        let mut game = super::super::room::start(prg, &rom.chr, 0);
        game.effects.weapon = super::Weapon::Homing;
        game.effects.attack = super::attack::CHARGING;
        game.world.frame_odd = false;
        for shot in &mut game.effects.projectiles {
            shot.kind = super::homing::HomingStage::Circling.byte();
        }
        let gather = &mut game.effects.projectiles[0];
        gather.kind = super::homing::HomingStage::Gather.byte();
        gather.param = 0;
        gather.vx = 6 << 8;
        game.slots.main[15].body.timer = 0x55;
        let (mut cx, work) = game.parts(prg);
        super::step(work, &mut cx, false);
        assert_eq!(game.slots.main[15].body.timer, 0);
        assert!(game.effects.projectiles.iter().all(|shot| shot.kind == 0));
        assert_eq!(game.effects.projectiles[0].vx.to_be_bytes()[0], 7);
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
