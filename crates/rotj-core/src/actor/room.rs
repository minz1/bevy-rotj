//! Room changes: the transition (bank 13 `$B7EC`), the loader (`$B40A`), the page
//! draw (`$B351`) and the room parameters (bank 14 `$D992`).

use super::Game;
use super::button::Buttons;
use super::engine::{rom_byte, rom_word};
use super::script::{Flow, Runner, wait};
use super::slots::Slots;
use super::spawn::spawn_row;
use super::stage::Fade;
use super::world::{
    Effects, Motion, Projectile, Schedule, View, VramWrite, World, attack, orientation, power_up,
    set_first_colours, stage_script,
};
use crate::levelmap::Cell;
use crate::levels::RoomAssets;
use crate::prg::Prg;
use crate::render::{Mirroring, ctrl, mask, ppu};
use crate::rooms::{Arrival, Chain, Raster};

/// The room change `$BA = 1` starts, one frame per [`RoomChange::step`] (`$B7EC`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomChange {
    /// The side the new room lies on (`$BB`, kept in `$15`).
    target: u8,
    /// The arrangement `$B927` replaced, restored at the end (`$1F`).
    replaced: Mirroring,
    /// The sprite colours the fade path keeps across the load (`$04F1-$04F3` in `$1A-$1C`).
    kept: [u8; 3],
    phase: Phase,
    /// Where the change is in [`FADE`], [`TURN`] or [`SLIDE`].
    runner: Runner,
    fade: Fade,
    /// The page being drawn, until its last step.
    draw: Option<PageDraw>,
    /// The column of the page beside the first, for a room two nametables wide or tall.
    second: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// The fade through black, the rows of [`FADE`].
    Fade,
    Align,
    /// The vertical room's turn to side by side, the rows of [`TURN`].
    Turn,
    /// The slide into the new room, the rows of [`SLIDE`].
    Slide,
    Done,
}

/// What the room change does, for the fade and for the slide, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoomRow {
    /// The sprite colours kept.
    KeepColours,
    /// A fade, rising or not and streaming the scroll or not, `Done` in its last frame.
    Fade(bool, bool),
    /// Frames the screen stays as it is.
    Wait(u16),
    /// Room + 2 loaded, its first row spawned, the colours back and its first page set up.
    LoadRoom,
    /// The page set up drawn, one step a frame, `Done` the frame after its last.
    Draw,
    /// The page beside the first set up, when the room has one.
    SecondPage,
    /// The arrangement restored and the room's parameters read.
    Restore,
    /// The change over, `$BA` cleared.
    Close,
    /// The camera's page set up.
    TurnPage,
    /// The arrangement turned side by side.
    Flip,
    /// The new room entered.
    Enter,
    /// The new room's first page set up.
    NewPage,
    /// The slide's first step.
    Slide,
    /// The slide's steps until the scroll is 0.
    Sliding,
    /// The new room's first row spawned and its first page set up.
    Spawn,
    /// The player placed and the change over.
    Place,
}

const FADE: &[RoomRow] = &[
    RoomRow::KeepColours,
    RoomRow::Fade(false, true),
    RoomRow::Wait(1),
    RoomRow::LoadRoom,
    RoomRow::Draw,
    RoomRow::SecondPage,
    RoomRow::Draw,
    RoomRow::Restore,
    RoomRow::Fade(true, false),
    RoomRow::Wait(1),
    RoomRow::Close,
];
const TURN: &[RoomRow] = &[RoomRow::TurnPage, RoomRow::Draw, RoomRow::Flip];
const SLIDE: &[RoomRow] = &[
    RoomRow::Enter,
    RoomRow::NewPage,
    RoomRow::Draw,
    RoomRow::Slide,
    RoomRow::Sliding,
    RoomRow::Spawn,
    RoomRow::Draw,
    RoomRow::SecondPage,
    RoomRow::Draw,
    RoomRow::Place,
];

impl RoomChange {
    /// The change toward `target` (`$BB`); with bit 7 set, a fade through black
    /// (`$BA8B`) instead of the slide.
    #[must_use]
    pub fn new(target: u8) -> RoomChange {
        RoomChange {
            target,
            replaced: Mirroring::default(),
            kept: [0; 3],
            phase: if target & 0x80 == 0 {
                Phase::Align
            } else {
                Phase::Fade
            },
            runner: Runner::default(),
            fade: Fade::new(false, true),
            draw: None,
            second: None,
        }
    }

    /// True once the last frame has run and the loop resumes (`$D46C`).
    #[must_use]
    pub fn done(&self) -> bool {
        self.phase == Phase::Done
    }

