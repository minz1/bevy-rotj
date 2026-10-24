//! The stage end (`$BA = 2`, bank 2 `$940C`), the next area's load (`$BA = 3`, bank 14
//! `$D513`) and the palette fades they share (`$C30A`, `$C35A`).

use super::Game;
use super::cutscene::Cutscene;
use super::engine::{draw, rom_byte, rom_word};
use super::room;
use super::script::{Flow, Runner, wait};
use super::world::{Motion, VramWrite, Weapon, attack, set_first_colours, sound, stage_script};
use crate::actor::button::Buttons;
use crate::actors::bank_slice;
use crate::ppulist::WriteList;
use crate::prg::Prg;
use crate::render::{Mirroring, ctrl, mask};

/// The stages there are: `$0400 / 2` indexes their tables.
const STAGES: u8 = 18;
/// The stage whose boss's defeat arms the ending (`$9C13`'s `$0400` test).
const FINAL_AREA: u8 = 0x22;

/// What the engine reads by stage: the level packs' stage descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stage {
    /// The area its CHR and screen records come from.
    pub area: u8,
    /// The banner's and the pause screen's two digits (bank 2 `$93F0`, bank 8 `$9DD2`/`$9DE4`).
    pub banner: [u8; 2],
    /// The song it requests as it starts (bank 14 `$D980`).
    pub music: Music,
    /// The picture shown before its banner (bank 2 `$93C5`).
    pub picture: Option<u8>,
    /// The checkpoint's password, letters indexing `$9E47` at `letter - $41` (bank 8 `$9DF6`);
    /// `Some` exactly on the checkpoints, where Continue restarts and the levels start.
    pub password: Option<[u8; 4]>,
    /// What clearing it does.
    pub on_clear: OnClear,
}

/// The song a stage requests as it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Music {
    /// Request this song.
    Song(u8),
    /// Keep the song playing (the boss stages keep the intro's).
    Keep,
}

/// What clearing a stage does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnClear {
    /// The next stage loads.
    NextStage,
    /// The ending plays.
    Ending,
}

impl Stage {
    /// Stage `index`'s descriptor from the tables the game reads by `$0400 / 2`.
    #[must_use]
    pub fn from_rom(prg: &[u8], index: u8) -> Stage {
        let area = index * 2;
        let at = u16::from(index);
        let digits = rom_byte(prg, 2, 0x93F0 + at);
        let song = rom_byte(prg, 14, 0xD980 + at);
        let picture = rom_byte(prg, 2, 0x93C5 + at);
        let letters = rom_word(prg, 8, 0x9DF6 + 2 * at);
        Stage {
            area,
            banner: [digits >> 4, digits & 0x0F],
            music: if song & 0x80 == 0 {
                Music::Song(song)
            } else {
                Music::Keep
            },
            picture: (picture & 0x80 == 0).then_some(picture),
            password: (rom_byte(prg, 14, 0xD57D + at) == area)
                .then(|| [0, 1, 2, 3].map(|i| rom_byte(prg, 8, letters + i))),
            on_clear: if area == FINAL_AREA {
                OnClear::Ending
            } else {
                OnClear::NextStage
            },
        }
    }

    /// Every stage's descriptor from the ROM.
    #[must_use]
    pub fn all_from_rom(prg: &[u8]) -> Vec<Stage> {
        (0..STAGES)
            .map(|index| Stage::from_rom(prg, index))
            .collect()
    }
}

/// The stage `$0400` = `area` plays.
#[must_use]
pub fn at(stages: &[Stage], area: u8) -> &Stage {
    &stages[usize::from(area >> 1)]
}

/// The latest checkpoint at or before `area`'s stage, as an area: where Continue restarts
/// and whose password the pause shows.
///
/// # Panics
/// If no checkpoint precedes it.
#[must_use]
pub fn checkpoint(stages: &[Stage], area: u8) -> u8 {
    let index = (0..=area >> 1)
        .rev()
        .find(|&index| stages[usize::from(index)].password.is_some())
        .expect("the first stage is a checkpoint");
    index * 2
}

/// The area level `level` starts at: the `level`-th checkpoint.
#[must_use]
pub fn level_area(stages: &[Stage], level: usize) -> Option<u8> {
    (0..=u8::MAX)
        .zip(stages)
        .filter(|(_, stage)| stage.password.is_some())
        .nth(level)
        .map(|(index, _)| index * 2)
}

/// The stage-end script's state (`$0408-$040C`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StageEnd {
    /// The script's phase, 0-4 (`$0408`).
    pub phase: u8,
    /// The phase's frame counter (`$0409`).
    pub timer: u8,
    /// The departing sprite's screen x (`$040A`).
    pub x: u8,
    /// The departing sprite's screen y (`$040B`).
    pub y: u8,
    /// The closing fade's brightness cap (`$040C`).
    pub fade: u8,
}

/// One frame of the stage-end script (`$940C`): true when it ended the stage and
/// waited for the NMI itself (`$9539`).
///
/// # Panics
/// On a phase past the script's five (`$95AD`).
pub fn stage_end(game: &mut Game, prg: &Prg) -> bool {
    let x = game.effects.player.x;
    let end = &mut game.effects.stage_end;
    let mut pads = None;
    match end.phase {
        0 => {
            if x >= 0x2000 || (x >= 0x1F60 && !game.effects.player.faces_left()) {
                game.effects.music = sound::music::FADE_OUT;
                end.phase = 1;
                for projectile in &mut game.effects.projectiles {
                    projectile.kind = 0;
                }
                game.effects.player.power_up = 0;
                game.effects.attack &= attack::CHARGEABLE;
            }
        }
        1 => {
            if x >= 0x2740 {
                game.effects.palette[25..28].fill(0x0F);
                queue_palette(game);
                let end = &mut game.effects.stage_end;
                (end.timer, end.phase, end.y, end.x) = (0, 2, 0x70, 0xFF);
                game.effects.song = sound::song::STAGE_CLEAR;
                pads = Some(Buttons::NONE);
            } else {
                pads = Some(Buttons::RIGHT);
            }
        }
        2 => {
            depart(game, prg, 0x03, 0x0F);
            let end = &mut game.effects.stage_end;
            end.timer = end.timer.wrapping_add(1);
            if end.timer >= 0x80 {
                (end.phase, end.timer) = (3, 0);
            }
            pads = Some(Buttons::NONE);
        }
        3 => {
            depart(game, prg, 0x03, 0x0F);
            let end = &mut game.effects.stage_end;
            end.timer = end.timer.wrapping_add(1);
            let timer = end.timer;
            if timer < 0x40 {
                if timer.is_multiple_of(16) {
                    for (i, entry) in game.effects.palette[24..28].iter_mut().enumerate() {
                        let at = u16::from(timer >> 2) + u16::try_from(i).expect("< 4");
                        *entry = rom_byte(prg, 2, 0x95B9 + at);
                    }
                    queue_palette(game);
                }
            } else if timer == 0x60 {
                (end.phase, end.timer) = (4, 0);
            }
            pads = Some(Buttons::UP);
        }
        4 => {
            end.timer = end.timer.wrapping_add(1);
            let timer = end.timer;
            if timer >= 0x90 {
                if timer == 0x90 {
                    game.effects.music = sound::music::FADE_OUT;
                    end.fade = 0x40;
                }
                if timer.is_multiple_of(16) {
                    end.fade = end.fade.wrapping_sub(0x10);
                    if end.fade & 0x80 != 0 {
                        game.effects.hud = 0;
                        game.effects.transition = 3;
                        blackout(game, prg);
                        return true;
                    }
                }
                let cap = end.fade;
                queue_faded(game, cap);
            }
            depart(game, prg, 0x07, 0x1F);
            pads = Some(Buttons::UP);
        }
        other => panic!("stage-end phase {other} is past $940C's table"),
    }
    if let Some(pad) = pads {
        (game.world.pad, game.world.pressed) = (pad, pad);
    }
    false
}

