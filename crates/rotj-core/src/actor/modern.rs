//! The Modern picture's view of a frame: when it is drawn, and its background from the live map
//! (`docs/specs/2026-10-18-modern-picture-design.md`).

use super::stage::FINAL_AREA;
use super::world::{Ppu, orientation};
use super::{Game, Script};
use crate::chr;
use crate::levelmap::Cell;
use crate::render::{Line, ctrl, mask};

/// What the Modern picture draws of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct View {
    /// The screen's left edge in the level, in pixels.
    pub left: i32,
    /// The screen's top edge in the level, in pixels.
    pub top: i32,
    /// The playfield's four CHR banks, the same on every line above the strip.
    pub banks: [u8; 4],
    /// The boss bar below the playfield, on a `$D3` 4 frame.
    pub strip: Option<Strip>,
}

/// The screen's bottom rows drawn from the nametables with banks of their own (the boss bar).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Strip {
    /// The screen line of its first row.
    pub line: usize,
    /// That row's line state with fine y 0: its scroll and nametable.
    pub top: Line,
    /// Its four background banks.
    pub banks: [u8; 4],
}

/// The Modern view of the frame `lines` draws from `game` (its NMI state); `None` draws Classic.
#[must_use]
pub fn view(game: &Game, lines: &[Line]) -> Option<View> {
    let in_play = matches!(
        game.script,
        None | Some(Script::Pause(_) | Script::PowerUp(_) | Script::StageEnded)
    );
    let banks = background_banks(lines.first()?);
    let top = i32::from(rounded(game.world.camera_y));
    let strip = match game.scroll.split {
        split if !split.armed => None,
        // The final boss's death tiles (`$9249`) reach the nametables only, not the map.
        split if split.handler == 4 && game.world.area != FINAL_AREA => {
            Some(strip(lines, banks, top)?)
        }
        _ => return None,
    };
    let shown = lines.iter().all(|line| line.mask & mask::BACKGROUND != 0);
    let steady = strip.is_some() || lines.iter().all(|line| background_banks(line) == banks);
    (in_play && shown && steady && sprites_steady(game, lines)).then(|| View {
        left: screen_edge(game.world.camera_x, game.scroll.scroll_x),
        top,
        banks,
        strip,
    })
}

/// The boss bar of a `$D3` 4 frame, from the first line whose banks are not the playfield's.
fn strip(lines: &[Line], playfield: [u8; 4], top: i32) -> Option<Strip> {
    let changed = lines
        .iter()
        .position(|line| background_banks(line) != playfield)?;
    let banks = background_banks(lines.last()?);
    let now = background_banks(&lines[changed]);
    let mixed = (0..4).all(|slot| now[slot] == playfield[slot] || now[slot] == banks[slot]);
    let below = lines[changed + 1..]
        .iter()
        .all(|line| background_banks(line) == banks);
    let first = lines[changed];
    let line = changed.checked_sub(usize::from(first.v >> 12) & 7)?;
    let edge = (top + i32::try_from(line).ok()?) % 8 == 0;
    let rows = (lines.len() - line).div_ceil(8);
    let within = usize::from((first.v >> 5) & 0x1F) + rows <= 30;
    (mixed && below && edge && within).then_some(Strip {
        line,
        top: Line {
            v: first.v & 0x0FFF,
            ..first
        },
        banks,
    })
}

/// Whether every visible sprite sees one set of banks down its lines, as Modern draws it.
fn sprites_steady(game: &Game, lines: &[Line]) -> bool {
    game.oam
        .shown
        .iter()
        .flat_map(|record| record.sprites(256))
        .filter(|entry| entry.is_visible())
        .all(|entry| {
            let table = 4 * usize::from(entry.tile & 1);
            let first = usize::from(entry.y) + 1;
            let run = &lines[first.min(lines.len())..(first + 16).min(lines.len())];
            run.windows(2).all(|pair| {
                pair[0].sprite_chr[table..table + 4] == pair[1].sprite_chr[table..table + 4]
            })
        })
}

