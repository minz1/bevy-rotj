//! The bank-13 routines handlers share, each called where the 6502 calls it.

use super::oam::{DrawCall, Metasprite, attr};
use super::slots::{Body, Field, contact};
use super::world::{Cx, Effects, Motion, Player, Schedule, World, hurt, orientation, stage_script};
use crate::prg;
use crate::render::ppu;
use crate::sim::spawn::bank_slice;

/// The bits of [`Work::probe_flags`] (`$9F`), which [`Player::hit`](super::world::Player::hit)
/// also receives whole.
pub mod probe_flags {
    /// The tile dealt damage (spikes and the damage tiles).
    pub const DAMAGED: u8 = 0x80;
    /// The vertical probe met solid: the floor when falling, the ceiling when rising.
    pub const BLOCKED_Y: u8 = 0x40;
}

/// The values of [`Work::anchor`] (`$9A`); the port writes only 0 and this bit.
pub mod anchor {
    /// The actor sits in the band below the view height and is drawn bottom-anchored.
    pub const BOTTOM: u8 = 0x40;
}

/// The zero-page working set `$90-$A6`, carried from slot to slot as the game's is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Work {
    /// Level x, 1/16 px (`$90:$91`).
    pub x: u16,
    /// Level y, 1/16 px (`$92:$93`).
    pub y: u16,
    /// Screen x (`$94`).
    pub screen_x: u8,
    /// Screen y (`$95`).
    pub screen_y: u8,
    /// Metasprite pointer (`$96/$97`).
    pub pointer: u16,
    /// CHR shadow the metasprite selects (`$98`).
    pub chr_slot: u8,
    /// Attribute bits for the draw, merged into `$7C00` at the end (`$99`).
    pub attr: u8,
    /// Whether it is drawn bottom-anchored (`$9A`): 0 or [`anchor::BOTTOM`].
    pub anchor: u8,
    /// This frame's x step (`$9B:$9C`).
    pub step_x: i16,
    /// The y velocity before gravity (`$9D:$9E`).
    pub vy: i16,
    /// The probe's result flags (`$9F`), cleared on load: the [`probe_flags`] bits.
    pub probe_flags: u8,
    /// The reference x velocity (`$A0:$A1`).
    pub ref_vx: i16,
    /// Added to the x velocity at the end of the frame (`$A2:$A3`).
    pub ax: i16,
    /// Tile bits for the draw (`$A4`).
    pub tile: u8,
    /// Hit-box index (`$A5`).
    pub hitbox: u8,
    /// Frames lived (`$A6`).
    pub age: u8,
    /// Damage a response hands the player (`$A7`).
    pub damage: u8,
    /// The running slot's index, set by the actor loop (`$10`).
    pub slot: u8,
}

/// Where an actor is relative to the view (`$A852`'s result).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// On screen; the position is in [`Work::screen_x`] and [`Work::screen_y`].
    On,
    /// Off screen, within 128 px of it (`$80`).
    Near,
    /// Further off (`$C0`).
    Far,
}

/// The boss-defeated script's kind (`boss_defeated_script` family).
pub(super) const DEFEAT_SCRIPT: u8 = 0x50;
/// The harmless burst's kind (`harmless_burst` family).
const BURST: u8 = 0x08;
/// The pickup's kind (`pickup` family), a dying drop's successor.
pub(super) const PICKUP: u8 = 0x10;

const MARGIN: i32 = 128 * 16;
const BOTTOM_BAND: i32 = 80 * 16;
const FAR_BELOW: i32 = 384 * 16;

/// `$7DD0` bit 7 in the families that free themselves on leaving the screen once drawn:
/// it has been drawn.
pub const DRAWN: u8 = 0x80;

/// A byte of PRG `bank` at a CPU address.
pub(crate) fn rom_byte(prg: &[u8], bank: u8, addr: u16) -> u8 {
    prg::byte(prg, usize::from(bank), usize::from(addr)).expect("address inside PRG")
}

/// A little-endian word of PRG `bank` at a CPU address.
pub(crate) fn rom_word(prg: &[u8], bank: u8, addr: u16) -> u16 {
    u16::from_le_bytes([rom_byte(prg, bank, addr), rom_byte(prg, bank, addr + 1)])
}

/// The high byte of a word.
pub(crate) fn hi(value: u16) -> u8 {
    value.to_be_bytes()[0]
}

/// The low byte of a word.
pub(crate) fn lo(value: u16) -> u8 {
    value.to_be_bytes()[1]
}

/// `N` bytes of PRG `bank` from a CPU address.
pub(crate) fn rom_bytes<const N: usize>(prg: &[u8], bank: u8, addr: u16) -> [u8; N] {
    let bytes = bank_slice(prg, bank, addr & 0x1FFF).expect("address inside PRG");
    std::array::from_fn(|i| bytes[i])
}

/// `N` little-endian words of PRG `bank` from a CPU address.
pub(crate) fn rom_words<const N: usize>(prg: &[u8], bank: u8, addr: u16) -> [u16; N] {
    std::array::from_fn(|i| rom_word(prg, bank, addr + u16::try_from(2 * i).expect("table fits")))
}

/// `N` 16-bit values from a table of their high bytes at `hi` and one of their low bytes at `lo`.
pub(crate) fn rom_split_words<const N: usize>(prg: &[u8], bank: u8, hi: u16, lo: u16) -> [u16; N] {
    let (high, low): ([u8; N], [u8; N]) = (rom_bytes(prg, bank, hi), rom_bytes(prg, bank, lo));
    std::array::from_fn(|i| u16::from_be_bytes([high[i], low[i]]))
}

#[cfg(test)]
pub use super::combat::collect;
pub use super::combat::{
    BossContact, BossFlash, ContactMode, Hitboxes, boss_contact, boss_defeated, boss_flash,
    contact, dying, hurt_player, start_hurt, touch,
};
pub use super::tile_block::{FIELD_MASKS, TileBlock, copy_metatiles, queue_block};

