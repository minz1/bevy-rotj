//! The Modern picture's view of a frame: when it is drawn, and its background from the live map
//! (`docs/specs/2026-10-18-modern-picture-design.md`).

use super::stage::FINAL_AREA;
use super::world::{Ppu, orientation};
use super::{Game, Script};
use crate::chr;
use crate::levelmap::Cell;
use crate::render::{Line, ctrl, mask};
use crate::rooms::Chain;

/// What the Modern picture draws of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct View {
    /// The screen's left edge in the level, in pixels.
    pub left: i32,
    /// The screen's top edge in the level, in pixels.
    pub top: i32,
    /// Line 0's four CHR banks, the map playfield's.
    pub banks: [u8; 4],
    /// The line the playfield from the live map stops at: 240 without a split, 0 when every
    /// line is a run.
    pub map_end: usize,
    /// The lines from `map_end` down, as runs from the nametables, top to bottom.
    pub runs: Runs,
}

/// The most runs a frame drawn Modern has (area `$18` shows 7).
pub const MAX_RUNS: usize = 8;

/// A frame's runs, the unused ones `None` at the end.
pub type Runs = [Option<Run>; MAX_RUNS];

/// Lines of the screen drawn from the nametables in screen space: one scroll, one set of banks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    /// Its first line.
    pub line: usize,
    /// The line after its last.
    pub end: usize,
    /// `line`'s state: its scroll and nametable, its first tile row starting fine y px above.
    pub top: Line,
}

impl Run {
    /// How far above `line` its first tile row starts.
    #[must_use]
    pub fn fine_y(&self) -> usize {
        usize::from(self.top.v >> 12) & 7
    }

    /// Its four background banks.
    #[must_use]
    pub fn banks(&self) -> [u8; 4] {
        background_banks(&self.top)
    }
}

/// Whether the game is in play: the loop, a pause or a power-up's freeze, a stage's end.
#[must_use]
pub fn in_play(game: &Game) -> bool {
    matches!(
        game.script,
        None | Some(Script::Pause(_) | Script::PowerUp(_) | Script::StageEnded)
    )
}

/// The Modern view of the frame `lines` draws from `game` (its NMI state); `None` draws Classic.
#[must_use]
pub fn view(game: &Game, lines: &[Line]) -> Option<View> {
    let banks = background_banks(lines.first()?);
    let split = game.scroll.split;
    let (map_end, runs) = if split.armed {
        if split.chain != Chain::BossBar {
            return None;
        }
        let map_end = if game.world.area == FINAL_AREA {
            0
        } else {
            runs(lines, 0)?[0].map_or(0, |run| run.end)
        };
        (map_end, runs(lines, map_end)?)
    } else {
        (lines.len(), [None; MAX_RUNS])
    };
    let shown = lines.iter().all(|line| line.mask & mask::BACKGROUND != 0);
    let steady = split.armed || lines.iter().all(|line| background_banks(line) == banks);
    (in_play(game) && shown && steady && sprites_steady(game, lines)).then(|| View {
        left: screen_edge(game.world.camera_x, game.scroll.scroll_x),
        top: i32::from(rounded(game.world.camera_y)),
        banks,
        map_end,
        runs,
    })
}

/// Lines `from..` as runs, split where the scroll, the banks or the mirroring change, `v`
/// stops following the PPU's vertical increment, or it wraps past row 29; `None` past
/// [`MAX_RUNS`].
#[must_use]
pub fn runs(lines: &[Line], from: usize) -> Option<Runs> {
    let mut runs: Runs = [None; MAX_RUNS];
    let mut count = 0;
    for (y, line) in lines.iter().enumerate().skip(from) {
        if y > from && follows(&lines[y - 1], line) {
            if let Some(run) = &mut runs[count - 1] {
                run.end = y + 1;
            }
            continue;
        }
        *runs.get_mut(count)? = Some(Run {
            line: y,
            end: y + 1,
            top: *line,
        });
        count += 1;
    }
    Some(runs)
}

