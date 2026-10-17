//! The bank-13 routines handlers share, each called where the 6502 calls it.

use super::oam::{DrawCall, Metasprite};
use super::slots::Body;
use super::world::{Cx, Effects, Level, Motion, Player, Schedule, Weapon, World, pose};
use crate::actors::{CELL_PENDING, STATE_DROPS_PICKUP, bank_slice};

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
    /// Bit 6: drawn bottom-anchored (`$9A`).
    pub anchor: u8,
    /// This frame's x step (`$9B:$9C`).
    pub step_x: i16,
    /// The y velocity before gravity (`$9D:$9E`).
    pub vy: i16,
    /// The probe's result flags (`$9F`), cleared on load: bit 6 landed on a floor,
    /// bit 7 the metatile dealt damage.
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
    /// The 6502 overflow flag as the last instruction that sets it left it; weapon 5's
    /// charge (`$9790`) reads it from the routines that ran before. A `BIT $F7` always
    /// clears it: `$F7` bit 6 is never set (only the reset and the NMI's `EOR #$80`
    /// write `$F7`).
    pub cpu_overflow: bool,
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

const MARGIN: i32 = 128 * 16;
const BOTTOM_BAND: i32 = 80 * 16;
const FAR_BELOW: i32 = 384 * 16;

/// A byte of PRG `bank` at a CPU address.
pub(crate) fn rom_byte(prg: &[u8], bank: u8, addr: u16) -> u8 {
    bank_slice(prg, bank, addr & 0x1FFF).expect("address inside PRG")[0]
}

/// A little-endian word of PRG `bank` at a CPU address.
pub(crate) fn rom_word(prg: &[u8], bank: u8, addr: u16) -> u16 {
    u16::from_le_bytes([rom_byte(prg, bank, addr), rom_byte(prg, bank, addr + 1)])
}

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
    let full = world.orientation & 0xC0;
    let camera_x = i32::from(world.camera_x.wrapping_add(8) & 0xFFF0);
    let dx = i32::from(work.x) - camera_x;
    if dx < 0 {
        return if full == 0xC0 {
            Visibility::Near
        } else {
            left_or_above(dx)
        };
    }
    if dx >= world.view.width {
        return if full == 0xC0 {
            Visibility::Near
        } else {
            right_or_below(dx, world.view.width + MARGIN)
        };
    }
    work.screen_x = low_byte(dx >> 4);
    let dy = i32::from(work.y) - i32::from(world.camera_y.wrapping_add(8));
    if dy < 0 {
        return if full == 0x80 {
            Visibility::Near
        } else {
            left_or_above(dy)
        };
    }
    if dy >= world.view.height + BOTTOM_BAND {
        return if full == 0x80 {
            Visibility::Near
        } else {
            right_or_below(dy, FAR_BELOW)
        };
    }
    let row = if dy >= world.view.height {
        work.anchor = 0x40;
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

fn low_byte(value: i32) -> u8 {
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
    (sign >> 1) ^ 0x40
}

/// The map row base and page index under the working position (`$A0A9`: `$20`, `$21`).
#[must_use]
pub fn map_page(work: &Work, cx: &Cx<'_>) -> (u8, u8) {
    let [x_hi, _] = work.x.to_be_bytes();
    let [y_hi, _] = work.y.to_be_bytes();
    let (outer, inner) = if cx.world.orientation & 0x40 == 0 {
        (x_hi, y_hi)
    } else {
        (y_hi, x_hi)
    };
    let page = (inner & 0x10) | (outer >> 4);
    (cx.schedule.rows[usize::from(page)], page)
}

/// On a horizontal-then-vertical level (`$DD` bits 7-6 = `10`), is `row` one of the
/// first eight page bases (`$AEC6`, `row` in `$30`)?
#[must_use]
pub fn is_page_row(row: u8, cx: &Cx<'_>) -> bool {
    cx.world.orientation & 0xC0 == 0x80 && cx.schedule.rows[..8].contains(&row)
}

/// A block of nametable tiles to queue at a map cell, with its metatiles' attribute fields.
#[derive(Debug, Clone, Copy)]
pub struct TileBlock<'a> {
    /// Metatile column (`$24`).
    pub column: u8,
    /// Metatile row, page-relative in bits 0-3 (`$25`).
    pub row: u8,
    /// Width in tiles (`$20`).
    pub width: u8,
    /// Height in tiles (`$21`).
    pub height: u8,
    /// The tiles, row by row (`$22`).
    pub tiles: &'a [u8],
    /// Each metatile's 2-bit palette, row by row (`$26`); `None` leaves the attributes (`$28` bit 7).
    pub fields: Option<&'a [u8]>,
}

