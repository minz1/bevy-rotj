//! Weapon 4 (Three-Way) and its charged tethered-bubble attack (`$91B2`).

use super::super::engine::{Work, rom_word};
use super::super::slots::Counter;
use super::super::world::{Cx, Motion, attack, shot_flags, shot_state, sound};
use super::{
    AIMED_UP, CHARGE_START, Flight, ShotMode, VERTICAL, above_player, advance, ahead,
    beside_player, bounce_off_wall, draw, draw_alternate_frames, fly, free_slot, project_keep,
    project_or_free,
};

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThreeWayStage {
    /// Bubbles every 8th frame.
    Brew = CHARGE_START,
    /// A bubble.
    Bubble = 0x81,
    /// The tether.
    Tether = 0x82,
}

impl ThreeWayStage {
    const fn byte(self) -> u8 {
        self as u8
    }

    const fn from_byte(byte: u8) -> Option<ThreeWayStage> {
        match byte {
            CHARGE_START => Some(ThreeWayStage::Brew),
            0x81 => Some(ThreeWayStage::Bubble),
            0x82 => Some(ThreeWayStage::Tether),
            _ => None,
        }
    }
}

/// `$80` and up are its charged attack (`$92C4`).
pub(super) fn three_way(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    let projectile = cx.effects.projectiles[slot];
    if projectile.is_charged() {
        match ThreeWayStage::from_byte(projectile.kind) {
            Some(ThreeWayStage::Brew) => return three_way_brew(slot, cx),
            Some(ThreeWayStage::Bubble) => return three_way_bubble(slot, work, cx),
            Some(ThreeWayStage::Tether) => return three_way_tether(slot, work, cx),
            None => panic!(
                "handler 4's type ${:02X} is past the $95EF table",
                projectile.kind
            ),
        }
    }
    match fly(slot, work, cx) {
        Flight::Freed { .. } => return,
        Flight::Touched => {
            cx.effects.projectiles[slot].timer |= 1;
            project_or_free(slot, work, cx);
            return;
        }
        Flight::TouchedAgain => {
            if cx.effects.projectiles[slot].timer & AIMED_UP != 0 {
                #[cfg(test)]
                super::super::oracle::seen(0x9268);
                let pick = usize::from(cx.world.rng[0] & 0x02 | 0x01);
                let (vx, vy) = (
                    cx.prg.weapons.three_way_vx[pick],
                    cx.prg.weapons.three_way_vy[pick],
                );
                let projectile = &mut cx.effects.projectiles[slot];
                projectile.vx = vx;
                projectile.vy = vy;
                projectile.state = shot_state::SPENT;
                projectile.attr = 0;
                projectile.timer = 0x04;
                projectile.flags = shot_flags::INERT;
            } else {
                #[cfg(test)]
                super::super::oracle::seen(0x9275);
                let positive = cx.world.rng[0] & 0x80 == 0;
                bounce_off_wall(slot, cx, positive);
            }
        }
        Flight::On => {}
    }
    if !project_or_free(slot, work, cx) {
        return;
    }
    let projectile = &mut cx.effects.projectiles[slot];
    if ShotMode::of(projectile.state) == ShotMode::Spent {
        work.pointer = rom_word(cx.prg, 5, 0x92BE + u16::from(projectile.timer & 0xFE));
        projectile.count_down();
    } else {
        work.pointer = rom_word(cx.prg, 5, 0x92BA + u16::from((cx.world.frame & 0x04) >> 1));
    }
    draw_alternate_frames(slot, work, cx);
}

