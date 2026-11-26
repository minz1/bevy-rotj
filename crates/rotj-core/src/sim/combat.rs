//! Actor hit-boxes, player/projectile contact, boss damage, dying animations, and pickups.

use super::engine::{
    DEFEAT_SCRIPT, PICKUP, Visibility, Work, draw, on_screen, rom_bytes, rom_word,
};
use super::oam::attr;
use super::slots::{Body, Counter, contact, hit};
use super::spawn::STATE_DROPS_PICKUP;
use super::world::{
    Cx, Effects, Motion, Player, Weapon, World, attack, busy, hurt, pose, power_up, shot_flags,
    shot_state, sliding, sound,
};

/// How close the player must be to touch an actor, in pixels.
#[derive(Clone, Copy)]
pub struct Hitbox {
    /// Most x distance.
    pub half_width: u8,
    /// Most distance the player, or a projectile, may be above the actor.
    pub height: u8,
}

/// The hit-box tables (banks 13 and 14), each by [`Work::hitbox`] unless noted.
pub struct Hitboxes {
    /// [`contact`]'s (`$ABB2`, `$ABC1`).
    pub actors: [Hitbox; 15],
    /// [`projectile_hits`]' x reach (`$ABD0`).
    pub actor_projectile_half_widths: [u8; 15],
    /// [`boss_contact`]'s (`$B0F9`, `$B0FE`).
    pub bosses: [Hitbox; 5],
    /// [`boss_contact`]'s x reach for projectiles (`$B103`).
    pub boss_projectile_half_widths: [u8; 5],
    /// [`touch`]'s, for actors that only hurt (`$AF2A`, `$AF31`).
    pub hazards: [Hitbox; 7],
    /// [`player_proximity`]'s, whose callers all use hit box 0 (`$A0A7`, `$A0A8`).
    pub exit: Hitbox,
    /// Most x distance to ride a second-class platform, by its `scratch` (bank 14 `$CE7D`).
    pub platform_half_widths: [u8; 2],
}

impl Hitboxes {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Hitboxes {
        Hitboxes {
            actors: hitboxes(prg, 0xABB2, 0xABC1),
            actor_projectile_half_widths: rom_bytes(prg, 13, 0xABD0),
            bosses: hitboxes(prg, 0xB0F9, 0xB0FE),
            boss_projectile_half_widths: rom_bytes(prg, 13, 0xB103),
            hazards: hitboxes(prg, 0xAF2A, 0xAF31),
            exit: hitboxes::<1>(prg, 0xA0A7, 0xA0A8)[0],
            platform_half_widths: rom_bytes(prg, 14, 0xCE7D),
        }
    }
}

/// A bank-13 hit-box table from its half-width and height columns.
fn hitboxes<const N: usize>(prg: &[u8], half_width: u16, height: u16) -> [Hitbox; N] {
    let (x, y): ([u8; N], [u8; N]) = (rom_bytes(prg, 13, half_width), rom_bytes(prg, 13, height));
    std::array::from_fn(|id| Hitbox {
        half_width: x[id],
        height: y[id],
    })
}

fn overlaps(work: &Work, player: &Player, hitbox: Hitbox) -> bool {
    let near_y = if work.screen_y < player.screen_y {
        player.screen_y - work.screen_y < player.reach_y
    } else {
        work.screen_y - player.screen_y < hitbox.height
    };
    work.screen_x.abs_diff(player.screen_x) < hitbox.half_width && near_y
}