/// Loads the slot into the working set (`$A000`).
pub fn load(body: &Body, work: &mut Work) {
    work.x = body.x;
    work.y = body.y;
    work.chr_slot = body.chr_slot;
    work.probe_flags = 0;
    work.age = body.age;
}

/// Steps the working position by the slot's velocity (`$A79B`).
pub fn integrate(body: &Body, work: &mut Work) {
    work.ref_vx = body.ref_vx;
    work.ax = body.vx;
    work.step_x = body.vx.wrapping_add(body.vx_drift);
    work.x = work.x.wrapping_add_signed(work.step_x);
    work.vy = body.vy;
    work.y = work.y.wrapping_add_signed(work.vy);
}

/// Projects the working position onto the screen (`$A852`).
pub fn on_screen(work: &mut Work, world: &World) -> Visibility {
    work.anchor = 0;
    let (horizontal, vertical) = (
        orientation::horizontal_two_page(world.orientation),
        orientation::vertical_two_page(world.orientation),
    );
    let camera_x = i32::from(world.camera_x.wrapping_add(8) & 0xFFF0);
    let dx = i32::from(work.x) - camera_x;
    if dx < 0 {
        return if vertical {
            Visibility::Near
        } else {
            left_or_above(dx)
        };
    }
    if dx >= world.view.width {
        return if vertical {
            Visibility::Near
        } else {
            right_or_below(dx, world.view.width + MARGIN)
        };
    }
    work.screen_x = low_byte(dx >> 4);
    let dy = i32::from(work.y) - i32::from(world.camera_y.wrapping_add(8));
    if dy < 0 {
        return if horizontal {
            Visibility::Near
        } else {
            left_or_above(dy)
        };
    }
    if dy >= world.view.height + BOTTOM_BAND {
        return if horizontal {
            Visibility::Near
        } else {
            right_or_below(dy, FAR_BELOW)
        };
    }
    let row = if dy >= world.view.height {
        work.anchor = anchor::BOTTOM;
        dy + 256
    } else {
        dy
    };
    work.screen_y = low_byte(row >> 4);
    Visibility::On
}

/// The X register `$A852` leaves when it returns off screen: the high byte of the
/// distance on the axis that failed, which a handler that does not reload X indexes with.
#[must_use]
pub fn off_screen_x(work: &Work, world: &World) -> u8 {
    let (dx, left) = work
        .x
        .overflowing_sub(world.camera_x.wrapping_add(8) & 0xFFF0);
    let distance = if left || dx >= 0x1000 {
        dx
    } else {
        work.y.wrapping_sub(world.camera_y.wrapping_add(8))
    };
    distance.to_be_bytes()[0]
}

pub(super) fn low_byte(value: i32) -> u8 {
    value.to_le_bytes()[0]
}

fn left_or_above(d: i32) -> Visibility {
    if d >= -MARGIN {
        Visibility::Near
    } else {
        Visibility::Far
    }
}

fn right_or_below(d: i32, far: i32) -> Visibility {
    if d >= far {
        Visibility::Far
    } else {
        Visibility::Near
    }
}

/// The working x minus the player's, as the game's 16-bit difference (`$ABDF`).
#[must_use]
pub fn player_dx(work: &Work, player: &Player) -> i16 {
    work.x.wrapping_sub(player.x).cast_signed()
}

/// The working y minus the player's, as the game's 16-bit difference (`$AF43`).
#[must_use]
pub fn player_dy(work: &Work, player: &Player) -> i16 {
    work.y.wrapping_sub(player.y).cast_signed()
}

/// The `$99` bit 6 an actor facing `dx` away turns to (`$939C`, `$9972`).
#[must_use]
pub fn face(dx: i16) -> u8 {
    let sign = if dx < 0 { 0x80u8 } else { 0 };
    (sign >> 1) ^ attr::FLIP_X
}

/// The page column and page index under the working position (`$A0A9`: `$20`, `$21`).
#[must_use]
pub fn map_page(work: &Work, cx: &Cx<'_>) -> (u8, u8) {
    let [x_hi, _] = work.x.to_be_bytes();
    let [y_hi, _] = work.y.to_be_bytes();
    let vertical = cx.world.orientation & orientation::VERTICAL != 0;
    let page = crate::map::page_index(x_hi, y_hi, vertical);
    (cx.schedule.page_columns[usize::from(page)], page)
}

/// On a horizontal-then-vertical level (`$DD` bits 7-6 = `10`), is `column` one of the
/// first eight page columns (`$AEC6`, `column` in `$30`)?
#[must_use]
pub fn is_page_column(column: u8, cx: &Cx<'_>) -> bool {
    orientation::horizontal_two_page(cx.world.orientation)
        && cx.schedule.page_columns[..8].contains(&column)
}

/// The overflow flag `ADC` leaves for `a + b + carry`; `SBC b` is `ADC !b`.
#[must_use]
pub fn adc_overflows(a: u8, b: u8, carry: bool) -> bool {
    let sum = i16::from(a.cast_signed()) + i16::from(b.cast_signed()) + i16::from(carry);
    i8::try_from(sum).is_err()
}

/// The overflow flag a 16-bit signed subtraction leaves for `a - b`.
#[must_use]
pub fn sub_overflows16(a: u16, b: u16) -> bool {
    a.cast_signed().overflowing_sub(b.cast_signed()).1
}

/// Draws the metasprite at [`Work::pointer`] from `bank` (`$C247`; `$8FC4` uses bank 1).
pub fn draw(work: &mut Work, bank: u8, cx: &mut Cx<'_>) -> bool {
    let sprite = Metasprite::from_rom(cx.prg, bank, work.pointer).expect("metasprite");
    draw_metasprite(work, &sprite, cx)
}

/// Draws `sprite` where [`Work`] places it (`$C247`); the overflow flag it leaves.
pub fn draw_metasprite(work: &mut Work, sprite: &Metasprite, cx: &mut Cx<'_>) -> bool {
    let call = DrawCall {
        x: i16::from(work.screen_x),
        y: i16::from(work.screen_y),
        attr: work.attr,
        tile: work.tile,
        chr_slot: work.chr_slot,
        bottom: work.anchor & anchor::BOTTOM != 0,
    };
    cx.oam.draw(sprite, call, cx.world.frame_odd)
}

