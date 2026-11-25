//! Nametable tile-block queueing (`$A0CE`) and map metatile patching (`$AE0A`).

use super::engine::low_byte;
use super::world::{Cx, Level, World, orientation};
use crate::map::Cell;
use crate::render::ppu;

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
        if ppu::in_attribute_table(addr) {
            if world.orientation & orientation::VERTICAL != 0 {
                addr = ppu::wrap_to_top_rows(addr);
                if let Some(attributes) = attributes.as_mut() {
                    attributes.rows = attributes.rows.wrapping_add(1);
                }
            } else {
                let [hi, lo] = addr.to_be_bytes();
                let hi = if world.orientation & orientation::TWO_PAGES != 0 {
                    0x28
                } else {
                    hi
                };
                // Adds to the low byte only, as the ROM does.
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
    Some(if world.orientation & orientation::VERTICAL != 0 {
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
        if addr >= ppu::attribute_addr(0, 0, 0) {
            addr = ppu::skip_attributes(addr);
        }
        let far_page = block.column & 0x10 != 0;
        if world.orientation & orientation::TWO_PAGES != 0 && far_page {
            addr |= 0x0400;
        }
        addr = addr.wrapping_add(u16::from(block.column & 0x0F) << 1);
        if orientation::vertical_two_page(world.orientation) && far_page {
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
        let edge_hi = edge.to_be_bytes()[0];
        let right = if edge_hi >= 0x10 {
            let overhang = u8::try_from(super::scroll::tile_column(edge)).expect("< 32");
            width.wrapping_sub(overhang)
        } else {
            width
        };
        let mut addr = (u16::from(block.row & 0x0F) << 6)
            + (u16::from(column & 0x0F) << 1)
            + u16::from(left & 1);
        if world.orientation & orientation::TWO_PAGES == 0 {
            addr += ppu::nametable(0);
        } else if block.row & 0x10 != 0 {
            addr += ppu::nametable(1);
        } else {
            addr = addr.wrapping_sub(0x20).wrapping_add(ppu::nametable(0));
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
        let low_table = if orientation::horizontal_two_page(orientation) {
            hi & 0x0C != 0
        } else {
            hi & 0x0C == 0
        };
        let mirror = 0x7500 + u16::from(quad) + if low_table { 0x30 } else { 0x70 };
        let span = right.wrapping_sub(left);
        Attributes {
            addr: u16::from_be_bytes([hi | 3, quad | 0xC0]),
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
    let [_, lo] = addr.to_be_bytes();
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
    } else if ppu::in_bottom_band(addr) {
        row_start - 0x38
    } else {
        row_start
    };
}

/// Writes `metatiles` (rows of `width`) into the map from address `cell` on (`$AE0A`,
/// without its attribute pass `$AE2F`).
pub fn copy_metatiles(cell: u16, width: u8, metatiles: &[u8], level: &mut Level) {
    for (row, ids) in (0u16..).zip(metatiles.chunks(usize::from(width))) {
        for (column, &id) in (0u16..).zip(ids) {
            level.map[Cell::from_address(cell.wrapping_add(row * 0x100 + column))] = id;
        }
    }
}