/// The masks that clear one 2-bit palette field of an attribute byte.
pub const FIELD_MASKS: [u8; 4] = [0xFC, 0xF3, 0xCF, 0x3F];

struct Attributes {
    addr: u16,
    count: u8,
    rows: u8,
    start: u16,
    mirror: u16,
    field: usize,
}

/// Queues the visible part of a tile block and its attribute bytes (`$A0CE`).
pub fn queue_block(block: &TileBlock<'_>, cx: &mut Cx<'_>) {
    let world = cx.world;
    let vram = &mut cx.effects.vram;
    let width = block.width;
    let Some((mut addr, first, left, right)) = place(block, world, vram.origin) else {
        return;
    };
    let mut attributes = block
        .fields
        .map(|_| Attributes::new(addr, world.orientation, (first, left, right), width));

    let count = usize::from(right.wrapping_sub(left));
    let mut tile = usize::from(left);
    let mut row = first;
    for _ in first..block.height {
        vram.push(addr, 0, &block.tiles[tile..tile + count]);
        if let (Some(fields), Some(attributes)) = (block.fields, attributes.as_mut())
            && row & 1 == 0
        {
            store_fields(fields, attributes, addr, width, &mut vram.attributes);
        }
        addr = addr.wrapping_add(0x20);
        let [hi, lo] = addr.to_be_bytes();
        if hi & 0x23 == 0x23 && lo >= 0xC0 {
            if world.orientation & 0x40 != 0 {
                addr = u16::from_be_bytes([hi & 0x24, lo.wrapping_add(0x40)]);
                if let Some(attributes) = attributes.as_mut() {
                    attributes.rows = attributes.rows.wrapping_add(1);
                }
            } else {
                let hi = if world.orientation & 0x80 != 0 {
                    0x28
                } else {
                    hi
                };
                addr = u16::from_be_bytes([hi, lo.wrapping_add(0x20)]);
            }
        }
        row = row.wrapping_add(1);
        tile += usize::from(width);
    }

    let Some(mut attributes) = attributes else {
        return;
    };
    if attributes.count == 0 || attributes.rows == 0 {
        return;
    }
    let mirror = |at: u16| usize::from(at - 0x7530);
    for left in (0..attributes.rows.div_ceil(2)).rev() {
        let at = mirror(attributes.start);
        let bytes = vram.attributes[at..at + usize::from(attributes.count)].to_vec();
        vram.push(attributes.addr, 0, &bytes);
        if left == 0 {
            break;
        }
        attributes.start += 8;
        attributes.addr = attributes.addr.wrapping_add(8);
        if (attributes.addr >> 8) & 3 != 3 {
            attributes.addr = attributes.addr.wrapping_sub(0x40);
            attributes.start -= 0x40;
        }
    }
}