    /// Runs one frame of the change, up to its NMI wait.
    pub fn step(&mut self, game: &mut Game, prg: &Prg) {
        loop {
            match self.phase {
                Phase::Fade => {
                    if self.table_frame(FADE, game, prg) {
                        return;
                    }
                    self.phase = Phase::Done;
                }
                Phase::Align => {
                    if self.align(game, prg) {
                        return;
                    }
                    self.begin(Phase::Slide);
                }
                Phase::Turn => {
                    if self.table_frame(TURN, game, prg) {
                        return;
                    }
                    self.begin(Phase::Slide);
                }
                Phase::Slide => {
                    if self.table_frame(SLIDE, game, prg) {
                        return;
                    }
                    self.phase = Phase::Done;
                }
                Phase::Done => return,
            }
        }
    }

    /// Starts `phase` at the top of its table.
    fn begin(&mut self, phase: Phase) {
        self.phase = phase;
        self.runner = Runner::default();
    }

    /// Runs `rows` up to the frame's end (true) or until they run out.
    fn table_frame(&mut self, rows: &[RoomRow], game: &mut Game, prg: &Prg) -> bool {
        while let Some(&row) = self.runner.row(rows) {
            let frame = self.runner.frame;
            let flow = match row {
                RoomRow::KeepColours => {
                    self.kept.copy_from_slice(&game.effects.palette[17..20]);
                    Flow::Done
                }
                RoomRow::Fade(rising, streams) => {
                    if frame == 0 {
                        self.fade = Fade::new(rising, streams);
                    }
                    if self.fade.step(game, prg) {
                        Flow::Done
                    } else {
                        Flow::Waited
                    }
                }
                RoomRow::Wait(count) => wait(frame, count),
                RoomRow::LoadRoom => {
                    self.load_room(game, prg);
                    Flow::Done
                }
                RoomRow::Draw => match &mut self.draw {
                    Some(draw) => {
                        if draw.step(game) {
                            self.draw = None;
                        }
                        Flow::Waited
                    }
                    None => Flow::Done,
                },
                RoomRow::SecondPage => {
                    if let Some(column) = self.second.take() {
                        self.draw = Some(second_page(game, column));
                    }
                    Flow::Done
                }
                RoomRow::Restore => {
                    game.scroll.ppu_ctrl &= !ctrl::NAMETABLE;
                    game.scroll.mirroring = self.replaced;
                    parameters(game, prg);
                    game.oam.finish();
                    Flow::Done
                }
                RoomRow::Close => {
                    game.effects.transition = 0;
                    game.effects.transition_arg = 0;
                    Flow::Done
                }
                RoomRow::TurnPage => {
                    self.draw = Some(turn_page(game));
                    Flow::Done
                }
                RoomRow::Flip => {
                    if frame == 0 {
                        flip(game);
                    }
                    wait(frame, 1)
                }
                RoomRow::Enter => {
                    if frame == 0 {
                        self.replaced = enter(game, prg, self.target);
                    }
                    wait(frame, 1)
                }
                RoomRow::NewPage => {
                    self.draw = Some(new_page(game));
                    Flow::Done
                }
                RoomRow::Slide => {
                    if frame == 0 {
                        self.slide(game, prg);
                    }
                    wait(frame, 1)
                }
                RoomRow::Sliding => {
                    if game.scroll.scroll_x | game.scroll.scroll_y == 0 {
                        Flow::Done
                    } else {
                        self.slide(game, prg);
                        Flow::Waited
                    }
                }
                RoomRow::Spawn => {
                    spawn_first_row(game, prg);
                    self.set_pages(game);
                    Flow::Done
                }
                RoomRow::Place => {
                    place(game, prg, self.replaced);
                    Flow::Done
                }
            };
            if self.runner.advance(flow) {
                return true;
            }
        }
        false
    }

    /// The fade path's load: room + 2, its first row spawned, the kept colours back and
    /// its pages set up.
    fn load_room(&mut self, game: &mut Game, prg: &Prg) {
        game.world.room = game.world.room.wrapping_add(2);
        load(game, prg);
        self.replaced = game.scroll.mirroring;
        spawn_first_row(game, prg);
        game.effects.palette[17..20].copy_from_slice(&self.kept);
        self.set_pages(game);
    }

    /// The new room's first page set up to draw, and the page beside it to follow.
    fn set_pages(&mut self, game: &Game) {
        let (draw, second) = page_draws(game);
        self.draw = Some(draw);
        self.second = second;
    }

    /// One frame of the camera's move to a page edge (`$B7F3-$B8D9`), or the step
    /// past it; true when the frame ended.
    fn align(&mut self, game: &mut Game, prg: &Prg) -> bool {
        game.oam.next = 0;
        game.work.anchor = 0;
        let [cy, fy] = game.world.camera_y.to_be_bytes();
        let [cx, fx] = game.world.camera_x.to_be_bytes();
        if (cy & 0x0F) | fy == 0 && (cx & 0x0F) | fx == 0 {
            if game.world.orientation & orientation::VERTICAL != 0 {
                game.scroll.mirroring = Mirroring::SideBySide;
                self.begin(Phase::Turn);
                return true;
            }
            return false;
        }
        game.scroll.speed_y = align_speed(game.world.camera_y);
        game.scroll.speed_x = align_speed(game.world.camera_x);
        super::scroll::step(game, prg);
        let player = &mut game.effects.player;
        player.screen_x = drift(player.screen_x, game.scroll.speed_x);
        player.screen_y = drift(player.screen_y, game.scroll.speed_y);
        game.work.screen_x = player.screen_x;
        game.work.screen_y = player.screen_y;
        game.draw_player(prg, false);
        game.oam.finish();
        true
    }

