//! Batman's per-motion animation state handlers (standing, crouching, walking, jumping,
//! sliding, falling, aiming up, dying, boss entry, powering up, flying).

use super::super::engine::{self, adc_overflows, hi, lo, rom_word, sub_overflows16};
use super::super::oam::attr;
use super::super::slots::Counter;
use super::super::world::{
    EntryStep, Motion, Weapon, attack, busy, death, hurt, pose, power_up, set_first_colours,
    slide_ledge, sliding, sound, stage_script,
};
use super::{
    ENTRY_BANK, ENTRY_LINE_POINTERS, ENTRY_POSE_POINTERS, JUMP_VY, Port, SLIDE_ENDING,
    SLIDE_GLIDING, SLIDE_LEDGE_VY, SLIDE_VX_LEFT, SLIDE_VX_RIGHT,
};
use crate::palette;
use crate::render::ppu;
use crate::sim::button::Buttons;

impl Port<'_, '_> {
    /// `$8508`: whether a solid tile blocks the jump's launch (probes at x, x+$A0, x-$40).
    pub(super) fn jump_blocked(&mut self) -> bool {
        let y = hi(self.player.y.wrapping_sub(0x380));
        let x1 = self.player.x.wrapping_add(0xA0);
        let x2 = x1.wrapping_sub(0x0140);
        self.solid_x(self.player.x, y) || self.solid_x(x1, y) || self.solid_x(x2, y)
    }