fn place(block: &TileBlock<'_>, world: &World, origin: u16) -> Option<(u16, u8, u8, u8)> {
    let camera_x = world.camera_x & 0xFF80;
    let camera_y = world.camera_y & 0xFF80;
    let width = block.width;
    Some(if world.orientation & 0x40 != 0 {
        let delta = u16::from_be_bytes([block.row, 0]).wrapping_sub(camera_y);
        let (addr, first) = if delta & 0x8000 != 0 {
            let first = low_byte(i32::from(delta.wrapping_neg() >> 7));
            if first >= block.height {
                return None;
            }
            (0, first)
        } else {
            if delta >> 8 >= 0x0F {
                return None;
            }
            (delta >> 2, 0)
        };
        let mut addr = addr.wrapping_add(origin);
        let [hi, lo] = addr.to_be_bytes();
        if hi >= 0x24 || (hi == 0x23 && lo >= 0xC0) {
            let (lo, carry) = lo.overflowing_add(0x40);
            addr = u16::from_be_bytes([hi.wrapping_add(u8::from(carry)) & 0x23, lo]);
        }
        let far_page = block.column & 0x10 != 0;
        if world.orientation & 0x80 != 0 && far_page {
            addr |= 0x0400;
        }
        addr = addr.wrapping_add(u16::from(block.column & 0x0F) << 1);
        if world.orientation & 0xC0 == 0xC0 && far_page {
            addr |= 0x0400;
        }
        (addr, first, 0, width)
    } else {
        let mut column = block.column;
        let delta = u16::from_be_bytes([column, 0]).wrapping_sub(camera_x);
        let left = if delta & 0x8000 != 0 {
            let hidden = low_byte(i32::from(delta.wrapping_neg() >> 7));
            column = camera_x.to_be_bytes()[0];
            let shown = width.wrapping_sub(hidden);
            if shown == 0 || shown & 0x80 != 0 {
                return None;
            }
            hidden
        } else {
            if delta >> 8 >= 0x10 {
                return None;
            }
            0
        };
        let edge = u16::from_be_bytes([column, 0])
            .wrapping_add(u16::from(width) << 7)
            .wrapping_sub(camera_x);
        let [edge_hi, edge_lo] = edge.to_be_bytes();
        let right = if edge_hi >= 0x10 {
            width.wrapping_sub(((edge_hi & 0x0F) << 1) | (edge_lo >> 7))
        } else {
            width
        };
        let mut addr = (u16::from(block.row & 0x0F) << 6)
            + (u16::from(column & 0x0F) << 1)
            + u16::from(left & 1);
        if world.orientation & 0x80 == 0 {
            addr += 0x2000;
        } else if block.row & 0x10 != 0 {
            addr += 0x2400;
        } else {
            addr = addr.wrapping_sub(0x20).wrapping_add(0x2000);
        }
        (addr, 0, left, right)
    })
}

impl Attributes {
    fn new(addr: u16, orientation: u8, (first, left, right): (u8, u8, u8), width: u8) -> Self {
        let [hi, lo] = addr.to_be_bytes();
        let mut quad = (lo >> 4) | (hi << 4);
        if quad & 2 != 0 {
            quad = quad.wrapping_add(2);
        }
        let column = if lo & 3 == 3 { lo.wrapping_add(1) } else { lo };
        let quad = (quad & 0x38) | ((column >> 2) & 7);
        let low_table = if orientation & 0xC0 == 0x80 {
            hi & 0x0C != 0
        } else {
            hi & 0x0C == 0
        };
        let mirror = 0x7500 + u16::from(quad) + if low_table { 0x30 } else { 0x70 };
        let span = right.wrapping_sub(left);
        Attributes {
            addr: u16::from_be_bytes([hi | 3, quad.wrapping_add(0xC0)]),
            count: if lo & 1 != 0 {
                span >> 1
            } else {
                (span >> 1) + (span & 1)
            },
            rows: u8::from(lo & 0x40 != 0),
            start: mirror,
            mirror,
            field: usize::from(first >> 1) * usize::from(width >> 1)
                + usize::from(left.div_ceil(2)),
        }
    }
}

fn store_fields(
    fields: &[u8],
    attributes: &mut Attributes,
    addr: u16,
    width: u8,
    mirror: &mut [u8; 128],
) {
    let [hi, lo] = addr.to_be_bytes();
    let bottom = lo & 0x40 != 0;
    let row_start = attributes.mirror;
    let mut at = usize::from(row_start - 0x7530);
    let field = |n: u8| fields[attributes.field + usize::from(n)];
    let mut set = |at: usize, slot: usize, value: u8| {
        mirror[at] = (mirror[at] & FIELD_MASKS[slot]) | value;
    };
    let value = |n: u8, slot: usize| match slot {
        0 => field(n),
        1 => field(n) << 2,
        2 => (field(n) & 0x0F) << 4,
        _ => ((field(n) << 2) & 0x0F) << 4,
    };
    let (first, second) = if bottom { (2, 3) } else { (0, 1) };
    let mut n = 0u8;
    'row: {
        if ((lo >> 1) + (lo & 1)) & 1 != 0 {
            let slot = if bottom { 3 } else { 1 };
            set(at, slot, field(0) << (2 * slot));
            at += 1;
            n = 1;
            if n == attributes.count {
                break 'row;
            }
        }
        loop {
            set(at, first, value(n, first));
            n = n.wrapping_add(1);
            if n == attributes.count {
                break;
            }
            set(at, second, value(n, second));
            n = n.wrapping_add(1);
            at += 1;
            if n == attributes.count {
                break;
            }
        }
    }
    attributes.field += usize::from(width >> 1);
    attributes.rows = attributes.rows.wrapping_add(1);
    attributes.mirror = if bottom {
        row_start + 8
    } else if hi & 0x23 == 0x23 && lo >= 0x80 {
        row_start - 0x38
    } else {
        row_start
    };
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
pub fn draw(work: &mut Work, bank: u8, cx: &mut Cx<'_>) {
    let sprite = Metasprite::from_rom(cx.prg, bank, work.pointer).expect("metasprite");
    draw_metasprite(work, &sprite, cx);
}