/// Queues the status bar's two 5-digit counters: `value` and the boss's hit points (`$D1F5`).
pub fn queue_counters(value: u16, cx: &mut Cx<'_>) {
    for (number, column) in [(value, 8), (cx.effects.boss_hp, 18)] {
        let mut top = [0x0A; 6];
        let mut bottom = [0x0B; 6];
        let mut rest = number;
        for (i, place) in [10000, 1000, 100, 10, 1].into_iter().enumerate() {
            let digit = rest / place;
            rest %= place;
            top[i] = cx.prg.digit_tiles[usize::from(digit)];
            bottom[i] = top[i] | 1;
        }
        cx.effects.vram.push(ppu::tile_addr(0, 26, column), 0, &top);
        cx.effects
            .vram
            .push(ppu::tile_addr(0, 27, column), 0, &bottom);
    }
}

/// Bank 13 `$ADD8`: 16-bit pointer to the `$AC16` part-field table.
const PART_GROUPS_PTR: u16 = 0xADD8;
/// Bank 13 `$ACC6`: per-variant 16-bit pointers to four part records.
const PART_VARIANTS: u16 = 0xACC6;
/// Bank 13 `$ACBD`: per-variant sprite flags for the burst slot.
const PART_FLAGS: u16 = 0xACBD;
/// PRG bank holding the `$AC16` burst-part tables.
const PARTS_BANK: u8 = 13;

/// Spawns type `$08`'s four parts around the working position (`$AC16`); the slot used.
pub fn spawn_parts(variant: u8, work: &Work, cx: &mut Cx<'_>) -> Option<usize> {
    let index = cx.slots.free_main(0..16)?;
    let groups = rom_word(cx.prg, PARTS_BANK, PART_GROUPS_PTR);
    let parts = rom_word(cx.prg, PARTS_BANK, PART_VARIANTS + 2 * u16::from(variant));
    let slot = &mut cx.slots.main[index];
    for part in 0..4u16 {
        let data = rom_word(cx.prg, PARTS_BANK, parts + 2 * part);
        let byte = |i: u16| rom_byte(cx.prg, PARTS_BANK, data + i);
        let array = |i: u16| rom_word(cx.prg, PARTS_BANK, groups + 12 * part + 2 * i);
        let [x_hi, x_lo] = work
            .x
            .wrapping_add(rom_word(cx.prg, PARTS_BANK, data))
            .to_be_bytes();
        let [y_hi, y_lo] = work
            .y
            .wrapping_add(rom_word(cx.prg, PARTS_BANK, data + 2))
            .to_be_bytes();
        for (i, value) in [x_lo, x_hi, y_lo, y_hi, byte(4), byte(5)]
            .into_iter()
            .enumerate()
        {
            let field = Field::from_array(array(u16::try_from(i).expect("< 6")))
                .expect("$ADD8 table names a Body array");
            slot.body.set(field, value);
        }
    }
    slot.body.flags = rom_byte(cx.prg, PARTS_BANK, PART_FLAGS + u16::from(variant));
    slot.kind = BURST;
    slot.brain = super::families::Brain::new(BURST);
    slot.body.set(Field::RefVxHigh, 0x20);
    slot.body.contact = contact::PASSIVE | contact::SMITE_PROOF;
    Some(index)
}

/// The level end (bank 14 `$D274`), on the player's death or a boss's: the player
/// freezes in motion 7 with `$047A` = `$C0` and `$0481` = `$80`, `$B8`, `$0480`
/// and `$0465` clear, and every projectile is freed.
pub fn level_end(effects: &mut Effects) {
    let player = &mut effects.player;
    player.motion = Motion::Dying;
    player.facing |= attr::PALETTE;
    player.hurt = hurt::STUNNED | hurt::INVULNERABLE;
    player.stage_script = stage_script::FROZEN;
    player.death_timer = 0;
    player.death_flags = 0;
    effects.attack = 0;
    for projectile in &mut effects.projectiles {
        projectile.free();
    }
}

/// Frees the running slot (`$7CC0,x` = 0) where the rest of the loop can see it,
/// so a spawn later in the same handler may reuse it; the actor loop then keeps
/// what the slot holds instead of the handler's copy.
pub fn free_own_slot(kind: &mut u8, body: &Body, work: &Work, cx: &mut Cx<'_>) {
    *kind = super::slots::EMPTY;
    let own = &mut cx.slots.main[usize::from(work.slot)];
    own.free();
    own.body = body.clone();
}

/// Frees the slot and ORs `rearm` into its schedule cell (`$CD50`).
pub fn despawn(kind: &mut u8, body: &Body, rearm: u8, schedule: &mut Schedule) {
    schedule.cells[usize::from(body.cell)] |= rearm;
    *kind = super::slots::EMPTY;
}

/// How close the player is (`$A025`'s result).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proximity {
    /// Within reach (`$80`).
    Touching,
    /// Not within reach (`$00`).
    Clear,
    /// 384 px or more away on an axis (`$C0`).
    Far,
}

/// Classifies the player's distance by the hit box in [`Work::hitbox`] (`$A025`).
#[must_use]
pub fn player_proximity(work: &Work, cx: &Cx<'_>) -> Proximity {
    debug_assert_eq!(work.hitbox, 0, "every caller sets hit box 0");
    let player = &cx.effects.player;
    let exit = cx.prg.hitboxes.exit;
    let class = |distance: u16| {
        if distance >= 0x1800 {
            Proximity::Far
        } else {
            Proximity::Clear
        }
    };
    let dx = work.x.abs_diff(player.x);
    if dx >= 0x1000 {
        return class(dx);
    }
    if low_byte(i32::from(dx >> 4)) >= exit.half_width {
        return Proximity::Clear;
    }
    let distance = work.y.abs_diff(player.y);
    if distance >= 0x1000 {
        return class(distance);
    }
    let reach = if work.y < player.y {
        player.reach_y
    } else {
        exit.height
    };
    if low_byte(i32::from(distance >> 4)) >= reach {
        Proximity::Clear
    } else {
        Proximity::Touching
    }
}