/// Type `$80` (`$92D3`): while B is held, a type `$81` bubble from the player
/// every eighth frame (effect `$3F`); let go, it becomes the type `$82` tether
/// (effect `$2A`).
fn three_way_brew(slot: usize, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::super::oracle::seen(0x92D3);
    if cx.effects.attack & attack::CHARGING == 0 {
        cx.effects.sound[1] = sound::THREE_WAY_TETHER;
        three_way_let_go(slot, cx);
        return;
    }
    let projectile = &mut cx.effects.projectiles[slot];
    let count = projectile.part_timer;
    projectile.part_timer.tick_up();
    if count & 0x07 != 0 {
        return;
    }
    let Some(free) = free_slot(cx) else {
        return;
    };
    cx.effects.sound[0] = sound::THREE_WAY_BUBBLE;
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
    projectile.x = x;
    projectile.y = y;
    three_way_blow(free, vx, cx);
}

/// `$9489`/`$959F`'s shared part: `slot` becomes a type `$81` bubble with x
/// velocity `vx`, rising at `$FF:($F9 | $E0)`, its palette `$9637` by `$F1` bit 3.
fn three_way_blow(slot: usize, vx: i16, cx: &mut Cx<'_>) {
    let attr = cx.prg.weapons.bubble_palette[usize::from(cx.world.frame & 0x08 != 0)];
    let rise = cx.world.rng[0] | 0xE0;
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.kind = ThreeWayStage::Bubble.byte();
    projectile.state = 0;
    projectile.part_timer = 0;
    projectile.flags = shot_flags::PASSES_SHIELDS;
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
    projectile.kind = ThreeWayStage::Tether.byte();
    projectile.damage = 1;
    projectile.state = 0;
    projectile.param = 0;
    projectile.part = 0;
    projectile.flags = shot_flags::UNSTOPPABLE;
    projectile.part_timer = if aim_up { VERTICAL } else { 0 };
    projectile.vx = vx;
    projectile.vy = vy;
    projectile.x = x;
    projectile.y = y;
}

/// Type `$81` (`$930B`): a bubble rising for 48 frames, slowing by 1 a frame.
fn three_way_bubble(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::super::oracle::seen(0x930B);
    let projectile = &mut cx.effects.projectiles[slot];
    if ShotMode::of(projectile.state) == ShotMode::Spent {
        projectile.free();
        return;
    }
    projectile.part_timer.tick_up();
    if projectile.part_timer >= 0x30 {
        projectile.free();
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
    super::super::oracle::seen(0x935E);
    work.anchor = 0;
    work.attr = 0;
    work.tile = 0;
    work.chr_slot = 4;
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.part.tick_up();
    if projectile.part >= 0xC0 {
        projectile.free();
        return;
    }
    if projectile.part_timer & VERTICAL != 0 {
        #[cfg(test)]
        super::super::oracle::seen(0x93BA);
        projectile.vy = three_way_pull(projectile.vy, projectile.y & 0x8000 != 0, 0x50, 0xB0);
    } else {
        projectile.vx = three_way_pull(projectile.vx, projectile.x & 0x8000 != 0, 0x40, 0xC0);
    }
    let x = projectile.x.wrapping_add(projectile.vx.cast_unsigned());
    let y = projectile.y.wrapping_add(projectile.vy.cast_unsigned());
    projectile.x = x;
    projectile.y = y;
    let count = projectile.part;
    (work.x, work.y) = beside_player(cx, x, y.wrapping_sub(0x0180));
    if count.is_multiple_of(8)
        && let Some(free) = free_slot(cx)
    {
        cx.effects.sound[0] = sound::THREE_WAY_BUBBLE;
        let projectile = &mut cx.effects.projectiles[free];
        projectile.x = work.x;
        projectile.y = work.y;
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
    if behind {
        let [hi, lo] = velocity.wrapping_add(4).to_be_bytes();
        let lo = if (toward..0x80).contains(&lo) {
            toward
        } else {
            lo
        };
        i16::from_be_bytes([hi, lo])
    } else {
        let [hi, lo] = velocity.wrapping_sub(4).to_be_bytes();
        let lo = if (0x80..away).contains(&lo) { away } else { lo };
        i16::from_be_bytes([hi, lo])
    }
}