/// Draws `sprite` where [`Work`] places it (`$C247`).
pub fn draw_metasprite(work: &mut Work, sprite: &Metasprite, cx: &mut Cx<'_>) {
    let call = DrawCall {
        x: i16::from(work.screen_x),
        y: i16::from(work.screen_y),
        attr: work.attr,
        tile: work.tile,
        chr_slot: work.chr_slot,
        bottom: work.anchor & 0x40 != 0,
    };
    work.cpu_overflow = cx.oam.draw(sprite, call, cx.world.frame_odd);
}

fn overlaps(work: &Work, player: &Player, x_table: u16, y_table: u16, prg: &[u8]) -> bool {
    let hitbox = u16::from(work.hitbox);
    let near_y = if work.screen_y < player.screen_y {
        player.screen_y - work.screen_y < player.reach_y
    } else {
        work.screen_y - player.screen_y < rom_byte(prg, 13, y_table + hitbox)
    };
    work.screen_x.abs_diff(player.screen_x) < rom_byte(prg, 13, x_table + hitbox) && near_y
}

/// Contact with the player (`$A9C4`): an item is collected, the actor or the player hurt.
pub fn contact(kind: &mut u8, body: &mut Body, work: &mut Work, damage: u8, cx: &mut Cx<'_>) {
    let gates = cx.effects.player.flags;
    let checks_player = if body.contact & 0x40 == 0 {
        gates[1] & 0x80 != 0 || (gates[0] & 0x80 == 0 && gates[2] & 0x40 == 0)
    } else {
        body.contact & 0x01 != 0
    };
    if checks_player && overlaps(work, &cx.effects.player, 0xABB2, 0xABC1, cx.prg) {
        if body.contact & 0x40 != 0 {
            collect(kind, body, cx);
            return;
        }
        if gates[1] & 0x80 != 0 && body.contact & 0x10 != 0 {
            hurt_actor(body, work, cx);
            return;
        }
        if gates[1] & 0x80 == 0 || (gates[0] & 0x80 == 0 && gates[2] & 0x40 == 0) {
            hurt_player(cx.effects);
            let player = &mut cx.effects.player;
            player.energy = player.energy.wrapping_sub(damage);
        }
    }
    projectile_hits(body, work, cx);
}

/// Checks the player's projectiles against the actor: shields deflect, others hurt (`$AA57`).
pub fn projectile_hits(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    let hitbox = u16::from(work.hitbox);
    let [seed, noise] = cx.world.rng;
    for index in (0..16).rev() {
        let projectile = cx.effects.projectiles[index];
        if projectile.kind == 0 || projectile.flags & 0x80 != 0 {
            continue;
        }
        if work.screen_x.abs_diff(projectile.screen_x) >= rom_byte(cx.prg, 13, 0xABD0 + hitbox) {
            continue;
        }
        let near_y = if work.screen_y < projectile.screen_y {
            projectile.screen_y - work.screen_y < 5
        } else {
            work.screen_y - projectile.screen_y < rom_byte(cx.prg, 13, 0xABC1 + hitbox)
        };
        if !near_y {
            continue;
        }
        let projectile = &mut cx.effects.projectiles[index];
        if body.contact & 0x80 != 0 {
            if projectile.flags & 0x20 != 0 {
                continue;
            }
            cx.effects.sound[0] = 0x3C;
            projectile.flags |= 0xE0;
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
            projectile.state |= 0x40;
            projectile.timer = 0x20;
            projectile.vy = i16::from_be_bytes([0xFF, !(seed & 0x7F)]);
            continue;
        }
        cx.effects.sound[0] = 0x38;
        spend(projectile, seed);
        let (health, killed) = body.health.overflowing_sub(u16::from(projectile.damage));
        body.health = health;
        if killed {
            die(body, work, cx);
        } else if body.hit & 0x40 == 0 {
            body.hit = 0x42;
        }
        return;
    }
}