/// The strip's tile and palette at a cell from its left (`top.fine_x` px off screen) and top.
#[must_use]
pub fn strip_tile(ppu: &Ppu, strip: &Strip, column: usize, row: usize) -> (u8, u8) {
    let coarse_y = (((strip.top.v >> 5) & 0x1F) + u16::try_from(row).expect("a strip row")) & 0x1F;
    let line = Line {
        v: (strip.top.v & !0x03E0) | (coarse_y << 5),
        fine_x: 0,
        ..strip.top
    };
    nametable_tile(ppu, &line, column * 8)
}

/// The tile and palette the nametables give line `line`'s background pixel `x`.
fn nametable_tile(ppu: &Ppu, line: &Line, x: usize) -> (u8, u8) {
    let cell = line.background_cell(x);
    let table = &ppu.nametables[cell.physical];
    (
        table[cell.tile_at],
        (table[cell.attribute_at] >> cell.shift) & 3,
    )
}

/// The background tile and palette at a level pixel, from the live map and the level's
/// attributes; `None` off the level.
#[must_use]
pub fn tile_at(game: &Game, x: i32, y: i32) -> Option<(u8, u8)> {
    let (x, y) = (u16::try_from(x).ok()?, u16::try_from(y).ok()?);
    let [column, row] = [x, y].map(|at| u8::try_from(at >> 4).ok());
    let (column, row) = (column?, row?);
    let vertical = game.world.orientation & orientation::VERTICAL != 0;
    let cell = Cell::at(column, row, &game.schedule.page_columns, vertical);
    let level = &game.effects.level;
    let corner = usize::from((y >> 3) & 1) * 2 + usize::from((x >> 3) & 1);
    let tile = level.tiles(level.map[cell])[corner];
    let at = usize::from(cell.column >> 4) * 64
        + usize::from(cell.row / 2) * 8
        + usize::from((cell.column & 0x0F) / 2);
    let shift = (cell.row & 1) * 4 + (cell.column & 1) * 2;
    Some((tile, (level.attributes[at] >> shift) & 3))
}

/// A camera coordinate (1/16 px) rounded to a pixel, as the game rounds its PPU scroll.
fn rounded(camera: u16) -> u16 {
    (camera.wrapping_add(8) >> 4) & 0x0FFF
}

/// The screen's left edge in level pixels: the camera rounded, moved by how far the PPU's
/// x scroll is from it (the final boss's shake).
fn screen_edge(camera: u16, scroll: u8) -> i32 {
    let rounded = rounded(camera);
    let offset = scroll.wrapping_sub(rounded.to_le_bytes()[0]).cast_signed();
    i32::from(rounded) + i32::from(offset)
}

/// The four banks a line's background is fetched from (`$2000` bit 4 picks the table).
fn background_banks(line: &Line) -> [u8; 4] {
    let first = if line.ctrl & ctrl::BACKGROUND_PATTERNS_HIGH == 0 {
        0
    } else {
        4
    };
    line.chr[first..first + 4].try_into().expect("four banks")
}

