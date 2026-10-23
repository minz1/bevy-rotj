//! The camera: its speed (bank 14 `$D2F3`), its motion, the nametable column and row
//! streaming and the edge spawns (bank 2 `$8528`).

use super::Game;
use super::engine::rom_byte;
use super::spawn::{SpawnTable, lock_at, spawn_at};
use super::world::orientation;
use crate::actors::CELL_PENDING;
use crate::levelmap::Cell;
use crate::render::{Mirroring, ctrl};
use crate::rooms::Raster;

/// The bits of [`Scroll::mode`] (`$C3`); the other bits have no reader.
pub mod mode {
    /// The camera x auto-scrolls instead of chasing the player.
    pub const AUTO_X: u8 = 0x80;
    /// The camera y auto-scrolls instead of chasing the player.
    pub const AUTO_Y: u8 = 0x40;
    /// The auto-scroll runs forward (`+$0A`: right in x, down in y); clear, it runs backward.
    pub const FORWARD: u8 = 0x01;
}

/// Scroll state outside the camera position.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scroll {
    /// The auto-scroll mode (`$C3`), the [`mode`] bits.
    pub mode: u8,
    /// The auto-scroll gate (`$B6`).
    pub gate: u8,
    /// This frame's camera x step, 1/16 px (`$C4:$C5`).
    pub speed_x: i16,
    /// This frame's camera y step (`$C6:$C7`).
    pub speed_y: i16,
    /// Sub-tile x accumulator; bit 7 marks an 8 px step (`$BE`).
    pub step_x: u8,
    /// Sub-tile y accumulator (`$BF`).
    pub step_y: u8,
    /// The camera y in the 240 px nametable, rows of 16 skipping the 16th (`$C8:$C9`).
    pub ring_y: u16,
    /// PPU x scroll (`$FD`).
    pub scroll_x: u8,
    /// PPU y scroll (`$FC`).
    pub scroll_y: u8,
    /// PPU control bits the NMI applies; bit 0 the x nametable (`$FF`).
    pub ppu_ctrl: u8,
    /// Attribute rows the column stream leaves alone, by bit 7 (`$60-$67`).
    pub fixed_quads: [u8; 8],
    /// Metatile rows the column stream leaves alone, by bit 7 (`$70-$7F`).
    pub fixed_rows: [u8; 16],
    /// The nametable arrangement the NMI applies (`$EC`).
    pub mirroring: Mirroring,
    /// The PPU mask the NMI applies (`$FE`): `$18` shows the background and sprites
    /// with the left 8 pixels clipped, `$1E` without the clip, 0 nothing.
    pub mask: u8,
    /// The raster split's header (`$D0-$D4`).
    pub split: Raster,
    /// The split's chain's own bytes (`$D5-$DC`, `bands[0]` is `$D5`).
    pub bands: [u8; 8],
    /// The room's first schedule column (`$0403`).
    pub start: u8,
}

/// Sets the camera speeds toward the player, or the auto-scroll speeds (`$D2F3`).
pub fn speed(game: &mut Game, prg: &[u8]) {
    let scroll = &mut game.scroll;
    let player = &game.effects.player;
    let toward = |position: u16, camera: u16| {
        let [hi, lo] = camera.to_be_bytes();
        let target = u16::from_be_bytes([rom_byte(prg, 14, 0xC836 + u16::from(hi)), lo]);
        position
            .wrapping_sub(target)
            .cast_signed()
            .clamp(-0x80, 0x80)
    };
    scroll.speed_x = if scroll.mode & mode::AUTO_X != 0 {
        auto(scroll.mode, scroll.gate, 0xF5)
    } else {
        toward(player.x, game.world.camera_x)
    };
    scroll.speed_y = if scroll.mode & mode::AUTO_Y != 0 {
        auto(scroll.mode, scroll.gate, 0xFC)
    } else {
        toward(player.y, game.world.camera_y)
    };
}

/// The auto-scroll speed: `$0A` forward, or back by `backward` (`$F5` in x, `$FC` in y).
fn auto(mode: u8, gate: u8, backward: u8) -> i16 {
    if mode & mode::FORWARD != 0 || gate == 0 {
        i16::from(0x0A & gate)
    } else {
        i16::from_be_bytes([gate, backward & gate])
    }
}

