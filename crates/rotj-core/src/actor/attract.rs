//! The attract mode (bank 2 `$99F7`): the title left waiting runs three story screens
//! from CHR copied to WRAM, then the boot copies its payload back and starts again, the
//! intro skipped if A, B or Start was held.

use super::Game;
use super::cutscene::draw_sprite_with;
use super::engine::{rom_byte, rom_word};
use super::room::build_palette;
use super::stage::{Fade, blackout, queue_palette};
use super::world::VramWrite;

/// The buttons that leave the attract mode while held (`$F6 & $D0`).
const HELD_TO_LEAVE: u8 = 0xD0;

/// The CHR the screens read from WRAM (`$9B51`): banks `$EC-$EE` at `$7000`, the first
/// screen's two nametables and the third's page, and banks `$F4-$F5` at `$6000`, the
/// first screen's next page and the second's.
const CITY: usize = 0xEC * 0x400;
const STORY: usize = 0xF4 * 0x400;

/// The first screen's colour steps (`$9C70`) and its lit colours (`$9C76`).
const FIRST_COLOURS: u16 = 0x9C70;
const LIT_COLOURS: u16 = 0x9C76;
/// The sprites: the first screen's (`$9CA7`), the second's (`$9DA5`), the third's
/// figure (`$9E80`) and its stars' two sizes, two frames each (`$9F3B`).
const FIRST_SPRITE: u16 = 0x9CA7;
const SECOND_SPRITE: u16 = 0x9DA5;
const FIGURE: u16 = 0x9E80;
const STAR_SPRITES: u16 = 0x9F3B;
/// The stars' places as the third screen starts them (`$9EBF`, to WRAM `$6F00`).
const STARS: u16 = 0x9EBF;

/// The attract mode, one frame per [`Attract::step`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Attract {
    step: Step,
    fade: Fade,
    /// The screen's x scroll, 1/256 px (`$11:$12`).
    scroll: u16,
    /// The first screen's second phase has begun (`$10`).
    lit: bool,
    /// The first screen's frame count, then its lit flag (`$1F`).
    count: u8,
    /// The first screen's lit frames (`$1E`).
    lit_frames: u8,
    /// Bytes of a nametable page copied so far (`$1B:$1C`, `$20:$21`).
    copied: u16,
    /// Frames of the third screen, in fours (`$13`).
    quarters: u8,
    /// The first screen has run its course (`$13 = $80`).
    over: bool,
    /// The stars' x and y (WRAM `$6F00-$6F1F`).
    stars: [u8; 32],
    /// The mode has ended: A, B or Start held to leave it, and the frame waited.
    ended: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Start,
    Copy,
    Dark,
    First,
    FirstFadeIn,
    FirstScreen { started: bool },
    FirstFadeOut,
    SecondPage,
    SecondFadeIn,
    SecondScreen { started: bool },
    ThirdDark,
    ThirdPage,
    ThirdFadeIn,
    ThirdScreen { started: bool },
    End(bool),
    Leave(bool),
}

impl Attract {
    pub(super) fn new() -> Attract {
        Attract {
            step: Step::Start,
            fade: Fade::new(true, false),
            scroll: 0,
            lit: false,
            count: 0,
            lit_frames: 0,
            copied: 0,
            quarters: 0,
            over: false,
            stars: [0; 32],
            ended: None,
        }
    }

    /// `Some(held)` once the mode has ended, `held` when A, B or Start ended it.
    pub(super) fn ended(&self) -> Option<bool> {
        self.ended
    }

    /// Runs one frame up to its NMI wait.
    pub(super) fn step(&mut self, game: &mut Game, prg: &[u8], chr: &[u8]) {
        while !self.frame(game, prg, chr) {}
    }

    /// The step's part of the frame: true when it ended at the NMI wait.
    fn frame(&mut self, game: &mut Game, prg: &[u8], chr: &[u8]) -> bool {
        match self.step {
            Step::Start
            | Step::Copy
            | Step::Dark
            | Step::First
            | Step::FirstFadeIn
            | Step::FirstScreen { .. }
            | Step::FirstFadeOut => self.first_screen(game, prg, chr),
            Step::SecondPage | Step::SecondFadeIn | Step::SecondScreen { .. } => {
                self.second_screen(game, prg, chr)
            }
            Step::ThirdDark | Step::ThirdPage | Step::ThirdFadeIn | Step::ThirdScreen { .. } => {
                self.third_screen(game, prg, chr)
            }
            Step::End(start) => {
                dark(game, prg);
                self.step = Step::Leave(start);
                true
            }
            Step::Leave(start) => {
                if self.ended.is_none() {
                    game.scroll.mask = 0;
                    game.effects.music = 0xFF;
                    self.ended = Some(start);
                }
                true
            }
        }
    }

