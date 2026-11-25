//! Batman's movement (bank 4 `$800B`), the stage loop's `$D408` call.
//!
//! It reads the player out of [`Effects`](super::world::Effects), computes the
//! step from his x and y velocities, runs the game's collision probes (bank 14
//! `$C99D`/`$CAFE`/`$CD5F`/`$C93B`), applies gravity and writes him back. See
//! `docs/ACTORS.md` §9h.

use super::collision::{self, Probe};
use super::engine::{self, Work, adc_overflows, anchor, hi, rom_word, sub_overflows16};
use super::oam::attr;
use super::slots::Counter;
use super::world::{
    Cx, Motion, Player, Weapon, attack, death, hurt, orientation, pose, power_up,
    set_first_colours, sound, stage_script,
};
use crate::palette;
use crate::sim::button::Buttons;

/// Projectile kind of the companion shots (`$9D97`, `$9DD1`).
const PARTNER: u8 = 1;

/// Initial upward velocity of a jump (`-44` subpixels/frame).
const JUMP_VY: i16 = i16::from_be_bytes([0xFF, 0xD4]);
/// Downward snap velocity when a slide clears a ledge onto a floor below.
const SLIDE_LEDGE_VY: i16 = i16::from_be_bytes([0x00, 0x40]);
/// Initial leftward slide launch velocity.
const SLIDE_VX_LEFT: i16 = i16::from_be_bytes([0xFF, 0x70]);
/// Initial rightward slide launch velocity.
const SLIDE_VX_RIGHT: i16 = i16::from_be_bytes([0x00, 0x90]);
/// `pose_timer` flag while gliding in a slide.
const SLIDE_GLIDING: u8 = 0x80;
/// `pose_timer` flag while decelerating out of a slide.
const SLIDE_ENDING: u8 = 0x40;
/// `projectile.timer` bit 7 when a shot is launched upward.
const SHOT_AIMED_UP: u8 = 0x80;
/// PRG bank holding the boss-entry pose and line tables.
const ENTRY_BANK: u8 = 14;
/// Boss-entry pose pointer table (`$D1A4`, bank 14).
const ENTRY_POSE_POINTERS: u16 = 0xD1A4;
/// Boss-entry grapnel-line pointer table (`$D1D8`, bank 14).
const ENTRY_LINE_POINTERS: u16 = 0xD1D8;

/// Test-only branch coverage; the test asserts and reports which paths ran.
#[cfg(test)]
macro_rules! hit {
    ($bit:literal) => {
        crate::sim::player::PATHS.with(|paths| paths.set(paths.get() | 1u64 << $bit));
    };
}
#[cfg(not(test))]
macro_rules! hit {
    ($bit:literal) => {};
}

#[cfg(test)]
thread_local! {
    /// Seen branches: bit 0 script exit, 1 ceiling snap, 2 wall-ahead block,
    /// 3 slide off a ledge, 4 airborne x ramp, 5 ride stood, 6 landing response,
    /// 7 air -> 3, 8 air -> 5, 9 damage response. Per thread, so tests running
    /// beside each other do not mix their paths.
    pub(crate) static PATHS: ::std::cell::Cell<u64> = const { ::std::cell::Cell::new(0) };
}

mod fire;
mod physics;
mod state;

pub use physics::step_move;

/// One `anim`'s per-`anim` properties (bank 4), the tables `$88` apart.
pub struct Frame {
    /// The overlay drawn with the shot pose (`$9705`).
    pub overlay: u8,
    /// The flash frame drawn with the power-up off (`$97A4`).
    pub flash: u8,
    /// The flash frame drawn with the power-up on (`$982C`).
    pub power_up_flash: u8,
    /// The contact reach below him (`$989C`).
    pub reach_y: u8,
}

/// The player's animation strips (bank 4), each named for the motion it animates.
pub struct Animations {
    /// Turning from standing, by `pose_timer >> 1` (0-1) (`$96DB`).
    pub standing_turn: [u8; 2],
    /// Turning while walking, by `pose_timer >> 1` (0-2) (`$96DB`).
    pub walking_turn: [u8; 3],
    /// The short landing, by `pose_step` (any byte: it carries over from other motions) (`$96DD`).
    pub landing: [u8; 256],
    /// The long landing, by `pose_timer >> 1` (0-16) (`$96E0`).
    pub long_landing: [u8; 0x11],
    /// Turning while crouched, by `pose_timer >> 1` (0-1) (`$96F1`).
    pub crouching_turn: [u8; 2],
    /// Turning in the air, by `step >> 1` (0-7) (`$96F3`).
    pub jump_turn: [u8; 8],
    /// The walk cycle, by `pose_timer` (0-7) (`$96FD`).
    pub walking: [u8; 8],
    /// The walk-attack frame, by `pose_timer` or the charged attack's slot (0-15 or `$FF`) (`$96FD`).
    pub melee: [u8; 256],
    /// The grounded slide's first frames, by `pose_timer >> 1` (0-1) (`$979D`).
    pub slide_start: [u8; 2],
    /// The airborne slide's frames, by `pose_timer >> 1` (0-1) (`$979F`).
    pub slide_air: [u8; 2],
    /// The standing drift, by `pose_timer >> 1` (0-2) (`$97A1`).
    pub standing_drift: [u8; 3],
}