/// Moves the camera, streams the entering column or row and spawns at the edge (`$8528`).
pub fn step(game: &mut Game, prg: &[u8]) {
    let across = game.world.orientation & orientation::TWO_PAGES != 0;
    if game.world.orientation & orientation::VERTICAL == 0 {
        horizontal(game, prg);
        if across {
            across_y(game);
        } else {
            game.scroll.speed_y = 0;
        }
    } else {
        vertical(game, prg);
        if across {
            across_x(game);
        } else {
            game.scroll.speed_x = 0;
        }
    }
}

/// The x move across a vertical room's two nametables, camera x 0 to `$1000` (`$85F7`).
fn across_x(game: &mut Game) {
    let scroll = &mut game.scroll;
    let camera = game.world.camera_x;
    let speed = scroll.speed_x;
    if speed < 0 {
        if camera == 0 {
            scroll.speed_x = 0;
            return;
        }
        let moved = camera.wrapping_add(speed.cast_unsigned());
        if moved & 0x8000 == 0 {
            game.world.camera_x = moved;
            scroll.ppu_ctrl &= !ctrl::NAMETABLE_X;
        } else {
            game.world.camera_x = 0;
        }
    } else {
        if camera & 0xF000 != 0 {
            scroll.speed_x = 0;
            return;
        }
        let moved = camera.wrapping_add(u16::from(speed.to_le_bytes()[0]));
        if moved & 0xF000 == 0 {
            game.world.camera_x = moved;
        } else {
            game.world.camera_x = 0x1000;
            scroll.ppu_ctrl |= ctrl::NAMETABLE_X;
        }
    }
    let [column, fraction] = game.world.camera_x.to_be_bytes();
    let (scroll_x, wrapped) =
        ((fraction >> 4) | (column << 4)).overflowing_add(u8::from(fraction & 8 != 0));
    game.scroll.scroll_x = scroll_x;
    if wrapped {
        game.scroll.ppu_ctrl |= ctrl::NAMETABLE_X;
    }
}

/// The y move across a horizontal room's two nametables, ring rows 0 to `$10` (`$86D9`).
fn across_y(game: &mut Game) {
    let scroll = &mut game.scroll;
    let speed = scroll.speed_y;
    let [mut row, fraction] = scroll
        .ring_y
        .wrapping_add(speed.cast_unsigned())
        .to_be_bytes();
    let mut clamp = None;
    if speed < 0 {
        if row & 0x0F == 0x0F {
            row = row.wrapping_sub(1);
            if row & 0x80 != 0 {
                clamp = Some((0, 0x01));
            }
        }
    } else {
        if row & 0x0F == 0x0F {
            row = row.wrapping_add(1);
        }
        if row >= 0x10 {
            clamp = Some((0x10, 0x10));
        }
    }
    scroll.ring_y = u16::from_be_bytes([row, fraction]);
    if let Some((ring, camera)) = clamp {
        scroll.ring_y = u16::from_be_bytes([ring, 0]);
        game.world.camera_y = u16::from_be_bytes([camera, 0]);
        scroll.scroll_y = 0;
        scroll.speed_y = 0;
    }
    let [row, fraction] = scroll.ring_y.to_be_bytes();
    if row & 0xF0 == 0 {
        scroll.ppu_ctrl |= ctrl::NAMETABLE_Y;
    } else {
        scroll.ppu_ctrl &= !ctrl::NAMETABLE_Y;
    }
    game.world.camera_y = game
        .world
        .camera_y
        .wrapping_add(scroll.speed_y.cast_unsigned());
    scroll.scroll_y = (fraction >> 4) | (row << 4);
    if fraction & 8 != 0 {
        scroll.scroll_y = scroll.scroll_y.wrapping_add(1);
        if scroll.scroll_y >= 0xF0 {
            scroll.scroll_y &= 0x0F;
            scroll.ppu_ctrl &= !ctrl::NAMETABLE_Y;
        }
    }
}

fn accumulate(step: &mut u8, speed: i16) -> bool {
    *step = step.wrapping_add(speed.to_le_bytes()[0]);
    let crossed = *step & 0x80 != 0;
    *step &= 0x7F;
    crossed
}