/// Moves the departing sprite up and left on the timer's beats and draws it (`$955A`, `$9574`).
fn depart(game: &mut Game, prg: &Prg, x_beat: u8, y_beat: u8) {
    let end = &mut game.effects.stage_end;
    if end.timer & x_beat == 0 {
        end.x = end.x.wrapping_sub(1);
        if end.timer & y_beat == 0 {
            end.y = end.y.wrapping_sub(1);
        }
    }
    let (x, y) = (end.x, end.y);
    let (mut cx, work) = game.parts(prg);
    work.screen_x = x;
    work.screen_y = y;
    work.pointer = rom_word(prg, 2, 0x95B7);
    work.chr_slot = 7;
    work.attr = 2;
    work.anchor = 0;
    work.tile = 0xC0;
    draw(work, 1, &mut cx);
}

/// Queues the palette shadow (`$C34B`).
pub(super) fn queue_palette(game: &mut Game) {
    let effects = &mut game.effects;
    effects.vram.push_palette(&effects.palette);
}

/// Queues the palette with every colour's brightness capped at `cap` (`$C30A`, `X = $FF`).
fn queue_faded(game: &mut Game, cap: u8) {
    let mut faded = game.effects.palette;
    for (i, colour) in faded.iter_mut().enumerate() {
        if !i.is_multiple_of(4) && *colour >= cap {
            *colour = (*colour & 0x0F) | cap;
        }
    }
    game.effects.vram.push_palette(&faded);
}

/// Builds and queues the dark palette, background and sprite set 0 (`$C35A`, before its NMI wait).
pub(super) fn blackout(game: &mut Game, prg: &[u8]) {
    room::build_palette(prg, 0, 0, &mut game.effects.palette);
    queue_palette(game);
}

/// The full meter's power-up, `$BA = 5` (`$D58F`), one frame per [`PowerUp::step`]:
/// 38 frames of the background flashing (counted in `$0408`), then
/// [`PowerUp::finish`] at the next frame's start.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PowerUp {
    frames: u8,
}

impl PowerUp {
    /// True once the 38 flashing frames have run.
    #[must_use]
    pub fn done(&self) -> bool {
        self.frames == 0x26
    }

    /// One flashing frame up to its NMI wait: the effect `$31`, every projectile
    /// freed and `$C2` kept in `$0409` on the first, then each palette's first colour
    /// `$30` every fourth frame and `$0F` otherwise, the scroll and the palette upload.
    pub fn step(&mut self, game: &mut Game, prg: &[u8]) {
        let effects = &mut game.effects;
        if self.frames == 0 {
            effects.sound[0] = sound::POWER_UP;
            for projectile in &mut effects.projectiles {
                projectile.kind = 0;
            }
            effects.attack &= attack::CHARGEABLE;
            effects.stage_end.timer = effects.scroll_lock;
            effects.scroll_lock = 0xFF;
            effects.stage_end.phase = 0;
            effects.stage_end.x = 0;
        } else {
            effects.stage_end.phase = effects.stage_end.phase.wrapping_add(1);
        }
        self.frames += 1;
        let flash = u16::from(effects.stage_end.phase.is_multiple_of(4));
        let colour = rom_byte(prg, 14, 0xD60B + flash);
        set_first_colours(&mut effects.palette, colour);
        effects.palette_dirty = 0x80;
        super::scroll::step(game, prg);
        super::frame::queue_palette(game);
    }

    /// The power-up's end (`$D5E6-$D60A`): Batman held in motion 8 (his motion kept
    /// in `$040F`), the odd-frame flag set, and four pieces of weapon 6 placed around
    /// him (`$D64A`), which close in through the weapons' handler 6.
    pub fn finish(game: &mut Game, prg: &[u8]) {
        let effects = &mut game.effects;
        effects.stage_end.phase = effects.stage_end.phase.wrapping_add(1);
        let player = &mut effects.player;
        player.stage_script = stage_script::FROZEN;
        effects.transition = 0;
        effects.stage_args[1] = player.motion.byte();
        player.motion = Motion::PoweringUp;
        game.world.frame_odd = true;
        effects.weapon = Weapon::PowerUpPieces;
        let (x, y) = (player.x, player.y);
        for (index, piece) in (0..4u16).zip(effects.projectiles.iter_mut()) {
            let byte = |table: u16| rom_byte(prg, 14, table + index);
            let word = |hi: u16, lo: u16| u16::from_be_bytes([byte(hi), byte(lo)]);
            *piece = super::world::Projectile {
                kind: 6,
                flags: 0xE0,
                damage: 0,
                vx: word(0xD6C3, 0xD6BF).cast_signed(),
                vy: word(0xD6CB, 0xD6C7).cast_signed(),
                param: u8::try_from(index).expect("< 4"),
                part: 0,
                part_timer: byte(0xD6CF),
                x: x.wrapping_add(word(0xD6B3, 0xD6AF)),
                y: y.wrapping_add(word(0xD6BB, 0xD6B7)),
                ..*piece
            };
        }
    }
}

/// A fade in or out, one frame per [`Fade::step`] (`$C2B8`, `$C2E1`, `$B237`, `$D61B`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Fade {
    /// The brightness cap (`$10`, `$12`).
    cap: u8,
    /// Frames left at this cap (`$11`).
    ticks: u8,
    /// Brightening from black (`$C2B8`, `$B237`) rather than darkening.
    rising: bool,
    /// Whether each frame also streams the scroll (`$D29B`).
    streams: bool,
}