/// The tile bits for the CHR slot in [`Work::chr_slot`] (`$AC06`).
pub fn tile_bits(work: &mut Work, prg: &[u8]) {
    work.tile = rom_byte(prg, 13, 0xAC0E + u16::from(work.chr_slot));
}

/// The highest free slot among 16-23, where children go (`$AF38`).
#[must_use]
pub fn free_child_slot(slots: &super::slots::Slots) -> Option<usize> {
    slots.free_main(16..24)
}

/// Gravity and the x velocity at the end of a handler, then the store (`$A8E4`).
pub fn end_frame(body: &mut Body, work: &mut Work) {
    let [hi, lo] = work.vy.wrapping_add(6).to_be_bytes();
    body.vy = if hi & 0x80 == 0 {
        i16::from(lo.min(0x7F))
    } else {
        i16::from_be_bytes([0xFF, lo.max(0x80)])
    };
    let vx = body.vx.wrapping_add(work.ax);
    let [hi, lo] = vx.to_be_bytes();
    let snap = if hi & 0x80 == 0 {
        lo < 0x07
    } else {
        lo >= 0xFA
    };
    body.vx = if snap { 0 } else { vx };
    store(body, work);
}

/// Stores the working position, attribute bits and age (`$A934`).
pub fn store(body: &mut Body, work: &mut Work) {
    body.x = work.x;
    body.y = work.y;
    work.attr &= attr::FLIP_Y | attr::FLIP_X | attr::BEHIND;
    body.flags = (body.flags & attr::PALETTE) | work.attr;
    body.age = work.age;
}

#[cfg(test)]
mod layout {
    use super::Work;

    fn word(ram: &[u8; 2048], hi: usize) -> u16 {
        u16::from_be_bytes([ram[hi], ram[hi + 1]])
    }

    fn put(ram: &mut [u8; 2048], hi: usize, value: u16) {
        ram[hi..hi + 2].copy_from_slice(&value.to_be_bytes());
    }

    impl Work {
        pub(crate) fn lift(ram: &[u8; 2048]) -> Work {
            Work {
                x: word(ram, 0x90),
                y: word(ram, 0x92),
                screen_x: ram[0x94],
                screen_y: ram[0x95],
                pointer: u16::from_le_bytes([ram[0x96], ram[0x97]]),
                chr_slot: ram[0x98],
                attr: ram[0x99],
                anchor: ram[0x9A],
                step_x: word(ram, 0x9B).cast_signed(),
                vy: word(ram, 0x9D).cast_signed(),
                probe_flags: ram[0x9F],
                ref_vx: word(ram, 0xA0).cast_signed(),
                ax: word(ram, 0xA2).cast_signed(),
                tile: ram[0xA4],
                hitbox: ram[0xA5],
                age: ram[0xA6],
                damage: ram[0xA7],
                slot: ram[0x10],
            }
        }

        pub(crate) fn project(&self, ram: &mut [u8; 2048]) {
            put(ram, 0x90, self.x);
            put(ram, 0x92, self.y);
            ram[0x94] = self.screen_x;
            ram[0x95] = self.screen_y;
            ram[0x96..0x98].copy_from_slice(&self.pointer.to_le_bytes());
            ram[0x98] = self.chr_slot;
            ram[0x99] = self.attr;
            ram[0x9A] = self.anchor;
            put(ram, 0x9B, self.step_x.cast_unsigned());
            put(ram, 0x9D, self.vy.cast_unsigned());
            ram[0x9F] = self.probe_flags;
            put(ram, 0xA0, self.ref_vx.cast_unsigned());
            put(ram, 0xA2, self.ax.cast_unsigned());
            ram[0xA4] = self.tile;
            ram[0xA5] = self.hitbox;
            ram[0xA6] = self.age;
            ram[0xA7] = self.damage;
            ram[0x10] = self.slot;
        }
    }
}

/// Adds the signed velocity byte at `velocity` to the position split across `hi`:`lo`
/// (the four-part bursts' parts, `$08` and `$92`), returning the new position.
pub fn move_by_velocity(body: &mut Body, lo: u16, hi: u16, velocity: u16) -> u16 {
    let field = |array: u16| {
        Field::from_array(array).expect("the four-part bursts' tables name Body arrays")
    };
    let (lo, hi, velocity) = (field(lo), field(hi), field(velocity));
    let velocity = i16::from(body.get(velocity).cast_signed());
    let moved = u16::from_be_bytes([body.get(hi), body.get(lo)]).wrapping_add_signed(velocity);
    let [high, low] = moved.to_be_bytes();
    body.set(lo, low);
    body.set(hi, high);
    moved
}

#[cfg(test)]
mod tests {
    use super::super::oracle::{Routine, Scene, assert_routine};
    use super::*;