/// Checks the frame's Modern background against what the PPU fetches: each tile starting in
/// x 8-247 on the lines a TV shows (8-231), at its first line, the palette only where the tile
/// has a pixel to colour (`chr` its patterns). Left out: the vertical ring's seam the overscan
/// hides, and the 8 px edges, where the column the game streams next can show (a scrolled
/// line's, or one frame's as the camera lands). Returns how many cells showed the one
/// difference the game makes: in a vertical room, a page's row 14 with its row 15's palette,
/// which the game streams into row 14's attribute slot as its scroll locks at the page's end.
/// A strip's lines are checked by [`check_strip`] instead.
///
/// # Errors
/// The first cell that differs otherwise.
pub fn check_background(
    view: &View,
    game: &Game,
    lines: &[Line],
    ppu: &Ppu,
    chr: &[u8],
) -> Result<usize, String> {
    let vertical = game.world.orientation & orientation::VERTICAL != 0;
    let mut row_15_palettes = 0;
    let end = view.strip.map_or(232, |strip| strip.line.min(232));
    let tops = lines
        .iter()
        .enumerate()
        .filter(|&(y, line)| (8..end).contains(&y) && (y == 8 || line.v & 0x7000 == 0));
    for (y, line) in tops {
        for x in cells(line) {
            let (tile, palette) = nametable_tile(ppu, line, x);
            let at = (
                view.left + i32::try_from(x).expect("a screen x"),
                view.top + i32::try_from(y).expect("a line"),
            );
            let model = tile_at(game, at.0, at.1);
            let row_15 = || {
                (vertical && (at.1 >> 4) & 0x0F == 14)
                    .then(|| tile_at(game, at.0, at.1 + 16))
                    .flatten()
                    .map(|(_, below)| below)
            };
            match model {
                Some(model)
                    if model.0 == tile && (model.1 == palette || blank(chr, view.banks, tile)) => {}
                Some(model) if model.0 == tile && row_15() == Some(palette) => row_15_palettes += 1,
                _ => {
                    return Err(format!(
                        "line {y} x {x} (level {at:?}): the map gives {model:02X?}, the nametable {:02X?}",
                        (tile, palette)
                    ));
                }
            }
        }
    }
    if let Some(strip) = &view.strip {
        check_strip(strip, lines, ppu, chr)?;
    }
    Ok(row_15_palettes)
}

/// The screen x of each of the line's 8 px cells that starts in x 8-247.
fn cells(line: &Line) -> impl Iterator<Item = usize> {
    let first = usize::from((8 - line.fine_x % 8) % 8);
    (first..248).step_by(8).filter(|&x| x >= 8)
}

/// Checks the strip's TV-visible lines are one unscrolled run its banks draw the same.
///
/// # Errors
/// The first line or cell that differs.
fn check_strip(strip: &Strip, lines: &[Line], ppu: &Ppu, chr: &[u8]) -> Result<(), String> {
    for (y, line) in lines.iter().enumerate().take(232).skip(strip.line.max(8)) {
        let (row, fine_y) = ((y - strip.line) / 8, usize::from(line.v >> 12) & 7);
        if line.fine_x != strip.top.fine_x || fine_y != (y - strip.line) % 8 {
            return Err(format!(
                "line {y}: fine x {} and fine y {fine_y} in the strip, its top's fine x {}",
                line.fine_x, strip.top.fine_x
            ));
        }
        let banks = background_banks(line);
        for x in cells(line) {
            let drawn = nametable_tile(ppu, line, x);
            let model = strip_tile(ppu, strip, (x + usize::from(line.fine_x)) / 8, row);
            let pattern = |banks: [u8; 4]| {
                let bytes = chr::tile(chr, &banks, usize::from(drawn.0) * 16);
                (bytes[fine_y], bytes[fine_y + 8])
            };
            if drawn != model || pattern(banks) != pattern(strip.banks) {
                return Err(format!(
                    "line {y} x {x}: the strip's row {row} gives {model:02X?} in banks {:02X?}, the nametable {drawn:02X?} in {banks:02X?}",
                    strip.banks
                ));
            }
        }
    }
    Ok(())
}

/// Whether background tile `tile` of `banks` is colour 0 throughout.
fn blank(chr: &[u8], banks: [u8; 4], tile: u8) -> bool {
    chr::tile(chr, &banks, usize::from(tile) * 16)
        .iter()
        .all(|&byte| byte == 0)
}

#[cfg(test)]
mod tests {
    use super::super::{Script, raster};
    use super::*;

    /// Stage 1-1's first loop frame and the lines it is drawn with, or `None` without the ROM.
    fn stage_1() -> Option<(Game, Vec<Line>)> {
        let rom = rotj_rom::for_test()?;
        let game = super::super::room::start(crate::prg::for_test()?, &rom.chr, 0);
        let lines = raster::lines(&raster::Registers::of(&game), &game);
        Some((game, lines))
    }