fn edge_spawn(game: &mut Game, prg: &[u8], cell: u8) {
    if game.schedule.cells[usize::from(cell)] & CELL_PENDING == 0 {
        return;
    }
    let table = SpawnTable::import(prg, game.world.area, game.world.room);
    spawn_at(&mut game.slots, &mut game.schedule, &table, cell);
}

fn cell(page_columns: &[u8; 32], position: u8, ahead: bool) -> u8 {
    let page = usize::from(position >> 4) + usize::from(ahead);
    page_columns[page] | (position & 0x0F)
}

fn horizontal(game: &mut Game, prg: &[u8]) {
    let [column, fraction] = game.world.camera_x.to_be_bytes();
    let speed = game.scroll.speed_x;
    let right = speed >= 0;
    if right && game.effects.scroll_lock & 1 != 0 {
        game.scroll.speed_x = 0;
    } else if accumulate(&mut game.scroll.step_x, speed) {
        let (mask, at_edge) = if right {
            (1, fraction & 0x80 != 0)
        } else {
            (2, fraction & 0x80 == 0)
        };
        if at_edge {
            let left = cell(&game.schedule.page_columns, column, false);
            game.effects.scroll_lock =
                lock_at(&game.schedule, left, mask, 0x0C, game.effects.scroll_lock);
            edge_spawn(game, prg, cell(&game.schedule.page_columns, column, right));
            if game.effects.scroll_lock & mask != 0 {
                if right {
                    stream_column(game, true);
                }
                let column = if right {
                    column.wrapping_add(1)
                } else {
                    column
                };
                game.world.camera_x = u16::from_be_bytes([column, 0]);
                game.scroll.step_x = 0;
                set_scroll_x(game);
                return;
            }
        }
        stream_column(game, right);
    }
    game.world.camera_x = game
        .world
        .camera_x
        .wrapping_add(game.scroll.speed_x.cast_unsigned());
    set_scroll_x(game);
}

fn set_scroll_x(game: &mut Game) {
    let [column, fraction] = game.world.camera_x.to_be_bytes();
    let scroll = (fraction >> 4) | (column << 4);
    game.scroll.scroll_x = scroll.wrapping_add(u8::from(fraction & 8 != 0));
}

fn vertical(game: &mut Game, prg: &[u8]) {
    let [row, fraction] = game.world.camera_y.to_be_bytes();
    let speed = game.scroll.speed_y;
    let down = speed >= 0;
    if down && game.effects.scroll_lock & 4 != 0 {
        game.scroll.speed_y = 0;
        return;
    }
    if accumulate(&mut game.scroll.step_y, speed) {
        let (mask, at_edge) = if down {
            (4, fraction & 0x80 != 0)
        } else {
            (8, fraction & 0x80 == 0)
        };
        let mut locked = false;
        if at_edge {
            let top = cell(&game.schedule.page_columns, row, false);
            game.effects.scroll_lock =
                lock_at(&game.schedule, top, mask, 0x03, game.effects.scroll_lock);
            edge_spawn(game, prg, cell(&game.schedule.page_columns, row, down));
            locked = game.effects.scroll_lock & mask != 0;
            if locked {
                game.world.camera_y &= 0xFF00;
                game.scroll.step_y = 0;
                game.scroll.speed_y = 0;
                game.scroll.ring_y &= 0xFF00;
                if down {
                    game.world.camera_y = game.world.camera_y.wrapping_add(0x100);
                    let mut ring = game.scroll.ring_y.to_be_bytes()[0].wrapping_add(1);
                    if ring & 0x0F == 0x0F {
                        ring = ring.wrapping_add(1);
                    }
                    game.scroll.ring_y = u16::from_be_bytes([ring, 0]);
                }
            }
        }
        if !locked || down {
            stream_row(game, down);
            let origin = &mut game.effects.vram.origin;
            if down {
                *origin = origin.wrapping_add(0x20);
                let [hi, lo] = origin.to_be_bytes();
                if lo >= 0xC0 && hi >= 0x23 {
                    *origin = 0x2000;
                }
            } else {
                *origin = origin.wrapping_sub(0x20);
                if *origin < 0x2000 {
                    *origin = 0x23A0;
                }
            }
        }
    }
    move_y(game);
}