/// Contact with the player (`$A9C4`): an item is collected, the actor or the player hurt.
pub fn contact(kind: &mut u8, body: &mut Body, work: &mut Work, damage: u8, cx: &mut Cx<'_>) {
    let checks_player = if body.contact & contact::PASSIVE == 0 {
        cx.effects.player.sliding & sliding::ON != 0
            || (cx.effects.player.power_up & power_up::ON == 0
                && cx.effects.player.hurt & hurt::INVULNERABLE == 0)
    } else {
        body.contact & contact::COLLECTS != 0
    };
    let hitbox = cx.prg.hitboxes.actors[usize::from(work.hitbox)];
    if checks_player && overlaps(work, &cx.effects.player, hitbox) {
        if body.contact & contact::PASSIVE != 0 {
            collect(kind, body, cx);
            return;
        }
        if cx.effects.player.sliding & sliding::ON != 0 && body.contact & contact::SLIDE_HURTS != 0
        {
            hurt_actor(body, work, cx);
            return;
        }
        if cx.effects.player.sliding & sliding::ON == 0
            || (cx.effects.player.power_up & power_up::ON == 0
                && cx.effects.player.hurt & hurt::INVULNERABLE == 0)
        {
            hurt_player(cx.effects);
            let player = &mut cx.effects.player;
            player.energy = player.energy.wrapping_sub(damage);
        }
    }
    projectile_hits(body, work, cx);
}

/// Checks the player's projectiles against the actor: shields deflect, others hurt (`$AA57`).
pub fn projectile_hits(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    let id = usize::from(work.hitbox);
    let (half_width, height) = (
        cx.prg.hitboxes.actor_projectile_half_widths[id],
        cx.prg.hitboxes.actors[id].height,
    );
    let [seed, noise] = cx.world.rng;
    for index in (0..16).rev() {
        let projectile = cx.effects.projectiles[index];
        if projectile.is_free() || projectile.flags & shot_flags::HARMLESS != 0 {
            continue;
        }
        if work.screen_x.abs_diff(projectile.screen_x) >= half_width {
            continue;
        }
        let near_y = if work.screen_y < projectile.screen_y {
            projectile.screen_y - work.screen_y < 5
        } else {
            work.screen_y - projectile.screen_y < height
        };
        if !near_y {
            continue;
        }
        let projectile = &mut cx.effects.projectiles[index];
        if body.contact & contact::SHIELDED != 0 {
            if projectile.flags & shot_flags::PASSES_SHIELDS != 0 {
                continue;
            }
            cx.effects.sound.push(0, sound::SHOT_DEFLECTED);
            projectile.flags |= shot_flags::INERT;
            let away_right = if projectile.vx == 0 {
                noise & 0x01 == 0
            } else {
                projectile.vx < 0
            };
            projectile.vx = if away_right {
                i16::from(noise & 0x3F)
            } else {
                i16::from_be_bytes([0xFF, !(noise & 0x3F)])
            };
            projectile.state |= shot_state::FALLING;
            projectile.timer = 0x20;
            projectile.vy = i16::from_be_bytes([0xFF, !(seed & 0x7F)]);
            continue;
        }
        cx.effects.sound.push(0, sound::ACTOR_HIT);
        spend(projectile, seed);
        let (health, killed) = body.health.overflowing_sub(u16::from(projectile.damage));
        body.health = health;
        if killed {
            die(body, work, cx);
        } else if body.hit & hit::FLASHING == 0 {
            body.hit = hit::STRUCK;
        }
        return;
    }
}

/// A projectile that hit bounces off and is spent (`$B108`).
fn spend(projectile: &mut super::world::Projectile, rng_f9: u8) {
    if projectile.flags & shot_flags::PIERCING != 0 {
        return;
    }
    projectile.flags |= shot_flags::HARMLESS;
    let random = if rng_f9 & 0x80 == 0 { 0x20 } else { -0x20 };
    if projectile.is_aimed_up() {
        projectile.vy = 0x20;
        projectile.vx = random;
    } else {
        projectile.vx = if projectile.vx < 0 { 0x20 } else { -0x20 };
        projectile.vy = random;
    }
    projectile.state = shot_state::SPENT;
    projectile.attr = 0;
    projectile.param = 0;
    projectile.timer = 4;
}

/// The player's attack lands: 8 off the actor's health, then a flash or death (`$AA2C`).
fn hurt_actor(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    cx.effects.sound.push(0, sound::ACTOR_HIT);
    let (health, killed) = body.health.overflowing_sub(8);
    body.health = health;
    if killed {
        die(body, work, cx);
    } else if body.hit & hit::FLASHING == 0 {
        body.hit = hit::STRUCK;
    }
}

