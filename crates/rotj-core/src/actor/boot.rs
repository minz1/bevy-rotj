//! Power-on to the first loop frame: the reset (`$E2E6`, bank 14 `$D394`), the intro and
//! title payload (CHR banks `$F0-$F7` copied to WRAM by bank 2 `$98C3` and entered at
//! `$6000`), then the new game's stage start (`$D3C6`), one frame per [`Boot::step`].

use super::Game;
use super::attract::Attract;
use super::cutscene::{ScreenClear, copy_frame, draw_block, draw_sprite};
use super::sound_test::SoundTest;
use super::stage::{Fade, NextArea, blackout, queue_palette};
use crate::levelmap::BOOT_FIRST_BANK;

/// The WRAM byte at `addr` once `$98C3` has copied the payload (CHR `$F0-$F7`, in order).
fn payload_byte(chr: &[u8], addr: u16) -> u8 {
    chr[usize::from(BOOT_FIRST_BANK) * 0x400 + usize::from(addr - 0x6000)]
}

/// Where the payload's buffer starts, which the screen clear fills (`$7FC0-$7FFF`).
const BUFFER: u16 = 0x7FC0;

/// The payload's WRAM at `addr`, the buffer as `buffer` holds it.
fn wram(chr: &[u8], buffer: &[u8; 64], addr: u16) -> u8 {
    match addr.checked_sub(BUFFER) {
        Some(at) => buffer[usize::from(at)],
        None => payload_byte(chr, addr),
    }
}

fn payload_bytes(chr: &[u8], addr: u16, len: u16) -> Vec<u8> {
    (0..len).map(|i| payload_byte(chr, addr + i)).collect()
}

/// The 32-byte palette at `addr`.
fn palette(chr: &[u8], addr: u16) -> [u8; 32] {
    std::array::from_fn(|i| payload_byte(chr, addr + u16::try_from(i).expect("< 32")))
}

/// The intro's and title's palette (`$691C`).
const PALETTE: u16 = 0x691C;
/// The intro's and title's draw list (`$6601`).
const BLOCKS: u16 = 0x6601;
/// Where each of the title's twelve blocks starts in [`BLOCKS`] (`$65F5`).
const TITLE_BLOCKS: u16 = 0x65F5;
/// The flicker's sprite palettes, one every eight frames (`$693C`).
const FLICKER_PALETTES: u16 = 0x693C;
/// The title's cursor metasprite (`$6988`).
const CURSOR: u16 = 0x6988;
/// The cursor's two places: game start and password (`$2F`).
const GAME_START: u8 = 0x38;
const PASSWORD: u8 = 0x98;
/// The draw list the chosen line flashes from (`$66F3`).
const FLASH_BLOCKS: u16 = 0x66F3;
/// Frames the title waits for a choice before the attract mode (`$20:$21`).
const TITLE_FRAMES: u16 = 0x0700;
/// The buttons that skip the intro and choose on the title: A, B and Start (`$7E60`).
const CONFIRM: u8 = 0xD0;
/// A and Start held at the stage start enter the sound test (`$D3C6`).
const SOUND_TEST: u8 = 0x90;

/// The password screen's CHR banks (`$6E0A`), draw list (`$6992`) and palette (`$6DEA`).
const PASSWORD_BANKS: u16 = 0x6E0A;
const PASSWORD_BLOCKS: u16 = 0x6992;
const PASSWORD_PALETTE: u16 = 0x6DEA;
/// The entry's metasprite as the buffer starts it (`$6E12`, 38 bytes).
const ENTRY: u16 = 0x6E12;
/// Each letter's top tile (`$6E52`); the bottom one is `$10` on.
const LETTER_TILES: u16 = 0x6E52;
/// The fourteen passwords, four letters each, the stage they start by their place;
/// the fourteenth starts the thirteenth stage too (`$6E6C`).
const PASSWORDS: u16 = 0x6E6C;
/// The rejected password's message (`$6E38`) and the screen's two fixed sprites.
const REJECTED: u16 = 0x6E38;
const PASSWORD_SPRITES: [(u16, u8, u8); 2] = [(0x6EA4, 0x50, 0xA0), (0x6F0A, 0xA0, 0xC0)];

/// What the password screen shows under its two fixed sprites (`$2B`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shown {
    /// The letters from the buffer (`$2B = 0`).
    Letters,
    /// The rejection message (bit 7).
    Rejected,
    /// Nothing (`$7F`).
    Nothing,
}

/// The password screen's entry state (`$20-$29`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Entry {
    /// Each letter, 0-25 (`$20-$23`).
    letters: [u8; 4],
    /// The letter being changed (`$24`).
    column: u8,
    /// Counts frames for the blink (`$25`).
    ticks: u8,
    /// The changed letter's blink phase (`$26`).
    blink: bool,
    /// Frames to the next repeat of a held direction (`$27`).
    repeat_in: u8,
    /// The repeat period, shortening while held (`$28`).
    period: u8,
    /// A held direction repeats this frame (`$29`).
    repeat: bool,
}