    /// One 16 px step of the slide into the new room (`$B95D-$B9FA`).
    fn slide(&self, game: &mut Game, prg: &Prg) {
        game.oam.next = 0;
        let scroll = &mut game.scroll;
        let player = &mut game.effects.player;
        if self.target & 3 != 0 {
            let wrapped = if self.target & 1 != 0 {
                player.screen_x = player.screen_x.wrapping_sub(0x10).max(0x20);
                let (x, carry) = scroll.scroll_x.overflowing_add(0x10);
                scroll.scroll_x = x;
                carry
            } else {
                player.screen_x = player.screen_x.wrapping_add(0x10).min(0xD0);
                let (x, borrow) = scroll.scroll_x.overflowing_sub(0x10);
                scroll.scroll_x = x;
                borrow
            };
            if wrapped {
                scroll.ppu_ctrl ^= ctrl::NAMETABLE_X;
            }
        } else if self.target & 8 == 0 {
            player.screen_y = player.screen_y.wrapping_sub(0x10).max(0x40);
            let y = scroll.scroll_y.wrapping_add(0x10);
            scroll.scroll_y = if y >= 0xF0 {
                scroll.ppu_ctrl ^= ctrl::NAMETABLE_Y;
                0
            } else {
                y
            };
        } else {
            player.screen_y = player.screen_y.wrapping_add(0x10).min(0xD0);
            let y = scroll.scroll_y.wrapping_sub(0x10);
            scroll.scroll_y = if y >= 0xF0 {
                scroll.ppu_ctrl ^= ctrl::NAMETABLE_Y;
                0xE0
            } else {
                y
            };
        }
        game.work.screen_x = player.screen_x;
        game.work.screen_y = player.screen_y;
        game.draw_player(prg, false);
        game.oam.finish();
        super::frame::animate_chr(game);
    }
}

/// The new room's camera page drawn into the first nametable, and for a room two
/// nametables across (`$DD` bit 7) the column of the page beside it (`$BA0B-$BA51`).
fn page_draws(game: &Game) -> (PageDraw, Option<u8>) {
    let orientation = game.world.orientation;
    let camera = if orientation & orientation::VERTICAL != 0 {
        game.world.camera_y
    } else {
        game.world.camera_x
    };
    let page = usize::from(camera >> 12);
    let (first, second) = (
        game.schedule.page_columns[page],
        game.schedule.page_columns[page + 16],
    );
    let (first, second) = if orientation::horizontal_two_page(orientation) {
        (second, first)
    } else {
        (first, second)
    };
    let second = (orientation & orientation::TWO_PAGES != 0).then_some(second);
    (
        PageDraw::new(first, 0x60, 0x20, game.scroll.mirroring),
        second,
    )
}

/// The second page of a room two nametables wide or tall (`$BA3C-$BA51`, `$BB18-$BB2D`):
/// into `$2400`, from map row 1 when the room is tall.
fn second_page(game: &mut Game, column: u8) -> PageDraw {
    game.scroll.ppu_ctrl &= !ctrl::NAMETABLE;
    let row = if game.world.orientation & orientation::VERTICAL == 0 {
        0x61
    } else {
        0x60
    };
    PageDraw::new(column, row, 0x24, game.scroll.mirroring)
}

/// The vertical room's camera page, drawn again before the arrangement turns.
fn turn_page(game: &Game) -> PageDraw {
    let page = usize::from(game.world.camera_y >> 12)
        + usize::from(game.scroll.ppu_ctrl & ctrl::NAMETABLE_X) * 16;
    PageDraw::new(
        game.schedule.page_columns[page],
        0x60,
        table(game.scroll.ppu_ctrl),
        game.scroll.mirroring,
    )
}

/// The new room's first page, the slide's way in.
fn new_page(game: &Game) -> PageDraw {
    PageDraw::new(
        game.scroll.start,
        0x60,
        table(game.scroll.ppu_ctrl),
        game.scroll.mirroring,
    )
}

/// The arrangement turned side by side for the slide out of a vertical room.
fn flip(game: &mut Game) {
    let scroll = &mut game.scroll;
    let flipped = scroll.ppu_ctrl ^ ctrl::NAMETABLE_Y;
    scroll.scroll_y = 0;
    scroll.mirroring = Mirroring::SideBySide;
    scroll.ppu_ctrl = (flipped & !ctrl::NAMETABLE) | ((flipped & ctrl::NAMETABLE_Y) >> 1);
}

