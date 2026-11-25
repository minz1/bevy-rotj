//! Weapon 2 (Splitter) and its charged orbiting-blade attack (`$8891`).

use super::super::engine::{self, Work, rom_word};
use super::super::oam::attr;
use super::super::slots::Counter;
use super::super::world::{Cx, Motion, Weapon, attack, shot_flags, shot_state, sound};
use super::{
    AIMED_UP, CHARGE_START, Flight, NO_SPLIT, SECOND_HALF, ShotMode, beside_player, draw,
    draw_alternate_frames, fly, project_keep, project_or_free,
};

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SplitterStage {
    /// The four parts orbiting the player.
    Orbit = CHARGE_START,
    /// A part let go.
    Thrown = 0x81,
}

impl SplitterStage {
    const fn byte(self) -> u8 {
        self as u8
    }
}

/// Handler `$8891` (`$0476` 2): the splitter, which flies on, pushing its target into
/// cruise, and splits at a wall or when spent.
pub(super) fn splitter(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    if cx.effects.projectiles[slot].kind == SplitterStage::Orbit.byte() {
        splitter_orbit(slot, work, cx);
        return;
    }
    match fly(slot, work, cx) {
        Flight::Freed { .. } => return,
        Flight::Touched => {
            cx.effects.sound[0] = sound::IMPACT;
            cx.effects.projectiles[slot].timer |= 1;
            project_or_free(slot, work, cx);
            return;
        }
        Flight::TouchedAgain => {
            cx.effects.sound[0] = sound::IMPACT;
            splitter_split(slot, cx);
            return;
        }
        Flight::On => {}
    }
    if !project_or_free(slot, work, cx) {
        return;
    }
    let (attr, mode, timer, bounce, charged) = {
        let projectile = &cx.effects.projectiles[slot];
        (
            projectile.attr,
            ShotMode::of(projectile.state),
            projectile.timer,
            projectile.param,
            projectile.is_charged(),
        )
    };
    work.attr = attr;
    if mode == ShotMode::Spent {
        if bounce & NO_SPLIT == 0 {
            splitter_split(slot, cx);
            return;
        }
        work.pointer = rom_word(cx.prg, 5, 0x8A4B + u16::from(timer < 8) * 2);
        let projectile = &mut cx.effects.projectiles[slot];
        projectile.timer.tick_up();
        if projectile.timer >= 0x20 {
            projectile.free();
            return;
        }
    } else if timer & AIMED_UP != 0 {
        if mode == ShotMode::Flying {
            let projectile = &mut cx.effects.projectiles[slot];
            let [hi, sub] = projectile.vy.wrapping_sub(2).to_be_bytes();
            projectile.vy = i16::from_be_bytes([hi, if sub >= 0xD0 { sub } else { 0xC0 }]);
        }
        work.pointer = rom_word(cx.prg, 5, 0x8A43 + u16::from(charged) * 2);
    } else {
        if mode == ShotMode::Flying {
            let projectile = &mut cx.effects.projectiles[slot];
            if attr & attr::FLIP_X != 0 {
                let [hi, sub] = projectile.vx.wrapping_sub(2).to_be_bytes();
                projectile.vx = i16::from_be_bytes([hi, if sub >= 0xD0 { sub } else { 0xC0 }]);
            } else {
                let [hi, add] = projectile.vx.wrapping_add(2).to_be_bytes();
                projectile.vx = i16::from_be_bytes([hi, if add < 0x30 { add } else { 0x40 }]);
            }
        }
        work.pointer = rom_word(cx.prg, 5, 0x8A47 + u16::from(charged) * 2);
    }
    draw_alternate_frames(slot, work, cx);
}

/// Splits the shot into three parts, then one more (`$89AC`).
fn splitter_split(source: usize, cx: &mut Cx<'_>) {
    let mut part = 0usize;
    let mut slot = 0usize;
    while slot < 16 {
        if cx.effects.projectiles[slot].is_free() {
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
    projectile.kind = Weapon::Splitter.shot_kind();
    projectile.state = shot_state::SPENT;
    projectile.attr = 0;
    projectile.timer = 0;
    (projectile.x, projectile.y) = (src.x, src.y);
    projectile.flags = shot_flags::UNSTOPPABLE;
    projectile.damage = 0x02;
}

/// Handler 2's charged attack, type `$80` (`$8A4F`): four parts circle the player
/// while B is held, then fly off as type `$81` parts.
fn splitter_orbit(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::super::oracle::seen(0x8A4F);
    let projectile = cx.effects.projectiles[slot];
    let second_half_offset = if projectile.param & SECOND_HALF != 0 {
        4
    } else {
        0
    };
    let turn = u16::from(projectile.part) + second_half_offset;
    let [dvx] = engine::rom_split_words(cx.prg, 5, 0x8C79 + turn, 0x8C71 + turn);
    let [dvy] = engine::rom_split_words(cx.prg, 5, 0x8C89 + turn, 0x8C81 + turn);
    let vx = projectile.vx.cast_unsigned().wrapping_add(dvx);
    let vy = projectile.vy.cast_unsigned().wrapping_add(dvy);
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.vx = vx.cast_signed();
    projectile.vy = vy.cast_signed();
    projectile.part_timer.tick_up();
    if projectile.part_timer >= 0x40 {
        projectile.param ^= SECOND_HALF;
        projectile.part_timer = 0;
        cx.effects.sound[1] = sound::SPLITTER_ORBIT;
    }
    let back = vx.wrapping_neg();
    let motion = usize::from(cx.effects.player.motion.byte());
    let left = cx.effects.player.faces_left();
    (work.x, work.y) = if cx.effects.player.motion == Motion::AimingUp {
        #[cfg(test)]
        super::super::oracle::seen(0x8AFF);
        let dx = if left { 0xFFE0 } else { 0x0020 };
        let (x, y) = beside_player(cx, dx, 0xFBF0);
        (x.wrapping_add(vy), y.wrapping_add(back))
    } else if left {
        let (x, y) = beside_player(
            cx,
            cx.prg.weapons.split_left_dx[motion],
            cx.prg.weapons.split_dy[motion],
        );
        // Classic adds the negate's carry.
        let x = x.wrapping_add(back).wrapping_add(u16::from(vx == 0));
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
    super::super::oracle::seen(0x8BCB);
    cx.effects.sound[0] = sound::SPLITTER_SHOT;
    let player = &cx.effects.player;
    let (aim_up, left) = (player.motion == Motion::AimingUp, player.faces_left());
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.kind = SplitterStage::Thrown.byte();
    projectile.x = work.x;
    projectile.y = work.y;
    projectile.flags = 0;
    projectile.attr = 0;
    projectile.damage = 2;
    projectile.state = 0;
    let pose = if aim_up {
        #[cfg(test)]
        super::super::oracle::seen(0x8C18);
        projectile.timer = AIMED_UP;
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
    let tables = if projectile.param & SECOND_HALF != 0 {
        0x8C67
    } else {
        0x8C61
    };
    let (table, part) = (
        rom_word(cx.prg, 5, tables + pose),
        u16::from(projectile.part),
    );
    let [vx] = engine::rom_split_words(cx.prg, 5, table + part + 4, table + part);
    let [vy] = engine::rom_split_words(cx.prg, 5, table + part + 12, table + part + 8);
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.vx = vx.cast_signed();
    projectile.vy = vy.cast_signed();
}