impl Fade {
    pub(super) fn new(rising: bool, streams: bool) -> Fade {
        Fade {
            cap: if rising { 0 } else { 0x30 },
            ticks: 3,
            rising,
            streams,
        }
    }

    /// One frame of the fade, up to its NMI wait; true on its last frame.
    pub(super) fn step(&mut self, game: &mut Game, prg: &[u8]) -> bool {
        self.ticks = self.ticks.wrapping_sub(1);
        let mut last = false;
        if self.ticks & 0x80 != 0 {
            if !self.rising {
                self.cap = self.cap.wrapping_sub(0x10);
                if self.cap & 0x80 != 0 {
                    blackout(game, prg);
                    return true;
                }
            }
            queue_faded(game, self.cap);
            self.ticks = 3;
            if self.rising {
                self.cap = self.cap.wrapping_add(0x10);
                last = self.cap & 0x80 != 0;
            }
        }
        if self.streams {
            super::scroll::step(game, prg);
        }
        last
    }
}

/// The next area's load `$BA = 3` starts (`$D513`), one frame per [`NextArea::step`]:
/// the fade out, the stage banner (bank 2 `$92AD`) and the area's start (bank 13 `$B200`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextArea {
    step: Step,
    /// Where the load is in [`FADE_OUT`], [`BANNER`] or [`AREA`].
    runner: Runner,
    /// The area restarts after a life lost (`$D527`) instead of advancing.
    life_lost: bool,
    /// A new game's first area (`$D3D8`), loaded over the boot's payload.
    new_game: bool,
    /// The continue screen, once the last life is gone.
    screen: Option<Continue>,
    /// The boss room's start, when `$0404` bit 7 selects it (`$BB3D`).
    intro: BossIntro,
    /// The banner's cutscene, when the area has one and `$0406` is set (`$98BD`).
    cutscene: Option<Cutscene>,
    fade: Fade,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// The fade out, the rows of [`FADE_OUT`].
    FadeOut,
    Banner,
    Cutscene,
    Continue,
    /// The banner shown and held, the rows of [`BANNER`].
    ShowBanner,
    Clear,
    BossIntro,
    /// The area loaded and faded in, the rows of [`AREA`].
    Area,
    Reset,
    Done,
}

/// The load's rows: before the banner, for the banner and for the area's start, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoadRow {
    /// The frame's sprites finished.
    FinishSprites,
    /// A fade, rising or not and streaming the scroll or not, `Done` in its last frame.
    Fade(bool, bool),
    /// Frames the screen stays as it is.
    Wait(u16),
    /// The banner drawn and shown.
    Draw,
    /// The banner's palette (`$1B`).
    Palette,
    /// Frames the banner holds, or less once A, B or Start is pressed.
    Hold(u16),
    /// The area start's first frame.
    ClearForArea,
    /// The area loaded.
    Load,
    /// Rendering on.
    Show,
    /// The area's song requested and its room's parameters read.
    Enter,
    /// The load over, `$AF` and `$BA` cleared.
    Finish,
}

const FADE_OUT: &[LoadRow] = &[
    LoadRow::FinishSprites,
    LoadRow::Fade(false, true),
    LoadRow::Wait(1),
];
const BANNER: &[LoadRow] = &[
    LoadRow::Draw,
    LoadRow::Palette,
    LoadRow::Fade(true, false),
    LoadRow::Wait(2),
    // the original's $40, set in the second frame of Wait(2)
    LoadRow::Hold(0x3F),
    LoadRow::Fade(false, false),
    LoadRow::Wait(1),
];
const AREA: &[LoadRow] = &[
    LoadRow::ClearForArea,
    LoadRow::Load,
    LoadRow::Show,
    LoadRow::Enter,
    LoadRow::Fade(true, true),
    LoadRow::Finish,
];

impl Default for NextArea {
    fn default() -> Self {
        NextArea {
            step: Step::FadeOut,
            runner: Runner::default(),
            life_lost: false,
            new_game: false,
            screen: None,
            intro: BossIntro::default(),
            cutscene: None,
            fade: Fade::new(false, true),
        }
    }
}

impl NextArea {
    /// The restart after a life lost, `$BA = 4` (`$D527`): the same fade, banner and
    /// area start, with a life taken and the area kept.
    #[must_use]
    pub fn life_lost() -> NextArea {
        NextArea {
            life_lost: true,
            ..NextArea::default()
        }
    }

    /// A new game's banner and first area (bank 2 `$92AD`, bank 13 `$B200`), the area
    /// already chosen (`$D3D8`).
    #[must_use]
    pub fn new_game() -> NextArea {
        NextArea {
            step: Step::Banner,
            new_game: true,
            ..NextArea::default()
        }
    }

    /// What WRAM holds when the area loads: the boot's payload, a cutscene's, or the
    /// last area.
    fn leftover(&self) -> room::Leftover {
        if self.new_game {
            room::Leftover::Boot
        } else if self.cutscene.is_some() {
            room::Leftover::Cutscene
        } else {
            room::Leftover::LastArea
        }
    }

    /// True once the last frame has run and the loop resumes (`$D46C`).
    #[must_use]
    pub fn done(&self) -> bool {
        self.step == Step::Done
    }

    /// True once GAME OVER has run its course and the game resets (`JMP ($FFFC)`).
    #[must_use]
    pub fn reset(&self) -> bool {
        self.step == Step::Reset
    }

    /// True while the banner's cutscene runs.
    #[cfg(test)]
    pub(crate) fn in_cutscene(&self) -> bool {
        self.step == Step::Cutscene
    }

    /// Runs one frame of the load, up to its NMI wait.
    ///
    /// # Panics
    /// On a level that does not decode.
    pub fn step(&mut self, game: &mut Game, prg: &Prg, chr: &[u8]) {
        loop {
            match self.step {
                Step::FadeOut => {
                    if self.table_frame(FADE_OUT, game, prg, chr) {
                        return;
                    }
                    self.step = Step::Banner;
                }
                Step::Banner => {
                    if self.banner(game, prg) {
                        return;
                    }
                }
                Step::Cutscene => return self.cutscene_frame(game, prg, chr),
                Step::ShowBanner => {
                    if self.table_frame(BANNER, game, prg, chr) {
                        return;
                    }
                    self.step = Step::Clear;
                }
                Step::Continue => {
                    if self.continue_screen(game, prg) {
                        return;
                    }
                }
                Step::Clear if game.effects.hud & 0x80 != 0 => {
                    self.step = Step::BossIntro;
                }
                Step::Clear => self.begin(Step::Area),
                Step::BossIntro => {
                    if self.intro.step(game, prg, chr) {
                        self.step = Step::Done;
                    }
                    return;
                }
                Step::Area => {
                    if self.table_frame(AREA, game, prg, chr) {
                        return;
                    }
                    self.step = Step::Done;
                }
                Step::Reset | Step::Done => return,
            }
        }
    }