/// The player's constant tables (bank 4, and bank 14's entry poses).
pub struct Tables {
    /// The animation strips.
    pub animations: Animations,
    /// By `anim` (0-`$87`: the strips, literals up to `$48`, room.rs's `$85`) (`$9705`-`$989C`).
    pub frames: [Frame; 0x88],
    /// The shot overlay's frames during the attack, by `overlay - $14` (0-7) (`$978D`).
    pub attacking_shot_frames: [u8; 8],
    /// The shot overlay's frames otherwise, by `overlay - $14` (0-7) (`$9795`).
    pub idle_shot_frames: [u8; 8],
    /// The muzzle flash's x offset facing right, by `motion` (0-11) (`$966B`).
    pub muzzle_x_right: [u8; 12],
    /// The muzzle flash's x offset facing left, by `motion` (0-11) (`$9677`).
    pub muzzle_x_left: [u8; 12],
    /// The muzzle flash's y offset, by `motion` (0-11) (`$9683`).
    pub muzzle_y: [u8; 12],
    /// The palette set when the power-up's meter empties (`$9662`).
    pub weapon_palette: [u8; 3],
    /// The palette set at the death's first beat (`$918D`).
    pub death_palette: [u8; 4],
    /// The highest projectile slot each weapon may use, by `weapon` (0-6) (`$9B08`).
    pub slot_limits: [u8; 7],
    /// The shot's launch x offset facing right, by `motion` (0-11) (`$9ECD`).
    pub launch_x_right: [i16; 12],
    /// The shot's launch x offset facing left, by `motion` (0-11) (`$9EED`).
    pub launch_x_left: [i16; 12],
    /// The shot's launch y offset, by `motion` (0-11) (`$9ED5`).
    pub launch_y: [i16; 12],
    /// The bolt's launch speed, by side (0 facing left, 1 facing right) (`$9E03`/`$9D48`).
    pub bolt_launch: [i16; 2],
    /// The homing shot's launch speed, by side (`$9E03`/`$9D6D`).
    pub homing_launch: [i16; 2],
    /// The splitter's launch speed, by side (`$9E03`/`$9D9E`).
    pub splitter_launch: [i16; 2],
    /// The wave's launch speed, by side (`$9E03`/`$9E01`).
    pub wave_launch: [i16; 2],
    /// The two-part shots' velocity, by part; high bytes the weave direction (`$9E05`/`$9E07`).
    pub pair_velocities: [i16; 2],
    /// The three-way shot's velocity aiming up, by part (`$9CD3`/`$9CD6`).
    pub threeway_up_velocities: [i16; 3],
    /// The three-way shot's velocity ahead, by part (`$9E6C`/`$9E6F`).
    pub threeway_ahead_velocities: [i16; 3],
    /// The super shot's fan velocity, by `(frame >> 2) & $0F` (0-15) (`$9D0D`/`$9D1D`).
    pub super_fan_velocities: [i16; 16],
    /// The projectile's facing attribute, by side (0 facing left, 1 facing right) (`$9DFF`).
    pub shot_attributes: [u8; 2],
    /// The dying overlay's attribute, by `((death_timer - $30) & $1E) >> 1` (0-11) (`$91A9`).
    pub death_attributes: [u8; 12],
    /// The power-up's facing, by `(frame >> 1) & 1` (`$91D6`).
    pub power_up_facings: [u8; 2],
    /// The boss entry's held pose, by `entry_timer >> 2` (0-2) (bank 14 `$D174`).
    pub entry_poses: [u8; 3],
    /// The boss entry's held second pose, by `entry_timer >> 2` (0-2) (bank 14 `$D177`).
    pub entry_pose_second: [u8; 3],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        let overlay: [u8; 0x88] = engine::rom_bytes(prg, 4, 0x9705);
        let flash: [u8; 0x88] = engine::rom_bytes(prg, 4, 0x97A4);
        let power_up_flash: [u8; 0x88] = engine::rom_bytes(prg, 4, 0x982C);
        let reach_y: [u8; 0x88] = engine::rom_bytes(prg, 4, 0x989C);
        Tables {
            animations: Animations {
                standing_turn: engine::rom_bytes(prg, 4, 0x96DB),
                walking_turn: engine::rom_bytes(prg, 4, 0x96DB),
                landing: engine::rom_bytes(prg, 4, 0x96DD),
                long_landing: engine::rom_bytes(prg, 4, 0x96E0),
                crouching_turn: engine::rom_bytes(prg, 4, 0x96F1),
                jump_turn: engine::rom_bytes(prg, 4, 0x96F3),
                walking: engine::rom_bytes(prg, 4, 0x96FD),
                melee: engine::rom_bytes(prg, 4, 0x96FD),
                slide_start: engine::rom_bytes(prg, 4, 0x979D),
                slide_air: engine::rom_bytes(prg, 4, 0x979F),
                standing_drift: engine::rom_bytes(prg, 4, 0x97A1),
            },
            frames: std::array::from_fn(|i| Frame {
                overlay: overlay[i],
                flash: flash[i],
                power_up_flash: power_up_flash[i],
                reach_y: reach_y[i],
            }),
            attacking_shot_frames: engine::rom_bytes(prg, 4, 0x978D),
            idle_shot_frames: engine::rom_bytes(prg, 4, 0x9795),
            muzzle_x_right: engine::rom_bytes(prg, 4, 0x966B),
            muzzle_x_left: engine::rom_bytes(prg, 4, 0x9677),
            muzzle_y: engine::rom_bytes(prg, 4, 0x9683),
            weapon_palette: engine::rom_bytes(prg, 4, 0x9662),
            death_palette: engine::rom_bytes(prg, 4, 0x918D),
            slot_limits: engine::rom_bytes(prg, 4, 0x9B08),
            launch_x_right: engine::rom_words(prg, 4, 0x9ECD).map(u16::cast_signed),
            launch_x_left: engine::rom_words(prg, 4, 0x9EED).map(u16::cast_signed),
            launch_y: engine::rom_words(prg, 4, 0x9ED5).map(u16::cast_signed),
            bolt_launch: engine::rom_split_words(prg, 4, 0x9E03, 0x9D48).map(u16::cast_signed),
            homing_launch: engine::rom_split_words(prg, 4, 0x9E03, 0x9D6D).map(u16::cast_signed),
            splitter_launch: engine::rom_split_words(prg, 4, 0x9E03, 0x9D9E).map(u16::cast_signed),
            wave_launch: engine::rom_split_words(prg, 4, 0x9E03, 0x9E01).map(u16::cast_signed),
            pair_velocities: engine::rom_split_words(prg, 4, 0x9E07, 0x9E05).map(u16::cast_signed),
            threeway_up_velocities: engine::rom_split_words(prg, 4, 0x9CD6, 0x9CD3)
                .map(u16::cast_signed),
            threeway_ahead_velocities: engine::rom_split_words(prg, 4, 0x9E6F, 0x9E6C)
                .map(u16::cast_signed),
            super_fan_velocities: engine::rom_split_words(prg, 4, 0x9D1D, 0x9D0D)
                .map(u16::cast_signed),
            shot_attributes: engine::rom_bytes(prg, 4, 0x9DFF),
            death_attributes: engine::rom_bytes(prg, 4, 0x91A9),
            power_up_facings: engine::rom_bytes(prg, 4, 0x91D6),
            entry_poses: engine::rom_bytes(prg, 14, 0xD174),
            entry_pose_second: engine::rom_bytes(prg, 14, 0xD177),
        }
    }
}