/// Whether `line` draws on from `above` in one run.
fn follows(above: &Line, line: &Line) -> bool {
    let wraps_nametable = above.v & 0x73E0 == 0x73A0;
    !wraps_nametable
        && line.v == super::raster::down(above.v)
        && line.fine_x == above.fine_x
        && line.mirroring == above.mirroring
        && background_banks(line) == background_banks(above)
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

/// The run's tile and palette at a cell from its left (`top.fine_x` px off screen) and its first
/// tile row.
#[must_use]
pub fn run_tile(ppu: &Ppu, run: &Run, column: usize, row: usize) -> (u8, u8) {
    let coarse_y = (((run.top.v >> 5) & 0x1F) + u16::try_from(row).expect("a run's row")) & 0x1F;
    let line = Line {
        v: (run.top.v & !0x03E0) | (coarse_y << 5),
        fine_x: 0,
        ..run.top
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
/// The map is checked down to `map_end`, and each run's lines by [`check_run`].
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
    let end = view.map_end.min(232);
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
    for run in view.runs.iter().flatten() {
        check_run(run, lines, ppu)?;
    }
    Ok(row_15_palettes)
}

/// The screen x of each of the line's 8 px cells that starts in x 8-247.
fn cells(line: &Line) -> impl Iterator<Item = usize> {
    let first = usize::from((8 - line.fine_x % 8) % 8);
    (first..248).step_by(8).filter(|&x| x >= 8)
}

/// Checks the run's TV-visible lines are one run in its banks: each line's fine x is its top's,
/// its fine y follows on, and each cell's tile is [`run_tile`]'s.
///
/// # Errors
/// The first line or cell that differs.
fn check_run(run: &Run, lines: &[Line], ppu: &Ppu) -> Result<(), String> {
    let shown = run.line.max(8)..run.end.min(232);
    for (y, line) in lines.iter().enumerate().take(shown.end).skip(shown.start) {
        let below = y - run.line + run.fine_y();
        let (row, fine_y) = (below / 8, usize::from(line.v >> 12) & 7);
        let banks = background_banks(line);
        if line.fine_x != run.top.fine_x || fine_y != below % 8 || banks != run.banks() {
            return Err(format!(
                "line {y}: fine x {}, fine y {fine_y}, banks {banks:02X?} in the run from line {}, its top's fine x {}, banks {:02X?}",
                line.fine_x,
                run.line,
                run.top.fine_x,
                run.banks()
            ));
        }
        for x in cells(line) {
            let drawn = nametable_tile(ppu, line, x);
            let model = run_tile(ppu, run, (x + usize::from(line.fine_x)) / 8, row);
            if drawn != model {
                return Err(format!(
                    "line {y} x {x}: the run's row {row} gives {model:02X?}, the nametable {drawn:02X?}"
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

    /// Stage 1-1's second loop frame with a `$D3` 4 split armed, and its lines.
    fn boss_bar_frame() -> Option<(Game, Vec<Line>)> {
        let ((mut game, _), rom) = (stage_1()?, rotj_rom::for_test()?);
        game.frame(crate::prg::for_test().expect("the PRG"), &rom.chr);
        game.scroll.split.armed = true;
        game.scroll.split.chain = Chain::BossBar;
        game.scroll.split.first_irq_cycles = 0x5E00;
        game.scroll.split.next_irq = 0;
        let lines = raster::lines(&raster::Registers::of(&game), &game);
        Some((game, lines))
    }

    /// The runs' `(line, end)`s.
    fn spans(runs: &Runs) -> Vec<(usize, usize)> {
        runs.iter()
            .flatten()
            .map(|run| (run.line, run.end))
            .collect()
    }

    #[test]
    fn a_d3_4_boss_bar_is_the_map_then_runs_and_each_reason_makes_it_classic() {
        let Some((game, lines)) = boss_bar_frame() else {
            return;
        };
        let shown = view(&game, &lines).expect("a boss bar frame");
        let slot_3 = lines[0].chr[3];
        assert_eq!(shown.map_end, 192, "the map down to the bank change");
        assert_eq!(spans(&shown.runs), [(192, 193), (193, 240)]);
        assert_eq!(
            shown.runs[1].map(|bar| (bar.top.v, bar.banks())),
            Some((0x1300, [0x39, 0x3A, 0x3B, slot_3])),
            "the bar one line into its row, in its banks"
        );
        assert_eq!(
            lines[192].chr[..3],
            [0x39, 0x2D, 0x2E],
            "slot 0 changed only"
        );
        let with = |change: &dyn Fn(&mut Vec<Line>)| {
            let mut lines = lines.clone();
            change(&mut lines);
            view(&game, &lines).map(|view| (view.map_end, spans(&view.runs)))
        };
        assert_eq!(
            with(&|lines| lines[192].chr = lines[193].chr),
            Some((192, vec![(192, 240)])),
            "line 192 all the bar's"
        );
        let playfield = lines[0].chr;
        assert_eq!(
            with(&|lines| lines[192..195]
                .iter_mut()
                .for_each(|line| line.chr = playfield)),
            Some((195, vec![(195, 240)])),
            "the change a row's fourth line in"
        );
        assert_eq!(
            with(&|lines| lines.iter_mut().for_each(|line| line.chr = playfield)),
            Some((240, vec![])),
            "no bank change: all the map"
        );
        let mut off_edge = game.clone();
        off_edge.world.camera_y = 4 << 4;
        assert_eq!(
            view(&off_edge, &lines).map(|view| view.map_end),
            Some(192),
            "off a tile edge"
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
        other.scroll.split.chain = Chain::BossFloor;
        assert_eq!(view(&other, &lines), None, "another handler");
    }

    #[test]
    fn runs_split_where_the_scroll_banks_or_increment_change() {
        let Some((_, lines)) = boss_bar_frame() else {
            return;
        };
        assert_eq!(
            runs(&lines, 0).as_ref().map(spans),
            Some(vec![(0, 192), (192, 193), (193, 240)])
        );
        let mut shifted = lines.clone();
        shifted[100].fine_x ^= 1;
        assert_eq!(
            runs(&shifted, 0).as_ref().map(spans),
            Some(vec![
                (0, 100),
                (100, 101),
                (101, 192),
                (192, 193),
                (193, 240)
            ]),
            "a fine x change"
        );
        let from_row = |row: u16| {
            let mut from = lines.clone();
            let mut v = row << 5;
            for line in &mut from {
                line.v = v;
                v = raster::down(v);
                line.chr = lines[0].chr;
            }
            from
        };
        assert_eq!(
            runs(&from_row(20), 0).as_ref().map(spans),
            Some(vec![(0, 80), (80, 240)]),
            "the PPU's wrap past row 29"
        );
        assert_eq!(
            runs(&from_row(30), 0).as_ref().map(spans),
            Some(vec![(0, 240)]),
            "row 31 on to row 0 of the same nametable"
        );
        let mut jumped = from_row(0);
        jumped[50].v ^= 0x20;
        assert_eq!(
            runs(&jumped, 0).as_ref().map(spans),
            Some(vec![(0, 50), (50, 51), (51, 240)]),
            "a $2006 write moving v"
        );
        let mut crowded = lines.clone();
        for line in &mut crowded[..=MAX_RUNS] {
            line.fine_x = u8::try_from(line.v >> 12).expect("a fine y") & 1;
        }
        assert_eq!(runs(&crowded, 192), runs(&lines, 192), "from line 192");
        assert_eq!(runs(&crowded, 0), None, "past MAX_RUNS");
    }

    #[test]
    fn the_final_boss_s_room_is_all_runs() {
        let Some((mut game, lines)) = boss_bar_frame() else {
            return;
        };
        game.world.area = FINAL_AREA;
        let shown = view(&game, &lines).expect("the final boss's room");
        assert_eq!(
            (shown.map_end, spans(&shown.runs)),
            (0, vec![(0, 192), (192, 193), (193, 240)]),
            "every line from the nametables"
        );
        assert_eq!(shown.runs[0].map(|run| run.top), Some(lines[0]));
        let mut unarmed = game.clone();
        unarmed.scroll.split.armed = false;
        let unarmed_lines = raster::lines(&raster::Registers::of(&unarmed), &unarmed);
        assert_eq!(
            view(&unarmed, &unarmed_lines).map(|view| (view.map_end, view.runs[0])),
            Some((240, None)),
            "no split armed: from the map"
        );
        let mut shaken = game.clone();
        shaken.scroll.scroll_x = 0xFC;
        let shaken_lines = raster::lines(&raster::Registers::of(&shaken), &shaken);
        assert_eq!(
            view(&shaken, &shaken_lines)
                .and_then(|view| view.runs[0])
                .map(|run| (run.top.v & 0x1F, run.top.fine_x)),
            Some((31, 4)),
            "a shaken frame's run from coarse x 31"
        );
    }
}
