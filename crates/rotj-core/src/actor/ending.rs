//! The ending, `$BA = 6` (bank 14 `$D967`, bank 2 `$999C`): the music stopped, a fade
//! out, the WRAM payload copied from CHR (`$99A9`) and entered at `$6135`. It never
//! returns: four scenes and the credits, then the last screen holds for good (`$6656`).

use super::Game;
use super::cutscene::{ScreenClear, copy_frame, draw_block, draw_sprite, payload_byte, read_bytes};
use super::script::{Flow, Runner, wait};
use super::stage::Fade;

/// What the payload does from `$999C` on, in order: each step runs one frame or more,
/// every frame ending at an NMI wait, or none and the next step runs in the same frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// The music stopped (`$40 = $FF`).
    StopMusic,
    /// A song (`$41`).
    Song(u8),
    /// The PPU mask the NMI applies (`$FE`).
    Mask(u8),
    /// A sound effect (`$42`).
    Sound(u8),
    /// The palette faded out to the dark one (`$C2E1`).
    FadeOut,
    /// The palette faded in from black (`$C2B8`).
    FadeIn,
    /// `$99A9`'s three frames.
    CopyPayload,
    /// The screen cleared, the nametables filled with a tile (`$7EC9`).
    Clear(u8),
    /// The CHR shadows from `$E0` on, from a payload table.
    Banks(u16, u8),
    /// The palette (`$04E0`) from a payload table.
    Palette(u16),
    /// The buffer (`$7FC0`) from a payload table.
    Buffer(u16, u8),
    /// The buffer's halves filled (`$7FA6`, `$7FB1`).
    Fill(u8, u8),
    /// The buffer's palette queued (`$7F89`), one frame.
    ShowBuffer,
    /// The second nametable patterned from `$66E0`, a 64-byte row a frame (`$66A9`).
    Pattern,
    /// Blocks of the draw list, one a frame from its start (`$7E69`).
    Blocks(u16, u8),
    /// The title's raster split (`$D3 = $0C`, first IRQ `$1E00` cycles in).
    Split,
    /// The credits' parallax split (`$D3 = $0D`, first IRQ `$1D00` cycles in).
    Parallax,
    /// The nametable arrangement (`$EC`, FME-7 command `$C`).
    Mirroring(u8),
    /// Every sprite parked (`$C16F`).
    Park,
    /// Frames of nothing but the wait.
    Wait(u16),
    /// The falling figure and the burst beside it (`$61B2-$622C`).
    Fall,
    /// The figure rising away (`$623E-$6271`).
    Rise,
    /// The watching figure drawn (`$6423`).
    Watcher,
    /// The watcher and the city's three bursts, 256 frames (`$6356-$63D4`).
    Skyline,
    /// The watcher alone, a frame each (`$63D7-$63E4`).
    Watch(u16),
    /// Three colours of sprite palette 0 lit (`$63E7`).
    Light,
    /// Every colour white (`$63FD`).
    White,
    /// The walker reset and drawn walking in (`$64B0-$64BC`).
    WalkIn,
    /// The walker, a frame each (`$6511`).
    Walk(u16),
    /// The credits' 28 lines, each held `$A0` frames under the walker (`$64C9-$64EC`).
    Credits,
    /// The walker walking off (`$64ED-$64FA`).
    WalkOff,
    /// The last screen, held for good (`$6656`).
    Hold,
}

