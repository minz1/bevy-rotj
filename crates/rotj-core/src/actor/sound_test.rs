//! The sound test (bank 12 `$9A36`), entered when A and Start are held as the title
//! hands over to the stage start (`$D3C6`): up and down choose the row, left and right
//! the number, A plays the song or the effect or, on the last row, resets the game, B
//! stops the music.

use super::Game;
use super::cutscene::draw_sprite;
use super::engine::rom_byte;
use super::script::{Flow, Runner, wait};
use super::stage::blackout;
use super::world::{VramWrite, sound};
use crate::actor::button::Buttons;
use crate::actors::bank_slice;
use crate::ppulist::WriteList;
use crate::render::{ctrl, mask, ppu};

/// Its screen's direct writes (`$9BEB`), palette (`$9C36`), cursor (`$9C56`, at x `$68`
/// and a row's y from `$9C62`), the number's metasprite (`$9C65`) and tiles (`$9C6B`),
/// the songs (`$9C85`), the effects (`$9C92`) and the CHR banks (`$9CAC`), in bank 12.
const TEXT: u16 = 0x9BEB;
const PALETTE: u16 = 0x9C36;
const CURSOR: u16 = 0x9C56;
const ROWS: u16 = 0x9C62;
const NUMBER: u16 = 0x9C65;
const DIGITS: u16 = 0x9C6B;
const SONGS: u16 = 0x9C85;
const EFFECTS: u16 = 0x9C92;
const BANKS: u16 = 0x9CAC;

/// The rows: the songs, the effects and the exit (`$10`).
const SONG_ROW: u8 = 0;
const EFFECT_ROW: u8 = 1;
/// How many songs (`$11`) and effects (`$12`) it offers.
const SONG_COUNT: u8 = 0x0D;
const EFFECT_COUNT: u8 = 0x1A;

/// The sound test, one frame per [`SoundTest::step`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SoundTest {
    step: Step,
    /// Where the sound test is in [`SETUP`] or [`SONG`].
    runner: Runner,
    /// The row (`$10`).
    row: u8,
    /// The song shown (`$11`).
    song: u8,
    /// The effect shown (`$12`).
    effect: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// The screen set up (`$9A36-$9A7F`), the rows of [`SETUP`].
    Setup,
    /// A song started from the song row, the rows of [`SONG`].
    Song,
    Running,
    Exit,
}

/// What the sound test does before it runs, and after A on the song row, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    /// The music stopped and the screen dark.
    StopAndDark,
    /// Tall sprites, NMIs and rendering off.
    SpritesTall,
    /// The screen drawn ([`show_screen`]).
    Screen,
    /// Rendering on and the sprites and palette drawn.
    Show,
    /// The song shown started (`$9C85`).
    PlaySong,
}

const SETUP: &[Row] = &[Row::StopAndDark, Row::SpritesTall, Row::Screen, Row::Show];
const SONG: &[Row] = &[Row::PlaySong, Row::Show];

impl SoundTest {
    pub(super) fn new() -> SoundTest {
        SoundTest {
            step: Step::Setup,
            runner: Runner::default(),
            row: SONG_ROW,
            song: 0,
            effect: 0,
        }
    }

    /// Runs one frame up to its NMI wait; true, without a frame, once A on the last row
    /// has waited its frame and the game resets (`JMP $E2E6`).
    pub(super) fn step(&mut self, game: &mut Game, prg: &[u8]) -> bool {
        loop {
            match self.step {
                Step::Setup => {
                    if self.table_frame(SETUP, game, prg) {
                        return false;
                    }
                    self.step = Step::Running;
                }
                Step::Song => {
                    if self.table_frame(SONG, game, prg) {
                        return false;
                    }
                    self.step = Step::Running;
                }
                Step::Running => {
                    if game
                        .world
                        .pressed
                        .intersects(!(Buttons::SELECT | Buttons::START))
                    {
                        self.input(game, prg);
                    }
                    if self.step == Step::Running {
                        self.draw(game, prg);
                    }
                    return false;
                }
                Step::Exit => return true,
            }
        }
    }

    /// Starts `step` at the top of its table.
    fn begin(&mut self, step: Step) {
        self.step = step;
        self.runner = Runner::default();
    }

