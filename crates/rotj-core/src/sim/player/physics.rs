//! Batman's movement physics (`$800B`): wall/floor/ceiling probes, riding platforms, gravity.

use super::super::collision::{self, Probe, probe};
use super::super::engine::{self, Work, probe_flags};
use super::super::slots::Counter;
use super::super::world::{
    Cx, Motion, Player, attack, hurt, pose, slide_ledge, sound, stage_script,
};
use crate::sim::button::Buttons;

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
        if floor(&mut player, work, probe, cx) {
            work.probe_flags |= probe_flags::BLOCKED_Y;
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
            work.y = work.y.wrapping_add(u16::from(add));
            work.vy = 0;
        }
    }

    wall_ahead(&mut player, work, probe, cx);
    ground_or_ride(pad, falling, &mut player, work, probe, cx);

    // `$82B9-$8310`: a damage response (`$9F` bit 7, set by the `$C99D`/`$CAFE`
    // class handlers on a damage tile) costs energy and knocks the player back.
    if work.probe_flags & probe_flags::DAMAGED != 0
        && (player.hit & probe_flags::DAMAGED != 0 || player.hurt & hurt::INVULNERABLE == 0)
        && !player.out_of_energy()
    {
        hit!(9);
        player.energy = player.energy.wrapping_sub(work.damage);
        cx.effects.sound.push(0, sound::PLAYER_HURT);
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
        player.walk_timer.tick_up();
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
        probe::LEFTWARD
    } else {
        probe.x = work.x.wrapping_add(0xA0);
        probe::RIGHTWARD
    };
    probe.y = work.y.wrapping_sub(0x0140);
    let mut blocked = wall(player, work, probe, direction, cx);
    if !blocked {
        probe.y = work.y.wrapping_sub(0x80);
        blocked = wall(player, work, probe, direction, cx);
    }
    if !blocked && player.motion != Motion::Sliding && player.motion != Motion::Crouching {
        probe.y = work.y.wrapping_sub(0x0220);
        blocked = wall(player, work, probe, direction, cx);
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
    let mut grounded = work.probe_flags & probe_flags::BLOCKED_Y != 0;
    if falling {
        probe.y = work.y;
        if !grounded {
            probe.x = work.x.wrapping_add(0xA0);
            grounded = floor(player, work, probe, cx);
            if !grounded {
                probe.x = work.x.wrapping_sub(0xA0);
                grounded = floor(player, work, probe, cx);
            }
        }
        if grounded {
            work.probe_flags |= probe_flags::BLOCKED_Y;
        }
        // `$824F`: fall onto a platform, or settle the motion state.
        let stood = collision::ride(work, probe, cx);
        if stood || grounded {
            if stood {
                hit!(5);
            }
            player.carrier = i16::from_le_bytes(probe.landing);
            landing_response(pad, player);
        } else if air_state(player, work, probe, cx) {
            player.carrier = 0;
        }
    } else if !grounded {
        probe.y = work.y.wrapping_sub(0x02B0);
        probe.x = work.x.wrapping_add(0xA0);
        grounded = wall(player, work, probe, 0x08, cx);
        if !grounded {
            probe.x = work.x.wrapping_sub(0xA0);
            grounded = wall(player, work, probe, 0x08, cx);
        }
        if grounded {
            work.probe_flags |= probe_flags::BLOCKED_Y;
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
/// (`$B9`) and writes (`$A8`, `$0472`); true when a wall blocked.
fn wall(
    player: &mut Player,
    work: &mut Work,
    probe: &mut Probe,
    flags: u8,
    cx: &mut Cx<'_>,
) -> bool {
    with_player(player, cx, |cx| collision::wall(work, probe, flags, cx))
}

/// `$CAFE` on the player's working copy, as [`wall`] (spikes underfoot, `$CD09`); true
/// when it landed.
fn floor(player: &mut Player, work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) -> bool {
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
        // Classic's rule: `$8427` leaves `$40` or 0, so only `facing`'s bit 7 decides, never the pad.
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
    if player.pose_step & slide_ledge::PROBED != 0 {
        return;
    }
    work.vy = 0;
    player.vx = 0;
    work.ax = 0;
    player.sliding = 0;
    probe.x = work.x;
    probe.y = work.y.wrapping_add(0x0100);
    let solid = collision::solid(probe, 0x80 | probe::FLOORS_SOLID, cx);
    player.pose_step = if solid {
        slide_ledge::PROBED | slide_ledge::FLOOR_BELOW
    } else {
        slide_ledge::PROBED
    };
    player.pose_timer = 0;
    work.y = probe.y;
    player.anim = 0x3C;
}

/// Gravity on the working y velocity (`$8311-$8339`), written back to the player.
pub(super) fn gravity(player: &mut Player, work: &Work) {
    let [hi, lo] = work.vy.wrapping_add(6).to_be_bytes();
    let (hi, lo) = if hi & 0x80 != 0 {
        (0xFF, if lo & 0x80 != 0 { lo } else { 0x80 })
    } else {
        (0x00, if lo & 0x80 == 0 { lo } else { 0x7F })
    };
    player.vy = i16::from_be_bytes([hi, lo]);
}