// ---------------------------------------------------------------------------
// `$84AB`: the player's animation state machine and draw (`$D42C` -> `$D42F`).
// Every arm below is one 6502 block; comments give the addresses.
// ---------------------------------------------------------------------------

/// One frame of Batman's animate-and-draw (`$84AB`).
///
/// `pressed`/`pad` are `$F5`/`$F6`, which the stun routine (`$92E8`) masks in
/// place; the caller gets the masked values back. `work` is the zero-page set
/// `$90-$A6`; `probe` reproduces the `$20-$28` working area.
pub fn step_draw(
    pressed: &mut Buttons,
    pad: &mut Buttons,
    work: &mut Work,
    cx: &mut Cx<'_>,
) -> bool {
    let mut player = cx.effects.player;
    let overflow;
    {
        let mut port = Port {
            player: &mut player,
            work,
            cx,
            probe: Probe::default(),
            pressed: *pressed,
            pad: *pad,
            overflow: false,
        };
        port.run();
        *pressed = port.pressed;
        *pad = port.pad;
        overflow = port.overflow;
    }
    cx.effects.player = player;
    overflow
}

/// Batman's draw alone (`$94A6`), which the room change calls through `$D6FA`.
pub fn draw_player(work: &mut Work, cx: &mut Cx<'_>) {
    let mut player = cx.effects.player;
    {
        let mut port = Port {
            player: &mut player,
            work,
            cx,
            probe: Probe::default(),
            pressed: Buttons::NONE,
            pad: Buttons::NONE,
            overflow: false,
        };
        port.draw();
    }
    cx.effects.player = player;
}

pub(super) struct Port<'a, 'b> {
    player: &'a mut Player,
    work: &'a mut Work,
    cx: &'a mut Cx<'b>,
    probe: Probe,
    pressed: Buttons,
    pad: Buttons,
    /// The 6502 overflow flag as the code so far left it (weapon 5's charge reads it).
    overflow: bool,
}

impl Port<'_, '_> {
    fn rw(&self, addr: u16) -> u16 {
        rom_word(self.cx.prg, 4, addr)
    }

    /// `$8E1F`-style screen x: the low byte of the projected `x - camera`.
    fn screen_x(&self, x: u16) -> u8 {
        (x.wrapping_sub(self.cx.world.camera_x) >> 4).to_le_bytes()[0]
    }

    /// `BIT`'s overflow: the flag takes bit 6 of `value`.
    fn set_overflow_from(&mut self, value: u8) {
        self.overflow = value & 0x40 != 0;
    }

    /// `$C93B`'s probe, which leaves `BIT $DD`'s overflow flag (the vertical-room bit);
    /// true when solid.
    fn solid_at(&mut self, flags: u8) -> bool {
        self.overflow = self.cx.world.orientation & orientation::VERTICAL != 0;
        collision::solid(&mut self.probe, flags, self.cx)
    }

    /// Is the metatile at `x`, `y_hi` solid (`$C93B` with flags `$80`)?
    fn solid_x(&mut self, x: u16, y_hi: u8) -> bool {
        self.probe.x = u16::from(x.to_be_bytes()[0]) << 8;
        self.probe.y = u16::from(y_hi) << 8;
        self.solid_at(0x80)
    }

