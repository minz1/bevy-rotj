//! Room changes: the transition (bank 13 `$B7EC`), the loader (`$B40A`), the page
//! draw (`$B351`) and the room parameters (bank 14 `$D992`).

use super::Game;
use super::engine::{rom_byte, rom_word};
use super::slots::Slots;
use super::spawn::{SpawnTable, spawn_row};
use super::stage::Fade;
use super::world::{
    Effects, Motion, Projectile, Schedule, View, VramWrite, World, orientation, set_first_colours,
};
use crate::actors::spawn_schedule;
use crate::collision::rows_from_rom;

/// The room change `$BA = 1` starts, one frame per [`RoomChange::step`] (`$B7EC`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomChange {
    /// The side the new room lies on (`$BB`, kept in `$15`).
    target: u8,
    /// The arrangement `$B927` replaced, restored at the end (`$1F`).
    replaced: u8,
    /// The sprite colours the fade path keeps across the load (`$04F1-$04F3` in `$1A-$1C`).
    kept: [u8; 3],
    phase: Phase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    FadeStart,
    FadeOut(Fade),
    FadeLoad,
    FadeEnter,
    FadeIn(Fade),
    Close,
    Align,
    Turn,
    Draw(PageDraw, Then),
    Flip,
    Enter,
    NewPage,
    Slide,
    Sliding,
    Spawn,
    SecondPage(u8),
    Place,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Then {
    Flip,
    Slide,
    SecondPage(u8),
    Place,
    FadeIn,
}

impl RoomChange {
    /// The change toward `target` (`$BB`); with bit 7 set, a fade through black
    /// (`$BA8B`) instead of the slide.
    #[must_use]
    pub fn new(target: u8) -> RoomChange {
        RoomChange {
            target,
            replaced: 0,
            kept: [0; 3],
            phase: if target & 0x80 == 0 {
                Phase::Align
            } else {
                Phase::FadeStart
            },
        }
    }

    /// True once the last frame has run and the loop resumes (`$D46C`).
    #[must_use]
    pub fn done(&self) -> bool {
        self.phase == Phase::Done
    }

    /// Runs one frame of the change, up to its NMI wait.
    pub fn step(&mut self, game: &mut Game, prg: &[u8]) {
        loop {
            match self.phase {
                Phase::FadeStart
                | Phase::FadeOut(_)
                | Phase::FadeLoad
                | Phase::FadeEnter
                | Phase::FadeIn(_)
                | Phase::Close => {
                    if self.fade(game, prg) {
                        return;
                    }
                }
                Phase::Align => {
                    if self.align(game, prg) {
                        return;
                    }
                }
                Phase::Turn => {
                    let page = usize::from(game.world.camera_y >> 12)
                        + usize::from(game.scroll.ppu_ctrl & 1) * 16;
                    let draw = PageDraw::new(
                        game.schedule.rows[page],
                        0x60,
                        table(game.scroll.ppu_ctrl),
                        game.scroll.mirroring,
                    );
                    self.phase = Phase::Draw(draw, Then::Flip);
                }
                Phase::Draw(mut draw, then) => {
                    let finished = draw.step(game);
                    self.phase = match (finished, then) {
                        (false, _) => Phase::Draw(draw, then),
                        (true, Then::Flip) => Phase::Flip,
                        (true, Then::Slide) => Phase::Slide,
                        (true, Then::SecondPage(column)) => Phase::SecondPage(column),
                        (true, Then::Place) => Phase::Place,
                        (true, Then::FadeIn) => Phase::FadeEnter,
                    };
                    return;
                }
                Phase::Flip => {
                    let scroll = &mut game.scroll;
                    let flipped = scroll.ppu_ctrl ^ 2;
                    scroll.scroll_y = 0;
                    scroll.mirroring = 0;
                    scroll.ppu_ctrl = (flipped & 0xFC) | ((flipped & 2) >> 1);
                    self.phase = Phase::Enter;
                    return;
                }
                Phase::Enter => {
                    self.replaced = enter(game, prg, self.target);
                    self.phase = Phase::NewPage;
                    return;
                }
                Phase::NewPage => {
                    let draw = PageDraw::new(
                        game.scroll.start,
                        0x60,
                        table(game.scroll.ppu_ctrl),
                        game.scroll.mirroring,
                    );
                    self.phase = Phase::Draw(draw, Then::Slide);
                }
                Phase::Slide => {
                    self.slide(game, prg);
                    self.phase = Phase::Sliding;
                    return;
                }
                Phase::Sliding => {
                    if game.scroll.scroll_x | game.scroll.scroll_y == 0 {
                        self.phase = Phase::Spawn;
                        continue;
                    }
                    self.slide(game, prg);
                    return;
                }
                Phase::Spawn => {
                    spawn_first_row(game, prg);
                    let (draw, then) = page_draws(game, Then::Place);
                    self.phase = Phase::Draw(draw, then);
                }
                Phase::SecondPage(column) => self.phase = self.second_page(game, column),
                Phase::Place => {
                    place(game, prg, self.replaced);
                    self.phase = Phase::Done;
                    return;
                }
                Phase::Done => return,
            }
        }
    }