    /// The first screen (`$99F7-$9A6A`): the city scrolled into the second nametable,
    /// the story's next page drawn over the first.
    fn first_screen(&mut self, game: &mut Game, prg: &[u8], chr: &[u8]) -> bool {
        let held = game.world.pad & HELD_TO_LEAVE != 0;
        match self.step {
            Step::Start => {
                stop(game);
                self.step = Step::Copy;
            }
            Step::Copy => {
                copy(game, chr);
                self.step = Step::Dark;
            }
            Step::Dark => {
                blackout(game, prg);
                self.step = Step::First;
            }
            Step::First => {
                game.scroll.mask = 0x18;
                game.scroll.scroll_x = 0;
                game.scroll.scroll_y = 0;
                build_palette(prg, 0x28, 0x13, &mut game.effects.palette);
                game.effects.palette[7] = 1;
                game.effects.song = 2;
                self.fade = Fade::new(true, false);
                self.step = Step::FirstFadeIn;
                return false;
            }
            Step::FirstFadeIn => {
                if self.fade.step(game, prg) {
                    self.reset_counts();
                    self.step = Step::FirstScreen { started: false };
                }
            }
            Step::FirstScreen { started } => {
                if started && self.over {
                    self.fade = Fade::new(false, false);
                    self.step = Step::FirstFadeOut;
                    return false;
                }
                if started && held {
                    self.step = Step::End(true);
                    return false;
                }
                self.first_frame(game, prg, chr);
                self.step = Step::FirstScreen { started: true };
            }
            Step::FirstFadeOut => {
                if self.fade.step(game, prg) {
                    self.step = Step::SecondPage;
                    self.copied = 0;
                }
            }
            _ => unreachable!("first_screen: {:?}", self.step),
        }
        true
    }

    /// The second screen (`$9A6D-$9AC5`): its page into the second nametable, scrolled.
    fn second_screen(&mut self, game: &mut Game, prg: &[u8], chr: &[u8]) -> bool {
        let held = game.world.pad & HELD_TO_LEAVE != 0;
        match self.step {
            Step::SecondPage => {
                if self.copied == 0 {
                    self.reset_counts();
                    game.scroll.ppu_ctrl &= 0xFC;
                    game.scroll.mask = 0;
                    game.oam.clear();
                }
                if self.copy_page(game, chr, STORY + 0x400, 0x2400) {
                    game.scroll.mask = 0x18;
                    for (shadow, bank) in game.oam.chr[..4].iter_mut().zip(0x78..) {
                        *shadow = bank;
                    }
                    self.show(game, prg, 0x29, 0x14);
                    self.step = Step::SecondFadeIn;
                    return false;
                }
            }
            Step::SecondFadeIn => {
                if self.fade.step(game, prg) {
                    self.step = Step::SecondScreen { started: false };
                }
            }
            Step::SecondScreen { started } => {
                if started && game.scroll.scroll_x >= 0x58 {
                    self.step = Step::ThirdDark;
                    return false;
                }
                if started && held {
                    self.step = Step::End(true);
                    return false;
                }
                self.second_frame(game, prg);
                self.step = Step::SecondScreen { started: true };
            }
            _ => unreachable!("second_screen: {:?}", self.step),
        }
        true
    }

    /// The third screen (`$9AC8-$9B32`): its page into the first nametable, the figure and the stars.
    fn third_screen(&mut self, game: &mut Game, prg: &[u8], chr: &[u8]) -> bool {
        let held = game.world.pad & HELD_TO_LEAVE != 0;
        match self.step {
            Step::ThirdDark => {
                dark(game, prg);
                self.copied = 0;
                self.step = Step::ThirdPage;
            }
            Step::ThirdPage => {
                if self.copied == 0 {
                    for (i, star) in (0..).zip(&mut self.stars) {
                        *star = rom_byte(prg, 2, STARS + i);
                    }
                    self.reset_counts();
                    game.scroll.ppu_ctrl &= 0xFC;
                    game.scroll.mask = 0;
                    game.oam.clear();
                }
                if self.copy_page(game, chr, CITY, 0x2000) {
                    game.scroll.mask = 0x18;
                    for (shadow, bank) in game.oam.chr[..4].iter_mut().zip(0x64..) {
                        *shadow = bank;
                    }
                    game.scroll.scroll_x = 0;
                    game.scroll.scroll_y = 0;
                    draw_figure(game, prg);
                    self.show(game, prg, 0x2A, 0x15);
                    self.step = Step::ThirdFadeIn;
                    return false;
                }
            }
            Step::ThirdFadeIn => {
                if self.fade.step(game, prg) {
                    self.step = Step::ThirdScreen { started: false };
                }
            }
            Step::ThirdScreen { started } => {
                if !started {
                    game.oam.clear();
                    self.quarters = 0;
                } else if game.world.frame.is_multiple_of(4) {
                    self.quarters += 1;
                    if self.quarters == 0xB0 {
                        self.step = Step::End(false);
                        return false;
                    }
                }
                if started && held {
                    self.step = Step::End(true);
                    return false;
                }
                self.third_frame(game, prg);
                self.step = Step::ThirdScreen { started: true };
            }
            _ => unreachable!("third_screen: {:?}", self.step),
        }
        true
    }

