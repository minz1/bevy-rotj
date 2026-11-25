//! The pause (bank 8 `$9C37`): Start in the loop freezes the game under a dimmed
//! palette and the pause screen's sprites until Start is pressed again.

use super::Game;
use super::engine::{draw, draw_metasprite, rom_byte, rom_word};
use super::oam::{Mark, Metasprite};
use super::stage;
use crate::prg::Prg;
use crate::sim::button::Buttons;
use crate::sim::world::{set_first_colours, sound};

/// The area name's metasprite the pause builds in WRAM (`$7FD0-$7FE1`).
pub const NAME: u16 = 0x7FD0;

/// The pause, one frame per [`Pause::step`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pause {
    step: Step,
    /// The cycling colour of the sprite palette's second set (`$1B`).
    colour: u8,
    /// Batman's sprite banks, swapped for the font's (`$1C-$1F`).
    saved: [u8; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Start,
    Draw,
    Held,
    Done,
}

impl Default for Pause {
    fn default() -> Self {
        Pause {
            step: Step::Start,
            colour: 0x27,
            saved: [0; 4],
        }
    }
}

/// PRG bank holding the pause screen's layout, palette and glyph tables.
const PAUSE_BANK: u8 = 8;
/// PRG bank holding the pause screen's metasprites.
const DRAW_BANK: u8 = 1;
/// Bank 8 `$9DC2`: the pause screen's 16 sprite palette entries.
const SPRITE_PALETTE: u16 = 0x9DC2;
/// Bank 8 `$9E1A`: 16-bit metasprite pointers for digits 0-9.
const DIGIT_POINTERS: u16 = 0x9E1A;
/// Bank 8 `$9E2E`: screen X of the five pause card metasprites.
const ELEMENT_X: u16 = 0x9E2E;
/// Bank 8 `$9E33`: screen Y of the five pause card metasprites.
const ELEMENT_Y: u16 = 0x9E33;
/// Bank 8 `$9E38`: 16-bit metasprite pointers of the five pause card elements.
const ELEMENT_POINTERS: u16 = 0x9E38;
/// Bank 8 `$9E42`: attribute bytes of the five pause card elements.
const ELEMENT_ATTR: u16 = 0x9E42;
/// Bank 8 `$9E47`: tile indices for password letters `A`-`Z`.
const LETTER_TILES: u16 = 0x9E47;

impl Pause {
    /// True once Start has ended the pause; the loop body runs next (`$9CA8`).
    #[must_use]
    pub fn done(&self) -> bool {
        self.step == Step::Done
    }

    /// Runs one frame up to its NMI wait (`$9CAD`).
    pub fn step(&mut self, game: &mut Game, prg: &Prg) {
        match self.step {
            Step::Start => {
                game.effects.music = sound::music::PAUSE;
                game.effects.song = sound::song::PAUSE;
                game.scroll.gate = 0;
                game.scroll.speed_x = 0;
                game.scroll.speed_y = 0;
                game.oam.clear();
                self.saved.copy_from_slice(&game.oam.chr[4..]);
                for (bank, shadow) in (0xCC..).zip(&mut game.oam.chr[4..]) {
                    *shadow = bank;
                }
                self.queue_dimmed(game, prg);
                self.step = Step::Draw;
            }
            Step::Draw => {
                self.frame(game, prg);
                self.step = Step::Held;
                return;
            }
            Step::Held if !game.world.pressed.contains(Buttons::START) => {
                self.frame(game, prg);
                return;
            }
            Step::Held => {
                game.oam.chr[4..].copy_from_slice(&self.saved);
                let effects = &mut game.effects;
                effects.vram.push_palette(&effects.palette);
                game.oam.clear();
                game.effects.music = sound::music::RESUME;
                game.effects.song = sound::song::PAUSE;
                self.step = Step::Done;
            }
            Step::Done => return,
        }
        super::scroll::step(game, prg);
    }

    /// One paused frame (`$9C70-$9C85`): the sprites, the colour stepped, the palette.
    fn frame(&mut self, game: &mut Game, prg: &Prg) {
        game.oam.marking = Mark::Pause;
        draw_screen(game, prg);
        game.oam.marking = Mark::Scene;
        self.colour += 1;
        if self.colour >= 0x2D {
            self.colour = 0x20;
        }
        self.queue_dimmed(game, prg);
        game.oam.finish();
        super::scroll::step(game, prg);
    }

    /// Queues the palette with the background one step darker and the pause's sprite
    /// colours, each set's first black (`$9CB3`).
    fn queue_dimmed(&self, game: &mut Game, prg: &[u8]) {
        let mut palette = game.effects.palette;
        for colour in &mut palette[..16] {
            *colour = colour.checked_sub(0x10).unwrap_or(*colour);
        }
        for (i, colour) in (0..).zip(&mut palette[16..]) {
            *colour = rom_byte(prg, PAUSE_BANK, SPRITE_PALETTE + i);
        }
        palette[22] = self.colour;
        set_first_colours(&mut palette, crate::palette::BLACK);
        game.effects.vram.push_palette(&palette);
    }
}

/// The pause screen's sprites (`$9CE5`): five metasprites from bank 1, the area's
/// name built at [`NAME`], the lives left and the stage's two digits.
fn draw_screen(game: &mut Game, prg: &Prg) {
    let area = game.world.area;
    let lives = game.effects.lives.min(9);
    let (mut cx, work) = game.parts(prg);
    work.anchor = 0;
    work.tile = 0;
    for i in (0..5).rev() {
        work.screen_x = rom_byte(prg, PAUSE_BANK, ELEMENT_X + i);
        work.screen_y = rom_byte(prg, PAUSE_BANK, ELEMENT_Y + i);
        work.attr = rom_byte(prg, PAUSE_BANK, ELEMENT_ATTR + i);
        work.pointer = rom_word(prg, PAUSE_BANK, ELEMENT_POINTERS + 2 * i);
        draw(work, DRAW_BANK, &mut cx);
    }
    work.screen_y = 0xBF;
    let password = stage::at(&prg.levels, stage::checkpoint(&prg.levels, area))
        .password
        .expect("a checkpoint has a password");
    cx.effects.pause_aux = 0;
    let name = &mut cx.effects.pause_name;
    name[0] = 0x11;
    for ((i, letter), piece) in (0..4u8)
        .zip(password)
        .zip(name[1..17].as_chunks_mut::<4>().0)
    {
        let tile = rom_byte(
            prg,
            PAUSE_BANK,
            LETTER_TILES + u16::from(letter.wrapping_sub(0x41)),
        );
        piece.copy_from_slice(&[tile, 0xF0u8.wrapping_add(9 * i), 0, 0xEF]);
    }
    work.pointer = NAME;
    let sprite = Metasprite::from_bytes(&cx.effects.pause_name).expect("the area's name");
    draw_metasprite(work, &sprite, &mut cx);
    let [tens, ones] = stage::at(&prg.levels, area).banner;
    let digits = [(0x94, 0x87, lives), (0x8C, 0x6F, tens), (0x9C, 0x6F, ones)];
    for (x, y, digit) in digits {
        work.screen_x = x;
        work.screen_y = y;
        work.pointer = rom_word(prg, PAUSE_BANK, DIGIT_POINTERS + 2 * u16::from(digit));
        draw(work, DRAW_BANK, &mut cx);
    }
}