/// Restores the arrangement, places the player and reads the room's parameters, and
/// ends the change (`$BA3C-$BA88`, `$D4EF`).
fn place(game: &mut Game, prg: &Prg, replaced: Mirroring) {
    game.scroll.ppu_ctrl &= !ctrl::NAMETABLE;
    game.scroll.mirroring = replaced;
    let player = &mut game.effects.player;
    let [x, _] = player.x.to_be_bytes();
    let [y, _] = player.y.to_be_bytes();
    let (sx, sy) = (player.screen_x, player.screen_y);
    player.x = u16::from_be_bytes([x | (sx >> 4), sx << 4]);
    player.y = u16::from_be_bytes([y | (sy >> 4), (sy << 4) | 0x0F]);
    player.vy = 6;
    game.effects.busy = 0;
    parameters(game, prg);
    game.effects.transition = 0;
    game.effects.transition_arg = 0;
}

/// The nametable `$B351` draws a page into for the scroll's arrangement (`$B8E7`, `$B945`).
fn table(ppu_ctrl: u8) -> u8 {
    if ppu_ctrl & ctrl::NAMETABLE != 0 {
        0x20
    } else {
        0x24
    }
}

/// The camera speed toward the nearest 256 px page edge (`$B80C-$B865`).
fn align_speed(camera: u16) -> i16 {
    let [page, fraction] = camera.to_be_bytes();
    match page & 0x0F {
        0 if fraction < 0x40 => -i16::from(fraction),
        0..8 => -0x40,
        _ => 0x40,
    }
}

/// A screen coordinate moved against the camera's step (`$B874-$B8CE`).
fn drift(screen: u8, speed: i16) -> u8 {
    let moved = (u16::from(screen) << 4).wrapping_sub(speed.cast_unsigned());
    (moved >> 4).to_le_bytes()[0]
}

/// The sixteen-frame draw of one 256 px page into a nametable (`$B351`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageDraw {
    /// The page's base map column (`$20`).
    pub column: u8,
    /// The next map row, `$60`-based (`$21`).
    pub row: u8,
    /// The next PPU address (`$22:$23`).
    pub addr: u16,
}

impl PageDraw {
    /// Starts drawing the page at map column `column` from map row `row` into the
    /// nametable at `table` (`$20`, `$24`, `$28`), as `$B351` sets up.
    #[must_use]
    pub fn new(column: u8, row: u8, table: u8, mirroring: Mirroring) -> PageDraw {
        let table = if table != 0x20 && mirroring != Mirroring::SideBySide {
            0x28
        } else {
            table
        };
        PageDraw {
            column: column & 0xF0,
            row,
            addr: u16::from_be_bytes([table, 0]),
        }
    }

    /// One frame of the draw: a metatile row, or the attributes once the rows are
    /// done; true when the page is finished.
    pub fn step(&mut self, game: &mut Game) -> bool {
        let [hi, lo] = self.addr.to_be_bytes();
        if hi & 0x23 == 0x23 && lo == 0xC0 {
            let page = usize::from(self.column >> 4) * 64;
            let attributes = &game.effects.level.attributes[page..page + 64];
            let mirror = if hi & 0xFC == 0x20 { 0 } else { 64 };
            let vram = &mut game.effects.vram;
            vram.attributes[mirror..mirror + 64].copy_from_slice(attributes);
            vram.push(self.addr, 0, attributes);
            super::frame::animate_chr(game);
            return true;
        }
        let level = &game.effects.level;
        let mut bytes = [0u8; 64];
        for column in 0..16u8 {
            let cell = Cell::in_page(self.row & 0x0F, self.column, column);
            let corners = level.tiles(level.map[cell]);
            let at = usize::from(column) * 2;
            bytes[at..at + 2].copy_from_slice(&corners[..2]);
            bytes[32 + at..34 + at].copy_from_slice(&corners[2..]);
        }
        game.effects.vram.push(self.addr, 0, &bytes);
        super::frame::animate_chr(game);
        self.row = self.row.wrapping_add(1);
        self.addr = self.addr.wrapping_add(0x40);
        false
    }
}

/// Room `$0402 = room` of that stage.
pub(crate) fn room_assets(prg: &Prg, area: u8, room: u8) -> &RoomAssets {
    &super::stage::assets(&prg.levels, area).rooms[usize::from(room >> 1)]
}