    /// `$9397`: the death, `$C2` and the music stopped, then `$D274`.
    fn die(&mut self) {
        self.cx.effects.scroll_lock = 0xFF;
        self.cx.effects.music = sound::music::STOP;
        self.cx.effects.player = *self.player;
        engine::level_end(self.cx.effects);
        *self.player = self.cx.effects.player;
    }

    /// Bit 7 of the `asl` that tests the pad's direction against `$046F`.
    fn faces_press(&self) -> bool {
        self.pad.contains(Buttons::RIGHT) != self.player.faces_left()
    }

    fn run(&mut self) {
        if self.player.power_up & power_up::ON != 0 {
            self.player.pose |= pose::SHOT;
        }
        self.player.moved = 0;
        self.project();
        match self.player.motion {
            Motion::Standing => self.standing(),
            Motion::Crouching => self.crouching(),
            Motion::Walking => self.walking(),
            Motion::Jumping => self.jumping(),
            Motion::Sliding => self.sliding(),
            Motion::Falling => self.falling(),
            Motion::AimingUp => self.aiming_up(),
            Motion::Dying => self.dying(),
            Motion::PoweringUp => self.powering_up(),
            Motion::BossEntry => self.boss_entry(),
            Motion::Held => {}
            Motion::Flying => self.flying(),
        }
    }

    /// `$93A0`: projects the player onto the screen and falls into the draw.
    fn project(&mut self) {
        self.set_overflow_from(self.player.stage_script);
        if self.player.stage_script & stage_script::HIDDEN != 0 {
            return;
        }
        let [screen_hi, screen_lo] = self
            .player
            .x
            .wrapping_sub(self.cx.world.camera_x)
            .to_be_bytes();
        let mut sx = (screen_lo >> 4) | (screen_hi << 4);
        if screen_lo & 0x08 != 0 {
            sx = sx.wrapping_add(1);
        }
        self.player.screen_x = sx;
        self.work.screen_x = sx;
        if sx < 0x14 {
            let push = (0x14u8.wrapping_sub(sx)).wrapping_mul(16);
            self.player.x = self.player.x.wrapping_add(u16::from(push));
            self.player.screen_x = 0x14;
            self.work.screen_x = 0x14;
            if self.crush(hi(self.player.x.wrapping_add(0x0C))) {
                self.die();
                self.player.anim = 0x40;
            }
        } else if sx >= 0xE8 {
            let mag = sx.wrapping_sub(0xE7).wrapping_mul(16);
            self.player.x = self
                .player
                .x
                .wrapping_add(0xFF00 | u16::from(mag.wrapping_neg()));
            self.player.screen_x = 0xE7;
            self.work.screen_x = 0xE7;
            if self.crush(hi(self.player.x.wrapping_sub(0xC0))) {
                self.die();
                self.player.anim = 0x40;
            }
        }
        let camy = self.cx.world.camera_y;
        let [dy_hi, dy_lo] = self.player.y.wrapping_sub(camy).to_be_bytes();
        self.overflow = sub_overflows16(self.player.y, camy);
        if dy_hi >= 0x0F {
            self.die();
            self.player.stage_script = stage_script::FROZEN | stage_script::HIDDEN;
            self.player.death_flags = death::NO_OVERLAY;
            return;
        }
        let mut sy = (dy_lo >> 4) | (dy_hi << 4);
        self.overflow = adc_overflows(sy, 0, dy_lo & 0x01 != 0);
        if dy_lo & 0x01 != 0 {
            sy = sy.wrapping_add(1);
        }
        self.player.screen_y = sy;
        self.work.screen_y = sy;
        self.work.anchor = if sy >= 0xF0 { anchor::BOTTOM } else { 0 };
        self.draw();
    }

    /// `$9425`: stops the pushed player and runs the three crush probes at
    /// `y-0x140`, `y-0x80` and `y-0x200`.
    fn crush(&mut self, x_hi: u8) -> bool {
        self.player.vx = 0;
        let y = self.player.y;
        let ys = [
            hi(y.wrapping_sub(0x140)),
            hi(y.wrapping_sub(0x80)),
            hi(y.wrapping_sub(0x200)),
        ];
        ys.into_iter().any(|y| {
            self.probe.x = u16::from(x_hi) << 8;
            self.probe.y = u16::from(y) << 8;
            self.solid_at(0x80)
        })
    }

    /// `$94A6`: the draw, its overlay and the hurt flash.
    fn draw(&mut self) {
        self.set_overflow_from(self.player.hurt);
        if self.player.hurt & hurt::STUNNED == 0 && self.player.hurt & hurt::INVULNERABLE != 0 {
            self.player.hurt_timer.tick_down();
            if self.player.hurt_timer == 0 {
                self.player.hurt &= !(hurt::STUNNED | hurt::INVULNERABLE);
            } else {
                self.overflow = false;
                if self.cx.world.frame_odd {
                    return;
                }
            }
        }
        self.work.attr = self.player.facing;
        let anim = self.player.anim;
        let frame = &self.cx.prg.player.frames[usize::from(anim)];
        self.player.reach_y = frame.reach_y;
        let overlay = frame.overlay;
        let table = if anim & 0x80 != 0 { 0x9A24 } else { 0x9924 };
        self.work.pointer = self.rw(table + u16::from(anim << 1));
        self.work.chr_slot = 5;
        self.work.tile = 0;
        self.overflow = engine::draw(self.work, 1, self.cx);
        if overlay != 0 {
            let index = if self.player.pose & pose::SHOT != 0 {
                let x = usize::from(overlay.wrapping_sub(0x14));
                let player = &self.cx.prg.player;
                if (0x03..0x13).contains(&self.player.attack_timer) {
                    player.attacking_shot_frames[x]
                } else {
                    player.idle_shot_frames[x]
                }
            } else {
                overlay
            };
            self.work.pointer = self.rw(0x9924 + u16::from(index) * 2);
            self.overflow = engine::draw(self.work, 1, self.cx);
        }
        self.drain_power_up();
    }

