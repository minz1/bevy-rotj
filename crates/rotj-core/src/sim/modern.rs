//! The Modern picture's view of a frame: when it is drawn, and its background from the live map
//! and per-split nametable runs (`docs/specs/2026-11-25-1.2.0-enhanced-presentation-design.md`).

use super::stage::{self, FINAL_AREA};
use super::world::{Ppu, orientation};
use super::{Game, Script};
#[cfg(test)]
use crate::chr;
use crate::levels::Levels;
use crate::map::{Cell, Chain};
use crate::render::{Line, SCREEN_H, SCREEN_W, ctrl, mask};

/// The Modern tilemap's size in 8x8 tiles: the screen and one more each way, for the scroll.
pub const COLUMNS: usize = SCREEN_W / 8 + 1;
/// See [`COLUMNS`].
pub const ROWS: usize = SCREEN_H / 8 + 1;

/// One piece's `COLUMNS * ROWS` background grid of `(tile, palette)` pairs.
pub type TileGrid = Vec<Option<(u8, u8)>>;

/// What the Modern picture draws of the frame on screen, taken with the page it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModernFrame {
    /// Where the screen is in the level, and the ordered background pieces covering `0..240`.
    pub view: View,
    /// `$2001`'s greyscale bit.
    pub greyscale: bool,
    /// Map tiles (`COLUMNS * ROWS`) for each [`Piece::Map`] in [`View::pieces`], tagged by its
    /// piece index.
    pub map_tiles: Vec<(usize, TileGrid)>,
}

impl ModernFrame {
    /// The frame `lines` draws from `game`, or `None` when it is drawn Classic.
    #[must_use]
    pub(crate) fn of(game: &Game, levels: &Levels, lines: &[Line]) -> Option<ModernFrame> {
        let view = view(game, levels, lines)?;
        let mut map_tiles = Vec::new();
        for (index, piece) in view.pieces.iter().enumerate() {
            let Some(Piece::Map {
                line,
                end,
                left,
                top,
                ..
            }) = *piece
            else {
                continue;
            };
            let base_left = left - left.rem_euclid(8);
            let base_top = top - top.rem_euclid(8);
            let height = i32::try_from(end - line).expect("a line count");
            let max_y = top + height;
            let tiles = (0..ROWS)
                .flat_map(|row| (0..COLUMNS).map(move |column| (column, row)))
                .map(|(column, row)| {
                    let [x, y] =
                        [column, row].map(|at| i32::try_from(at * 8).expect("a screen's worth"));
                    (base_top + y < max_y)
                        .then(|| tile_at(game, base_left + x, base_top + y))
                        .flatten()
                })
                .collect();
            map_tiles.push((index, tiles));
        }
        Some(ModernFrame {
            view,
            greyscale: lines
                .first()
                .is_some_and(|line| line.mask & mask::GREYSCALE != 0),
            map_tiles,
        })
    }

    /// The pre-fetched map tile grid for piece `piece_index`, if it is a [`Piece::Map`].
    #[must_use]
    pub fn tiles_for(&self, piece_index: usize) -> Option<&[Option<(u8, u8)>]> {
        self.map_tiles
            .iter()
            .find(|(index, _)| *index == piece_index)
            .map(|(_, tiles)| tiles.as_slice())
    }
}

/// The most pieces (map playfields + nametable runs) a Modern frame can have (area `$18` shows 7).
pub const MAX_RUNS: usize = 8;

/// A frame's runs, the unused ones `None` at the end.
pub type Runs = [Option<Run>; MAX_RUNS];

/// A frame's ordered vertical pieces, the unused ones `None` at the end.
pub type Pieces = [Option<Piece>; MAX_RUNS];

/// One vertical slice of a Modern frame: either a live-map playfield or a nametable run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Piece {
    /// Screen lines `line..end` drawn from the live level map: screen `(0, line)` maps to
    /// level pixel `(left, top)`.
    Map {
        /// First screen scanline.
        line: usize,
        /// Scanline after the last.
        end: usize,
        /// Level x pixel shown at screen `(0, line)`.
        left: i32,
        /// Level y pixel shown at screen `(0, line)`.
        top: i32,
        /// Four 1 KiB background CHR banks.
        banks: [u8; 4],
    },
    /// Screen lines `run.line..run.end` drawn from the PPU nametables.
    Run(Run),
}