/// Loads room `$0402` of area `$0400`: clears the slots, sets the palette, CHR
/// banks, orientation, page order, camera, player start and schedule (`$B40A`).
pub fn load(game: &mut Game, prg: &Prg) {
    let (area, room) = (game.world.area, game.world.room);
    game.slots = Slots::default();
    game.effects.projectiles = [Projectile::default(); 16];
    game.effects.pause_name = [0; 18];
    game.effects.pause_aux = 0;
    let layout = &room_assets(prg, area, room).room;
    let kept: [u8; 3] = game.effects.palette[17..20].try_into().expect("3 bytes");
    build_palette(
        prg,
        layout.background_palette,
        layout.sprite_palette,
        &mut game.effects.palette,
    );
    game.effects.palette[17..20].copy_from_slice(&kept);
    set_first_colours(&mut game.effects.palette, 0x0F);
    game.oam.chr[..4].copy_from_slice(&layout.chr);
    game.world.orientation = layout.orientation;
    animate_banks(game, prg);
    game.schedule.page_columns = layout.pages.map(|page| page << 4);
    if orientation::horizontal_two_page(game.world.orientation) {
        raise_attributes(
            &mut game.effects.level,
            &game.schedule.page_columns,
            layout.raised_page_count,
        );
    }
    let start = layout.start;
    let player = &mut game.effects.player;
    player.x = start.player_x;
    player.y = start.player_y;
    game.world.camera_x = start.camera_x;
    game.world.camera_y = start.camera_y;
    game.scroll.ring_y = start.ring_y;
    game.effects.scroll_lock = start.scroll_lock;
    game.scroll.mode = start.scroll_mode;
    game.scroll.mirroring = start.mirroring;
    game.scroll.start = start.first_spawn_column;
    player.facing = start.facing;

    game.scroll.gate = 0xFF;
    game.effects.vram.origin = ppu::nametable(0);
    game.effects.attack &= attack::CHARGEABLE;
    player.vx = 0;
    player.motion = Motion::Standing;
    player.pose = 0;
    player.hurt = 0;
    player.sliding = 0;
    player.death_flags = 0;
    player.stage_script = 0;
    player.pose_timer = 0;
    player.pose_step = 0;
    game.oam.start = 0;
    game.scroll.step_x = 0;
    game.scroll.step_y = 0;
    game.scroll.scroll_x = 0;
    game.scroll.scroll_y = 0;
    game.effects.palette_dirty = 0;
    game.scroll.speed_x = 0;
    game.scroll.speed_y = 0;
    game.schedule.cells = super::stage::assets(&prg.levels, area).schedule;
    let player = &mut game.effects.player;
    player.carrier = 0;
    player.vy = 0;
    player.attack_timer = 0;
}

/// Moves the first `pages` pages' attributes up a metatile row, for a room two
/// nametables tall whose lower page is drawn from map row 1 (`$B586`).
fn raise_attributes(level: &mut crate::collision::Level, rows: &[u8; 32], pages: u8) {
    for &column in &rows[..usize::from(pages)] {
        let page = usize::from(column >> 4) * 64;
        for at in page..page + 64 {
            let below = level
                .attributes
                .get(at + 8)
                .copied()
                .unwrap_or_else(|| level.unread_at(0x600 + at + 8).expect("$7A00-$7A07"));
            level.attributes[at] = (level.attributes[at] >> 4) | (below << 4);
        }
    }
}

/// The game at the first loop frame of `level` (`$20`, 0-12): the new-game state
/// (`$D48D`), the level and its first room (bank 13 `$B200`), the first page's writes
/// in [`Vram::direct`](super::world::Vram::direct).
///
/// # Panics
/// If `level` is past the stages' checkpoints or its data fails to decode.
#[must_use]
pub fn start(prg: &Prg, chr: &[u8], level: usize) -> Game {
    let area = super::stage::level_area(&prg.levels, level).expect("a level the stages have");
    let mut game = Game {
        slots: Slots::default(),
        world: World {
            camera_x: 0,
            camera_y: 0,
            view: View {
                width: 256 * 16,
                height: 240 * 16,
            },
            orientation: 0,
            rng: [0x49, 0x2F],
            frame_odd: false,
            frame: 0,
            area,
            room: 0,
            pad: Buttons::NONE,
            pressed: Buttons::NONE,
            spawn_distance: 0,
        },
        schedule: Schedule {
            cells: [0; 256],
            page_columns: [0; 32],
        },
        effects: Effects {
            meter: 0xFE,
            lives: 2,
            ..Effects::default()
        },
        oam: super::oam::OamBuilder::new(),
        work: super::engine::Work::default(),
        scroll: super::scroll::Scroll {
            split: INTRO_SPLIT,
            bands: INTRO_BANDS,
            ..super::scroll::Scroll::default()
        },
        display: super::frame::Display {
            prg_windows: [0xC0, 0x1E],
            ..super::frame::Display::default()
        },
        script: None,
        probe: crate::actor::collision::Probe::default(),
    };
    game.effects.player.energy = 0x0E;
    enter_area(&mut game, prg, chr, Leftover::Boot);
    // shortcut: Batman's sprite banks as stage 1-1 starts with them; the player draw sets them.
    game.oam.chr[5..8].copy_from_slice(&[0xF5, 0xF6, 0xF7]);
    game.scroll.ppu_ctrl = ctrl::NMI | ctrl::TALL_SPRITES | ctrl::SPRITE_PATTERNS_HIGH;
    game.scroll.mask = mask::RENDERING;
    parameters(&mut game, prg);
    game
}