    /// A frame of the second screen (`$9A9C-$9AB4`): the scroll right at a quarter pixel
    /// a frame, its sprite on even frames behind the background.
    fn second_frame(&mut self, game: &mut Game, prg: &[u8]) {
        self.scroll = self.scroll.wrapping_add(0x40);
        game.scroll.scroll_x = self.scroll.to_be_bytes()[0];
        if !game.world.frame_odd {
            let pointer = rom_word(prg, 2, SECOND_SPRITE);
            let read = |addr| rom_byte(prg, 2, addr);
            draw_sprite_with(game, read, pointer, (0x80, 0xC0), 0x20);
        }
        queue_palette(game);
        game.oam.finish();
    }

    /// A frame of the third screen (`$9B09-$9B15`): the figure and the stars.
    fn third_frame(&mut self, game: &mut Game, prg: &[u8]) {
        draw_figure(game, prg);
        self.draw_stars(game, prg);
        queue_palette(game);
        game.oam.finish();
    }

    /// `$9D79`: the counts `$10-$1F` cleared.
    fn reset_counts(&mut self) {
        self.scroll = 0;
        self.lit = false;
        self.count = 0;
        self.lit_frames = 0;
        self.quarters = 0;
        self.over = false;
        self.copied = 0;
    }

    /// One 64-byte run of a nametable page from CHR `from` to PPU `to` (`$9D3F`,
    /// `$9E29`); true, without a frame, once the page's 1 KiB is queued.
    fn copy_page(&mut self, game: &mut Game, chr: &[u8], from: usize, to: u16) -> bool {
        if self.copied == 0x400 {
            return true;
        }
        let at = usize::from(self.copied);
        game.effects
            .vram
            .push(to + self.copied, 0, &chr[from + at..from + at + 0x40]);
        self.copied += 0x40;
        false
    }

    /// A screen shown: rendering on, the scroll home, its palette (`$C367`) with each
    /// set's first colour `$03` (`$9F5B`), and the fade in (`$C2B8`).
    fn show(&mut self, game: &mut Game, prg: &[u8], background: u8, sprites: u8) {
        game.scroll.scroll_x = 0;
        game.scroll.scroll_y = 0;
        build_palette(prg, background, sprites, &mut game.effects.palette);
        for first in game.effects.palette.iter_mut().step_by(4) {
            *first = 3;
        }
        self.fade = Fade::new(true, false);
    }

    /// A frame of the first screen (`$9A45-$9A58`): its colours, its sprite while on
    /// screen, the scroll right at half a pixel a frame into the second nametable.
    fn first_frame(&mut self, game: &mut Game, prg: &[u8], chr: &[u8]) {
        self.first_colours(game, prg, chr);
        let table = u16::from(game.scroll.ppu_ctrl & 1) << 8;
        let [high, x] = 0x0170u16
            .wrapping_sub(table | u16::from(game.scroll.scroll_x))
            .to_be_bytes();
        game.work.screen_x = x;
        if high == 0 {
            let pointer = rom_word(prg, 2, FIRST_SPRITE);
            draw_sprite_with(game, |addr| rom_byte(prg, 2, addr), pointer, (x, 0x9F), 0);
        }
        let (scroll, wrapped) = self.scroll.overflowing_add(0x80);
        self.scroll = scroll;
        game.scroll.scroll_x = scroll.to_be_bytes()[0];
        if wrapped {
            self.scroll = 0xFFFF;
            game.scroll.ppu_ctrl |= 1;
            game.scroll.scroll_x = 0;
        }
        game.oam.finish();
    }

