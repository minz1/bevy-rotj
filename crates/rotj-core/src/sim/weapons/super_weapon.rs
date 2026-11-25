//! Weapon 5 (Power-up Super) and its charged screen-clearing attack (`$9647`).

use super::super::engine::{Work, rom_word};
use super::super::scroll::Scroll;
use super::super::slots::{Counter, contact, hit};
use super::super::world::{
    Cx, Motion, Weapon, attack, busy, power_up, set_first_colours, shot_flags, sound, stage_script,
};
use super::{
    CHARGE_START, LIT, above_player, advance, ahead, beside_player, draw, flying_shot,
    project_keep, sprite_by_timer,
};
use crate::palette;

/// The super weapon's attack.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SuperStage {
    /// The charge that claims the other slots.
    Unleash = CHARGE_START,
    /// A part orbiting the centre.
    Orbiter = 0x81,
    /// The pair spawner.
    Pairs = 0x82,
    /// A slot the attack claimed.
    Claimed = 0x83,
    /// The orbit's centre.
    Centre = 0x84,
}

impl SuperStage {
    pub(super) const fn byte(self) -> u8 {
        self as u8
    }

    const fn from_byte(byte: u8) -> Option<SuperStage> {
        match byte {
            CHARGE_START => Some(SuperStage::Unleash),
            0x81 => Some(SuperStage::Orbiter),
            0x82 => Some(SuperStage::Pairs),
            0x83 => Some(SuperStage::Claimed),
            0x84 => Some(SuperStage::Centre),
            _ => None,
        }
    }
}

/// and up are its super attack. False when the handler returns before the other slots.
pub(super) fn super_weapon(
    slot: usize,
    work: &mut Work,
    cx: &mut Cx<'_>,
    scroll: &mut Scroll,
    overflow: &mut bool,
) -> bool {
    if cx.effects.projectiles[slot].is_charged() {
        return super_attack(slot, work, cx, scroll, overflow);
    }
    #[cfg(test)]
    super::super::oracle::seen(0x9668);
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
    let kind = cx.effects.projectiles[slot].kind;
    match SuperStage::from_byte(kind) {
        Some(SuperStage::Unleash) => super_unleash(slot, cx, scroll, *overflow),
        Some(SuperStage::Orbiter) => {
            *overflow = false;
            super_orbiter(slot, work, cx, scroll)
        }
        Some(SuperStage::Pairs) => {
            super_pairs(slot, cx);
            true
        }
        Some(SuperStage::Claimed | SuperStage::Centre) => true,
        None => panic!("handler 5's type ${kind:02X} is past the $9D24 table"),
    }
}

/// Type `$80` (`$9790`): with the meter at 2 or more, the charge claims every
/// other slot (type `$83`), stops the scroll and the player (`$0481` bit 7) and
/// becomes the type `$82` that releases the orbiting pairs; with less it is
/// dropped and the handler returns.
///
/// Classic's rule: the charge waits while `overflow`, the 6502's overflow flag, is set.
/// The game tests `LDA $B8; BVC`, and `LDA` leaves V as the code before left it
/// (`docs/re/ACTORS.md` §9g): the last writer among Batman's frame (`$84AB`: set while B is
/// held, while he stands invulnerable, or turning mid-jump or rising from a crouch), the
/// last live second-class actor's projection and draw (which usually clear it), and a shot
/// in a higher slot.
fn super_unleash(slot: usize, cx: &mut Cx<'_>, scroll: &mut Scroll, overflow: bool) -> bool {
    #[cfg(test)]
    super::super::oracle::overflow_at(0x9790, overflow);
    if overflow {
        return true;
    }
    if cx.effects.meter < 2 {
        #[cfg(test)]
        super::super::oracle::seen(0x979E);
        cx.effects.attack &= attack::CHARGEABLE;
        cx.effects.projectiles[slot].free();
        return false;
    }
    #[cfg(test)]
    super::super::oracle::seen(0x97AC);
    cx.effects.busy = busy::SCRIPTED;
    cx.effects.sound[0] = sound::SUPER_UNLEASH;
    cx.effects.player.stage_script |= stage_script::FROZEN;
    scroll.gate = 0;
    scroll.speed_x = 0;
    scroll.speed_y = 0;
    let kept = cx.effects.scroll_lock;
    cx.effects.scroll_lock = 0xFF;
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.kind = SuperStage::Pairs.byte();
    projectile.part = 0;
    projectile.part_timer = 0;
    projectile.timer = kept;
    for projectile in &mut cx.effects.projectiles {
        if !projectile.is_charged() {
            projectile.kind = SuperStage::Claimed.byte();
            projectile.flags = shot_flags::INERT;
        }
    }
    let (x, y) = if cx.effects.player.motion == Motion::AimingUp {
        #[cfg(test)]
        super::super::oracle::seen(0x97F9);
        above_player(cx)
    } else {
        let motion = usize::from(cx.effects.player.motion.byte());
        beside_player(cx, ahead(cx, motion), cx.prg.weapons.ahead_dy[motion])
    };
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.x = x;
    projectile.y = y;
    true
}