    /// `$9535`: the power-up's meter drain and its end, then the flash and muzzle overlays.
    fn drain_power_up(&mut self) {
        self.set_overflow_from(self.player.power_up);
        if self.player.power_up & power_up::ON != 0 {
            self.player.flash_timer.tick_up();
            if self.player.flash_timer.is_multiple_of(64) {
                self.overflow = adc_overflows(self.cx.effects.meter, !2, true);
                self.cx.effects.meter = self.cx.effects.meter.wrapping_sub(2);
                if self.cx.effects.meter.ran_out() {
                    let weapon = self.cx.effects.stored_weapon;
                    self.cx.effects.weapon = weapon;
                    self.cx.effects.attack = if weapon == Weapon::Bolt {
                        0
                    } else {
                        attack::CHARGEABLE
                    };
                    self.player.power_up &= !power_up::ON;
                    self.player.hurt = hurt::INVULNERABLE;
                    self.player.hurt_timer = 0x40;
                    for projectile in &mut self.cx.effects.projectiles {
                        projectile.free();
                    }
                    let palette = self.cx.prg.player.weapon_palette;
                    for i in 0..3u16 {
                        self.cx.effects.palette[0x11 + usize::from(i)] = palette[usize::from(i)];
                    }
                    set_first_colours(&mut self.cx.effects.palette, palette::BLACK);
                    self.cx.effects.palette_dirty = 0x80;
                }
            }
            self.flash(self.cx.prg.player.frames[usize::from(self.player.anim)].power_up_flash);
        } else {
            self.flash(self.cx.prg.player.frames[usize::from(self.player.anim)].flash);
        }
        self.muzzle();
    }

    /// Draws the flash frame `value` names, when non-zero (`$9591`/`$95B4`).
    fn flash(&mut self, value: u8) {
        if value == 0 {
            return;
        }
        self.work.pointer = self.rw(0x9924 + u16::from(value) * 2);
        self.overflow = engine::draw(self.work, 1, self.cx);
    }

    /// `$95E4`: the muzzle flash the draw adds over the projectile pose.
    fn muzzle(&mut self) {
        if self.cx.effects.attack & (attack::CHARGING | attack::CHARGED) == 0 {
            return;
        }
        self.set_overflow_from(self.player.pose);
        if self.player.pose & (pose::TURNING | pose::SHOT) != pose::SHOT {
            return;
        }
        self.work.attr = 0;
        if self.player.motion == Motion::AimingUp {
            let add = if self.player.faces_left() { 0xFE } else { 0x02 };
            self.work.screen_x = self.work.screen_x.wrapping_add(add);
            let (y, carry) = self.work.screen_y.overflowing_add(0xBF);
            self.overflow = adc_overflows(0xBF, self.work.screen_y, false);
            if !carry {
                return;
            }
            self.work.screen_y = y;
        } else {
            let motion = usize::from(self.player.motion.byte());
            let player = &self.cx.prg.player;
            let add = if self.player.faces_left() {
                player.muzzle_x_left[motion]
            } else {
                player.muzzle_x_right[motion]
            };
            let (x, carry) = self.work.screen_x.overflowing_add(add);
            self.overflow = adc_overflows(add, self.work.screen_x, false);
            self.work.screen_x = x;
            if carry != self.player.faces_left() {
                return;
            }
            let add = player.muzzle_y[motion];
            self.overflow = adc_overflows(add, self.work.screen_y, false);
            self.work.screen_y = self.work.screen_y.wrapping_add(add);
        }
        if self.cx.effects.attack & attack::CHARGED == 0 {
            return;
        }
        self.work.pointer = self.rw(0x9A38 + u16::from(self.cx.world.frame & 0x06));
        self.work.anchor = 0;
        self.work.tile = 0;
        self.overflow = engine::draw(self.work, 1, self.cx);
    }

    /// `$925D`: the hurt/death state.
    fn hurt(&mut self) {
        self.player.walk_gate = Buttons::NONE;
        self.player.hurt_timer.tick_down();
        if self.player.hurt_timer.ran_out() {
            self.set_overflow_from(self.player.energy);
            if self.player.out_of_energy() {
                self.die();
                return;
            }
            self.player.pose_timer = 0;
            self.player.pose_step = 0;
            self.player.motion = Motion::Standing;
            self.player.pose = 0;
            self.player.hurt = hurt::INVULNERABLE;
            self.player.hurt_timer = 0x40;
            self.player.facing &= attr::FLIP_X;
            self.player.pose = pose::LANDING;
            self.player.anim = 0x09;
            return;
        }
        self.player.pose &= !pose::SHOT;
        self.player.anim = 0x40;
        self.hurt_facing();
    }