/// The game from power-on, one frame per [`Boot::step`], done at the first loop frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Boot {
    step: Step,
    /// The last wait is followed by the intro's skip check (`$7E60`, `$2F` set).
    check: bool,
    clear: ScreenClear,
    /// WRAM `$7FC0-$7FFF`, which each screen clear fills.
    buffer: [u8; 64],
    fade: Fade,
    /// The draw list's read offset (`$20`).
    cursor: u8,
    /// A loop count (`$2B`).
    count: u8,
    /// Alternates every frame (`$2C`).
    toggle: bool,
    /// The flicker's y scroll (`$2D`).
    flicker: u8,
    /// The title's cursor x (`$2F`): [`GAME_START`] or [`PASSWORD`].
    choice: u8,
    /// Frames left on the title (`$20:$21`).
    timeout: u16,
    /// The level the stage start loads (`$20` at `$D3D8`).
    level: u8,
    /// The attract mode was left by Start: the intro is skipped (`$20` = `$80`).
    skip_intro: bool,
    /// A level forced at `$D3D8`, as a developer's poke would.
    forced: Option<u8>,
    entry: Entry,
    shown: Shown,
    sound_test: SoundTest,
    attract: Attract,
    stage: NextArea,
}

/// Where the boot is: a routine and its step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// The reset and the payload's copy (`$E2E6`, `$98C3`).
    Power(Power),
    /// The intro's first screen (`$6000-$604A`).
    Intro(Intro),
    /// The intro's sliding picture, its hold and its flicker (`$604A-$61AB`).
    Slide(Slide),
    /// The title drawn at once, when the intro is skipped (`$62A3`).
    TitleSetup(TitleSetup),
    /// The title and the chosen line's flash (`$61AC-$6293`).
    Title(Title),
    /// The attract mode (bank 2 `$99F7`).
    Attract,
    /// The password screen (`$6330`).
    Password(Password),
    /// The new game's start (`$D3C6`).
    StageStart,
    NewGameStart,
    NewGame,
    /// The sound test (bank 12 `$9A36`).
    SoundTest,
    /// The game resets (`JMP ($FFFC)`).
    Restart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Power {
    Reset,
    Palette,
    Payload,
    Copy(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Intro {
    Enter,
    Clear,
    Banks,
    BanksHold,
    Show,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slide {
    Setup,
    Block,
    Sliding,
    End,
    Blocks(u8),
    Settle,
    SettleBlock,
    Hold,
    HoldBlocks(u8),
    Flicker,
    TitleBlocks(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TitleSetup {
    Start,
    Clear,
    Banks,
    BanksHold,
    Palette,
    Blocks(u8),
    Show,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Title {
    Start,
    Waiting,
    Chosen,
    Flash(u8),
    FadeOut,
    Blank,
    StartClear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Password {
    Clear,
    Banks,
    Blocks(u8),
    Show,
    FadeIn,
    Start,
    Entry,
    Submitted,
    Verdict,
    Judged { accepted: bool },
    FadeOut { accepted: bool },
    EndClear { accepted: bool },
}

impl Game {
    /// The game at power-on, its RAM cleared; the [`Boot`] runs the reset first.
    /// `forced` overrides the level the title starts.
    #[must_use]
    pub fn power_on(forced: Option<u8>) -> Game {
        Game {
            slots: super::slots::Slots::default(),
            world: super::world::World {
                view: super::world::View {
                    width: 256 * 16,
                    height: 240 * 16,
                },
                ..super::world::World::default()
            },
            schedule: super::world::Schedule::default(),
            effects: super::world::Effects::default(),
            oam: super::oam::OamBuilder {
                page: [0; 256],
                left: 0,
                ..super::oam::OamBuilder::new()
            },
            work: super::engine::Work::default(),
            scroll: super::scroll::Scroll::default(),
            display: super::frame::Display::default(),
            script: Some(super::Script::Boot(Box::new(Boot::new(forced)))),
            probe: super::collision::Probe::default(),
        }
    }
}

impl Boot {
    /// The boot from the reset; `forced` overrides the level the title starts.
    #[must_use]
    pub fn new(forced: Option<u8>) -> Boot {
        Boot {
            step: Step::Power(Power::Reset),
            check: false,
            clear: ScreenClear::new(0),
            buffer: [0; 64],
            fade: Fade::new(false, false),
            cursor: 0,
            count: 0,
            toggle: false,
            flicker: 0,
            choice: GAME_START,
            timeout: 0,
            level: 0,
            skip_intro: false,
            forced,
            entry: Entry::default(),
            shown: Shown::Letters,
            sound_test: SoundTest::new(),
            attract: Attract::new(),
            stage: NextArea::new_game(),
        }
    }

    /// A reader of the payload at `addr`, the buffer (`$7FC0-$7FFF`) as it holds it.
    fn reader<'a>(&self, chr: &'a [u8]) -> impl Fn(u16) -> u8 + Copy + 'a {
        let buffer = self.buffer;
        move |addr| wram(chr, &buffer, addr)
    }

    /// True once the stage has started; the loop body runs next (`$D3F9`).
    #[must_use]
    pub fn done(&self) -> bool {
        self.step == Step::NewGame && self.stage.done()
    }

    /// True once the password screen's Select or the sound test's exit has reset the
    /// game (`JMP ($FFFC)`).
    #[must_use]
    pub fn reset(&self) -> bool {
        self.step == Step::Restart
    }

    /// True while the title waits for a choice.
    #[must_use]
    pub fn on_title(&self) -> bool {
        self.step == Step::Title(Title::Waiting)
    }

    /// Runs one frame up to its NMI wait.
    pub fn step(&mut self, game: &mut Game, prg: &[u8], chr: &[u8]) {
        if std::mem::take(&mut self.check) && game.world.pressed & CONFIRM != 0 {
            self.step = Step::TitleSetup(TitleSetup::Start);
        }
        while !self.frame(game, prg, chr) {}
    }

    /// One step's part of the frame: true when it ended at the NMI wait, false when the
    /// next step runs in the same frame.
    fn frame(&mut self, game: &mut Game, prg: &[u8], chr: &[u8]) -> bool {
        match self.step {
            Step::Power(step) => self.power_frame(step, game, prg),
            Step::Intro(step) => self.intro_frame(step, game, chr),
            Step::Slide(step) => self.slide_frame(step, game, chr),
            Step::TitleSetup(step) => self.title_setup_frame(step, game, chr),
            Step::Title(step) => self.title_frame(step, game, prg, chr),
            Step::Password(step) => self.password_frame(step, game, prg, chr),
            Step::Attract => {
                if let Some(held) = self.attract.ended() {
                    self.level = 0;
                    self.skip_intro = held;
                    self.step = Step::Power(Power::Copy(0));
                    return false;
                }
                self.attract.step(game, prg, chr);
                true
            }
            Step::StageStart => {
                if game.world.pad == SOUND_TEST {
                    self.sound_test = SoundTest::new();
                    self.step = Step::SoundTest;
                    return false;
                }
                let level = self.forced.unwrap_or(self.level);
                game.world.area = super::engine::rom_byte(prg, 14, 0xD94E + u16::from(level));
                blackout(game, prg);
                self.step = Step::NewGameStart;
                true
            }
            Step::NewGameStart => {
                new_game(game);
                self.step = Step::NewGame;
                false
            }
            Step::NewGame => {
                self.stage.step(game, prg, chr);
                true
            }
            Step::SoundTest => {
                if self.sound_test.step(game, prg) {
                    self.step = Step::Restart;
                }
                true
            }
            Step::Restart => true,
        }
    }

    /// The reset to its first NMI wait, the dark palette (`$D3BB`) and the payload's
    /// copy (`$98C3`, again from `$98CB` after the attract mode).
    fn power_frame(&mut self, step: Power, game: &mut Game, prg: &[u8]) -> bool {
        match step {
            Power::Reset => {
                reset(game);
                self.step = Step::Power(Power::Palette);
            }
            Power::Palette => {
                for first in game.effects.palette.iter_mut().step_by(4) {
                    *first = 0x0F;
                }
                blackout(game, prg);
                self.step = Step::Power(Power::Payload);
            }
            Power::Payload => {
                game.effects.music = 0xFF;
                self.level = 0;
                self.step = Step::Power(Power::Copy(0));
                return false;
            }
            Power::Copy(frame) => {
                copy_frame(game, prg, frame, BOOT_FIRST_BANK);
                self.step = if frame == 2 {
                    Step::Intro(Intro::Enter)
                } else {
                    Step::Power(Power::Copy(frame + 1))
                };
            }
        }
        true
    }

    /// The intro's first screen (`$6000-$604A`): the screen cleared, the title's banks
    /// and attributes, rendering on; skipped to the title if the attract mode was left.
    fn intro_frame(&mut self, step: Intro, game: &mut Game, chr: &[u8]) -> bool {
        match step {
            Intro::Enter => {
                game.scroll.ppu_ctrl = 0xA8;
                game.scroll.mask = 0;
                self.clear = ScreenClear::new(0);
                self.step = Step::Intro(Intro::Clear);
            }
            Intro::Clear => {
                if self.clear.frame() == 0 {
                    game.effects.palette = palette(chr, PALETTE);
                    if self.skip_intro {
                        self.step = Step::TitleSetup(TitleSetup::Start);
                        return false;
                    }
                }
                if self.clear_step(game) {
                    self.step = Step::Intro(Intro::Banks);
                    return false;
                }
                self.check = self.clear.frame() > 1;
            }
            Intro::Banks => {
                show_banks(game, chr);
                self.check = true;
                self.step = Step::Intro(Intro::BanksHold);
            }
            Intro::BanksHold => self.step = Step::Intro(Intro::Show),
            Intro::Show => {
                game.scroll.mask = 0x1E;
                queue_palette(game);
                self.step = Step::Slide(Slide::Setup);
            }
        }
        true
    }

    /// The intro's picture (`$604A-$61AB`): six blocks of the draw list, each slid in by
    /// the raster split (`$D3 = 9`), held, flickered over the first nametable, then the
    /// title's last five blocks. Every wait is followed by the skip check.
    fn slide_frame(&mut self, step: Slide, game: &mut Game, chr: &[u8]) -> bool {
        let read = self.reader(chr);
        self.check = true;
        let next = match step {
            Slide::Setup => {
                arm_slide(game);
                self.cursor = 0;
                self.count = 6;
                self.step = Step::Slide(Slide::Block);
                return false;
            }
            Slide::Block => {
                let room = &mut game.scroll.room;
                room[4] = 0;
                room[5..=10].fill(0);
                self.toggle = false;
                draw_block(game, read, BLOCKS, &mut self.cursor);
                Slide::Sliding
            }
            Slide::Sliding => {
                game.scroll.room[0] = if self.toggle { 0x80 } else { 0 };
                self.toggle = !self.toggle;
                if game.scroll.room[10] & 0x1F == 0x1F {
                    self.step = Step::Slide(Slide::End);
                    return false;
                }
                queue_palette(game);
                Slide::Sliding
            }
            Slide::End => {
                game.scroll.room[0] = 0;
                game.scroll.ppu_ctrl = 0xA8;
                queue_palette(game);
                Slide::Blocks(0)
            }
            Slide::Blocks(2) => {
                self.count -= 1;
                let next = if self.count == 0 {
                    Slide::Settle
                } else {
                    Slide::Block
                };
                self.step = Step::Slide(next);
                return false;
            }
            Slide::Blocks(drawn) => {
                draw_block(game, read, BLOCKS, &mut self.cursor);
                Slide::Blocks(drawn + 1)
            }
            Slide::Settle => {
                game.scroll.room[0] = 0;
                Slide::SettleBlock
            }
            Slide::SettleBlock => {
                draw_block(game, read, BLOCKS, &mut self.cursor);
                self.count = 0x10;
                Slide::Hold
            }
            Slide::Hold if self.count == 0 => {
                self.step = Step::Slide(Slide::HoldBlocks(0));
                return false;
            }
            Slide::Hold => {
                queue_palette(game);
                self.count -= 1;
                Slide::Hold
            }
            Slide::HoldBlocks(2) => {
                self.count = 0x20;
                self.toggle = false;
                self.flicker = 0xC0;
                self.step = Step::Slide(Slide::Flicker);
                return false;
            }
            Slide::HoldBlocks(drawn) => {
                draw_block(game, read, BLOCKS, &mut self.cursor);
                Slide::HoldBlocks(drawn + 1)
            }
            Slide::Flicker if self.flicker == 0 => {
                let scroll = &mut game.scroll;
                (scroll.scroll_x, scroll.scroll_y, scroll.mirroring) = (0, 0, 0);
                self.step = Step::Slide(Slide::TitleBlocks(0));
                return false;
            }
            Slide::Flicker => {
                self.flicker_frame(game, chr);
                Slide::Flicker
            }
            Slide::TitleBlocks(5) => {
                self.check = false;
                self.step = Step::Title(Title::Start);
                return false;
            }
            Slide::TitleBlocks(drawn) => {
                draw_block(game, read, BLOCKS, &mut self.cursor);
                Slide::TitleBlocks(drawn + 1)
            }
        };
        self.step = Step::Slide(next);
        true
    }

    /// The title drawn at once (`$62A3-$62F4`): the screen cleared, the banks and
    /// attributes, the palette, the draw list's twelve blocks, rendering on.
    fn title_setup_frame(&mut self, step: TitleSetup, game: &mut Game, chr: &[u8]) -> bool {
        let read = self.reader(chr);
        let next = match step {
            TitleSetup::Start => {
                game.effects.music = 0xFF;
                self.clear = ScreenClear::new(0);
                self.step = Step::TitleSetup(TitleSetup::Clear);
                return false;
            }
            TitleSetup::Clear => {
                if self.clear_step(game) {
                    self.step = Step::TitleSetup(TitleSetup::Banks);
                    return false;
                }
                TitleSetup::Clear
            }
            TitleSetup::Banks => {
                show_banks(game, chr);
                TitleSetup::BanksHold
            }
            TitleSetup::BanksHold => TitleSetup::Palette,
            TitleSetup::Palette => {
                game.scroll.ppu_ctrl = 0xA8;
                game.effects.vram.push(0x3F00, 0, &palette(chr, PALETTE));
                TitleSetup::Blocks(0)
            }
            TitleSetup::Blocks(12) => {
                game.scroll.mask = 0x1E;
                self.step = Step::TitleSetup(TitleSetup::Show);
                return false;
            }
            TitleSetup::Blocks(drawn) => {
                self.cursor = read(TITLE_BLOCKS + u16::from(drawn));
                draw_block(game, read, BLOCKS, &mut self.cursor);
                TitleSetup::Blocks(drawn + 1)
            }
            TitleSetup::Show => {
                self.step = Step::Title(Title::Start);
                return true;
            }
        };
        self.step = Step::TitleSetup(next);
        true
    }

    /// The title (`$61AC-$6232`): its song, the cursor moved by left, right or Select,
    /// the attract mode when left waiting; A, B or Start flash the chosen line
    /// (`$6233-$6293`) and start the game or open the password screen.
    fn title_frame(&mut self, step: Title, game: &mut Game, prg: &[u8], chr: &[u8]) -> bool {
        let read = self.reader(chr);
        let next = match step {
            Title::Start => {
                game.effects.song = 0x01;
                self.timeout = TITLE_FRAMES;
                self.choice = GAME_START;
                self.draw_title(game, chr);
                Title::Waiting
            }
            Title::Waiting => {
                self.timeout -= 1;
                if self.timeout == 0 {
                    self.attract = Attract::new();
                    self.step = Step::Attract;
                    return false;
                }
                let world = &game.world;
                if world.pressed & CONFIRM != 0 {
                    self.step = Step::Title(Title::Chosen);
                    return false;
                }
                if world.pad & 0x02 != 0 {
                    self.choice = GAME_START;
                } else if world.pad & 0x01 != 0 {
                    self.choice = PASSWORD;
                } else if world.pressed & 0x20 != 0 {
                    self.choice = if self.choice == PASSWORD {
                        GAME_START
                    } else {
                        PASSWORD
                    };
                }
                self.draw_title(game, chr);
                Title::Waiting
            }
            Title::Chosen => {
                game.effects.music = 0xFF;
                self.count = 10;
                Title::Flash(0)
            }
            Title::Flash(12) => {
                self.count -= 1;
                self.step = Step::Title(if self.count == 0 {
                    self.fade = Fade::new(false, false);
                    Title::FadeOut
                } else {
                    Title::Flash(0)
                });
                return false;
            }
            Title::Flash(phase) => {
                if phase == 0 || phase == 6 {
                    if self.count == 10 && phase == 0 {
                        game.effects.sound[0] = 0x3B;
                    }
                    self.cursor = match (phase, self.choice) {
                        (6, _) => 0,
                        (_, GAME_START) => 8,
                        _ => 0x10,
                    };
                    draw_block(game, read, FLASH_BLOCKS, &mut self.cursor);
                } else {
                    queue_palette(game);
                }
                Title::Flash(phase + 1)
            }
            Title::FadeOut => {
                if self.fade.step(game, prg) {
                    Title::Blank
                } else {
                    Title::FadeOut
                }
            }
            Title::Blank => {
                game.scroll.mask = 0;
                self.clear = ScreenClear::new(0);
                if self.choice != GAME_START {
                    self.step = Step::Password(Password::Clear);
                    return true;
                }
                self.level = 0;
                Title::StartClear
            }
            Title::StartClear => {
                if self.clear_step(game) {
                    self.step = Step::StageStart;
                    return false;
                }
                Title::StartClear
            }
        };
        self.step = Step::Title(next);
        true
    }

    /// The password screen (`$6330-$65F4`): drawn and faded in, then the entry until A,
    /// B or Start submits it or Select fades out and resets the game.
    fn password_frame(&mut self, step: Password, game: &mut Game, prg: &[u8], chr: &[u8]) -> bool {
        let read = self.reader(chr);
        let next = match step {
            Password::Clear => {
                if self.clear_step(game) {
                    self.step = Step::Password(Password::Banks);
                    return false;
                }
                Password::Clear
            }
            Password::Banks => {
                for (i, shadow) in (0..).zip(&mut game.oam.chr) {
                    *shadow = read(PASSWORD_BANKS + i);
                }
                self.cursor = 0;
                Password::Blocks(0)
            }
            Password::Blocks(11) => {
                self.step = Step::Password(Password::Show);
                return false;
            }
            Password::Blocks(drawn) => {
                draw_block(game, read, PASSWORD_BLOCKS, &mut self.cursor);
                Password::Blocks(drawn + 1)
            }
            Password::Show => {
                for (i, byte) in (0..).zip(&mut self.buffer[..0x26]) {
                    *byte = payload_byte(chr, ENTRY + i);
                }
                self.shown = Shown::Letters;
                self.draw_password(game, chr);
                game.effects.palette = palette(chr, PASSWORD_PALETTE);
                game.scroll.mask = 0x1E;
                self.fade = Fade::new(true, false);
                self.step = Step::Password(Password::FadeIn);
                return false;
            }
            Password::FadeIn => {
                if self.fade.step(game, prg) {
                    Password::Start
                } else {
                    Password::FadeIn
                }
            }
            Password::Start => {
                self.entry = Entry {
                    period: 0x1F,
                    ..Entry::default()
                };
                self.step = Step::Password(Password::Entry);
                return false;
            }
            Password::Entry => {
                if game.world.pressed & CONFIRM != 0 {
                    self.step = Step::Password(Password::Submitted);
                    return false;
                }
                if game.world.pressed & 0x20 != 0 {
                    self.fade = Fade::new(false, false);
                    self.step = Step::Password(Password::FadeOut { accepted: false });
                    return false;
                }
                self.entry_frame(game, chr);
                Password::Entry
            }
            Password::Submitted => {
                for at in (3..0x40).step_by(4) {
                    self.buffer[at] = 2;
                }
                self.shown = Shown::Letters;
                self.draw_password(game, chr);
                Password::Verdict
            }
            _ => return self.verdict_frame(step, game, prg, chr),
        };
        self.step = Step::Password(next);
        true
    }

    /// The password checked (`$654C-$65F4`): a right one flashes for 128 frames, fades out
    /// and starts its stage; a wrong one shows its message blinking for 128 frames.
    fn verdict_frame(&mut self, step: Password, game: &mut Game, prg: &[u8], chr: &[u8]) -> bool {
        let next = match step {
            Password::Verdict => {
                let accepted = self.check_password(chr);
                self.entry.ticks = 0;
                self.entry.blink = false;
                if accepted {
                    self.buffer[0x21] = 0;
                    self.buffer[0] = 0x21;
                    game.effects.sound[0] = 0x3B;
                } else {
                    game.effects.sound[0] = 0x45;
                }
                self.step = Step::Password(Password::Judged { accepted });
                return false;
            }
            Password::Judged { accepted } if self.entry.ticks == 0x80 => {
                self.step = Step::Password(if accepted {
                    self.fade = Fade::new(false, false);
                    Password::FadeOut { accepted }
                } else {
                    Password::Entry
                });
                return false;
            }
            Password::Judged { accepted } => {
                let entry = &mut self.entry;
                entry.ticks += 1;
                if entry.ticks.is_multiple_of(8) {
                    entry.blink = !entry.blink;
                }
                self.shown = match (entry.blink, accepted) {
                    (false, _) => Shown::Nothing,
                    (true, true) => Shown::Letters,
                    (true, false) => Shown::Rejected,
                };
                self.draw_password(game, chr);
                step
            }
            Password::FadeOut { accepted } => {
                if self.fade.step(game, prg) {
                    self.clear = ScreenClear::new(0);
                    Password::EndClear { accepted }
                } else {
                    step
                }
            }
            Password::EndClear { accepted } => {
                if !self.clear_step(game) {
                    return true;
                }
                self.step = if accepted {
                    Step::StageStart
                } else {
                    Step::Restart
                };
                return !accepted;
            }
            _ => unreachable!("an entry step: {step:?}"),
        };
        self.step = Step::Password(next);
        true
    }

    /// A frame of the entry (`$6399-$64A4`): the buttons, the letters into the entry's
    /// metasprite, the sprites, the blink and the repeat's count.
    fn entry_frame(&mut self, game: &mut Game, chr: &[u8]) {
        self.enter(game);
        self.show_entry(chr);
        self.shown = Shown::Letters;
        self.draw_password(game, chr);
        let entry = &mut self.entry;
        entry.ticks = entry.ticks.wrapping_add(1);
        if entry.ticks.is_multiple_of(8) {
            entry.blink = !entry.blink;
        }
        entry.repeat_in = entry.repeat_in.wrapping_sub(1);
        entry.repeat = entry.repeat_in == 0;
    }

    /// The entry's buttons (`$6399-$643A`): up and down change the letter, left and
    /// right move along; a press sounds `$39` and starts the repeat, a held direction
    /// repeats each time its count runs out, faster each time.
    fn enter(&mut self, game: &mut Game) {
        let (pressed, held) = (game.world.pressed, game.world.pad);
        let entry = &mut self.entry;
        let column = usize::from(entry.column);
        for bit in [0x08, 0x04, 0x02, 0x01] {
            if pressed & bit != 0 {
                entry.period = 0x1F;
            } else if held & bit != 0 && entry.repeat {
                entry.period -= 4;
                if entry.period == 0x0B {
                    entry.period = 0x0F;
                }
            } else {
                continue;
            }
            entry.repeat_in = entry.period;
            game.effects.sound[0] = 0x39;
            let letter = &mut entry.letters[column];
            match bit {
                0x08 => *letter = letter.checked_sub(1).unwrap_or(0x19),
                0x04 => *letter = (*letter + 1) % 0x1A,
                0x02 => entry.column = entry.column.wrapping_sub(1) & 3,
                _ => entry.column = (entry.column + 1) & 3,
            }
            return;
        }
    }

    /// The letters into the entry's metasprite (`$643C-$647F`), the changed one
    /// blinking, and the cursor under it.
    fn show_entry(&mut self, chr: &[u8]) {
        let entry = &self.entry;
        for (column, letter) in (0..4u8).zip(entry.letters) {
            let at = usize::from(1 + 8 * column);
            let tile = payload_byte(chr, LETTER_TILES + u16::from(letter));
            let attr = if column == entry.column && !entry.blink {
                0
            } else {
                2
            };
            self.buffer[at] = tile;
            self.buffer[at + 4] = tile.wrapping_add(0x10);
            self.buffer[at + 2] = attr;
            self.buffer[at + 6] = attr;
        }
        self.buffer[0x22] = 4u8.wrapping_add(0x20u8.wrapping_mul(entry.column));
    }

    /// The password screen's sprites (`$64C6`): its two fixed ones, then the entry, the
    /// rejection or nothing at (`$48`, `$E0`).
    fn draw_password(&self, game: &mut Game, chr: &[u8]) {
        let read = self.reader(chr);
        for (pointer, x, y) in PASSWORD_SPRITES {
            draw_sprite(game, read, pointer, x, y);
        }
        match self.shown {
            Shown::Letters => draw_sprite(game, read, BUFFER, 0x48, 0xE0),
            Shown::Rejected => draw_sprite(game, read, REJECTED, 0x48, 0xE0),
            Shown::Nothing => {}
        }
        game.oam.finish();
    }

    /// Whether the letters are one of the passwords, checked from the last (`$6550`);
    /// if so the level is its place, the last two both the thirteenth (`$659F`).
    fn check_password(&mut self, chr: &[u8]) -> bool {
        let letters = self.entry.letters;
        let found = (0..14u8).rev().find(|&place| {
            (0..4).all(|i| {
                payload_byte(chr, PASSWORDS + u16::from(4 * place + i)) == letters[usize::from(i)]
            })
        });
        if let Some(place) = found {
            self.level = place.min(12);
        }
        found.is_some()
    }

    /// A frame of the screen clear and the buffer it fills (`$7EE7`, `$7EF9`, `$7F38`);
    /// true, without a frame, once it has run.
    fn clear_step(&mut self, game: &mut Game) -> bool {
        let done = self.clear.step(game);
        match self.clear.frame() {
            2 => self.buffer[..32].fill(0x0F),
            3 => self.buffer.fill(0),
            _ => {}
        }
        done
    }

    /// The title's frame (`$61F7-$621A`): the cursor drawn, the sprites parked, the palette.
    fn draw_title(&self, game: &mut Game, chr: &[u8]) {
        draw_sprite(
            game,
            |addr| payload_byte(chr, addr),
            CURSOR,
            self.choice,
            0x94,
        );
        game.oam.finish();
        queue_palette(game);
    }

    /// A frame of the flicker (`$611B-$616E`): the first nametable at the rising y
    /// scroll and the second at 0 on alternate frames, a sprite palette every eight
    /// of the first 32 frames.
    fn flicker_frame(&mut self, game: &mut Game, chr: &[u8]) {
        let scroll = &mut game.scroll;
        (scroll.scroll_y, scroll.mirroring) = if self.toggle {
            (0, 3)
        } else {
            (self.flicker, 2)
        };
        self.toggle = !self.toggle;
        if self.count != 0 {
            if self.count.is_multiple_of(8) {
                let at = FLICKER_PALETTES + u16::from((self.count >> 2) - 2);
                let source = u16::from_le_bytes([payload_byte(chr, at), payload_byte(chr, at + 1)]);
                let palette = payload_bytes(chr, source, 16);
                game.effects.vram.push(0x3F00, 0, &palette);
            }
            self.count -= 1;
        }
        if self.count == 0 {
            self.flicker = self.flicker.wrapping_add(1);
            if self.flicker == 0xF0 {
                self.flicker = 0;
            }
        }
        queue_palette(game);
    }
}

/// The reset to its first NMI wait (`$E2E6-$D3B8`): RAM cleared (`$E330`, `$C117`), the
/// PRG windows (`$E337`), `$0402 = $FF`, `$0401 = 2`, the random bytes, the VRAM queue's
/// pointer and PPU control `$A8`; the sound driver's init (bank 10 `$8000`) is the app's.
fn reset(game: &mut Game) {
    game.display.prg_windows = [0x1B, 0x1E];
    game.world.room = 0xFF;
    game.effects.animated = 2;
    game.world.rng = [0x49, 0x2F];
    game.effects.vram.top = 0x7F;
    game.scroll.ppu_ctrl = 0xA8;
}

/// The intro's raster split (`$604A-$6060`): `$D3 = 9`, its first IRQ `$2680` cycles
/// after the NMI, the first nametable from y `$C0`.
fn arm_slide(game: &mut Game) {
    let room = &mut game.scroll.room;
    room[3] = 9;
    [room[1], room[2]] = [0x80, 0x26];
    game.scroll.scroll_y = 0xC0;
}

/// The title's CHR banks `$C8-$CB` and the first nametable's attribute rows from `$68F5`
/// (`$62F7`).
fn show_banks(game: &mut Game, chr: &[u8]) {
    for (bank, shadow) in (0xC8..).zip(&mut game.oam.chr[..4]) {
        *shadow = bank;
    }
    let attributes = payload_bytes(chr, 0x68F5, 0x1F);
    game.effects.vram.push(0x23D9, 0, &attributes);
}

/// A new game's state (bank 14 `$D48D`): the first room, no weapon, the meter, full
/// energy, two lives, the PRG windows and the VRAM queue reset.
fn new_game(game: &mut Game) {
    game.world.room = 0;
    let effects = &mut game.effects;
    effects.weapon = 0;
    effects.cutscene_id = 0;
    effects.meter = 0xFE;
    effects.player.energy = 0x0E;
    effects.lives = 2;
    effects.vram.top = 0x7F;
    game.display.prg_windows = [0xC0, 0x1E];
    game.oam.chr[4] = 0x9D;
}

#[cfg(test)]
mod tests {
    use super::super::oracle::{Scene, assert_boot};

    /// The buttons in the controller's shift order, as the oracle takes them.
    const A: u8 = 0x01;
    const B: u8 = 0x02;
    const SELECT: u8 = 0x04;
    const START: u8 = 0x08;
    const UP: u8 = 0x10;
    const DOWN: u8 = 0x20;
    const LEFT: u8 = 0x40;
    const RIGHT: u8 = 0x80;

    /// `tools/rotj-oracle/tests/run1.rs`'s input: Start tapped every 20 frames from 100,
    /// then right held with A and B tapped.
    fn run1(frame: u64) -> u8 {
        let mut pad = 0;
        if (100..=900).contains(&frame) && frame % 20 < 3 {
            pad |= START;
        }
        if frame > 900 {
            pad |= RIGHT;
            if frame % 60 < 3 {
                pad |= A;
            }
            if frame % 137 < 3 {
                pad |= B;
            }
        }
        pad
    }

    /// Start held for seven frames from each of `presses`, and nothing else.
    fn start_at(presses: &[u64]) -> impl Fn(u64) -> u8 {
        held(presses.iter().map(|&from| (from, START, 7)).collect())
    }

    fn boot(frames: u64, pad: impl Fn(u64) -> u8) -> Option<super::super::oracle::GameRun> {
        let scene = Scene::power_on((0..=frames).map(pad).collect())?;
        Some(assert_boot(scene, frames, None))
    }

    /// The pads of `presses`: each `(frame, buttons, frames held)`, in the shift order.
    fn held(presses: Vec<(u64, u8, u64)>) -> impl Fn(u64) -> u8 {
        move |frame| {
            let held = presses
                .iter()
                .filter(|&&(at, _, held)| (at..at + held).contains(&frame));
            held.fold(0, |pad, &(_, buttons, _)| pad | buttons)
        }
    }

    /// Right then Start on the title, and on the password screen from frame 560 each
    /// letter of `letters` stepped to (down to 13, else up), then Start at the returned
    /// frame.
    fn password(letters: [u8; 4]) -> (Vec<(u64, u8, u64)>, u64) {
        let mut presses = vec![(300, RIGHT, 5), (310, START, 5)];
        let mut at = 560;
        for letter in letters {
            let (button, count) = if letter <= 13 {
                (DOWN, letter)
            } else {
                (UP, 26 - letter)
            };
            for _ in 0..count {
                presses.push((at, button, 1));
                at += 2;
            }
            presses.push((at, RIGHT, 1));
            at += 2;
        }
        presses.push((at, START, 1));
        (presses, at)
    }

    /// The password screen: three of the fourteen passwords (`$6E6C`) start their
    /// stage, the fourteenth the thirteenth's, like the oracle.
    #[test]
    fn a_password_starts_its_stage_like_the_oracle() {
        for (letters, area) in [
            ([11, 15, 17, 25], 0x00),
            ([6, 15, 19, 22], 0x0C),
            ([13, 23, 17, 3], 0x20),
        ] {
            let (presses, end) = password(letters);
            let pads: Vec<u8> = (0..=end + 600).map(held(presses)).collect();
            let Some(mut scene) = Scene::power_on(pads.clone()) else {
                return;
            };
            scene.run(end + 600, &[]);
            assert_eq!(scene.machine.bus.ram[0x400], area, "{letters:?}");
            let run = boot(end + 600, |frame| {
                pads[usize::try_from(frame).expect("small")]
            })
            .expect("the ROM");
            assert!(run.frames > 100, "{letters:?}: {run:?}");
        }
    }

    /// A held direction repeats faster and faster; a wrong password is rejected, and
    /// Select then resets the game, like the oracle.
    #[test]
    fn a_wrong_password_is_rejected_and_select_resets() {
        let (mut presses, end) = password([1, 2, 3, 4]);
        presses.push((600, DOWN, 90));
        presses.push((end + 200, SELECT, 1));
        let Some(run) = boot(end + 400, held(presses)) else {
            return;
        };
        assert!(run.reset && run.frames == 0, "{run:?}");
    }

    /// A and Start held through the title's last frame enter the sound test (`$D3C6`):
    /// a song played, changed, stopped with B, effects played and changed both ways,
    /// then A on the last row resets the game, like the oracle.
    #[test]
    fn the_sound_test_plays_and_resets_like_the_oracle() {
        let mut presses = vec![(300, START, 1), (420, START | A, 120)];
        let mut at = 600;
        for buttons in [
            A, RIGHT, RIGHT, A, LEFT, LEFT, LEFT, A, B, DOWN, A, RIGHT, A, LEFT, LEFT, A, UP, UP,
            LEFT, RIGHT, DOWN, DOWN, DOWN, A,
        ] {
            presses.push((at, buttons, 2));
            at += 12;
        }
        let Some(run) = boot(at + 100, held(presses)) else {
            return;
        };
        assert!(run.reset && run.frames == 0, "{run:?}");
        assert!(run.script_frames > 500, "{run:?}");
    }

    /// Left on the title, the game runs the attract mode's three screens and then the
    /// intro again; A, B or Start held in each screen leaves it for the title, like the
    /// oracle.
    #[test]
    #[ignore = "long lockstep from power-on; the gates run it in release with --ignored"]
    fn the_attract_mode_plays_like_the_oracle() {
        let Some(run) = boot(4500, |_| 0) else {
            return;
        };
        assert_eq!(run.frames, 0, "{run:?}");
        for at in [2300, 3000, 3500] {
            let presses = vec![(at, START, 3), (at + 200, START, 3)];
            let Some(run) = boot(at + 800, held(presses)) else {
                return;
            };
            assert!(run.frames > 100, "Start held at {at}: {run:?}");
        }
    }

    /// The port alone, its raster IRQ model ([`raster_irqs`](super::super::cutscene::raster_irqs))
    /// run as the app runs it in place of the oracle's bytes, holds the oracle's raster
    /// bytes `$D0-$DC` at every NMI wait from power-on to the title.
    #[test]
    fn the_intro_runs_its_own_raster_split() {
        let Some(mut scene) = Scene::power_on(Vec::new()) else {
            return;
        };
        let (prg, chr) = (scene.prg(), scene.machine.bus.cart.chr.clone());
        let waits = scene.run(300, &[0xC232]);
        let mut game = super::Game::power_on(None);
        let mut raster = false;
        for (wait, oracle) in waits.iter().enumerate() {
            game.frame(&prg, &chr, &mut |_, _| {});
            assert_eq!(
                game.scroll.room[..],
                oracle.ram[0xD0..0xDD],
                "wait {wait}, frame {}",
                oracle.frame
            );
            if raster {
                super::super::raster::run_irqs(&mut game);
            }
            game.nmi(0);
            raster = game.scroll.room[0] & 0x80 != 0;
        }
        assert!(waits.len() > 280, "{} waits", waits.len());
    }

    #[test]
    fn start_on_the_title_starts_stage_1_1_like_the_oracle() {
        let Some(run) = boot(700, start_at(&[300])) else {
            return;
        };
        assert_eq!((run.script_frames, run.frames), (612, 60));
    }

    #[test]
    fn run1_skips_the_intro_and_plays_like_the_oracle() {
        let Some(run) = boot(3000, run1) else {
            return;
        };
        assert_eq!(
            (run.frames, run.pauses),
            (221, 12),
            "the last pause holds to the end"
        );
    }

    /// Start skips the intro from its every phase to the title, which a later Start
    /// leaves for stage 1-1 (a press the intro does not check is not a skip).
    #[test]
    #[ignore = "long lockstep from power-on; the gates run it in release with --ignored"]
    fn start_anywhere_in_the_intro_plays_like_the_oracle() {
        for from in [
            20, 40, 54, 55, 56, 60, 61, 75, 120, 190, 210, 250, 283, 287, 1000,
        ] {
            let Some(run) = boot(from + 900, start_at(&[from, from + 150, from + 400])) else {
                return;
            };
            println!("Start at {from}: {run:?}");
            assert!(run.frames > 30, "Start at {from}: {run:?}");
        }
    }
}
