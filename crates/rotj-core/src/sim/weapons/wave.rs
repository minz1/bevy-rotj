//! Weapon 3 (Wave) and its charged weaving spread attack (`$8D39`).

use super::super::engine::{Work, rom_word};
use super::super::oam::attr;
use super::super::slots::Counter;
use super::super::world::{Cx, Motion, Projectile, attack, shot_flags, sound};
use super::{
    AIMED_UP, CHARGE_START, Flight, ShotMode, WEAVE_INCREASING, advance, beside_player,
    bounce_off_wall, draw, draw_on_odd_frames, fly, free_slot, project_keep, project_or_free,
};

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaveStage {
    /// Gathering for 8 frames.
    Charge = CHARGE_START,
    /// The 64-frame fan.
    Spread = 0x81,
    /// The fan's shots: every `kind` from here up.
    SpreadShot = 0x82,
}

impl WaveStage {
    const fn byte(self) -> u8 {
        self as u8
    }

    const fn from_byte(byte: u8) -> Option<WaveStage> {
        match byte {
            CHARGE_START => Some(WaveStage::Charge),
            0x81 => Some(WaveStage::Spread),
            0x82.. => Some(WaveStage::SpreadShot),
            _ => None,
        }
    }
}

/// Handler `$8D39` (`$0476` 3): the wave, two parts that weave. False when the charge
/// returns before the other slots (`$8FD6`).
pub(super) fn wave(slot: usize, work: &mut Work, cx: &mut Cx<'_>) -> bool {
    match WaveStage::from_byte(cx.effects.projectiles[slot].kind) {
        Some(WaveStage::Charge) => return wave_charge(slot, work, cx),
        Some(WaveStage::Spread) => return wave_spread(slot, cx),
        Some(WaveStage::SpreadShot) => {
            wave_spread_shot(slot, work, cx);
            return true;
        }
        None => {}
    }
    match fly(slot, work, cx) {
        Flight::Freed { .. } => return true,
        Flight::Touched => {
            cx.effects.projectiles[slot].timer |= 1;
            project_or_free(slot, work, cx);
            return true;
        }
        Flight::TouchedAgain => {
            let positive = cx.world.rng[0] & 0x80 == 0;
            bounce_off_wall(slot, cx, positive);
        }
        Flight::On => {}
    }
    wave_flight(slot, work, cx);
    true
}

/// Handler 3's charged attack, type `$80` (`$8F03`): it gathers in front of the
/// player for eight frames while B is held, then becomes the `$81` spread.
fn wave_charge(slot: usize, work: &mut Work, cx: &mut Cx<'_>) -> bool {
    #[cfg(test)]
    super::super::oracle::seen(0x8F03);
    if cx.effects.attack & attack::CHARGING == 0 {
        cx.effects.projectiles[slot].free();
        return true;
    }
    let motion = usize::from(cx.effects.player.motion.byte());
    (work.x, work.y) = if cx.effects.player.motion == Motion::AimingUp {
        #[cfg(test)]
        super::super::oracle::seen(0x8F16);
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
    projectile.part_timer.tick_up();
    if projectile.part_timer != 8 {
        return true;
    }
    projectile.kind = WaveStage::Spread.byte();
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
    super::super::oracle::seen(0x8FC2);
    if cx.effects.attack & attack::CHARGING != 0 {
        cx.effects.sound[0] = sound::WAVE_SPREAD;
    }
    let count = cx.effects.projectiles[slot].part_timer;
    if cx.effects.attack & attack::CHARGING == 0 || count >= 0x40 {
        for projectile in &mut cx.effects.projectiles {
            projectile.free();
        }
        cx.effects.attack &= attack::CHARGEABLE;
        return false;
    }
    cx.effects.projectiles[slot].part_timer.tick_up();
    if count & 3 != 0 {
        return true;
    }
    let Some(free) = free_slot(cx) else {
        #[cfg(test)]
        super::super::oracle::seen(0x8FE8);
        return true;
    };
    let source = cx.effects.projectiles[slot];
    let angle = usize::from(source.part);
    let along = cx.prg.weapons.spread_vx[angle];
    let motion = usize::from(cx.effects.player.motion.byte());
    let left = cx.effects.player.faces_left();
    let (x, y) = if cx.effects.player.motion == Motion::AimingUp {
        #[cfg(test)]
        super::super::oracle::seen(0x9023);
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
    projectile.kind = WaveStage::SpreadShot.byte();
    projectile.part_timer = 0;
    projectile.damage = 1;
    projectile.flags = shot_flags::INERT;
    projectile.x = x;
    projectile.y = y;
    if cx.effects.player.motion == Motion::AimingUp {
        projectile.vx = along;
        projectile.vy = -0x50;
        projectile.timer = AIMED_UP;
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
    super::super::oracle::seen(0x90E1);
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.flags = shot_flags::UNSTOPPABLE;
    projectile.part_timer.tick_up();
    if projectile.part_timer >= 0x18 {
        projectile.free();
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
    let table = if projectile.timer & AIMED_UP != 0 {
        0x913A
    } else {
        0x913E
    };
    work.pointer = rom_word(cx.prg, 5, table + u16::from(projectile.part));
    draw(work, cx);
}

/// Projects, weaves and draws the wave's part (`$8D7E`-`$8E62`).
fn wave_flight(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    if !project_or_free(slot, work, cx) {
        return;
    }
    let (mode, timer) = {
        let projectile = &cx.effects.projectiles[slot];
        (ShotMode::of(projectile.state), projectile.timer)
    };
    if mode == ShotMode::Spent {
        if cx.world.frame_odd {
            let projectile = &mut cx.effects.projectiles[slot];
            projectile.count_down();
            return;
        }
        work.pointer = rom_word(cx.prg, 5, 0x8EEF + u16::from(timer & 0xFE));
        if cx.effects.projectiles[slot].count_down() {
            return;
        }
        draw(work, cx);
        return;
    }
    if timer & AIMED_UP != 0 {
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
    if cx.effects.projectiles[slot].param & WEAVE_INCREASING == 0 {
        let projectile = &mut cx.effects.projectiles[slot];
        let velocity = axis(projectile, horizontal);
        *velocity = velocity.wrapping_sub(4);
        let sub = velocity.to_be_bytes()[1];
        if (0x40..0xC0).contains(&sub) {
            #[cfg(test)]
            super::super::oracle::seen(if horizontal { 0x8DEE } else { 0x8E3D });
            projectile.param = sub;
        }
        return true;
    }
    let projectile = &mut cx.effects.projectiles[slot];
    let velocity = axis(projectile, horizontal);
    *velocity = velocity.wrapping_add(4);
    let add = velocity.to_be_bytes()[1];
    if !(0x40..0xC0).contains(&add) {
        return true;
    }
    projectile.param = add;
    !horizontal || add == 0x40
}

/// The velocity the weave adjusts.
fn axis(projectile: &mut Projectile, horizontal: bool) -> &mut i16 {
    if horizontal {
        &mut projectile.vx
    } else {
        &mut projectile.vy
    }
}
