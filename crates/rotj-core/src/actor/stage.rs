//! The stage end (`$BA = 2`, bank 2 `$940C`), the next area's load (`$BA = 3`, bank 14
//! `$D513`) and the palette fades they share (`$C30A`, `$C35A`).

use super::Game;
use super::cutscene::Cutscene;
use super::engine::{draw, rom_byte, rom_word};
use super::room;
use super::world::{Motion, VramWrite};

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
pub fn stage_end(game: &mut Game, prg: &[u8]) -> bool {
    let x = game.effects.player.x;
    let end = &mut game.effects.stage_end;
    let mut pads = None;
    match end.phase {
        0 => {
            if x >= 0x2000 || (x >= 0x1F60 && game.effects.player.facing & 0x40 == 0) {
                game.effects.music = 0xFC;
                end.phase = 1;
                for weapon in &mut game.effects.projectiles {
                    weapon.kind = 0;
                }
                game.effects.player.flags[0] = 0;
                game.effects.attack &= 0x80;
            }
        }
        1 => {
            if x >= 0x2740 {
                game.effects.palette[25..28].fill(0x0F);
                queue_palette(game);
                let end = &mut game.effects.stage_end;
                (end.timer, end.phase, end.y, end.x) = (0, 2, 0x70, 0xFF);
                game.effects.song = 0x0E;
                pads = Some(0);
            } else {
                pads = Some(1);
            }
        }
        2 => {
            depart(game, prg, 0x03, 0x0F);
            let end = &mut game.effects.stage_end;
            end.timer = end.timer.wrapping_add(1);
            if end.timer >= 0x80 {
                (end.phase, end.timer) = (3, 0);
            }
            pads = Some(0);
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
            pads = Some(8);
        }
        4 => {
            end.timer = end.timer.wrapping_add(1);
            let timer = end.timer;
            if timer >= 0x90 {
                if timer == 0x90 {
                    game.effects.music = 0xFC;
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
            pads = Some(8);
        }
        other => panic!("stage-end phase {other} is past $940C's table"),
    }
    if let Some(pad) = pads {
        (game.world.pad, game.world.pressed) = (pad, pad);
    }
    false
}

/// Moves the departing sprite up and left on the timer's beats and draws it (`$955A`, `$9574`).
fn depart(game: &mut Game, prg: &[u8], x_beat: u8, y_beat: u8) {
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
    effects.vram.push(0x3F00, 0, &effects.palette);
}

/// Queues the palette with every colour's brightness capped at `cap` (`$C30A`, `X = $FF`).
fn queue_faded(game: &mut Game, cap: u8) {
    let faded: Vec<u8> = (game.effects.palette.iter().enumerate())
        .map(|(i, &colour)| {
            if i.is_multiple_of(4) || colour < cap {
                colour
            } else {
                (colour & 0x0F) | cap
            }
        })
        .collect();
    game.effects.vram.push(0x3F00, 0, &faded);
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
            effects.sound[0] = 0x31;
            for weapon in &mut effects.projectiles {
                weapon.kind = 0;
            }
            effects.attack &= 0x80;
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
        for first in effects.palette.iter_mut().step_by(4) {
            *first = colour;
        }
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
        player.stage_script = 0x80;
        effects.transition = 0;
        effects.stage_args[1] = player.motion.byte();
        player.motion = Motion::PoweringUp;
        game.world.frame_odd = true;
        effects.weapon = 6;
        let (x, y) = (player.x, player.y);
        for (index, piece) in (0..4u16).zip(effects.projectiles.iter_mut()) {
            let byte = |table: u16| rom_byte(prg, 14, table + index);
            let word = |hi: u16, lo: u16| u16::from_be_bytes([byte(hi), byte(lo)]);
            let [x_hi, x_lo] = x.wrapping_add(word(0xD6B3, 0xD6AF)).to_be_bytes();
            let [y_hi, y_lo] = y.wrapping_add(word(0xD6BB, 0xD6B7)).to_be_bytes();
            *piece = super::world::Projectile {
                kind: 6,
                flags: 0xE0,
                damage: 0,
                vx: word(0xD6C3, 0xD6BF).cast_signed(),
                vy: word(0xD6CB, 0xD6C7).cast_signed(),
                param: u8::try_from(index).expect("< 4"),
                part: 0,
                part_timer: byte(0xD6CF),
                level_x_hi: x_hi,
                level_x_lo: x_lo,
                level_y_hi: y_hi,
                level_y_lo: y_lo,
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
    /// Frames the banner has left (`$10`).
    hold: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Start,
    FadeOut,
    Banner,
    Cutscene,
    BannerDraw,
    BannerPalette,
    BannerFadeIn,
    BannerHold,
    BannerHolding,
    BannerFadeOut,
    Continue,
    Reset,
    Clear,
    BossIntro,
    Load,
    Show,
    Enter,
    FadeIn,
    Done,
}

impl Default for NextArea {
    fn default() -> Self {
        NextArea {
            step: Step::Start,
            life_lost: false,
            new_game: false,
            screen: None,
            intro: BossIntro::default(),
            cutscene: None,
            fade: Fade::new(false, true),
            hold: 0,
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
    pub fn step(&mut self, game: &mut Game, prg: &[u8], chr: &[u8]) {
        loop {
            match self.step {
                Step::Start => {
                    game.oam.finish();
                    self.step = Step::FadeOut;
                }
                Step::FadeOut => {
                    if self.fade.step(game, prg) {
                        self.step = Step::Banner;
                    }
                    return;
                }
                Step::Banner => {
                    if self.banner(game, prg) {
                        return;
                    }
                }
                Step::Cutscene => return self.cutscene_frame(game, prg, chr),
                Step::BannerDraw => {
                    show_banner(game, prg);
                    self.step = Step::BannerPalette;
                    return;
                }
                Step::BannerPalette => {
                    room::build_palette(prg, 0x1B, 0, &mut game.effects.palette);
                    self.fade = Fade::new(true, false);
                    self.step = Step::BannerFadeIn;
                }
                Step::BannerFadeIn => {
                    if self.fade.step(game, prg) {
                        self.step = Step::BannerHold;
                    }
                    return;
                }
                Step::BannerHold => {
                    self.hold = 0x40;
                    self.step = Step::BannerHolding;
                    return;
                }
                Step::BannerHolding => {
                    self.hold = self.hold.wrapping_sub(1);
                    if self.hold != 0 && game.world.pressed & 0xD0 == 0 {
                        return;
                    }
                    self.fade = Fade::new(false, false);
                    self.step = Step::BannerFadeOut;
                }
                Step::BannerFadeOut => {
                    if self.fade.step(game, prg) {
                        self.step = Step::Clear;
                    }
                    return;
                }
                Step::Continue => {
                    if self.continue_screen(game, prg) {
                        return;
                    }
                }
                Step::Clear if game.effects.hud & 0x80 != 0 => {
                    self.step = Step::BossIntro;
                }
                Step::Clear => {
                    clear_for_area(game);
                    self.step = Step::Load;
                    return;
                }
                Step::BossIntro => {
                    if self.intro.step(game, prg, chr) {
                        self.step = Step::Done;
                    }
                    return;
                }
                Step::Load => {
                    start_area(game, prg, chr, self.leftover());
                    self.step = Step::Show;
                    return;
                }
                Step::Show => {
                    game.scroll.mask |= 0x18;
                    self.step = Step::Enter;
                    return;
                }
                Step::Enter => {
                    request_song(game, prg);
                    room::parameters(game, prg);
                    self.fade = Fade::new(true, true);
                    self.step = Step::FadeIn;
                }
                Step::FadeIn => {
                    if self.fade.step(game, prg) {
                        game.effects.busy = 0;
                        game.effects.transition = 0;
                        self.step = Step::Done;
                    }
                    return;
                }
                Step::Reset | Step::Done => return,
            }
        }
    }

    /// The area's indices, or a life taken (`$D527`), then the banner's dark screen;
    /// true when the frame ended, false when the last life went and the continue
    /// screen takes over.
    fn banner(&mut self, game: &mut Game, prg: &[u8]) -> bool {
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
        let scene = rom_byte(prg, 2, 0x93C5 + u16::from(game.world.area >> 1));
        if scene & 0x80 == 0 && game.effects.cutscene_id != 0 {
            game.effects.cutscene_id = 0;
            self.cutscene = Some(Cutscene::new(scene));
            self.step = Step::Cutscene;
            return false;
        }
        hide_for_banner(game, prg);
        self.step = Step::BannerDraw;
        true
    }

    /// A frame of the banner's cutscene, and the dark screen after its last.
    fn cutscene_frame(&mut self, game: &mut Game, prg: &[u8], chr: &[u8]) {
        let cutscene = self.cutscene.as_mut().expect("the cutscene");
        if cutscene.step(game, prg, chr) {
            hide_for_banner(game, prg);
            self.step = Step::BannerDraw;
        }
    }

    /// A frame of the continue screen; false when Start continued and the stage's first
    /// area loads in this frame (`$D53B`: two lives, `$0404` clear).
    fn continue_screen(&mut self, game: &mut Game, prg: &[u8]) -> bool {
        let screen = self.screen.as_mut().expect("the continue screen");
        match screen.step(game, prg) {
            Outcome::Running => true,
            Outcome::Continued => {
                let area = u16::from(game.world.area >> 1);
                game.world.area = rom_byte(prg, 14, 0xD57D + area);
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
    game.scroll.mirroring = 2;
    game.scroll.room[0] = 0;
    game.scroll.mask = 0;
    game.scroll.ppu_ctrl &= 0x7C;
    game.effects.music = 0xFF;
    blackout(game, prg);
}

/// The banner drawn with rendering off, its font banks and the NMI's scroll
/// (`$92E1-$93A4`).
fn show_banner(game: &mut Game, prg: &[u8]) {
    game.scroll.scroll_y = 0;
    game.scroll.scroll_x = 4;
    let banner = banner(prg, game.world.area);
    game.effects.vram.direct.extend(banner);
    for (bank, shadow) in (0xCC..).zip(&mut game.oam.chr[..4]) {
        *shadow = bank;
    }
    game.scroll.ppu_ctrl |= 0x80;
    game.scroll.mask = 0x18;
}

/// The area start (bank 13 `$B200`) up to its scroll reset (`$BE6E`), over what
/// `leftover` says WRAM holds.
fn start_area(game: &mut Game, prg: &[u8], chr: &[u8], leftover: room::Leftover) {
    room::enter_area(game, prg, chr, leftover);
    game.scroll.scroll_x = 0;
    game.scroll.scroll_y = 0;
    game.scroll.ppu_ctrl = (game.scroll.ppu_ctrl & 0xFC) | 0x80;
}

/// The area start's first frame (bank 13 `$BD4D-$BD51`, `$BE58`): the music
/// stopped, every sprite parked, the IRQ split off.
fn clear_for_area(game: &mut Game) {
    game.effects.music = 0xFF;
    game.oam.clear();
    game.scroll.room[0] = 0;
    game.scroll.ppu_ctrl &= 0x7F;
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
    mirroring: u8,
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
    pub fn step(&mut self, game: &mut Game, prg: &[u8], chr: &[u8]) -> bool {
        match self.step {
            Intro::Clear => {
                game.effects.music = 0xFF;
                for slot in &mut game.slots.main {
                    slot.kind = 0;
                }
                for slot in &mut game.slots.second {
                    slot.kind = 0;
                }
                game.scroll.mirroring = 0;
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
                game.scroll.ppu_ctrl = (game.scroll.ppu_ctrl & 0xFC) | 0x80;
                self.step = Intro::Show;
            }
            Intro::Show => {
                game.scroll.mask |= 0x18;
                self.step = Intro::Start;
            }
            Intro::Start => {
                game.effects.song = 0x0A;
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
    fn slide(&mut self, game: &mut Game, prg: &[u8]) -> bool {
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
        game.scroll.ppu_ctrl |= 1;
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
    fn enter(&self, game: &mut Game, prg: &[u8]) {
        game.scroll.ppu_ctrl &= 0xFC;
        game.scroll.mirroring = self.mirroring;
        room::parameters(game, prg);
        game.oam.clear();
        let effects = &mut game.effects;
        for first in effects.palette.iter_mut().step_by(4) {
            *first = 0x0F;
        }
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
        player.stage_script = 0xC0;
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
    game.effects.vram.push(0x3F00, 0, &lit);
}

/// The area's song request (`$D973`: `$41 = $D980[$0400 / 2]` unless bit 7 is set).
fn request_song(game: &mut Game, prg: &[u8]) {
    let song = rom_byte(prg, 14, 0xD980 + u16::from(game.world.area >> 1));
    if song & 0x80 == 0 {
        game.effects.song = song;
    }
}

/// The banner's direct PPU writes (`$92E9-$938B`): the first nametable cleared, the
/// "STAGE" text from `$93D7` and the stage's two digits (`$93F0`, glyphs `$9402`).
fn banner(prg: &[u8], area: u8) -> Vec<VramWrite> {
    let mut writes = text(prg, 0x93D7);
    let digits = rom_byte(prg, 2, 0x93F0 + u16::from(area >> 1));
    for (digit, column) in [(digits >> 4, 0x21AF), (digits & 0x0F, 0x21B1)] {
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
fn text(prg: &[u8], mut at: u16) -> Vec<VramWrite> {
    let mut writes = vec![VramWrite {
        addr: 0x2000,
        flags: 0,
        bytes: vec![0; 1024],
    }];
    while rom_byte(prg, 2, at) & 0x80 == 0 {
        let addr = rom_word(prg, 2, at).swap_bytes();
        let count = rom_byte(prg, 2, at + 2);
        let bytes = (0..u16::from(count))
            .map(|i| rom_byte(prg, 2, at + 3 + i))
            .collect();
        writes.push(VramWrite {
            addr,
            flags: 0,
            bytes,
        });
        at += 3 + u16::from(count);
    }
    writes
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
    /// 0 holding the digit, 1 splitting it away, 2 closing the next one in (`$10`).
    phase: u8,
    /// The digit shown (`$11`).
    digit: u8,
    /// Frames the digit holds (`$12`), and the frames the screens after it wait (`$10`).
    timer: u8,
    /// How fast the digit splits (`$13:$14`).
    speed: u16,
    /// How far it has split, its high byte in tile rows times 8 (`$15:$16`).
    offset: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Countdown {
    Setup,
    Draw,
    Show,
    First,
    Palette,
    Count,
    Check,
    Leave,
    Clear,
    Continued,
    Wait,
    Over,
    OverDraw,
    OverShow,
    OverPalette,
    OverScroll,
    OverCheck,
    Reset,
}

impl Default for Continue {
    fn default() -> Self {
        Continue {
            step: Countdown::Setup,
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
                    game.scroll.mirroring = 2;
                    game.scroll.room[0] = 0;
                    game.scroll.mask &= 0xE7;
                    game.scroll.ppu_ctrl &= 0x7C;
                    game.effects.music = 0xFF;
                    blackout(game, prg);
                    self.step = Countdown::Draw;
                }
                Countdown::Draw => {
                    game.scroll.scroll_x = 0;
                    game.scroll.scroll_y = 0;
                    game.effects.vram.direct.extend(text(prg, 0x91B7));
                    for (glyph, cells) in game.effects.level.map[..640].chunks_mut(64).enumerate() {
                        let rows = 0x9204 + 8 * u16::try_from(glyph).expect("10 glyphs");
                        for (row, line) in cells.chunks_mut(8).enumerate() {
                            let bits = rom_byte(prg, 2, rows + u16::try_from(row).expect("< 8"));
                            for (column, cell) in line.iter_mut().enumerate() {
                                *cell = if bits << column & 0x80 != 0 { 0x11 } else { 0 };
                            }
                        }
                    }
                    game.scroll.ppu_ctrl |= 0x80;
                    self.step = Countdown::Show;
                }
                Countdown::Show => {
                    game.scroll.mask |= 0x18;
                    self.step = Countdown::First;
                }
                Countdown::First => {
                    (self.phase, self.digit, self.timer) = (0, 9, 0x30);
                    (self.speed, self.offset) = (0, 0);
                    for (bank, shadow) in (0xCC..).zip(&mut game.oam.chr[..4]) {
                        *shadow = bank;
                    }
                    self.draw_digit(game);
                    self.step = Countdown::Palette;
                }
                Countdown::Palette => {
                    room::build_palette(prg, 0x1B, 0, &mut game.effects.palette);
                    queue_palette(game);
                    self.step = Countdown::Count;
                }
                Countdown::Count => {
                    self.step = Countdown::Check;
                    if self.count(game, prg) {
                        self.step = Countdown::Wait;
                    }
                }
                Countdown::Check => {
                    self.step = if game.world.pad & 0x10 != 0 {
                        Countdown::Leave
                    } else {
                        Countdown::Count
                    };
                    continue;
                }
                Countdown::Leave => {
                    blackout(game, prg);
                    self.step = Countdown::Clear;
                }
                Countdown::Clear => {
                    game.effects.attack = 0;
                    game.effects.weapon = 0;
                    self.step = Countdown::Continued;
                }
                Countdown::Continued => return Outcome::Continued,
                Countdown::Wait => {
                    if self.timer == 0 {
                        self.step = Countdown::Over;
                        continue;
                    }
                    self.timer -= 1;
                }
                Countdown::Over
                | Countdown::OverDraw
                | Countdown::OverShow
                | Countdown::OverPalette
                | Countdown::OverScroll
                | Countdown::OverCheck => return self.game_over(game, prg),
                Countdown::Reset => return Outcome::Reset,
            }
            return Outcome::Running;
        }
    }

    /// A frame of GAME OVER (`$9081`): the text scrolled in from below, held for `$C0`
    /// even frames or until a button is pressed.
    fn game_over(&mut self, game: &mut Game, prg: &[u8]) -> Outcome {
        loop {
            match self.step {
                Countdown::Over => {
                    game.effects.music = 0xFF;
                    game.scroll.ppu_ctrl &= 0x7F;
                    game.scroll.mask &= 0xE7;
                    self.step = Countdown::OverDraw;
                }
                Countdown::OverDraw => {
                    game.effects.vram.direct.extend(text(prg, 0x925A));
                    game.scroll.ppu_ctrl |= 0x80;
                    self.step = Countdown::OverShow;
                }
                Countdown::OverShow => {
                    game.scroll.mask |= 0x18;
                    self.step = Countdown::OverPalette;
                }
                Countdown::OverPalette => {
                    game.effects.song = 0x08;
                    room::build_palette(prg, 0x1B, 0, &mut game.effects.palette);
                    queue_palette(game);
                    game.scroll.scroll_y = 0xB0;
                    self.timer = 0xC0;
                    self.step = Countdown::OverScroll;
                }
                Countdown::OverScroll => {
                    let scroll = &mut game.scroll.scroll_y;
                    if *scroll != 0 {
                        *scroll = scroll.wrapping_add(1);
                        if *scroll == 0xF0 {
                            *scroll = 0;
                        }
                    }
                    self.step = Countdown::OverCheck;
                }
                Countdown::OverCheck => {
                    self.step = Countdown::OverScroll;
                    if game.world.pressed & 0xF0 != 0 {
                        self.step = Countdown::Reset;
                    } else if !game.world.frame_odd {
                        self.timer = self.timer.wrapping_sub(1);
                        if self.timer == 0 {
                            self.step = Countdown::Reset;
                        }
                    }
                    continue;
                }
                _ => return Outcome::Reset,
            }
            return Outcome::Running;
        }
    }

    /// One frame of the countdown (`$8FC9`, through `$9254[$10]`); true when the last
    /// digit has gone and the screen blacked out (`$9031`).
    fn count(&mut self, game: &mut Game, prg: &[u8]) -> bool {
        match self.phase {
            0 => {
                self.timer = self.timer.wrapping_sub(1);
                if self.timer & 0x80 != 0 {
                    game.effects.sound[0] = if self.digit == 0 { 0x46 } else { 0x41 };
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
                        self.timer = 49;
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
            buffer[split..0x20].copy_from_slice(&level.map[glyph..glyph + 0x20 - split]);
            buffer[0x20..0x40 - split]
                .copy_from_slice(&level.map[glyph + 0x20 + split..glyph + 0x40]);
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
            game.world.pressed = pressed;
            load.step(&mut game, &scene.prg(), &scene.machine.bus.cart.chr);
            frames += 1;
        }
        assert_eq!(game.world.area, 2);
        frames
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