    /// One step of the fade path (`$BA8B-$BB3A`); true when the frame ended.
    fn fade(&mut self, game: &mut Game, prg: &[u8]) -> bool {
        match self.phase {
            Phase::FadeStart => {
                self.kept.copy_from_slice(&game.effects.palette[17..20]);
                self.phase = Phase::FadeOut(Fade::new(false, true));
            }
            Phase::FadeOut(mut fade) => {
                self.phase = if fade.step(game, prg) {
                    Phase::FadeLoad
                } else {
                    Phase::FadeOut(fade)
                };
                return true;
            }
            Phase::FadeLoad => {
                game.world.room = game.world.room.wrapping_add(2);
                load(game, prg);
                self.replaced = game.scroll.mirroring;
                spawn_first_row(game, prg);
                game.effects.palette[17..20].copy_from_slice(&self.kept);
                let (draw, then) = page_draws(game, Then::FadeIn);
                self.phase = Phase::Draw(draw, then);
            }
            Phase::FadeEnter => {
                game.scroll.ppu_ctrl &= 0xFC;
                game.scroll.mirroring = self.replaced;
                parameters(game, prg);
                game.oam.finish();
                self.phase = Phase::FadeIn(Fade::new(true, false));
            }
            Phase::FadeIn(mut fade) => {
                self.phase = if fade.step(game, prg) {
                    Phase::Close
                } else {
                    Phase::FadeIn(fade)
                };
                return true;
            }
            Phase::Close => {
                game.effects.transition = 0;
                game.effects.transition_arg = 0;
                self.phase = Phase::Done;
                return true;
            }
            _ => unreachable!("not a fade phase"),
        }
        false
    }

    /// The second page of a room two nametables wide or tall (`$BA3C-$BA51`, `$BB18-$BB2D`):
    /// into `$2400`, from map row 1 when the room is tall.
    fn second_page(&self, game: &mut Game, column: u8) -> Phase {
        game.scroll.ppu_ctrl &= 0xFC;
        let row = if game.world.orientation & orientation::VERTICAL == 0 {
            0x61
        } else {
            0x60
        };
        let draw = PageDraw::new(column, row, 0x24, game.scroll.mirroring);
        let then = if self.target & 0x80 == 0 {
            Then::Place
        } else {
            Then::FadeIn
        };
        Phase::Draw(draw, then)
    }