    #[test]
    fn play_is_modern_and_each_reason_makes_it_classic() {
        let Some((game, lines)) = stage_1() else {
            return;
        };
        assert!(view(&game, &lines).is_some(), "stage 1-1 in play");
        let mut script = game.clone();
        script.script = Some(Script::Ending(super::super::ending::Ending::default()));
        let mut split = game.clone();
        split.scroll.split.armed = true;
        let mut masked = lines.clone();
        masked[100].mask &= !mask::BACKGROUND;
        let mut banked = lines.clone();
        banked[100].chr[0] ^= 1;
        assert_eq!(view(&script, &lines), None, "a script screen");
        assert_eq!(view(&split, &lines), None, "a raster split");
        assert_eq!(view(&game, &masked), None, "the background off on a line");
        assert_eq!(
            view(&game, &banked),
            None,
            "the banks changing down the screen"
        );
    }

    #[test]
    fn a_d3_4_boss_bar_is_a_strip_and_each_reason_makes_it_classic() {
        let (Some((mut game, _)), Some(rom)) = (stage_1(), rotj_rom::for_test()) else {
            return;
        };
        game.frame(crate::prg::for_test().expect("the PRG"), &rom.chr);
        game.scroll.split.armed = true;
        game.scroll.split.handler = 4;
        game.scroll.split.first_irq_cycles = 0x5E00;
        game.scroll.split.next_irq = 0;
        let lines = raster::lines(&raster::Registers::of(&game), &game);
        let strip = view(&game, &lines).and_then(|view| view.strip);
        let playfield = lines[0].chr;
        let slot_3 = playfield[3];
        assert_eq!(
            strip.map(|strip| (strip.line, strip.top.v, strip.banks)),
            Some((192, 0x0300, [0x39, 0x3A, 0x3B, slot_3])),
            "the bar at line 192, its line mixed"
        );
        assert_eq!(
            lines[192].chr[..3],
            [0x39, 0x2D, 0x2E],
            "slot 0 changed only"
        );
        let with = |change: &dyn Fn(&mut Vec<Line>)| {
            let mut lines = lines.clone();
            change(&mut lines);
            view(&game, &lines).map(|view| view.strip.map(|strip| strip.line))
        };
        assert_eq!(
            with(&|lines| lines[192].chr = lines[193].chr),
            Some(Some(192)),
            "line 192 all the strip's"
        );
        assert_eq!(
            with(&|lines| lines[192].chr[0] = 0x40),
            None,
            "line 192 a bank neither's"
        );
        assert_eq!(
            with(&|lines| lines.iter_mut().for_each(|line| line.chr = playfield)),
            None,
            "no bank change"
        );
        assert_eq!(
            with(&|lines| lines[200].chr[1] ^= 1),
            None,
            "a later bank change"
        );
        let sprite = game
            .oam
            .shown
            .iter()
            .flat_map(|record| record.sprites(256))
            .find(|entry| entry.is_visible())
            .expect("a sprite on screen");
        let table = 4 * usize::from(sprite.tile & 1);
        assert_eq!(
            with(&|lines| lines[usize::from(sprite.y) + 9].sprite_chr[table] ^= 1),
            None,
            "a sprite spanning a bank change"
        );
        let mut other = game.clone();
        other.scroll.split.handler = 3;
        assert_eq!(view(&other, &lines), None, "another handler");
        let mut off_edge = game.clone();
        off_edge.world.camera_y = 4 << 4;
        assert_eq!(view(&off_edge, &lines), None, "off a tile edge");
        assert_eq!(
            with(&|lines| lines[192..195]
                .iter_mut()
                .for_each(|line| line.chr = playfield)),
            Some(Some(192)),
            "the change a row's fourth line in"
        );
        assert_eq!(
            with(&|lines| lines[192].v = (lines[192].v & !0x03E0) | (26 << 5)),
            None,
            "rows past its nametable's"
        );
        let mut last = game.clone();
        last.world.area = FINAL_AREA;
        assert_eq!(view(&last, &lines), None, "the final boss's room");
    }
}