impl Piece {
    /// First screen scanline covered by this piece.
    #[must_use]
    pub const fn line(self) -> usize {
        match self {
            Self::Map { line, .. } => line,
            Self::Run(run) => run.line,
        }
    }

    /// Scanline after the last covered by this piece.
    #[must_use]
    pub const fn end(self) -> usize {
        match self {
            Self::Map { end, .. } => end,
            Self::Run(run) => run.end,
        }
    }

    /// True if this piece draws from the live level map.
    #[must_use]
    pub const fn is_map(self) -> bool {
        matches!(self, Self::Map { .. })
    }
}

/// What the Modern picture draws of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct View {
    /// The screen's nominal left edge in the level, in pixels.
    pub left: i32,
    /// The screen's nominal top edge in the level, in pixels.
    pub top: i32,
    /// Line 0's four CHR banks.
    pub banks: [u8; 4],
    /// Ordered top-to-bottom vertical pieces covering `0..240`.
    pub pieces: Pieces,
}

impl View {
    /// True when at least one piece draws from the live level map.
    #[must_use]
    pub fn has_map(&self) -> bool {
        self.pieces.iter().flatten().any(|piece| piece.is_map())
    }
}

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

    /// Its 9-bit horizontal nametable pixel scroll (`0..512`).
    #[must_use]
    pub fn scroll_x(&self) -> u16 {
        let nt_x = (self.top.v >> 10) & 1;
        let coarse_x = self.top.v & 0x1F;
        (nt_x << 8) | (coarse_x << 3) | u16::from(self.top.fine_x)
    }
}

/// Whether the game is in play: the loop, a pause or a power-up's freeze, a stage's end.
#[must_use]
pub(crate) fn in_play(game: &Game) -> bool {
    matches!(
        game.script,
        None | Some(Script::Pause(_) | Script::PowerUp(_) | Script::StageEnded)
    )
}

/// Aligns `rounded(camera_x)` to the nametable X scroll of `run` (nearest level x matching
/// `run`'s horizontal ring: 512 px for `SideBySide` mirroring, 256 px for 1-screen-wide mirroring).
fn map_left_from_run(camera_x: u16, run: &Run) -> i32 {
    let period = if run.top.mirroring == crate::render::Mirroring::SideBySide {
        512
    } else {
        256
    };
    let half = period / 2;
    let base = i32::from(rounded(camera_x));
    let target = i32::from(run.scroll_x()).rem_euclid(period);
    let delta = (target - base + half).rem_euclid(period) - half;
    base + delta
}

/// Converts `run` into a [`Piece::Map`] whose `(left, top)` matches the run's live map position.
fn map_piece(game: &Game, run: Run, top: i32) -> Piece {
    Piece::Map {
        line: run.line,
        end: run.end,
        left: map_left_from_run(game.world.camera_x, &run),
        top,
        banks: run.banks(),
    }
}

/// True if `run` is a playfield run drawn from the live map in `chain` (`index` of `count`).
fn is_playfield_run(
    chain: Chain,
    vertical: bool,
    index: usize,
    count: usize,
    run: Run,
    lines_len: usize,
) -> bool {
    match chain {
        Chain::BossBar => index == 0,
        Chain::FixedRow => !vertical && index == 0,
        Chain::TwoBands => index + 1 == count && run.end == lines_len && run.line >= 100,
        Chain::ThreeBands => index + 1 == count && run.end == lines_len && run.line >= 120,
        Chain::BossFloor => index == 2 && run.line >= 100 && run.end < lines_len,
        Chain::FixedTop => true,
        Chain::FourBands
        | Chain::FourBandsTall
        | Chain::SwingingRow
        | Chain::Intro
        | Chain::Cutscene
        | Chain::Closing
        | Chain::EndingTitle
        | Chain::Credits => false,
    }
}