/// Type `$82` (`$9B4A`): every 17th frame, two claimed slots become a type `$81`
/// orbiter and the type `$84` centre it circles, at the charge's position; after
/// five pairs the charge is claimed itself.
fn super_pairs(slot: usize, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::super::oracle::seen(0x9B4A);
    let projectile = &mut cx.effects.projectiles[slot];
    if projectile.part != 0x10 {
        projectile.part.tick_up();
        return;
    }
    projectile.part = 0;
    let claimed = |cx: &Cx<'_>, below: usize| {
        (0..below)
            .rev()
            .find(|&i| cx.effects.projectiles[i].kind == SuperStage::Claimed.byte())
            .expect("the charge claimed every other slot, enough for five pairs")
    };
    let orbiter = claimed(cx, 16);
    cx.effects.projectiles[orbiter].kind = SuperStage::Orbiter.byte();
    let centre = claimed(cx, orbiter);
    cx.effects.projectiles[centre].kind = SuperStage::Centre.byte();
    let source = cx.effects.projectiles[slot];
    let orbiter_weapon = &mut cx.effects.projectiles[orbiter];
    (orbiter_weapon.x, orbiter_weapon.y) = (0x0100, 0);
    orbiter_weapon.flags = shot_flags::INERT;
    orbiter_weapon.param = 0;
    orbiter_weapon.part = if source.part_timer == 0 { LIT } else { 0 };
    orbiter_weapon.vx = -8;
    orbiter_weapon.vy = 0x0080;
    orbiter_weapon.timer = source.timer;
    orbiter_weapon.state = source.state;
    orbiter_weapon.part_timer = u8::try_from(centre).expect("slot < 16");
    let centre_weapon = &mut cx.effects.projectiles[centre];
    centre_weapon.x = source.x;
    centre_weapon.y = source.y;
    centre_weapon.vy = 0x0080;
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.part_timer.tick_up();
    if projectile.part_timer >= 5 {
        projectile.kind = SuperStage::Claimed.byte();
    }
}

/// Type `$81` (`$9843`): an orbiter. Each frame it strikes every actor on the
/// screen (`$9F4F`), springs about its centre (its position an offset from it,
/// drawn turned 45 degrees and mirrored on odd frames) and, for the first pair,
/// flashes the palette; after `$E0` frames the attack ends and the handler returns.
fn super_orbiter(slot: usize, work: &mut Work, cx: &mut Cx<'_>, scroll: &mut Scroll) -> bool {
    #[cfg(test)]
    super::super::oracle::seen(0x9843);
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
    work.x = centre_weapon.x.wrapping_add(dx.cast_unsigned());
    work.y = centre_weapon.y.wrapping_add(dy.cast_unsigned());
    if project_keep(slot, work, cx) {
        let hi = cx.effects.projectiles[slot].x.to_be_bytes()[0];
        let (table, step) = if hi & 0x80 != 0 {
            (0x9C99, !hi & 0x0F)
        } else {
            (0x9C19, hi & 0x0F)
        };
        work.pointer = rom_word(cx.prg, 5, table + u16::from(step << 1 | mirror));
        draw(work, cx);
    }
    let projectile = &mut cx.effects.projectiles[slot];
    projectile.param.tick_up();
    if projectile.param.is_multiple_of(32) {
        cx.effects.sound[0] = sound::SUPER_ORBITER;
    }
    if cx.effects.projectiles[slot].param >= 0xE0 {
        super_end_attack(slot, cx, scroll);
        return false;
    }
    super_spring(slot, centre, cx);
    if cx.effects.projectiles[slot].part & LIT != 0 {
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
    let above = projectile.y & 0x8000 != 0;
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
        projectile.y = 0;
        projectile.vy = projectile.vy.wrapping_add(0x30);
    } else {
        projectile.vx = (!projectile.vx).wrapping_add(0x0C);
        projectile.y = 0xFFFF;
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
    let first = if cx.effects.projectiles[slot].param.is_multiple_of(8) {
        palette::WHITE
    } else {
        palette::BLACK
    };
    set_first_colours(&mut cx.effects.palette, first);
    cx.effects.palette_dirty = 0x80;
}

/// `$9AF7`: the super attack ends: the power-up gone (`$0472` bit 7, the meter
/// `$FE`), the stored weapon back, the scroll and `$C2` restored, the palette's
/// sprite set from `$9D21` and every slot freed.
fn super_end_attack(slot: usize, cx: &mut Cx<'_>, scroll: &mut Scroll) {
    #[cfg(test)]
    super::super::oracle::seen(0x9AF7);
    let effects = &mut *cx.effects;
    effects.player.power_up &= !power_up::ON;
    effects.player.stage_script = 0;
    effects.meter = 0xFE;
    effects.weapon = effects.stored_weapon;
    effects.attack = if effects.weapon == Weapon::Bolt {
        0
    } else {
        attack::CHARGEABLE
    };
    effects.palette[17..20].copy_from_slice(&cx.prg.weapons.super_palette);
    effects.scroll_lock = effects.projectiles[slot].timer;
    set_first_colours(&mut effects.palette, palette::BLACK);
    effects.palette_dirty = 0x80;
    effects.busy = 0;
    for projectile in &mut effects.projectiles {
        projectile.free();
    }
    scroll.gate = 0xFF;
}

/// `$9F4F`: takes `$C0` hit points from every hittable actor in slots 0-15 on
/// the screen; one it kills drops `$4C` px and starts dying (`$7D70` = `$80`).
fn super_smite(cx: &mut Cx<'_>) {
    let [camera_x, _] = cx.world.camera_x.to_be_bytes();
    let camera_y = cx.world.camera_y;
    for actor in cx.slots.main[..16].iter_mut().rev() {
        if actor.is_free() {
            continue;
        }
        let body = &mut actor.body;
        if body.contact & contact::SMITE_PROOF != 0 || body.hit & hit::DYING != 0 {
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