    #[test]
    fn byte_carry_is_word_maths() {
        for hi in [0x00, 0x7F, 0x80, 0xFF] {
            for lo in 0..=0xFFu8 {
                let w = u16::from_be_bytes([hi, lo]);
                for b in 0..=0xFFu8 {
                    let (sum, carry) = lo.overflowing_add(b);
                    let (diff, borrow) = lo.overflowing_sub(b);
                    let sign = if b & 0x80 != 0 { 0xFF } else { 0 };
                    let add = u16::from_be_bytes([hi.wrapping_add(u8::from(carry)), sum]);
                    let sub = u16::from_be_bytes([hi.wrapping_sub(u8::from(borrow)), diff]);
                    let signed = u16::from_be_bytes([
                        hi.wrapping_add(sign).wrapping_add(u8::from(carry)),
                        sum,
                    ]);
                    let minus = u16::from_be_bytes([
                        hi.wrapping_add(0xFF).wrapping_add(u8::from(carry)),
                        sum,
                    ]);
                    assert_eq!(w.wrapping_add(u16::from(b)), add);
                    assert_eq!(w.wrapping_sub(u16::from(b)), sub);
                    assert_eq!(w.wrapping_add_signed(i16::from(b.cast_signed())), signed);
                    assert_eq!(w.wrapping_add(0xFF00 | u16::from(b)), minus);
                }
            }
        }
        for a_hi in 0..=0xFFu8 {
            for b_hi in 0..=0xFFu8 {
                for (a_lo, b_lo) in [(0x00u8, 0x00u8), (0xFF, 0x01), (0x80, 0x80)] {
                    let (lo, carry) = a_lo.overflowing_add(b_lo);
                    let hi = a_hi.wrapping_add(b_hi).wrapping_add(u8::from(carry));
                    let v = adc_overflows(a_hi, b_hi, carry);
                    let a = u16::from_be_bytes([a_hi, a_lo]).cast_signed();
                    let b = u16::from_be_bytes([b_hi, b_lo]).cast_signed();
                    let (word, overflow) = a.overflowing_add(b);
                    assert_eq!(word.cast_unsigned(), u16::from_be_bytes([hi, lo]));
                    assert_eq!(overflow, v);
                }
            }
        }
    }

    #[test]
    fn nametable_steps_are_word_maths() {
        for addr in 0..=0xFFFFu16 {
            let [hi, lo] = addr.to_be_bytes();
            let (lo_next, carry) = lo.overflowing_add(0x40);
            let step = u16::from_be_bytes([hi.wrapping_add(u8::from(carry)) & 0x23, lo_next]);
            assert_eq!(addr.wrapping_add(0x40) & 0x23FF, step);
            assert_eq!(addr >= 0x23C0, hi >= 0x24 || (hi == 0x23 && lo >= 0xC0));
            if hi & 0x23 == 0x23 && lo >= 0xC0 {
                let wrap = u16::from_be_bytes([hi & 0x24, lo.wrapping_add(0x40)]);
                assert_eq!(addr & 0x243F, wrap);
            }
        }
        for quad in 0..=0x3Fu8 {
            assert_eq!(quad | 0xC0, quad.wrapping_add(0xC0));
        }
    }

    /// Bank 14's `$C5F2`/`$C6F2` pair is `x << 4`, which its readers compute instead.
    #[test]
    fn the_shift_table_is_times_sixteen() {
        let Some(prg) = crate::prg::for_test() else {
            return;
        };
        for x in 0..=255u16 {
            let entry = [rom_byte(prg, 14, 0xC5F2 + x), rom_byte(prg, 14, 0xC6F2 + x)];
            assert!(u16::from_be_bytes(entry) == x << 4, "entry {x}");
        }
    }

    #[test]
    fn adc_overflow_edge_cases() {
        assert!(adc_overflows(0x7F, 1, false)); // $7F + 1 crosses into -128
        assert!(adc_overflows(0x80, 0x80, false)); // -128 + -128
        assert!(adc_overflows(0x7F, 0, true)); // the carry in crosses
        assert!(!adc_overflows(0xFF, 0xFF, true)); // -1 + -1 + 1
        assert!(!adc_overflows(0x10, 0x20, false));
    }

    #[test]
    fn sub_overflow_edge_cases() {
        assert!(sub_overflows16(0x8000, 1)); // i16::MIN - 1
        assert!(sub_overflows16(0, 0x8000)); // 0 - i16::MIN
        assert!(sub_overflows16(0x7FFF, 0xFFFF)); // 32767 + 1
        assert!(!sub_overflows16(0x4000, 0x4000));
        assert!(!sub_overflows16(0x8000, 0x8000));
    }