/// The Modern view of the frame `lines` draws from `game` (its NMI state) in `levels`; `None`
/// draws Classic.
#[must_use]
pub(crate) fn view(game: &Game, levels: &Levels, lines: &[Line]) -> Option<View> {
    let banks = background_banks(lines.first()?);
    let split = game.scroll.split;
    let left = screen_edge(game.world.camera_x, game.scroll.x);
    let top = i32::from(rounded(game.world.camera_y));
    let pieces = if split.armed {
        let raw = runs(lines, 0)?;
        let count = raw.iter().flatten().count();
        let final_area = stage::at(levels, game.world.area).area == FINAL_AREA;
        let vertical = game.world.orientation & orientation::VERTICAL != 0;
        let mut out: Pieces = [None; MAX_RUNS];
        let mut cursor = 0usize;
        for (index, run) in raw.iter().flatten().copied().enumerate() {
            if !final_area
                && is_playfield_run(split.chain, vertical, index, count, run, lines.len())
            {
                // Trim leading/trailing lines whose metatile row is marked fixed (`fixed_rows`)
                // so only map-streamed rows draw from `tile_at`, plus clamp `BossFloor` before
                // the status bar overwrites row 24 (line 192) and keep `FixedTop`'s fixed middle
                // art (lines 111..194) as a nametable run even when `scroll.x == 0` merges runs.
                let is_fixed = |y: usize| {
                    if split.chain == Chain::FixedTop && (111..194).contains(&y) {
                        return true;
                    }
                    if split.chain == Chain::BossFloor && y >= 192 {
                        return true;
                    }
                    let row = (top + i32::try_from(y).expect("a line")) >> 4;
                    usize::try_from(row)
                        .ok()
                        .and_then(|r| game.scroll.fixed_rows.get(r))
                        .is_some_and(|&flag| flag & 0x80 != 0)
                };
                let mut y = run.line;
                while y < run.end {
                    let fixed = is_fixed(y);
                    let seg_start = y;
                    while y < run.end && is_fixed(y) == fixed {
                        y += 1;
                    }
                    let seg_run = Run {
                        line: seg_start,
                        end: y,
                        top: lines[seg_start],
                    };
                    let piece = if fixed {
                        Piece::Run(seg_run)
                    } else {
                        let piece_top = top + i32::try_from(seg_start).expect("a line");
                        map_piece(game, seg_run, piece_top)
                    };
                    *out.get_mut(cursor)? = Some(piece);
                    cursor += 1;
                }
            } else {
                *out.get_mut(cursor)? = Some(Piece::Run(run));
                cursor += 1;
            }
        }
        out
    } else {
        let mut out: Pieces = [None; MAX_RUNS];
        out[0] = Some(Piece::Map {
            line: 0,
            end: lines.len(),
            left,
            top,
            banks,
        });
        out
    };
    let shown = lines.iter().all(|line| line.mask & mask::BACKGROUND != 0);
    let steady = split.armed || lines.iter().all(|line| background_banks(line) == banks);
    (in_play(game) && shown && steady).then_some(View {
        left,
        top,
        banks,
        pieces,
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
pub(crate) fn tile_at(game: &Game, x: i32, y: i32) -> Option<(u8, u8)> {
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
/// has a pixel to colour (`chr` its patterns). Each [`Piece::Map`] is checked against the live
/// level map; each [`Piece::Run`] is checked by [`check_run`].
///
/// # Errors
/// The first cell that differs otherwise.
#[cfg(test)]
pub(crate) fn check_background(
    view: &View,
    game: &Game,
    lines: &[Line],
    ppu: &Ppu,
    chr: &[u8],
) -> Result<usize, String> {
    let vertical = game.world.orientation & orientation::VERTICAL != 0;
    let mut row_15_palettes = 0;
    for piece in view.pieces.iter().flatten() {
        match *piece {
            Piece::Map {
                line: start,
                end,
                left,
                top,
                banks,
            } => {
                let vis_start = start.max(8);
                let vis_end = end.min(232);
                let tops = lines.iter().enumerate().filter(|&(y, line)| {
                    (vis_start..vis_end).contains(&y) && (y == vis_start || line.v & 0x7000 == 0)
                });
                for (y, line) in tops {
                    for x in cells(line) {
                        let (tile, palette) = nametable_tile(ppu, line, x);
                        let at = (
                            left + i32::try_from(x).expect("a screen x"),
                            top + i32::try_from(y - start).expect("a line"),
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
                                if model.0 == tile
                                    && (model.1 == palette || blank(chr, banks, tile)) => {}
                            Some(model) if model.0 == tile && row_15() == Some(palette) => {
                                row_15_palettes += 1;
                            }
                            _ => {
                                return Err(format!(
                                    "line {y} x {x} (level {at:?}): the map gives {model:02X?}, the nametable {:02X?}",
                                    (tile, palette)
                                ));
                            }
                        }
                    }
                }
            }
            Piece::Run(run) => check_run(&run, lines, ppu)?,
        }
    }
    Ok(row_15_palettes)
}

/// The screen x of each of the line's 8 px cells that starts in x 8-247.
#[cfg(test)]
fn cells(line: &Line) -> impl Iterator<Item = usize> {
    let first = usize::from((8 - line.fine_x % 8) % 8);
    (first..248).step_by(8).filter(|&x| x >= 8)
}

/// Checks the run's TV-visible lines are one run in its banks: each line's fine x is its top's,
/// its fine y follows on, and each cell's tile is [`run_tile`]'s.
///
/// # Errors
/// The first line or cell that differs.
#[cfg(test)]
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
#[cfg(test)]
fn blank(chr: &[u8], banks: [u8; 4], tile: u8) -> bool {
    chr::tile(chr, &banks, usize::from(tile) * 16)
        .iter()
        .all(|&byte| byte == 0)
}

#[cfg(test)]
mod tests {
    use super::super::{Script, raster};
    use super::*;

    fn levels() -> &'static Levels {
        &crate::prg::for_test().expect("the PRG").levels
    }

    /// Stage 1-1's first loop frame and the lines it is drawn with, or `None` without the ROM.
    fn stage_1() -> Option<(Game, Vec<Line>)> {
        let rom = rotj_rom::for_test()?;
        let game = super::super::room::start(crate::prg::for_test()?, &rom.chr, 0);
        let lines = raster::lines(&raster::Registers::of(&game), &game);
        Some((game, lines))
    }

    /// `(is_map, line, end)` for every piece in `view`.
    fn piece_summary(view: &View) -> Vec<(bool, usize, usize)> {
        view.pieces
            .iter()
            .flatten()
            .map(|p| (p.is_map(), p.line(), p.end()))
            .collect()
    }

    #[test]
    fn play_is_modern_and_each_reason_makes_it_classic() {
        let Some((game, lines)) = stage_1() else {
            return;
        };
        let shown = view(&game, levels(), &lines).expect("stage 1-1 in play");
        assert_eq!(piece_summary(&shown), [(true, 0, 240)]);
        let mut script = game.clone();
        script.script = Some(Script::Ending(super::super::ending::Ending::default()));
        let mut split = game.clone();
        split.scroll.split.armed = true;
        split.scroll.split.chain = Chain::FourBands;
        let mut masked = lines.clone();
        masked[100].mask &= !mask::BACKGROUND;
        let mut banked = lines.clone();
        banked[100].chr[0] ^= 1;
        assert_eq!(view(&script, levels(), &lines), None, "a script screen");
        assert_eq!(
            view(&split, levels(), &lines).map(|v| piece_summary(&v)),
            Some(vec![(false, 0, 240)]),
            "a parallax-only band room: every line a run"
        );
        assert_eq!(
            view(&game, levels(), &masked),
            None,
            "the background off on a line"
        );
        assert_eq!(
            view(&game, levels(), &banked),
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
    fn a_d3_4_boss_bar_is_the_map_then_runs_and_bank_split_sprites_stay_modern() {
        let Some((game, lines)) = boss_bar_frame() else {
            return;
        };
        let shown = view(&game, levels(), &lines).expect("a boss bar frame");
        let slot_3 = lines[0].chr[3];
        assert_eq!(
            piece_summary(&shown),
            [(true, 0, 192), (false, 192, 193), (false, 193, 240)]
        );
        assert_eq!(
            match shown.pieces[2] {
                Some(Piece::Run(bar)) => Some((bar.top.v, bar.banks())),
                _ => None,
            },
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
            view(&game, levels(), &lines).map(|v| piece_summary(&v))
        };
        assert_eq!(
            with(&|lines| lines[192].chr = lines[193].chr),
            Some(vec![(true, 0, 192), (false, 192, 240)]),
            "line 192 all the bar's"
        );
        let playfield = lines[0].chr;
        assert_eq!(
            with(&|lines| lines[192..195]
                .iter_mut()
                .for_each(|line| line.chr = playfield)),
            Some(vec![(true, 0, 195), (false, 195, 240)]),
            "the change a row's fourth line in"
        );
        assert_eq!(
            with(&|lines| lines.iter_mut().for_each(|line| line.chr = playfield)),
            Some(vec![(true, 0, 240)]),
            "no bank change: all the map"
        );
        // Bank-split sprites are now sliced in Modern rather than forcing Classic fallback.
        let sprite = game
            .oam
            .shown
            .iter()
            .flat_map(|record| record.sprites(256))
            .find(|entry| entry.is_visible())
            .expect("a sprite on screen");
        let table = 4 * usize::from(sprite.tile & 1);
        assert!(
            with(&|lines| lines[usize::from(sprite.y) + 9].sprite_chr[table] ^= 1).is_some(),
            "a sprite spanning a bank change stays Modern"
        );
    }

    #[test]
    fn check_run_refuses_a_row_or_a_fine_y_off() {
        let Some((_, mut lines)) = boss_bar_frame() else {
            return;
        };
        let (mut v, chr) = (0, lines[0].chr);
        for line in &mut lines {
            (line.v, line.fine_x, line.chr) = (v, 0, chr);
            v = raster::down(v);
        }
        let mut ppu = Ppu::new();
        for table in &mut ppu.nametables {
            for (at, tile) in table.iter_mut().enumerate() {
                *tile = u8::try_from(at % 256).expect("a byte");
            }
        }
        let run = runs(&lines, 0).expect("runs")[0].expect("a run");
        assert_eq!(check_run(&run, &lines, &ppu), Ok(()));
        let mut row_off = run;
        row_off.top.v += 0x20;
        assert!(check_run(&row_off, &lines, &ppu).is_err(), "a row off");
        let mut fine_y_off = run;
        fine_y_off.top.v |= 0x3000;
        assert!(
            check_run(&fine_y_off, &lines, &ppu).is_err(),
            "a fine y off"
        );
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
        let shown = view(&game, levels(), &lines).expect("the final boss's room");
        assert_eq!(
            piece_summary(&shown),
            [(false, 0, 192), (false, 192, 193), (false, 193, 240)],
            "every line from the nametables"
        );
        assert_eq!(
            match shown.pieces[0] {
                Some(Piece::Run(run)) => Some(run.top),
                _ => None,
            },
            Some(lines[0])
        );
        let mut unarmed = game.clone();
        unarmed.scroll.split.armed = false;
        let unarmed_lines = raster::lines(&raster::Registers::of(&unarmed), &unarmed);
        assert_eq!(
            view(&unarmed, levels(), &unarmed_lines).map(|v| piece_summary(&v)),
            Some(vec![(true, 0, 240)]),
            "no split armed: from the map"
        );
        let mut shaken = game.clone();
        shaken.scroll.x = 0xFC;
        let shaken_lines = raster::lines(&raster::Registers::of(&shaken), &shaken);
        assert_eq!(
            view(&shaken, levels(), &shaken_lines).and_then(|v| match v.pieces[0] {
                Some(Piece::Run(run)) => Some((run.top.v & 0x1F, run.top.fine_x)),
                _ => None,
            }),
            Some((31, 4)),
            "a shaken frame's run from coarse x 31"
        );
    }
}