    /// Starts `step` at the top of its table.
    fn begin(&mut self, step: Step) {
        self.step = step;
        self.runner = Runner::default();
    }

    /// Runs `table`'s rows up to the frame's end (true) or until it runs out.
    fn table_frame(&mut self, table: &[LoadRow], game: &mut Game, prg: &Prg, chr: &[u8]) -> bool {
        while let Some(&row) = self.runner.row(table) {
            let frame = self.runner.frame;
            let flow = match row {
                LoadRow::FinishSprites => {
                    game.oam.finish();
                    Flow::Done
                }
                LoadRow::Fade(rising, streams) => {
                    if frame == 0 {
                        self.fade = Fade::new(rising, streams);
                    }
                    if self.fade.step(game, prg) {
                        Flow::Done
                    } else {
                        Flow::Waited
                    }
                }
                LoadRow::Wait(count) => wait(frame, count),
                LoadRow::Draw => {
                    if frame == 0 {
                        show_banner(game, prg);
                    }
                    wait(frame, 1)
                }
                LoadRow::Palette => {
                    room::build_palette(prg, 0x1B, 0, &mut game.effects.palette);
                    Flow::Done
                }
                LoadRow::Hold(count) => {
                    if frame >= count || game.world.pressed.intersects(Buttons::ACCEPT) {
                        Flow::Done
                    } else {
                        Flow::Waited
                    }
                }
                LoadRow::ClearForArea => {
                    if frame == 0 {
                        clear_for_area(game);
                    }
                    wait(frame, 1)
                }
                LoadRow::Load => {
                    if frame == 0 {
                        start_area(game, prg, chr, self.leftover());
                    }
                    wait(frame, 1)
                }
                LoadRow::Show => {
                    if frame == 0 {
                        game.scroll.mask |= mask::RENDERING;
                    }
                    wait(frame, 1)
                }
                LoadRow::Enter => {
                    request_song(game, prg);
                    room::parameters(game, prg);
                    Flow::Done
                }
                LoadRow::Finish => {
                    game.effects.busy = 0;
                    game.effects.transition = 0;
                    Flow::Done
                }
            };
            if self.runner.advance(flow) {
                return true;
            }
        }
        false
    }

    /// The area's indices, or a life taken (`$D527`), then the banner's dark screen;
    /// true when the frame ended, false when the last life went and the continue
    /// screen takes over.
    fn banner(&mut self, game: &mut Game, prg: &Prg) -> bool {
        if !self.life_lost {
            if !self.new_game {
                game.world.area = game.world.area.wrapping_add(2);
            }
        } else if self.screen.is_none() {
            let lives = &mut game.effects.lives;
            *lives = lives.wrapping_sub(1);
            if *lives & 0x80 != 0 {
                self.screen = Some(Continue::default());
                self.step = Step::Continue;
                return false;
            }
        }
        game.world.room = 0;
        if let Some(scene) = at(&prg.stages, game.world.area).picture
            && game.effects.cutscene_id != 0
        {
            game.effects.cutscene_id = 0;
            self.cutscene = Some(Cutscene::new(scene));
            self.step = Step::Cutscene;
            return false;
        }
        hide_for_banner(game, prg);
        self.begin(Step::ShowBanner);
        true
    }

    /// A frame of the banner's cutscene, and the dark screen after its last.
    fn cutscene_frame(&mut self, game: &mut Game, prg: &[u8], chr: &[u8]) {
        let cutscene = self.cutscene.as_mut().expect("the cutscene");
        if cutscene.step(game, prg, chr) {
            hide_for_banner(game, prg);
            self.begin(Step::ShowBanner);
        }
    }

    /// A frame of the continue screen; false when Start continued and the stage's first
    /// area loads in this frame (`$D53B`: two lives, `$0404` clear).
    fn continue_screen(&mut self, game: &mut Game, prg: &Prg) -> bool {
        let screen = self.screen.as_mut().expect("the continue screen");
        match screen.step(game, prg) {
            Outcome::Running => true,
            Outcome::Continued => {
                game.world.area = checkpoint(&prg.stages, game.world.area);
                game.effects.lives = 2;
                game.effects.hud = 0;
                self.step = Step::Banner;
                false
            }
            Outcome::Reset => {
                self.step = Step::Reset;
                true
            }
        }
    }
}

/// The dark screen before the banner (`$92C6-$92DE`).
fn hide_for_banner(game: &mut Game, prg: &[u8]) {
    game.scroll.mirroring = Mirroring::SingleLow;
    game.scroll.split.armed = false;
    game.scroll.mask = 0;
    game.scroll.ppu_ctrl &= !(ctrl::NMI | ctrl::NAMETABLE);
    game.effects.music = sound::music::STOP;
    blackout(game, prg);
}

/// The banner drawn with rendering off, its font banks and the NMI's scroll
/// (`$92E1-$93A4`).
fn show_banner(game: &mut Game, prg: &Prg) {
    game.scroll.scroll_y = 0;
    game.scroll.scroll_x = 4;
    let banner = banner(prg, game.world.area);
    game.effects.vram.direct.extend(banner);
    for (bank, shadow) in (0xCC..).zip(&mut game.oam.chr[..4]) {
        *shadow = bank;
    }
    game.scroll.ppu_ctrl |= ctrl::NMI;
    game.scroll.mask = mask::RENDERING;
}

/// The area start (bank 13 `$B200`) up to its scroll reset (`$BE6E`), over what
/// `leftover` says WRAM holds.
fn start_area(game: &mut Game, prg: &[u8], chr: &[u8], leftover: room::Leftover) {
    room::enter_area(game, prg, chr, leftover);
    game.scroll.scroll_x = 0;
    game.scroll.scroll_y = 0;
    game.scroll.ppu_ctrl = (game.scroll.ppu_ctrl & !ctrl::NAMETABLE) | ctrl::NMI;
}

/// The area start's first frame (bank 13 `$BD4D-$BD51`, `$BE58`): the music
/// stopped, every sprite parked, the IRQ split off.
fn clear_for_area(game: &mut Game) {
    game.effects.music = sound::music::STOP;
    game.oam.clear();
    game.scroll.split.armed = false;
    game.scroll.ppu_ctrl &= !ctrl::NMI;
    game.scroll.mask = 0;
}

