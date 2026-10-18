//! Batman's movement (bank 4 `$800B`), the stage loop's `$D408` call.
//!
//! It reads the player out of [`Effects`](super::world::Effects), computes the
//! step from his x and y velocities, runs the game's collision probes (bank 14
//! `$C99D`/`$CAFE`/`$CD5F`/`$C93B`), applies gravity and writes him back. See
//! `docs/ACTORS.md` §9h.

use super::collision::{self, Probe};
use super::engine::{self, Work, adc_overflows, rom_word, sub_overflows16};
use super::world::{
    Cx, Motion, Player, Projectile, Weapon, attack, busy, hurt, orientation, pose, power_up,
    set_first_colours, sliding, stage_script,
};
use crate::actor::button::Buttons;
use crate::actors::STATE_UNTARGETED;

/// Test-only branch coverage; the test asserts and reports which paths ran.
#[cfg(test)]
macro_rules! hit {
    ($bit:literal) => {
        crate::actor::player::PATHS.with(|paths| paths.set(paths.get() | 1u64 << $bit));
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

/// The shared tail of the forward shots (`$9D65`, `$9D8C`): no y motion.
fn shot_ahead(projectile: &mut Projectile, damage: u8) {
    projectile.damage = damage;
    projectile.vy = 0;
    projectile.state = 0;
    projectile.timer = 0;
    projectile.flags = 0;
}

/// A part's start at its source's level position (`$9F5B`).
fn at_source(projectile: &mut Projectile, source: &Projectile) {
    projectile.level_x_hi = source.level_x_hi;
    projectile.level_x_lo = source.level_x_lo;
    projectile.level_y_hi = source.level_y_hi;
    projectile.level_y_lo = source.level_y_lo;
}

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
    /// The two-part shots' velocity, by part; its high bytes are the `target` (`$9E05`/`$9E07`).
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

/// The walk target table (`$84A0` high, `$84A3` low), by direction index:
/// right, left, no direction.
const WALK_TARGET: [i16; 3] = [0x0022, -0x0022, 0];

/// One frame of Batman's movement (`$800B`).
///
/// It reads this frame's `$F6` from `cx.world.pad`. `probe` is the game's `$20-$28` working area,
/// seeded from the frame's RAM: the ride fall-through reads landing bytes
/// `$27/$28` a probe may have left, and those are also this call's inputs.
pub fn step_move(work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) {
    let pad = cx.world.pad;
    let mut player = cx.effects.player;
    if player.stage_script & stage_script::FROZEN != 0 {
        hit!(0);
        return;
    }
    work.probe_flags = 0;
    work.attr = 0;
    player.hit = 0;
    player.moved = 0;
    work.age = player.walk_timer;

    work.ref_vx = WALK_TARGET[walk_intent(pad, &mut player)];
    work.ax = player.vx;
    work.step_x = player.vx.wrapping_add(player.carrier);
    work.vy = player.vy;
    work.x = player.x.wrapping_add_signed(work.step_x);
    work.y = player.y.wrapping_add_signed(work.vy);

    // `$80F6`: the body wall probe, a metatile above the feet.
    probe.x = work.x;
    probe.y = work.y.wrapping_sub(0x80);
    wall(&mut player, work, probe, 0xC0, cx);

    let falling = work.vy >= 0;
    if falling {
        // `$8135`: land on the floor at the feet.
        probe.y = work.y;
        if floor(&mut player, work, probe, cx) < 0x80 {
            work.probe_flags |= 0x40;
        }
    } else {
        // `$8103`: rising into the ceiling `$2B0` above.
        let below = work.y.wrapping_sub(0x02B0);
        if below < 0x8000 {
            probe.y = below;
            wall(&mut player, work, probe, 0x08, cx);
        } else {
            // Above `$2B0` (`$8112`): snap to the next `..B0` line and stop.
            hit!(1);
            let add = below.to_le_bytes()[0].wrapping_neg();
            let [hi, lo] = work.y.to_be_bytes();
            let (lo, carry) = lo.overflowing_add(add);
            work.y = u16::from_be_bytes([hi.wrapping_add(u8::from(carry)), lo]);
            work.vy = 0;
        }
    }

    wall_ahead(&mut player, work, probe, cx);
    ground_or_ride(pad, falling, &mut player, work, probe, cx);

    // `$82B9-$8310`: a damage response (`$9F` bit 7, set by the `$C99D`/`$CAFE`
    // class handlers on a damage tile) costs energy and knocks the player back.
    if work.probe_flags & 0x80 != 0
        && (player.hit & 0x80 != 0 || player.hurt & hurt::INVULNERABLE == 0)
        && player.energy & 0x80 == 0
    {
        hit!(9);
        player.energy = player.energy.wrapping_sub(work.damage);
        cx.effects.sound[0] = 0x25;
        cx.effects.attack &= attack::CHARGEABLE;
        let knock_back = engine::start_hurt(&mut player);
        if player.motion.on_foot() {
            work.ax = knock_back;
        }
    }

    gravity(&mut player, work);
    player.moved = 0x80;
    if player.motion == Motion::Jumping || player.motion == Motion::Falling {
        player.vx = work.ax;
    } else {
        player.vx = player.vx.wrapping_add(work.ax);
        let lo = player.vx.to_be_bytes()[1];
        if (player.vx < 0 && lo >= 0xFA) || (player.vx >= 0 && lo < 0x07) {
            player.vx = 0;
        }
    }
    player.x = work.x;
    player.y = work.y;
    player.walk_timer = work.age;
    player.facing = (player.facing & 0xCF) | work.attr;
    if player.vx == 0 && !pad.intersects(Buttons::SIDEWAYS) {
        player.walk_timer = 1;
    } else {
        player.walk_timer = player.walk_timer.wrapping_add(1);
    }
    cx.effects.player = player;
}

/// `$8148`: the wall ahead of the step, probed at three heights.
fn wall_ahead(player: &mut Player, work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) {
    if work.step_x == 0 {
        return;
    }
    let direction = if work.step_x < 0 {
        probe.x = work.x.wrapping_sub(0xA0);
        2
    } else {
        probe.x = work.x.wrapping_add(0xA0);
        1
    };
    probe.y = work.y.wrapping_sub(0x0140);
    let mut blocked = wall(player, work, probe, direction, cx) < 0x80;
    if !blocked {
        probe.y = work.y.wrapping_sub(0x80);
        blocked = wall(player, work, probe, direction, cx) < 0x80;
    }
    if !blocked && player.motion != Motion::Sliding && player.motion != Motion::Crouching {
        probe.y = work.y.wrapping_sub(0x0220);
        blocked = wall(player, work, probe, direction, cx) < 0x80;
    }
    if blocked {
        hit!(2);
        player.vx = 0;
        player.carrier = 0;
    }
}

/// `$81CA-$8268`: the floor or ceiling under the step, the ride check and, off
/// the ground, the motion state.
fn ground_or_ride(
    pad: Buttons,
    falling: bool,
    player: &mut Player,
    work: &mut Work,
    probe: &mut Probe,
    cx: &mut Cx<'_>,
) {
    let mut grounded = work.probe_flags & 0x40 != 0;
    if falling {
        probe.y = work.y;
        if !grounded {
            probe.x = work.x.wrapping_add(0xA0);
            grounded = floor(player, work, probe, cx) < 0x80;
            if !grounded {
                probe.x = work.x.wrapping_sub(0xA0);
                grounded = floor(player, work, probe, cx) < 0x80;
            }
        }
        if grounded {
            work.probe_flags |= 0x40;
        }
        // `$824F`: fall onto a platform, or settle the motion state.
        let stood = collision::ride(work, probe, cx);
        if stood || grounded {
            if stood {
                hit!(5);
            }
            player.carrier = i16::from_be_bytes([probe.landing[1], probe.landing[0]]);
            landing_response(pad, player);
        } else if air_state(player, work, probe, cx) {
            player.carrier = 0;
        }
    } else if !grounded {
        probe.y = work.y.wrapping_sub(0x02B0);
        probe.x = work.x.wrapping_add(0xA0);
        grounded = wall(player, work, probe, 0x08, cx) < 0x80;
        if !grounded {
            probe.x = work.x.wrapping_sub(0xA0);
            grounded = wall(player, work, probe, 0x08, cx) < 0x80;
        }
        if grounded {
            work.probe_flags |= 0x40;
        } else if air_state(player, work, probe, cx) {
            player.carrier = 0;
        }
    }
}

/// Runs `f` on the player's working copy: the game's collision probes read and write
/// him through `cx` (the spike response's `$A8`, `$0472`), so the copy must be current.
fn with_player<R>(player: &mut Player, cx: &mut Cx<'_>, f: impl FnOnce(&mut Cx<'_>) -> R) -> R {
    cx.effects.player = *player;
    let result = f(cx);
    *player = cx.effects.player;
    result
}

/// `$C99D` on the player's working copy, which the spike response (`$CAA9`) reads
/// (`$B9`) and writes (`$A8`, `$0472`).
fn wall(player: &mut Player, work: &mut Work, probe: &mut Probe, flags: u8, cx: &mut Cx<'_>) -> u8 {
    with_player(player, cx, |cx| collision::wall(work, probe, flags, cx))
}

/// `$CAFE` on the player's working copy, as [`wall`] (spikes underfoot, `$CD09`).
fn floor(player: &mut Player, work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) -> u8 {
    with_player(player, cx, |cx| collision::floor(work, probe, cx))
}

/// The walk intent and its ±2 ramp (`$801F-$809A`); returns the target index.
fn walk_intent(pad: Buttons, player: &mut Player) -> usize {
    if player.motion == Motion::Jumping {
        // Jumping: ramp the x velocity by 2 a frame, in place.
        if player.pose & pose::TURNING == 0
            && player.hurt & hurt::STUNNED == 0
            && pad.intersects(Buttons::SIDEWAYS)
        {
            hit!(4);
            let lo = player.vx.to_be_bytes()[1];
            if pad.contains(Buttons::RIGHT) {
                player.vx = if player.vx >= 0 && lo >= 0x20 {
                    0x0020
                } else {
                    player.vx.wrapping_add(2)
                };
            } else {
                player.vx = if player.vx < 0 && lo < 0xE0 {
                    -0x0020
                } else {
                    player.vx.wrapping_sub(2)
                };
            }
        }
        2
    } else {
        let direction = (pad & player.walk_gate & Buttons::SIDEWAYS).bits();
        if direction == 0 {
            2
        } else {
            usize::from(direction >> 1)
        }
    }
}

/// The walk response after a platform or stop (`$83BE`), for motion 3 and 5.
fn landing_response(pad: Buttons, player: &mut Player) {
    if player.motion != Motion::Jumping && player.motion != Motion::Falling {
        return;
    }
    hit!(6);
    player.vx >>= 1;
    if pad.contains(Buttons::DOWN) {
        player.walk_timer = 0xFF;
        player.motion = Motion::Crouching;
        player.pose &= pose::SHOT;
        player.pose_timer = player.pose;
        player.pose_step = player.pose;
        player.anim = 0x04;
    } else if pad.intersects(Buttons::SIDEWAYS) {
        // Turn when the pressed direction is not the facing one.
        let mut toward = (pad & Buttons::SIDEWAYS).bits();
        toward = (toward >> 1) | ((toward & 1) << 7);
        toward >>= 1;
        if (toward ^ player.facing) & 0x80 == 0 {
            stop(player);
        } else {
            player.motion = Motion::Walking;
            player.pose_timer = 0;
            player.pose_step = 0;
            player.pose = (player.pose & pose::SHOT) | pose::LANDING;
            player.walk_timer = 0xFF;
            player.anim = 0x3E;
        }
    } else {
        stop(player);
    }
}

/// The stopped state (`$8403`).
fn stop(player: &mut Player) {
    player.pose_timer = 0;
    player.pose_step = 0;
    player.motion = Motion::Standing;
    if player.pose & pose::SHOT != 0 {
        player.pose = pose::SHOT;
        player.anim = 0x25;
    } else {
        player.pose = pose::LANDING | pose::LONG_LANDING;
        player.anim = 0x07;
    }
}

/// The airborne state (`$8268`); true when the carrier must be cleared.
fn air_state(player: &mut Player, work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) -> bool {
    match player.motion {
        Motion::Jumping => true,
        Motion::Sliding => {
            hit!(3);
            slide_off_ledge(player, work, probe, cx);
            true
        }
        _ if player.pose & pose::SHOT != 0 => {
            hit!(7);
            player.motion = Motion::Jumping;
            player.pose &= pose::SHOT;
            player.pose_timer = 0;
            player.pose_step = 5;
            player.anim = 0x1C;
            false
        }
        _ => {
            hit!(8);
            player.motion = Motion::Falling;
            player.pose_timer = 0;
            player.pose_step = 0;
            player.pose = 0;
            player.anim = 0x1F;
            true
        }
    }
}

/// `$8451`: the slide off a ledge: the probe that lands it or lets it fall.
fn slide_off_ledge(player: &mut Player, work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) {
    if player.pose_step & 0x80 != 0 {
        return;
    }
    work.vy = 0;
    player.vx = 0;
    work.ax = 0;
    player.sliding = 0;
    probe.x = work.x;
    probe.y = work.y.wrapping_add(0x0100);
    let solid = collision::solid(probe, 0x84, cx);
    player.pose_step = if solid & 0x80 != 0 { 0xC0 } else { 0x80 };
    player.pose_timer = 0;
    work.y = probe.y;
    player.anim = 0x3C;
}

/// Gravity on the working y velocity (`$8311-$8339`), written back to the player.
fn gravity(player: &mut Player, work: &Work) {
    let [hi, lo] = work.vy.to_be_bytes();
    let (lo, carry) = lo.overflowing_add(6);
    let hi = hi.wrapping_add(u8::from(carry));
    let (hi, lo) = if hi & 0x80 != 0 {
        (0xFF, if lo & 0x80 != 0 { lo } else { 0x80 })
    } else {
        (0x00, if lo & 0x80 == 0 { lo } else { 0x7F })
    };
    player.vy = i16::from_be_bytes([hi, lo]);
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

struct Port<'a, 'b> {
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

    /// The player's level y, low and high bytes.
    fn y(&self) -> (u8, u8) {
        let [hi, lo] = self.player.y.to_be_bytes();
        (lo, hi)
    }

    /// `$8E1F`-style screen x: the low byte of the projected `x - camera`.
    fn screen_x(&self, x: u16) -> u8 {
        (x.wrapping_sub(self.cx.world.camera_x) >> 4).to_le_bytes()[0]
    }

    /// `BIT`'s overflow: the flag takes bit 6 of `value`.
    fn set_overflow_from(&mut self, value: u8) {
        self.overflow = value & 0x40 != 0;
    }

    /// `$C93B`'s probe, which leaves `BIT $DD`'s overflow flag (the vertical-room bit).
    fn solid_at(&mut self, flags: u8) -> u8 {
        self.overflow = self.cx.world.orientation & orientation::VERTICAL != 0;
        collision::solid(&mut self.probe, flags, self.cx)
    }

    /// Is the metatile at `x`, `y_hi` solid (`$C93B` with flags `$80`)?
    fn solid_x(&mut self, x: u16, y_hi: u8) -> bool {
        self.probe.x = u16::from(x.to_be_bytes()[0]) << 8;
        self.probe.y = u16::from(y_hi) << 8;
        self.solid_at(0x80) & 0x80 != 0
    }

    /// `$9397`: the death, `$C2` and the music stopped, then `$D274`.
    fn die(&mut self) {
        self.cx.effects.scroll_lock = 0xFF;
        self.cx.effects.music = 0xFF;
        self.cx.effects.player = *self.player;
        engine::level_end(self.cx.effects);
        *self.player = self.cx.effects.player;
    }

    /// Bit 7 of the `asl` that tests the pad's direction against `$046F`.
    fn faces_press(&self) -> bool {
        self.pad.contains(Buttons::RIGHT) != ((self.player.facing & 0x40) != 0)
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
        let cam = self.cx.world.camera_x;
        let [xhi, xlo] = self.player.x.to_be_bytes();
        let (low, borrow) = xlo.overflowing_sub(cam.to_be_bytes()[1]);
        let hi = xhi
            .wrapping_sub(cam.to_be_bytes()[0])
            .wrapping_sub(u8::from(borrow));
        let mut sx = (low >> 4) | (hi << 4);
        if low & 0x08 != 0 {
            sx = sx.wrapping_add(1);
        }
        self.player.screen_x = sx;
        self.work.screen_x = sx;
        if sx < 0x14 {
            let push = (0x14u8.wrapping_sub(sx)).wrapping_mul(16);
            let [hi0, lo0] = self.player.x.to_be_bytes();
            let (lo, carry) = lo0.overflowing_add(push);
            self.player.x = u16::from_be_bytes([hi0.wrapping_add(u8::from(carry)), lo]);
            self.player.screen_x = 0x14;
            self.work.screen_x = 0x14;
            let [hi1, lo1] = self.player.x.to_be_bytes();
            let (_, carry) = lo1.overflowing_add(0x0C);
            if self.crush(hi1.wrapping_add(u8::from(carry))) {
                self.die();
                self.player.anim = 0x40;
            }
        } else if sx >= 0xE8 {
            let mag = sx.wrapping_sub(0xE7).wrapping_mul(16);
            let [hi0, lo0] = self.player.x.to_be_bytes();
            let (lo, carry) = lo0.overflowing_add(mag.wrapping_neg());
            self.player.x =
                u16::from_be_bytes([hi0.wrapping_add(0xFF).wrapping_add(u8::from(carry)), lo]);
            self.player.screen_x = 0xE7;
            self.work.screen_x = 0xE7;
            let [hi1, lo1] = self.player.x.to_be_bytes();
            let (_, borrow) = lo1.overflowing_sub(0xC0);
            if self.crush(hi1.wrapping_sub(u8::from(borrow))) {
                self.die();
                self.player.anim = 0x40;
            }
        }
        let (ylo, yhi) = self.y();
        let camy = self.cx.world.camera_y;
        let (dy_lo, borrow) = ylo.overflowing_sub(camy.to_be_bytes()[1]);
        let dy_hi = yhi
            .wrapping_sub(camy.to_be_bytes()[0])
            .wrapping_sub(u8::from(borrow));
        self.overflow = sub_overflows16(self.player.y, camy);
        if dy_hi >= 0x0F {
            self.die();
            self.player.stage_script = stage_script::FROZEN | stage_script::HIDDEN;
            self.player.death_flags = 0x80;
            return;
        }
        let mut sy = (dy_lo >> 4) | (dy_hi << 4);
        self.overflow = adc_overflows(sy, 0, dy_lo & 0x01 != 0);
        if dy_lo & 0x01 != 0 {
            sy = sy.wrapping_add(1);
        }
        self.player.screen_y = sy;
        self.work.screen_y = sy;
        self.work.anchor = if sy >= 0xF0 { 0x40 } else { 0 };
        self.draw();
    }

    /// `$9425`: stops the pushed player and runs the three crush probes at
    /// `y-0x140`, `y-0x80` and `y-0x200`.
    fn crush(&mut self, x_hi: u8) -> bool {
        self.player.vx = 0;
        let (ylo, yhi) = self.y();
        let ys = [
            yhi.wrapping_sub(1).wrapping_sub(u8::from(ylo < 0x40)),
            yhi.wrapping_sub(u8::from(ylo < 0x80)),
            yhi.wrapping_sub(2),
        ];
        ys.into_iter().any(|y| {
            self.probe.x = u16::from(x_hi) << 8;
            self.probe.y = u16::from(y) << 8;
            self.solid_at(0x80) & 0x80 != 0
        })
    }

    /// `$94A6`: the draw, its overlay and the hurt flash.
    fn draw(&mut self) {
        self.set_overflow_from(self.player.hurt);
        if self.player.hurt & hurt::STUNNED == 0 && self.player.hurt & hurt::INVULNERABLE != 0 {
            self.player.hurt_timer = self.player.hurt_timer.wrapping_sub(1);
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
            self.player.flash_timer = self.player.flash_timer.wrapping_add(1);
            if self.player.flash_timer.trailing_zeros() >= 6 {
                self.overflow = adc_overflows(self.cx.effects.meter, !2, true);
                self.cx.effects.meter = self.cx.effects.meter.wrapping_sub(2);
                if self.cx.effects.meter & 0x80 != 0 {
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
                        projectile.kind = 0;
                    }
                    let palette = self.cx.prg.player.weapon_palette;
                    for i in 0..3u16 {
                        self.cx.effects.palette[0x11 + usize::from(i)] = palette[usize::from(i)];
                    }
                    set_first_colours(&mut self.cx.effects.palette, 0x0F);
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
            let add = if self.player.facing & 0x40 != 0 {
                0xFE
            } else {
                0x02
            };
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
            let add = if self.player.facing & 0x40 != 0 {
                player.muzzle_x_left[motion]
            } else {
                player.muzzle_x_right[motion]
            };
            let (x, carry) = self.work.screen_x.overflowing_add(add);
            self.overflow = adc_overflows(add, self.work.screen_x, false);
            self.work.screen_x = x;
            if carry != (self.player.facing & 0x40 != 0) {
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
        self.player.hurt_timer = self.player.hurt_timer.wrapping_sub(1);
        if self.player.hurt_timer & 0x80 != 0 {
            self.set_overflow_from(self.player.energy);
            if self.player.energy & 0x80 != 0 {
                self.die();
                return;
            }
            self.player.pose_timer = 0;
            self.player.pose_step = 0;
            self.player.motion = Motion::Standing;
            self.player.pose = 0;
            self.player.hurt = hurt::INVULNERABLE;
            self.player.hurt_timer = 0x40;
            self.player.facing &= 0x40;
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
        let mut x = self.player.facing & 0x40;
        if self.cx.world.frame & 0x04 == 0 {
            x |= 0x03;
        }
        self.player.facing = x;
    }

    /// `$92E8`: the frame Batman spends stunned while knocked back, which masks `$F5`/`$F6`
    /// in place.
    fn stunned(&mut self) {
        if self.player.hurt & hurt::STUNNED == 0 {
            return;
        }
        self.player.hurt_timer = self.player.hurt_timer.wrapping_sub(1);
        if self.player.hurt_timer & 0x80 != 0 {
            if self.player.energy & 0x80 != 0 {
                self.die();
                return;
            }
            self.player.hurt = hurt::INVULNERABLE;
            self.player.hurt_timer = 0x40;
            self.player.facing &= 0x40;
            return;
        }
        self.cx.effects.attack &= attack::CHARGEABLE;
        // `ASL` of `$B8 & $80` shifts the bit out: both always get 0.
        self.player.pose = 0;
        self.player.walk_gate = Buttons::NONE;
        self.pressed &= Buttons::START;
        self.pad &= Buttons::START;
        self.player.facing = if self.cx.world.frame & 0x04 != 0 {
            self.player.facing & 0x40
        } else {
            (self.player.facing & 0x40) | 0x03
        };
    }

    /// `$92B4`: down pressed: the crouch pose, which the next frame turns into crouching.
    fn start_crouch(&mut self) {
        self.set_overflow_from(self.player.pose);
        if self.player.pose & pose::TURNING != 0 {
            self.player.facing ^= 0x40;
        }
        self.player.anim = 0x01;
        self.player.pose =
            (self.player.pose & pose::SHOT) | pose::RESUME | Motion::Crouching.byte();
    }

    /// `$92D1`: the solid probe two rows above the player, the headroom check for standing
    /// from a crouch or slide.
    fn headroom_blocked(&mut self) -> bool {
        let (ylo, yhi) = self.y();
        let y = yhi.wrapping_sub(2).wrapping_sub(u8::from(ylo < 0x80));
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

    /// The `$B8`-gated projectile spawn most projectile poses share (`$866C`, `$88A7`, ...).
    fn reset_and_shoot(&mut self) {
        if self.cx.effects.attack & attack::CHARGEABLE == 0 {
            self.cx.effects.attack = 0;
            self.player.charge_timer = 0;
        } else if self.cx.effects.attack & attack::CHARGED == 0 {
            self.cx.effects.attack = attack::CHARGEABLE | attack::CHARGING;
            self.player.charge_timer = 0;
        }
        self.spawn_shot();
    }

    /// `$9AF5`: fires the player's projectile into the first free `$7BD0` slot.
    fn spawn_shot(&mut self) {
        let weapon = self.cx.effects.weapon;
        let start = self.cx.prg.player.slot_limits[usize::from(weapon.byte())];
        let Some(slot) = (0..=usize::from(start))
            .rev()
            .find(|&slot| self.cx.effects.projectiles[slot].kind == 0)
        else {
            return;
        };
        let motion = usize::from(self.player.motion.byte());
        let aim_up = self.player.motion == Motion::AimingUp;
        let face_left = self.player.facing & 0x40 != 0;
        let add = |a_hi: u8, a_lo: u8, b_hi: u8, b_lo: u8| {
            let (lo, carry) = a_lo.overflowing_add(b_lo);
            let hi = a_hi.wrapping_add(b_hi).wrapping_add(u8::from(carry));
            (hi, lo, (a_hi, b_hi, carry))
        };
        let (x_hi, x_lo, x_adc) = {
            let [hi, lo] = self.player.x.to_be_bytes();
            if aim_up {
                if face_left {
                    add(0xFF, 0xE0, hi, lo)
                } else {
                    add(0x00, 0x20, hi, lo)
                }
            } else {
                let offset = if face_left {
                    self.cx.prg.player.launch_x_left[motion]
                } else {
                    self.cx.prg.player.launch_x_right[motion]
                };
                let [off_hi, off_lo] = offset.to_be_bytes();
                add(off_hi, off_lo, hi, lo)
            }
        };
        let (y_hi, y_lo, y_adc) = {
            let [hi, lo] = self.player.y.to_be_bytes();
            if aim_up {
                add(0xFB, 0xF0, hi, lo)
            } else {
                let [off_hi, off_lo] = self.cx.prg.player.launch_y[motion].to_be_bytes();
                add(off_hi, off_lo, hi, lo)
            }
        };
        let (a_hi, b_hi, carry) = if aim_up { y_adc } else { x_adc };
        self.overflow = adc_overflows(a_hi, b_hi, carry);
        let projectile = &mut self.cx.effects.projectiles[slot];
        projectile.kind = (weapon.byte() + 1) << 1;
        projectile.level_x_hi = x_hi;
        projectile.level_x_lo = x_lo;
        projectile.level_y_hi = y_hi;
        projectile.level_y_lo = y_lo;
        if aim_up {
            projectile.attr = 0;
        }
        assert_ne!(
            weapon,
            Weapon::PowerUpPieces,
            "$9AF5 has no handler for the pieces"
        );
        let table = if aim_up { 0x9EB5 } else { 0x9EC1 };
        let handler = self.rw(table + u16::from(weapon.byte()) * 2);
        #[cfg(test)]
        super::oracle::seen(handler);
        self.cx.effects.sound[0] = match handler {
            0x9C06 | 0x9D6F => 0x2F,
            0x9C2E | 0x9CD9 | 0x9DA0 | 0x9E72 => 0x43,
            0x9C7E | 0x9E09 => 0x29,
            _ => 0x2D,
        };
        if aim_up {
            self.shoot_up(slot, handler);
        } else {
            // The `$9DA0`/`$9E09`/`$9E72` handlers test the facing with `BIT $046F`.
            if matches!(handler, 0x9DA0 | 0x9E09 | 0x9E72) {
                self.set_overflow_from(self.player.facing);
            }
            self.shoot_ahead(slot, handler, face_left);
        }
    }

    /// The upward projectile handlers `$9EB5[$0476]` (`$9BCE`-`$9D0C`).
    fn shoot_up(&mut self, slot: usize, handler: u16) {
        let fan = usize::from((self.cx.world.frame >> 2) & 0x0F);
        match handler {
            0x9BCE => self.shot_up(slot, -0x40, 1),
            0x9BF5 => {
                self.shot_up(slot, -0x20, 2);
                self.cx.effects.projectiles[slot].param = self.target();
            }
            0x9C06 => self.shot_up(slot, -0x10, 2),
            0x9C2E => {
                let vx = self.cx.prg.player.pair_velocities;
                let target = vx.map(|v| v.to_be_bytes()[0]);
                let part = |projectile: &mut Projectile, y: usize| {
                    projectile.vy = -0x40;
                    projectile.timer = 0xFF;
                    projectile.vx = vx[y];
                    projectile.state = 0;
                    projectile.flags = 0;
                    projectile.param = target[y];
                    projectile.damage = 2;
                };
                part(&mut self.cx.effects.projectiles[slot], 1);
                let source = self.cx.effects.projectiles[slot];
                self.spawn_parts(1, |projectile, _| {
                    part(projectile, 0);
                    projectile.kind = 1;
                    at_source(projectile, &source);
                });
            }
            0x9C7E => {
                let vx = self.cx.prg.player.threeway_up_velocities;
                let part = |projectile: &mut Projectile, n: usize| {
                    projectile.vy = -0x40;
                    projectile.vx = vx[n];
                    projectile.timer = 0x80;
                    projectile.state = 0;
                    projectile.attr = 0;
                    projectile.flags = 0;
                    projectile.kind = 0x0A;
                    projectile.damage = 1;
                };
                part(&mut self.cx.effects.projectiles[slot], 2);
                let source = self.cx.effects.projectiles[slot];
                self.spawn_parts(2, |projectile, n| {
                    part(projectile, n);
                    at_source(projectile, &source);
                });
            }
            0x9CD9 => {
                let vx = self.cx.prg.player.super_fan_velocities[fan];
                let projectile = &mut self.cx.effects.projectiles[slot];
                projectile.vy = -0x40;
                projectile.vx = vx;
                projectile.timer = 0x80;
                projectile.state = 0;
                projectile.flags = 0;
                projectile.damage = 4;
            }
            other => panic!("${other:04X} is not an upward projectile handler"),
        }
    }

    /// The forward projectile handlers `$9EC1[$0476]` (`$9D2D`-`$9EB4`).
    fn shoot_ahead(&mut self, slot: usize, handler: u16, face_left: bool) {
        let right = usize::from(!face_left);
        let left = usize::from(face_left);
        let fan = usize::from((self.cx.world.frame >> 2) & 0x0F);
        match handler {
            0x9D2D => {
                let (attr, vx) = (
                    self.cx.prg.player.shot_attributes[right],
                    self.cx.prg.player.bolt_launch[right],
                );
                let projectile = &mut self.cx.effects.projectiles[slot];
                projectile.attr = attr;
                projectile.vx = vx;
                shot_ahead(projectile, 1);
            }
            0x9D4A => {
                let vx = self.cx.prg.player.homing_launch[right];
                let target = self.target();
                let projectile = &mut self.cx.effects.projectiles[slot];
                projectile.vx = vx;
                projectile.param = target;
                shot_ahead(projectile, 1);
            }
            0x9D6F => {
                let (attr, vx) = (
                    self.cx.prg.player.shot_attributes[right],
                    self.cx.prg.player.splitter_launch[right],
                );
                let projectile = &mut self.cx.effects.projectiles[slot];
                projectile.attr = attr;
                projectile.vx = vx;
                shot_ahead(projectile, 2);
            }
            0x9DA0 => {
                let vy = self.cx.prg.player.pair_velocities;
                let target = vy.map(|v| v.to_be_bytes()[0]);
                let (attr, vx) = (
                    self.cx.prg.player.shot_attributes[right],
                    self.cx.prg.player.wave_launch[right],
                );
                let part = |projectile: &mut Projectile, y: usize| {
                    projectile.vy = vy[y];
                    projectile.param = target[y];
                    projectile.damage = 2;
                    projectile.state = 0;
                    projectile.timer = 0;
                    projectile.flags = 0;
                    projectile.attr = attr;
                    projectile.vx = vx;
                };
                part(&mut self.cx.effects.projectiles[slot], 1);
                let source = self.cx.effects.projectiles[slot];
                self.spawn_parts(1, |projectile, _| {
                    part(projectile, 0);
                    projectile.kind = 1;
                    at_source(projectile, &source);
                });
            }
            0x9E09 => {
                let (attr, vx) = (
                    self.cx.prg.player.shot_attributes[left],
                    self.cx.prg.player.pair_velocities[left],
                );
                let vy = self.cx.prg.player.threeway_ahead_velocities;
                let part = |projectile: &mut Projectile, n: usize| {
                    projectile.vx = vx;
                    projectile.attr = attr;
                    projectile.kind = 0x0A;
                    projectile.vy = vy[n];
                    projectile.timer = 0x80;
                    projectile.state = 0;
                    projectile.flags = 0;
                    projectile.damage = 1;
                };
                part(&mut self.cx.effects.projectiles[slot], 2);
                let source = self.cx.effects.projectiles[slot];
                self.spawn_parts(2, |projectile, n| {
                    part(projectile, n);
                    at_source(projectile, &source);
                });
            }
            0x9E72 => {
                let vy = self.cx.prg.player.super_fan_velocities[fan];
                let (attr, vx) = (
                    self.cx.prg.player.shot_attributes[left],
                    self.cx.prg.player.pair_velocities[left],
                );
                let projectile = &mut self.cx.effects.projectiles[slot];
                projectile.vy = vy;
                projectile.state = 0;
                projectile.timer = 0;
                projectile.flags = 0;
                projectile.vx = vx;
                projectile.attr = attr;
                projectile.damage = 4;
            }
            other => panic!("${other:04X} is not a forward projectile handler"),
        }
    }

    /// The shared tail of the upward shots (`$9BCE`, `$9C0E`): `vy` up, no x motion.
    fn shot_up(&mut self, slot: usize, vy: i16, damage: u8) {
        let projectile = &mut self.cx.effects.projectiles[slot];
        projectile.vy = vy;
        projectile.timer = 0x80;
        projectile.flags = 0;
        projectile.vx = 0;
        projectile.state = 0;
        projectile.damage = damage;
    }

    /// Fills the free slots below 16, highest first, with the parts numbered
    /// `last - 1` down to 0 (`$9C39`, `$9C8D`, `$9AAD`). Returns the X it leaves:
    /// part 0's slot, or `$FF` when the slots ran out.
    fn spawn_parts(&mut self, last: usize, mut part: impl FnMut(&mut Projectile, usize)) -> u8 {
        let mut free = (0..16u8)
            .rev()
            .filter(|&slot| self.cx.effects.projectiles[usize::from(slot)].kind == 0)
            .collect::<Vec<_>>()
            .into_iter();
        let mut left = 0xFF;
        for n in (0..last).rev() {
            let Some(slot) = free.next() else {
                return 0xFF;
            };
            part(&mut self.cx.effects.projectiles[usize::from(slot)], n);
            left = slot;
        }
        left
    }

    /// `$9A56`: the charged attack, a type `$80` part (four for weapon 2) in the
    /// first free `$7BD0` slot. Returns the X it leaves, the last part's slot or `$FF`.
    fn spawn_melee(&mut self) -> u8 {
        let Some(slot) = (0..16)
            .rev()
            .find(|&slot| self.cx.effects.projectiles[slot].kind == 0)
        else {
            return 0xFF;
        };
        let left = u8::try_from(slot).expect("slot below 16");
        let weapon = self.cx.effects.weapon;
        assert_ne!(
            weapon,
            Weapon::PowerUpPieces,
            "$9A56 has no handler for the pieces"
        );
        let charge = |projectile: &mut Projectile| {
            projectile.kind = super::weapons::CHARGE_START;
            projectile.part_timer = 0;
            projectile.flags = 0xE0;
        };
        let handler = self.rw(0x9A75 + u16::from(weapon.byte()) * 2);
        #[cfg(test)]
        super::oracle::seen(handler);
        match handler {
            0x9A81 => {
                self.cx.effects.sound[0] = 0x2E;
                let projectile = &mut self.cx.effects.projectiles[slot];
                charge(projectile);
                projectile.param = 0;
                projectile.part = 0;
                projectile.damage = 0;
                projectile.vx = i16::from_be_bytes([0, projectile.vx.to_be_bytes()[1]]);
            }
            0x9AA0 => {
                self.cx.effects.sound[0] = 0x30;
                let orbit = |projectile: &mut Projectile, n: usize| {
                    charge(projectile);
                    projectile.part = u8::try_from(n).expect("four parts");
                    projectile.param = 0;
                    projectile.vx = 0;
                    projectile.vy = 0;
                };
                orbit(&mut self.cx.effects.projectiles[slot], 3);
                return self.spawn_parts(3, orbit);
            }
            0x9AE3 => {
                let projectile = &mut self.cx.effects.projectiles[slot];
                charge(projectile);
                projectile.damage = 0;
            }
            other => panic!("${other:04X} is not a charged attack handler"),
        }
        left
    }

    /// `$9F05`: a live, hittable actor slot below 16 from a random start, or `$FF`.
    fn target(&self) -> u8 {
        let [direction, start] = self.cx.world.rng;
        let mut slot = start & 0x0F;
        for _ in 0..16 {
            let actor = &self.cx.slots.main[usize::from(slot)];
            if actor.kind != 0
                && actor.body.contact & 0x20 == 0
                && actor.body.state & STATE_UNTARGETED == 0
            {
                return slot;
            }
            slot = if direction & 0x80 != 0 {
                (slot + 1) & 0x0F
            } else {
                slot.wrapping_sub(1) & 0x0F
            };
        }
        0xFF
    }

    /// `$9359`: the fire button's spawn and muzzle handling.
    fn fire(&mut self) {
        if self.cx.world.frame.trailing_zeros() >= 2 {
            self.spawn_shot();
        }
        if self.cx.effects.attack & attack::CHARGING != 0 {
            let held = self.pad.contains(Buttons::B);
            self.set_overflow_from(self.pad.bits());
            if self.cx.effects.attack & attack::CHARGED != 0 {
                if held {
                    return;
                }
                self.spawn_melee();
            } else if held {
                self.player.charge_timer = self.player.charge_timer.wrapping_add(1);
                if self.player.charge_timer >= 0x30 {
                    self.cx.effects.attack =
                        attack::CHARGEABLE | attack::CHARGING | attack::CHARGED;
                }
                return;
            }
            self.cx.effects.attack = attack::CHARGEABLE;
        }
        self.set_overflow_from(self.pressed.bits());
        if self.pressed.contains(Buttons::B) {
            self.cx.effects.attack = attack::CHARGEABLE | attack::CHARGING;
            self.player.charge_timer = 0;
        }
    }

    /// `$8508`: whether a solid tile blocks the jump's launch (probes at x, x+$A0, x-$40).
    fn jump_blocked(&mut self) -> bool {
        let (ylo, yhi) = self.y();
        let y = yhi.wrapping_sub(3).wrapping_sub(u8::from(ylo < 0x80));
        let x1 = self.player.x.wrapping_add(0xA0);
        let x2 = x1.wrapping_sub(0x0140);
        self.solid_x(self.player.x, y) || self.solid_x(x1, y) || self.solid_x(x2, y)
    }

    /// `$84D1`: motion 0, standing.
    fn standing(&mut self) {
        self.set_overflow_from(self.player.hurt);
        if self.player.hurt & hurt::STUNNED != 0 {
            self.hurt();
            return;
        }
        self.player.walk_gate = Buttons::SIDEWAYS;
        if self.player.pose & pose::RESUME != 0 {
            self.standing_resume();
        } else {
            self.standing_input();
        }
    }

    /// `$84E8`: the standing player resumes from a saved motion.
    fn standing_resume(&mut self) {
        self.player.anim = 0x01;
        self.player.motion =
            Motion::from_byte(self.player.pose & pose::SAVED_MOTION).expect("a saved motion");
        self.player.pose &= pose::SHOT;
        if self.player.motion == Motion::Jumping {
            self.cx.effects.sound[0] = 0x24;
            if self.jump_blocked() {
                self.cx.effects.attack &= attack::CHARGEABLE;
                self.player.pose = pose::LANDING | pose::LONG_LANDING;
            } else {
                self.player.vy = i16::from_be_bytes([0xFF, 0xD4]);
                self.set_vx_from_pad();
                self.set_overflow_from(self.player.pose);
                if self.player.pose & pose::SHOT != 0 {
                    self.player.anim = 0x27;
                }
            }
        }
        self.player.pose_timer = 0;
        self.player.pose_step = 0;
    }

    /// `$857F`: standing with no saved state.
    fn standing_input(&mut self) {
        self.set_overflow_from(self.pressed.bits());
        if !self.pressed.contains(Buttons::A) && self.pad.contains(Buttons::UP) {
            self.player.motion = Motion::AimingUp;
            self.player.pose &= pose::SHOT;
            self.player.pose_timer = 0;
            self.player.pose_step = 0;
            self.player.anim = 0x45;
            return;
        }
        if self.pressed.contains(Buttons::A) {
            self.standing_jump();
        } else if self.pressed.contains(Buttons::B) {
            self.standing_shot();
        } else if self.pad.intersects(Buttons::SIDEWAYS) {
            self.standing_walk();
        } else if self.pad.contains(Buttons::DOWN) {
            self.start_crouch();
        } else {
            self.set_overflow_from(self.player.pose);
            if self.player.pose & pose::TURNING != 0 {
                self.standing_turn();
            } else if self.player.pose & pose::SHOT == 0 {
                self.standing_idle();
            } else {
                self.standing_attack();
            }
        }
    }

    /// `$862F`: A while standing: the jump starts next frame (the resume flag, motion 3).
    fn standing_jump(&mut self) {
        self.set_overflow_from(self.player.pose);
        if self.player.pose & pose::TURNING != 0 {
            self.player.facing ^= 0x40;
        }
        self.player.anim = if self.player.pose & pose::SHOT != 0 {
            0x27
        } else {
            0x01
        };
        self.player.pose = (self.player.pose & pose::SHOT) | pose::RESUME | Motion::Jumping.byte();
    }

    /// `$8655`: B while standing: the attack, turning first if the pose says so.
    fn standing_shot(&mut self) {
        if self.player.pose & pose::TURNING != 0 {
            self.player.pose &= !pose::TURNING;
            self.player.facing ^= 0x40;
        }
        self.set_overflow_from(self.player.pose);
        if self.player.pose & pose::SHOT != 0 {
            self.reset_and_shoot();
            self.player.attack_timer = 2;
            self.player.anim = 0x25;
        } else {
            self.player.pose = pose::SHOT;
            self.player.attack_timer = 0;
            self.player.anim = 0x24;
        }
    }

    /// `$869C`: direction pressed while standing.
    fn standing_walk(&mut self) {
        if self.faces_press() {
            self.player.pose = if self.player.pose & pose::SHOT != 0 {
                0x40
            } else {
                0x10
            };
            self.player.pose_timer = 0;
            self.player.pose_step = 0;
            self.player.motion = Motion::Walking;
            self.player.anim = 0x3E;
        } else if self.player.pose & pose::TURNING != 0 {
            self.standing_turn();
        } else {
            self.player.pose = (self.player.pose & pose::SHOT) | pose::TURNING;
            self.player.anim = 0x02;
            self.player.pose_timer = 0;
        }
    }

    /// `$86DE`.
    fn standing_turn(&mut self) {
        self.player.pose_timer = self.player.pose_timer.wrapping_add(1);
        let x = u16::from(self.player.pose_timer >> 1);
        if x < 2 {
            self.player.anim = self.cx.prg.player.animations.standing_turn[usize::from(x)];
            return;
        }
        self.player.pose = if self.player.pose & pose::SHOT != 0 {
            0x40
        } else {
            0x11
        };
        self.player.pose_timer = 2;
        self.player.anim = 0x07;
        self.player.facing ^= 0x40;
    }

    /// `$85C9`: standing idle, cycling the stand animation.
    fn standing_idle(&mut self) {
        let pose_bits = self.player.pose & (pose::LANDING | pose::LONG_LANDING);
        if pose_bits != 0 {
            self.player.pose_timer = self.player.pose_timer.wrapping_add(1);
            if pose_bits & pose::LONG_LANDING != 0 {
                let x = u16::from(self.player.pose_timer >> 1);
                if x < 0x11 {
                    self.player.anim = self.cx.prg.player.animations.long_landing[usize::from(x)];
                    return;
                }
            } else if self.player.pose_timer < 0x05 {
                self.player.anim =
                    self.cx.prg.player.animations.landing[usize::from(self.player.pose_step)];
                return;
            } else {
                self.player.pose_timer = 0;
                self.player.pose_step = self.player.pose_step.wrapping_add(1);
                if self.player.pose_step < 0x03 {
                    self.player.anim =
                        self.cx.prg.player.animations.landing[usize::from(self.player.pose_step)];
                    return;
                }
            }
            self.player.pose = 0;
            self.player.pose_timer = 0;
            self.player.pose_step = 0;
        }
        if self.player.vx == 0 {
            self.player.anim = 0;
            return;
        }
        self.player.pose_timer = self.player.pose_timer.wrapping_add(1);
        let x = u16::from(self.player.pose_timer >> 1);
        if x < 3 {
            self.player.anim = self.cx.prg.player.animations.standing_drift[usize::from(x)];
            return;
        }
        self.player.pose_timer = self.player.pose_timer.wrapping_sub(1);
        self.player.anim = 0x44;
    }

    /// `$8710`: the standing attack (the pose's attack flag set).
    fn standing_attack(&mut self) {
        self.set_overflow_from(self.player.power_up);
        if self.player.power_up & power_up::ON != 0 {
            self.fire();
            self.player.anim = 0x25;
            return;
        }
        match self.player.attack_timer {
            0 => {
                self.player.attack_timer = 1;
                return;
            }
            1 => {
                self.cx.effects.attack = if self.cx.effects.attack & attack::CHARGEABLE != 0 {
                    attack::CHARGEABLE | attack::CHARGING
                } else {
                    0
                };
                self.player.charge_timer = 0;
                self.spawn_shot();
                self.player.anim = 0x25;
                self.player.attack_timer = self.player.attack_timer.wrapping_add(1);
                return;
            }
            _ => {}
        }
        self.set_overflow_from(self.cx.effects.attack);
        if self.cx.effects.attack & attack::CHARGING != 0 {
            self.melee_block();
            if self.player.attack_timer >= 0x14 {
                self.player.attack_timer = 0x13;
            }
        }
        self.player.anim = match self.player.attack_timer {
            0..0x14 => 0x25,
            0x14..0x16 => 0x24,
            _ => {
                self.player.pose &= !pose::SHOT;
                0
            }
        };
        self.player.attack_timer = self.player.attack_timer.wrapping_add(1);
    }

    /// The shared `$B8`-gated melee block (`$8743`, `$882F`, `$8AEA`, ...).
    /// Returns the X `$9A56` left when it ran, which walking's `$8B2B` indexes `$96FD` with.
    fn melee_block(&mut self) -> Option<u8> {
        if self.cx.effects.attack & attack::CHARGING == 0 {
            return None;
        }
        self.set_overflow_from(self.pad.bits());
        if !self.pad.contains(Buttons::B) {
            self.cx.effects.attack = attack::CHARGEABLE;
            return None;
        }
        if self.cx.effects.attack & attack::CHARGED != 0 {
            return None;
        }
        self.player.charge_timer = self.player.charge_timer.wrapping_add(1);
        if self.player.charge_timer < 0x30 {
            return None;
        }
        let left = self.spawn_melee();
        self.cx.effects.attack |= attack::CHARGED;
        Some(left)
    }

    /// `$8795`: motion 1.
    fn crouching(&mut self) {
        self.stunned();
        self.player.walk_gate = Buttons::NONE;
        self.set_overflow_from(self.pressed.bits());
        if self.pressed.contains(Buttons::A) {
            if self.pad.contains(Buttons::DOWN) {
                self.crouching_slide();
            } else {
                self.crouching_release();
            }
        } else if self.pressed.contains(Buttons::B) {
            self.crouching_shot();
        } else if self.pad.contains(Buttons::DOWN) {
            self.crouching_held();
        } else {
            self.crouching_release();
        }
    }

    /// `$87A9`/`$87AE`: the down-held direction and turn handling.
    fn crouching_held(&mut self) {
        self.set_overflow_from(self.player.pose);
        if self.player.pose & pose::TURNING != 0 {
            self.player.pose_timer = self.player.pose_timer.wrapping_add(1);
            let x = u16::from(self.player.pose_timer >> 1);
            if x < 2 {
                self.player.anim = self.cx.prg.player.animations.crouching_turn[usize::from(x)];
                return;
            }
            self.player.pose &= pose::SHOT;
            self.player.pose_timer = 0;
            self.player.facing ^= 0x40;
        }
        if self.pad.intersects(Buttons::SIDEWAYS) {
            self.crouching_turn();
        } else {
            self.crouching_attack();
        }
    }

    /// `$8819`: crouching's attack.
    fn crouching_attack(&mut self) {
        self.set_overflow_from(self.player.power_up);
        if self.player.power_up & power_up::ON != 0 {
            self.fire();
            self.player.anim = 0x26;
            return;
        }
        self.set_overflow_from(self.player.pose);
        if self.player.pose & pose::SHOT == 0 {
            self.player.anim = 0x04;
            return;
        }
        self.player.attack_timer = self.player.attack_timer.wrapping_add(1);
        self.set_overflow_from(self.cx.effects.attack);
        if self.cx.effects.attack & attack::CHARGING != 0 {
            self.melee_block();
            if self.player.attack_timer >= 0x14 {
                self.player.attack_timer = 0x13;
            }
        }
        if self.player.attack_timer >= 0x14 {
            self.player.pose &= !pose::SHOT;
            self.player.anim = 0x04;
        } else {
            self.player.anim = 0x26;
        }
    }

    /// `$887C`: turning while crouched, or the attack if facing the press.
    fn crouching_turn(&mut self) {
        if self.faces_press() {
            self.crouching_attack();
            return;
        }
        self.player.pose = (self.player.pose & pose::SHOT) | pose::TURNING;
        self.player.pose_timer = 0;
        self.player.anim = 0x05;
    }

    /// `$87EB`: crouching's idle.
    fn crouching_release(&mut self) {
        if self.headroom_blocked() {
            self.crouching_held();
            return;
        }
        self.set_overflow_from(self.player.pose);
        if self.player.pose & pose::TURNING != 0 {
            self.player.facing ^= 0x40;
        }
        self.player.pose = if self.player.pose & pose::SHOT != 0 {
            0x40
        } else {
            0x11
        };
        self.player.pose_timer = 0;
        self.player.motion = Motion::Standing;
        self.player.anim = 0x01;
    }

    /// `$889A`: motion 1 + the second pad shoulder.
    fn crouching_shot(&mut self) {
        self.set_overflow_from(self.player.pose);
        if self.player.pose & pose::TURNING != 0 {
            self.player.facing ^= 0x40;
        }
        self.reset_and_shoot();
        self.player.attack_timer = 0;
        self.player.pose = (self.player.pose & !pose::TURNING) | pose::SHOT;
        self.player.anim = 0x26;
    }

    /// `$88D3`: motion 1 + the first pad shoulder + down.
    fn crouching_slide(&mut self) {
        self.set_overflow_from(self.player.pose);
        if self.player.pose & pose::TURNING != 0 {
            self.player.facing ^= 0x40;
        }
        self.player.anim = 0x38;
        self.player.motion = Motion::Sliding;
        self.player.pose = 0;
        self.player.pose_timer = 0;
        self.player.pose_step = 0;
        self.cx.effects.attack &= attack::CHARGEABLE;
    }

    /// `$88FC`: motion 2.
    fn walking(&mut self) {
        self.set_overflow_from(self.player.hurt);
        if self.player.hurt & hurt::STUNNED != 0 {
            self.hurt();
            return;
        }
        self.player.walk_gate = Buttons::SIDEWAYS;
        if self.player.pose & pose::RESUME != 0 {
            self.player.motion = Motion::Crouching;
            self.player.pose &= pose::SHOT;
            self.player.pose_timer = 0;
            self.player.pose_step = 0;
            self.player.anim = 0x01;
            return;
        }
        self.walking_input();
    }

    /// `$892E`: the walk's pad: aim up, jump, fire, attack, walk on or stop.
    fn walking_input(&mut self) {
        self.set_overflow_from(self.pressed.bits());
        if !self.pressed.contains(Buttons::A) && self.pad.contains(Buttons::UP) {
            self.player.motion = Motion::AimingUp;
            self.player.pose &= pose::SHOT;
            self.player.pose_timer = 0;
            self.player.pose_step = 0;
            self.player.anim = 0x45;
            return;
        }
        if self.pressed.contains(Buttons::A) {
            self.walking_jump();
        } else if self.pressed.contains(Buttons::B) {
            self.walking_shot();
        } else if self.pad.contains(Buttons::DOWN) {
            self.start_crouch();
        } else if self.pad.intersects(Buttons::SIDEWAYS) {
            self.walking_step();
        } else {
            self.walking_stop();
        }
    }

    /// `$8968`: motion 2 with no direction held.
    fn walking_stop(&mut self) {
        self.set_overflow_from(self.player.pose);
        if self.player.pose & pose::TURNING != 0 {
            self.player.facing ^= 0x40;
        }
        self.player.pose_timer = 0;
        self.player.pose_step = 0;
        self.player.motion = Motion::Standing;
        if self.player.pose & pose::SHOT != 0 {
            self.player.pose = pose::SHOT;
            self.player.anim = 0x25;
        } else {
            self.player.pose = if self.player.vx == 0 {
                pose::LANDING
            } else {
                0
            };
            self.player.anim = 0x09;
        }
    }

    /// `$89A8`: motion 2 + the first pad shoulder (a side-jump).
    fn walking_jump(&mut self) {
        self.cx.effects.sound[0] = 0x24;
        if self.player.pose & pose::TURNING != 0 {
            self.player.pose &= !pose::TURNING;
            self.player.facing ^= 0x40;
        }
        if self.jump_blocked() {
            // `$8A07`: `JMP $895F`, walking's direction dispatch.
            if self.pad.intersects(Buttons::SIDEWAYS) {
                self.walking_step();
            } else {
                self.walking_stop();
            }
            return;
        }
        self.player.vy = i16::from_be_bytes([0xFF, 0xD4]);
        self.set_vx_from_pad();
        self.player.motion = Motion::Jumping;
        self.player.pose &= pose::SHOT;
        self.player.pose_timer = 0;
        self.player.pose_step = 0;
        self.player.anim = 0x1C;
    }

    /// `$8B32`: motion 2 + the second pad shoulder.
    fn walking_shot(&mut self) {
        if self.player.pose & pose::TURNING != 0 {
            self.player.pose &= !pose::TURNING;
            self.player.facing ^= 0x40;
        }
        self.reset_and_shoot();
        self.set_overflow_from(self.player.pose);
        if self.player.pose & pose::SHOT != 0 {
            self.player.attack_timer = 4;
            return;
        }
        self.player.attack_timer = 0;
        self.player.pose = (self.player.pose & !pose::TURNING) | pose::SHOT;
        self.walking_pose();
    }

    /// `$8A32`: motion 2 with a direction held.
    fn walking_step(&mut self) {
        if self.faces_press() {
            self.player.pose_step = self.player.pose_step.wrapping_add(1);
            if self.player.pose & pose::LANDING != 0 {
                if self.player.pose_step < 6 {
                    self.player.anim = 0x3E;
                    return;
                }
                self.player.pose_timer = 0;
                self.player.pose_step = 0;
                self.player.pose &= !pose::LANDING;
            }
            if self.player.pose_step >= 8 {
                self.player.pose_step = 0;
                self.player.pose_timer = self.player.pose_timer.wrapping_add(1);
                if self.player.pose_timer >= 8 {
                    self.player.pose_timer = 0;
                }
            }
            self.walking_pose();
            return;
        }
        self.set_overflow_from(self.player.pose);
        if self.player.pose & pose::TURNING == 0 {
            self.player.pose = (self.player.pose & pose::SHOT) | pose::TURNING;
            self.player.pose_timer = 0;
            self.player.anim = 0x02;
            return;
        }
        self.player.pose_timer = self.player.pose_timer.wrapping_add(1);
        let x = u16::from(self.player.pose_timer >> 1);
        if x < 3 {
            self.player.anim = self.cx.prg.player.animations.walking_turn[usize::from(x)];
            return;
        }
        self.player.facing ^= 0x40;
        self.player.pose_step = 0;
        self.player.pose_timer = 0;
        self.player.pose &= pose::SHOT;
        self.player.anim = 0x0E;
    }

    /// `$8AC8`: walking's walking tail and its attack.
    fn walking_pose(&mut self) {
        self.set_overflow_from(self.player.pose);
        if self.player.pose & pose::SHOT == 0 {
            self.player.anim =
                self.cx.prg.player.animations.walking[usize::from(self.player.pose_timer)];
            return;
        }
        self.set_overflow_from(self.player.power_up);
        if self.player.power_up & power_up::ON != 0 {
            self.fire();
            self.player.attack_timer = 0x10;
            self.player.anim =
                self.cx.prg.player.animations.walking[usize::from(self.player.pose_timer)];
            return;
        }
        self.player.attack_timer = self.player.attack_timer.wrapping_add(1);
        let mut x = self.player.pose_timer;
        self.set_overflow_from(self.cx.effects.attack);
        if self.cx.effects.attack & attack::CHARGING != 0 {
            x = self.melee_block().unwrap_or(x);
            if self.player.attack_timer >= 0x13 {
                self.player.attack_timer = 0x12;
            }
        }
        if self.player.attack_timer >= 0x16 {
            self.player.pose &= !pose::SHOT;
        }
        self.player.anim = self.cx.prg.player.animations.melee[usize::from(x)];
    }

    /// `$8B78`: jumping, the held jump's lift on odd frames.
    fn jumping(&mut self) {
        self.player.walk_gate = Buttons::SIDEWAYS;
        self.stunned();
        if self.player.pose & pose::RESUME == 0 && self.cx.world.frame_odd {
            if self.pad.contains(Buttons::A) {
                self.player.pose_step = self.player.pose_step.wrapping_add(1);
                if self.player.pose_step < 0x04 {
                    self.jump_boost();
                    self.jumping_input();
                    return;
                }
            }
            self.player.pose_step = self.player.pose_step.wrapping_add(1);
            if self.player.pose_step < 0x02 {
                self.jump_boost();
            } else {
                self.player.pose |= pose::RESUME;
            }
        }
        self.jumping_input();
    }

    /// `$8B99`: the held-jump y velocity trim.
    fn jump_boost(&mut self) {
        let [hi, lo] = self.player.vy.to_be_bytes();
        let (lo, borrow) = lo.overflowing_sub(0x16);
        self.player.vy = i16::from_be_bytes([hi.wrapping_sub(u8::from(borrow)), lo]);
    }

    /// `$8BBC`: the jump's controls: fire, turn.
    fn jumping_input(&mut self) {
        self.set_overflow_from(self.pressed.bits());
        if self.pressed.contains(Buttons::B) {
            self.jumping_shot();
            return;
        }
        self.set_overflow_from(self.player.pose);
        if self.pad.intersects(Buttons::SIDEWAYS)
            && !self.faces_press()
            && self.player.pose & pose::TURNING == 0
        {
            self.player.pose = (self.player.pose & pose::SHOT) | pose::TURNING;
            self.player.pose_timer = 0;
            self.player.anim = 0x1D;
            return;
        }
        if self.player.pose & pose::TURNING != 0 {
            self.player.pose_timer = self.player.pose_timer.wrapping_add(1);
            let mut step = self.player.pose_timer;
            if self.player.pose_timer == 0x04 {
                self.player.pose &= !pose::TURNING;
                self.player.facing ^= 0x40;
                step = 0x04;
            }
            let x = u16::from(step >> 1);
            if x < 8 {
                self.player.anim = self.cx.prg.player.animations.jump_turn[usize::from(x)];
                return;
            }
        }
        if self.player.pose & pose::SHOT == 0 {
            self.player.anim = 0x1C;
            self.player.pose &= pose::RESUME;
            self.player.pose_timer = 0;
            return;
        }
        if self.player.power_up & power_up::ON != 0 {
            self.fire();
            self.player.anim = 0x27;
            return;
        }
        self.player.attack_timer = self.player.attack_timer.wrapping_add(1);
        self.set_overflow_from(self.cx.effects.attack);
        if self.cx.effects.attack & attack::CHARGING != 0 {
            self.melee_block();
            if self.player.attack_timer >= 0x13 {
                self.player.attack_timer = 0x12;
            }
        }
        if self.player.attack_timer < 0x15 {
            self.player.anim = 0x27;
            return;
        }
        self.player.pose &= !pose::SHOT;
        self.player.anim = 0x1C;
        self.player.pose &= pose::RESUME;
        self.player.pose_timer = 0;
    }

    /// `$8C8A`: motion 3 + the second pad shoulder.
    fn jumping_shot(&mut self) {
        if self.player.pose & pose::TURNING != 0 {
            self.player.pose &= !pose::TURNING;
            self.player.facing ^= 0x40;
        }
        self.reset_and_shoot();
        self.player.attack_timer = 0;
        self.player.pose |= pose::SHOT;
        self.player.anim = 0x27;
    }

    /// `$8CC6`: sliding.
    fn sliding(&mut self) {
        self.stunned();
        self.player.walk_gate = Buttons::NONE;
        self.set_overflow_from(self.player.pose_step);
        if self.player.pose_step & 0x80 == 0 {
            self.slide_on_ground();
            return;
        }
        self.player.vy = 0;
        self.player.sliding = 0;
        let x = u16::from(self.player.pose_timer >> 1);
        if x < 2 {
            self.player.anim = self.cx.prg.player.animations.slide_air[usize::from(x)];
            self.player.pose_timer = self.player.pose_timer.wrapping_add(1);
            return;
        }
        if self.player.pose_step & 0x40 == 0 {
            self.player.motion = Motion::Falling;
            self.player.pose_timer = 0;
            self.player.pose_step = 0;
            self.player.pose = 0;
            self.player.anim = 0x1F;
            return;
        }
        let [hi, lo] = self.player.y.to_be_bytes();
        self.overflow = adc_overflows(lo, !0x20, true);
        let (lo, borrow) = lo.overflowing_sub(0x20);
        self.player.y = u16::from_be_bytes([hi.wrapping_sub(u8::from(borrow)), lo]);
        self.player.pose = pose::LANDING | pose::LONG_LANDING;
        self.player.vy = i16::from_be_bytes([0x00, 0x40]);
        self.player.motion = Motion::Standing;
        self.player.pose_timer = 0;
        self.player.pose_step = 0;
        self.player.anim = 0x01;
    }

    /// `$8D38`: the slide on the ground (no landing flag).
    fn slide_on_ground(&mut self) {
        self.set_overflow_from(self.player.pose_timer);
        if self.player.pose_timer & 0x80 != 0 {
            self.slide_glide();
            return;
        }
        if self.player.pose_timer & 0x40 != 0 {
            self.slide_end();
            return;
        }
        if !self.pad.contains(Buttons::DOWN) {
            self.slide_jump();
            return;
        }
        let mut x = u16::from(self.player.pose_timer >> 1);
        if x >= 2 {
            self.slide_launch();
            return;
        }
        if x == 1 {
            if self.slide_blocked() {
                return;
            }
        } else {
            x = 0;
        }
        self.player.anim = self.cx.prg.player.animations.slide_start[usize::from(x)];
        self.player.pose_timer = self.player.pose_timer.wrapping_add(1);
    }

    /// `$8D6D`: the slide's launch: a push and the speed in the facing direction.
    fn slide_launch(&mut self) {
        self.cx.effects.sound[0] = 0x24;
        self.player.sliding = sliding::ON;
        let [hi, lo] = self.player.x.to_be_bytes();
        let x = self.player.x.cast_signed();
        self.overflow = if self.player.facing & 0x40 == 0 {
            x.overflowing_add(0x80).1
        } else {
            x.overflowing_sub(0x80).1
        };
        if self.player.facing & 0x40 == 0 {
            let (lo, carry) = lo.overflowing_add(0x80);
            self.player.x = u16::from_be_bytes([hi.wrapping_add(u8::from(carry)), lo]);
            self.player.vx = i16::from_be_bytes([0x00, 0x90]);
        } else {
            let (lo, borrow) = lo.overflowing_sub(0x80);
            self.player.x = u16::from_be_bytes([hi.wrapping_sub(u8::from(borrow)), lo]);
            self.player.vx = i16::from_be_bytes([0xFF, 0x70]);
        }
        self.player.anim = 0x3A;
        self.player.pose_timer = 0x80;
    }

    /// `$8DBB`: the slide gliding until the screen's edge or its speed runs out.
    fn slide_glide(&mut self) {
        self.player.anim = 0x3A;
        let cam = self.cx.world.camera_x;
        self.overflow = sub_overflows16(self.player.x, cam);
        let [xhi, xlo] = self.player.x.to_be_bytes();
        let (_, borrow) = xlo.overflowing_sub(cam.to_be_bytes()[1]);
        let hi = xhi
            .wrapping_sub(cam.to_be_bytes()[0])
            .wrapping_sub(u8::from(borrow));
        let vx_lo = self.player.vx.to_be_bytes()[1];
        let end = if self.player.vx < 0 {
            hi < 0x02 || vx_lo >= 0xF0
        } else {
            hi >= 0x0D || vx_lo < 0x10
        };
        if end {
            self.player.pose_timer = 0x40;
            self.player.anim = 0x3B;
        }
    }

    /// `$8DFE`: something solid 24 px ahead, which ends the slide in a crouch.
    fn slide_blocked(&mut self) -> bool {
        let (ylo, yhi) = self.y();
        let py = yhi.wrapping_sub(u8::from(ylo < 0x80));
        let dx = self.screen_x(self.player.x);
        self.overflow = sub_overflows16(self.player.x, self.cx.world.camera_x);
        let solid = if self.player.facing & 0x40 == 0 {
            dx >= 0xD0 || self.solid_x(self.player.x.wrapping_add(0x180), py)
        } else {
            dx < 0x1A || self.solid_x(self.player.x.wrapping_sub(0x180), py)
        };
        if solid {
            self.player.anim = 0x04;
            self.player.motion = Motion::Crouching;
            self.player.pose = 0;
            self.player.pose_timer = 0;
            self.player.pose_step = 0;
        }
        solid
    }

    /// `$8E71`: the slide's end, back into a crouch.
    fn slide_end(&mut self) {
        self.player.sliding = 0;
        self.player.anim = 0x3B;
        self.player.motion = Motion::Crouching;
        self.player.pose = 0;
        self.player.pose_timer = 0;
        self.player.pose_step = 0;
    }

    /// `$8E8C`: down released mid-slide: the jump.
    fn slide_jump(&mut self) {
        if self.headroom_blocked() {
            self.slide_end();
            return;
        }
        self.player.vy = i16::from_be_bytes([0xFF, 0xD4]);
        self.player.pose_timer = 0;
        self.player.pose_step = 0;
        self.player.pose = 0;
        self.player.anim = 0x1C;
        self.player.motion = Motion::Jumping;
    }

    /// `$8EB1`: motion 5.
    fn falling(&mut self) {
        self.player.walk_gate = Buttons::SIDEWAYS;
        self.set_overflow_from(self.pressed.bits());
        if self.pressed.contains(Buttons::B) {
            self.player.pose = pose::SHOT | pose::RESUME;
            self.player.attack_timer = 0;
            self.player.motion = Motion::Jumping;
            self.player.anim = 0x27;
            return;
        }
        if self.pad.intersects(Buttons::SIDEWAYS) && !self.faces_press() {
            self.player.pose = pose::TURNING;
            self.player.motion = Motion::Jumping;
            self.player.pose_timer = 0;
            self.player.anim = 0x1D;
            return;
        }
        self.player.anim = 0x1F;
        self.player.pose = pose::RESUME;
        self.player.pose_timer = 0x20;
    }

    /// `$8F01`: motion 6, aiming up.
    fn aiming_up(&mut self) {
        self.set_overflow_from(self.player.hurt);
        if self.player.hurt & hurt::STUNNED != 0 {
            self.hurt();
            return;
        }
        self.player.walk_gate = Buttons::NONE;
        if self.pad.contains(Buttons::UP) {
            self.set_overflow_from(self.pressed.bits());
            if self.pressed.contains(Buttons::A) {
                self.aiming_up_jump();
            } else if self.pressed.contains(Buttons::B) {
                self.aiming_up_shot();
            } else if !self.pad.intersects(Buttons::SIDEWAYS) {
                self.aiming_up_attack();
            } else {
                self.aiming_up_turn();
            }
            return;
        }
        self.set_overflow_from(self.player.pose);
        if self.player.pose & pose::SHOT != 0 {
            self.player.pose = pose::SHOT;
            self.player.pose_timer = 0;
            self.player.motion = Motion::Standing;
            self.player.anim = 0x25;
            return;
        }
        if self.player.pose & pose::LANDING != 0 {
            self.player.pose_timer = 0;
            self.player.motion = Motion::Standing;
            self.player.pose = 0;
        } else {
            self.player.pose = pose::LANDING;
        }
        self.player.anim = 0x45;
    }

    /// `$901C`: motion 6 + the first pad shoulder (launch upward).
    fn aiming_up_jump(&mut self) {
        if self.jump_blocked() {
            self.player.anim = 0x46;
            return;
        }
        self.player.vy = i16::from_be_bytes([0xFF, 0xD4]);
        self.set_vx_from_pad();
        self.player.motion = Motion::Jumping;
        self.player.pose &= pose::SHOT;
        self.player.pose_timer = 0;
        self.player.pose_step = 0;
        self.player.anim = 0x1C;
    }

    /// `$8FFA`: motion 6 + the second pad shoulder.
    fn aiming_up_shot(&mut self) {
        self.set_overflow_from(self.player.pose);
        if self.player.pose & pose::SHOT == 0 {
            self.player.attack_timer = 0;
            self.player.pose = (self.player.pose & !pose::TURNING) | pose::SHOT;
            self.player.anim = 0x47;
            return;
        }
        self.player.attack_timer = 1;
        self.aiming_up_attack();
    }

    /// `$8FE6`: motion 6 aiming up while turning.
    fn aiming_up_turn(&mut self) {
        if !self.faces_press() {
            self.player.facing ^= 0x40;
        }
        self.aiming_up_attack();
    }

    /// `$8F60`: aiming up's attack.
    fn aiming_up_attack(&mut self) {
        self.set_overflow_from(self.player.pose);
        if self.player.pose & pose::SHOT == 0 {
            self.player.anim = 0x46;
            return;
        }
        if self.player.power_up & power_up::ON != 0 {
            self.fire();
            self.player.anim = 0x48;
            return;
        }
        self.player.attack_timer = self.player.attack_timer.wrapping_add(1);
        self.set_overflow_from(self.cx.effects.attack);
        if self.cx.effects.attack & attack::CHARGING != 0 {
            self.melee_block();
            if self.player.attack_timer >= 0x13 {
                self.player.attack_timer = 0x12;
            }
        }
        if self.player.attack_timer >= 0x14 {
            self.player.pose &= !pose::SHOT;
            self.player.anim = 0x46;
            return;
        }
        match self.player.attack_timer {
            2 => {
                if self.cx.effects.attack & attack::CHARGEABLE != 0
                    && self.cx.effects.attack & attack::CHARGED == 0
                {
                    self.cx.effects.attack = attack::CHARGEABLE | attack::CHARGING;
                }
                self.player.charge_timer = 0;
                self.spawn_shot();
                self.player.anim = 0x48;
            }
            n if n > 2 => self.player.anim = 0x48,
            _ => self.player.anim = 0x47,
        }
    }

    /// `$908D`: motion 7, dying: the flash over the pose, the burst from `$0480` = `$30`,
    /// and at `$C0` the life lost (`$BA = 4`) with full energy.
    fn dying(&mut self) {
        self.cx.effects.busy = busy::SCRIPTED;
        let timer = self.player.death_timer;
        if timer >= 0x30 {
            if timer < 0x48 {
                let frame = (timer - 0x30) & 0x1E;
                self.work.pointer = self.rw(0x9191 + u16::from(frame));
                self.work.attr = self.cx.prg.player.death_attributes[usize::from(frame >> 1)];
                self.overlay(5);
            }
            self.player.death_timer = timer.wrapping_add(1);
        } else {
            if timer == 0x2F {
                self.cx.effects.sound[1] = 0x33;
                self.player.stage_script = stage_script::FROZEN | stage_script::HIDDEN;
                self.player.death_flags |= 0x01;
            }
            if timer.is_multiple_of(16) {
                self.cx.effects.sound[1] = 0x40;
            }
            self.set_overflow_from(self.player.death_flags);
            if self.player.death_flags & 0x80 == 0 {
                self.work.pointer = self.rw(0x9A40 + u16::from(timer & 0x06));
                self.work.attr = self.player.facing & 0x03;
                self.overlay(4);
            }
            self.player.death_timer = timer.wrapping_add(1);
            let mut beat = self.player.death_timer;
            if beat == 4 {
                let palette = self.cx.prg.player.death_palette;
                for i in 0..4u8 {
                    self.cx.effects.palette[16 + usize::from(i)] = palette[usize::from(i)];
                }
                self.cx.effects.palette_dirty = 0x80;
                beat = 0x80;
            }
            if beat.is_multiple_of(8) {
                self.player.facing ^= 0x03;
            }
        }
        if self.player.death_timer < 0xC0 {
            return;
        }
        self.set_overflow_from(self.cx.effects.hud);
        let effects = &mut *self.cx.effects;
        effects.transition = 4;
        self.player.energy = 0x0E;
        if effects.hud & 0x80 == 0 {
            effects.attack = 0;
            self.player.power_up = 0;
            effects.weapon = Weapon::Bolt;
            return;
        }
        if effects.weapon == Weapon::Super {
            effects.weapon = effects.stored_weapon;
        }
        if effects.weapon != Weapon::Bolt {
            effects.attack = attack::CHARGEABLE;
        }
        self.player.power_up = 0;
    }

    /// The dying overlay at the player's screen position, unless `$0465` bit 7 hides it.
    fn overlay(&mut self, chr_slot: u8) {
        self.work.anchor = 0;
        self.work.tile = 0;
        self.work.chr_slot = chr_slot;
        self.work.screen_x = self.player.screen_x;
        self.work.screen_y = self.player.screen_y;
        self.set_overflow_from(self.player.death_flags);
        if self.player.death_flags & 0x80 == 0 {
            self.overflow = engine::draw(self.work, 1, self.cx);
        }
    }

    /// `$D03C` (bank 14, through `$91D8`): motion 9, the boss room's entry: Batman
    /// lowered in on his line (`$D17A`, `$D1B0`), the tally counted up to `$B2:$B3`
    /// beside the boss's hit points, the pose held, then motion 0.
    fn boss_entry(&mut self) {
        self.cx.effects.busy = busy::SCRIPTED;
        let player = &mut *self.player;
        match player.entry_step {
            0 => {
                player.entry_timer = player.entry_timer.wrapping_add(1);
                if player.entry_timer >= 4 {
                    self.cx.effects.sound[0] = 0x3E;
                    (player.entry_timer, player.tally_digit) = (0, 0);
                    player.entry_step += 1;
                    return;
                }
                self.work.chr_slot = 5;
                self.entry_pose(1);
                self.entry_line(0);
            }
            1 => {
                player.entry_timer = player.entry_timer.wrapping_add(1);
                let pose = if player.entry_timer & 0x02 != 0 { 1 } else { 2 };
                self.work.chr_slot = 5;
                self.entry_pose(pose);
                self.entry_line(pose);
                let (tally, digit) = (self.player.tally, self.player.tally_digit);
                self.queue_tally(tally, Some(digit));
                if self.cx.world.frame.is_multiple_of(16) {
                    self.cx.effects.sound[0] = 0x3E;
                }
                let player = &mut *self.player;
                player.tally_digit += 1;
                if player.tally_digit >= 0x0A {
                    player.tally_digit = 0;
                }
                player.tally = player.tally.wrapping_add(0x64);
                let colour = if player.tally >= u16::from_le_bytes(self.cx.effects.boss_pool) {
                    player.entry_step += 1;
                    player.entry_timer = 0;
                    0x0F
                } else if self.cx.world.frame.is_multiple_of(8) {
                    0x30
                } else {
                    0x0F
                };
                set_first_colours(&mut self.cx.effects.palette, colour);
                let effects = &mut *self.cx.effects;
                effects.vram.push(0x3F00, 0, &effects.palette);
                // `$C34B` -> `$C0C6`: its `ADC #$00` on the return address's high byte ($C3).
                self.overflow = false;
            }
            2 => {
                if player.entry_timer == 0 {
                    let target = u16::from_le_bytes(self.cx.effects.boss_pool);
                    self.queue_tally(target, None);
                }
                self.work.chr_slot = 5;
                self.entry_pose(1);
                self.entry_line(3);
                let player = &mut *self.player;
                player.entry_timer = player.entry_timer.wrapping_add(1);
                if player.entry_timer >= 4 {
                    self.cx.effects.sound[0] = 0x23;
                    player.entry_step += 1;
                    player.entry_timer = 0;
                }
            }
            _ => {
                let held = usize::from(player.entry_timer >> 2);
                self.work.chr_slot = 5;
                self.entry_pose(self.cx.prg.player.entry_poses[held]);
                self.work.chr_slot = 4;
                let player = &mut *self.player;
                player.entry_timer = player.entry_timer.wrapping_add(1);
                if player.entry_timer < 0x0C {
                    self.entry_pose(self.cx.prg.player.entry_pose_second[held]);
                    return;
                }
                self.cx.effects.song = 0x03;
                self.cx.oam.chr[4] = 0xA2;
                player.motion = Motion::Standing;
                player.stage_script = 0;
                self.cx.effects.busy = 0;
            }
        }
    }

    /// `$D17A`: Batman's entry pose `pose` (`$D1A4`), at (`$3E`, `$B0`).
    fn entry_pose(&mut self, pose: u8) {
        self.work.attr = 0;
        self.work.anchor = 0;
        self.work.tile = 0;
        self.work.screen_x = 0x3E;
        self.work.screen_y = 0xB0;
        self.work.pointer = rom_word(self.cx.prg, 14, 0xD1A4 + 2 * u16::from(pose));
        self.overflow = engine::draw(self.work, 1, self.cx);
    }

    /// `$D1B0`: the line above him, frame `line` (`$D1D8`), at (`$3E`, `$78`).
    fn entry_line(&mut self, line: u8) {
        self.work.pointer = rom_word(self.cx.prg, 14, 0xD1D8 + 2 * u16::from(line));
        self.work.attr |= 0x03;
        self.work.chr_slot = 4;
        self.work.screen_x = 0x3E;
        self.work.screen_y = 0x78;
        self.overflow = engine::draw(self.work, 1, self.cx);
    }

    /// `$D1E0`/`$D1F5` and `$D200`: queues `tally` at `$2348` (its last two digits
    /// `last` when given) and the boss's hit points at `$2352`, each five decimal
    /// digits and a 0, two tile rows (`$C199`, `$D24A`).
    fn queue_tally(&mut self, tally: u16, last: Option<u8>) {
        let prg = self.cx.prg;
        let rows = |value: u16, last: Option<u8>| {
            let mut digits = [10000, 1000, 100, 10, 1].map(|unit| (value / unit % 10) as u8);
            if let Some(digit) = last {
                digits[3] = digit;
                digits[4] = digit;
            }
            let mut top = [0x0A; 6];
            for (tile, digit) in top.iter_mut().zip(digits) {
                *tile = prg.digit_tiles[usize::from(digit)];
            }
            let mut bottom = top.map(|tile| tile | 1);
            bottom[5] = 0x0B;
            (top, bottom)
        };
        let vram = &mut self.cx.effects.vram;
        let (top, bottom) = rows(tally, last);
        vram.push(0x2348, 0, &top);
        vram.push(0x2368, 0, &bottom);
        let (top, bottom) = rows(self.cx.effects.boss_hp, None);
        let vram = &mut self.cx.effects.vram;
        vram.push(0x2352, 0, &top);
        vram.push(0x2372, 0, &bottom);
    }

    /// `$91B5`: motion 8, held while the power-up's pieces close in: hidden on odd frames.
    fn powering_up(&mut self) {
        self.overflow = false;
        if self.cx.world.frame_odd {
            self.player.stage_script = stage_script::FROZEN | stage_script::HIDDEN;
        } else {
            self.player.stage_script = stage_script::FROZEN;
            let x = u16::from((self.cx.world.frame >> 1) & 0x01);
            self.player.facing =
                (self.player.facing & 0xFC) | self.cx.prg.player.power_up_facings[usize::from(x)];
        }
    }

    /// `$91DB`: motion 11.
    fn flying(&mut self) {
        self.player.walk_gate = Buttons::DPAD;
        self.stunned();
        self.set_overflow_from(self.pressed.bits());
        if self.pressed.contains(Buttons::B) {
            self.cx.effects.attack = if self.cx.effects.attack & attack::CHARGEABLE != 0 {
                attack::CHARGEABLE | attack::CHARGING
            } else {
                0
            };
            self.player.charge_timer = 0;
            self.spawn_shot();
            self.player.attack_timer = 0;
            self.player.pose |= pose::SHOT;
            return;
        }
        self.set_overflow_from(self.player.pose);
        if self.player.pose & pose::SHOT == 0 {
            self.player.pose &= pose::RESUME;
            self.player.pose_timer = 0;
            return;
        }
        self.player.attack_timer = self.player.attack_timer.wrapping_add(1);
        self.set_overflow_from(self.cx.effects.attack);
        if self.cx.effects.attack & attack::CHARGING != 0 {
            self.melee_block();
            if self.player.attack_timer >= 0x13 {
                self.player.attack_timer = 0x12;
            }
        }
        if self.player.attack_timer < 0x15 {
            return;
        }
        self.player.pose &= !pose::SHOT;
        self.player.pose &= pose::RESUME;
        self.player.pose_timer = 0;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::button::Buttons;
    use crate::actor::collision::Probe;
    use crate::actor::oracle::{Routine, Scene, assert_game, assert_routine, was_seen};
    use crate::actor::world::Cx;
    use crate::levelmap::Cell;

    use super::{draw_player, step_draw, step_move};

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
        // The states the two scenes reached; motions 7 and 9 are unported.
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