/// The actor is killed: sound `$21`, 76 px lower, the dying animation starts (`$AB13`).
fn die(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    cx.effects.sound.push(0, sound::ACTOR_DIES);
    work.y = work.y.wrapping_add(0x04C0);
    body.hit = hit::DYING;
    body.dying_timer = 0;
}

/// Outcome of [`boss_flash`] (`$AF97`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BossFlash {
    /// The boss is alive; `work.attr` carries its normal or hit-flash palette.
    Alive,
    /// The boss is dying; `show` is true on frames its body still draws (odd frames before `$40`).
    Dying {
        /// Whether the dying boss still draws this frame (`$C0` vs `$80`).
        show: bool,
    },
}

impl BossFlash {
    /// Maps back to the 6502 accumulator `$AF97` returns, for oracle-A tests.
    #[cfg(test)]
    pub(crate) const fn to_accumulator(self) -> u8 {
        match self {
            Self::Alive => 0,
            Self::Dying { show: true } => 0xC0,
            Self::Dying { show: false } => 0x80,
        }
    }
}

/// Which collision arms [`boss_contact`] (`$AFD6`, `Y` register) runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactMode {
    /// Player touch and projectile scan (`Y = $00`).
    Both,
    /// Player touch only; no slide damage and no projectile scan (`Y = $80`).
    TouchOnly,
    /// Projectile scan only; skips player touch (`Y = $40`/`$C0`).
    ShotsOnly,
}

impl ContactMode {
    #[cfg(test)]
    pub(crate) const fn from_byte(y: u8) -> Self {
        if y & 0x40 != 0 {
            Self::ShotsOnly
        } else if y & 0x80 != 0 {
            Self::TouchOnly
        } else {
            Self::Both
        }
    }
}

/// Outcome of [`boss_contact`] (`$AFD6`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BossContact {
    /// No reported hit or damage (`A = $00`).
    None,
    /// Hurt the player in [`ContactMode::TouchOnly`] (`A = $80`).
    HurtPlayer,
    /// Took damage while already flashing or on the killing blow (`A = hit::FLASHING`).
    Flashing,
    /// Took a fresh hit and started flashing (`A = hit::STRUCK`).
    Struck,
}

impl BossContact {
    /// Whether the boss took damage this frame (either fresh or while flashing).
    #[must_use]
    pub const fn was_damaged(self) -> bool {
        matches!(self, Self::Flashing | Self::Struck)
    }

    /// Maps back to the 6502 accumulator `$AFD6` returns, for oracle-A tests.
    #[cfg(test)]
    pub(crate) const fn to_accumulator(self) -> u8 {
        match self {
            Self::None => 0,
            Self::HurtPlayer => 0x80,
            Self::Flashing => hit::FLASHING,
            Self::Struck => hit::STRUCK,
        }
    }
}

/// Counts down an actor's hit-flash timer (`body.hit`), clearing bit 7 first (`(hit & $7F) - 1`).
fn tick_flash(hit: &mut u8) {
    *hit = (*hit & 0x7F).wrapping_sub(1);
}

/// `body.dying_timer` once a boss's death flicker finishes (`$AF97`).
const BOSS_DYING_DONE: u8 = 0x80;

/// A boss's hit flash or dying count (`$AF97`).
pub fn boss_flash(body: &mut Body, work: &mut Work, world: &World) -> BossFlash {
    if body.hit & hit::DYING != 0 {
        body.dying_timer.tick_up();
        if body.dying_timer >= 0x40 {
            body.dying_timer = BOSS_DYING_DONE;
            return BossFlash::Dying { show: false };
        }
        return BossFlash::Dying {
            show: world.frame_odd,
        };
    }
    if body.hit & hit::FLASHING == 0 {
        work.attr = body.flags & !attr::PALETTE;
    } else {
        tick_flash(&mut body.hit);
        work.attr = body.flags | attr::PALETTE;
    }
    BossFlash::Alive
}