/// The boss room's start (bank 13 `$BB3D`, `$0404` bit 7), one frame per
/// [`BossIntro::step`]: the boss's name card slides in from the right and fades
/// to white (`$BB81`), then the room loads and its page is drawn (`$BD6C`), the
/// boss's hit points are set and Batman enters in motion 9.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BossIntro {
    step: Intro,
    /// The card's phase: 0 sliding in, 1 holding, 2 fading to white (`$12`).
    phase: u8,
    /// The slide's speed (`$10:$11`).
    speed: u16,
    /// How far the card has slid (`$14:$15`).
    offset: u16,
    /// The fade's frame count (`$13`).
    fades: u8,
    /// The fade's brightness cap, then the hold's frames left (`$18`).
    cap: u8,
    /// The arrangement the room loader set, restored at the end (`$1F`).
    mirroring: Mirroring,
    /// The room's page draw (`$B351`).
    page: Option<room::PageDraw>,
    /// The bottom rows' 64-byte runs queued so far.
    rows: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Intro {
    #[default]
    Clear,
    Card,
    Show,
    Start,
    Slide,
    Page,
    Rows,
    Attributes,
    Enter,
    Done,
}

impl BossIntro {
    /// Runs one frame up to its NMI wait; true on the last.
    pub fn step(&mut self, game: &mut Game, prg: &Prg, chr: &[u8]) -> bool {
        match self.step {
            Intro::Clear => {
                game.effects.music = sound::music::STOP;
                for slot in &mut game.slots.main {
                    slot.kind = 0;
                }
                for slot in &mut game.slots.second {
                    slot.kind = 0;
                }
                game.scroll.mirroring = Mirroring::SideBySide;
                clear_for_area(game);
                self.step = Intro::Card;
            }
            Intro::Card => {
                room::load_level(game, prg, chr, room::Leftover::LastArea);
                let vram = &mut game.effects.vram;
                vram.direct.push(VramWrite {
                    addr: 0x2000,
                    flags: 0,
                    bytes: vec![0; 2048],
                });
                for (row, addr) in (0u16..).zip([0x2160, 0x2180, 0x21A0, 0x21C0, 0x21E0]) {
                    let bytes = (0..5).map(|i| rom_byte(prg, 13, 0xBEDD + row * 5 + i));
                    vram.direct.push(VramWrite {
                        addr,
                        flags: 0,
                        bytes: bytes.collect(),
                    });
                }
                game.scroll.scroll_x = 0;
                game.scroll.scroll_y = 0;
                game.scroll.ppu_ctrl = (game.scroll.ppu_ctrl & !ctrl::NAMETABLE) | ctrl::NMI;
                self.step = Intro::Show;
            }
            Intro::Show => {
                game.scroll.mask |= mask::RENDERING;
                self.step = Intro::Start;
            }
            Intro::Start => {
                game.effects.song = sound::song::BOSS_CARD;
                let sprites = game.effects.stage_args[0];
                room::build_palette(prg, 0x0F, sprites, &mut game.effects.palette);
                queue_palette(game);
                for i in 0..3u8 {
                    let bank = rom_byte(prg, 13, 0xBCB0 + u16::from(i));
                    game.oam.chr[usize::from(i)] = bank;
                    game.oam.chr[4 + usize::from(i)] = bank;
                }
                self.step = Intro::Slide;
                self.slide(game, prg);
            }
            Intro::Slide => {
                if self.slide(game, prg) {
                    self.enter_room(game, prg);
                }
            }
            Intro::Page => {
                let page = self.page.as_mut().expect("the page draw");
                if page.step(game) {
                    self.step = Intro::Rows;
                }
            }
            Intro::Rows => {
                let at = rom_word(prg, 14, 0xD87C) + u16::from(self.rows) * 0x40;
                let bytes: Vec<u8> = (0..0x40).map(|i| rom_byte(prg, 14, at + i)).collect();
                let addr = 0x2300 + u16::from(self.rows) * 0x40;
                game.effects.vram.push(addr, 0, &bytes);
                self.rows += 1;
                if self.rows == 3 {
                    self.step = Intro::Attributes;
                }
            }
            Intro::Attributes => {
                let bytes: Vec<u8> = (0..0x10).map(|i| rom_byte(prg, 14, 0xD93E + i)).collect();
                game.effects.vram.push(0x23F0, 0, &bytes);
                self.step = Intro::Enter;
            }
            Intro::Enter => {
                self.enter(game, prg);
                self.step = Intro::Done;
                return true;
            }
            Intro::Done => return true,
        }
        false
    }

    /// One frame of the card (`$BB81`, through `$BC9A[$12]`, and the draw `$BC3E`);
    /// true when the fade has run and the room loads in this frame instead.
    fn slide(&mut self, game: &mut Game, prg: &Prg) -> bool {
        match self.phase {
            0 => {
                self.speed = self.speed.wrapping_add(0x20);
                if self.speed >> 8 >= 0x0C {
                    self.speed = 0x0600;
                }
                self.offset = self.offset.wrapping_add(self.speed);
                if self.offset >> 8 >= 0x50 {
                    self.offset = 0x5000;
                    self.cap = 0x60;
                    self.phase = 1;
                    let vram = &mut game.effects.vram;
                    vram.push(
                        0x21A5,
                        0,
                        &[rom_byte(prg, 13, 0xBCA0), rom_byte(prg, 13, 0xBCA1)],
                    );
                    vram.push(
                        0x21C5,
                        0,
                        &[rom_byte(prg, 13, 0xBCA2), rom_byte(prg, 13, 0xBCA3)],
                    );
                } else if self.fades < 0x0A {
                    if self.fades.is_multiple_of(2) {
                        queue_faded(game, self.cap);
                        self.cap = self.cap.wrapping_add(0x10);
                    }
                    self.fades += 1;
                }
            }
            1 => {
                self.cap = self.cap.wrapping_sub(1);
                if self.cap == 0 {
                    (self.phase, self.fades, self.cap) = (2, 0, 0);
                }
            }
            _ => {
                if self.fades < 0x0A {
                    if self.fades.is_multiple_of(2) {
                        queue_whitened(game, self.cap);
                        self.cap = self.cap.wrapping_add(0x10);
                    }
                } else if self.fades == 0x0A {
                    game.effects.palette.fill(0x30);
                    queue_palette(game);
                } else if self.fades >= 0x0E {
                    return true;
                }
                self.fades += 1;
            }
        }
        let [hi, lo] = self.offset.wrapping_neg().to_be_bytes();
        let scroll = hi.wrapping_add(u8::from(lo & 0x80 != 0));
        game.scroll.scroll_x = scroll;
        game.scroll.ppu_ctrl |= ctrl::NAMETABLE_X;
        let (mut cx, work) = game.parts(prg);
        work.anchor = 0;
        work.attr = 0;
        work.chr_slot = 0;
        work.tile = 0;
        work.screen_x = scroll.wrapping_sub(0x14);
        work.screen_y = 0x87;
        work.pointer = rom_word(prg, 13, 0xBCA4);
        draw(work, 13, &mut cx);
        let boss = u16::from(cx.effects.stage_args[1]);
        work.pointer = rom_word(prg, 13, 0xBCA6 + 2 * boss);
        work.attr = 0;
        draw(work, 13, &mut cx);
        game.oam.finish();
        false
    }