/// The raster header `$D0-$D4` as the intro leaves it: its setup (WRAM `$604C-$6060`)
/// and IRQ chain (bank 2 `$96AF-$972C`). An area with no raster split sets only `$D0`
/// (`$D9B5`), so the first area after boot starts with the rest of these and [`INTRO_BANDS`].
const INTRO_SPLIT: Raster = Raster {
    armed: false,
    first_irq_cycles: 0x2680,
    chain: Chain::Intro,
    next_irq: 0,
};
/// The raster bands `$D5-$DC` as the intro leaves them.
const INTRO_BANDS: [u8; 8] = [0, 0, 0, 0, 0, 0x1F, 0, 0];

/// What WRAM `$7000-$7FFF` holds when an area's level loads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leftover {
    /// The intro and title's payload: the first load after boot.
    Boot,
    /// A cutscene's payload.
    Cutscene,
    /// The last area's state.
    LastArea,
}

/// The area's start up to its first frame's NMI wait (`$B203-$B22C`, the level from
/// `$BD5E`): the level, the room loader, the sprite palette, the first page (queued in
/// [`Vram::direct`](super::world::Vram::direct)), the player's screen position
/// (`$BF28`), the room's spawns (`$D714`) and the kept weapon, over what `leftover`
/// says WRAM holds.
///
/// # Panics
/// If the area's level or room data fails to decode.
pub fn enter_area(game: &mut Game, prg: &Prg, chr: &[u8], leftover: Leftover) {
    load_level(game, prg, chr, leftover);
    load(game, prg);
    for (i, entry) in game.effects.palette[17..20].iter_mut().enumerate() {
        *entry = rom_byte(prg, 13, 0xB26A + u16::try_from(i).expect("< 3"));
    }
    let page = first_page(game);
    game.effects.vram.direct.extend(page);
    let player = &mut game.effects.player;
    player.screen_x = (player.x >> 4).to_le_bytes()[0];
    player.screen_y = (player.y >> 4).to_le_bytes()[0];
    spawn_first_row(game, prg);
    if game.effects.player.power_up & power_up::ON != 0 {
        game.effects.player.power_up = 0;
        game.effects.weapon = game.effects.stored_weapon;
    }
    game.oam.chr[4] = 0xA0;
}

/// The area's level, room 0 (`$0402 = 0`, bank 2 `$89FE`, `$8B99`, `$89C1`): the
/// map, tile tables, classes and attributes, and the second nametable's attribute
/// shadow the stream leaves in `$7570`. The loader decompresses over `$7000-$7FFF` as
/// it finds them, over what `leftover` says WRAM holds, and keeps what its stream does
/// not reach.
///
/// # Panics
/// If the area's level fails to decode.
pub fn load_level(game: &mut Game, prg: &Prg, chr: &[u8], leftover: Leftover) {
    game.world.room = 0;
    let image = match leftover {
        Leftover::Boot => crate::levelmap::boot_image(chr).expect("the boot payload's CHR"),
        Leftover::Cutscene => crate::levelmap::payload_image(chr).expect("the payload's CHR"),
        Leftover::LastArea => wram_7000(game),
    };
    game.effects.level = crate::collision::Level::from_terrain(
        &super::stage::assets(&prg.levels, game.world.area).terrain,
        image,
    );
    let level = &game.effects.level;
    for (i, shadow) in game.effects.vram.attributes[64..].iter_mut().enumerate() {
        *shadow = level.streamed(0x570 + i);
    }
}

/// WRAM `$7000-$7FFF` as the port's state holds it; the slot area's bytes no array
/// owns are zero, as every room load leaves them (`$B5E2`). The zeroed unowned slot
/// bytes rely on the room load (`$B40A`/`$B5E2`) clearing `$7B00-$7FFF`.
fn wram_7000(game: &Game) -> [u8; 0x1000] {
    use crate::collision::CLASS_COUNT;
    let mut image = [0; 0x1000];
    let level = &game.effects.level;
    for (id, corners) in level.corners.iter().enumerate() {
        for (table, &corner) in corners.iter().enumerate() {
            image[table * CLASS_COUNT + id] = corner;
        }
    }
    image[0x340..0x410].copy_from_slice(&level.classes);
    image[0x410..0x510].copy_from_slice(&game.schedule.cells);
    image[0x530..0x5B0].copy_from_slice(&game.effects.vram.attributes);
    image[0x600..0xA00].copy_from_slice(&level.attributes);
    level.unpack_unread(&mut image);
    for (addr, byte) in (0x7B00..).zip(&mut image[0xB00..]) {
        *byte = super::slots::owned(&game.slots, &game.effects.projectiles, addr).unwrap_or(0);
    }
    image[0xFD0..0xFE2].copy_from_slice(&game.effects.pause_name);
    image[0xFF1] = game.effects.pause_aux;
    image
}

