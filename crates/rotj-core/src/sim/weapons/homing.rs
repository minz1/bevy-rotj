//! Weapon 1 (Batarang / Homing) and its charged attack (`$8170`).

use super::super::engine::{Work, anchor, rom_word};
use super::super::slots::{Field, contact};
use super::super::world::{Cx, Motion, Orbit, attack, shot_flags, sound};
use super::{
    CHARGE_START, CIRCLED, above_player, advance, beside_player, draw, flying_shot, project_keep,
    project_or_free,
};
use crate::sim::button::Buttons;
use crate::sim::spawn::STATE_UNTARGETED;

/// The homing weapon's charged attack, by projectile `kind`.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HomingStage {
    /// Gathering a ball while B is held.
    Gather = CHARGE_START,
    /// The ball thrown.
    Thrown = 0x81,
    /// The three parts circling the ball (they do nothing on their own).
    Circling = 0x82,
}

impl HomingStage {
    pub(super) const fn byte(self) -> u8 {
        self as u8
    }

    const fn from_byte(byte: u8) -> Option<HomingStage> {
        match byte {
            CHARGE_START => Some(HomingStage::Gather),
            0x81 => Some(HomingStage::Thrown),
            0x82 => Some(HomingStage::Circling),
            _ => None,
        }
    }
}