    /// The room's load in the card's last frame (`$BD6C`): the room loader, the
    /// player's screen position, the first row of spawns and the page draw's first row.
    fn enter_room(&mut self, game: &mut Game, prg: &[u8]) {
        room::load(game, prg);
        let player = &mut game.effects.player;
        player.screen_x = (player.x >> 4).to_le_bytes()[0];
        player.screen_y = (player.y >> 4).to_le_bytes()[0];
        self.mirroring = game.scroll.mirroring;
        room::spawn_first_row(game, prg);
        let mut page = room::PageDraw::new(game.scroll.start, 0x60, 0x20, game.scroll.mirroring);
        page.step(game);
        self.page = Some(page);
        self.step = Intro::Page;
    }

    /// The room's start (`$BDE2-$BE47`): the arrangement back, the room's parameters,
    /// every sprite parked, the palette's first colours black and the sprite set
    /// from `$BE4B`, the boss's hit points (`$BE4E[$040F]`), the tally's target
    /// `$1F40`, Batman in motion 9, the palette queued, the area's song and `$BA = 0`.
    fn enter(&self, game: &mut Game, prg: &Prg) {
        game.scroll.ppu_ctrl &= !ctrl::NAMETABLE;
        game.scroll.mirroring = self.mirroring;
        room::parameters(game, prg);
        game.oam.clear();
        let effects = &mut game.effects;
        set_first_colours(&mut effects.palette, 0x0F);
        for i in 0..3u8 {
            effects.palette[17 + usize::from(i)] = rom_byte(prg, 13, 0xBE4B + u16::from(i));
        }
        effects.boss_hp = rom_word(prg, 13, 0xBE4E + 2 * u16::from(effects.stage_args[1]));
        effects.boss_pool = [0x40, 0x1F];
        let player = &mut effects.player;
        (player.entry_step, player.entry_timer, player.tally) = (0, 0, 0);
        player.tally_digit = 0;
        player.entry_chr = game.oam.chr[4];
        player.motion = Motion::BossEntry;
        player.stage_script = stage_script::FROZEN | stage_script::HIDDEN;
        queue_palette(game);
        request_song(game, prg);
        game.effects.transition = 0;
    }
}

/// Queues the palette with every colour below `cap` raised to its brightness and
/// each palette's first colour set to it (`$BEF6`, the fade to white).
fn queue_whitened(game: &mut Game, cap: u8) {
    let mut lit = game.effects.palette;
    for (i, colour) in lit.iter_mut().enumerate() {
        if i.is_multiple_of(4) {
            *colour = cap;
        } else if *colour < cap {
            *colour = (*colour & 0x0F) | cap;
        }
    }
    game.effects.vram.push_palette(&lit);
}

/// The stage's song request (`$D973`).
fn request_song(game: &mut Game, prg: &Prg) {
    if let Music::Song(song) = at(&prg.stages, game.world.area).music {
        game.effects.song = song;
    }
}

/// The banner's direct PPU writes (`$92E9-$938B`): the first nametable cleared, the
/// "STAGE" text from `$93D7` and the stage's two digits (glyphs `$9402`).
fn banner(prg: &Prg, area: u8) -> Vec<VramWrite> {
    let mut writes = text(prg, 0x93D7);
    let [tens, ones] = at(&prg.stages, area).banner;
    for (digit, column) in [(tens, 0x21AF), (ones, 0x21B1)] {
        let glyph = rom_byte(prg, 2, 0x9402 + u16::from(digit));
        for (row, tile) in [(0, glyph), (0x20, glyph | 1)] {
            writes.push(VramWrite {
                addr: column + row,
                flags: 0,
                bytes: vec![tile],
            });
        }
    }
    writes
}

/// The first nametable cleared (`$90FB`) and a bank-2 text table's runs written
/// straight to the PPU: each a big-endian address, a count and the bytes, up to an
/// address with bit 7 set.
fn text(prg: &[u8], at: u16) -> Vec<VramWrite> {
    let list = bank_slice(prg, 2, at & 0x1FFF).expect("a bank-2 text table");
    let runs = WriteList::new(list).map(|run| VramWrite {
        addr: run.addr,
        flags: 0,
        bytes: run.data.to_vec(),
    });
    std::iter::once(VramWrite {
        addr: 0x2000,
        flags: 0,
        bytes: vec![0; 1024],
    })
    .chain(runs)
    .collect()
}

/// The continue screen's text (`$91B7`) and its big digits from `$9204`, NMIs on.
fn draw_digits(game: &mut Game, prg: &[u8]) {
    game.scroll.scroll_x = 0;
    game.scroll.scroll_y = 0;
    game.effects.vram.direct.extend(text(prg, 0x91B7));
    let glyphs = &mut game.effects.level.map.as_bytes_mut()[..640];
    for (glyph, cells) in glyphs.chunks_mut(64).enumerate() {
        let rows = 0x9204 + 8 * u16::try_from(glyph).expect("10 glyphs");
        for (row, line) in cells.chunks_mut(8).enumerate() {
            let bits = rom_byte(prg, 2, rows + u16::try_from(row).expect("< 8"));
            for (column, cell) in line.iter_mut().enumerate() {
                *cell = if bits << column & 0x80 != 0 { 0x11 } else { 0 };
            }
        }
    }
    game.scroll.ppu_ctrl |= ctrl::NMI;
}

/// What a frame of the [`Continue`] screen ended in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Still on screen.
    Running,
    /// Start was pressed: the stage restarts from its first area.
    Continued,
    /// GAME OVER ran its course: the game resets (`JMP ($FFFC)`).
    Reset,
}