const SCRIPT: &[Action] = &[
    // $999C
    Action::StopMusic,
    Action::FadeOut,
    Action::CopyPayload,
    // $6135: the title over the patterned second nametable
    Action::Clear(SCREEN_FILL),
    Action::Song(0x0D),
    Action::Banks(0x67EC, 8),
    Action::Palette(0x67F4),
    Action::Pattern,
    Action::Fill(BUFFER_FILL, BUFFER_FILL_TAIL),
    Action::Blocks(SCREEN_BLOCKS, 5),
    Action::Split,
    Action::Mask(0x1E),
    Action::FadeIn,
    Action::Wait(0x90),
    Action::Buffer(0x689C, 6),
    Action::Fall,
    Action::Park,
    Action::Wait(0x60),
    Action::Buffer(0x685A, 10),
    Action::Rise,
    Action::Park,
    Action::Wait(0x60),
    Action::FadeOut,
    // $62DE: the skyline
    Action::Clear(SCREEN_FILL),
    Action::Banks(0x6AAD, 7),
    Action::Fill(BUFFER_FILL, BUFFER_FILL_TAIL),
    Action::Blocks(SCREEN_BLOCKS, 2),
    Action::Fill(0, 0),
    Action::Blocks(0x68A5, 9),
    Action::Palette(0x6AB4),
    Action::Buffer(0x6AB4, 32),
    Action::Watcher,
    Action::Mask(0x1E),
    Action::FadeIn,
    Action::Skyline,
    Action::Watch(0x5F),
    Action::Light,
    Action::ShowBuffer,
    Action::Sound(0x23),
    Action::Wait(0x21),
    Action::White,
    Action::ShowBuffer,
    Action::FadeOut,
    Action::Wait(0x21),
    // $6444: the credits
    Action::Clear(0),
    Action::Banks(0x6CEA, 8),
    Action::Mirroring(2),
    Action::Palette(0x6CF2),
    Action::Blocks(0x6B02, 5),
    Action::Parallax,
    Action::Mask(0x1E),
    Action::WalkIn,
    Action::FadeIn,
    Action::Walk(0xA0),
    Action::Credits,
    Action::WalkOff,
    Action::Park,
    Action::Wait(0x60),
    Action::FadeOut,
    // $65BB
    Action::Clear(0),
    Action::Banks(0x7103, 4),
    Action::Palette(0x7107),
    Action::Blocks(0x70C3, 2),
    Action::Mask(0x1E),
    Action::FadeIn,
    Action::Wait(0x180),
    Action::FadeOut,
    // $6624: the last screen
    Action::Clear(0),
    Action::Palette(0x7127),
    Action::Blocks(0x7147, 3),
    Action::Mask(0x1E),
    Action::FadeIn,
    Action::Hold,
];

/// The `$6D12` table's credit lines and the list they are read from.
const CREDIT_LINES: u16 = 0x6D12;
const CREDIT_LIST: u16 = 0x6D2E;
const CREDITS: u8 = 0x1C;
/// Frames a credit line is held under the walker (`$6511`).
const CREDIT_HOLD_FRAMES: u8 = 0xA0;
/// The draw-list read offset that marks a line with no hold (`$6511`).
const NO_HOLD_OFFSET: u8 = 0x10;

/// The title's and skyline's screen fill (`$67`).
const SCREEN_FILL: u8 = 0x67;
/// The buffer's two fill bytes (`$68`, `$66`).
const BUFFER_FILL: u8 = 0x68;
const BUFFER_FILL_TAIL: u8 = 0x66;
/// The title's and skyline's block list (`$6720`).
const SCREEN_BLOCKS: u16 = 0x6720;
/// The burst's shape pointers and per-shape hold frames (`$686E`, `$686A`).
const BURST_SHAPES: u16 = 0x686E;
const BURST_HOLDS: u16 = 0x686A;
/// The falling figure's pose tiles (`$68A2`).
const FALL_POSE_TILES: u16 = 0x68A2;
/// The rising figure's sprite and pose tiles (`$6814`, `$6864`).
const RISE_FIGURE: u16 = 0x6814;
const RISE_POSE_TILES: u16 = 0x6864;
/// The walker's cape, body and pose-frame tables (`$6F39`, `$6F5F`, `$6F59`).
const WALKER_CAPES: u16 = 0x6F39;
const WALKER_BODIES: u16 = 0x6F5F;
const WALKER_POSE_FRAMES: u16 = 0x6F59;

/// The ending, one frame per [`Ending::step`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ending {
    /// Where the ending is in [`SCRIPT`].
    runner: Runner,
    fade: Option<Fade>,
    clear: ScreenClear,
    /// WRAM `$7FC0-$7FFF`, the payload's buffer for palettes, fills and built sprites.
    buffer: [u8; 64],
    /// The draw list's read offset (`$20`).
    cursor: u8,
    figure: Figure,
    bursts: [Burst; 3],
    walker: Walker,
    /// Credit lines drawn (`$2B`).
    credit: u8,
    /// Frames left in a credit line's hold (`$21`).
    hold: u8,
}

impl Default for Ending {
    fn default() -> Self {
        Ending {
            runner: Runner::default(),
            fade: None,
            clear: ScreenClear::new(0),
            buffer: [0; 64],
            cursor: 0,
            figure: Figure::default(),
            bursts: [Burst::default(); 3],
            walker: Walker::default(),
            credit: 0,
            hold: 0,
        }
    }
}

impl Ending {
    /// True once the last screen holds (`$6656`), which it does for good.
    #[must_use]
    pub fn over(&self) -> bool {
        self.runner.row(SCRIPT) == Some(&Action::Hold)
    }