/// Spawns the room's first schedule row (`$D714`): from `$0403`, or in a room two
/// nametables tall from `$0440[(camera x page & 15) << 4]`, as the game indexes it.
///
/// # Panics
/// If that index runs past the page order, or the schedule names a record the room lacks.
pub fn spawn_first_row(game: &mut Game, prg: &Prg) {
    let table = &room_assets(prg, game.world.area, game.world.room).spawns;
    let start = if orientation::horizontal_two_page(game.world.orientation) {
        let page = game.world.camera_x.to_be_bytes()[0] & 0x0F;
        *game
            .schedule
            .page_columns
            .get(usize::from(page) << 4)
            .expect("$D73C reads past the page order $0440-$045F")
    } else {
        game.scroll.start
    };
    spawn_row(&mut game.slots, &mut game.schedule, table, start);
}

/// The nametable writes of `$B26D`: the camera's page, drawn straight to the PPU
/// at the room start, and its attributes kept in the queue's shadow (`$7530`).
///
/// # Panics
/// For a room with `$DD` bit 7 (two pages), which is not ported.
pub fn first_page(game: &mut Game) -> Vec<VramWrite> {
    let orientation = game.world.orientation;
    assert!(
        orientation & orientation::TWO_PAGES == 0,
        "a two-page room start ($B2A6) is not ported"
    );
    let camera = if orientation & orientation::VERTICAL != 0 {
        game.world.camera_y
    } else {
        game.world.camera_x
    };
    let column = game.schedule.page_columns[usize::from(camera >> 12)] & 0xF0;
    let level = &game.effects.level;
    let mut writes: Vec<VramWrite> = (0..15u8)
        .map(|row| {
            let mut bytes = [0u8; 64];
            for i in 0..16u8 {
                let tiles = level.tiles(level.map[Cell::in_page(row, column, i)]);
                let at = usize::from(i) * 2;
                bytes[at..at + 2].copy_from_slice(&tiles[..2]);
                bytes[32 + at..34 + at].copy_from_slice(&tiles[2..]);
            }
            VramWrite {
                addr: ppu::tile_addr(0, u16::from(row) * 2, 0),
                flags: 0,
                bytes: bytes.to_vec(),
            }
        })
        .collect();
    let page = usize::from(column >> 4) * 64;
    let attributes = &level.attributes[page..page + 64];
    game.effects.vram.attributes[..64].copy_from_slice(attributes);
    writes.push(VramWrite {
        addr: ppu::attribute_addr(0, 0, 0),
        flags: 0,
        bytes: attributes.to_vec(),
    });
    writes
}

/// The palette from a background and a sprite set (`$C367`).
pub(crate) fn build_palette(prg: &[u8], background: u8, sprites: u8, palette: &mut [u8; 32]) {
    for set in (0..8u16).rev() {
        let table = rom_word(prg, 14, 0xC3A6 + 2 * set);
        let pick = if set >= 4 { sprites } else { background };
        let entry = u16::from(rom_byte(prg, 14, table + u16::from(pick)));
        let at = usize::from(set) * 4 + 1;
        palette[at] = rom_byte(prg, 14, 0xC4BA + entry);
        palette[at + 1] = rom_byte(prg, 14, 0xC522 + entry);
        palette[at + 2] = rom_byte(prg, 14, 0xC58A + entry);
    }
}

/// Sets up the CHR shadows whose bank byte has bit 7 set as animations (`$B47D`).
fn animate_banks(game: &mut Game, prg: &[u8]) {
    let display = &mut game.display;
    let animated = &mut game.effects.animated;
    *animated = 0;
    for shadow in (0..4).rev() {
        let bank = game.oam.chr[shadow];
        *animated = (*animated << 1) | (bank >> 7);
        if bank & 0x80 == 0 {
            continue;
        }
        let byte = animation_byte(prg, bank);
        let frames = byte(0);
        display.timers[shadow] = [byte(1), frames, 0, 0];
        for frame in 0..frames {
            display.banks[shadow * 8 + usize::from(frame)] = byte(2 + u16::from(frame));
        }
        game.oam.chr[shadow] = loaded_bank(prg, bank);
    }
}

/// The bank a room loads for `bank`: an animation handle's (bit 7) last frame.
pub(crate) fn loaded_bank(prg: &[u8], bank: u8) -> u8 {
    if bank & 0x80 == 0 {
        return bank;
    }
    let byte = animation_byte(prg, bank);
    match byte(0) {
        0 => bank,
        frames => byte(1 + u16::from(frames)),
    }
}

/// The bytes of animation handle `bank`'s record: its frame count, timer and frames.
fn animation_byte(prg: &[u8], bank: u8) -> impl Fn(u16) -> u8 + '_ {
    let record = rom_word(prg, 14, 0xC06C + u16::from((bank << 1) & 0x7F));
    move |i| rom_byte(prg, 0, record + i)
}