/// The continue screen after the last life (bank 2 `$8F05`), one frame per
/// [`Continue::step`]: a countdown from 9 in big digits that Start ends with a
/// continue; when it runs out, GAME OVER (`$9081`) and a reset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Continue {
    step: Countdown,
    /// Where the screen is in [`SETUP`], [`GAME_OVER`] or [`LEAVE`].
    runner: Runner,
    /// 0 holding the digit, 1 splitting it away, 2 closing the next one in (`$10`).
    phase: u8,
    /// The digit shown (`$11`).
    digit: u8,
    /// Frames the digit holds (`$12`), and GAME OVER's hold (`$10`).
    timer: u8,
    /// How fast the digit splits (`$13:$14`).
    speed: u16,
    /// How far it has split, its high byte in tile rows times 8 (`$15:$16`).
    offset: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Countdown {
    /// The screen set up, the rows of [`SETUP`].
    Setup,
    Count,
    Check,
    /// The wait and GAME OVER's screen (`$9081`), the rows of [`GAME_OVER`].
    GameOver,
    /// GAME OVER's text scrolled in from below, held for `$C0` even frames or until a
    /// button is pressed.
    Scroll,
    ScrollCheck,
    /// Start pressed, the rows of [`LEAVE`].
    Leave,
}

/// What the continue screen does before its countdown, before GAME OVER and after Start,
/// in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContinueRow {
    /// One screen, the split, rendering and NMIs off, the music stopped and the screen dark.
    Hide,
    /// The text (`$91B7`) and the big digits from `$9204` drawn, NMIs on.
    Digits,
    /// Rendering on.
    Show,
    /// The countdown from 9, its sprite banks and its first digit.
    FirstDigit,
    /// The screen's palette (`$1B`).
    Palette,
    /// Frames the screen stays dark.
    Wait(u16),
    /// The music stopped, NMIs and rendering off.
    Stop,
    /// GAME OVER's text (`$925A`) drawn, NMIs on.
    OverText,
    /// The GAME OVER song, the palette, the text below the screen and its hold.
    OverPalette,
    /// The screen dark.
    Blackout,
    /// The attack cleared and the Bolt back.
    ClearWeapon,
}

const SETUP: &[ContinueRow] = &[
    ContinueRow::Hide,
    ContinueRow::Digits,
    ContinueRow::Show,
    ContinueRow::FirstDigit,
    ContinueRow::Palette,
];
const GAME_OVER: &[ContinueRow] = &[
    ContinueRow::Wait(49),
    ContinueRow::Stop,
    ContinueRow::OverText,
    ContinueRow::Show,
    ContinueRow::OverPalette,
];
const LEAVE: &[ContinueRow] = &[ContinueRow::Blackout, ContinueRow::ClearWeapon];

impl Default for Continue {
    fn default() -> Self {
        Continue {
            step: Countdown::Setup,
            runner: Runner::default(),
            phase: 0,
            digit: 0,
            timer: 0,
            speed: 0,
            offset: 0,
        }
    }
}

impl Continue {
    /// Runs one frame of the screen up to its NMI wait; a continue or the reset end
    /// it without a frame of their own.
    pub fn step(&mut self, game: &mut Game, prg: &[u8]) -> Outcome {
        loop {
            match self.step {
                Countdown::Setup => {
                    if self.table_frame(SETUP, game, prg) {
                        return Outcome::Running;
                    }
                    self.step = Countdown::Count;
                }
                Countdown::Count => {
                    if self.count(game, prg) {
                        self.begin(Countdown::GameOver);
                    } else {
                        self.step = Countdown::Check;
                    }
                    return Outcome::Running;
                }
                Countdown::Check => {
                    if game.world.pad.contains(Buttons::START) {
                        self.begin(Countdown::Leave);
                    } else {
                        self.step = Countdown::Count;
                    }
                }
                Countdown::Leave => {
                    if self.table_frame(LEAVE, game, prg) {
                        return Outcome::Running;
                    }
                    return Outcome::Continued;
                }
                Countdown::GameOver => {
                    if self.table_frame(GAME_OVER, game, prg) {
                        return Outcome::Running;
                    }
                    self.step = Countdown::Scroll;
                }
                Countdown::Scroll => {
                    let scroll = &mut game.scroll.scroll_y;
                    if *scroll != 0 {
                        *scroll = scroll.wrapping_add(1);
                        if *scroll == 0xF0 {
                            *scroll = 0;
                        }
                    }
                    self.step = Countdown::ScrollCheck;
                    return Outcome::Running;
                }
                Countdown::ScrollCheck => {
                    if game.world.pressed.intersects(Buttons::FACE) {
                        return Outcome::Reset;
                    }
                    if !game.world.frame_odd {
                        self.timer = self.timer.wrapping_sub(1);
                        if self.timer == 0 {
                            return Outcome::Reset;
                        }
                    }
                    self.step = Countdown::Scroll;
                }
            }
        }
    }

    /// Starts `step` at the top of its table.
    fn begin(&mut self, step: Countdown) {
        self.step = step;
        self.runner = Runner::default();
    }

    /// Runs `table`'s rows up to the frame's end (true) or until it runs out.
    fn table_frame(&mut self, table: &[ContinueRow], game: &mut Game, prg: &[u8]) -> bool {
        while let Some(&row) = self.runner.row(table) {
            let frame = self.runner.frame;
            let flow = match row {
                ContinueRow::Wait(count) => wait(frame, count),
                ContinueRow::Hide => {
                    if frame == 0 {
                        game.scroll.mirroring = Mirroring::SingleLow;
                        game.scroll.split.armed = false;
                        game.scroll.mask &= !mask::RENDERING;
                        game.scroll.ppu_ctrl &= !(ctrl::NMI | ctrl::NAMETABLE);
                        game.effects.music = sound::music::STOP;
                        blackout(game, prg);
                    }
                    wait(frame, 1)
                }
                ContinueRow::Digits => {
                    if frame == 0 {
                        draw_digits(game, prg);
                    }
                    wait(frame, 1)
                }
                ContinueRow::Show => {
                    if frame == 0 {
                        game.scroll.mask |= mask::RENDERING;
                    }
                    wait(frame, 1)
                }
                ContinueRow::FirstDigit => {
                    if frame == 0 {
                        (self.phase, self.digit, self.timer) = (0, 9, 0x30);
                        (self.speed, self.offset) = (0, 0);
                        for (bank, shadow) in (0xCC..).zip(&mut game.oam.chr[..4]) {
                            *shadow = bank;
                        }
                        self.draw_digit(game);
                    }
                    wait(frame, 1)
                }
                ContinueRow::Palette => {
                    if frame == 0 {
                        room::build_palette(prg, 0x1B, 0, &mut game.effects.palette);
                        queue_palette(game);
                    }
                    wait(frame, 1)
                }
                ContinueRow::Stop => {
                    if frame == 0 {
                        game.effects.music = sound::music::STOP;
                        game.scroll.ppu_ctrl &= !ctrl::NMI;
                        game.scroll.mask &= !mask::RENDERING;
                    }
                    wait(frame, 1)
                }
                ContinueRow::OverText => {
                    if frame == 0 {
                        game.effects.vram.direct.extend(text(prg, 0x925A));
                        game.scroll.ppu_ctrl |= ctrl::NMI;
                    }
                    wait(frame, 1)
                }
                ContinueRow::OverPalette => {
                    if frame == 0 {
                        game.effects.song = sound::song::GAME_OVER;
                        room::build_palette(prg, 0x1B, 0, &mut game.effects.palette);
                        queue_palette(game);
                        game.scroll.scroll_y = 0xB0;
                        self.timer = 0xC0;
                    }
                    wait(frame, 1)
                }
                ContinueRow::Blackout => {
                    if frame == 0 {
                        blackout(game, prg);
                    }
                    wait(frame, 1)
                }
                ContinueRow::ClearWeapon => {
                    if frame == 0 {
                        game.effects.attack = 0;
                        game.effects.weapon = Weapon::Bolt;
                    }
                    wait(frame, 1)
                }
            };
            if self.runner.advance(flow) {
                return true;
            }
        }
        false
    }