    /// `$9274`: the knocked-back facing, which alternates every fourth frame.
    fn hurt_facing(&mut self) {
        let mut x = self.player.facing & attr::FLIP_X;
        if self.cx.world.frame & 0x04 == 0 {
            x |= attr::PALETTE;
        }
        self.player.facing = x;
    }

    /// `$92E8`: the frame Batman spends stunned while knocked back, which masks `$F5`/`$F6`
    /// in place.
    fn stunned(&mut self) {
        if self.player.hurt & hurt::STUNNED == 0 {
            return;
        }
        self.player.hurt_timer.tick_down();
        if self.player.hurt_timer.ran_out() {
            if self.player.out_of_energy() {
                self.die();
                return;
            }
            self.player.hurt = hurt::INVULNERABLE;
            self.player.hurt_timer = 0x40;
            self.player.facing &= attr::FLIP_X;
            return;
        }
        self.cx.effects.attack &= attack::CHARGEABLE;
        // `ASL` of `$B8 & $80` shifts the bit out: both always get 0.
        self.player.pose = 0;
        self.player.walk_gate = Buttons::NONE;
        self.pressed &= Buttons::START;
        self.pad &= Buttons::START;
        self.player.facing = if self.cx.world.frame & 0x04 != 0 {
            self.player.facing & attr::FLIP_X
        } else {
            (self.player.facing & attr::FLIP_X) | attr::PALETTE
        };
    }

    /// `$92B4`: down pressed: the crouch pose, which the next frame turns into crouching.
    fn start_crouch(&mut self) {
        self.set_overflow_from(self.player.pose);
        if self.player.pose & pose::TURNING != 0 {
            self.player.facing ^= attr::FLIP_X;
        }
        self.player.anim = 0x01;
        self.player.pose =
            (self.player.pose & pose::SHOT) | pose::RESUME | Motion::Crouching.byte();
    }

    /// `$92D1`: the solid probe two rows above the player, the headroom check for standing
    /// from a crouch or slide.
    fn headroom_blocked(&mut self) -> bool {
        let y = hi(self.player.y.wrapping_sub(0x280));
        let x_hi = self.player.x.to_be_bytes()[0];
        self.solid_x(u16::from(x_hi) << 8, y)
    }