/// A boss against the player and his projectiles (`$AFD6`); touching him drains `amount` from `$B2`.
pub fn boss_contact(
    amount: u16,
    mode: ContactMode,
    body: &mut Body,
    work: &Work,
    cx: &mut Cx<'_>,
) -> BossContact {
    let id = usize::from(work.hitbox);
    let hitbox = cx.prg.hitboxes.bosses[id];
    let (half_width, height) = (
        cx.prg.hitboxes.boss_projectile_half_widths[id],
        hitbox.height,
    );
    if mode != ContactMode::ShotsOnly {
        let attacking = cx.effects.player.sliding & sliding::ON != 0;
        let guarded = cx.effects.player.hurt & hurt::INVULNERABLE != 0;
        if (attacking || !guarded) && overlaps(work, &cx.effects.player, hitbox) {
            if attacking && cx.effects.boss_hp == 0 {
                return BossContact::None;
            }
            if attacking
                && mode != ContactMode::TouchOnly
                && body.contact & contact::SLIDE_HURTS != 0
            {
                cx.effects.sound.push(0, sound::IMPACT);
                return boss_damaged(8, body, cx);
            }
            if !attacking || !guarded {
                hurt_player(cx.effects);
                let pool = u16::from_le_bytes(cx.effects.boss_pool).wrapping_sub(amount);
                cx.effects.boss_pool = pool.to_le_bytes();
                return if mode == ContactMode::TouchOnly {
                    BossContact::HurtPlayer
                } else {
                    BossContact::None
                };
            }
        }
        if mode == ContactMode::TouchOnly {
            return BossContact::None;
        }
    }
    for index in (0..16).rev() {
        let projectile = &mut cx.effects.projectiles[index];
        if projectile.is_free() || projectile.flags & shot_flags::HARMLESS != 0 {
            continue;
        }
        if work.screen_x.abs_diff(projectile.screen_x) >= half_width {
            continue;
        }
        let near_y = if work.screen_y < projectile.screen_y {
            projectile.screen_y - work.screen_y < 3
        } else {
            work.screen_y - projectile.screen_y < height
        };
        if !near_y {
            continue;
        }
        spend(projectile, cx.world.rng[0]);
        let damage = projectile.damage << 4;
        return boss_damaged(damage, body, cx);
    }
    BossContact::None
}

fn boss_damaged(damage: u8, body: &mut Body, cx: &mut Cx<'_>) -> BossContact {
    let (hp, killed) = cx.effects.boss_hp.overflowing_sub(u16::from(damage));
    cx.effects.boss_hp = hp;
    if killed || hp == 0 {
        cx.effects.sound.push(1, sound::EXPLOSION);
        body.hit = hit::DYING;
        body.dying_timer = 0;
        cx.effects.boss_hp = 0;
        BossContact::Flashing
    } else if body.hit & hit::FLASHING != 0 {
        BossContact::Flashing
    } else {
        body.hit = hit::STRUCK;
        BossContact::Struck
    }
}

/// The boss is beaten: music `$FC`, `$AF` set and a type-`$50` slot with `timer` (`$B1C4`).
pub fn boss_defeated(timer: u8, cx: &mut Cx<'_>) {
    cx.effects.busy = busy::SCRIPTED;
    cx.effects.music = sound::music::FADE_OUT;
    // ponytail: no free slot spawns nothing; the game writes past the slot arrays then
    let Some(index) = cx.slots.free_main(0..16) else {
        return;
    };
    let slot = &mut cx.slots.main[index];
    slot.body.counter = 0;
    slot.body.anim = 1;
    slot.body.timer = timer;
    slot.kind = DEFEAT_SCRIPT;
    slot.brain = super::families::Brain::new(DEFEAT_SCRIPT);
}

/// Contact that only hurts the player, with no weapon scan (`$AEE3`); true when it hit.
pub fn touch(work: &Work, damage: u8, cx: &mut Cx<'_>) -> bool {
    if cx.effects.player.power_up & power_up::ON != 0
        || cx.effects.player.hurt & hurt::INVULNERABLE != 0
        || !overlaps(
            work,
            &cx.effects.player,
            cx.prg.hitboxes.hazards[usize::from(work.hitbox)],
        )
    {
        return false;
    }
    hurt_player(cx.effects);
    let player = &mut cx.effects.player;
    player.energy = player.energy.wrapping_sub(damage);
    true
}