/// A projectile that hit bounces off and is spent (`$B108`).
fn spend(projectile: &mut super::world::Projectile, rng_f9: u8) {
    if projectile.flags & 0x40 != 0 {
        return;
    }
    projectile.flags |= 0x80;
    let random = if rng_f9 & 0x80 == 0 { 0x20 } else { -0x20 };
    if projectile.timer & 0x80 == 0 {
        projectile.vx = if projectile.vx < 0 { 0x20 } else { -0x20 };
        projectile.vy = random;
    } else {
        projectile.vy = 0x20;
        projectile.vx = random;
    }
    projectile.state = 0x80;
    projectile.attr = 0;
    projectile.param = 0;
    projectile.timer = 4;
}

/// The player's attack lands: 8 off the actor's health, then a flash or death (`$AA2C`).
fn hurt_actor(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    cx.effects.sound[0] = 0x38;
    let (health, killed) = body.health.overflowing_sub(8);
    body.health = health;
    if killed {
        die(body, work, cx);
    } else if body.hit & 0x40 == 0 {
        body.hit = 0x42;
    }
}

/// The actor is killed: sound `$21`, 76 px lower, the dying animation starts (`$AB13`).
fn die(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    cx.effects.sound[0] = 0x21;
    work.y = work.y.wrapping_add(0x04C0);
    body.hit = 0x80;
    body.dying_timer = 0;
}

/// Writes `metatiles` (rows of `width`) into the map from address `cell` on (`$AE0A`,
/// without its attribute pass `$AE2F`).
pub fn copy_metatiles(cell: u16, width: u8, metatiles: &[u8], level: &mut Level) {
    for (row, ids) in (0u16..).zip(metatiles.chunks(usize::from(width))) {
        for (column, &id) in (0u16..).zip(ids) {
            let at = cell.wrapping_add(row * 0x100 + column) - 0x6000;
            level.map[usize::from(at)] = id;
        }
    }
}

/// A boss's hit flash or dying count (`$AF97`): 0 alive, else bit 7 with bit 6 on odd frames.
pub fn boss_flash(body: &mut Body, work: &mut Work, world: &World) -> u8 {
    if body.hit & 0x80 != 0 {
        body.dying_timer = body.dying_timer.wrapping_add(1);
        if body.dying_timer >= 0x40 {
            body.dying_timer = 0x80;
            return 0x80;
        }
        return if world.frame_odd { 0xC0 } else { 0x80 };
    }
    if body.hit & 0x40 == 0 {
        work.attr = body.flags & 0xFC;
    } else {
        body.hit = (body.hit & 0x7F).wrapping_sub(1);
        work.attr = body.flags | 0x03;
    }
    0
}

/// A boss against the player and his projectiles (`$AFD6`): `$80` when it hurt him in
/// `mode` `$80`, `$40`/`$42` when it was hit, else 0; touching him drains `amount` from `$B2`.
pub fn boss_contact(amount: u16, mode: u8, body: &mut Body, work: &Work, cx: &mut Cx<'_>) -> u8 {
    if mode & 0x40 == 0 {
        let gates = cx.effects.player.flags;
        let attacking = gates[1] & 0x80 != 0;
        let guarded = gates[2] & 0x40 != 0;
        if (attacking || !guarded) && overlaps(work, &cx.effects.player, 0xB0F9, 0xB0FE, cx.prg) {
            if attacking && cx.effects.boss_hp == 0 {
                return 0;
            }
            if attacking && mode != 0x80 && body.contact & 0x10 != 0 {
                cx.effects.sound[0] = 0x22;
                return boss_damaged(8, body, cx);
            }
            if !attacking || !guarded {
                hurt_player(cx.effects);
                let pool = u16::from_le_bytes(cx.effects.boss_pool).wrapping_sub(amount);
                cx.effects.boss_pool = pool.to_le_bytes();
                return mode & 0x80;
            }
        }
        if mode & 0x80 != 0 {
            return 0;
        }
    }
    let hitbox = u16::from(work.hitbox);
    for index in (0..16).rev() {
        let projectile = &mut cx.effects.projectiles[index];
        if projectile.kind == 0 || projectile.flags & 0x80 != 0 {
            continue;
        }
        if work.screen_x.abs_diff(projectile.screen_x) >= rom_byte(cx.prg, 13, 0xB103 + hitbox) {
            continue;
        }
        let near_y = if work.screen_y < projectile.screen_y {
            projectile.screen_y - work.screen_y < 3
        } else {
            work.screen_y - projectile.screen_y < rom_byte(cx.prg, 13, 0xB0FE + hitbox)
        };
        if !near_y {
            continue;
        }
        spend(projectile, cx.world.rng[0]);
        let damage = projectile.damage << 4;
        return boss_damaged(damage, body, cx);
    }
    0
}