    /// One frame of the camera's move to a page edge (`$B7F3-$B8D9`), or the step
    /// past it; true when the frame ended.
    fn align(&mut self, game: &mut Game, prg: &[u8]) -> bool {
        game.oam.next = 0;
        game.work.anchor = 0;
        let [cy, fy] = game.world.camera_y.to_be_bytes();
        let [cx, fx] = game.world.camera_x.to_be_bytes();
        if (cy & 0x0F) | fy == 0 && (cx & 0x0F) | fx == 0 {
            if game.world.orientation & orientation::VERTICAL != 0 {
                game.scroll.mirroring = 0;
                self.phase = Phase::Turn;
                return true;
            }
            self.phase = Phase::Enter;
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
    fn slide(&self, game: &mut Game, prg: &[u8]) {
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
                scroll.ppu_ctrl ^= 1;
            }
        } else if self.target & 8 == 0 {
            player.screen_y = player.screen_y.wrapping_sub(0x10).max(0x40);
            let y = scroll.scroll_y.wrapping_add(0x10);
            scroll.scroll_y = if y >= 0xF0 {
                scroll.ppu_ctrl ^= 2;
                0
            } else {
                y
            };
        } else {
            player.screen_y = player.screen_y.wrapping_add(0x10).min(0xD0);
            let y = scroll.scroll_y.wrapping_sub(0x10);
            scroll.scroll_y = if y >= 0xF0 {
                scroll.ppu_ctrl ^= 2;
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

/// The new room's camera page drawn into the first nametable, then `last`, or for a
/// room two nametables across (`$DD` bit 7) the page beside it first (`$BA0B-$BA51`).
fn page_draws(game: &Game, last: Then) -> (PageDraw, Then) {
    let orientation = game.world.orientation;
    let camera = if orientation & orientation::VERTICAL != 0 {
        game.world.camera_y
    } else {
        game.world.camera_x
    };
    let page = usize::from(camera >> 12);
    let (first, second) = (game.schedule.rows[page], game.schedule.rows[page + 16]);
    let (first, second) = if orientation::horizontal_two_page(orientation) {
        (second, first)
    } else {
        (first, second)
    };
    let then = if orientation & orientation::TWO_PAGES == 0 {
        last
    } else {
        Then::SecondPage(second)
    };
    (
        PageDraw::new(first, 0x60, 0x20, game.scroll.mirroring),
        then,
    )
}

/// Restores the arrangement, places the player and reads the room's parameters, and
/// ends the change (`$BA3C-$BA88`, `$D4EF`).
fn place(game: &mut Game, prg: &[u8], replaced: u8) {
    game.scroll.ppu_ctrl &= 0xFC;
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
    if ppu_ctrl & 3 != 0 { 0x20 } else { 0x24 }
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
    pub fn new(column: u8, row: u8, table: u8, mirroring: u8) -> PageDraw {
        let table = if table != 0x20 && mirroring & 3 != 0 {
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
        for column in 0..16 {
            let at = usize::from(self.row & 0x0F) * 256 + usize::from(self.column) + column;
            let corners = level.tiles(level.map[at]);
            bytes[column * 2..column * 2 + 2].copy_from_slice(&corners[..2]);
            bytes[32 + column * 2..34 + column * 2].copy_from_slice(&corners[2..]);
        }
        game.effects.vram.push(self.addr, 0, &bytes);
        super::frame::animate_chr(game);
        self.row = self.row.wrapping_add(1);
        self.addr = self.addr.wrapping_add(0x40);
        false
    }
}

/// Loads room `$0402` of area `$0400`: clears the slots, sets the palette, CHR
/// banks, orientation, page order, camera, player start and schedule (`$B40A`).
pub fn load(game: &mut Game, prg: &[u8]) {
    let (area, room) = (game.world.area, game.world.room);
    game.slots = Slots::default();
    game.effects.projectiles = [Projectile::default(); 16];
    game.effects.pause_name = [0; 18];
    game.effects.pause_aux = 0;
    let record = rom_word(
        prg,
        0,
        rom_word(prg, 14, 0xC048 + u16::from(area)) + u16::from(room),
    );
    let byte = |i: u16| rom_byte(prg, 0, record + i);
    let sets = rom_word(prg, 13, 0xB5F5 + u16::from(area));
    let sprites = rom_byte(prg, 13, sets + u16::from(room >> 1));
    let kept: [u8; 3] = game.effects.palette[17..20].try_into().expect("3 bytes");
    build_palette(prg, byte(0), sprites, &mut game.effects.palette);
    game.effects.palette[17..20].copy_from_slice(&kept);
    set_first_colours(&mut game.effects.palette, 0x0F);
    for (i, bank) in game.oam.chr[..4].iter_mut().enumerate() {
        *bank = byte(1 + u16::try_from(i).expect("< 4"));
    }
    game.world.orientation = byte(5);
    animate_banks(game, prg);
    game.schedule.rows = rows_from_rom(prg, area, room).expect("the room's page order");
    if orientation::horizontal_two_page(game.world.orientation) {
        raise_attributes(&mut game.effects.level, &game.schedule.rows, byte(6));
    }
    let start = rom_word(
        prg,
        13,
        rom_word(prg, 13, 0xB653 + u16::from(area)) + u16::from(room),
    );
    let start = |i: u16| rom_byte(prg, 13, start + i);
    let player = &mut game.effects.player;
    player.x = u16::from_be_bytes([start(0), start(1)]);
    player.y = u16::from_be_bytes([start(2), start(3)]);
    game.world.camera_x = u16::from_be_bytes([start(4), start(5)]);
    game.world.camera_y = u16::from_be_bytes([start(6), start(7)]);
    game.scroll.ring_y = u16::from_be_bytes([start(8), start(9)]);
    game.effects.scroll_lock = start(10);
    game.scroll.mode = start(11);
    game.scroll.mirroring = start(12);
    game.scroll.start = start(13);
    player.facing = start(14);

    game.scroll.gate = 0xFF;
    game.effects.vram.origin = 0x2000;
    game.effects.attack &= 0x80;
    player.vx = 0;
    player.motion = Motion::Standing;
    player.pose = 0;
    player.flags[2] = 0;
    player.flags[1] = 0;
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
    game.schedule.cells = spawn_schedule(prg, area)
        .expect("the area's schedule")
        .try_into()
        .expect("256 cells");
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
/// If `level` is outside the ROM's level table or its data fails to decode.
#[must_use]
pub fn start(prg: &[u8], chr: &[u8], level: usize) -> Game {
    let area = crate::levelmap::area_for_level(level).expect("a level 0-12");
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
            area: u8::try_from(area * 2).expect("an area index"),
            room: 0,
            pad: 0,
            pressed: 0,
            spawn_distance: 0,
        },
        schedule: Schedule {
            cells: [0; 256],
            rows: [0; 32],
        },
        effects: Effects {
            meter: 0xFE,
            lives: 2,
            ..Effects::default()
        },
        oam: super::oam::OamBuilder::new(),
        work: super::engine::Work::default(),
        scroll: super::scroll::Scroll {
            room: INTRO_RASTER,
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
    game.scroll.ppu_ctrl = 0xA8;
    game.scroll.mask = 0x18;
    parameters(&mut game, prg);
    game
}

/// The raster bytes `$D0-$DC` as the intro leaves them: its setup (WRAM `$604C-$6060`)
/// and IRQ chain (bank 2 `$96AF-$972C`). An area with no raster split sets only `$D0`
/// (`$D9B5`), so the first area after boot starts with the rest.
const INTRO_RASTER: [u8; 13] = [0, 0x80, 0x26, 0x09, 0, 0, 0, 0, 0, 0, 0x1F, 0, 0];

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
pub fn enter_area(game: &mut Game, prg: &[u8], chr: &[u8], leftover: Leftover) {
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
    if game.effects.player.flags[0] & 0x80 != 0 {
        game.effects.player.flags[0] = 0;
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
pub fn load_level(game: &mut Game, prg: &[u8], chr: &[u8], leftover: Leftover) {
    game.world.room = 0;
    let image = match leftover {
        Leftover::Boot => crate::levelmap::boot_image(chr).expect("the boot payload's CHR"),
        Leftover::Cutscene => crate::levelmap::payload_image(chr).expect("the payload's CHR"),
        Leftover::LastArea => wram_7000(game),
    };
    game.effects.level =
        crate::collision::Level::loaded(prg, chr, usize::from(game.world.area >> 1), image)
            .expect("the level decodes");
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
pub fn spawn_first_row(game: &mut Game, prg: &[u8]) {
    let table = SpawnTable::import(prg, game.world.area, game.world.room);
    let start = if orientation::horizontal_two_page(game.world.orientation) {
        let page = game.world.camera_x.to_be_bytes()[0] & 0x0F;
        *game
            .schedule
            .rows
            .get(usize::from(page) << 4)
            .expect("$D73C reads past the page order $0440-$045F")
    } else {
        game.scroll.start
    };
    spawn_row(&mut game.slots, &mut game.schedule, &table, start);
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
    let column = game.schedule.rows[usize::from(camera >> 12)] & 0xF0;
    let level = &game.effects.level;
    let mut writes: Vec<VramWrite> = (0..15u16)
        .map(|row| {
            let mut bytes = [0u8; 64];
            for i in 0..16 {
                let at = usize::from(row) * 256 + usize::from(column) + i;
                let tiles = level.tiles(level.map[at]);
                bytes[i * 2..i * 2 + 2].copy_from_slice(&tiles[..2]);
                bytes[32 + i * 2..34 + i * 2].copy_from_slice(&tiles[2..]);
            }
            VramWrite {
                addr: 0x2000 + row * 0x40,
                flags: 0,
                bytes: bytes.to_vec(),
            }
        })
        .collect();
    let page = usize::from(column >> 4) * 64;
    let attributes = &level.attributes[page..page + 64];
    game.effects.vram.attributes[..64].copy_from_slice(attributes);
    writes.push(VramWrite {
        addr: 0x23C0,
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
        let record = rom_word(prg, 14, 0xC06C + u16::from((bank << 1) & 0x7F));
        let byte = |i: u16| rom_byte(prg, 0, record + i);
        let frames = byte(0);
        display.timers[shadow] = [byte(1), frames, 0, 0];
        let mut last = bank;
        for frame in 0..frames {
            last = byte(2 + u16::from(frame));
            display.banks[shadow * 8 + usize::from(frame)] = last;
        }
        game.oam.chr[shadow] = last;
    }
}

/// The room's parameters, the stream masks and the room's player setup (`$D992`).
pub fn parameters(game: &mut Game, prg: &[u8]) {
    let (area, room) = (game.world.area, game.world.room);
    let data = rom_word(
        prg,
        14,
        rom_word(prg, 14, 0xDA18 + u16::from(area)) + u16::from(room),
    );
    let byte = |i: u16| rom_byte(prg, 14, data + i);
    let scroll = &mut game.scroll;
    if byte(0) == 0x77 {
        scroll.room[0] = 0;
        scroll.fixed_rows = [0; 16];
        scroll.fixed_quads = [0; 8];
        return;
    }
    scroll.room = [0; 13];
    for i in 0..5 {
        scroll.room[i] = byte(u16::try_from(i).expect("small"));
    }
    for i in 0..16 {
        scroll.fixed_rows[i] = byte(5 + u16::try_from(i).expect("small"));
    }
    for i in 0..8 {
        scroll.fixed_quads[i] = byte(21 + u16::try_from(i).expect("small"));
    }
    let player = &mut game.effects.player;
    match rom_word(prg, 14, data + 29) {
        0xDA0E => {}
        0xDA0F => (player.motion, player.stage_script) = (Motion::Held, 0xE0),
        0xD9FF => (player.anim, player.motion, player.stage_script) = (0x85, Motion::Flying, 0xA0),
        other => panic!("room handler ${other:04X} ($D9FC) is not ported"),
    }
}

/// Enters the next room of the area (`$B927-$B942`): the loader, the arrangement for
/// the slide toward `target`'s side, and the palette upload; the arrangement it replaced.
pub fn enter(game: &mut Game, prg: &[u8], target: u8) -> u8 {
    game.world.room = game.world.room.wrapping_add(2);
    load(game, prg);
    let replaced = game.scroll.mirroring;
    game.scroll.mirroring = u8::from(target.trailing_zeros() >= 2);
    game.effects.vram.push(0x3F00, 0, &game.effects.palette);
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
        let prg = scene.prg();
        let chr = scene.machine.bus.cart.chr.clone();
        let first = scene.run(1, &[0xD3F9]).remove(0);
        let oracle = State::lift(&first);
        let mut port = start(&prg, &chr, 0);
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