/// The room's parameters, the stream masks and the room's player setup (`$D992`).
pub fn parameters(game: &mut Game, prg: &Prg) {
    let layout = &room_assets(prg, game.world.area, game.world.room).room;
    let scroll = &mut game.scroll;
    let flag = |set: bool| if set { 0x80 } else { 0 };
    let Some(raster) = layout.raster else {
        scroll.split.armed = false;
        scroll.fixed_rows = [0; 16];
        scroll.fixed_quads = [0; 8];
        return;
    };
    scroll.split = raster;
    scroll.bands = [0; 8];
    scroll.fixed_rows = layout.fixed_rows.map(flag);
    scroll.fixed_quads = layout.fixed_quads.map(flag);
    let player = &mut game.effects.player;
    match layout.arrival {
        Arrival::Unchanged => {}
        Arrival::Held => {
            (player.motion, player.stage_script) = (
                Motion::Held,
                stage_script::FROZEN | stage_script::HIDDEN | stage_script::KEEP_PADS,
            );
        }
        Arrival::Flying => {
            (player.anim, player.motion, player.stage_script) = (
                0x85,
                Motion::Flying,
                stage_script::FROZEN | stage_script::KEEP_PADS,
            );
        }
    }
}

/// Enters the next room of the area (`$B927-$B942`): the loader, the arrangement for
/// the slide toward `target`'s side, and the palette upload; the arrangement it replaced.
pub fn enter(game: &mut Game, prg: &Prg, target: u8) -> Mirroring {
    game.world.room = game.world.room.wrapping_add(2);
    load(game, prg);
    let replaced = game.scroll.mirroring;
    game.scroll.mirroring = if target.is_multiple_of(4) {
        Mirroring::Stacked
    } else {
        Mirroring::SideBySide
    };
    game.effects.vram.push_palette(&game.effects.palette);
    replaced
}

#[cfg(test)]
mod tests {
    use super::super::oracle::{Scene, State, assert_routine};
    use super::{PageDraw, enter, raise_attributes, start};

    fn page_frames(start: u64, frames: u64) -> usize {
        let Some(mut scene) = Scene::tas(start) else {
            return usize::try_from(frames).expect("small");
        };
        assert_routine(&mut scene, frames, (0xC23F, 14), &[0xC232], |call| {
            let ram = &call.begin.ram;
            let mut draw = PageDraw {
                column: ram[0x20],
                row: ram[0x21].wrapping_add(1),
                addr: u16::from_be_bytes([ram[0x22], ram[0x23]]).wrapping_add(0x40),
            };
            draw.step(&mut call.state);
        })
    }

    #[test]
    fn page_draw_matches_b351() {
        assert_eq!(page_frames(614, 15), 15);
        assert_eq!(page_frames(646, 15), 15);
        assert_eq!(page_frames(1256, 15), 15);
        assert_eq!(page_frames(1274, 15), 15);
    }

    #[test]
    fn entering_matches_b927() {
        let Some(mut scene) = Scene::tas(1272) else {
            return;
        };
        let checked = assert_routine(&mut scene, 1, (0xC23F, 14), &[0xC232], |call| {
            let target = call.begin.ram[0x15];
            enter(&mut call.state, call.prg, target);
        });
        assert_eq!(checked, 1);
    }

    /// `$B586`: each page's attribute row takes the nybble from the row below.
    #[test]
    fn raise_attributes_shifts_a_page_up_one_row() {
        let mut level = crate::collision::Level::default();
        level.attributes[0] = 0x12;
        level.attributes[8] = 0x34;
        let mut rows = [0u8; 32];
        rows[0] = 0x00;
        raise_attributes(&mut level, &rows, 1);
        assert_eq!(level.attributes[0], 0x41);
    }

    #[test]
    fn the_stage_start_matches_the_first_loop_frame() {
        let Some(mut scene) = Scene::tas(350) else {
            return;
        };
        let prg = Scene::prg();
        let chr = scene.machine.bus.cart.chr.clone();
        let first = scene.run(1, &[0xD3F9]).remove(0);
        let oracle = State::lift(&first);
        let mut port = start(prg, &chr, 0);
        port.world.rng = oracle.world.rng;
        port.world.frame = oracle.world.frame;
        port.world.frame_odd = oracle.world.frame_odd;
        port.world.pad = oracle.world.pad;
        port.world.pressed = oracle.world.pressed;
        assert_eq!(port.slots, oracle.slots);
        assert_eq!(port.world, oracle.world);
        assert_eq!(port.schedule, oracle.schedule);
        assert_eq!(port.effects.player, oracle.effects.player);
        assert_eq!(port.scroll, oracle.scroll);
        assert_eq!(port.display, oracle.display);
        assert_eq!(port.oam.chr, oracle.oam.chr);
        assert!(
            port.effects.level == oracle.effects.level,
            "the level differs"
        );
        assert_eq!(
            port.effects.vram.attributes[64..],
            oracle.effects.vram.attributes[64..]
        );
    }
}