    /// Runs one frame up to its NMI wait.
    pub fn step(&mut self, game: &mut Game, prg: &[u8], chr: &[u8]) {
        loop {
            let action = *self.runner.row(SCRIPT).expect("the ending holds for good");
            let flow = self.run(action, game, prg, chr);
            if self.runner.advance(flow) {
                return;
            }
        }
    }

    /// Runs a step's frame, or the whole of a step that takes none.
    fn run(&mut self, action: Action, game: &mut Game, prg: &[u8], chr: &[u8]) -> Flow {
        let read = |addr| wram(chr, &self.buffer, addr);
        let frames = |count: u16| wait(self.runner.frame, count);
        match action {
            Action::FadeOut | Action::FadeIn => {
                if self.runner.frame == 0 {
                    self.fade = Some(Fade::new(action == Action::FadeIn, false));
                }
                let Some(fade) = &mut self.fade else {
                    return Flow::Done;
                };
                if fade.step(game, prg) {
                    self.fade = None;
                }
                Flow::Waited
            }
            Action::CopyPayload => {
                if self.runner.frame < 3 {
                    copy_frame(
                        game,
                        prg,
                        u8::try_from(self.runner.frame).expect("< 3"),
                        crate::levelmap::PAYLOAD_FIRST_BANK,
                    );
                }
                frames(3)
            }
            Action::Clear(fill) => {
                if self.runner.frame == 0 {
                    self.clear = ScreenClear::new(fill);
                }
                if !self.clear.step(game) {
                    return Flow::Waited;
                }
                self.buffer = [0; 64];
                Flow::Done
            }
            Action::ShowBuffer => {
                if self.runner.frame == 0 {
                    game.effects.vram.push(0x3F00, 0, &self.buffer[..32]);
                }
                frames(1)
            }
            Action::Pattern => {
                if self.runner.frame < 15 {
                    let row = read_bytes(&read, 0x66E0, 0x40);
                    game.effects
                        .vram
                        .push(0x2400 + self.runner.frame * 0x40, 0, &row);
                }
                frames(15)
            }
            Action::Blocks(list, count) => {
                if self.runner.frame == 0 {
                    self.cursor = 0;
                }
                if self.runner.frame < u16::from(count) {
                    let buffer = &self.buffer;
                    draw_block(game, |addr| wram(chr, buffer, addr), list, &mut self.cursor);
                }
                frames(u16::from(count))
            }
            Action::Wait(count) => frames(count),
            Action::Fall => self.fall(game, chr),
            Action::Rise => self.rise(game, chr),
            Action::Skyline => self.skyline(game, chr),
            Action::Watch(count) => {
                if self.runner.frame < count {
                    watcher(game, read);
                }
                frames(count)
            }
            Action::Walk(count) => {
                if self.runner.frame < count {
                    self.walker.step(game, read);
                }
                frames(count)
            }
            Action::Credits => self.credits(game, chr),
            Action::WalkOff => {
                if self.runner.frame == 0 {
                    self.walker.mode = 0xFF;
                } else if self.walker.x == 0xFE {
                    return Flow::Done;
                }
                self.walker.step(game, read);
                Flow::Waited
            }
            Action::Hold => Flow::Waited,
            _ => {
                self.set(action, game, chr);
                Flow::Done
            }
        }
    }

    /// A step that takes no frame.
    fn set(&mut self, action: Action, game: &mut Game, chr: &[u8]) {
        let read = |addr| wram(chr, &self.buffer, addr);
        match action {
            Action::StopMusic => game.effects.music = 0xFF,
            Action::Song(song) => game.effects.song = song,
            Action::Mask(mask) => game.scroll.mask = mask,
            Action::Sound(sound) => game.effects.sound[0] = sound,
            Action::Banks(table, count) => {
                for (i, shadow) in (0..u16::from(count)).zip(&mut game.oam.chr) {
                    *shadow = read(table + i);
                }
            }
            Action::Palette(table) => {
                for (i, colour) in (0..).zip(&mut game.effects.palette) {
                    *colour = read(table + i);
                }
            }
            Action::Buffer(table, len) => {
                let bytes = read_bytes(&read, table, u16::from(len));
                self.buffer[..bytes.len()].copy_from_slice(&bytes);
            }
            Action::Fill(low, high) => {
                self.buffer[..32].fill(low);
                self.buffer[32..].fill(high);
            }
            Action::Split => {
                let room = &mut game.scroll.room;
                room[3] = 0x0C;
                room[5] = 0;
                arm_split(room, 0x1E00);
            }
            Action::Parallax => {
                let room = &mut game.scroll.room;
                room[5..=10].fill(0);
                game.scroll.scroll_x = 0;
                room[3] = 0x0D;
                arm_split(room, 0x1D00);
            }
            Action::Mirroring(mirroring) => game.scroll.mirroring = mirroring,
            Action::Park => game.oam.clear(),
            Action::Watcher => watcher(game, read),
            Action::Light => {
                self.buffer[0x11..0x14].fill(0x30);
                game.effects.palette[0x11..0x14].fill(0x30);
            }
            Action::White => {
                self.buffer[..32].fill(0x30);
                game.effects.palette.fill(0x30);
            }
            Action::WalkIn => {
                self.walker = Walker::default();
                self.walker.step(game, read);
            }
            _ => unreachable!("{action:?} takes frames"),
        }
    }