/// The splitter's charged attack: `kind` `$81` and up fly the plain path, its high bit
/// Handler `$8170` (`$0476` 1): the homing shot; types `$80` and up run its charged attack (`$847D`).
pub(super) fn homing(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    let projectile = cx.effects.projectiles[slot];
    if !projectile.is_charged() {
        flying_shot(slot, work, cx, |cx| homing_steer(slot, cx), 0x8477);
        return;
    }
    if cx.world.frame.is_multiple_of(0x20) {
        cx.effects.sound.push(0, sound::CHARGE);
    }
    match HomingStage::from_byte(projectile.kind) {
        Some(HomingStage::Gather) => homing_gather(slot, work, cx),
        Some(HomingStage::Thrown) => homing_thrown(slot, work, cx),
        Some(HomingStage::Circling) => {}
        None => panic!(
            "handler 1's type ${:02X} is past the $876E table",
            projectile.kind
        ),
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
    let mut target = projectile.target();
    if target.is_none_or(|target| cx.slots.is_free(usize::from(target))) {
        target = homing_retarget(slot, cx);
    }
    if target
        .is_some_and(|target| cx.get(usize::from(target), Field::State) & STATE_UNTARGETED != 0)
    {
        target = homing_retarget(slot, cx);
    }
    let Some(target) = target else {
        return pointer;
    };
    #[cfg(test)]
    super::super::oracle::seen(0x831E);
    let target = usize::from(target);
    let position = |cx: &Cx<'_>, hi: Field, lo: Field| {
        u16::from_be_bytes([cx.get(target, hi), cx.get(target, lo)])
    };
    let projectile = &cx.effects.projectiles[slot];
    let dy = position(cx, Field::YHigh, Field::YLow).wrapping_sub(projectile.y);
    let dx = position(cx, Field::XHigh, Field::XLow).wrapping_sub(projectile.x);
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
fn homing_retarget(slot: usize, cx: &mut Cx<'_>) -> Option<u8> {
    let hittable = |&target: &usize| {
        let actor = &cx.slots.main[target];
        !actor.is_free()
            && actor.body.contact & contact::UNTARGETED == 0
            && actor.body.state & STATE_UNTARGETED == 0
    };
    let found = if slot & 1 == 0 {
        (0..16).find(hittable)
    } else {
        (0..16).rev().find(hittable)
    };
    let target = found.map(|slot| u8::try_from(slot).expect("slot < 16"));
    cx.effects.projectiles[slot].set_target(target);
    target
}

/// Type `$80` (`$8496`): the charge gathers in front of the player while B is
/// held, three type `$82` parts circling it once its seven frames have run
/// (`$7D00` counting them); letting go before that clears the charge, and after it,
/// or A or a direction pressed, throws it as type `$81`.
fn homing_gather(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    let held = cx.effects.attack & attack::CHARGING != 0;
    if cx.effects.projectiles[slot].param & CIRCLED == 0 {
        if !held {
            #[cfg(test)]
            super::super::oracle::seen(0x8712);
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
    super::super::oracle::seen(0x84B6);
    homing_in_front(slot, work, cx);
    let step = cx.effects.projectiles[slot].vx.to_be_bytes()[0];
    work.pointer = rom_word(cx.prg, 5, 0x8774 + u16::from(step) * 2);
    if !cx.world.frame_odd {
        let step = step.wrapping_add(1);
        let projectile = &mut cx.effects.projectiles[slot];
        projectile.vx = i16::from_be_bytes([step, projectile.vx.to_be_bytes()[1]]);
        if step >= 7 {
            let mut spawned = true;
            if cx.effects.projectiles[slot].param & CIRCLED == 0 {
                cx.effects.projectiles[slot].param = CIRCLED;
                spawned = homing_spawn_circlers(slot, cx);
            }
            if spawned {
                let projectile = &mut cx.effects.projectiles[slot];
                projectile.vx = i16::from_be_bytes([0, projectile.vx.to_be_bytes()[1]]);
            } else {
                // The ROM's `STA $7D00,X` with X = $FF.
                cx.set(15, Field::Timer, 0);
            }
        }
    }
    work.y = work.y.wrapping_add(0x0380);
    if project_keep(slot, work, cx) || work.anchor & anchor::BOTTOM != 0 {
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
        if projectile.is_charged() {
            projectile.free();
        }
    }
}

/// `$85CF`: three type `$82` parts, in free slots first, then over the plain shots;
/// their slots kept in the charge's `$7F50` (two nibbles) and `$7F60`. False when
/// even that finds too few and every slot is freed (`$8617`, which leaves X = `$FF`).
fn homing_spawn_circlers(source: usize, cx: &mut Cx<'_>) -> bool {
    #[cfg(test)]
    super::super::oracle::seen(0x85CF);
    let mut parts = [0u8; 3];
    let mut count = 0;
    let mut at = 15i8;
    let mut over_shots = false;
    loop {
        if at < 0 {
            if over_shots {
                #[cfg(test)]
                super::super::oracle::seen(0x8617);
                for projectile in &mut cx.effects.projectiles {
                    projectile.free();
                }
                return false;
            }
            over_shots = true;
            at = 15;
            continue;
        }
        let index = usize::try_from(at).expect("non-negative");
        let existing = cx.effects.projectiles[index];
        if (over_shots && !existing.is_charged()) || existing.is_free() {
            let angle = cx.prg.weapons.circle_angles[count];
            let projectile = &mut cx.effects.projectiles[index];
            projectile.kind = HomingStage::Circling.byte();
            projectile.damage = 1;
            projectile.flags = shot_flags::INERT;
            projectile.state = 0;
            projectile.set_orbit(Orbit::from_byte(angle));
            parts[count] = at.cast_unsigned();
            count += 1;
            if count == 3 {
                cx.effects.projectiles[source].set_circlers(parts);
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
    super::super::oracle::seen(0x8525);
    work.pointer = rom_word(cx.prg, 5, 0x887F);
    work.attr = 0;
    let centre = cx.effects.projectiles[slot];
    for part in centre.circlers() {
        let part = usize::from(part);
        cx.effects.projectiles[part].flags = shot_flags::UNSTOPPABLE;
        let (dx, dy) = cx.prg.weapons.circle_offsets[cx.effects.projectiles[part].orbit().offset()];
        work.x = centre.x.wrapping_add(dx);
        work.y = centre.y.wrapping_add(dy);
        let projectile = &mut cx.effects.projectiles[part];
        projectile.set_orbit(projectile.orbit().next());
        if project_keep(slot, work, cx) || work.anchor & anchor::BOTTOM != 0 {
            draw(work, cx);
            let projectile = &mut cx.effects.projectiles[part];
            projectile.screen_x = work.screen_x;
            projectile.screen_y = work.screen_y;
        } else {
            cx.effects.projectiles[part].flags = shot_flags::INERT;
        }
    }
}

/// `$864A`: the charge becomes type `$81`, thrown by the pad's direction (none:
/// down when aiming up, else the way he faces) from in front of the player.
fn homing_launch(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::super::oracle::seen(0x864A);
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
        super::super::oracle::seen(0x8668);
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
    projectile.kind = HomingStage::Thrown.byte();
    homing_in_front(slot, work, cx);
}

/// Type `$81` (`$8515`): the thrown charge flies, its parts circling; off the
/// screen it is freed with every other charged slot.
fn homing_thrown(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::super::oracle::seen(0x8515);
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
    projectile.x = x;
    projectile.y = y;
    (work.x, work.y) = (x, y);
}