fn move_y(game: &mut Game) {
    let speed = game.scroll.speed_y;
    game.world.camera_y = game.world.camera_y.wrapping_add(speed.cast_unsigned());
    let [ring_row, ring_fraction] = game.scroll.ring_y.to_be_bytes();
    let [speed_hi, speed_lo] = speed.to_be_bytes();
    let (fraction, carry) = ring_fraction.overflowing_add(speed_lo);
    let mut row = ring_row
        .wrapping_add(speed_hi)
        .wrapping_add(u8::from(carry));
    if row & 0x0F == 0x0F {
        row = if speed < 0 {
            row.wrapping_sub(1)
        } else {
            row.wrapping_add(1)
        };
    }
    game.scroll.ring_y = u16::from_be_bytes([row, fraction]);
    let mut scroll = (row << 4) | (fraction >> 4);
    if fraction & 8 != 0 {
        scroll = scroll.wrapping_add(1);
        if scroll >= 0xF0 {
            scroll = 0;
        }
    }
    game.scroll.scroll_y = scroll;
}

fn stream_column(game: &mut Game, right: bool) {
    if game.world.orientation & orientation::TWO_PAGES != 0 {
        stream_column_across(game, right);
        return;
    }
    let [camera, fraction] = game.world.camera_x.to_be_bytes();
    let column = if right {
        camera.wrapping_add(0x10)
    } else {
        camera
    };
    let base = cell(&game.schedule.page_columns, column, false);
    let tile_column = u16::from(((camera & 0x0F) << 1) | (fraction >> 7));
    let half = usize::from(fraction >> 7);
    let level = &game.effects.level;
    let tiles = |row: u8| {
        let id = level.map[Cell { row, column: base }];
        let corners = level.tiles(id);
        [corners[half], corners[half + 2]]
    };
    let vram = &mut game.effects.vram;
    let mut push_run = |top: u8, end: u8| {
        let bytes: Vec<u8> = (top..end).flat_map(tiles).collect();
        vram.push(0x2000 + u16::from(top) * 64 + tile_column, 4, &bytes);
    };
    let mut run_end = None;
    for row in (0..15u8).rev() {
        if game.scroll.fixed_rows[usize::from(row)] & 0x80 != 0 {
            if let Some(end) = run_end.take() {
                push_run(row + 1, end);
            }
        } else if run_end.is_none() {
            run_end = Some(row + 1);
        }
    }
    if let Some(end) = run_end {
        push_run(0, end);
    }

    let keep = if camera & 1 == 0 { 0xCC } else { 0x33 };
    let page = usize::from(base >> 4) * 64;
    let quad = usize::from((camera & 0x0F) >> 1);
    let source = &level.attributes[page..page + 64];
    let fixed = &game.scroll.fixed_quads;
    for pair in 0..4 {
        let lower = 0x20 + quad + 8 * pair;
        let upper = lower - 0x20;
        let mirror = &mut vram.attributes;
        let below = merge(&mut mirror[lower], source[lower], keep, fixed[4 + pair]);
        let above = merge(&mut mirror[upper], source[upper], keep, fixed[pair]);
        let at = 0x23C0 + u16::try_from(upper).expect("an attribute index");
        vram.push(at, 4, &[above, below]);
    }
}

/// One nametable half [`stream_column_across`] fills: its map column, the metatile
/// rows drawn, the VRAM base and the attribute shadow offset.
struct AcrossPage {
    base: u8,
    rows: std::ops::Range<u8>,
    table: u16,
    mirror: usize,
}