    /// Runs `table`'s rows up to the frame's end (true) or until it runs out.
    fn table_frame(&mut self, table: &[Row], game: &mut Game, prg: &[u8]) -> bool {
        while let Some(&row) = self.runner.row(table) {
            let frame = self.runner.frame;
            let flow = match row {
                Row::StopAndDark => {
                    if frame == 0 {
                        game.effects.music = sound::music::STOP;
                        blackout(game, prg);
                    }
                    wait(frame, 1)
                }
                Row::SpritesTall => {
                    if frame == 0 {
                        game.scroll.ppu_ctrl = (game.scroll.ppu_ctrl
                            & !(ctrl::NMI | ctrl::VERTICAL_INCREMENT | ctrl::NAMETABLE))
                            | ctrl::TALL_SPRITES;
                        game.scroll.mask = 0;
                    }
                    wait(frame, 1)
                }
                Row::Screen => {
                    if frame == 0 {
                        show_screen(game, prg);
                    }
                    wait(frame, 1)
                }
                Row::Show => {
                    if frame == 0 {
                        game.scroll.mask = mask::RENDERING;
                        self.draw(game, prg);
                    }
                    wait(frame, 1)
                }
                Row::PlaySong => {
                    game.effects.song = rom_byte(prg, 12, SONGS + u16::from(self.song));
                    Flow::Done
                }
            };
            if self.runner.advance(flow) {
                return true;
            }
        }
        false
    }

    /// A button pressed (`$9A90-$9B38`); A on the song or the exit row leaves the frame
    /// with nothing drawn.
    fn input(&mut self, game: &mut Game, prg: &[u8]) {
        let pressed = game.world.pressed;
        if pressed.contains(Buttons::A) {
            match self.row {
                SONG_ROW => {
                    game.effects.music = sound::music::STOP;
                    self.begin(Step::Song);
                }
                EFFECT_ROW => {
                    game.effects.sound[0] = rom_byte(prg, 12, EFFECTS + u16::from(self.effect));
                }
                _ => {
                    game.scroll.mask = 0;
                    self.step = Step::Exit;
                }
            }
        } else if pressed.contains(Buttons::B) {
            game.effects.music = sound::music::STOP;
        } else if pressed.intersects(Buttons::SIDEWAYS) {
            let right = pressed.contains(Buttons::RIGHT);
            match self.row {
                SONG_ROW => {
                    game.effects.music = sound::music::STOP;
                    self.song = step_wrapping(self.song, right, SONG_COUNT);
                }
                EFFECT_ROW => self.effect = step_wrapping(self.effect, right, EFFECT_COUNT),
                _ => {}
            }
        } else {
            self.row = step_wrapping(self.row, pressed.contains(Buttons::DOWN), 3);
        }
    }

    /// The sprites and palette (`$9B7D`, `$C145`, `$9BDC`): the cursor at its row, the
    /// song's and the effect's numbers.
    fn draw(self, game: &mut Game, prg: &[u8]) {
        let rom = |addr: u16| rom_byte(prg, 12, addr);
        let cursor = u16::from_le_bytes([rom(CURSOR), rom(CURSOR + 1)]);
        draw_sprite(game, rom, cursor, 0x68, rom(ROWS + u16::from(self.row)));
        for (number, x, y) in [(self.song, 0x9C, 0x70), (self.effect, 0xB4, 0x90)] {
            let mut sprite: [u8; 6] =
                std::array::from_fn(|i| rom(NUMBER + u16::try_from(i).expect("< 6")));
            sprite[1] = rom(DIGITS + u16::from(number));
            draw_sprite(game, |addr| sprite[usize::from(addr - 0x20)], 0x20, x, y);
        }
        game.oam.finish();
        let palette: Vec<u8> = (0..32).map(|i| rom(PALETTE + i)).collect();
        game.effects.vram.push_palette(&palette);
    }
}

/// The screen drawn with rendering off (`$9A4E-$9A73`): the sprites parked, the first
/// nametable cleared and the text from `$9BEB` written, the font's banks, NMIs back on.
fn show_screen(game: &mut Game, prg: &[u8]) {
    let rom = |addr: u16| rom_byte(prg, 12, addr);
    game.oam.clear();
    game.scroll.scroll_x = 0;
    game.scroll.scroll_y = 0;
    let direct = &mut game.effects.vram.direct;
    direct.push(VramWrite {
        addr: ppu::nametable(0),
        flags: 0,
        bytes: vec![0; 0x400],
    });
    let text = bank_slice(prg, 12, TEXT & 0x1FFF).expect("the sound test's text");
    direct.extend(WriteList::new(text).map(|run| VramWrite {
        addr: run.addr,
        flags: 0,
        bytes: run.data.to_vec(),
    }));
    for (i, shadow) in (0..).zip(&mut game.oam.chr) {
        *shadow = rom(BANKS + i);
    }
    game.scroll.ppu_ctrl |= ctrl::NMI;
}

/// `value` one up (wrapping to 0 at `count`) or one down (wrapping to `count - 1`).
fn step_wrapping(value: u8, up: bool, count: u8) -> u8 {
    if up {
        (value + 1) % count
    } else {
        value.checked_sub(1).unwrap_or(count - 1)
    }
}