    /// The first screen's colours (`$9BDA`): one colour stepped up every eight frames,
    /// then, once the scroll has crossed into the second nametable, three colours lit
    /// in turn every four frames while the next page is drawn over the first, 64 bytes a
    /// frame from WRAM `$6000`, and the screen over after 256 frames.
    fn first_colours(&mut self, game: &mut Game, prg: &[u8], chr: &[u8]) {
        if !self.lit {
            self.count += 1;
            if self.count.is_multiple_of(8) {
                let step = self.count >> 3;
                if step == 7 {
                    self.lit = true;
                    self.count = 0;
                    self.lit_frames = 0;
                    self.copied = 0;
                    return;
                }
                game.effects.palette[7] = rom_byte(prg, 2, FIRST_COLOURS + u16::from(step - 1));
            }
            queue_palette(game);
            return;
        }
        if self.count == 0 {
            let table = game.scroll.ppu_ctrl & 3;
            if table != 0 {
                self.count = table;
            }
            return;
        }
        self.lit_frames = self.lit_frames.wrapping_add(1);
        if self.lit_frames == 0 {
            self.over = true;
            return;
        }
        if self.lit_frames.is_multiple_of(4) {
            let colour = rom_byte(prg, 2, LIT_COLOURS + u16::from((self.lit_frames >> 2) & 3));
            for at in [1, 5, 0x1D] {
                game.effects.palette[at] = colour;
            }
            queue_palette(game);
        } else if self.copied < 0x400 {
            let at = usize::from(self.copied);
            let bytes = &chr[STORY + at..STORY + at + 0x40];
            game.effects.vram.push(0x2000 + self.copied, 0, bytes);
            self.copied += 0x40;
        }
    }

    /// The third screen's sixteen stars (`$9EDF`): each drifts up, the far ones three
    /// times as fast and right every other frame, back to the bottom past `$30`.
    fn draw_stars(&mut self, game: &mut Game, prg: &[u8]) {
        for star in (0..16usize).rev() {
            let far = star >= 8;
            let size = if far { 4 } else { 0 } + if game.world.frame & 4 != 0 { 2 } else { 0 };
            let pointer = rom_word(prg, 2, STAR_SPRITES + size);
            let x = self.stars[2 * star];
            if game.world.frame_odd {
                self.stars[2 * star] = x.wrapping_add(1);
            }
            let y = self.stars[2 * star + 1];
            let rise = if far { 3 } else { 1 };
            let mut next = y.wrapping_sub(rise);
            if next < 0x30 {
                next = 0xB0;
            }
            self.stars[2 * star + 1] = next;
            draw_sprite_with(game, |addr| rom_byte(prg, 2, addr), pointer, (x, y), 0);
        }
    }
}

/// The first frame (`$99F7-$9A14`): the music stopped, the sprites parked, the CHR
/// banks to copy from (`$EC-$EF`), rendering and NMIs off (`$9985`).
fn stop(game: &mut Game) {
    game.effects.music = 0xFF;
    game.oam.clear();
    game.scroll.mirroring = 0;
    game.scroll.ppu_ctrl &= 0xFB;
    for (shadow, bank) in game.oam.chr[..4].iter_mut().zip(0xEC..) {
        *shadow = bank;
    }
    game.display.prg_windows[0] = 0xC0;
    game.scroll.room[0] = 0;
    game.scroll.ppu_ctrl &= 0x7B;
    game.scroll.mask = 0;
}

/// The copies with NMIs off (`$9B51`, `$9BA8`): both nametables written from CHR
/// `$ED-$EE` straight to the PPU, the screen's banks `$1C-$1F`, NMIs on (`$996D`).
fn copy(game: &mut Game, chr: &[u8]) {
    game.effects.vram.direct.push(VramWrite {
        addr: 0x2000,
        flags: 0,
        bytes: chr[CITY + 0x400..CITY + 0xC00].to_vec(),
    });
    for (shadow, bank) in game.oam.chr[..4].iter_mut().zip(0x1C..) {
        *shadow = bank;
    }
    game.scroll.ppu_ctrl |= 0x80;
}

/// The third screen's figure (`$9E63`) at (`$60`, `$8F`).
fn draw_figure(game: &mut Game, prg: &[u8]) {
    let pointer = rom_word(prg, 2, FIGURE);
    draw_sprite_with(
        game,
        |addr| rom_byte(prg, 2, addr),
        pointer,
        (0x60, 0x8F),
        0,
    );
}

/// Every set's first colour `$0F`, then the dark palette (`$9F69`).
fn dark(game: &mut Game, prg: &[u8]) {
    for first in game.effects.palette.iter_mut().step_by(4) {
        *first = 0x0F;
    }
    blackout(game, prg);
}