fn boss_damaged(damage: u8, body: &mut Body, cx: &mut Cx<'_>) -> u8 {
    let (hp, killed) = cx.effects.boss_hp.overflowing_sub(u16::from(damage));
    cx.effects.boss_hp = hp;
    if killed || hp == 0 {
        cx.effects.sound[1] = 0x23;
        body.hit = 0x80;
        body.dying_timer = 0;
        cx.effects.boss_hp = 0;
        0x40
    } else if body.hit & 0x40 != 0 {
        0x40
    } else {
        body.hit = 0x42;
        0x42
    }
}

/// The boss is beaten: music `$FC`, `$AF` set and a type-`$50` slot with `timer` (`$B1C4`).
pub fn boss_defeated(timer: u8, cx: &mut Cx<'_>) {
    cx.effects.busy = 0x80;
    cx.effects.music = 0xFC;
    // ponytail: no free slot spawns nothing; the game writes past the slot arrays then
    let Some(slot) = cx.slots.main[..16]
        .iter_mut()
        .rev()
        .find(|slot| slot.kind == 0)
    else {
        return;
    };
    slot.body.counter = 0;
    slot.body.anim = 1;
    slot.body.timer = timer;
    slot.kind = 0x50;
    slot.brain = super::families::Brain::new(0x50);
}

/// Queues the status bar's two 5-digit counters: `value` and the boss's hit points (`$D1F5`).
pub fn queue_counters(value: u16, cx: &mut Cx<'_>) {
    for (number, column) in [(value, 0x48), (cx.effects.boss_hp, 0x52)] {
        let mut top = [0x0A; 6];
        let mut bottom = [0x0B; 6];
        let mut rest = number;
        for (i, place) in [10000, 1000, 100, 10, 1].into_iter().enumerate() {
            let digit = rest / place;
            rest %= place;
            top[i] = rom_byte(cx.prg, 14, 0xD26A + digit);
            bottom[i] = top[i] | 1;
        }
        cx.effects.vram.push(0x2300 | column, 0, &top);
        cx.effects.vram.push(0x2320 | column, 0, &bottom);
    }
}

/// Contact that only hurts the player, with no weapon scan (`$AEE3`); true when it hit.
pub fn touch(work: &Work, damage: u8, cx: &mut Cx<'_>) -> bool {
    let gates = cx.effects.player.flags;
    if gates[0] & 0x80 != 0
        || gates[2] & 0x40 != 0
        || !overlaps(work, &cx.effects.player, 0xAF2A, 0xAF31, cx.prg)
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
    effects.sound[0] = 0x25;
    effects.attack &= 0x80;
    let player = &mut effects.player;
    player.pose &= !pose::SHOT;
    player.hurt_timer = 0x20;
    player.flags[2] = 0xC0;
    if player.motion == Motion::Falling {
        player.motion = Motion::Jumping;
    }
    if player.motion.on_foot() || player.motion == Motion::AimingUp {
        player.vx = if player.facing & 0x40 == 0 {
            -0x20
        } else {
            0x20
        };
    }
}

/// Spawns type `$08`'s four parts around the working position (`$AC16`); the slot used.
pub fn spawn_parts(variant: u8, work: &Work, cx: &mut Cx<'_>) -> Option<usize> {
    let index = (0..16).rev().find(|&slot| cx.slots.main[slot].kind == 0)?;
    let groups = rom_word(cx.prg, 13, 0xADD8);
    let parts = rom_word(cx.prg, 13, 0xACC6 + 2 * u16::from(variant));
    let slot = &mut cx.slots.main[index];
    let [x_hi, x_lo] = work.x.to_be_bytes();
    let [y_hi, y_lo] = work.y.to_be_bytes();
    for part in 0..4u16 {
        let data = rom_word(cx.prg, 13, parts + 2 * part);
        let byte = |i: u16| rom_byte(cx.prg, 13, data + i);
        let array = |i: u16| rom_word(cx.prg, 13, groups + 12 * part + 2 * i);
        let (x_lo, carry) = x_lo.overflowing_add(byte(0));
        let x_hi = x_hi.wrapping_add(byte(1)).wrapping_add(u8::from(carry));
        let (y_lo, carry) = y_lo.overflowing_add(byte(2));
        let y_hi = y_hi.wrapping_add(byte(3)).wrapping_add(u8::from(carry));
        for (i, value) in [x_lo, x_hi, y_lo, y_hi, byte(4), byte(5)]
            .into_iter()
            .enumerate()
        {
            slot.body
                .set_byte(array(u16::try_from(i).expect("< 6")), value);
        }
    }
    slot.body.flags = rom_byte(cx.prg, 13, 0xACBD + u16::from(variant));
    slot.kind = 0x08;
    slot.brain = super::families::Brain::new(0x08);
    slot.body.set_byte(0x7ED0, 0x20);
    slot.body.contact = 0x48;
    Some(index)
}

