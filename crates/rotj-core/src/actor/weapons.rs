//! Batman's projectiles: the bank-5 routine the gameplay loop calls at `$D444`.
//!
//! The routine dispatches on `$0476` through the table at `$801B`. [`step`] ports
//! handlers `$8029` (`$0476` 0), `$8170` (1), `$8891` (2), `$8D39` (3), `$91B2` (4), `$9647` (5)
//! and `$9D2E` (6, the power-up's pieces), the
//! bank-5 subroutines they call (`$9E13`, `$9E46`, `$9ED6`) and the draw (`$C247`,
//! [`engine::draw`]); `docs/ACTORS.md` §9g.

use super::collision::{self, Probe};
use super::engine::{self, Work, rom_byte, rom_word};
use super::scroll::Scroll;
use super::slots;
use super::world::{Cx, Weapon};

/// One call of the bank-5 weapons routine (`$D444` -> `$D447`).
pub fn step(work: &mut Work, cx: &mut Cx<'_>) {
    // The handlers below take the scroll directly; lend it out of the context.
    let mut scroll = std::mem::take(cx.scroll);
    match cx.effects.weapon {
        handler @ 0..=5 => run(work, cx, &mut scroll, handler),
        6 => pieces(work, cx),
        other => panic!("bank-5 weapon handler for $0476={other:02X} is past the $801B table"),
    }
    *cx.scroll = scroll;
}

/// Handler 6 (`$9D2E`), the power-up's four pieces: on even frames each flies (its
/// velocity stepped by `$9E00-$9E0F` for its corner `$7F40`) and is drawn kept on
/// the screen; after 8 steps it is gone and Batman is powered up: `$0472` and `$B8`
/// set, weapon 5, his motion back from `$040F`, `$C2` from `$0409`, and for the
/// pieces with `$7F60` bit 7 the sprite palette's second set from `$9E10`.
fn pieces(work: &mut Work, cx: &mut Cx<'_>) {
    if cx.world.frame_odd {
        return;
    }
    work.anchor = 0;
    work.tile = 0;
    work.attr = 3;
    work.chr_slot = 5;
    for slot in (0..16).rev() {
        if cx.effects.weapons[slot].kind == 0 {
            continue;
        }
        integrate(slot, work, cx);
        if project_kept(slot, work, cx) {
            let frame = rom_byte(cx.prg, 5, 0x9DF0 + u16::from(cx.effects.weapons[slot].part));
            work.pointer = rom_word(cx.prg, 5, 0x9DF8 + u16::from(frame));
            draw(work, cx);
        }
        let prg = cx.prg;
        let effects = &mut *cx.effects;
        let piece = &mut effects.weapons[slot];
        let corner = u16::from(piece.param);
        let step = |hi: u16, lo: u16| {
            u16::from_be_bytes([rom_byte(prg, 5, hi + corner), rom_byte(prg, 5, lo + corner)])
        };
        piece.vx = piece.vx.wrapping_add_unsigned(step(0x9E04, 0x9E00));
        piece.vy = piece.vy.wrapping_add_unsigned(step(0x9E0C, 0x9E08));
        piece.part = piece.part.wrapping_add(1);
        if piece.part < 8 {
            continue;
        }
        piece.kind = 0;
        let lit = piece.part_timer & 0x80 != 0;
        let player = &mut effects.player;
        player.flags[0] = 0x80;
        effects.attack = 0x80;
        effects.weapon = 5;
        player.facing &= 0xFC;
        player.stage_script = 0;
        player.flags[2] = 0;
        player.flash_timer = 0;
        effects.busy = 0;
        effects.scroll_lock = effects.stage_end.timer;
        player.motion = effects.stage_args[1];
        if lit {
            for i in 0..3u8 {
                effects.palette[17 + usize::from(i)] = rom_byte(prg, 5, 0x9E10 + u16::from(i));
            }
            effects.palette_dirty = 0x80;
        }
    }
}

/// The shared slot scan (`$8037`): `$9A`/`$A4`/`$98` reset, then slots 15 down to 0.
fn run(work: &mut Work, cx: &mut Cx<'_>, scroll: &mut Scroll, handler: u8) {
    work.anchor = 0;
    work.tile = 0;
    work.chr_slot = 4;
    if handler == 4 {
        work.attr = 0;
    }
    for slot in (0..16).rev() {
        if cx.effects.weapons[slot].kind != 0 {
            match handler {
                0 => zero(slot, work, cx),
                1 => one(slot, work, cx),
                2 => two(slot, work, cx),
                4 => four(slot, work, cx),
                5 => {
                    if !five(slot, work, cx, scroll) {
                        return;
                    }
                }
                _ => {
                    if !three(slot, work, cx) {
                        return;
                    }
                }
            }
        }
    }
}

/// Gravity's `$7EB0`/`$7D20` add: `$7EB0 += 6`, carrying into the high byte; returns
/// the overflow flag the `ADC #$06` leaves.
fn gravity(weapon: &mut Weapon) -> bool {
    let [hi, lo] = weapon.vy.to_be_bytes();
    let (lo_after, carry) = lo.overflowing_add(6);
    let hi = if carry { hi.wrapping_add(1) } else { hi };
    weapon.vy = i16::from_be_bytes([hi, lo_after]);
    engine::adc_overflows(lo, 6, false)
}

/// Integrates the projectile's level position by its velocity (`$9E13`).
fn integrate(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    let weapon = &mut cx.effects.weapons[slot];
    let (x_lo, carry) = weapon
        .level_x_lo
        .overflowing_add(weapon.vx.to_be_bytes()[1]);
    let x_hi = weapon
        .level_x_hi
        .wrapping_add(weapon.vx.to_be_bytes()[0])
        .wrapping_add(u8::from(carry));
    weapon.level_x_lo = x_lo;
    weapon.level_x_hi = x_hi;
    let (y_lo, carry) = weapon
        .level_y_lo
        .overflowing_add(weapon.vy.to_be_bytes()[1]);
    let y_hi = weapon
        .level_y_hi
        .wrapping_add(weapon.vy.to_be_bytes()[0])
        .wrapping_add(u8::from(carry));
    weapon.level_y_lo = y_lo;
    weapon.level_y_hi = y_hi;
    work.x = u16::from_be_bytes([x_hi, x_lo]);
    work.y = u16::from_be_bytes([y_hi, y_lo]);
}

/// Projects the working position onto the screen, freeing the slot when off it (`$9E46`, `A = 0`).
fn project(slot: usize, work: &mut Work, cx: &mut Cx<'_>) -> bool {
    on_screen(slot, work, cx, false)
}

/// `$9E46` with `A = $80`: off the screen keeps the slot, and a row just below it
/// sets the bottom anchor `$9A`.
fn project_kept(slot: usize, work: &mut Work, cx: &mut Cx<'_>) -> bool {
    on_screen(slot, work, cx, true)
}

/// `$9E46`: on the screen writes `$94/$95` and `$7B80/$7B90`.
fn on_screen(slot: usize, work: &mut Work, cx: &mut Cx<'_>, keep: bool) -> bool {
    let (camera_x, camera_y) = (cx.world.camera_x, cx.world.camera_y);
    // Off the screen the last `BIT $30` (`A` is `$00` or `$80`) clears the overflow flag.
    work.cpu_overflow = false;
    let dx = work.x.wrapping_sub(camera_x);
    if work.x < camera_x || dx.to_be_bytes()[0] >= 0x10 {
        if !keep {
            cx.effects.weapons[slot].kind = 0;
        }
        return false;
    }
    work.screen_x = (dx >> 4).to_le_bytes()[0];
    let dy = work.y.wrapping_sub(camera_y);
    if work.y < camera_y || dy.to_be_bytes()[0] >= 0x0F {
        if !keep {
            cx.effects.weapons[slot].kind = 0;
        } else if dy.to_be_bytes()[0] < 0x15 {
            work.anchor = 0x40;
        }
        return false;
    }
    work.screen_y = (dy >> 4).to_le_bytes()[0];
    work.cpu_overflow = engine::sub_overflows16(work.y, camera_y);
    let weapon = &mut cx.effects.weapons[slot];
    weapon.screen_y = work.screen_y;
    weapon.screen_x = work.screen_x;
    true
}

/// Draws [`Work::pointer`] from the window its address falls in (`$C247`); a stale
/// timer can point past the sprite tables, into the loop's own bank 14 or bank 15.
fn draw(work: &mut Work, cx: &mut Cx<'_>) {
    let bank = match work.pointer {
        0x8000..=0x9FFF => 5,
        0xA000..=0xBFFF => 1,
        0xC000..=0xDFFF => 14,
        0xE000..=0xFFFF => 15,
        other => panic!("weapon metasprite ${other:04X} is below the PRG windows"),
    };
    engine::draw(work, bank, cx);
}

/// Is the metatile at the working position solid (`$C93B`)?
fn solid(work: &Work, flags: u8, cx: &Cx<'_>) -> bool {
    let mut probe = Probe {
        x: work.x,
        y: work.y,
        ..Probe::default()
    };
    collision::solid(&mut probe, flags, cx) & 0x80 != 0
}

/// Draws only on every other frame (`LDA $10; EOR $F1; LSR; BCS`).
fn draw_parity(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    if (u8::try_from(slot).expect("slot < 16") ^ cx.world.frame) & 1 == 0 {
        draw(work, cx);
    }
}

/// Handler `$8029` (`$0476` 0): the fired projectile.
fn zero(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    level_shot(slot, work, cx, by_timer(slot, [0x814C, 0x814A]), 0x814E);
}

/// Handler `$8170` (`$0476` 1): the homing shot; types `$80` and up are its
/// charged attack (`$847D`).
fn one(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    let kind = cx.effects.weapons[slot].kind;
    if kind < 0x80 {
        level_shot(slot, work, cx, |cx| home(slot, cx), 0x8477);
        return;
    }
    if cx.world.frame.trailing_zeros() >= 5 {
        cx.effects.sound[0] = 0x2E;
    }
    match kind {
        0x80 => gather(slot, work, cx),
        0x81 => throw(slot, work, cx),
        0x82 => {}
        other => panic!("handler 1's type ${other:02X} is past the $876E table"),
    }
}