    /// `$84D1`: motion 0, standing.
    pub(super) fn standing(&mut self) {
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
    pub(super) fn standing_resume(&mut self) {
        self.player.anim = 0x01;
        self.player.motion =
            Motion::from_byte(self.player.pose & pose::SAVED_MOTION).expect("a saved motion");
        self.player.pose &= pose::SHOT;
        if self.player.motion == Motion::Jumping {
            self.cx.effects.sound.push(0, sound::JUMP);
            if self.jump_blocked() {
                self.cx.effects.attack &= attack::CHARGEABLE;
                self.player.pose = pose::LANDING | pose::LONG_LANDING;
            } else {
                self.player.vy = JUMP_VY;
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
    pub(super) fn standing_input(&mut self) {
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
    pub(super) fn standing_jump(&mut self) {
        self.set_overflow_from(self.player.pose);
        if self.player.pose & pose::TURNING != 0 {
            self.player.facing ^= attr::FLIP_X;
        }
        self.player.anim = if self.player.pose & pose::SHOT != 0 {
            0x27
        } else {
            0x01
        };
        self.player.pose = (self.player.pose & pose::SHOT) | pose::RESUME | Motion::Jumping.byte();
    }

    /// `$8655`: B while standing: the attack, turning first if the pose says so.
    pub(super) fn standing_shot(&mut self) {
        if self.player.pose & pose::TURNING != 0 {
            self.player.pose &= !pose::TURNING;
            self.player.facing ^= attr::FLIP_X;
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
    pub(super) fn standing_walk(&mut self) {
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
    pub(super) fn standing_turn(&mut self) {
        self.player.pose_timer.tick_up();
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
        self.player.facing ^= attr::FLIP_X;
    }

    /// `$85C9`: standing idle, cycling the stand animation.
    pub(super) fn standing_idle(&mut self) {
        let pose_bits = self.player.pose & (pose::LANDING | pose::LONG_LANDING);
        if pose_bits != 0 {
            self.player.pose_timer.tick_up();
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
                self.player.pose_step.tick_up();
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
        self.player.pose_timer.tick_up();
        let x = u16::from(self.player.pose_timer >> 1);
        if x < 3 {
            self.player.anim = self.cx.prg.player.animations.standing_drift[usize::from(x)];
            return;
        }
        self.player.pose_timer.tick_down();
        self.player.anim = 0x44;
    }

    /// `$8710`: the standing attack (the pose's attack flag set).
    pub(super) fn standing_attack(&mut self) {
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
                self.player.attack_timer.tick_up();
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
        self.player.attack_timer.tick_up();
    }

    /// The shared `$B8`-gated melee block (`$8743`, `$882F`, `$8AEA`, ...).
    /// Returns the X `$9A56` left when it ran, which walking's `$8B2B` indexes `$96FD` with.
    pub(super) fn melee_block(&mut self) -> Option<u8> {
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
        self.player.charge_timer.tick_up();
        if self.player.charge_timer < 0x30 {
            return None;
        }
        let left = self.spawn_melee();
        self.cx.effects.attack |= attack::CHARGED;
        Some(left)
    }

    /// `$8795`: motion 1.
    pub(super) fn crouching(&mut self) {
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
    pub(super) fn crouching_held(&mut self) {
        self.set_overflow_from(self.player.pose);
        if self.player.pose & pose::TURNING != 0 {
            self.player.pose_timer.tick_up();
            let x = u16::from(self.player.pose_timer >> 1);
            if x < 2 {
                self.player.anim = self.cx.prg.player.animations.crouching_turn[usize::from(x)];
                return;
            }
            self.player.pose &= pose::SHOT;
            self.player.pose_timer = 0;
            self.player.facing ^= attr::FLIP_X;
        }
        if self.pad.intersects(Buttons::SIDEWAYS) {
            self.crouching_turn();
        } else {
            self.crouching_attack();
        }
    }

    /// `$8819`: crouching's attack.
    pub(super) fn crouching_attack(&mut self) {
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
        self.player.attack_timer.tick_up();
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
    pub(super) fn crouching_turn(&mut self) {
        if self.faces_press() {
            self.crouching_attack();
            return;
        }
        self.player.pose = (self.player.pose & pose::SHOT) | pose::TURNING;
        self.player.pose_timer = 0;
        self.player.anim = 0x05;
    }

    /// `$87EB`: crouching's idle.
    pub(super) fn crouching_release(&mut self) {
        if self.headroom_blocked() {
            self.crouching_held();
            return;
        }
        self.set_overflow_from(self.player.pose);
        if self.player.pose & pose::TURNING != 0 {
            self.player.facing ^= attr::FLIP_X;
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
    pub(super) fn crouching_shot(&mut self) {
        self.set_overflow_from(self.player.pose);
        if self.player.pose & pose::TURNING != 0 {
            self.player.facing ^= attr::FLIP_X;
        }
        self.reset_and_shoot();
        self.player.attack_timer = 0;
        self.player.pose = (self.player.pose & !pose::TURNING) | pose::SHOT;
        self.player.anim = 0x26;
    }

    /// `$88D3`: motion 1 + the first pad shoulder + down.
    pub(super) fn crouching_slide(&mut self) {
        self.set_overflow_from(self.player.pose);
        if self.player.pose & pose::TURNING != 0 {
            self.player.facing ^= attr::FLIP_X;
        }
        self.player.anim = 0x38;
        self.player.motion = Motion::Sliding;
        self.player.pose = 0;
        self.player.pose_timer = 0;
        self.player.pose_step = 0;
        self.cx.effects.attack &= attack::CHARGEABLE;
    }

    /// `$88FC`: motion 2.
    pub(super) fn walking(&mut self) {
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
    pub(super) fn walking_input(&mut self) {
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
    pub(super) fn walking_stop(&mut self) {
        self.set_overflow_from(self.player.pose);
        if self.player.pose & pose::TURNING != 0 {
            self.player.facing ^= attr::FLIP_X;
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
    pub(super) fn walking_jump(&mut self) {
        self.cx.effects.sound.push(0, sound::JUMP);
        if self.player.pose & pose::TURNING != 0 {
            self.player.pose &= !pose::TURNING;
            self.player.facing ^= attr::FLIP_X;
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
        self.player.vy = JUMP_VY;
        self.set_vx_from_pad();
        self.player.motion = Motion::Jumping;
        self.player.pose &= pose::SHOT;
        self.player.pose_timer = 0;
        self.player.pose_step = 0;
        self.player.anim = 0x1C;
    }

    /// `$8B32`: motion 2 + the second pad shoulder.
    pub(super) fn walking_shot(&mut self) {
        if self.player.pose & pose::TURNING != 0 {
            self.player.pose &= !pose::TURNING;
            self.player.facing ^= attr::FLIP_X;
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
    pub(super) fn walking_step(&mut self) {
        if self.faces_press() {
            self.player.pose_step.tick_up();
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
                self.player.pose_timer.tick_up();
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
        self.player.pose_timer.tick_up();
        let x = u16::from(self.player.pose_timer >> 1);
        if x < 3 {
            self.player.anim = self.cx.prg.player.animations.walking_turn[usize::from(x)];
            return;
        }
        self.player.facing ^= attr::FLIP_X;
        self.player.pose_step = 0;
        self.player.pose_timer = 0;
        self.player.pose &= pose::SHOT;
        self.player.anim = 0x0E;
    }

    /// `$8AC8`: walking's walking tail and its attack.
    pub(super) fn walking_pose(&mut self) {
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
        self.player.attack_timer.tick_up();
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
    pub(super) fn jumping(&mut self) {
        self.player.walk_gate = Buttons::SIDEWAYS;
        self.stunned();
        if self.player.pose & pose::RESUME == 0 && self.cx.world.frame_odd {
            if self.pad.contains(Buttons::A) {
                self.player.pose_step.tick_up();
                if self.player.pose_step < 0x04 {
                    self.jump_boost();
                    self.jumping_input();
                    return;
                }
            }
            self.player.pose_step.tick_up();
            if self.player.pose_step < 0x02 {
                self.jump_boost();
            } else {
                self.player.pose |= pose::RESUME;
            }
        }
        self.jumping_input();
    }

    /// `$8B99`: the held-jump y velocity trim.
    pub(super) fn jump_boost(&mut self) {
        self.player.vy = self.player.vy.wrapping_sub(0x16);
    }

    /// `$8BBC`: the jump's controls: fire, turn.
    pub(super) fn jumping_input(&mut self) {
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
            self.player.pose_timer.tick_up();
            let mut step = self.player.pose_timer;
            if self.player.pose_timer == 0x04 {
                self.player.pose &= !pose::TURNING;
                self.player.facing ^= attr::FLIP_X;
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
        self.player.attack_timer.tick_up();
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
    pub(super) fn jumping_shot(&mut self) {
        if self.player.pose & pose::TURNING != 0 {
            self.player.pose &= !pose::TURNING;
            self.player.facing ^= attr::FLIP_X;
        }
        self.reset_and_shoot();
        self.player.attack_timer = 0;
        self.player.pose |= pose::SHOT;
        self.player.anim = 0x27;
    }

    /// `$8CC6`: sliding.
    pub(super) fn sliding(&mut self) {
        self.stunned();
        self.player.walk_gate = Buttons::NONE;
        self.set_overflow_from(self.player.pose_step);
        if self.player.pose_step & slide_ledge::PROBED == 0 {
            self.slide_on_ground();
            return;
        }
        self.player.vy = 0;
        self.player.sliding = 0;
        let x = u16::from(self.player.pose_timer >> 1);
        if x < 2 {
            self.player.anim = self.cx.prg.player.animations.slide_air[usize::from(x)];
            self.player.pose_timer.tick_up();
            return;
        }
        if self.player.pose_step & slide_ledge::FLOOR_BELOW == 0 {
            self.player.motion = Motion::Falling;
            self.player.pose_timer = 0;
            self.player.pose_step = 0;
            self.player.pose = 0;
            self.player.anim = 0x1F;
            return;
        }
        self.overflow = adc_overflows(lo(self.player.y), !0x20, true);
        self.player.y = self.player.y.wrapping_sub(0x20);
        self.player.pose = pose::LANDING | pose::LONG_LANDING;
        self.player.vy = SLIDE_LEDGE_VY;
        self.player.motion = Motion::Standing;
        self.player.pose_timer = 0;
        self.player.pose_step = 0;
        self.player.anim = 0x01;
    }

    /// `$8D38`: the slide on the ground (no landing flag).
    pub(super) fn slide_on_ground(&mut self) {
        self.set_overflow_from(self.player.pose_timer);
        if self.player.pose_timer & SLIDE_GLIDING != 0 {
            self.slide_glide();
            return;
        }
        if self.player.pose_timer & SLIDE_ENDING != 0 {
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
        self.player.pose_timer.tick_up();
    }

    /// `$8D6D`: the slide's launch: a push and the speed in the facing direction.
    pub(super) fn slide_launch(&mut self) {
        self.cx.effects.sound.push(0, sound::JUMP);
        self.player.sliding = sliding::ON;
        let x = self.player.x.cast_signed();
        self.overflow = if self.player.faces_left() {
            x.overflowing_sub(0x80).1
        } else {
            x.overflowing_add(0x80).1
        };
        if self.player.faces_left() {
            self.player.x = self.player.x.wrapping_sub(0x80);
            self.player.vx = SLIDE_VX_LEFT;
        } else {
            self.player.x = self.player.x.wrapping_add(0x80);
            self.player.vx = SLIDE_VX_RIGHT;
        }
        self.player.anim = 0x3A;
        self.player.pose_timer = SLIDE_GLIDING;
    }

    /// `$8DBB`: the slide gliding until the screen's edge or its speed runs out.
    pub(super) fn slide_glide(&mut self) {
        self.player.anim = 0x3A;
        let cam = self.cx.world.camera_x;
        self.overflow = sub_overflows16(self.player.x, cam);
        let dx_hi = hi(self.player.x.wrapping_sub(cam));
        let vx_lo = self.player.vx.to_be_bytes()[1];
        let end = if self.player.vx < 0 {
            dx_hi < 0x02 || vx_lo >= 0xF0
        } else {
            dx_hi >= 0x0D || vx_lo < 0x10
        };
        if end {
            self.player.pose_timer = SLIDE_ENDING;
            self.player.anim = 0x3B;
        }
    }

    /// `$8DFE`: something solid 24 px ahead, which ends the slide in a crouch.
    pub(super) fn slide_blocked(&mut self) -> bool {
        let py = hi(self.player.y.wrapping_sub(0x80));
        let dx = self.screen_x(self.player.x);
        self.overflow = sub_overflows16(self.player.x, self.cx.world.camera_x);
        let solid = if self.player.faces_left() {
            dx < 0x1A || self.solid_x(self.player.x.wrapping_sub(0x180), py)
        } else {
            dx >= 0xD0 || self.solid_x(self.player.x.wrapping_add(0x180), py)
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
    pub(super) fn slide_end(&mut self) {
        self.player.sliding = 0;
        self.player.anim = 0x3B;
        self.player.motion = Motion::Crouching;
        self.player.pose = 0;
        self.player.pose_timer = 0;
        self.player.pose_step = 0;
    }

    /// `$8E8C`: down released mid-slide: the jump.
    pub(super) fn slide_jump(&mut self) {
        if self.headroom_blocked() {
            self.slide_end();
            return;
        }
        self.player.vy = JUMP_VY;
        self.player.pose_timer = 0;
        self.player.pose_step = 0;
        self.player.pose = 0;
        self.player.anim = 0x1C;
        self.player.motion = Motion::Jumping;
    }

    /// `$8EB1`: motion 5.
    pub(super) fn falling(&mut self) {
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
    pub(super) fn aiming_up(&mut self) {
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
    pub(super) fn aiming_up_jump(&mut self) {
        if self.jump_blocked() {
            self.player.anim = 0x46;
            return;
        }
        self.player.vy = JUMP_VY;
        self.set_vx_from_pad();
        self.player.motion = Motion::Jumping;
        self.player.pose &= pose::SHOT;
        self.player.pose_timer = 0;
        self.player.pose_step = 0;
        self.player.anim = 0x1C;
    }

    /// `$8FFA`: motion 6 + the second pad shoulder.
    pub(super) fn aiming_up_shot(&mut self) {
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
    pub(super) fn aiming_up_turn(&mut self) {
        if !self.faces_press() {
            self.player.facing ^= attr::FLIP_X;
        }
        self.aiming_up_attack();
    }

    /// `$8F60`: aiming up's attack.
    pub(super) fn aiming_up_attack(&mut self) {
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
        self.player.attack_timer.tick_up();
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
    pub(super) fn dying(&mut self) {
        self.cx.effects.busy = busy::SCRIPTED;
        let timer = self.player.death_timer;
        if timer >= 0x30 {
            if timer < 0x48 {
                let frame = (timer - 0x30) & 0x1E;
                self.work.pointer = self.rw(0x9191 + u16::from(frame));
                self.work.attr = self.cx.prg.player.death_attributes[usize::from(frame >> 1)];
                self.overlay(5);
            }
            self.player.death_timer.tick_up();
        } else {
            if timer == 0x2F {
                self.cx.effects.sound.push(1, sound::PLAYER_DIES);
                self.player.stage_script = stage_script::FROZEN | stage_script::HIDDEN;
                self.player.death_flags |= death::BURST;
            }
            if timer.is_multiple_of(16) {
                self.cx.effects.sound.push(1, sound::DEATH_TICK);
            }
            self.set_overflow_from(self.player.death_flags);
            if self.player.death_flags & death::NO_OVERLAY == 0 {
                self.work.pointer = self.rw(0x9A40 + u16::from(timer & 0x06));
                self.work.attr = self.player.facing & attr::PALETTE;
                self.overlay(4);
            }
            self.player.death_timer.tick_up();
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
                self.player.facing ^= attr::PALETTE;
            }
        }
        if self.player.death_timer < 0xC0 {
            return;
        }
        self.set_overflow_from(self.cx.effects.hud);
        let effects = &mut *self.cx.effects;
        effects.transition = super::super::world::Transition::LifeLost;
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
    pub(super) fn overlay(&mut self, chr_slot: u8) {
        self.work.anchor = 0;
        self.work.tile = 0;
        self.work.chr_slot = chr_slot;
        self.work.screen_x = self.player.screen_x;
        self.work.screen_y = self.player.screen_y;
        self.set_overflow_from(self.player.death_flags);
        if self.player.death_flags & death::NO_OVERLAY == 0 {
            self.overflow = engine::draw(self.work, 1, self.cx);
        }
    }

    /// `$D03C` (bank 14, through `$91D8`): motion 9, the boss room's entry: Batman
    /// lowered in on his line (`$D17A`, `$D1B0`), the tally counted up to `$B2:$B3`
    /// beside the boss's hit points, the pose held, then motion 0.
    pub(super) fn boss_entry(&mut self) {
        self.cx.effects.busy = busy::SCRIPTED;
        let player = &mut *self.player;
        match EntryStep::from_byte(player.entry_step) {
            Some(EntryStep::Arriving) => {
                player.entry_timer.tick_up();
                if player.entry_timer >= 4 {
                    self.cx.effects.sound.push(0, sound::TALLY_TICK);
                    (player.entry_timer, player.tally_digit) = (0, 0);
                    player.entry_step = EntryStep::Tallying.byte();
                    return;
                }
                self.work.chr_slot = 5;
                self.entry_pose(1);
                self.entry_line(0);
            }
            Some(EntryStep::Tallying) => {
                player.entry_timer.tick_up();
                let pose = if player.entry_timer & 0x02 != 0 { 1 } else { 2 };
                self.work.chr_slot = 5;
                self.entry_pose(pose);
                self.entry_line(pose);
                let (tally, digit) = (self.player.tally, self.player.tally_digit);
                self.queue_tally(tally, Some(digit));
                if self.cx.world.frame.is_multiple_of(16) {
                    self.cx.effects.sound.push(0, sound::TALLY_TICK);
                }
                let player = &mut *self.player;
                player.tally_digit += 1;
                if player.tally_digit >= 0x0A {
                    player.tally_digit = 0;
                }
                player.tally = player.tally.wrapping_add(0x64);
                let colour = if player.tally >= u16::from_le_bytes(self.cx.effects.boss_pool) {
                    player.entry_step = EntryStep::Tallied.byte();
                    player.entry_timer = 0;
                    palette::BLACK
                } else if self.cx.world.frame.is_multiple_of(8) {
                    palette::WHITE
                } else {
                    palette::BLACK
                };
                set_first_colours(&mut self.cx.effects.palette, colour);
                let effects = &mut *self.cx.effects;
                effects.vram.push_palette(&effects.palette);
                // `$C34B` -> `$C0C6`: its `ADC #$00` on the return address's high byte ($C3).
                self.overflow = false;
            }
            Some(EntryStep::Tallied) => {
                if player.entry_timer == 0 {
                    let target = u16::from_le_bytes(self.cx.effects.boss_pool);
                    self.queue_tally(target, None);
                }
                self.work.chr_slot = 5;
                self.entry_pose(1);
                self.entry_line(3);
                let player = &mut *self.player;
                player.entry_timer.tick_up();
                if player.entry_timer >= 4 {
                    self.cx.effects.sound.push(0, sound::EXPLOSION);
                    player.entry_step = EntryStep::Posing.byte();
                    player.entry_timer = 0;
                }
            }
            Some(EntryStep::Posing) | None => {
                let held = usize::from(player.entry_timer >> 2);
                self.work.chr_slot = 5;
                self.entry_pose(self.cx.prg.player.entry_poses[held]);
                self.work.chr_slot = 4;
                let player = &mut *self.player;
                player.entry_timer.tick_up();
                if player.entry_timer < 0x0C {
                    self.entry_pose(self.cx.prg.player.entry_pose_second[held]);
                    return;
                }
                self.cx.effects.song = sound::song::BOSS_FIGHT;
                self.cx.oam.chr[4] = 0xA2;
                player.motion = Motion::Standing;
                player.stage_script = 0;
                self.cx.effects.busy = 0;
            }
        }
    }

    /// `$D17A`: Batman's entry pose `pose` (`$D1A4`), at (`$3E`, `$B0`).
    pub(super) fn entry_pose(&mut self, pose: u8) {
        self.work.attr = 0;
        self.work.anchor = 0;
        self.work.tile = 0;
        self.work.screen_x = 0x3E;
        self.work.screen_y = 0xB0;
        self.work.pointer = rom_word(
            self.cx.prg,
            ENTRY_BANK,
            ENTRY_POSE_POINTERS + 2 * u16::from(pose),
        );
        self.overflow = engine::draw(self.work, 1, self.cx);
    }

    /// `$D1B0`: the line above him, frame `line` (`$D1D8`), at (`$3E`, `$78`).
    pub(super) fn entry_line(&mut self, line: u8) {
        self.work.pointer = rom_word(
            self.cx.prg,
            ENTRY_BANK,
            ENTRY_LINE_POINTERS + 2 * u16::from(line),
        );
        self.work.attr |= attr::PALETTE;
        self.work.chr_slot = 4;
        self.work.screen_x = 0x3E;
        self.work.screen_y = 0x78;
        self.overflow = engine::draw(self.work, 1, self.cx);
    }

    /// `$D1E0`/`$D1F5` and `$D200`: queues `tally` at `$2348` (its last two digits
    /// `last` when given) and the boss's hit points at `$2352`, each five decimal
    /// digits and a 0, two tile rows (`$C199`, `$D24A`).
    pub(super) fn queue_tally(&mut self, tally: u16, last: Option<u8>) {
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
        vram.push(ppu::tile_addr(0, 26, 8), 0, &top);
        vram.push(ppu::tile_addr(0, 27, 8), 0, &bottom);
        let (top, bottom) = rows(self.cx.effects.boss_hp, None);
        let vram = &mut self.cx.effects.vram;
        vram.push(ppu::tile_addr(0, 26, 18), 0, &top);
        vram.push(ppu::tile_addr(0, 27, 18), 0, &bottom);
    }

    /// `$91B5`: motion 8, held while the power-up's pieces close in: hidden on odd frames.
    pub(super) fn powering_up(&mut self) {
        self.overflow = false;
        if self.cx.world.frame_odd {
            self.player.stage_script = stage_script::FROZEN | stage_script::HIDDEN;
        } else {
            self.player.stage_script = stage_script::FROZEN;
            let x = u16::from(self.cx.world.frame & 0x02 != 0);
            self.player.facing = (self.player.facing & !attr::PALETTE)
                | self.cx.prg.player.power_up_facings[usize::from(x)];
        }
    }

    /// `$91DB`: motion 11.
    pub(super) fn flying(&mut self) {
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
        self.player.attack_timer.tick_up();
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