/// The entering column of a room two nametables tall (`$DD = $80`, bank 2 `$824F`): rows
/// 0-14 of the `$0450` page into `$2000`, rows 1-15 of the `$0440` page into `$2800`,
/// then their attributes, with no fixed rows or quads.
fn stream_column_across(game: &mut Game, right: bool) {
    let [camera, fraction] = game.world.camera_x.to_be_bytes();
    let column = if right {
        camera.wrapping_add(0x10)
    } else {
        camera
    };
    let page = usize::from(column >> 4);
    let page_columns = &game.schedule.page_columns;
    let pages = [
        AcrossPage {
            base: page_columns[page + 16] | (column & 0x0F),
            rows: 0..15,
            table: 0x2000,
            mirror: 0,
        },
        AcrossPage {
            base: page_columns[page] | (column & 0x0F),
            rows: 1..16,
            table: 0x2800,
            mirror: 64,
        },
    ];
    let tile_column = u16::from(((camera & 0x0F) << 1) | (fraction >> 7));
    let half = usize::from(fraction >> 7);
    let level = &game.effects.level;
    let vram = &mut game.effects.vram;
    for entry in &pages {
        let bytes: Vec<u8> = entry
            .rows
            .clone()
            .flat_map(|row| {
                let corners = level.tiles(
                    level.map[Cell {
                        row,
                        column: entry.base,
                    }],
                );
                [corners[half], corners[half + 2]]
            })
            .collect();
        vram.push(entry.table + tile_column, 4, &bytes);
    }
    let keep = if camera & 1 == 0 { 0xCC } else { 0x33 };
    let quad = usize::from((camera & 0x0F) >> 1);
    for entry in &pages {
        let from = usize::from(entry.base >> 4) * 64;
        let source = &level.attributes[from..from + 64];
        for pair in 0..4 {
            let lower = 0x20 + quad + 8 * pair;
            let upper = lower - 0x20;
            let below = merge(
                &mut vram.attributes[entry.mirror + lower],
                source[lower],
                keep,
                0,
            );
            let above = merge(
                &mut vram.attributes[entry.mirror + upper],
                source[upper],
                keep,
                0,
            );
            let at = entry.table + 0x3C0 + u16::try_from(upper).expect("an attribute index");
            vram.push(at, 4, &[above, below]);
        }
    }
}

fn merge(mirror: &mut u8, source: u8, keep: u8, fixed: u8) -> u8 {
    if fixed & 0x80 == 0 {
        *mirror = (*mirror & keep) | (source & !keep);
    }
    *mirror
}

fn stream_row(game: &mut Game, down: bool) {
    let [camera, fraction] = game.world.camera_y.to_be_bytes();
    let row = if down {
        camera.wrapping_add(0x0F)
    } else {
        camera
    };
    let page = usize::from(row >> 4);
    let tables: &[usize] = if game.world.orientation & orientation::TWO_PAGES == 0 {
        &[0]
    } else {
        &[0, 1]
    };
    let bases: Vec<u8> = tables
        .iter()
        .map(|&table| game.schedule.page_columns[page + 16 * table])
        .collect();
    let level = &game.effects.level;
    let half = if fraction & 0x80 != 0 { 2 } else { 0 };
    let vram = &mut game.effects.vram;
    for (&table, &base) in tables.iter().zip(&bases) {
        let bytes: Vec<u8> = (0..16u8)
            .flat_map(|column| {
                let id = level.map[Cell::in_page(row & 0x0F, base, column)];
                let corners = level.tiles(id);
                [corners[half], corners[half + 1]]
            })
            .collect();
        let at = vram.origin | (u16::try_from(table).expect("0 or 1") << 10);
        vram.push(at, 0, &bytes);
    }

    let [hi, lo] = vram.origin.to_be_bytes();
    let quads = usize::from((((hi << 1) | (lo >> 7)) << 3) & 0x38);
    let keep = if lo & 0x40 != 0 { 0x0F } else { 0xF0 };
    let take: u8 = if row & 1 != 0 { 0xF0 } else { 0x0F };
    let swap = take | keep != 0xFF;
    let take = if swap { !take } else { take };
    for (&table, &base) in tables.iter().zip(&bases) {
        let source = usize::from(base >> 4) * 64 + usize::from(row & 0x0E) * 4;
        let mirror = &mut vram.attributes[table * 64..table * 64 + 64];
        let mut bytes = [0u8; 8];
        for (i, byte) in bytes.iter_mut().enumerate().rev() {
            let mut value = level.attributes[source + i];
            if swap {
                value = value.rotate_left(4);
            }
            *byte = (value & take) | (mirror[quads + i] & keep);
            mirror[quads + i] = *byte;
        }
        let at = 0x23C0 | u16::try_from(quads & 0x3F).expect("an attribute row");
        vram.push(
            at | (u16::try_from(table).expect("0 or 1") << 10),
            0,
            &bytes,
        );
    }
}

#[cfg(test)]
mod layout {
    use super::{Mirroring, Raster, Scroll};