/// The player is hit: sound `$25`, hurt timer, and knock-back when standing (`$B17F`).
pub fn hurt_player(effects: &mut Effects) {
    effects.sound.push(0, sound::PLAYER_HURT);
    effects.attack &= attack::CHARGEABLE;
    let player = &mut effects.player;
    let knock_back = start_hurt(player);
    if player.motion.on_foot() || player.motion == Motion::AimingUp {
        player.vx = knock_back;
    }
}

/// The hurt's start, the same for a damage tile (`$82B9`) and an actor (`$B17F`): the shot
/// pose dropped, stunned and invulnerable for 32 frames, a fall turned into a jump; the
/// knock-back speed, away from where Batman faces. The callers add the sound and drop a charge.
pub fn start_hurt(player: &mut Player) -> i16 {
    player.pose &= !pose::SHOT;
    player.hurt_timer = 0x20;
    player.hurt = hurt::STUNNED | hurt::INVULNERABLE;
    if player.motion == Motion::Falling {
        player.motion = Motion::Jumping;
    }
    if player.faces_left() { 0x20 } else { -0x20 }
}

/// The hit flash, or dying and then freed or turned into item drop `$10` (`$A56E`).
pub fn dying(
    kind: &mut u8,
    body: &mut Body,
    work: &mut Work,
    variant: u8,
    cx: &mut Cx<'_>,
) -> bool {
    if body.hit & hit::DYING == 0 {
        if body.hit & hit::FLASHING == 0 {
            work.attr = body.flags & !attr::PALETTE;
        } else {
            tick_flash(&mut body.hit);
            work.attr = body.flags | attr::PALETTE;
        }
        return false;
    }
    let frame = u16::from(body.dying_timer & 0xFE);
    let (limit, table) = if variant & 0x80 == 0 {
        (0x0E, 0xA61F)
    } else {
        (0x10, 0xA62D)
    };
    work.pointer = rom_word(cx.prg, 13, table + frame);
    work.attr = 3;
    work.chr_slot = 4;
    work.tile = 0;
    if on_screen(work, cx.world) == Visibility::On {
        draw(work, 13, cx);
    }
    body.dying_timer.tick_up();
    if body.dying_timer < limit {
        return true;
    }
    if body.state & STATE_DROPS_PICKUP == 0 {
        *kind = super::slots::EMPTY;
        return true;
    }
    *kind = PICKUP;
    body.phase = 0;
    body.flags = 0;
    body.vx_drift = 0;
    body.vx = 0;
    body.ref_vx = 0;
    body.vy = -0x40;
    body.contact = contact::PASSIVE | contact::COLLECTS | contact::SMITE_PROOF;
    true
}

/// Picks up an item drop (`$AB39`): the weapon, or meter capsules up to the level end.
pub fn collect(kind: &mut u8, body: &Body, cx: &mut Cx<'_>) {
    let effects = &mut *cx.effects;
    effects.sound.push(1, sound::PICKUP);
    if effects.player.out_of_energy() {
        *kind = super::slots::EMPTY;
        return;
    }
    match body.state & !STATE_DROPS_PICKUP {
        0..=3 => {
            if effects.player.power_up & power_up::ON == 0 {
                effects.weapon = Weapon::from_byte((body.state & !STATE_DROPS_PICKUP) + 1)
                    .expect("a pickup gives weapons 1-4");
            }
            effects.attack = attack::CHARGEABLE;
            for projectile in &mut effects.projectiles {
                projectile.free();
            }
        }
        4 => {
            let meter = effects.meter.wrapping_add(2);
            if meter < 0x0E {
                effects.meter = meter;
            } else {
                effects.meter = 0x0E;
                if effects.player.power_up & power_up::ON == 0 {
                    effects.stored_weapon = effects.weapon;
                    effects.transition = super::world::Transition::PowerUp;
                    effects.busy = busy::SCRIPTED;
                }
            }
        }
        state => panic!("item state {state} jumps past the $ABA8 table"),
    }
    *kind = super::slots::EMPTY;
}