/// The level end (bank 14 `$D274`), on the player's death or a boss's: the player
/// freezes in motion 7 with `$047A` = `$C0` and `$0481` = `$80`, `$B8`, `$0480`
/// and `$0465` clear, and every projectile is freed.
pub fn level_end(effects: &mut Effects) {
    let player = &mut effects.player;
    player.motion = Motion::Dying;
    player.facing |= 3;
    player.flags[2] = 0xC0;
    player.stage_script = 0x80;
    player.death_timer = 0;
    player.death_flags = 0;
    effects.attack = 0;
    for projectile in &mut effects.projectiles {
        projectile.kind = 0;
    }
}

/// Frees the running slot (`$7CC0,x` = 0) where the rest of the loop can see it,
/// so a spawn later in the same handler may reuse it; the actor loop then keeps
/// what the slot holds instead of the handler's copy.
pub fn free_own_slot(kind: &mut u8, body: &Body, work: &Work, cx: &mut Cx<'_>) {
    *kind = 0;
    let own = &mut cx.slots.main[usize::from(work.slot)];
    own.kind = 0;
    own.body = body.clone();
}

/// Frees the slot and ORs `rearm` into its schedule cell (`$CD50`).
pub fn despawn(kind: &mut u8, body: &Body, rearm: u8, schedule: &mut Schedule) {
    schedule.cells[usize::from(body.cell)] |= rearm;
    *kind = 0;
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
    let player = &cx.effects.player;
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
    if low_byte(i32::from(dx >> 4)) >= rom_byte(cx.prg, 13, 0xA0A7 + u16::from(work.hitbox)) {
        return Proximity::Clear;
    }
    let distance = work.y.abs_diff(player.y);
    if distance >= 0x1000 {
        return class(distance);
    }
    let reach = if work.y < player.y {
        player.reach_y
    } else {
        rom_byte(cx.prg, 13, 0xA0A8)
    };
    if low_byte(i32::from(distance >> 4)) >= reach {
        Proximity::Clear
    } else {
        Proximity::Touching
    }
}