    impl Scroll {
        pub(crate) fn lift(ram: &[u8; 2048]) -> Scroll {
            Scroll {
                mode: ram[0xC3],
                gate: ram[0xB6],
                speed_x: i16::from_be_bytes([ram[0xC4], ram[0xC5]]),
                speed_y: i16::from_be_bytes([ram[0xC6], ram[0xC7]]),
                step_x: ram[0xBE],
                step_y: ram[0xBF],
                ring_y: u16::from_be_bytes([ram[0xC8], ram[0xC9]]),
                scroll_x: ram[0xFD],
                scroll_y: ram[0xFC],
                ppu_ctrl: ram[0xFF],
                fixed_quads: ram[0x60..0x68].try_into().expect("8 bytes"),
                fixed_rows: ram[0x70..0x80].try_into().expect("16 bytes"),
                mirroring: {
                    assert!(ram[0xEC] < 4, "$EC holds more than the mapper's two bits");
                    Mirroring::from_register(ram[0xEC])
                },
                mask: ram[0xFE],
                split: {
                    assert!(matches!(ram[0xD0], 0 | 0x80), "$D0 holds more than bit 7");
                    Raster {
                        armed: ram[0xD0] == 0x80,
                        first_irq_cycles: u16::from_le_bytes([ram[0xD1], ram[0xD2]]),
                        handler: ram[0xD3],
                        next_irq: ram[0xD4],
                    }
                },
                bands: ram[0xD5..0xDD].try_into().expect("8 bytes"),
                start: ram[0x403],
            }
        }

        pub(crate) fn project(&self, ram: &mut [u8; 2048]) {
            ram[0xC3] = self.mode;
            ram[0xB6] = self.gate;
            ram[0xC4..0xC6].copy_from_slice(&self.speed_x.to_be_bytes());
            ram[0xC6..0xC8].copy_from_slice(&self.speed_y.to_be_bytes());
            ram[0xBE] = self.step_x;
            ram[0xBF] = self.step_y;
            ram[0xC8..0xCA].copy_from_slice(&self.ring_y.to_be_bytes());
            ram[0xFD] = self.scroll_x;
            ram[0xFC] = self.scroll_y;
            ram[0xFF] = self.ppu_ctrl;
            ram[0x60..0x68].copy_from_slice(&self.fixed_quads);
            ram[0x70..0x80].copy_from_slice(&self.fixed_rows);
            ram[0xEC] = self.mirroring.register();
            ram[0xFE] = self.mask;
            ram[0xD0] = if self.split.armed { 0x80 } else { 0 };
            ram[0xD1..0xD3].copy_from_slice(&self.split.first_irq_cycles.to_le_bytes());
            ram[0xD3] = self.split.handler;
            ram[0xD4] = self.split.next_irq;
            ram[0xD5..0xDD].copy_from_slice(&self.bands);
            ram[0x403] = self.start;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::oracle::{Scene, assert_routine};
    use super::{auto, speed, step};

    /// The y auto-scroll backward step (`$D34C`): `$C7 = $FC & $B6`, `$C6 = $B6`.
    #[test]
    fn y_auto_scroll_steps_back_with_the_fc_mask() {
        // $C3 bit 0 clear and $B6 nonzero takes $D34C over the $0A forward arm.
        assert_eq!(auto(0x40, 0xFF, 0xFC), -4);
        assert_eq!(auto(0x40, 0xFD, 0xFC), -516);
        // $C3 bit 0 set, or a zero gate, takes the $0A forward arm ($D358).
        assert_eq!(auto(0x41, 0xFF, 0xFC), 0x0A);
        assert_eq!(auto(0x40, 0x00, 0xFC), 0);
    }

    #[test]
    fn speed_matches_d2f3() {
        let Some(mut scene) = Scene::tas(351) else {
            return;
        };
        let checked = assert_routine(&mut scene, 1680, (0xD414, 14), &[0xD417], |call| {
            speed(&mut call.state, call.prg);
        });
        assert!(checked > 1000, "{checked}");
    }

    #[test]
    fn step_matches_8528() {
        let Some(mut scene) = Scene::tas(351) else {
            return;
        };
        let checked = assert_routine(&mut scene, 1680, (0xD420, 14), &[0xD423], |call| {
            step(&mut call.state, call.prg);
        });
        assert!(checked > 1000, "{checked}");
    }
}