/// `$8288`: picks the homing shot's sprite and `$7C20` by its heading, then turns
/// it 2 a frame toward its target slot `$7F40` (a new one from `$83FC` when that
/// one is gone).
fn home(slot: usize, cx: &mut Cx<'_>) -> u16 {
    let weapon = cx.effects.weapons[slot];
    let lo = weapon.vy.to_be_bytes()[1];
    let base = match (weapon.vx < 0, weapon.vy < 0) {
        (false, true) => 0,
        (false, false) => 5,
        (true, true) => 10,
        (true, false) => 15,
    };
    let heading = if weapon.vx == 0 {
        base + 4
    } else if weapon.vy < 0 {
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
    cx.effects.weapons[slot].attr = rom_byte(cx.prg, 5, 0x8437 + heading);
    let pointer = rom_word(cx.prg, 5, 0x844B + heading * 2);
    let actor = |cx: &Cx<'_>, target: u8, array: u16| {
        slots::read(cx.slots, &cx.effects.weapons, array + u16::from(target))
    };
    let mut target = weapon.param;
    if target & 0x80 != 0 || actor(cx, target, 0x7CC0) == 0 {
        target = retarget(slot, cx);
    }
    if target & 0x80 == 0 && actor(cx, target, 0x7D50) & 0x40 != 0 {
        target = retarget(slot, cx);
    }
    if target & 0x80 != 0 {
        return pointer;
    }
    #[cfg(test)]
    super::oracle::seen(0x831E);
    let position = |cx: &Cx<'_>, hi: u16, lo: u16| {
        u16::from_be_bytes([actor(cx, target, hi), actor(cx, target, lo)])
    };
    let weapon = &cx.effects.weapons[slot];
    let dy = position(cx, 0x7B40, 0x7B60)
        .wrapping_sub(u16::from_be_bytes([weapon.level_y_hi, weapon.level_y_lo]));
    let dx = position(cx, 0x7B00, 0x7B20)
        .wrapping_sub(u16::from_be_bytes([weapon.level_x_hi, weapon.level_x_lo]));
    let (left, up) = (dx & 0x8000 != 0, dy & 0x8000 != 0);
    let weapon = &mut cx.effects.weapons[slot];
    weapon.vx = turn(weapon.vx, left);
    if left && up {
        loop {
            weapon.vy = weapon.vy.wrapping_sub(2);
            let [hi, lo] = weapon.vy.to_be_bytes();
            if weapon.vy >= 0 {
                break;
            }
            if lo < 0xE0 {
                weapon.vy = i16::from_be_bytes([hi, 0xE0]);
                break;
            }
        }
    } else {
        weapon.vy = turn(weapon.vy, up);
    }
    pointer
}

/// One axis of `$8288`'s turn: 2 down (`down`) or up, the low byte held to
/// `$E0` going negative and `$20` going positive.
fn turn(velocity: i16, down: bool) -> i16 {
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
fn retarget(slot: usize, cx: &mut Cx<'_>) -> u8 {
    let hittable = |&target: &usize| {
        let actor = &cx.slots.main[target];
        actor.kind != 0 && actor.body.contact & 0x20 == 0 && actor.body.state & 0x40 == 0
    };
    let found = if slot & 1 == 0 {
        (0..16).find(hittable)
    } else {
        (0..16).rev().find(hittable)
    };
    let target = found.map_or(0xFF, |slot| u8::try_from(slot).expect("slot < 16"));
    cx.effects.weapons[slot].param = target;
    target
}

/// Type `$80` (`$8496`): the charge gathers in front of the player while B is
/// held, three type `$82` parts circling it once its seven frames have run
/// (`$7D00` counting them); letting go before that clears the charge, and after it,
/// or A or a direction pressed, throws it as type `$81`.
fn gather(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    let held = cx.effects.attack & 0x40 != 0;
    if cx.effects.weapons[slot].param & 0x80 == 0 {
        if !held {
            #[cfg(test)]
            super::oracle::seen(0x8712);
            clear_charge(cx);
            return;
        }
    } else if cx.world.pad & 0x8F != 0 || !held {
        launch(slot, work, cx);
        throw(slot, work, cx);
        return;
    }
    #[cfg(test)]
    super::oracle::seen(0x84B6);
    in_front(slot, work, cx);
    let step = cx.effects.weapons[slot].vx.to_be_bytes()[0];
    work.pointer = rom_word(cx.prg, 5, 0x8774 + u16::from(step) * 2);
    if !cx.world.frame_odd {
        let step = step.wrapping_add(1);
        let weapon = &mut cx.effects.weapons[slot];
        weapon.vx = i16::from_be_bytes([step, weapon.vx.to_be_bytes()[1]]);
        if step >= 7 {
            let mut reset = slot;
            if cx.effects.weapons[slot].param & 0x80 == 0 {
                cx.effects.weapons[slot].param = 0x80;
                if !spawn_circle(slot, cx) {
                    reset = 0xFF;
                }
            }
            let at = 0x7D00 + u16::try_from(reset).expect("a byte");
            slots::write(cx.slots, &mut cx.effects.weapons, at, 0);
        }
    }
    work.y = work.y.wrapping_add(0x0380);
    if project_kept(slot, work, cx) || work.anchor & 0x40 != 0 {
        work.attr = 0;
        draw(work, cx);
        if cx.effects.weapons[slot].param != 0 {
            circle(slot, work, cx);
        }
    }
}

/// `$8712`: frees every slot of type `$80` and up.
fn clear_charge(cx: &mut Cx<'_>) {
    for weapon in &mut cx.effects.weapons {
        if weapon.kind & 0x80 != 0 {
            weapon.kind = 0;
        }
    }
}

/// `$85CF`: three type `$82` parts, in free slots first, then over the plain shots;
/// their slots kept in the charge's `$7F50` (two nibbles) and `$7F60`. False when
/// even that finds too few and every slot is freed (`$8617`, which leaves X = `$FF`).
fn spawn_circle(source: usize, cx: &mut Cx<'_>) -> bool {
    #[cfg(test)]
    super::oracle::seen(0x85CF);
    let mut parts = [0u8; 3];
    let mut count = 0;
    let mut at = 15i8;
    let mut over_shots = false;
    loop {
        if at < 0 {
            if over_shots {
                for weapon in &mut cx.effects.weapons {
                    weapon.kind = 0;
                }
                return false;
            }
            over_shots = true;
            at = 15;
            continue;
        }
        let index = usize::try_from(at).expect("non-negative");
        let kind = cx.effects.weapons[index].kind;
        if (over_shots && kind < 0x80) || kind == 0 {
            let angle = rom_byte(cx.prg, 5, 0x887C + u16::try_from(count).expect("< 3"));
            let weapon = &mut cx.effects.weapons[index];
            weapon.kind = 0x82;
            weapon.damage = 1;
            weapon.flags = 0xE0;
            weapon.state = 0;
            weapon.param = angle;
            parts[count] = at.cast_unsigned();
            count += 1;
            if count == 3 {
                let weapon = &mut cx.effects.weapons[source];
                weapon.part = parts[0] << 4 | parts[1];
                weapon.part_timer = parts[2];
                return true;
            }
            over_shots = false;
        }
        at -= 1;
    }
}

/// `$8525`: draws the three parts circling the charge at `slot`, each a step
/// further round its 32 angles (`$87FC`).
fn circle(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::oracle::seen(0x8525);
    work.pointer = rom_word(cx.prg, 5, 0x887F);
    work.attr = 0;
    let centre = cx.effects.weapons[slot];
    for part in [
        centre.part >> 4,
        centre.part & 0x0F,
        centre.part_timer & 0x0F,
    ] {
        let part = usize::from(part);
        cx.effects.weapons[part].flags = 0x60;
        let at = u16::from(cx.effects.weapons[part].param << 2);
        work.x = u16::from_be_bytes([centre.level_x_hi, centre.level_x_lo]).wrapping_add(word(
            cx,
            0x87FC + at,
            0x87FD + at,
        ));
        work.y = u16::from_be_bytes([centre.level_y_hi, centre.level_y_lo]).wrapping_add(word(
            cx,
            0x87FE + at,
            0x87FF + at,
        ));
        let weapon = &mut cx.effects.weapons[part];
        weapon.param = weapon.param.wrapping_add(1);
        if weapon.param >= 0x20 {
            weapon.param = 0;
        }
        if project_kept(slot, work, cx) || work.anchor & 0x40 != 0 {
            draw(work, cx);
            let weapon = &mut cx.effects.weapons[part];
            weapon.screen_x = work.screen_x;
            weapon.screen_y = work.screen_y;
        } else {
            cx.effects.weapons[part].flags = 0xE0;
        }
    }
}

/// `$864A`: the charge becomes type `$81`, thrown by the pad's direction (none:
/// down when aiming up, else the way he faces) from in front of the player.
fn launch(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::oracle::seen(0x864A);
    cx.effects.attack &= 0x80;
    let pad = cx.world.pad;
    let direction = if pad.trailing_zeros() >= 4 {
        if cx.effects.player.motion == 6 {
            3
        } else {
            u16::from(faces_left(cx))
        }
    } else if pad.trailing_zeros() >= 2 {
        #[cfg(test)]
        super::oracle::seen(0x8668);
        if pad & 0x08 != 0 { 2 } else { 3 }
    } else {
        u16::from(pad & 0x01 == 0)
    };
    let vx = word(cx, 0x8885 + direction, 0x8881 + direction);
    let vy = word(cx, 0x888D + direction, 0x8889 + direction);
    let weapon = &mut cx.effects.weapons[slot];
    weapon.vx = vx.cast_signed();
    weapon.vy = vy.cast_signed();
    weapon.kind = 0x81;
    in_front(slot, work, cx);
}

/// Type `$81` (`$8515`): the thrown charge flies, its parts circling; off the
/// screen it is freed with every other charged slot.
fn throw(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::oracle::seen(0x8515);
    integrate(slot, work, cx);
    if project(slot, work, cx) {
        circle(slot, work, cx);
    } else {
        clear_charge(cx);
    }
}

/// `$86A5`: puts the charge in front of the player (`$9EA7` when aiming up, else
/// the `$8726`-`$8762` offsets by motion), in its slot and the working position.
fn in_front(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    let motion = u16::from(cx.effects.player.motion);
    let (x, y) = if cx.effects.player.motion == 6 {
        above(cx)
    } else {
        let dx = if faces_left(cx) {
            word(cx, 0x874A + motion, 0x873E + motion)
        } else {
            word(cx, 0x8732 + motion, 0x8726 + motion)
        };
        beside_player(cx, dx, word(cx, 0x8762 + motion, 0x8756 + motion))
    };
    let weapon = &mut cx.effects.weapons[slot];
    [weapon.level_x_hi, weapon.level_x_lo] = x.to_be_bytes();
    [weapon.level_y_hi, weapon.level_y_lo] = y.to_be_bytes();
    (work.x, work.y) = (x, y);
}

/// `$9EA7`'s spot: 2 px ahead of the player and `$410` up.
fn above(cx: &Cx<'_>) -> (u16, u16) {
    #[cfg(test)]
    super::oracle::seen(0x9EA7);
    beside_player(cx, if faces_left(cx) { 0xFFE0 } else { 0x0020 }, 0xFBF0)
}

/// The in-flight sprite of handlers 0 and 5: the pointer at `sprites[$7CB0 >> 7]`.
fn by_timer(slot: usize, sprites: [u16; 2]) -> impl FnOnce(&mut Cx<'_>) -> u16 {
    move |cx| {
        let timer = cx.effects.weapons[slot].timer;
        rom_word(cx.prg, 5, sprites[usize::from(timer >> 7)])
    }
}

/// Handler `$9647` (`$0476` 5, the power-up's weapon): handler 0's flight and wall
/// bounce with its own sprites (`$9771`, `$9779`, the spent `$977B`); types `$80`
/// and up are its super attack. False when the handler returns before the other slots.
fn five(slot: usize, work: &mut Work, cx: &mut Cx<'_>, scroll: &mut Scroll) -> bool {
    if cx.effects.weapons[slot].kind >= 0x80 {
        return unleashed(slot, work, cx, scroll);
    }
    #[cfg(test)]
    super::oracle::seen(0x9668);
    level_shot(slot, work, cx, by_timer(slot, [0x9771, 0x9779]), 0x977B);
    true
}

/// Handler 5's types `$80` and up (`$9781`, dispatch `$9D24`), the power-up's
/// super attack; false when the handler returns before the other slots.
fn unleashed(slot: usize, work: &mut Work, cx: &mut Cx<'_>, scroll: &mut Scroll) -> bool {
    match cx.effects.weapons[slot].kind {
        0x80 => unleash(slot, work, cx, scroll),
        0x81 => orbiter(slot, work, cx, scroll),
        0x82 => {
            pairs(slot, cx);
            true
        }
        0x83 | 0x84 => true,
        other => panic!("handler 5's type ${other:02X} is past the $9D24 table"),
    }
}

/// Type `$80` (`$9790`): with the meter at 2 or more, the charge claims every
/// other slot (type `$83`), stops the scroll and the player (`$0481` bit 7) and
/// becomes the type `$82` that releases the orbiting pairs; with less it is
/// dropped and the handler returns. It waits while the overflow flag is set:
/// the game tests `LDA $B8; BVC`, and `LDA` leaves V as the code before left it
/// (`docs/ACTORS.md` §9g).
fn unleash(slot: usize, work: &Work, cx: &mut Cx<'_>, scroll: &mut Scroll) -> bool {
    #[cfg(test)]
    super::oracle::overflow_at(0x9790, work.cpu_overflow);
    if work.cpu_overflow {
        return true;
    }
    if cx.effects.meter < 2 {
        #[cfg(test)]
        super::oracle::seen(0x979E);
        cx.effects.attack &= 0x80;
        cx.effects.weapons[slot].kind = 0;
        return false;
    }
    #[cfg(test)]
    super::oracle::seen(0x97AC);
    cx.effects.busy = 0x80;
    cx.effects.sound[0] = 0x47;
    cx.effects.player.stage_script |= 0x80;
    scroll.gate = 0;
    scroll.speed_x = 0;
    scroll.speed_y = 0;
    let kept = cx.effects.scroll_lock;
    cx.effects.scroll_lock = 0xFF;
    let weapon = &mut cx.effects.weapons[slot];
    weapon.kind = 0x82;
    weapon.part = 0;
    weapon.part_timer = 0;
    weapon.timer = kept;
    for weapon in &mut cx.effects.weapons {
        if weapon.kind & 0x80 == 0 {
            weapon.kind = 0x83;
            weapon.flags = 0xE0;
        }
    }
    let (x, y) = if cx.effects.player.motion == 6 {
        #[cfg(test)]
        super::oracle::seen(0x97F9);
        above(cx)
    } else {
        let motion = u16::from(cx.effects.player.motion);
        beside_player(
            cx,
            ahead(cx, motion),
            word(cx, 0x9F43 + motion, 0x9F37 + motion),
        )
    };
    let weapon = &mut cx.effects.weapons[slot];
    [weapon.level_x_hi, weapon.level_x_lo] = x.to_be_bytes();
    [weapon.level_y_hi, weapon.level_y_lo] = y.to_be_bytes();
    true
}

/// Type `$82` (`$9B4A`): every 17th frame, two claimed slots become a type `$81`
/// orbiter and the type `$84` centre it circles, at the charge's position; after
/// five pairs the charge is claimed itself.
fn pairs(slot: usize, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::oracle::seen(0x9B4A);
    let weapon = &mut cx.effects.weapons[slot];
    if weapon.part != 0x10 {
        weapon.part += 1;
        return;
    }
    weapon.part = 0;
    let claimed = |cx: &Cx<'_>, below: usize| {
        (0..below)
            .rev()
            .find(|&i| cx.effects.weapons[i].kind == 0x83)
            .expect("the charge claimed every other slot, enough for five pairs")
    };
    let orbiter = claimed(cx, 16);
    cx.effects.weapons[orbiter].kind = 0x81;
    let centre = claimed(cx, orbiter);
    cx.effects.weapons[centre].kind = 0x84;
    let source = cx.effects.weapons[slot];
    let orbiter_weapon = &mut cx.effects.weapons[orbiter];
    [
        orbiter_weapon.level_x_hi,
        orbiter_weapon.level_x_lo,
        orbiter_weapon.level_y_hi,
        orbiter_weapon.level_y_lo,
    ] = [1, 0, 0, 0];
    orbiter_weapon.flags = 0xE0;
    orbiter_weapon.param = 0;
    orbiter_weapon.part = if source.part_timer == 0 { 0x80 } else { 0 };
    orbiter_weapon.vx = -8;
    orbiter_weapon.vy = 0x0080;
    orbiter_weapon.timer = source.timer;
    orbiter_weapon.state = source.state;
    orbiter_weapon.part_timer = u8::try_from(centre).expect("slot < 16");
    let centre_weapon = &mut cx.effects.weapons[centre];
    [centre_weapon.level_x_hi, centre_weapon.level_x_lo] = [source.level_x_hi, source.level_x_lo];
    [centre_weapon.level_y_hi, centre_weapon.level_y_lo] = [source.level_y_hi, source.level_y_lo];
    centre_weapon.vy = 0x0080;
    let weapon = &mut cx.effects.weapons[slot];
    weapon.part_timer += 1;
    if weapon.part_timer >= 5 {
        weapon.kind = 0x83;
    }
}