    /// One frame of the countdown (`$8FC9`, through `$9254[$10]`); true when the last
    /// digit has gone and the screen blacked out (`$9031`).
    fn count(&mut self, game: &mut Game, prg: &[u8]) -> bool {
        match self.phase {
            0 => {
                self.timer = self.timer.wrapping_sub(1);
                if self.timer & 0x80 != 0 {
                    game.effects.sound[0] = if self.digit == 0 {
                        sound::COUNTDOWN_END
                    } else {
                        sound::COUNTDOWN_TICK
                    };
                    self.phase = 1;
                    (self.speed, self.offset) = (0, 0);
                }
                return false;
            }
            1 => {
                self.speed = self.speed.wrapping_add(0x20);
                if self.speed >> 8 >= 6 {
                    self.speed = 0x0400;
                }
                self.offset = self.offset.wrapping_add(self.speed);
                if self.offset >> 8 >= 0x20 {
                    self.offset = 0x2000;
                    self.phase = 2;
                    self.digit = self.digit.wrapping_sub(1);
                    if self.digit & 0x80 != 0 {
                        blackout(game, prg);
                        return true;
                    }
                }
            }
            _ => {
                let [rows, low] = self.offset.to_be_bytes();
                self.offset = if let Some(rows) = rows.checked_sub(4) {
                    u16::from_be_bytes([rows, low])
                } else {
                    (self.timer, self.phase, self.speed) = (0x30, 0, 0);
                    0
                };
            }
        }
        self.draw_digit(game);
        false
    }

    /// Queues the digit split by the offset (`$9135`): its top half moved down and
    /// its bottom half up, through the buffer `$7000-$703F`, as eight rows at `$218C`.
    fn draw_digit(&self, game: &mut Game) {
        let level = &mut game.effects.level;
        let glyph = usize::from(self.digit) * 64;
        let split = usize::from(self.offset.to_be_bytes()[0] & 0xF8);
        let mut buffer = [0u8; 64];
        if split < 0x20 {
            let glyphs = level.map.as_bytes();
            buffer[split..0x20].copy_from_slice(&glyphs[glyph..glyph + 0x20 - split]);
            buffer[0x20..0x40 - split].copy_from_slice(&glyphs[glyph + 0x20 + split..glyph + 0x40]);
        }
        for (corners, &byte) in level.corners.iter_mut().zip(&buffer) {
            corners[0] = byte;
        }
        if split >= 0x20 {
            return;
        }
        for (row, bytes) in (0u16..).zip(buffer.chunks(8)) {
            game.effects.vram.push(0x218C + row * 0x20, 0, bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::oracle::Scene;
    use super::NextArea;

    fn load_frames(pressed: u8) -> usize {
        let Some(scene) = Scene::tas(350) else {
            return if pressed == 0 { 165 } else { 102 };
        };
        let mut game = super::room::start(&scene.prg(), &scene.machine.bus.cart.chr, 0);
        let mut load = NextArea::default();
        let mut frames = 0;
        while !load.done() {
            game.world.pressed = super::super::button::Buttons::from_bits(pressed);
            load.step(&mut game, &scene.prg(), &scene.machine.bus.cart.chr);
            frames += 1;
        }
        assert_eq!(game.world.area, 2);
        frames
    }

    #[test]
    fn the_descriptors_answer_what_the_tables_did() {
        let Some(prg) = crate::prg::for_test() else {
            return;
        };
        let byte = |bank, addr| super::rom_byte(prg, bank, addr);
        let stages = &prg.stages;
        assert_eq!(stages.len(), 18);
        for index in 0..18u8 {
            let (at, area) = (u16::from(index), index * 2);
            let stage = super::at(stages, area);
            assert_eq!(stage.area, area);
            let (digits, song, picture) = (
                byte(2, 0x93F0 + at),
                byte(14, 0xD980 + at),
                byte(2, 0x93C5 + at),
            );
            assert!(
                stage.banner == [digits >> 4, digits & 0x0F],
                "stage {index}'s banner"
            );
            assert!(
                stage.music
                    == if song & 0x80 == 0 {
                        super::Music::Song(song)
                    } else {
                        super::Music::Keep
                    },
                "stage {index}'s song"
            );
            assert!(
                stage.picture == (picture & 0x80 == 0).then_some(picture),
                "stage {index}'s picture"
            );
            assert!(
                stage.banner == [byte(8, 0x9DD2 + at), byte(8, 0x9DE4 + at)],
                "stage {index}'s pause digits"
            );
            let letters = super::rom_word(prg, 8, 0x9DF6 + 2 * at);
            let shown = super::at(stages, super::checkpoint(stages, area)).password;
            assert!(
                shown == Some([0, 1, 2, 3].map(|i| byte(8, letters + i))),
                "stage {index}'s pause letters"
            );
            assert!(
                super::checkpoint(stages, area) == byte(14, 0xD57D + at),
                "stage {index}'s continue"
            );
            assert_eq!(stage.on_clear == super::OnClear::Ending, index == 17);
        }
        for level in 0..13u8 {
            let area = super::level_area(stages, usize::from(level));
            assert!(
                area == Some(byte(14, 0xD94E + u16::from(level))),
                "level {level}"
            );
        }
        assert_eq!(super::level_area(stages, 13), None);
        let table = crate::levelmap::LEVEL_AREA_TABLE;
        assert!(
            (0..13).all(|level| super::level_area(stages, level) == Some(table[level])),
            "LEVEL_AREA_TABLE follows the checkpoints"
        );
    }

    #[test]
    fn the_banner_holds_64_frames_unless_start_a_or_b_is_pressed() {
        if rotj_rom::for_test().is_none() {
            return;
        }
        assert_eq!(load_frames(0), 165);
        assert_eq!(load_frames(0x10), 102);
        assert_eq!(load_frames(0x01), 165);
    }
}