    /// `$9338`: sets the x velocity from the pad's walk bits.
    fn set_vx_from_pad(&mut self) {
        self.player.vx = match (self.pad & Buttons::SIDEWAYS).bits() {
            0 => 0,
            1 => 0x0010,
            _ => -0x0010,
        };
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::map::Cell;
    use crate::sim::button::Buttons;
    use crate::sim::collision::Probe;
    use crate::sim::engine::hi;
    use crate::sim::oracle::{Routine, Scene, assert_game, assert_routine, was_seen};
    use crate::sim::world::Cx;

    use super::{draw_player, step_draw, step_move};

    #[test]
    fn byte_borrows_are_word_maths() {
        for vy in 0..=0xFFFFu16 {
            let [hi, lo] = vy.to_be_bytes();
            let (lo, carry) = lo.overflowing_add(6);
            let hi = hi.wrapping_add(u8::from(carry));
            let (hi, lo) = if hi & 0x80 != 0 {
                (0xFF, if lo & 0x80 != 0 { lo } else { 0x80 })
            } else {
                (0x00, if lo & 0x80 == 0 { lo } else { 0x7F })
            };
            let work = crate::sim::engine::Work {
                vy: vy.cast_signed(),
                ..Default::default()
            };
            let mut player = crate::sim::world::Player::default();
            super::physics::gravity(&mut player, &work);
            assert_eq!(player.vy, i16::from_be_bytes([hi, lo]));
        }
        for y in 0..=0xFFFFu16 {
            let [yhi, ylo] = y.to_be_bytes();
            assert_eq!(
                hi(y.wrapping_sub(0x140)),
                yhi.wrapping_sub(1).wrapping_sub(u8::from(ylo < 0x40))
            );
            assert_eq!(
                hi(y.wrapping_sub(0x80)),
                yhi.wrapping_sub(u8::from(ylo < 0x80))
            );
            assert_eq!(hi(y.wrapping_sub(0x200)), yhi.wrapping_sub(2));
            assert_eq!(
                hi(y.wrapping_sub(0x280)),
                yhi.wrapping_sub(2).wrapping_sub(u8::from(ylo < 0x80))
            );
            assert_eq!(
                hi(y.wrapping_sub(0x380)),
                yhi.wrapping_sub(3).wrapping_sub(u8::from(ylo < 0x80))
            );
            let (_, carry) = ylo.overflowing_add(0x0C);
            assert_eq!(hi(y.wrapping_add(0x0C)), yhi.wrapping_add(u8::from(carry)));
            let (_, borrow) = ylo.overflowing_sub(0xC0);
            assert_eq!(hi(y.wrapping_sub(0xC0)), yhi.wrapping_sub(u8::from(borrow)));
        }
    }

    /// Every `anim` a strip or `flight_steer`'s `walk_frames` gives indexes `frames`.
    #[test]
    fn every_anim_is_in_the_tables() {
        let Some(prg) = crate::prg::for_test() else {
            return;
        };
        let tables = &prg.player;
        let animations = &tables.animations;
        for strip in [
            &animations.standing_turn[..],
            &animations.walking_turn[..],
            &animations.landing[..],
            &animations.long_landing[..],
            &animations.crouching_turn[..],
            &animations.jump_turn[..],
            &animations.walking[..],
            &animations.melee[..],
            &animations.slide_start[..],
            &animations.slide_air[..],
            &animations.standing_drift[..],
            &prg.families.flight_steer.walk_frames[..],
        ] {
            assert!(
                strip
                    .iter()
                    .all(|&anim| usize::from(anim) < tables.frames.len()),
                "a strip value is not below $88"
            );
        }
    }

    /// Every nonzero overlay picks a shot frame inside the window's 8 entries.
    #[test]
    fn the_shot_overlay_frames_are_in_range() {
        let Some(prg) = crate::prg::for_test() else {
            return;
        };
        let tables = &prg.player;
        for (anim, frame) in tables.frames.iter().enumerate() {
            if frame.overlay != 0 {
                let x = usize::from(frame.overlay.wrapping_sub(0x14));
                assert!(x < tables.attacking_shot_frames.len(), "anim {anim}");
            }
        }
    }

    /// Runs `step_draw` on `r`'s lifted state, returning the masked pads.
    fn run_step_draw(r: &mut Routine<'_>) -> (u8, u8) {
        let mut pressed = Buttons::from_bits(r.begin.ram[0xF5]);
        let mut pad = Buttons::from_bits(r.begin.ram[0xF6]);
        let prg = r.prg;
        let state = &mut r.state;
        let mut cx = Cx {
            prg,
            world: &state.world,
            slots: &mut state.slots,
            schedule: &mut state.schedule,
            effects: &mut state.effects,
            scroll: &mut state.scroll,
            oam: &mut state.oam,
        };
        step_draw(&mut pressed, &mut pad, &mut state.work, &mut cx);
        (pressed.bits(), pad.bits())
    }

    /// Runs `draw_player` on `r`'s lifted state.
    fn run_draw_player(r: &mut Routine<'_>) {
        let prg = r.prg;
        let state = &mut r.state;
        let mut cx = Cx {
            prg,
            world: &state.world,
            slots: &mut state.slots,
            schedule: &mut state.schedule,
            effects: &mut state.effects,
            scroll: &mut state.scroll,
            oam: &mut state.oam,
        };
        draw_player(&mut state.work, &mut cx);
    }

    /// One scene's worth of compared brackets, counting the motion state
    /// (`$046E`) and stage-script byte (`$0481`) each began with.
    fn run(
        scene: Option<Scene>,
        frames: u64,
        motions: &mut BTreeMap<u8, usize>,
        scripts: &mut BTreeMap<u8, usize>,
    ) -> usize {
        let Some(mut scene) = scene else {
            return 0;
        };
        assert_routine(&mut scene, frames, (0x800B, 4), &[0xD40B], |r| {
            *motions.entry(r.begin.ram[0x46E]).or_default() += 1;
            *scripts.entry(r.begin.ram[0x481]).or_default() += 1;
            let mut probe = Probe::lift(&r.begin.ram);
            r.in_slot(|_, work, cx| step_move(work, &mut probe, cx));
        })
    }

    /// `$800B` matches the oracle from the stage 1-1 TAS (351-2030) and the route.
    #[test]
    fn matches_the_oracle_on_stage_1_1() {
        if rotj_rom::for_test().is_none() {
            return;
        }
        super::PATHS.set(0);
        let mut motions = BTreeMap::new();
        let mut scripts = BTreeMap::new();
        let checked = run(Scene::tas(351), 2030 - 351, &mut motions, &mut scripts)
            + run(Scene::route(640), 359, &mut motions, &mut scripts);
        assert!(checked >= 1500, "$800B: {checked} brackets compared");
        // The early exit at `$8000` is taken only while `$0481` bit 7 is set; the
        // values stage 1-1 uses also set bit 5, so it returns before clearing
        // `$F5/$F6` (the one path `step_move` does not model).
        assert!(
            scripts
                .keys()
                .all(|value| value & 0x80 == 0 || value & 0x20 != 0),
            "$0481 values {scripts:?}"
        );
        // Every motion value the routine dispatches on was compared.
        for motion in 0..=5 {
            assert!(motions.contains_key(&motion), "motion {motion} unexercised");
        }
        let paths = super::PATHS.get();
        // Coverage pin: script exit, wall block, slide off a ledge, airborne ramp,
        // ride, landing response and both air transitions ran; the damage
        // response (`$82B9`, bit 9) is exercised by its own poked test below
        // (this route holds no damage tile) and the ceiling snap (`$8112`) did
        // not run.
        assert_eq!(paths, 0b1_0111_1101, "paths {paths:#b}");
        println!(
            "$800B: {checked} brackets; motions {motions:?}; $0481 {scripts:?}; paths {paths:#b}"
        );
    }

    /// `$D42C` -> `$D42F`: the animate-and-draw over the stage 1-1 TAS.
    #[test]
    fn draw_matches_the_oracle_over_the_tas() {
        if rotj_rom::for_test().is_none() {
            return;
        }
        let mut motions: BTreeMap<u8, usize> = BTreeMap::new();
        let mut checked = 0;
        let draw_case =
            |scene: Option<Scene>, frames: u64, motions: &mut BTreeMap<u8, usize>| -> usize {
                let Some(mut scene) = scene else {
                    return 0;
                };
                assert_routine(&mut scene, frames, (0xD42C, 14), &[0xD42F], |r| {
                    *motions.entry(r.begin.ram[0x46E]).or_default() += 1;
                    let (pressed, pad) = run_step_draw(r);
                    r.writes.push((0xF5, pressed));
                    r.writes.push((0xF6, pad));
                })
            };
        checked += draw_case(Scene::tas(351), 2030 - 351, &mut motions);
        checked += draw_case(Scene::route(640), 359, &mut motions);
        assert!(checked >= 1900, "$84AB: {checked} brackets compared");
        // The states the two scenes reached; motions 7 and 9 are not reached by them.
        println!("$84AB: {checked} brackets; motions {motions:?}");
        for handler in [0x9C2E, 0x9D2D, 0x9D6F, 0x9DA0] {
            assert!(
                was_seen(handler),
                "projectile handler ${handler:04X} did not run"
            );
        }
        assert!(!motions.is_empty(), "$84AB: no motion compared");
    }

    /// `$D6FA` -> `$D704`: the room change's own `$94A6` draw.
    #[test]
    fn draw_matches_the_oracle_in_the_room_changes() {
        if rotj_rom::for_test().is_none() {
            return;
        }
        let mut checked = 0;
        let mut motions: BTreeMap<u8, usize> = BTreeMap::new();
        for start in [630, 1243] {
            let Some(mut scene) = Scene::tas(start) else {
                continue;
            };
            checked += assert_routine(&mut scene, 16, (0xD6FA, 14), &[0xD704], |r| {
                *motions.entry(r.begin.ram[0x46E]).or_default() += 1;
                run_draw_player(r);
            });
        }
        assert!(checked >= 20, "$94A6: {checked} calls compared");
        println!("$94A6: {checked} calls; motions {motions:?}");
    }

    /// A jump blocked by a solid metatile 56 px up, at the player's column and behind him.
    #[test]
    fn a_blocked_jump_matches_the_oracle() {
        for (behind, direction) in [(false, 0x80), (true, 0)] {
            let Some(mut scene) = Scene::tas(2425) else {
                return;
            };
            let ram = &scene.machine.bus.ram;
            let x = u16::from_be_bytes([ram[0x460], ram[0x461]]);
            let [column, _] = if behind { x - 0xA0 } else { x }.to_be_bytes();
            let feet = ram[0x462];
            let page_columns: [u8; 32] = ram[0x440..0x460].try_into().expect("32 pages");
            let wram = &mut scene.machine.bus.wram;
            let ground =
                wram[Cell::at(x.to_be_bytes()[0], feet + 1, &page_columns, false).offset()];
            wram[Cell::at(column, feet - 3, &page_columns, false).offset()] = ground;
            scene.script(20, |i| direction | u8::from((1..4).contains(&i)));
            let blocked = scene.clone().run(20, &[0x8A07, 0x854C]);
            assert!(
                blocked.iter().any(|peek| peek.bank == Some(4)),
                "behind {behind}: the oracle's side-jump was not blocked"
            );
            let run = assert_game(scene, 20);
            assert!(!run.hung, "behind {behind}: {run:?}");
        }
    }

    /// `$82B9`: a damage-class tile under the player drains energy.
    #[test]
    fn a_damage_tile_costs_energy() {
        let Some(mut scene) = Scene::route(700) else {
            return;
        };
        // Make every solid metatile a damage class (`$7340` class 3 is stored as 6).
        for class in &mut scene.machine.bus.wram[0x1340..0x1410] {
            if *class != 0 {
                *class = 6;
            }
        }
        let before = scene.machine.bus.ram[0x478];
        let checked = assert_routine(&mut scene, 4, (0x800B, 4), &[0xD40B], |r| {
            let mut probe = Probe::lift(&r.begin.ram);
            r.in_slot(|_, work, cx| step_move(work, &mut probe, cx));
        });
        let after = scene.machine.bus.ram[0x478];
        assert!(checked >= 3, "$82B9: {checked} brackets compared");
        assert!(
            after != before,
            "energy did not drop ({before:#04X} -> {after:#04X})"
        );
        println!("$82B9: {checked} brackets; energy {before:#04X} -> {after:#04X}");
    }

    /// The spike response's `$A8`/`$0472` write survives `step_move`: the wall and floor
    /// probes run on the player's working copy, which the movement does not overwrite.
    #[test]
    fn the_spike_response_survives_step_move() {
        let Some(mut scene) = Scene::route(700) else {
            return;
        };
        // Make every solid metatile a spike class (`$10`, stored as `$20`).
        for class in &mut scene.machine.bus.wram[0x1340..0x1410] {
            if *class != 0 {
                *class = 0x20;
            }
        }
        let spiked = scene.clone().run(4, &[0xCAA9]).len();
        assert!(spiked > 0, "the oracle never reached the spike response");
        let checked = assert_routine(&mut scene, 4, (0x800B, 4), &[0xD40B], |r| {
            let mut probe = Probe::lift(&r.begin.ram);
            r.in_slot(|_, work, cx| step_move(work, &mut probe, cx));
        });
        assert!(checked >= 3, "$800B: {checked} brackets compared");
    }
}