/// Type `$81` (`$9843`): an orbiter. Each frame it strikes every actor on the
/// screen (`$9F4F`), springs about its centre (its position an offset from it,
/// drawn turned 45 degrees and mirrored on odd frames) and, for the first pair,
/// flashes the palette; after `$E0` frames the attack ends and the handler returns.
fn orbiter(slot: usize, work: &mut Work, cx: &mut Cx<'_>, scroll: &mut Scroll) -> bool {
    #[cfg(test)]
    super::oracle::seen(0x9843);
    if cx.effects.meter < 2 {
        cx.effects.meter = 2;
    }
    smite(cx);
    integrate(slot, work, cx);
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
    let centre = usize::from(cx.effects.weapons[slot].part_timer);
    let centre_weapon = cx.effects.weapons[centre];
    work.x = u16::from_be_bytes([centre_weapon.level_x_hi, centre_weapon.level_x_lo])
        .wrapping_add(dx.cast_unsigned());
    work.y = u16::from_be_bytes([centre_weapon.level_y_hi, centre_weapon.level_y_lo])
        .wrapping_add(dy.cast_unsigned());
    if project_kept(slot, work, cx) {
        let hi = cx.effects.weapons[slot].level_x_hi;
        let (table, step) = if hi & 0x80 != 0 {
            (0x9C99, !hi & 0x0F)
        } else {
            (0x9C19, hi & 0x0F)
        };
        work.pointer = rom_word(cx.prg, 5, table + u16::from(step << 1 | mirror));
        draw(work, cx);
    }
    let weapon = &mut cx.effects.weapons[slot];
    weapon.param = weapon.param.wrapping_add(1);
    if weapon.param.trailing_zeros() >= 5 {
        cx.effects.sound[0] = 0x48;
    }
    if cx.effects.weapons[slot].param >= 0xE0 {
        end_attack(slot, cx, scroll);
        return false;
    }
    spring(slot, centre, cx);
    work.cpu_overflow = false;
    if cx.effects.weapons[slot].part & 0x80 != 0 {
        flash(slot, cx);
    }
    true
}

/// The orbiter's spring (`$99F7`-`$9AAA`): its y velocity pulled `$3C` back
/// toward the centre; when the pull, with the centre's `$7EB0` added, crosses
/// zero, the x velocity turns, the offset snaps to the centre and `$30` more goes
/// on, kept in the centre's y velocity too.
fn spring(slot: usize, centre: usize, cx: &mut Cx<'_>) {
    let kept = cx.effects.weapons[centre].vy;
    let weapon = &mut cx.effects.weapons[slot];
    let above = weapon.level_y_hi & 0x80 != 0;
    weapon.vy = if above {
        weapon.vy.wrapping_add(0x3C)
    } else {
        weapon.vy.wrapping_sub(0x3C)
    };
    if (kept.wrapping_add(weapon.vy) < 0) == above {
        return;
    }
    if above {
        weapon.vx = (!weapon.vx).wrapping_sub(0x0C);
        [weapon.level_y_hi, weapon.level_y_lo] = [0, 0];
        weapon.vy = weapon.vy.wrapping_add(0x30);
    } else {
        weapon.vx = (!weapon.vx).wrapping_add(0x0C);
        [weapon.level_y_hi, weapon.level_y_lo] = [0xFF, 0xFF];
        weapon.vy = weapon.vy.wrapping_sub(0x30);
    }
    let vy = weapon.vy;
    cx.effects.weapons[centre].vy = vy;
}

/// `$9AAD`: the first orbiter's flash, the sprite set `$9D19` on odd frames and
/// `$9D1D` on even ones, every palette's first colour `$30` each eighth frame and
/// `$0F` otherwise.
fn flash(slot: usize, cx: &mut Cx<'_>) {
    let table = if cx.world.frame_odd { 0x9D19 } else { 0x9D1D };
    for i in 0..4u16 {
        cx.effects.palette[16 + usize::from(i)] = rom_byte(cx.prg, 5, table + i);
    }
    let first = if cx.effects.weapons[slot].param.trailing_zeros() >= 3 {
        0x30
    } else {
        0x0F
    };
    for colour in cx.effects.palette.iter_mut().step_by(4) {
        *colour = first;
    }
    cx.effects.palette_dirty = 0x80;
}

/// `$9AF7`: the super attack ends: the power-up gone (`$0472` bit 7, the meter
/// `$FE`), the stored weapon back, the scroll and `$C2` restored, the palette's
/// sprite set from `$9D21` and every slot freed.
fn end_attack(slot: usize, cx: &mut Cx<'_>, scroll: &mut Scroll) {
    #[cfg(test)]
    super::oracle::seen(0x9AF7);
    let effects = &mut *cx.effects;
    effects.player.flags[0] &= 0x7F;
    effects.player.stage_script = 0;
    effects.meter = 0xFE;
    effects.weapon = effects.stored_weapon;
    effects.attack = if effects.weapon == 0 { 0 } else { 0x80 };
    for i in 0..3u16 {
        effects.palette[17 + usize::from(i)] = rom_byte(cx.prg, 5, 0x9D21 + i);
    }
    effects.scroll_lock = effects.weapons[slot].timer;
    for colour in effects.palette.iter_mut().step_by(4) {
        *colour = 0x0F;
    }
    effects.palette_dirty = 0x80;
    effects.busy = 0;
    for weapon in &mut effects.weapons {
        weapon.kind = 0;
    }
    scroll.gate = 0xFF;
}

