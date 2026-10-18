//! The Modern picture's view of a frame: when it is drawn, and its background from the live map
//! (`docs/specs/2026-10-18-modern-picture-design.md`).

use super::world::{Ppu, orientation};
use super::{Game, Script};
use crate::levelmap::Cell;
use crate::render::Line;

/// What the Modern picture draws of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct View {
    /// The screen's left edge in the level, in pixels.
    pub left: i32,
    /// The screen's top edge in the level, in pixels.
    pub top: i32,
    /// The background's four CHR banks, the same on every line.
    pub banks: [u8; 4],
}

/// The Modern view of the frame `lines` draws from `game` (the state at its NMI), or `None`
/// when the frame is drawn Classic: a script screen, the background off or its banks changing
/// down the screen, or a raster split armed.
#[must_use]
pub fn view(game: &Game, lines: &[Line]) -> Option<View> {
    let in_play = matches!(
        game.script,
        None | Some(Script::Pause(_) | Script::PowerUp(_) | Script::StageEnded)
    );
    let banks = background_banks(lines.first()?);
    let steady = lines
        .iter()
        .all(|line| line.mask & 0x08 != 0 && background_banks(line) == banks);
    let split = game.scroll.split.armed;
    (in_play && steady && !split).then(|| View {
        left: screen_edge(game.world.camera_x, game.scroll.scroll_x),
        top: i32::from(rounded(game.world.camera_y)),
        banks,
    })
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
    let first = if line.ctrl & 0x10 == 0 { 0 } else { 4 };
    line.chr[first..first + 4].try_into().expect("four banks")
}

/// Checks the frame's Modern background against what the PPU fetches: each tile starting in
/// x 8-247 on the lines a TV shows (8-231), at its first line, the palette only where the tile
/// has a pixel to colour (`chr` its patterns). Left out: the vertical ring's seam the overscan
/// hides, and the 8 px edges, where the column the game streams next can show (a scrolled
/// line's, or one frame's as the camera lands). Returns how many cells showed the one
/// difference the game makes: in a vertical room, a page's row 14 with its row 15's palette,
/// which the game streams into row 14's attribute slot as its scroll locks at the page's end.
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
    let tops = lines
        .iter()
        .enumerate()
        .filter(|&(y, line)| (8..232).contains(&y) && (y == 8 || line.v & 0x7000 == 0));
    for (y, line) in tops {
        let first = usize::from((8 - line.fine_x % 8) % 8);
        for x in (first..256).step_by(8).filter(|x| (8..248).contains(x)) {
            let cell = line.background_cell(x);
            let table = &ppu.nametables[cell.physical];
            let (tile, palette) = (
                table[cell.tile_at],
                (table[cell.attribute_at] >> cell.shift) & 3,
            );
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
    Ok(row_15_palettes)
}

/// Whether background tile `tile` of `banks` is colour 0 throughout.
fn blank(chr: &[u8], banks: [u8; 4], tile: u8) -> bool {
    let bank = usize::from(banks[usize::from(tile >> 6)]) * 1024 % chr.len();
    let at = bank + usize::from(tile & 0x3F) * 16;
    chr[at..at + 16].iter().all(|&byte| byte == 0)
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
        masked[100].mask &= !0x08;
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
}