    /// Route frames 841-999: types `$06`, `$0A` and `$22` all run.
    fn routine(entry: u16, exits: &[u16], port: impl FnMut(&mut Routine<'_>)) {
        let Some(mut scene) = Scene::route(840) else {
            return;
        };
        let checked = assert_routine(&mut scene, 159, (entry, 13), exits, port);
        assert!(checked > 100, "${entry:04X}: {checked} calls");
    }

    #[test]
    fn census_of_stage_1_1() {
        let expected: [(u16, &[u16]); 8] = [
            (0x81D4, &[0xA000, 0xA025]),
            (0x823C, &[0xA000, 0xA852, 0xA8E4, 0xA9C4, 0xAA57, 0xCD50]),
            (0x8506, &[0xA852, 0xC247]),
            (
                0x85FD,
                &[
                    0xA000, 0xA79B, 0xA852, 0xA8E4, 0xA9C4, 0xAA57, 0xABDF, 0xAC16, 0xB17F, 0xC247,
                ],
            ),
            (
                0x8B55,
                &[
                    0xA000, 0xA79B, 0xA7DD, 0xA852, 0xA8E4, 0xA9C4, 0xAA57, 0xAB39, 0xABA0, 0xAF6A,
                    0xC247, 0xCAFE, 0xCB39, 0xCB68, 0xCD5F,
                ],
            ),
            (
                0x9555,
                &[
                    0xA000, 0xA56E, 0xA601, 0xA79B, 0xA7DD, 0xA7F0, 0xA80A, 0xA837, 0xA852, 0xA8E4,
                    0xA95E, 0xA9C4, 0xAA57, 0xAB13, 0xABDF, 0xAC06, 0xAF38, 0xAF6A, 0xB17F, 0xC247,
                    0xC26F, 0xC99D, 0xCAFE, 0xCB39, 0xCB68, 0xCD50, 0xCD5F,
                ],
            ),
            (
                0x9823,
                &[
                    0xA000, 0xA852, 0xA934, 0xAC06, 0xAEE3, 0xB17F, 0xC247, 0xC93B,
                ],
            ),
            (
                0x9953,
                &[
                    0xA000, 0xA79B, 0xA7DD, 0xA852, 0xA8E4, 0xC247, 0xCAFE, 0xCB39, 0xCB68,
                ],
            ),
        ];
        let Some(census) = super::super::oracle::census() else {
            return;
        };
        let actual: Vec<(u16, Vec<u16>)> = census
            .into_iter()
            .map(|(family, routines)| (family, routines.into_iter().collect()))
            .collect();
        let expected: Vec<(u16, Vec<u16>)> =
            expected.iter().map(|(f, r)| (*f, r.to_vec())).collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn census_of_stage_1_2() {
        let expected: [(u16, &[u16]); 10] = [
            (
                0x823C,
                &[
                    0xA000, 0xA0A9, 0xA0B8, 0xA0CE, 0xA1C4, 0xA239, 0xA369, 0xA463, 0xA490, 0xA56E,
                    0xA59F, 0xA852, 0xA8E4, 0xA9C4, 0xAA57, 0xAC16, 0xAEC6, 0xB108, 0xC0C6, 0xC247,
                    0xC26F,
                ],
            ),
            (0x8506, &[0xA852, 0xC247]),
            (
                0x8B55,
                &[
                    0xA000, 0xA79B, 0xA7DD, 0xA852, 0xA8E4, 0xA9C4, 0xAA57, 0xAB39, 0xAB93, 0xAF6A,
                    0xB108, 0xC247, 0xCAFE, 0xCB39, 0xCB68, 0xCD5F,
                ],
            ),
            (0x8C56, &[0xAF38]),
            (
                0x8D70,
                &[
                    0xA000, 0xA7DD, 0xA852, 0xAC16, 0xAEE3, 0xC247, 0xC26F, 0xCAFE, 0xCB39, 0xCB68,
                ],
            ),
            (0x8FD9, &[0xA852, 0xAEE3, 0xC247]),
            (
                0x9364,
                &[
                    0xA000, 0xA56E, 0xA601, 0xA852, 0xA8E4, 0xA9C4, 0xAA57, 0xAB13, 0xABDF, 0xAC06,
                    0xAF38, 0xAF43, 0xC247,
                ],
            ),
            (
                0x94B2,
                &[0xA000, 0xA852, 0xA934, 0xAC06, 0xAEE3, 0xC247, 0xC93B],
            ),
            (
                0x9823,
                &[0xA000, 0xA852, 0xA934, 0xAC06, 0xAEE3, 0xC247, 0xC93B],
            ),
            (
                0x98D6,
                &[
                    0xA000, 0xA56E, 0xA601, 0xA79B, 0xA7DD, 0xA7F0, 0xA80A, 0xA852, 0xA8E4, 0xA9C4,
                    0xAA57, 0xAB13, 0xABDF, 0xAC06, 0xAF43, 0xAF7D, 0xC247, 0xC26F, 0xC93B, 0xC99D,
                    0xCAFE, 0xCB39, 0xCB68,
                ],
            ),
        ];
        let Some(mut scene) = Scene::tas(2047) else {
            return;
        };
        let mut census = std::collections::BTreeMap::new();
        super::super::oracle::survey(&mut scene, 6045 - 2047, &mut census);
        let actual: Vec<(u16, Vec<u16>)> = census
            .into_iter()
            .map(|(family, routines)| (family, routines.into_iter().collect()))
            .collect();
        let expected: Vec<(u16, Vec<u16>)> =
            expected.iter().map(|(f, r)| (*f, r.to_vec())).collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn load_matches_a000() {
        routine(0xA000, &[0xA024], |r| {
            let slot = r.slot();
            load(&r.state.slots.main[slot].body, &mut r.state.work);
        });
    }

    #[test]
    fn integrate_matches_a79b() {
        routine(0xA79B, &[0xA7DC], |r| {
            let slot = r.slot();
            integrate(&r.state.slots.main[slot].body, &mut r.state.work);
        });
    }

    #[test]
    fn on_screen_matches_a852() {
        routine(0xA852, &[0xA8AF, 0xA8DC, 0xA8E3], |r| {
            let seen = on_screen(&mut r.state.work, &r.state.world);
            r.a = Some(match seen {
                Visibility::On => 0,
                Visibility::Near => 0x80,
                Visibility::Far => 0xC0,
            });
        });
    }

    #[test]
    fn end_frame_matches_the_oracle() {
        routine(0xA8E4, &[0xA95D], |r| {
            let slot = r.slot();
            end_frame(&mut r.state.slots.main[slot].body, &mut r.state.work);
        });
    }

    /// Route frames 901-930 with the `$0A` in slot 3 given x velocity and drift.
    fn moving(entry: u16, exits: &[u16], port: impl FnMut(&mut Routine<'_>)) {
        let Some(mut scene) = Scene::route(900) else {
            return;
        };
        let wram = &mut scene.machine.bus.wram;
        assert_eq!(wram[0x1CC3], 0x0A);
        for (at, value) in [
            (0x1E33, 0x01),
            (0x1E43, 0x23),
            (0x1E53, 0xFF),
            (0x1E63, 0xC0),
        ] {
            wram[at] = value;
        }
        let checked = assert_routine(&mut scene, 30, (entry, 13), exits, port);
        assert!(checked >= 30, "${entry:04X}: {checked} calls");
    }

    #[test]
    fn integrate_and_end_frame_move_with_velocity() {
        moving(0xA79B, &[0xA7DC], |r| {
            let slot = r.slot();
            integrate(&r.state.slots.main[slot].body, &mut r.state.work);
        });
        moving(0xA8E4, &[0xA95D], |r| {
            let slot = r.slot();
            end_frame(&mut r.state.slots.main[slot].body, &mut r.state.work);
        });
    }

    fn check(
        scene: Option<Scene>,
        frames: u64,
        (entry, bank): (u16, u8),
        exits: &[u16],
        min: usize,
        port: impl FnMut(&mut Routine<'_>),
    ) {
        let Some(mut scene) = scene else {
            return;
        };
        let checked = assert_routine(&mut scene, frames, (entry, bank), exits, port);
        assert!(checked >= min, "${entry:04X}: {checked} calls");
    }

    const AF97_EXITS: [u16; 5] = [0x8D27, 0x9482, 0x9942, 0x84F3, 0x8C65];
    const AFD6_EXITS: [u16; 14] = [
        0x9CA5, 0x9E14, 0x9F3C, 0x8FA8, 0x91A3, 0x95FD, 0x9AC8, 0x8726, 0x885A, 0x8E6F, 0x9511,
        0x95F7, 0x9697, 0x9766,
    ];

    #[test]
    fn boss_flash_matches_af97() {
        check(Scene::tas(6281), 490, (0xAF97, 13), &AF97_EXITS, 100, |r| {
            let a = r
                .in_slot(|slot, work, cx| boss_flash(&mut slot.body, work, cx.world))
                .to_accumulator();
            r.a = Some(a);
        });
    }

    #[test]
    fn boss_contact_matches_afd6() {
        check(Scene::tas(6281), 360, (0xAFD6, 13), &AFD6_EXITS, 100, |r| {
            let (a, x, y) = (r.begin.cpu.a, r.begin.cpu.x, r.begin.cpu.y);
            let amount = u16::from_be_bytes([x, a]);
            let mode = ContactMode::from_byte(y);
            let a = r
                .in_slot(|slot, work, cx| boss_contact(amount, mode, &mut slot.body, work, cx))
                .to_accumulator();
            r.a = Some(a);
        });
    }

    #[test]
    fn boss_defeated_matches_b1c4() {
        check(
            Scene::tas(6575),
            2,
            (0xB1C4, 13),
            &[0x9959, 0x8D3C, 0x9496, 0x850A, 0x8C76],
            1,
            |r| {
                let timer = r.begin.cpu.x;
                r.in_slot(|_, _, cx| boss_defeated(timer, cx));
            },
        );
    }

    #[test]
    fn queue_counters_matches_d1f5() {
        check(
            Scene::tas(6281),
            300,
            (0xD1F5, 14),
            &[0x8A85, 0xD10A],
            50,
            |r| {
                let value = u16::from_le_bytes([r.begin.ram[0x20], r.begin.ram[0x21]]);
                r.in_slot(|_, _, cx| queue_counters(value, cx));
            },
        );
    }

    fn near_slot(mut scene: Scene, slot: usize) -> Scene {
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.player(x, y);
        scene
    }

    #[test]
    fn player_proximity_matches_a025() {
        let scene = Scene::replay("stage1-inputs.csv", 990);
        check(
            scene,
            34,
            (0xA025, 13),
            &[0xA04D, 0xA050, 0xA0A6],
            20,
            |r| {
                let seen = r.in_slot(|_, work, cx| player_proximity(work, cx));
                r.a = Some(match seen {
                    Proximity::Touching => 0x80,
                    Proximity::Clear => 0,
                    Proximity::Far => 0xC0,
                });
            },
        );
    }

    #[test]
    fn dying_matches_a56e() {
        if rotj_rom::for_test().is_none() {
            return;
        }
        let exits = [0xA5CF, 0xA600, 0xA614, 0xA61E];
        check(Scene::route(845), 115, (0xA56E, 13), &exits, 100, |r| {
            let variant = r.begin.cpu.a;
            let dying = r
                .in_slot(|slot, work, cx| dying(&mut slot.kind, &mut slot.body, work, variant, cx));
            r.a = Some(if dying { 0x80 } else { 0 });
        });
        let mut deaths = 0;
        let scene = Scene::route(900).map(|scene| scene.brawl(1, 0x300, 120));
        check(scene, 120, (0xA56E, 13), &exits, 100, |r| {
            let variant = r.begin.cpu.a;
            let dying = r
                .in_slot(|slot, work, cx| dying(&mut slot.kind, &mut slot.body, work, variant, cx));
            deaths += usize::from(dying);
            r.a = Some(if dying { 0x80 } else { 0 });
        });
        assert!(deaths >= 14, "{deaths} dying frames");
    }

    #[test]
    fn store_matches_a934() {
        check(Scene::route(840), 159, (0xA934, 13), &[0xA95D], 100, |r| {
            let slot = r.slot();
            store(&mut r.state.slots.main[slot].body, &mut r.state.work);
        });
    }

    #[test]
    fn store_in_child_slots_writes_the_weapon_bytes_they_alias() {
        let Some(mut scene) = Scene::route(996) else {
            return;
        };
        scene.machine.bus.trace.pokes.push((0xA934, 0x00A6, 0x5A));
        let mut children = 0;
        let checked = assert_routine(&mut scene, 10, (0xA934, 13), &[0xA95D], |r| {
            children += usize::from(r.slot() >= 16);
            r.in_slot(|slot, work, _| store(&mut slot.body, work));
        });
        assert!(
            checked >= 10 && children >= 5,
            "{checked} calls, {children} in 16-23"
        );
    }

    #[test]
    fn collect_matches_ab39() {
        let scene = Scene::replay("stage1-inputs.csv", 960).map(|scene| near_slot(scene, 0));
        check(scene, 30, (0xAB39, 13), &[0xAB47, 0xABA7], 1, |r| {
            assert_eq!(usize::from(r.begin.cpu.x), r.slot());
            r.in_slot(|slot, _, cx| collect(&mut slot.kind, &slot.body, cx));
        });
    }

    #[test]
    fn tile_bits_match_ac06() {
        check(Scene::route(845), 115, (0xAC06, 13), &[0xAC0D], 100, |r| {
            tile_bits(&mut r.state.work, r.prg);
        });
    }

    #[test]
    fn free_child_slot_matches_af38() {
        check(Scene::route(995), 5, (0xAF38, 13), &[0xAF42], 1, |r| {
            let found = free_child_slot(&r.state.slots);
            r.y = Some(found.map_or(0xFF, |slot| u8::try_from(slot - 16).expect("< 8")));
        });
    }

    #[test]
    fn despawn_matches_cd50() {
        let scene = Scene::replay("stage1-inputs.csv", 915);
        check(scene, 6, (0xCD50, 14), &[0xCD5E], 1, |r| {
            assert_eq!(usize::from(r.begin.cpu.x), r.slot());
            let rearm = r.begin.cpu.a;
            r.in_slot(|slot, _, cx| despawn(&mut slot.kind, &slot.body, rearm, cx.schedule));
        });
    }

    fn fights() -> Vec<Option<Scene>> {
        vec![
            Scene::route(900).map(|scene| scene.brawl(0, 0x300, 160)),
            Scene::route(900).map(|scene| scene.brawl(1, 0x300, 160)),
            Scene::route(960).map(|scene| near_slot(scene, 3)),
            Scene::route(960).map(|scene| near_slot(scene, 1)),
            Scene::route(997).map(|scene| near_slot(scene, 23)),
            Scene::route(997).map(|scene| {
                let mut scene = near_slot(scene, 23);
                scene.machine.bus.trace.pokes.push((0xD450, 0x046E, 0));
                scene
            }),
        ]
    }

    #[test]
    fn contact_matches_a9c4() {
        if rotj_rom::for_test().is_none() {
            return;
        }
        let exits = [0xAA61, 0xAB2C, 0xAB38, 0xAB47, 0xABA7];
        let mut taken = 0;
        for scene in fights() {
            check(scene, 160, (0xA9C4, 13), &exits, 50, |r| {
                let damage = r.begin.cpu.a;
                let before = r.state.effects.clone();
                r.in_slot(|slot, work, cx| {
                    contact(&mut slot.kind, &mut slot.body, work, damage, cx);
                });
                taken += usize::from(r.state.effects != before);
            });
        }
        assert!(taken >= 10, "{taken} contacts taken");
    }

    #[test]
    fn touch_matches_aee3() {
        if rotj_rom::for_test().is_none() {
            return;
        }
        let mut hits = 0;
        for scene in fights() {
            check(scene, 160, (0xAEE3, 13), &[0xAF26, 0xAF29], 0, |r| {
                let damage = r.begin.cpu.a;
                let hit = r.in_slot(|_, work, cx| touch(work, damage, cx));
                hits += usize::from(hit);
                r.a = Some(if hit { 0xFF } else { 0 });
            });
        }
        assert!(hits >= 1, "{hits} touches");
    }

    #[test]
    fn hurt_player_matches_b17f() {
        for scene in fights() {
            check(scene, 160, (0xB17F, 13), &[0xB1C3], 0, |r| {
                hurt_player(&mut r.state.effects);
            });
        }
    }

    #[test]
    fn spawn_parts_matches_ac16() {
        let scene = Scene::route(960).map(|scene| near_slot(scene, 3));
        check(scene, 160, (0xAC16, 13), &[0xAC22, 0xACBC], 1, |r| {
            let variant = r.begin.cpu.a;
            r.in_slot(|_, work, cx| spawn_parts(variant, work, cx));
        });
    }

    #[test]
    fn player_dy_matches_af43() {
        check(Scene::tas(2500), 1400, (0xAF43, 13), &[0xAF69], 2, |r| {
            let dy = player_dy(&r.state.work, &r.state.effects.player);
            let [hi, lo] = dy.unsigned_abs().to_be_bytes();
            r.writes = vec![
                (0x20, lo),
                (0x21, hi),
                (0x22, if dy < 0 { 0x80 } else { 0 }),
            ];
        });
    }

    #[test]
    fn map_page_matches_a0a9() {
        check(Scene::tas(2860), 20, (0xA0A9, 13), &[0xA0CD], 1, |r| {
            let (base, page) = r.in_slot(|_, work, cx| map_page(work, cx));
            r.writes = vec![(0x20, base), (0x21, page)];
        });
    }

    #[test]
    fn is_page_column_matches_aec6() {
        check(
            Scene::tas(2860),
            20,
            (0xAEC6, 13),
            &[0xAED0, 0xAEDF, 0xAEE2],
            1,
            |r| {
                let row = r.begin.ram[0x30];
                let found = r.in_slot(|_, _, cx| is_page_column(row, cx));
                r.a = Some(if found { 0x80 } else { 0 });
            },
        );
    }

    #[test]
    fn player_dx_matches_abdf() {
        routine(0xABDF, &[0xAC05], |r| {
            let dx = player_dx(&r.state.work, &r.state.effects.player);
            let [hi, lo] = dx.unsigned_abs().to_be_bytes();
            r.writes = vec![
                (0x20, lo),
                (0x21, hi),
                (0x22, if dx < 0 { 0x80 } else { 0 }),
            ];
        });
    }

    fn world() -> World {
        World::at(1000 * 16)
    }

    #[test]
    fn on_screen_classifies_both_margins() {
        let at = |x: u16| {
            let mut work = Work {
                x: x * 16,
                y: 100 * 16,
                ..Work::default()
            };
            let seen = on_screen(&mut work, &world());
            (seen, work.screen_x, work.screen_y)
        };
        assert_eq!(at(1100), (Visibility::On, 100, 99));
        assert_eq!(at(999).0, Visibility::Near);
        assert_eq!(at(800).0, Visibility::Far);
        assert_eq!(at(1300).0, Visibility::Near);
        assert_eq!(at(1700).0, Visibility::Far);
    }

    #[test]
    fn end_frame_matches_a8e4() {
        let run = |vx: i16, vy: i16, flags: u8, attr: u8| {
            let mut body = Body {
                flags,
                vx,
                ..Body::default()
            };
            let mut work = Work {
                attr,
                vy,
                ..Work::default()
            };
            end_frame(&mut body, &mut work);
            (body.vx, body.vy, body.flags)
        };
        assert_eq!(run(0x0105, 0, 0, 0).0, 0);
        assert_eq!(run(0x0110, 0, 0, 0).0, 0x0110);
        assert_eq!(run(-3, 0, 0, 0).0, 0);
        assert_eq!(run(0, 0x7D, 0, 0).1, 0x7F);
        assert_eq!(run(0, -0x85, 0, 0).1, -0x7F);
        assert_eq!(run(0, 0, 0xFF, 0x20).2, 0x23);
    }
}