/// `$9F4F`: takes `$C0` hit points from every hittable actor in slots 0-15 on
/// the screen; one it kills drops `$4C` px and starts dying (`$7D70` = `$80`).
fn smite(cx: &mut Cx<'_>) {
    let [camera_x, _] = cx.world.camera_x.to_be_bytes();
    let camera_y = cx.world.camera_y;
    for actor in cx.slots.main[..16].iter_mut().rev() {
        let body = &mut actor.body;
        if actor.kind == 0 || body.contact & 0x08 != 0 || body.hit & 0x80 != 0 {
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
        body.hit = 0x80;
        body.dying_timer = 0;
    }
}

/// A shot's flight, wall bounce and draw: `flight` picks the sprite in flight
/// (and may change `$7C20`), `spent` is the spent sprites' pointer table.
fn level_shot(
    slot: usize,
    work: &mut Work,
    cx: &mut Cx<'_>,
    flight: impl FnOnce(&mut Cx<'_>) -> u16,
    spent: u16,
) {
    integrate(slot, work, cx);
    let phase = cx.effects.weapons[slot].state;
    if phase & 0x80 == 0 {
        if phase & 0x40 != 0 {
            let weapon = &mut cx.effects.weapons[slot];
            work.cpu_overflow = gravity(weapon);
            weapon.timer = weapon.timer.wrapping_sub(1);
            if weapon.timer & 0x80 != 0 {
                weapon.kind = 0;
                return;
            }
        } else if solid(work, phase << 1, cx) {
            let timer = cx.effects.weapons[slot].timer;
            if timer & 1 == 0 {
                #[cfg(test)]
                super::oracle::seen(0x80CD);
                cx.effects.weapons[slot].timer = timer | 1;
                project(slot, work, cx);
                return;
            }
            #[cfg(test)]
            super::oracle::seen(0x80DA);
            let positive = cx.world.rng[0] & 0x01 != 0;
            recover(slot, cx, positive);
        }
    }
    if !project(slot, work, cx) {
        return;
    }
    let (phase, timer) = {
        let weapon = &cx.effects.weapons[slot];
        (weapon.state, weapon.timer)
    };
    if phase & 0x80 == 0 {
        work.pointer = flight(cx);
    } else {
        work.pointer = rom_word(cx.prg, 5, spent + u16::from(timer & 0xFE));
        let weapon = &mut cx.effects.weapons[slot];
        weapon.timer = weapon.timer.wrapping_sub(1);
        if weapon.timer & 0x80 != 0 {
            weapon.kind = 0;
        }
    }
    work.attr = cx.effects.weapons[slot].attr;
    draw_parity(slot, work, cx);
}

/// Handler `$8891` (`$0476` 2): the boomerang, which splits into three.
fn two(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    if cx.effects.weapons[slot].kind == 0x80 {
        orbit(slot, work, cx);
        return;
    }
    integrate(slot, work, cx);
    let phase = cx.effects.weapons[slot].state;
    if phase & 0x80 == 0 {
        if phase & 0x40 != 0 {
            let weapon = &mut cx.effects.weapons[slot];
            gravity(weapon);
            weapon.timer = weapon.timer.wrapping_sub(1);
            if weapon.timer & 0x80 != 0 {
                weapon.kind = 0;
                return;
            }
        } else if solid(work, phase << 1, cx) {
            cx.effects.sound[0] = 0x22;
            let timer = cx.effects.weapons[slot].timer;
            if timer & 1 != 0 {
                split(slot, cx);
                return;
            }
            cx.effects.weapons[slot].timer = timer | 1;
            project(slot, work, cx);
            return;
        }
    }
    if !project(slot, work, cx) {
        return;
    }
    let (attr, phase, timer, bounce) = {
        let weapon = &cx.effects.weapons[slot];
        (weapon.attr, weapon.state, weapon.timer, weapon.param)
    };
    work.attr = attr;
    if phase & 0x80 != 0 {
        if bounce & 0x80 == 0 {
            split(slot, cx);
            return;
        }
        work.pointer = rom_word(cx.prg, 5, 0x8A4B + u16::from(timer < 8) * 2);
        let weapon = &mut cx.effects.weapons[slot];
        weapon.timer = weapon.timer.wrapping_add(1);
        if weapon.timer >= 0x20 {
            weapon.kind = 0;
            return;
        }
    } else {
        let kind = cx.effects.weapons[slot].kind;
        if timer & 0x80 != 0 {
            if phase & 0x40 == 0 {
                let weapon = &mut cx.effects.weapons[slot];
                let [hi, lo] = weapon.vy.to_be_bytes();
                let (sub, borrow) = lo.overflowing_sub(2);
                let hi = if borrow { hi.wrapping_sub(1) } else { hi };
                weapon.vy = i16::from_be_bytes([hi, if sub >= 0xD0 { sub } else { 0xC0 }]);
            }
            work.pointer = rom_word(cx.prg, 5, 0x8A43 + u16::from(kind & 0x80 != 0) * 2);
        } else {
            if phase & 0x40 == 0 {
                let weapon = &mut cx.effects.weapons[slot];
                let [hi, lo] = weapon.vx.to_be_bytes();
                if attr & 0x40 != 0 {
                    let (sub, borrow) = lo.overflowing_sub(2);
                    let hi = if borrow { hi.wrapping_sub(1) } else { hi };
                    weapon.vx = i16::from_be_bytes([hi, if sub >= 0xD0 { sub } else { 0xC0 }]);
                } else {
                    let (add, carry) = lo.overflowing_add(2);
                    let hi = if carry { hi.wrapping_add(1) } else { hi };
                    weapon.vx = i16::from_be_bytes([hi, if add < 0x30 { add } else { 0x40 }]);
                }
            }
            work.pointer = rom_word(cx.prg, 5, 0x8A47 + u16::from(kind & 0x80 != 0) * 2);
        }
    }
    draw_parity(slot, work, cx);
}

/// Splits a boomerang into three parts, then one more (`$89AC`).
fn split(source: usize, cx: &mut Cx<'_>) {
    let mut part = 0usize;
    let mut slot = 0usize;
    while slot < 16 {
        if cx.effects.weapons[slot].kind == 0 {
            spawn_part(slot, part, source, cx);
            part += 1;
            if part >= 3 {
                break;
            }
        }
        slot += 1;
    }
    spawn_part(source, part, source, cx);
}

/// Writes one split part at `source`'s position (`$89D7`).
fn spawn_part(slot: usize, part: usize, source: usize, cx: &mut Cx<'_>) {
    let src = cx.effects.weapons[source];
    let at = u16::try_from(part).expect("part < 4");
    let vx = i16::from_be_bytes([
        rom_byte(cx.prg, 5, 0x8A2F + at),
        rom_byte(cx.prg, 5, 0x8A33 + at),
    ]);
    let vy = i16::from_be_bytes([
        rom_byte(cx.prg, 5, 0x8A37 + at),
        rom_byte(cx.prg, 5, 0x8A3B + at),
    ]);
    let bounce = rom_byte(cx.prg, 5, 0x8A3F + at);
    let weapon = &mut cx.effects.weapons[slot];
    weapon.vx = vx;
    weapon.vy = vy;
    weapon.param = bounce;
    weapon.kind = 0x06;
    weapon.state = 0x80;
    weapon.attr = 0;
    weapon.timer = 0;
    weapon.level_x_hi = src.level_x_hi;
    weapon.level_x_lo = src.level_x_lo;
    weapon.level_y_hi = src.level_y_hi;
    weapon.level_y_lo = src.level_y_lo;
    weapon.flags = 0x60;
    weapon.damage = 0x02;
}

/// Handler `$8D39` (`$0476` 3): the homing missile. False when the handler
/// returns before the other slots (`$8FD6`).
fn three(slot: usize, work: &mut Work, cx: &mut Cx<'_>) -> bool {
    match cx.effects.weapons[slot].kind {
        0x80 => return charge(slot, work, cx),
        0x81 => return spread(slot, cx),
        0x82.. => {
            spread_shot(slot, work, cx);
            return true;
        }
        _ => {}
    }
    integrate(slot, work, cx);
    let phase = cx.effects.weapons[slot].state;
    if phase & 0x80 == 0 {
        if phase & 0x40 != 0 {
            let weapon = &mut cx.effects.weapons[slot];
            gravity(weapon);
            weapon.timer = weapon.timer.wrapping_sub(1);
            if weapon.timer & 0x80 != 0 {
                weapon.kind = 0;
                return true;
            }
        } else if solid(work, phase << 1, cx) {
            let timer = cx.effects.weapons[slot].timer;
            if timer & 1 == 0 {
                cx.effects.weapons[slot].timer = timer | 1;
                project(slot, work, cx);
                return true;
            }
            let positive = cx.world.rng[0] & 0x80 == 0;
            recover(slot, cx, positive);
        }
    }
    tail(slot, work, cx);
    true
}

/// Handler `$91B2` (`$0476` 4): the three-way shot, drawn with `$99 = 0`; types
/// `$80` and up are its charged attack (`$92C4`).
fn four(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    match cx.effects.weapons[slot].kind {
        0x80 => return brew(slot, cx),
        0x81 => return bubble(slot, work, cx),
        0x82 => return tether(slot, work, cx),
        other @ 0x83.. => panic!("handler 4's type ${other:02X} is past the $95EF table"),
        _ => {}
    }
    integrate(slot, work, cx);
    let phase = cx.effects.weapons[slot].state;
    if phase & 0x80 == 0 {
        if phase & 0x40 != 0 {
            let weapon = &mut cx.effects.weapons[slot];
            gravity(weapon);
            weapon.timer = weapon.timer.wrapping_sub(1);
            if weapon.timer & 0x80 != 0 {
                weapon.kind = 0;
                return;
            }
        } else if solid(work, phase << 1, cx) {
            let timer = cx.effects.weapons[slot].timer;
            if timer & 1 == 0 {
                cx.effects.weapons[slot].timer = timer | 1;
                project(slot, work, cx);
                return;
            }
            if timer & 0x80 != 0 {
                #[cfg(test)]
                super::oracle::seen(0x9268);
                let pick = u16::from(cx.world.rng[0] & 0x02 | 0x01);
                let vx = word(cx, 0x9EF5 + pick, 0x9EEF + pick);
                let vy = word(cx, 0x9F01 + pick, 0x9EFB + pick);
                let weapon = &mut cx.effects.weapons[slot];
                weapon.vx = vx.cast_signed();
                weapon.vy = vy.cast_signed();
                weapon.state = 0x80;
                weapon.attr = 0;
                weapon.timer = 0x04;
                weapon.flags = 0xE0;
            } else {
                #[cfg(test)]
                super::oracle::seen(0x9275);
                let positive = cx.world.rng[0] & 0x80 == 0;
                recover(slot, cx, positive);
            }
        }
    }
    if !project(slot, work, cx) {
        return;
    }
    let weapon = &mut cx.effects.weapons[slot];
    if weapon.state & 0x80 == 0 {
        work.pointer = rom_word(cx.prg, 5, 0x92BA + u16::from((cx.world.frame & 0x04) >> 1));
    } else {
        work.pointer = rom_word(cx.prg, 5, 0x92BE + u16::from(weapon.timer & 0xFE));
        weapon.timer = weapon.timer.wrapping_sub(1);
        if weapon.timer & 0x80 != 0 {
            weapon.kind = 0;
        }
    }
    draw_parity(slot, work, cx);
}

/// Type `$80` (`$92D3`): while B is held, a type `$81` bubble from the player
/// every eighth frame (effect `$3F`); let go, it becomes the type `$82` tether
/// (effect `$2A`).
fn brew(slot: usize, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::oracle::seen(0x92D3);
    if cx.effects.attack & 0x40 == 0 {
        cx.effects.sound[1] = 0x2A;
        let_go(slot, cx);
        return;
    }
    let weapon = &mut cx.effects.weapons[slot];
    let count = weapon.part_timer;
    weapon.part_timer = count.wrapping_add(1);
    if count & 0x07 != 0 {
        return;
    }
    let Some(free) = free_slot(cx) else {
        return;
    };
    cx.effects.sound[0] = 0x3F;
    let (x, y) = if cx.effects.player.motion == 6 {
        above(cx)
    } else {
        let motion = u16::from(cx.effects.player.motion);
        beside_player(
            cx,
            ahead(cx, motion),
            word(cx, 0x9F43 + motion, 0x9F37 + motion),
        )
    };
    let [random, sign] = cx.world.rng;
    let drift = random >> 4;
    let vx = if sign & 0x80 != 0 {
        i16::from_be_bytes([0xFF, drift | 0xFC])
    } else {
        i16::from_be_bytes([0, drift & 0x03])
    };
    let weapon = &mut cx.effects.weapons[free];
    [weapon.level_x_hi, weapon.level_x_lo] = x.to_be_bytes();
    [weapon.level_y_hi, weapon.level_y_lo] = y.to_be_bytes();
    blow(free, vx, cx);
}

/// The `$9F07`-`$9F2B` x offset ahead of the player for `motion`.
fn ahead(cx: &Cx<'_>, motion: u16) -> u16 {
    if faces_left(cx) {
        word(cx, 0x9F2B + motion, 0x9F1F + motion)
    } else {
        word(cx, 0x9F13 + motion, 0x9F07 + motion)
    }
}

/// `$9489`/`$959F`'s shared part: `slot` becomes a type `$81` bubble with x
/// velocity `vx`, rising at `$FF:($F9 | $E0)`, its palette `$9637` by `$F1` bit 3.
fn blow(slot: usize, vx: i16, cx: &mut Cx<'_>) {
    let attr = rom_byte(cx.prg, 5, 0x9637 + u16::from(cx.world.frame & 0x08 != 0));
    let rise = cx.world.rng[0] | 0xE0;
    let weapon = &mut cx.effects.weapons[slot];
    weapon.kind = 0x81;
    weapon.state = 0;
    weapon.part_timer = 0;
    weapon.flags = 0x20;
    weapon.vx = vx;
    weapon.vy = i16::from_be_bytes([0xFF, rise]);
    weapon.attr = attr;
}

/// `$9521`: B let go, the charge becomes the type `$82` tether, its position an
/// offset from the player: ahead of him (velocity `$9ED6[4/5]`) or, aiming up,
/// `$FD70` above him (`$9ED6[3]`, `$7F60` bit 7 marking it vertical).
fn let_go(slot: usize, cx: &mut Cx<'_>) {
    let motion = u16::from(cx.effects.player.motion);
    let (aim_up, left) = (cx.effects.player.motion == 6, faces_left(cx));
    let (x, y, pick) = if aim_up {
        (0, 0xFD70, 3)
    } else {
        let y = word(cx, 0x9F43 + motion, 0x9F37 + motion).wrapping_add(0x0180);
        (ahead(cx, motion), y, if left { 4 } else { 5 })
    };
    let vx = word(cx, 0x9EF5 + pick, 0x9EEF + pick);
    let vy = word(cx, 0x9F01 + pick, 0x9EFB + pick);
    let weapon = &mut cx.effects.weapons[slot];
    weapon.kind = 0x82;
    weapon.damage = 1;
    weapon.state = 0;
    weapon.param = 0;
    weapon.part = 0;
    weapon.flags = 0x60;
    weapon.part_timer = if aim_up { 0x80 } else { 0 };
    weapon.vx = vx.cast_signed();
    weapon.vy = vy.cast_signed();
    [weapon.level_x_hi, weapon.level_x_lo] = x.to_be_bytes();
    [weapon.level_y_hi, weapon.level_y_lo] = y.to_be_bytes();
}

/// Type `$81` (`$930B`): a bubble rising for 48 frames, slowing by 1 a frame.
fn bubble(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::oracle::seen(0x930B);
    let weapon = &mut cx.effects.weapons[slot];
    if weapon.state & 0x80 != 0 {
        weapon.kind = 0;
        return;
    }
    weapon.part_timer = weapon.part_timer.wrapping_add(1);
    if weapon.part_timer >= 0x30 {
        weapon.kind = 0;
        return;
    }
    work.anchor = 0;
    work.tile = 0;
    work.attr = weapon.attr;
    work.chr_slot = 4;
    integrate(slot, work, cx);
    if !project(slot, work, cx) {
        return;
    }
    let weapon = &mut cx.effects.weapons[slot];
    weapon.vy = weapon.vy.wrapping_add(1);
    work.pointer = rom_word(cx.prg, 5, 0x9635);
    draw(work, cx);
}

/// Type `$82` (`$935E`): for `$C0` frames the tether springs about the player (its
/// position an offset from him, its velocity pulled back 4 a frame), a bubble
/// leaving it every eighth frame.
fn tether(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::oracle::seen(0x935E);
    work.anchor = 0;
    work.attr = 0;
    work.tile = 0;
    work.chr_slot = 4;
    let weapon = &mut cx.effects.weapons[slot];
    weapon.part = weapon.part.wrapping_add(1);
    if weapon.part >= 0xC0 {
        weapon.kind = 0;
        return;
    }
    if weapon.part_timer & 0x80 != 0 {
        #[cfg(test)]
        super::oracle::seen(0x93BA);
        weapon.vy = pull(weapon.vy, weapon.level_y_hi & 0x80 != 0, 0x50, 0xB0);
    } else {
        weapon.vx = pull(weapon.vx, weapon.level_x_hi & 0x80 != 0, 0x40, 0xC0);
    }
    let x = u16::from_be_bytes([weapon.level_x_hi, weapon.level_x_lo])
        .wrapping_add(weapon.vx.cast_unsigned());
    let y = u16::from_be_bytes([weapon.level_y_hi, weapon.level_y_lo])
        .wrapping_add(weapon.vy.cast_unsigned());
    [weapon.level_x_hi, weapon.level_x_lo] = x.to_be_bytes();
    [weapon.level_y_hi, weapon.level_y_lo] = y.to_be_bytes();
    let count = weapon.part;
    (work.x, work.y) = beside_player(cx, x, y.wrapping_sub(0x0180));
    if count.trailing_zeros() >= 3
        && let Some(free) = free_slot(cx)
    {
        cx.effects.sound[0] = 0x3F;
        let weapon = &mut cx.effects.weapons[free];
        [weapon.level_x_hi, weapon.level_x_lo] = work.x.to_be_bytes();
        [weapon.level_y_hi, weapon.level_y_lo] = work.y.to_be_bytes();
        let vx = cx.effects.weapons[slot].vx;
        blow(free, vx, cx);
    }
    if project_kept(slot, work, cx) {
        let frame = (cx.effects.weapons[slot].part >> 1) & 0x06;
        work.pointer = rom_word(cx.prg, 5, 0x963F + u16::from(frame));
        draw(work, cx);
    }
}

/// One axis of the tether's spring: the low byte up 4 when the offset is negative
/// (`behind`) and down 4 otherwise, held to `toward`/`away` in `$xx-$7F`/`$80-$xx`.
fn pull(velocity: i16, behind: bool, toward: u8, away: u8) -> i16 {
    let [hi, lo] = velocity.to_be_bytes();
    if behind {
        let (lo, carry) = lo.overflowing_add(4);
        let hi = hi.wrapping_add(u8::from(carry));
        let lo = if (toward..0x80).contains(&lo) {
            toward
        } else {
            lo
        };
        i16::from_be_bytes([hi, lo])
    } else {
        let (lo, borrow) = lo.overflowing_sub(4);
        let hi = hi.wrapping_sub(u8::from(borrow));
        let lo = if (0x80..away).contains(&lo) { away } else { lo };
        i16::from_be_bytes([hi, lo])
    }
}

/// The highest free weapon slot.
fn free_slot(cx: &Cx<'_>) -> Option<usize> {
    (0..16).rev().find(|&i| cx.effects.weapons[i].kind == 0)
}

/// The big-endian word from bank 5 bytes `hi` and `lo`.
fn word(cx: &Cx<'_>, hi: u16, lo: u16) -> u16 {
    u16::from_be_bytes([rom_byte(cx.prg, 5, hi), rom_byte(cx.prg, 5, lo)])
}

/// The player's level position plus `(dx, dy)`.
fn beside_player(cx: &Cx<'_>, dx: u16, dy: u16) -> (u16, u16) {
    let player = &cx.effects.player;
    (player.x.wrapping_add(dx), player.y.wrapping_add(dy))
}

/// Is the player facing left (`$046F` bit 6)?
fn faces_left(cx: &Cx<'_>) -> bool {
    cx.effects.player.facing & 0x40 != 0
}

/// Handler 2's charged attack, type `$80` (`$8A4F`): four parts circle the player
/// while B is held, then fly off as type `$81` boomerangs.
fn orbit(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::oracle::seen(0x8A4F);
    let weapon = cx.effects.weapons[slot];
    let turn = u16::from(weapon.part) + if weapon.param & 0x80 != 0 { 4 } else { 0 };
    let vx = weapon
        .vx
        .cast_unsigned()
        .wrapping_add(word(cx, 0x8C79 + turn, 0x8C71 + turn));
    let vy = weapon
        .vy
        .cast_unsigned()
        .wrapping_add(word(cx, 0x8C89 + turn, 0x8C81 + turn));
    let weapon = &mut cx.effects.weapons[slot];
    weapon.vx = vx.cast_signed();
    weapon.vy = vy.cast_signed();
    weapon.part_timer = weapon.part_timer.wrapping_add(1);
    if weapon.part_timer >= 0x40 {
        weapon.param ^= 0x80;
        weapon.part_timer = 0;
        cx.effects.sound[1] = 0x30;
    }
    let [hi, lo] = vx.to_be_bytes();
    let (back_lo, carry) = (!lo).overflowing_add(1);
    let (back_hi, round) = (!hi).overflowing_add(u8::from(carry));
    let back = u16::from_be_bytes([back_hi, back_lo]);
    let motion = u16::from(cx.effects.player.motion);
    (work.x, work.y) = if cx.effects.player.motion == 6 {
        #[cfg(test)]
        super::oracle::seen(0x8AFF);
        let dx = if faces_left(cx) { 0xFFE0 } else { 0x0020 };
        let (x, y) = beside_player(cx, dx, 0xFBF0);
        (x.wrapping_add(vy), y.wrapping_add(back))
    } else if faces_left(cx) {
        let (x, y) = beside_player(
            cx,
            word(cx, 0x8D15 + motion, 0x8D09 + motion),
            word(cx, 0x8D2D + motion, 0x8D21 + motion),
        );
        let x = x.wrapping_add(back).wrapping_add(u16::from(round));
        (x, y.wrapping_add(vy))
    } else {
        let (x, y) = beside_player(
            cx,
            word(cx, 0x8CFD + motion, 0x8CF1 + motion),
            word(cx, 0x8D2D + motion, 0x8D21 + motion),
        );
        (x.wrapping_add(vx), y.wrapping_add(vy))
    };
    if project_kept(slot, work, cx) {
        work.anchor = 0;
        work.attr = 0;
        work.pointer = rom_word(cx.prg, 5, 0x8C6D + u16::from((cx.world.frame & 4) >> 1));
        draw(work, cx);
    }
    if cx.effects.attack & 0x40 != 0 {
        return;
    }
    #[cfg(test)]
    super::oracle::seen(0x8BCB);
    cx.effects.sound[0] = 0x2F;
    let (aim_up, left) = (cx.effects.player.motion == 6, faces_left(cx));
    let weapon = &mut cx.effects.weapons[slot];
    weapon.kind = 0x81;
    [weapon.level_x_hi, weapon.level_x_lo] = work.x.to_be_bytes();
    [weapon.level_y_hi, weapon.level_y_lo] = work.y.to_be_bytes();
    weapon.flags = 0;
    weapon.attr = 0;
    weapon.damage = 2;
    weapon.state = 0;
    let pose = if aim_up {
        #[cfg(test)]
        super::oracle::seen(0x8C18);
        weapon.timer = 0x80;
        4
    } else {
        weapon.timer = 0;
        if left {
            weapon.attr = 0x40;
            2
        } else {
            0
        }
    };
    let tables = if weapon.param & 0x80 != 0 {
        0x8C67
    } else {
        0x8C61
    };
    let (table, part) = (rom_word(cx.prg, 5, tables + pose), u16::from(weapon.part));
    let vx = word(cx, table + part + 4, table + part);
    let vy = word(cx, table + part + 12, table + part + 8);
    let weapon = &mut cx.effects.weapons[slot];
    weapon.vx = vx.cast_signed();
    weapon.vy = vy.cast_signed();
}

/// Handler 3's charged attack, type `$80` (`$8F03`): it gathers in front of the
/// player for eight frames while B is held, then becomes the `$81` spread.
fn charge(slot: usize, work: &mut Work, cx: &mut Cx<'_>) -> bool {
    #[cfg(test)]
    super::oracle::seen(0x8F03);
    if cx.effects.attack & 0x40 == 0 {
        cx.effects.weapons[slot].kind = 0;
        return true;
    }
    let motion = u16::from(cx.effects.player.motion);
    (work.x, work.y) = if cx.effects.player.motion == 6 {
        #[cfg(test)]
        super::oracle::seen(0x8F16);
        let dx = if faces_left(cx) { 0xFFE0 } else { 0x0020 };
        beside_player(cx, dx, 0xFDF0)
    } else {
        let dx = if faces_left(cx) {
            word(cx, 0x9168 + motion, 0x9162 + motion)
        } else {
            word(cx, 0x9156 + motion, 0x914A + motion)
        };
        beside_player(cx, dx, word(cx, 0x917A + motion, 0x916E + motion))
    };
    if project_kept(slot, work, cx) && !cx.world.frame_odd {
        let frame = cx.effects.weapons[slot].part_timer & 0x02;
        work.pointer = rom_word(cx.prg, 5, 0x9142 + u16::from(frame));
        draw(work, cx);
    }
    let weapon = &mut cx.effects.weapons[slot];
    weapon.part_timer = weapon.part_timer.wrapping_add(1);
    if weapon.part_timer != 8 {
        return true;
    }
    weapon.kind = 0x81;
    weapon.part_timer = 0;
    weapon.part = 0;
    spread(slot, cx)
}

/// Type `$81` (`$8FC2`): while B is held, a `$82` shot every fourth frame for 64
/// frames, its angle stepping through `$9192`; then every shot is cleared. With no
/// free slot the game's `INC $7F60,X` (X = `$FF`) writes only the FME-7 command
/// latch `$805F`, which every parameter write sets again first.
fn spread(slot: usize, cx: &mut Cx<'_>) -> bool {
    #[cfg(test)]
    super::oracle::seen(0x8FC2);
    if cx.effects.attack & 0x40 != 0 {
        cx.effects.sound[0] = 0x2C;
    }
    let count = cx.effects.weapons[slot].part_timer;
    if cx.effects.attack & 0x40 == 0 || count >= 0x40 {
        for weapon in &mut cx.effects.weapons {
            weapon.kind = 0;
        }
        cx.effects.attack &= 0x80;
        return false;
    }
    cx.effects.weapons[slot].part_timer = count.wrapping_add(1);
    if count & 3 != 0 {
        return true;
    }
    let Some(free) = free_slot(cx) else {
        #[cfg(test)]
        super::oracle::seen(0x8FE8);
        return true;
    };
    let source = cx.effects.weapons[slot];
    let angle = u16::from(source.part);
    let along = word(cx, 0x91A2 + angle, 0x9192 + angle).cast_signed();
    let motion = u16::from(cx.effects.player.motion);
    let left = faces_left(cx);
    let (x, y) = if cx.effects.player.motion == 6 {
        #[cfg(test)]
        super::oracle::seen(0x9023);
        beside_player(cx, if left { 0xFFE0 } else { 0x0020 }, 0xFBF0)
    } else {
        let dx = if left {
            word(cx, 0x9168 + motion, 0x9162 + motion)
        } else {
            word(cx, 0x9156 + motion, 0x914A + motion)
        };
        beside_player(cx, dx, word(cx, 0x9186 + motion, 0x916E + motion))
    };
    let weapon = &mut cx.effects.weapons[free];
    weapon.part = (source.part_timer & 0x04) >> 1;
    weapon.kind = 0x82;
    weapon.part_timer = 0;
    weapon.damage = 1;
    weapon.flags = 0xE0;
    [weapon.level_x_hi, weapon.level_x_lo] = x.to_be_bytes();
    [weapon.level_y_hi, weapon.level_y_lo] = y.to_be_bytes();
    if cx.effects.player.motion == 6 {
        weapon.vx = along;
        weapon.vy = -0x50;
        weapon.timer = 0x80;
        weapon.attr = 0;
    } else {
        weapon.attr = cx.effects.player.facing & 0x40;
        weapon.vx = if left { -0x50 } else { 0x50 };
        weapon.vy = along;
        weapon.timer = 0;
    }
    let next = source.part.wrapping_add(1);
    cx.effects.weapons[slot].part = if next < 0x10 { next } else { 0 };
    true
}

/// One spread shot, type `$82` and up (`$90E1`): 24 frames of flight.
fn spread_shot(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    #[cfg(test)]
    super::oracle::seen(0x90E1);
    let weapon = &mut cx.effects.weapons[slot];
    weapon.flags = 0x60;
    weapon.part_timer = weapon.part_timer.wrapping_add(1);
    if weapon.part_timer >= 0x18 {
        weapon.kind = 0;
        return;
    }
    work.attr = weapon.attr;
    work.anchor = 0;
    work.tile = 0;
    work.chr_slot = 4;
    integrate(slot, work, cx);
    if !project(slot, work, cx) || cx.world.frame_odd {
        return;
    }
    let weapon = &cx.effects.weapons[slot];
    let table = if weapon.timer & 0x80 != 0 {
        0x913A
    } else {
        0x913E
    };
    work.pointer = rom_word(cx.prg, 5, table + u16::from(weapon.part));
    draw(work, cx);
}

/// A shot bounces off a wall and resets its velocity (`$80DA`, `$8E7A`): a falling
/// one to a random side, a level one back, both with a random vertical sign;
/// `positive` is the random pick for `+$20`.
fn recover(slot: usize, cx: &mut Cx<'_>, positive: bool) {
    let random = if positive { 0x0020 } else { -0x0020 };
    let weapon = &mut cx.effects.weapons[slot];
    if weapon.timer & 0x80 != 0 {
        weapon.vy = 0x0020;
        weapon.vx = random;
    } else {
        weapon.vx = if weapon.vx < 0 { 0x0020 } else { -0x0020 };
        weapon.vy = random;
    }
    weapon.state = 0x80;
    weapon.attr = 0;
    weapon.timer = 0x04;
    weapon.flags = 0xE0;
}

/// Projects, bounces off the screen edges and draws the missile (`$8D7E`-`$8E62`).
fn tail(slot: usize, work: &mut Work, cx: &mut Cx<'_>) {
    if !project(slot, work, cx) {
        return;
    }
    let (phase, timer) = {
        let weapon = &cx.effects.weapons[slot];
        (weapon.state, weapon.timer)
    };
    if phase & 0x80 != 0 {
        if cx.world.frame_odd {
            let weapon = &mut cx.effects.weapons[slot];
            weapon.timer = weapon.timer.wrapping_sub(1);
            if weapon.timer & 0x80 != 0 {
                weapon.kind = 0;
            }
            return;
        }
        work.pointer = rom_word(cx.prg, 5, 0x8EEF + u16::from(timer & 0xFE));
        let weapon = &mut cx.effects.weapons[slot];
        weapon.timer = weapon.timer.wrapping_sub(1);
        if weapon.timer & 0x80 != 0 {
            weapon.kind = 0;
            return;
        }
        draw(work, cx);
        return;
    }
    if timer & 0x80 != 0 {
        work.attr = 0;
        work.pointer = rom_word(cx.prg, 5, 0x8EEB + u16::from(cx.world.frame & 2));
        if edge(slot, cx, true) {
            draw_check(work, cx);
            return;
        }
    }
    work.attr = cx.effects.weapons[slot].attr;
    work.pointer = rom_word(cx.prg, 5, 0x8EE7 + u16::from(cx.world.frame & 2));
    edge(slot, cx, false);
    draw_check(work, cx);
}

/// One screen-edge bounce (`$8DD8`-`$8E58`): `horizontal` picks x (`$7D00`/`$7D10`)
/// over y (`$7D20`/`$7EB0`). True when the caller draws without the other axis.
/// The `BMI`/`BEQ` after each `STA $7F40,X` tests the `CMP` before it, so a step
/// down always ends there and a step up across goes on to y unless it reached `$40`.
fn edge(slot: usize, cx: &mut Cx<'_>, horizontal: bool) -> bool {
    if cx.effects.weapons[slot].param & 0x80 == 0 {
        let weapon = &mut cx.effects.weapons[slot];
        let (hi, lo) = velocity(weapon, horizontal);
        let (sub, borrow) = lo.overflowing_sub(4);
        let hi = if borrow { hi.wrapping_sub(1) } else { hi };
        set_velocity(weapon, horizontal, hi, sub);
        if (0x40..0xC0).contains(&sub) {
            #[cfg(test)]
            super::oracle::seen(if horizontal { 0x8DEE } else { 0x8E3D });
            weapon.param = sub;
        }
        return true;
    }
    let weapon = &mut cx.effects.weapons[slot];
    let (hi, lo) = velocity(weapon, horizontal);
    let (add, carry) = lo.overflowing_add(4);
    let hi = if carry { hi.wrapping_add(1) } else { hi };
    set_velocity(weapon, horizontal, hi, add);
    if !(0x40..0xC0).contains(&add) {
        return true;
    }
    weapon.param = add;
    !horizontal || add == 0x40
}

/// The high and low bytes of the velocity `edge` adjusts.
fn velocity(weapon: &Weapon, horizontal: bool) -> (u8, u8) {
    let [hi, lo] = if horizontal {
        weapon.vx.to_be_bytes()
    } else {
        weapon.vy.to_be_bytes()
    };
    (hi, lo)
}

/// Writes the two velocity bytes `edge` adjusted.
fn set_velocity(weapon: &mut Weapon, horizontal: bool, hi: u8, lo: u8) {
    let value = i16::from_be_bytes([hi, lo]);
    if horizontal {
        weapon.vx = value;
    } else {
        weapon.vy = value;
    }
}

/// Draws only on the odd frames the missile's tail draws (`$8E5B`).
fn draw_check(work: &mut Work, cx: &mut Cx<'_>) {
    if cx.world.frame_odd {
        draw(work, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::super::oracle::{Scene, assert_game, assert_routine, was_seen};
    use super::super::world::Cx;

    /// Diffs the port against every `$D444` call of `scene`; returns `(calls, live)`.
    fn diff(scene: Option<Scene>, frames: u64) -> (usize, usize) {
        let Some(mut scene) = scene else {
            return (0, 0);
        };
        let mut live = 0;
        let checked = assert_routine(&mut scene, frames, (0xD444, 14), &[0xD447], |r| {
            let prg = r.prg;
            live += usize::from(
                r.state
                    .effects
                    .weapons
                    .iter()
                    .any(|weapon| weapon.kind != 0),
            );
            let mut cx = Cx {
                prg,
                world: &r.state.world,
                slots: &mut r.state.slots,
                schedule: &mut r.state.schedule,
                effects: &mut r.state.effects,
                scroll: &mut r.state.scroll,
                oam: &mut r.state.oam,
            };
            super::step(&mut r.state.work, &mut cx);
        });
        (checked, live)
    }

    #[test]
    fn matches_the_oracle_on_the_tas() {
        if rotj_rom::for_test().is_none() {
            return;
        }
        let (checked, live) = diff(Scene::tas(351), 1650);
        assert!(checked >= 1000, "{checked} calls compared");
        assert!(live >= 100, "{live} frames with a live projectile");
    }

    /// TAS 370 with `$0476` poked to `weapon` and one shot of type `kind` per
    /// timer (`$7CB0`) placed in the ground under the player, moving right;
    /// returns the `$D444` calls compared over 40 frames.
    fn grounded(weapon: u8, kind: u8, timers: &[u8]) -> usize {
        let Some(mut scene) = Scene::tas(370) else {
            return 0;
        };
        scene.machine.bus.ram[0x476] = weapon;
        let ram = &scene.machine.bus.ram;
        let (x, y) = (
            u16::from_be_bytes([ram[0x460], ram[0x461]]),
            u16::from_be_bytes([ram[0x462], ram[0x463]]).wrapping_add(0x0200),
        );
        for (slot, &timer) in timers.iter().enumerate() {
            let shot: [(u16, u8); 11] = [
                (0x7BD0, kind),
                (0x7BB0, x.to_be_bytes()[0]),
                (0x7BC0, x.to_be_bytes()[1]),
                (0x7C60, y.to_be_bytes()[0]),
                (0x7C70, y.to_be_bytes()[1]),
                (0x7D00, 0x00),
                (0x7D10, 0x40),
                (0x7D20, 0x00),
                (0x7EB0, 0x00),
                (0x7C80, 0x00),
                (0x7CB0, timer),
            ];
            for (array, value) in shot {
                scene.machine.bus.wram[usize::from(array - 0x6000) + 8 + slot] = value;
            }
        }
        let (checked, live) = diff(Some(scene), 40);
        assert!(live >= 3, "{live} frames with a live projectile");
        checked
    }

    /// `$80C5`: a weapon 0 shot in a wall marks the hit, then bounces out the
    /// next frame.
    #[test]
    fn a_shot_bounces_off_a_wall() {
        if grounded(0, 0x02, &[0]) == 0 {
            return;
        }
        assert!(was_seen(0x80CD), "the shot did not reach the wall");
        assert!(was_seen(0x80DA), "the shot did not bounce");
    }

    /// `$924E`: weapon 4's wall bounce, for a level shot and a falling one (`$9ED6`).
    #[test]
    fn a_weapon_4_shot_bounces_off_a_wall() {
        if grounded(4, 0x0A, &[0x00, 0x80]) == 0 {
            return;
        }
        assert!(was_seen(0x9275), "the level shot did not bounce");
        assert!(was_seen(0x9268), "the falling shot did not bounce");
    }

    /// `$91B2`: weapon 4's three-way shots (`$9E09`), fired walking right from TAS
    /// 370 with `$0476` poked to 4; the whole loop chained.
    #[test]
    fn weapon_4_fires_three_ways() {
        let Some(mut scene) = Scene::tas(370) else {
            return;
        };
        scene.machine.bus.ram[0x476] = 4;
        scene.script(200, |i| if i % 8 < 2 { 0x82 } else { 0x80 });
        let frames = assert_game(scene, 200).frames;
        assert!(frames >= 190, "{frames} loop frames compared");
        assert!(was_seen(0x9E09), "no three-way shot fired");
    }

    /// `$8170`: weapon 1's shots, fired walking right from TAS 370 with `$0476`
    /// poked to 1, home in on the actors about (`$8288`); the whole loop chained.
    #[test]
    fn weapon_1_shots_home_in() {
        let Some(mut scene) = Scene::tas(370) else {
            return;
        };
        scene.machine.bus.ram[0x476] = 1;
        scene.script(200, |i| if i % 8 < 2 { 0x82 } else { 0x80 });
        let frames = assert_game(scene, 200).frames;
        assert!(frames >= 190, "{frames} loop frames compared");
        assert!(was_seen(0x831E), "no shot turned toward a target");
    }

    /// `$847D`: weapon 1's charge gathers (`$8496`), three parts circle it
    /// (`$85CF`, `$8525`) and letting go throws it (`$864A`, `$8515`).
    #[test]
    fn weapon_1_charges_and_throws_a_circle() {
        let frames = charged(1, 120);
        if frames == 0 {
            return;
        }
        assert!(frames >= 190, "{frames} loop frames compared");
        for path in [0x84B6, 0x85CF, 0x8525, 0x864A, 0x8515] {
            assert!(was_seen(path), "${path:04X} did not run");
        }
    }

    /// Weapon 1's charge aimed up (`$9EA7`) and thrown up with Up held (`$864A`).
    #[test]
    fn weapon_1_charges_aiming_up() {
        let frames = charged_with(1, 120, 0x12, 0x10);
        if frames == 0 {
            return;
        }
        assert!(frames >= 190, "{frames} loop frames compared");
        for path in [0x9EA7, 0x8668] {
            assert!(was_seen(path), "${path:04X} did not run");
        }
    }

    /// Weapon 2's four parts circling (`$8AFF`) and thrown (`$8C18`) aiming up.
    #[test]
    fn weapon_2_charges_aiming_up() {
        let frames = charged_with(2, 120, 0x12, 0x10);
        if frames == 0 {
            return;
        }
        assert!(frames >= 190, "{frames} loop frames compared");
        for path in [0x8AFF, 0x8C18] {
            assert!(was_seen(path), "${path:04X} did not run");
        }
    }

    /// Weapon 3's charge gathering (`$8F16`) and its spread (`$9023`) aiming up.
    #[test]
    fn weapon_3_charges_aiming_up() {
        let frames = charged_with(3, 120, 0x12, 0x10);
        if frames == 0 {
            return;
        }
        assert!(frames >= 190, "{frames} loop frames compared");
        for path in [0x8F16, 0x9023] {
            assert!(was_seen(path), "${path:04X} did not run");
        }
    }

    /// `$92C4`: weapon 4's charge blows bubbles (`$92D3`, `$930B`) and, let go,
    /// springs about the player as a tether blowing more (`$935E`).
    #[test]
    fn weapon_4_charges_and_blows_bubbles() {
        let frames = charged(4, 120);
        if frames == 0 {
            return;
        }
        assert!(frames >= 190, "{frames} loop frames compared");
        for path in [0x92D3, 0x930B, 0x935E] {
            assert!(was_seen(path), "${path:04X} did not run");
        }
    }

    /// Weapon 4's charge aimed up (`$9EA7`), its tether springing vertically.
    #[test]
    fn weapon_4_charges_aiming_up() {
        let frames = charged_with(4, 120, 0x12, 0x10);
        if frames == 0 {
            return;
        }
        assert!(frames >= 190, "{frames} loop frames compared");
        assert!(was_seen(0x93BA), "the tether did not spring vertically");
    }

    /// `$8FE8` with no free slot: once weapon 3's charge is out, slots 0-13 get
    /// type `$82` spread shots at the player, so its spread finds none and `INC
    /// $7F60,X` writes the FME-7 command latch `$805F`; the loop chained through it
    /// and on still matches.
    #[test]
    fn weapon_3_spread_with_no_free_slot() {
        let Some(mut scene) = charge_scene(3) else {
            return;
        };
        scene.script(200, |i| if (15..135).contains(&i) { 0x02 } else { 0 });
        scene.run(66, &[]);
        let ram = &scene.machine.bus.ram;
        let (x, y) = (
            u16::from_be_bytes([ram[0x460], ram[0x461]]),
            u16::from_be_bytes([ram[0x462], ram[0x463]]).wrapping_sub(0x0100),
        );
        let wram = &mut scene.machine.bus.wram;
        assert_eq!(wram[0x1BDF], 0x80, "the charge is not in slot 15");
        for slot in 0..14 {
            let shot: [(u16, u8); 10] = [
                (0x7BD0, 0x82),
                (0x7BB0, x.to_be_bytes()[0]),
                (0x7BC0, x.to_be_bytes()[1]),
                (0x7C60, y.to_be_bytes()[0]),
                (0x7C70, y.to_be_bytes()[1]),
                (0x7D00, 0),
                (0x7D10, 0),
                (0x7D20, 0),
                (0x7EB0, 0),
                (0x7F60, 0),
            ];
            for (array, value) in shot {
                wram[usize::from(array - 0x6000) + slot] = value;
            }
        }
        let frames = assert_game(scene, 134).frames;
        assert!(frames >= 125, "{frames} loop frames compared");
        assert!(was_seen(0x8FE8), "the spread never found the slots full");
    }

    /// `$8712`: letting go of weapon 1's charge while it gathers clears it.
    #[test]
    fn weapon_1_charge_let_go_early_is_cleared() {
        let frames = charged(1, 0x36);
        if frames == 0 {
            return;
        }
        assert!(frames >= 110, "{frames} loop frames compared");
        assert!(was_seen(0x8712), "$8712 did not run");
    }

    /// The upward shots `$9EB5[$0476]` of weapons 0-5, fired standing at TAS 470
    /// with Up held, and their flight; the whole loop chained.
    #[test]
    fn every_weapon_fires_up() {
        for (weapon, handler) in [0x9BCE, 0x9BF5, 0x9C06, 0x9C2E, 0x9C7E, 0x9CD9]
            .into_iter()
            .enumerate()
        {
            let Some(mut scene) = Scene::tas(470) else {
                return;
            };
            scene.machine.bus.ram[0x476] = u8::try_from(weapon).expect("a weapon");
            scene.machine.bus.ram[0xB8] = 0x80;
            scene.machine.bus.wram[0x1BD0..0x1BE0].fill(0);
            scene.isolate(&[]);
            scene.script(120, |i| if i % 16 < 2 { 0x12 } else { 0x10 });
            let frames = assert_game(scene, 120).frames;
            assert!(
                frames >= 110,
                "weapon {weapon}: {frames} loop frames compared"
            );
            assert!(was_seen(handler), "${handler:04X} did not run");
        }
    }

    /// Stands at TAS 470 with no actors about, holding B for `hold` frames after a
    /// press, then lets go, with weapon `weapon` and no shots out; the whole loop
    /// chained from there.
    fn charged(weapon: u8, hold: u64) -> usize {
        charged_with(weapon, hold, 0x02, 0)
    }

    /// [`charged`] pressing B, then holding `buttons` (B among them), then `after`.
    fn charged_with(weapon: u8, hold: u64, buttons: u8, after: u8) -> usize {
        hold_b(charge_scene(weapon), hold, buttons, after, 60)
    }

    /// TAS 470 with weapon `weapon`, `$B8` bit 7 set, no shots out and no actors.
    fn charge_scene(weapon: u8) -> Option<Scene> {
        let mut scene = Scene::tas(470)?;
        scene.machine.bus.ram[0x476] = weapon;
        scene.machine.bus.ram[0xB8] = 0x80;
        scene.machine.bus.wram[0x1BD0..0x1BE0].fill(0);
        scene.isolate(&[]);
        Some(scene)
    }

    /// Chains the loop through `scene`: B pressed on frame 15, `buttons` held to
    /// `15 + hold`, then `after` for `tail` frames; returns the loop frames compared.
    fn hold_b(scene: Option<Scene>, hold: u64, buttons: u8, after: u8, tail: u64) -> usize {
        let Some(mut scene) = scene else {
            return 0;
        };
        scene.script(15 + hold + tail, |i| {
            if i == 15 {
                0x02
            } else if (16..15 + hold).contains(&i) {
                buttons
            } else if i >= 15 + hold {
                after
            } else {
                0
            }
        });
        assert_game(scene, 15 + hold + tail).frames
    }

    /// `$9781`: weapon 5's super attack, let go with the meter full: every slot
    /// claimed (`$9790`), five orbiting pairs released (`$9B4A`, `$9843`) and,
    /// `$E0` frames on, the power-up's end (`$9AF7`).
    #[test]
    fn weapon_5_unleashes_its_super_attack() {
        let scene = charge_scene(5).map(|mut scene| {
            scene.machine.bus.ram[0x477] = 0x0E;
            scene
        });
        let frames = hold_b(scene, 120, 0x02, 0, 300);
        if frames == 0 {
            return;
        }
        assert!(frames >= 420, "{frames} loop frames compared");
        for path in [0x97AC, 0x9B4A, 0x9843, 0x9AF7] {
            assert!(was_seen(path), "${path:04X} did not run");
        }
    }

    /// Weapon 5's super attack let go aiming up (`$97F9`).
    #[test]
    fn weapon_5_unleashes_aiming_up() {
        let scene = charge_scene(5).map(|mut scene| {
            scene.machine.bus.ram[0x477] = 0x0E;
            scene
        });
        let frames = hold_b(scene, 120, 0x12, 0x10, 300);
        if frames == 0 {
            return;
        }
        assert!(frames >= 420, "{frames} loop frames compared");
        assert!(was_seen(0x97F9), "$97F9 did not run");
    }

    /// `$9797`: with the meter below 2, weapon 5's charge is dropped.
    #[test]
    fn weapon_5_charge_needs_the_meter() {
        let scene = charge_scene(5).map(|mut scene| {
            scene.machine.bus.ram[0x477] = 0;
            scene
        });
        let frames = hold_b(scene, 120, 0x02, 0, 60);
        if frames == 0 {
            return;
        }
        assert!(frames >= 190, "{frames} loop frames compared");
        assert!(was_seen(0x979E), "$979E did not run");
    }

    /// `$9A56`: weapon 3's charged attack (`$9AE3`) gathers (`$8F03`), fires its
    /// spread (`$8FC2`) and the spread shots fly (`$90E1`).
    #[test]
    fn weapon_3_charges_and_fires_a_spread() {
        let frames = charged(3, 120);
        if frames == 0 {
            return;
        }
        assert!(frames >= 190, "{frames} loop frames compared");
        for path in [0x9AE3, 0x8F03, 0x8FC2, 0x90E1] {
            assert!(was_seen(path), "${path:04X} did not run");
        }
    }

    /// `$9AA0`: weapon 2's four parts circle the player (`$8A4F`) and fly off when
    /// B is let go (`$8BCB`).
    #[test]
    fn weapon_2_charges_and_throws_four_boomerangs() {
        let frames = charged(2, 120);
        if frames == 0 {
            return;
        }
        assert!(frames >= 190, "{frames} loop frames compared");
        for path in [0x9AA0, 0x8A4F, 0x8BCB] {
            assert!(was_seen(path), "${path:04X} did not run");
        }
    }

    /// `$9A81`: weapon 0's charged attack is a plain type `$80` shot.
    #[test]
    fn weapon_0_charges() {
        let frames = charged(0, 80);
        if frames == 0 {
            return;
        }
        assert!(frames >= 150, "{frames} loop frames compared");
        assert!(was_seen(0x9A81), "$9A81 did not run");
    }

    /// Pads that fire: walking right tapping B, walking right or left firing every
    /// third frame, jumping (A, then B twice in the air), crouched and aiming up.
    const FIRING: [fn(u64) -> u8; 6] = [
        |i| if i % 12 < 2 { 0x82 } else { 0x80 },
        |i| if i % 3 == 0 { 0x82 } else { 0x80 },
        |i| if i % 3 == 0 { 0x42 } else { 0x40 },
        |i| match i % 40 {
            0..2 => 0x81,
            2..12 => 0x01,
            12..14 | 20..22 => 0x02,
            _ => 0,
        },
        |i| if i % 6 < 2 { 0x22 } else { 0x20 },
        |i| if i % 6 < 2 { 0x12 } else { 0x10 },
    ];

    /// From TAS 470 with the TAS's weapon 3 and the actors about, each [`FIRING`]
    /// pattern chains 300 frames of the whole loop; firing walking right, a shield
    /// deflects a missile that falls into the screen-edge bounce's window (`$8E3D`).
    #[test]
    fn firing_scenes_match_the_oracle() {
        for pads in FIRING {
            let Some(mut scene) = Scene::tas(470) else {
                return;
            };
            scene.script(300, pads);
            let frames = assert_game(scene, 300).frames;
            assert!(frames >= 290, "{frames} loop frames compared");
        }
        assert!(was_seen(0x8E3D), "no deflected missile reached the window");
    }

    /// From TAS 600, past the stage's weapon pickup, `$0476` poked to `weapon`: each
    /// [`FIRING`] pattern chains 200 frames among the actors, the room change included.
    fn fires_in_every_pose(weapon: u8) {
        for pads in FIRING {
            let Some(mut scene) = Scene::tas(600) else {
                return;
            };
            scene.machine.bus.ram[0x476] = weapon;
            scene.script(200, pads);
            let frames = assert_game(scene, 200).frames;
            assert!(
                frames >= 140,
                "weapon {weapon}: {frames} loop frames compared"
            );
        }
    }

    /// The TAS's weapon 4 in stage 4: a bubble hit at 17353 is dropped before it counts.
    #[test]
    fn weapon_4_bubbles_match_the_tas_in_stage_4() {
        let Some(scene) = Scene::tas(17300) else {
            return;
        };
        assert_eq!(assert_game(scene, 100).frames, 99);
    }

    #[test]
    fn weapon_0_fires_in_every_pose() {
        fires_in_every_pose(0);
    }

    #[test]
    fn weapon_1_fires_in_every_pose() {
        fires_in_every_pose(1);
    }

    #[test]
    fn weapon_2_fires_in_every_pose() {
        fires_in_every_pose(2);
    }

    #[test]
    fn weapon_3_fires_in_every_pose() {
        fires_in_every_pose(3);
    }

    #[test]
    fn weapon_4_fires_in_every_pose() {
        fires_in_every_pose(4);
    }

    #[test]
    fn weapon_5_fires_in_every_pose() {
        fires_in_every_pose(5);
    }

    #[test]
    fn matches_the_oracle_on_the_route() {
        if rotj_rom::for_test().is_none() {
            return;
        }
        let (checked, _) = diff(Scene::route(600), 200);
        assert!(checked >= 150, "{checked} calls compared");
        let (checked, _) = diff(Scene::route(840), 200);
        assert!(checked >= 150, "{checked} calls compared");
    }
}