    /// A frame of the falling figure (`$61D0`), its pose's tile set in the built sprite
    /// (`$68A2`), until the burst at (`$68`, `$60`) has run its four shapes.
    fn fall(&mut self, game: &mut Game, chr: &[u8]) -> Flow {
        if self.runner.frame == 0 {
            self.figure = Figure {
                x: 0x68,
                y: 0x60,
                pose: 0,
            };
            self.bursts[0] = Burst::at(0x68, 0x60);
        } else if self.bursts[0].stage == 4 {
            return Flow::Done;
        } else {
            let figure = &mut self.figure;
            figure.x = if figure.x == 0 { 0 } else { figure.x - 1 };
            figure.y = (figure.y + 1).min(0xF0);
            figure.pose = (figure.pose + 1) % 6;
        }
        self.bursts[0].step(game, |addr| payload_byte(chr, addr));
        self.buffer[1] = payload_byte(chr, FALL_POSE_TILES + u16::from(self.figure.pose >> 1));
        let read = |addr| wram(chr, &self.buffer, addr);
        draw_sprite(game, read, BUILT, self.figure.x, self.figure.y);
        game.oam.finish();
        Flow::Waited
    }

    /// A frame of the figure rising away up-right (`$6254`) until x reaches `$FE`, its
    /// pose's tiles set in the built sprite (`$62C3`, `$6864`).
    fn rise(&mut self, game: &mut Game, chr: &[u8]) -> Flow {
        let figure = &mut self.figure;
        if self.runner.frame == 0 {
            *figure = Figure {
                x: 0x20,
                y: 0xFF,
                pose: 0,
            };
        } else {
            figure.pose = (figure.pose + 1) % 3;
            let tiles = RISE_POSE_TILES + 2 * u16::from(figure.pose);
            self.buffer[1] = payload_byte(chr, tiles);
            self.buffer[5] = payload_byte(chr, tiles + 1);
            figure.x = figure.x.wrapping_add(3);
            figure.y = figure.y.wrapping_sub(2);
            if figure.x == 0xFE {
                return Flow::Done;
            }
        }
        let (x, y) = (self.figure.x, self.figure.y);
        let read = |addr| wram(chr, &self.buffer, addr);
        draw_sprite(game, read, RISE_FIGURE, x, y);
        draw_sprite(game, read, BUILT, x, y);
        game.oam.finish();
        Flow::Waited
    }

    /// A frame of the skyline (`$6361`): the three bursts, from frames 0, `$38` and `$58`
    /// until each has run its shapes, then the watcher.
    fn skyline(&mut self, game: &mut Game, chr: &[u8]) -> Flow {
        const STARTS: [u16; 3] = [0, 0x38, 0x58];
        if self.runner.frame == 0x100 {
            return Flow::Done;
        }
        if self.runner.frame == 0 {
            self.bursts = [
                Burst::at(0x74, 0x98),
                Burst::at(0x80, 0xA4),
                Burst::at(0x7C, 0x94),
            ];
        }
        let read = |addr| payload_byte(chr, addr);
        for (burst, start) in self.bursts.iter_mut().zip(STARTS) {
            if self.runner.frame >= start && burst.stage != 4 {
                burst.step(game, read);
            }
        }
        watcher(game, read);
        Flow::Waited
    }