/// The hit flash, or dying and then freed or turned into item drop `$10` (`$A56E`).
pub fn dying(
    kind: &mut u8,
    body: &mut Body,
    work: &mut Work,
    variant: u8,
    cx: &mut Cx<'_>,
) -> bool {
    if body.hit & 0x80 == 0 {
        if body.hit & 0x40 == 0 {
            work.attr = body.flags & 0xFC;
        } else {
            body.hit = (body.hit & 0x7F).wrapping_sub(1);
            work.attr = body.flags | 0x03;
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
    body.dying_timer = body.dying_timer.wrapping_add(1);
    if body.dying_timer < limit {
        return true;
    }
    if body.state & STATE_DROPS_PICKUP == 0 {
        *kind = 0;
        return true;
    }
    *kind = 0x10;
    body.phase = 0;
    body.flags = 0;
    body.vx_drift = 0;
    body.vx = 0;
    body.ref_vx = 0;
    body.vy = -0x40;
    body.contact = 0x49;
    true
}

/// The tile bits for the CHR slot in [`Work::chr_slot`] (`$AC06`).
pub fn tile_bits(work: &mut Work, prg: &[u8]) {
    work.tile = rom_byte(prg, 13, 0xAC0E + u16::from(work.chr_slot));
}

/// The highest free slot among 16-23, where children go (`$AF38`).
#[must_use]
pub fn free_child_slot(slots: &super::slots::Slots) -> Option<usize> {
    (16..24).rev().find(|&slot| slots.main[slot].kind == 0)
}

/// Picks up an item drop (`$AB39`): the weapon, or meter capsules up to the level end.
pub fn collect(kind: &mut u8, body: &Body, cx: &mut Cx<'_>) {
    let effects = &mut *cx.effects;
    effects.sound[1] = 0x26;
    if effects.player.energy & 0x80 != 0 {
        *kind = 0;
        return;
    }
    let player_flags = effects.player.flags[0];
    match body.state & !STATE_DROPS_PICKUP {
        0..=3 => {
            if player_flags & 0x80 == 0 {
                effects.weapon = Weapon::from_byte((body.state & !STATE_DROPS_PICKUP) + 1)
                    .expect("a pickup gives weapons 1-4");
            }
            effects.attack = 0x80;
            for projectile in &mut effects.projectiles {
                projectile.kind = 0;
            }
        }
        4 => {
            let meter = effects.meter.wrapping_add(2);
            if meter < 0x0E {
                effects.meter = meter;
            } else {
                effects.meter = 0x0E;
                if player_flags & 0x80 == 0 {
                    effects.stored_weapon = effects.weapon;
                    effects.transition = 5;
                    effects.busy = 0x80;
                }
            }
        }
        state => panic!("item state {state} jumps past the $ABA8 table"),
    }
    *kind = 0;
}

/// Stands in for a missing handler: despawns when far off screen, as `$823C` does.
pub fn unported(kind: &mut u8, body: &Body, work: &mut Work, cx: &mut Cx<'_>) {
    load(body, work);
    if on_screen(work, cx.world) == Visibility::Far {
        despawn(kind, body, CELL_PENDING, cx.schedule);
    }
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
    work.attr &= 0xE0;
    body.flags = (body.flags & 0x03) | work.attr;
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
                cpu_overflow: false,
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
    let velocity = body.byte(velocity);
    let (low, carry) = body.byte(lo).overflowing_add(velocity);
    body.set_byte(lo, low);
    let high = body
        .byte(hi)
        .wrapping_add(if velocity & 0x80 != 0 { 0xFF } else { 0 })
        .wrapping_add(u8::from(carry));
    body.set_byte(hi, high);
    u16::from_be_bytes([high, low])
}

#[cfg(test)]
mod tests {
    use super::super::oracle::{Routine, Scene, assert_routine};
    use super::*;

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
            let a = r.in_slot(|slot, work, cx| boss_flash(&mut slot.body, work, cx.world));
            r.a = Some(a);
        });
    }

    #[test]
    fn boss_contact_matches_afd6() {
        check(Scene::tas(6281), 360, (0xAFD6, 13), &AFD6_EXITS, 100, |r| {
            let (a, x, y) = (r.begin.cpu.a, r.begin.cpu.x, r.begin.cpu.y);
            let amount = u16::from_be_bytes([x, a]);
            let a = r.in_slot(|slot, work, cx| boss_contact(amount, y, &mut slot.body, work, cx));
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
    fn is_page_row_matches_aec6() {
        check(
            Scene::tas(2860),
            20,
            (0xAEC6, 13),
            &[0xAED0, 0xAEDF, 0xAEE2],
            1,
            |r| {
                let row = r.begin.ram[0x30];
                let found = r.in_slot(|_, _, cx| is_page_row(row, cx));
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
    fn unported_actors_despawn_far_off_screen() {
        let world = world();
        let mut schedule = Schedule {
            cells: [0; 256],
            rows: [0; 32],
        };
        let (mut slots, mut effects, mut oam) = (
            super::super::slots::Slots::default(),
            super::super::world::Effects::default(),
            super::super::oam::OamBuilder::new(),
        );
        let mut scroll = super::super::scroll::Scroll::default();
        let mut cx = Cx {
            prg: &[],
            world: &world,
            slots: &mut slots,
            schedule: &mut schedule,
            effects: &mut effects,
            scroll: &mut scroll,
            oam: &mut oam,
        };
        let mut work = Work::default();
        for (x, kind) in [(1100, 6), (999, 6), (800, 0)] {
            let body = Body {
                x: x * 16,
                y: 100 * 16,
                cell: 5,
                ..Body::default()
            };
            let mut slot_kind = 6;
            unported(&mut slot_kind, &body, &mut work, &mut cx);
            assert_eq!(slot_kind, kind, "x {x}");
        }
        assert_eq!(schedule.cells[5], CELL_PENDING);
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
