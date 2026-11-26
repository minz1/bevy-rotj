//! Batman's weapon firing, melee attacks, and target acquisition (`$9AF5`, `$9063`).

use super::super::slots::{Counter, contact};
use super::super::world::{Motion, Projectile, Weapon, attack, shot_flags, sound};
use super::{PARTNER, Port, SHOT_AIMED_UP};
use crate::sim::button::Buttons;
use crate::sim::spawn::STATE_UNTARGETED;

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
    (projectile.x, projectile.y) = (source.x, source.y);
}

impl Port<'_, '_> {
    /// The `$B8`-gated projectile spawn most projectile poses share (`$866C`, `$88A7`, ...).
    pub(super) fn reset_and_shoot(&mut self) {
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
    pub(super) fn spawn_shot(&mut self) {
        let weapon = self.cx.effects.weapon;
        let start = self.cx.prg.player.slot_limits[usize::from(weapon.byte())];
        let Some(slot) = (0..=usize::from(start))
            .rev()
            .find(|&slot| self.cx.effects.projectiles[slot].is_free())
        else {
            return;
        };
        let motion = usize::from(self.player.motion.byte());
        let aim_up = self.player.motion == Motion::AimingUp;
        let face_left = self.player.faces_left();
        let launch = &self.cx.prg.player;
        let (x_offset, y_offset) = match (aim_up, face_left) {
            (true, true) => (-0x20, -0x410),
            (true, false) => (0x20, -0x410),
            (false, true) => (launch.launch_x_left[motion], launch.launch_y[motion]),
            (false, false) => (launch.launch_x_right[motion], launch.launch_y[motion]),
        };
        let (x, x_overflow) = x_offset.overflowing_add(self.player.x.cast_signed());
        let (y, y_overflow) = y_offset.overflowing_add(self.player.y.cast_signed());
        self.overflow = if aim_up { y_overflow } else { x_overflow };
        let projectile = &mut self.cx.effects.projectiles[slot];
        projectile.kind = weapon.shot_kind();
        projectile.x = x.cast_unsigned();
        projectile.y = y.cast_unsigned();
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
        super::super::oracle::seen(handler);
        self.cx.effects.sound.push(
            0,
            match handler {
                0x9C06 | 0x9D6F => sound::SPLITTER_SHOT,
                0x9C2E | 0x9CD9 | 0x9DA0 | 0x9E72 => sound::WAVE_SHOT,
                0x9C7E | 0x9E09 => sound::THREE_WAY_SHOT,
                _ => sound::SHOT,
            },
        );
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
    pub(super) fn shoot_up(&mut self, slot: usize, handler: u16) {
        let fan = usize::from((self.cx.world.frame >> 2) & 0x0F);
        match handler {
            0x9BCE => self.shot_up(slot, -0x40, 1),
            0x9BF5 => {
                self.shot_up(slot, -0x20, 2);
                self.cx.effects.projectiles[slot].set_target(self.target());
            }
            0x9C06 => self.shot_up(slot, -0x10, 2),
            0x9C2E => {
                let vx = self.cx.prg.player.pair_velocities;
                let weave = vx.map(|v| v.to_be_bytes()[0]);
                let part = |projectile: &mut Projectile, y: usize| {
                    projectile.vy = -0x40;
                    projectile.timer = 0xFF;
                    projectile.vx = vx[y];
                    projectile.state = 0;
                    projectile.flags = 0;
                    projectile.param = weave[y];
                    projectile.damage = 2;
                };
                part(&mut self.cx.effects.projectiles[slot], 1);
                let source = self.cx.effects.projectiles[slot];
                self.spawn_parts(1, |projectile, _| {
                    part(projectile, 0);
                    projectile.kind = PARTNER;
                    at_source(projectile, &source);
                });
            }
            0x9C7E => {
                let vx = self.cx.prg.player.threeway_up_velocities;
                let part = |projectile: &mut Projectile, n: usize| {
                    projectile.vy = -0x40;
                    projectile.vx = vx[n];
                    projectile.timer = SHOT_AIMED_UP;
                    projectile.state = 0;
                    projectile.attr = 0;
                    projectile.flags = 0;
                    projectile.kind = Weapon::ThreeWay.shot_kind();
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
                projectile.timer = SHOT_AIMED_UP;
                projectile.state = 0;
                projectile.flags = 0;
                projectile.damage = 4;
            }
            other => panic!("${other:04X} is not an upward projectile handler"),
        }
    }

    /// The forward projectile handlers `$9EC1[$0476]` (`$9D2D`-`$9EB4`).
    pub(super) fn shoot_ahead(&mut self, slot: usize, handler: u16, face_left: bool) {
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
                projectile.set_target(target);
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
                let weave = vy.map(|v| v.to_be_bytes()[0]);
                let (attr, vx) = (
                    self.cx.prg.player.shot_attributes[right],
                    self.cx.prg.player.wave_launch[right],
                );
                let part = |projectile: &mut Projectile, y: usize| {
                    projectile.vy = vy[y];
                    projectile.param = weave[y];
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
                    projectile.kind = PARTNER;
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
                    projectile.kind = Weapon::ThreeWay.shot_kind();
                    projectile.vy = vy[n];
                    projectile.timer = SHOT_AIMED_UP;
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
    pub(super) fn shot_up(&mut self, slot: usize, vy: i16, damage: u8) {
        let projectile = &mut self.cx.effects.projectiles[slot];
        projectile.vy = vy;
        projectile.timer = SHOT_AIMED_UP;
        projectile.flags = 0;
        projectile.vx = 0;
        projectile.state = 0;
        projectile.damage = damage;
    }

    /// Fills the free slots below 16, highest first, with the parts numbered
    /// `last - 1` down to 0 (`$9C39`, `$9C8D`, `$9AAD`). Returns the X it leaves:
    /// part 0's slot, or `$FF` when the slots ran out.
    pub(super) fn spawn_parts(
        &mut self,
        last: usize,
        mut part: impl FnMut(&mut Projectile, usize),
    ) -> u8 {
        let mut free = (0..16u8)
            .rev()
            .filter(|&slot| self.cx.effects.projectiles[usize::from(slot)].is_free())
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
    pub(super) fn spawn_melee(&mut self) -> u8 {
        let Some(slot) = (0..16)
            .rev()
            .find(|&slot| self.cx.effects.projectiles[slot].is_free())
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
            projectile.kind = super::super::weapons::CHARGE_START;
            projectile.part_timer = 0;
            projectile.flags = shot_flags::INERT;
        };
        let handler = self.rw(0x9A75 + u16::from(weapon.byte()) * 2);
        #[cfg(test)]
        super::super::oracle::seen(handler);
        match handler {
            0x9A81 => {
                self.cx.effects.sound.push(0, sound::CHARGE);
                let projectile = &mut self.cx.effects.projectiles[slot];
                charge(projectile);
                projectile.param = 0;
                projectile.part = 0;
                projectile.damage = 0;
                projectile.vx = i16::from_be_bytes([0, projectile.vx.to_be_bytes()[1]]);
            }
            0x9AA0 => {
                self.cx.effects.sound.push(0, sound::SPLITTER_ORBIT);
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

    /// `$9F05`: a live, hittable actor slot below 16 from a random start.
    pub(super) fn target(&self) -> Option<u8> {
        let [direction, start] = self.cx.world.rng;
        let mut slot = start & 0x0F;
        for _ in 0..16 {
            let actor = &self.cx.slots.main[usize::from(slot)];
            if !actor.is_free()
                && actor.body.contact & contact::UNTARGETED == 0
                && actor.body.state & STATE_UNTARGETED == 0
            {
                return Some(slot);
            }
            slot = if direction & 0x80 != 0 {
                (slot + 1) & 0x0F
            } else {
                slot.wrapping_sub(1) & 0x0F
            };
        }
        None
    }

    /// `$9359`: the fire button's spawn and muzzle handling.
    pub(super) fn fire(&mut self) {
        if self.cx.world.frame.is_multiple_of(4) {
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
                self.player.charge_timer.tick_up();
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
}