    /// A frame of the credits (`$64D5`): a line's block under the walker, then its hold.
    fn credits(&mut self, game: &mut Game, chr: &[u8]) -> Flow {
        if self.runner.frame == 0 {
            (self.credit, self.hold) = (0, 0);
        }
        let read = |addr| payload_byte(chr, addr);
        if self.hold == 0 {
            if self.credit == CREDITS {
                return Flow::Done;
            }
            self.walker.step(game, read);
            self.cursor = read(CREDIT_LINES + u16::from(self.credit));
            draw_block(game, read, CREDIT_LIST, &mut self.cursor);
            self.credit += 1;
            self.hold = if self.cursor == NO_HOLD_OFFSET {
                0
            } else {
                CREDIT_HOLD_FRAMES
            };
        } else {
            self.walker.step(game, read);
            self.hold -= 1;
        }
        Flow::Waited
    }
}

/// The metasprite the payload builds in its buffer (`$7FC0`).
const BUILT: u16 = 0x7FC0;

/// WRAM at `addr` in the ending: the payload, under the buffer it rewrites.
fn wram(chr: &[u8], buffer: &[u8; 64], addr: u16) -> u8 {
    match addr.checked_sub(BUILT) {
        Some(at) => buffer[usize::from(at)],
        None => payload_byte(chr, addr),
    }
}

/// Arms the raster split (`$D0-$D4`) `cycles` after the NMI.
fn arm_split(room: &mut [u8; 13], cycles: u16) {
    room[4] = 0;
    [room[1], room[2]] = cycles.to_le_bytes();
    room[0] = 0x80;
}

/// The watching figure (`$6423`: `$6AD4` at (`$78`, `$A8`)), the frame's sprites done.
fn watcher(game: &mut Game, read: impl Fn(u16) -> u8) {
    draw_sprite(game, read, 0x6AD4, 0x78, 0xA8);
    game.oam.finish();
}

/// The figure of the fall and the rise (`$20`, `$21`, `$22`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Figure {
    x: u8,
    y: u8,
    pose: u8,
}

/// A burst at a fixed place (`$665C`): its four shapes from `$686E`, each held the
/// frames `$686A` gives, every other frame drawing the shape before.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Burst {
    x: u8,
    y: u8,
    /// Frames into the shape (`$2E`).
    ticks: u8,
    /// The shape (`$2F`), 4 once run.
    stage: u8,
}

impl Burst {
    fn at(x: u8, y: u8) -> Burst {
        Burst {
            x,
            y,
            ..Burst::default()
        }
    }

    /// One frame: the next shape once this one's frames have run (the effect `$22`
    /// with the first), then the draw.
    fn step(&mut self, game: &mut Game, read: impl Fn(u16) -> u8) {
        if read(BURST_HOLDS + u16::from(self.stage)) < self.ticks {
            if self.stage == 0 {
                game.effects.sound[0] = 0x22;
            }
            self.stage += 1;
            self.ticks = 0;
        }
        let shape = if self.ticks & 1 == 0 {
            Some(self.stage)
        } else {
            self.stage.checked_sub(1)
        };
        if let Some(shape) = shape {
            let at = BURST_SHAPES + 2 * u16::from(shape);
            let pointer = u16::from_le_bytes([read(at), read(at + 1)]);
            draw_sprite(game, read, pointer, self.x, self.y);
        }
        self.ticks = self.ticks.wrapping_add(1);
    }
}

/// The credits' walker (`$6526`): in from the left to x `$80`, standing through the
/// credits, then off to the right.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Walker {
    /// Frames into the pose (`$2C`).
    tick: u8,
    /// Screen x (`$2D`).
    x: u8,
    /// The body's pose, 0-5 (`$2E`).
    pose: u8,
    /// 0 walking in, `$7F` standing, `$FF` walking off (`$2F`).
    mode: u8,
}

impl Walker {
    fn step(&mut self, game: &mut Game, read: impl Fn(u16) -> u8) {
        if self.mode & 0x80 != 0 {
            if self.pose == 0 {
                self.x = self.x.wrapping_add(2);
                self.tick &= 7;
            }
        } else if self.mode == 0 {
            self.tick &= 7;
            self.x = self.x.wrapping_add(1);
            if self.x == 0x80 {
                self.mode = 0x7F;
                self.tick = 0;
            }
        }
        self.tick = self.tick.wrapping_add(1);
        if read(WALKER_POSE_FRAMES + u16::from(self.pose)) < self.tick {
            self.tick = 0;
            self.pose = (self.pose + 1) % 6;
        }
        let word = |at: u16| u16::from_le_bytes([read(at), read(at + 1)]);
        let body = word(WALKER_BODIES + 2 * u16::from(self.pose));
        draw_sprite(game, &read, body, self.x, 0x60);
        let cape = word(WALKER_CAPES + u16::from(self.tick & 6));
        draw_sprite(game, &read, cape, self.x, 0x60);
        game.oam.finish();
    }
}
