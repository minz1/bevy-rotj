//! The app running the port's game (`rotj_core::actor::Game`) from power-on: the
//! boot, intro and title, then the game loop, one `Game::frame` per 60 Hz tick, the
//! NMI's queue drained into the nametables and the palette, its sound requests handed
//! to the music driver, and the picture composed from the game's own PPU state.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use rotj_core::actor::oam::{Mark, OamEntry};
use rotj_core::actor::world::{Ppu, SoundId};
use rotj_core::actor::{Game, Script, raster};
use rotj_core::levels::Levels;
use rotj_core::prg::Prg;
use rotj_core::render::{Frame, Line, MAX_SPRITES_PER_LINE, OAM_SPRITES, Picture};

use crate::input::NesInput;
use crate::mirror::Snapshot;
use crate::pacing::Step;
use crate::screen::{Screen, screen_frame, screen_image};
use crate::settings::Settings;
use crate::{GameState, RomData};

/// A level to start on instead of the title's, from `ROTJ_LEVEL` (0-12), forced where
/// the game reads it (`$D3D8`), as a developer's poke would: the app has no CLI parsing,
/// so the env var follows `ROTJ_RENDER_BACKEND`. 0 is the title's own start and an
/// unparsable or out-of-range value forces nothing, so a recording's `level0` always
/// means "as the title chose".
fn forced_level() -> Option<u8> {
    let value = std::env::var("ROTJ_LEVEL").ok()?;
    match value.parse::<u8>() {
        Ok(0) => None,
        Ok(level) if level < 13 => Some(level),
        _ => {
            warn!("ROTJ_LEVEL {value:?} is not a level 0-12; the title chooses");
            None
        }
    }
}

/// The game, its ROM data and the PPU memory the NMI writes.
#[derive(Resource)]
pub struct Play {
    game: Game,
    prg: Prg,
    pub(crate) chr: Vec<u8>,
    pub(crate) ppu: Ppu,
    /// The NMI armed the raster IRQ (`$D0` bit 7), so this frame is split.
    raster: bool,
    /// The lines the PPU draws this frame with, and the sprites its NMI copied.
    pub(crate) lines: Vec<Line>,
    pub(crate) oam: [u8; 256],
    /// Which drawer placed each of those sprites.
    pub(crate) marks: [Mark; 64],
    /// The game as the mirror shows it, taken with those sprites.
    pub(crate) shown: Snapshot,
    /// The HUD label's sprites the step before showed, which the game draws every other frame.
    pub(crate) label: Vec<OamEntry>,
    /// The level forced at `$D3D8`, kept for the boot after a reset.
    forced: Option<u8>,
}

impl Play {
    /// The game at power-on; `forced` overrides the level the title starts.
    #[must_use]
    pub fn power_on(prg: Prg, chr: Vec<u8>, forced: Option<u8>) -> Play {
        Play::running(Game::power_on(forced), prg, chr, Ppu::new(), forced)
    }

    /// `game` run to its first NMI wait.
    fn running(game: Game, prg: Prg, chr: Vec<u8>, ppu: Ppu, forced: Option<u8>) -> Play {
        let mut play = Play {
            oam: [0; 256],
            marks: [Mark::Scene; 64],
            shown: Snapshot::default(),
            label: Vec::new(),
            game,
            prg,
            chr,
            ppu,
            raster: false,
            lines: Vec::new(),
            forced,
        };
        let registers = raster::Registers::of(&play.game);
        play.copy_page(&registers);
        play.run_frame(registers);
        play
    }

    /// The game's frame up to its NMI wait, its direct PPU writes (made in vblank or
    /// with rendering off) and the lines the frame after `registers`' NMI is drawn with. A
    /// split frame's Modern picture is redone from those lines, which its IRQs drew from the
    /// RAM the loop wrote, and the game at the NMI: the pair the background gate checks.
    fn run_frame(&mut self, registers: raster::Registers) {
        let mut registers = registers;
        let split_at_nmi = self.game.scroll.split.armed.then(|| self.game.clone());
        self.game.frame(&self.prg, &self.chr);
        if self.game.reset() {
            self.game = Game::power_on(self.forced);
            self.raster = false;
            self.game.frame(&self.prg, &self.chr);
            registers = raster::Registers::of(&self.game);
        }
        let direct = std::mem::take(&mut self.game.effects.vram.direct);
        self.ppu.apply(&direct, self.game.scroll.mirroring);
        self.lines = raster::lines(&registers, &self.game);
        if let Some(game) = split_at_nmi {
            self.shown.modern = crate::modern::ModernFrame::of(&game, &self.lines);
        }
    }

    /// The game at the end of `level`'s first loop frame, its first page already in the
    /// nametables.
    #[cfg(test)]
    #[must_use]
    pub fn start(prg: Prg, chr: Vec<u8>, level: usize) -> Play {
        let mut game = rotj_core::actor::room::start(&prg, &chr, level);
        let mut ppu = Ppu::new();
        let page = std::mem::take(&mut game.effects.vram.direct);
        ppu.apply(&page, game.scroll.mirroring);
        ppu.palette = game.effects.palette;
        Play::running(game, prg, chr, ppu, None)
    }

    /// The last frame's raster IRQs, an NMI with joypad 1 held as `pad` and the frame of
    /// the game after it, whose lines it keeps for [`Play::compose`]; the sound requests
    /// the NMI hands the driver (`$40-$43`). A reset (GAME OVER, `JMP ($FFFC)`) runs on
    /// into the boot's first frame.
    pub fn step(&mut self, pad: u8) -> [Option<SoundId>; 4] {
        if self.raster {
            raster::run_irqs(&mut self.game);
        }
        let vblank = self.game.nmi(pad);
        self.raster = self.game.scroll.split.armed;
        self.ppu.apply(&vblank.writes, self.game.scroll.mirroring);
        let registers = raster::Registers::of(&self.game);
        self.label = marked(&self.oam, &self.marks, Mark::Label);
        self.copy_page(&registers);
        self.run_frame(registers);
        vblank.sound
    }

    /// Copies the page the NMI shows, its marks and the mirror's snapshot of the game.
    fn copy_page(&mut self, registers: &raster::Registers) {
        self.oam = self.game.oam.page;
        self.marks = self.game.oam.marks;
        self.shown = Snapshot::of(&self.game, &raster::lines(registers, &self.game));
    }

    /// True until the boot reaches the first loop frame.
    #[must_use]
    pub fn booting(&self) -> bool {
        matches!(&self.game.script, Some(Script::Boot(boot)) if !boot.done())
    }

    /// True while the game's own pause is up (Start in the loop, `$9C37`).
    #[must_use]
    pub fn paused(&self) -> bool {
        matches!(self.game.script, Some(Script::Pause(_)))
    }

    /// The game.
    #[cfg(test)]
    #[must_use]
    pub fn game(&self) -> &Game {
        &self.game
    }

    /// The game, to set a scene up.
    #[cfg(test)]
    pub fn game_mut(&mut self) -> &mut Game {
        &mut self.game
    }

    /// Composes the picture the PPU draws from the game's state: its nametables, palette
    /// and sprites, line by line as the NMI and the raster IRQs set them.
    pub fn compose(&self, frame: &mut Frame, sprites_per_line: usize, park: bool) {
        frame.render(
            &Picture {
                lines: &self.lines,
                vram: self.ppu.nametables.as_flattened(),
                palette: &self.ppu.palette,
                oam: &self.display_oam(park),
                chr: &self.chr,
            },
            sprites_per_line,
        );
    }

    /// The sprites the picture shows: with `park`, the game's UI sprites moved off screen,
    /// for Bevy UI to draw.
    fn display_oam(&self, park: bool) -> [u8; 256] {
        let mut oam = self.oam;
        if park {
            for (entry, mark) in self.marks.iter().enumerate() {
                if mark.is_ui() {
                    oam[entry * 4] = OamEntry::PARKED_Y;
                }
            }
        }
        oam
    }
}

/// The entries of `oam` that `mark` placed.
fn marked(oam: &[u8; 256], marks: &[Mark; 64], mark: Mark) -> Vec<OamEntry> {
    let entries = oam.as_chunks::<4>().0;
    (0..64)
        .filter(|&entry| marks[entry] == mark)
        .map(|entry| OamEntry::from(entries[entry]))
        .collect()
}

/// A session's pads for a replay against the oracle: one byte per [`Play::step`], the
/// pad it was given, to `<dir>/<unix millis>-level<N>.pads` (`ROTJ_RECORD_INPUT=<dir>`).
#[derive(Resource)]
pub struct Recorder(Option<File>);

impl Recorder {
    /// A recording of a session starting at `level`, named by the clock.
    #[must_use]
    pub fn open(dir: &Path, level: usize) -> Option<Recorder> {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis());
        Recorder::open_at(dir, millis, level)
    }

    /// A new file `<millis>-level<N>.pads`, or `<millis>-2-level<N>.pads` if a session
    /// started in the same millisecond took that; never an existing file or link.
    #[must_use]
    pub fn open_at(dir: &Path, millis: u128, level: usize) -> Option<Recorder> {
        let create = |name: String| {
            let path = dir.join(name);
            OpenOptions::new().write(true).create_new(true).open(&path)
        };
        match create(format!("{millis}-level{level}.pads"))
            .or_else(|_| create(format!("{millis}-2-level{level}.pads")))
        {
            Ok(file) => Some(Recorder(Some(file))),
            Err(error) => {
                warn!("not recording input to {}: {error}", dir.display());
                None
            }
        }
    }

    /// Appends `pad`; after a failed write, warns once and records nothing more.
    pub fn record(&mut self, pad: u8) {
        if let Some(file) = &mut self.0
            && let Err(error) = file.write_all(&[pad])
        {
            warn!("input recording stopped: {error}");
            self.0 = None;
        }
    }
}

/// The app's game plugin.
pub struct PlayPlugin;

impl Plugin for PlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, power_on)
            .add_systems(
                Update,
                tick.in_set(Step::Game)
                    .run_if(crate::pacing::due)
                    .run_if(menu_closed),
            )
            .add_systems(
                Update,
                (
                    crate::screen::apply_settings,
                    draw,
                    crate::screen::upload_frame,
                )
                    .chain()
                    .in_set(Step::Picture),
            );
    }
}

/// The game runs unless the settings menu is open.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
pub(crate) fn menu_closed(menu: Res<'_, crate::menu::Menu>) -> bool {
    !menu.open
}

/// Powers the game on, records its pads if asked (`ROTJ_RECORD_INPUT`) and shows it.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
fn power_on(mut commands: Commands<'_, '_>, screen: Res<'_, Screen>, rom: Res<'_, RomData>) {
    let forced = forced_level();
    let dir = std::env::var_os("ROTJ_RECORD_INPUT").filter(|dir| !dir.is_empty());
    let level = usize::from(forced.unwrap_or(0));
    if let Some(recorder) = dir.and_then(|dir| Recorder::open(Path::new(&dir), level)) {
        commands.insert_resource(recorder);
    }
    let levels = Levels::original(&rom.0.prg, &rom.0.chr).expect("the ROM's levels import");
    let prg = Prg::new(rom.0.prg.clone(), levels);
    commands.insert_resource(Play::power_on(prg, rom.0.chr.clone(), forced));
    crate::screen::spawn_screen_quad(&mut commands, &screen_image(&screen));
}

/// One frame of the game; the state follows it: the title while it boots (again after
/// GAME OVER's reset), gameplay from its first loop frame.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
pub(crate) fn tick(
    input: Res<'_, NesInput>,
    mut play: ResMut<'_, Play>,
    recorder: Option<ResMut<'_, Recorder>>,
    music: Option<ResMut<'_, crate::audio::Music>>,
    state: Res<'_, State<GameState>>,
    mut next: ResMut<'_, NextState<GameState>>,
) {
    let pad = input.game_byte();
    if let Some(mut recorder) = recorder {
        recorder.record(pad);
    }
    let sound = play.step(pad);
    if let Some(mut music) = music {
        for request in sound.into_iter().flatten() {
            music.request(request);
        }
    }
    let now = if play.booting() {
        GameState::Title
    } else {
        GameState::Playing
    };
    if *state.get() != now {
        next.set(now);
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
fn draw(play: Res<'_, Play>, settings: Res<'_, Settings>, mut screen: ResMut<'_, Screen>) {
    let limit = if settings.sprite_limit {
        MAX_SPRITES_PER_LINE
    } else {
        OAM_SPRITES
    };
    let faithful = settings.ui == crate::settings::UiStyle::Faithful;
    play.compose(screen_frame(&mut screen), limit, faithful);
    let hidden = settings.overscan == crate::settings::Overscan::Hidden;
    let edges = std::array::from_fn(|y| hidden && play.lines.get(y).is_some_and(Line::scrolled));
    screen.edges = edges;
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rotj_core::actor::stage::StageEnd;
    use rotj_core::actor::world::Motion;
    use rotj_core::digest::fnv1a;
    use rotj_core::render::{Mirroring, SCREEN_W, mask};
    use rotj_core::rooms::Chain;

    /// `rom`'s `Prg`, its levels imported once for every test.
    pub(crate) fn prg(rom: &rotj_rom::Rom) -> Prg {
        static LEVELS: std::sync::OnceLock<Levels> = std::sync::OnceLock::new();
        let levels = LEVELS
            .get_or_init(|| Levels::original(&rom.prg, &rom.chr).expect("the test ROM's levels"));
        Prg::new(rom.prg.clone(), levels.clone())
    }

    /// A committed digest of an fceux capture (`crates/app/digests.txt`).
    fn digest(name: &str) -> u64 {
        rotj_core::digest::table(include_str!("../digests.txt"), name)["-"]
    }

    /// Steps `play` with `pad` until `until` holds, at most `frames` times; whether it did.
    fn step_until(play: &mut Play, frames: usize, pad: u8, until: impl Fn(&Play) -> bool) -> bool {
        (0..frames).any(|_| {
            play.step(pad);
            until(play)
        })
    }

    fn on_title(play: &Play) -> bool {
        matches!(&play.game().script, Some(Script::Boot(boot)) if boot.on_title())
    }

    /// The game at power-on, or `None` (skipped) without the ROM.
    fn powered_on(forced: Option<u8>) -> Option<Play> {
        let rom = rotj_rom::for_test()?;
        Some(Play::power_on(prg(rom), rom.chr.clone(), forced))
    }

    /// The emulator's screenshots crop 8 lines top and bottom (256x224).
    const OVERSCAN: usize = 8;

    /// The title the port boots into is the emulator's own frame 300 (the attract
    /// capture's screenshot), pixel for pixel.
    #[test]
    fn the_title_draws_the_emulators_frame() {
        let Some(mut play) = powered_on(None) else {
            return;
        };
        assert!(step_until(&mut play, 400, 0, on_title), "no title");
        for _ in 0..10 {
            play.step(0);
        }
        let ours = picture(&play);
        let visible = &ours[OVERSCAN * SCREEN_W..(OVERSCAN + 224) * SCREEN_W];
        assert_eq!(
            fnv1a(visible),
            digest("title-300"),
            "the title differs from fceux's frame 300 (regenerate tools/emu/attract for the pixels)"
        );
    }

    /// The intro plays from power-on: its pictures change, the raster IRQ splits some.
    #[test]
    fn the_intro_animates() {
        let Some(mut play) = powered_on(None) else {
            return;
        };
        let mut pictures = std::collections::BTreeSet::new();
        let mut split = 0;
        for _ in 0..270 {
            play.step(0);
            split += usize::from(play.raster && play.game().scroll.split.chain == Chain::Intro);
            pictures.insert(picture(&play));
        }
        assert!(pictures.len() > 60, "{} distinct pictures", pictures.len());
        assert!(split > 20, "{split} split frames");
    }

    /// Start on the title plays the chosen line's flash, the banner and stage 1-1 to its
    /// first loop frame, the game's own songs requested on the way: the title's `$01`
    /// (`$61AE`) and the stage's `$04` (`$D973`).
    #[test]
    fn start_on_the_title_reaches_gameplay() {
        let Some(mut play) = powered_on(None) else {
            return;
        };
        let mut songs = Vec::new();
        let mut step = |play: &mut Play, pad: u8| {
            let sound = play.step(pad);
            if let Some(song) = sound[1] {
                songs.push(song.byte());
            }
        };
        while !on_title(&play) {
            step(&mut play, 0);
        }
        step(&mut play, 0x10);
        for _ in 0..400 {
            if !play.booting() {
                break;
            }
            step(&mut play, 0x10);
        }
        assert!(!play.booting(), "the stage never started");
        assert_eq!(play.game().world.area, 0);
        assert_eq!(songs, [0x01, 0x04]);
        assert_plays(&mut play, 60, 0x01);
    }

    /// `ROTJ_LEVEL` forces the title's level where the game reads it (`$D3D8`).
    #[test]
    fn a_forced_level_starts_there_from_the_title() {
        let Some(mut play) = powered_on(Some(5)) else {
            return;
        };
        assert!(step_until(&mut play, 400, 0, on_title), "no title");
        play.step(0x10);
        assert!(step_until(&mut play, 400, 0, |play| !play.booting()));
        assert_eq!(play.game().world.area, 0x0C, "stage 3-2's area");
    }

    fn play() -> Option<Play> {
        start(0)
    }

    /// The game at `level`'s first loop frame, or `None` (skipped) without the ROM.
    fn start(level: usize) -> Option<Play> {
        let rom = rotj_rom::for_test()?;
        Some(Play::start(prg(rom), rom.chr.clone(), level))
    }

    #[test]
    fn paused_follows_the_games_pause() {
        let Some(mut play) = play() else {
            return;
        };
        for _ in 0..30 {
            play.step(0);
        }
        assert!(!play.paused());
        play.step(0x10);
        for _ in 0..5 {
            play.step(0);
        }
        assert!(play.paused(), "Start pauses");
        play.step(0x10);
        for _ in 0..5 {
            play.step(0);
        }
        assert!(!play.paused(), "Start again resumes");
    }

    /// The marks of the entries visible on the OAM page.
    pub(crate) fn visible_marks(play: &Play) -> impl Iterator<Item = Mark> + '_ {
        play.oam
            .as_chunks::<4>()
            .0
            .iter()
            .zip(play.marks)
            .filter(|&(&bytes, _)| OamEntry::from(bytes).is_visible())
            .map(|(_, mark)| mark)
    }

    /// The shown sprites `mark` placed, on screen.
    pub(crate) fn shown(play: &Play, mark: Mark) -> usize {
        visible_marks(play).filter(|&at| at == mark).count()
    }

    /// Every marked sprite shown is drawable as UI exactly as the PPU draws it.
    fn assert_drawable(play: &Play) {
        if let Some(why) = rotj_core::actor::oam::undrawable(&play.oam, &play.marks, &play.lines) {
            panic!("{why}");
        }
    }

    /// One step with `pad`, checking the sprites it shows, which the state before it drew.
    fn checked_step(play: &mut Play, pad: u8) {
        play.step(pad);
        assert_drawable(play);
    }

    #[test]
    fn classic_shows_the_games_oam_and_faithful_parks_the_ui() {
        let Some(mut play) = play() else {
            return;
        };
        for _ in 0..4 {
            play.step(0);
        }
        assert_eq!(play.display_oam(false), play.oam);
        let parked = play.display_oam(true);
        for entry in 0..64 {
            let (ours, game) = (
                &parked[entry * 4..entry * 4 + 4],
                &play.oam[entry * 4..entry * 4 + 4],
            );
            if play.marks[entry].is_ui() {
                assert_eq!((ours[0], &ours[1..]), (OamEntry::PARKED_Y, &game[1..]));
            } else {
                assert_eq!(ours, game);
            }
        }
        assert!(play.marks.contains(&Mark::Hud));
    }

    #[test]
    fn the_label_carries_over_one_step() {
        let Some(mut play) = play() else {
            return;
        };
        let mut carried = 0;
        for _ in 0..8 {
            let before = marked(&play.oam, &play.marks, Mark::Label);
            play.step(0);
            assert_eq!(play.label, before);
            carried += usize::from(!play.label.is_empty() && !play.marks.contains(&Mark::Label));
        }
        assert!(
            carried >= 3,
            "the even frames carry the odd frames' label: {carried}"
        );
    }

    #[test]
    fn the_title_shows_only_its_cursor_as_ui() {
        let Some(mut play) = powered_on(None) else {
            return;
        };
        assert!(step_until(&mut play, 400, 0, on_title), "no title");
        for (i, pad) in [0x01, 0, 0x02, 0, 0x20, 0, 0, 0].into_iter().enumerate() {
            let title = on_title(&play);
            checked_step(&mut play, pad);
            let visible = visible_marks(&play).count();
            if title {
                assert!(shown(&play, Mark::Title) > 0, "step {i}");
                assert_eq!(shown(&play, Mark::Title), visible, "step {i}");
            }
        }
    }

    #[test]
    fn the_password_entry_shows_nine_six_or_none() {
        let Some(mut play) = powered_on(None) else {
            return;
        };
        assert!(step_until(&mut play, 400, 0, on_title), "no title");
        for pad in [0x01, 0, 0x80, 0] {
            checked_step(&mut play, pad);
        }
        let mut counts = std::collections::BTreeSet::new();
        let mut entered = false;
        for step in 0..1500 {
            let count = shown(&play, Mark::Password);
            counts.insert(count);
            let pad = if count == 9 && !entered && step % 2 == 0 {
                entered = step > 300;
                if entered { 0x80 } else { 0x04 }
            } else {
                0
            };
            checked_step(&mut play, pad);
        }
        assert!(counts.is_subset(&[0, 6, 9].into()), "{counts:?}");
        assert!(counts.contains(&9) && counts.contains(&6), "{counts:?}");
    }

    #[test]
    fn the_pause_card_is_all_ui() {
        let Some(mut play) = play() else {
            return;
        };
        for _ in 0..30 {
            checked_step(&mut play, 0);
        }
        assert!(shown(&play, Mark::Hud) > 0);
        checked_step(&mut play, 0x10);
        let mut paused = 0;
        for _ in 0..30 {
            let was = play.paused();
            checked_step(&mut play, 0);
            if was {
                let visible = visible_marks(&play).count();
                assert_eq!(shown(&play, Mark::Pause), visible);
                paused += 1;
            }
        }
        assert!(paused > 20, "{paused} paused steps");
        checked_step(&mut play, 0x10);
    }

    /// Area `$1C`'s IRQ reads the scroll the loop moves, so its runs come from the lines after it.
    #[test]
    fn a_split_frame_is_drawn_from_the_lines_after_its_loop() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let mut play = Play::start(prg(rom), rom.chr.clone(), 11);
        for frame in 0..60 {
            play.step(if frame % 4 < 2 { 0x41 } else { 0x01 });
            let modern = play.shown.modern.as_ref().expect("drawn Modern");
            assert_eq!(
                Some(modern.view.runs),
                rotj_core::actor::modern::runs(&play.lines, 0),
                "frame {frame}"
            );
        }
    }

    /// Arms `$BA`/`$BB` and runs the script that follows to its end.
    pub(crate) fn transition(play: &mut Play, kind: u8, arg: u8) {
        let effects = &mut play.game_mut().effects;
        (effects.transition, effects.transition_arg) = (kind, arg);
        play.step(0);
        for _ in 0..1200 {
            if play.game().script.is_none() {
                break;
            }
            play.step(0);
        }
        assert!(play.game().script.is_none(), "${kind:02X} never ended");
    }

    /// The area's last room into its boss room (`$0404` bit 7, `$BB3D`) and Batman's entry.
    pub(crate) fn enter_the_boss_room(play: &mut Play, area: u8) {
        play.game_mut().effects.hud = 0x80;
        transition(play, 3, 0);
        assert_eq!(play.game().world.area, area);
        for _ in 0..200 {
            if play.game().effects.player.motion != Motion::BossEntry {
                break;
            }
            play.step(0);
        }
        assert_ne!(
            play.game().effects.player.motion,
            Motion::BossEntry,
            "Batman never entered"
        );
    }

    fn picture(play: &Play) -> Vec<u8> {
        let mut frame = Frame::new();
        play.compose(&mut frame, MAX_SPRITES_PER_LINE, false);
        frame.indices().to_vec()
    }

    /// Plays `frames` frames holding `pad`: the picture must be neither blank nor frozen.
    fn assert_plays(play: &mut Play, frames: usize, pad: u8) {
        let before = picture(play);
        for _ in 0..frames {
            play.step(pad);
        }
        assert!(play.game().script.is_none(), "a script started");
        let after = picture(play);
        let backdrop = after[0];
        assert!(
            after.iter().any(|&pixel| pixel != backdrop),
            "the frame is blank"
        );
        assert_ne!(before, after, "the frame is stuck");
    }

    /// Where the room is: `(area, room, $DD)`.
    fn room(play: &Play) -> (u8, u8, u8) {
        let world = &play.game().world;
        (world.area, world.room, world.orientation)
    }

    /// Stage 4 as the TAS plays it: 4-1, then 4-2's rooms (the slide into a vertical
    /// room, `$BB = $61`; the fade out of it, `$E0`; the slide into the last, `$62`) and
    /// the stage 4 boss room.
    #[test]
    fn stage_4_plays_through_its_rooms_to_the_boss() {
        let Some(mut play) = start(6) else {
            return;
        };
        assert_plays(&mut play, 90, 0x01);
        transition(&mut play, 3, 0);
        assert_eq!(room(&play), (0x12, 0, 0x00));
        assert_plays(&mut play, 60, 0x01);
        for (arg, next) in [
            (0x61, (0x12, 2, 0x40)),
            (0xE0, (0x12, 4, 0x00)),
            (0x62, (0x12, 6, 0x40)),
        ] {
            transition(&mut play, 1, arg);
            assert_eq!(room(&play), next);
            assert_plays(&mut play, 60, 0x01);
        }
        enter_the_boss_room(&mut play, 0x14);
        assert_plays(&mut play, 60, 0);
    }

    /// Stage 5, which has no rooms and no boss: 5-1, then the one-screen 5-2.
    #[test]
    fn stage_5_plays_both_areas() {
        let Some(mut play) = start(8) else {
            return;
        };
        assert_plays(&mut play, 90, 0x01);
        transition(&mut play, 3, 0);
        assert_eq!(room(&play), (0x18, 0, 0x00));
        assert_plays(&mut play, 90, 0x01);
    }

    /// Stage 6 as the TAS plays it: 6-1's two slides (`$BB = $61`), 6-2 and the boss room.
    #[test]
    fn stage_6_plays_through_its_rooms_to_the_boss() {
        let Some(mut play) = start(10) else {
            return;
        };
        assert_plays(&mut play, 90, 0x01);
        for next in [(0x1A, 2, 0x00), (0x1A, 4, 0x00)] {
            transition(&mut play, 1, 0x61);
            assert_eq!(room(&play), next);
            assert_plays(&mut play, 60, 0x01);
        }
        transition(&mut play, 3, 0);
        assert_eq!(room(&play), (0x1C, 0, 0x00));
        assert_plays(&mut play, 60, 0x01);
        enter_the_boss_room(&mut play, 0x1E);
        assert_plays(&mut play, 60, 0);
    }

    /// Stage 7 as the TAS plays it: 7-1's fade (`$BB = $C1`) and slide (`$61`), then the
    /// last boss's room.
    #[test]
    fn stage_7_plays_through_its_rooms_to_the_last_boss() {
        let Some(mut play) = start(12) else {
            return;
        };
        assert_plays(&mut play, 90, 0x01);
        for (arg, next) in [(0xC1, (0x20, 2, 0x00)), (0x61, (0x20, 4, 0x00))] {
            transition(&mut play, 1, arg);
            assert_eq!(room(&play), next);
            assert_plays(&mut play, 60, 0x01);
        }
        enter_the_boss_room(&mut play, 0x22);
        assert_plays(&mut play, 60, 0);
    }

    /// The room start's page, drawn through the queue model, is the captured
    /// opening nametable.
    #[test]
    fn the_first_page_is_the_captured_nametable() {
        let Some(play) = play() else {
            return;
        };
        assert_eq!(
            fnv1a(&play.ppu.nametables[0][..960]),
            digest("stage-1-1-page-0"),
            "the first page differs from fceux's (tools/emu/stage, frame 700)"
        );
    }

    /// Half a minute holding Right, jumping and firing: the loop, its column
    /// streaming, the spawns, the actors and the shots all run in the port,
    /// through the room change at the end of the first room.
    #[test]
    fn the_game_runs_holding_right() {
        let Some(mut play) = play() else {
            return;
        };
        for frame in 0..1800 {
            let jump = if frame % 40 < 12 { 0x80 } else { 0 };
            let fire = if frame % 8 < 2 { 0x40 } else { 0 };
            play.step(0x01 | jump | fire);
        }
        assert_eq!(play.game().world.room, 2, "the room change never ran");
        let mut frame = Frame::new();
        play.compose(&mut frame, MAX_SPRITES_PER_LINE, false);
        assert!(frame.rgba().iter().any(|&byte| byte != 0));
    }

    /// Left standing in stage 1-2, Batman is pushed into the first pit, dies (motion 7)
    /// and the area restarts with a life fewer (`$D527`); play goes on.
    #[test]
    fn standing_in_stage_1_2_loses_a_life_and_restarts() {
        let Some(mut play) = start(1) else {
            return;
        };
        let mut died = false;
        for _ in 0..900 {
            play.step(0);
            died |= play.game().effects.player.motion == Motion::Dying;
        }
        let game = play.game();
        assert!(died, "Batman never died");
        assert_eq!((game.effects.lives, game.world.area), (1, 2));
        assert!(game.script.is_none());
        assert_ne!(game.effects.player.motion, Motion::Dying);
    }

    /// With no lives left the continue screen counts down, GAME OVER follows and
    /// the game resets into its boot.
    #[test]
    fn the_last_life_ends_in_game_over_and_a_reset() {
        let Some(mut play) = start(1) else {
            return;
        };
        assert!(
            step_until(&mut play, 4000, 0, Play::booting),
            "the game never reset"
        );
    }

    /// The load into the stage 1 boss room with `$0404` bit 7 set (`$BB3D`): the name
    /// card, the room, Batman's entry (motion 9), then a minute standing in the fight.
    #[test]
    fn the_boss_room_starts_and_batman_enters() {
        let Some(mut play) = start(1) else {
            return;
        };
        let effects = &mut play.game_mut().effects;
        (effects.hud, effects.transition) = (0x80, 3);
        let mut entered = false;
        for _ in 0..3600 {
            play.step(0);
            entered |= play.game().effects.player.motion == Motion::BossEntry;
        }
        assert!(entered, "Batman never entered");
        assert_eq!(play.game().world.area, 4);
        assert_ne!(play.game().effects.player.motion, Motion::BossEntry);
    }

    /// The stage 1 boss beaten (`$0406` set, `$BA = 3`, as type `$50` leaves them): the cutscene (`$98BD`, its song
    /// `$0F` and the raster split's slide), the STAGE 2-1 banner, area 6, and play goes on.
    #[test]
    fn beating_the_stage_1_boss_plays_the_cutscene_and_starts_stage_2() {
        let Some(mut play) = start(1) else {
            return;
        };
        let effects = &mut play.game_mut().effects;
        (effects.hud, effects.transition) = (0x80, 3);
        for _ in 0..600 {
            play.step(0);
        }
        assert_eq!(play.game().world.area, 4, "the boss room never loaded");
        let effects = &mut play.game_mut().effects;
        (effects.cutscene_id, effects.hud, effects.transition) = (0x0E, 0, 3);
        let mut requests = Vec::new();
        let mut split = false;
        for _ in 0..1200 {
            requests.extend(play.step(0).into_iter().flatten().map(SoundId::byte));
            split |= play.raster;
            if play.game().world.area == 6 && play.game().script.is_none() {
                break;
            }
        }
        assert_eq!(play.game().world.area, 6, "stage 2 never loaded");
        assert!(play.game().script.is_none(), "the load never finished");
        assert!(split, "the cutscene's split never ran");
        assert!(requests.contains(&0x0F), "{requests:x?}");
        for _ in 0..120 {
            play.step(0);
        }
        assert_eq!(play.game().world.area, 6);
    }

    /// Stage 2-1's room changes as the TAS takes them (`$BB` = `$61`, then `$81`): the
    /// slide into the two-nametable vertical room (`$DD = $C0`, `$BA46`), the camera
    /// following Batman up it (`$852F`, `$83C4`), and the fade into the third room (`$BA8B`).
    #[test]
    fn stage_2_1_changes_rooms_and_scrolls_up() {
        let Some(mut play) = start(2) else {
            return;
        };
        let change = |play: &mut Play, side: u8| {
            let effects = &mut play.game_mut().effects;
            (effects.transition, effects.transition_arg) = (1, side);
            play.step(0);
            for _ in 0..200 {
                if play.game().script.is_none() {
                    break;
                }
                play.step(0);
            }
            assert!(play.game().script.is_none(), "the room change never ended");
        };
        change(&mut play, 0x61);
        assert_eq!(
            (play.game().world.room, play.game().world.orientation),
            (2, 0xC0)
        );
        let camera = play.game().world.camera_y;
        play.game_mut().effects.player.y -= 0x0400;
        let mut lowest = camera;
        for _ in 0..30 {
            play.step(0);
            lowest = lowest.min(play.game().world.camera_y);
        }
        assert!(lowest < camera, "the camera never scrolled up");
        change(&mut play, 0x81);
        assert_eq!(
            (play.game().world.room, play.game().world.orientation),
            (4, 0)
        );
        for _ in 0..60 {
            play.step(0x01);
        }
        let mut frame = Frame::new();
        play.compose(&mut frame, MAX_SPRITES_PER_LINE, false);
        let backdrop = frame.indices()[0];
        assert!(
            frame.indices().iter().any(|&pixel| pixel != backdrop),
            "the frame is blank"
        );
    }

    /// The ending, as the stage 7 boss's last figure (type `$B8`) leaves it: `$BA = 6`
    /// in stage 7. It runs its song `$0D`, its scenes and credits, and holds its last
    /// screen. Sampled every 100 frames, the picture must show something and keep
    /// changing, so a blank or stuck ending fails.
    #[test]
    fn beating_the_final_boss_plays_the_ending() {
        let Some(mut play) = start(12) else {
            return;
        };
        play.game_mut().effects.transition = 6;
        let mut requests = Vec::new();
        let mut samples = Vec::new();
        let mut frame = Frame::new();
        for tick in 0..7000 {
            requests.extend(play.step(0).into_iter().flatten().map(SoundId::byte));
            if tick % 100 == 50 {
                play.compose(&mut frame, MAX_SPRITES_PER_LINE, false);
                samples.push(frame.indices().to_vec());
            }
        }
        let Some(rotj_core::actor::Script::Ending(ending)) = &play.game().script else {
            panic!("the ending never ran: {:?}", play.game().script);
        };
        assert!(ending.over(), "the ending never reached its last screen");
        for id in [0x0D, 0x22, 0x23] {
            assert!(
                requests.contains(&id),
                "${id:02X} never requested: {requests:x?}"
            );
        }
        let shown: Vec<&Vec<u8>> = samples
            .iter()
            .filter(|pixels| pixels.iter().any(|&pixel| pixel != pixels[0]))
            .collect();
        assert!(shown.len() >= 60, "{} of 70 samples drawn", shown.len());
        let mut distinct = shown.clone();
        distinct.sort();
        distinct.dedup();
        assert!(distinct.len() >= 45, "{} distinct pictures", distinct.len());
        assert!(
            shown.last() == samples.last().as_ref(),
            "the last screen is blank"
        );
    }

    /// Start pauses the game and freezes it; Start again resumes.
    #[test]
    fn start_pauses_and_resumes() {
        let Some(mut play) = play() else {
            return;
        };
        for _ in 0..60 {
            play.step(0x01);
        }
        play.step(0x10);
        let paused = play.game().clone();
        for _ in 0..30 {
            play.step(0x01);
        }
        assert!(matches!(
            play.game().script,
            Some(rotj_core::actor::Script::Pause(_))
        ));
        assert_eq!(play.game().effects.player.x, paused.effects.player.x);
        play.step(0x10);
        play.step(0x01);
        play.step(0x01);
        assert!(play.game().script.is_none());
        for _ in 0..30 {
            play.step(0x01);
        }
        assert_ne!(play.game().effects.player.x, paused.effects.player.x);
    }

    /// The stage end's departure (phases 2-4, as its walk-off leaves them), then the
    /// next area's load (the banner, area 2 and its song `$04`) with stage 1-2's
    /// first page in the nametable, and play goes on.
    #[test]
    fn the_stage_end_loads_stage_1_2() {
        let Some(mut play) = play() else {
            return;
        };
        for _ in 0..120 {
            play.step(0);
        }
        let effects = &mut play.game_mut().effects;
        effects.transition = 2;
        effects.stage_end = StageEnd {
            phase: 2,
            x: 0xFF,
            y: 0x70,
            ..StageEnd::default()
        };
        let mut requests = Vec::new();
        for _ in 0..800 {
            requests.extend(play.step(0).into_iter().flatten().map(SoundId::byte));
            if play.game().world.area == 2 && play.game().script.is_none() {
                break;
            }
        }
        assert_eq!(play.game().world.area, 2, "the next area never loaded");
        assert!(requests.contains(&0x04), "{requests:x?}");
        let fresh = start(1).expect("the ROM");
        assert_eq!(play.ppu.nametables[0], fresh.ppu.nametables[0]);
        for _ in 0..120 {
            play.step(0);
        }
        assert_eq!(play.game().world.area, 2);
    }

    /// A jump makes `Play::step` return effect `$42=$24` (`$84FD`/`$8508`,
    /// bank 4) on the take-off frame, and the driver plays it as the documented
    /// noise sweep (`docs/MUSIC.md` `$24`): the frame's writes include the noise
    /// channel. If the request were dropped, the driver would write none of them.
    #[test]
    fn a_jump_requests_the_jump_effect_and_the_driver_plays_it() {
        let Some(mut play) = play() else {
            return;
        };
        let request = (0..300)
            .map(|frame| play.step(if frame % 40 < 12 { 0x80 } else { 0 }))
            .find(|sound| sound[2] == SoundId::from_byte(rotj_music::JUMP_SFX))
            .expect("the jump requested $42=$24");
        assert_eq!(request[2].map(SoundId::byte), Some(0x24));
        assert_eq!(request[3], None, "the jump is slot $42");
        assert_eq!(
            play.game().effects.player.motion,
            Motion::Jumping,
            "the sound is the take-off"
        );

        let mut driver = crate::audio::driver();
        driver.request(request[2].expect("the jump's request").byte());
        let writes: Vec<(u16, u8)> = driver.tick().iter().map(|w| (w.addr, w.value)).collect();
        assert!(
            writes.contains(&(rotj_apu::noise::CTRL, 0x16)),
            "noise volume: {writes:04x?}"
        );
        assert!(
            writes.contains(&(rotj_apu::noise::TIMER, 0x0E)),
            "noise period: {writes:04x?}"
        );
        assert!(
            writes.contains(&(rotj_apu::noise::LENGTH, 0x08)),
            "noise length: {writes:04x?}"
        );
        assert!(
            writes.contains(&(rotj_apu::pulse2::CTRL, 0x10)),
            "the silent pulse voice: {writes:04x?}"
        );
    }

    /// Holding fire fires the default weapon: `Play::step` returns its effect
    /// `$42=$2D` (bank 4 `$9D2D`: `LDA #$2D / STA $42`).
    #[test]
    fn firing_requests_the_weapon_effect() {
        let Some(mut play) = play() else {
            return;
        };
        let request = (0..600)
            .map(|frame| play.step(if frame % 8 < 2 { 0x40 } else { 0 }))
            .find(|sound| sound[2] == SoundId::from_byte(0x2D))
            .expect("firing requested $42=$2D");
        assert_eq!(request[2].map(SoundId::byte), Some(0x2D));
        assert_eq!(request[3], None, "the fire effect is slot $42");
    }

    /// A damage-class tile under Batman makes `Play::step` return the damage
    /// effect `$42=$25` (`$82C6`/`$B17F`) and costs energy. Every solid metatile
    /// becomes class 3 (`$7340` holds `2 * class`, as in the core's own damage
    /// test), so the ground he lands on hurts.
    #[test]
    fn taking_damage_requests_the_damage_effect() {
        let Some(mut play) = play() else {
            return;
        };
        for class in &mut play.game_mut().effects.level.classes {
            if *class != 0 {
                *class = 6;
            }
        }
        let energy = play.game().effects.player.energy;
        let request = (0..120)
            .map(|_| play.step(0))
            .find(|sound| sound[2] == SoundId::from_byte(0x25))
            .expect("the damage requested $42=$25");
        assert_eq!(request[2].map(SoundId::byte), Some(0x25));
        assert_eq!(request[3], None, "the damage effect is slot $42");
        assert!(
            play.game().effects.player.energy < energy,
            "the hit cost energy ({} -> {})",
            energy,
            play.game().effects.player.energy
        );
    }

    /// A fresh directory under the system temp dir for one test.
    fn temp_dir(test: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rotj-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn the_recorder_writes_each_pad() {
        let dir = temp_dir("recorder-writes");
        let mut recorder = Recorder::open(&dir, 0).expect("recording");
        for pad in [0x81, 0x00, 0x42] {
            recorder.record(pad);
        }
        drop(recorder);
        let files: Vec<_> = std::fs::read_dir(&dir).expect("dir").flatten().collect();
        assert_eq!(files.len(), 1);
        let name = files[0].file_name().into_string().expect("utf-8");
        assert!(name.ends_with("-level0.pads"), "{name}");
        assert_eq!(
            std::fs::read(files[0].path()).expect("read"),
            [0x81, 0x00, 0x42]
        );
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_second_recorder_does_not_clobber() {
        let dir = temp_dir("recorder-clobber");
        let mut first = Recorder::open_at(&dir, 1234, 5).expect("first");
        let mut second = Recorder::open_at(&dir, 1234, 5).expect("second");
        assert!(
            Recorder::open_at(&dir, 1234, 5).is_none(),
            "a third gives up"
        );
        first.record(1);
        second.record(2);
        drop((first, second));
        assert_eq!(
            std::fs::read(dir.join("1234-level5.pads")).expect("first"),
            [1]
        );
        assert_eq!(
            std::fs::read(dir.join("1234-2-level5.pads")).expect("second"),
            [2]
        );
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_missing_directory_records_nothing() {
        let dir = temp_dir("recorder-missing").join("absent");
        assert!(Recorder::open(&dir, 0).is_none());
    }

    fn line(l: &rotj_oracle::Line) -> Line {
        Line {
            v: l.v,
            fine_x: l.fine_x,
            ctrl: l.ctrl,
            mask: l.mask,
            chr: l.chr,
            sprite_chr: l.sprite_chr,
            mirroring: Mirroring::from_register(l.mirroring),
        }
    }

    /// From power-on with no input (the intro, the title, the attract mode's demo and
    /// its pages), the app's every frame is the oracle's: its lines through the same
    /// renderer, from the app's own sequence of IRQs, NMI and loop, the sprites its NMI
    /// copied and the VRAM it wrote. Each tick is the oracle's next frame whose NMI woke
    /// the game's wait: a frame with no NMI (the boot and loads run with it off) or one
    /// whose NMI came before the game waited for it (a lag frame) is skipped, as the port
    /// takes no time for either. A split line is compared in the state the app draws it
    /// with; a line no state of which is the app's (a write the IRQs' jitter carries
    /// across the line's end) is counted, not compared. Blank lines draw only the
    /// backdrop, and blank frames are not compared: the boot's show the power-on
    /// palette, which the hardware leaves undefined.
    #[test]
    #[ignore = "3000 frames of the oracle (release, 5 s); the gates run it in release with --ignored"]
    fn the_app_draws_like_the_oracle_from_power_on() {
        let (Some(mut play), Some(mut m)) = (powered_on(None), rotj_oracle::power_on()) else {
            return;
        };
        m.bus.trace.peeks = vec![0xC232, 0xC23F, 0xE1D0];
        let mut waiting = false;
        let mut lagged = |m: &mut rotj_oracle::Machine| {
            let mut lag = false;
            for peek in m.bus.trace.peeked.drain(..) {
                match peek.pc {
                    0xC232 => waiting = true,
                    0xC23F => waiting = false,
                    _ => lag |= !waiting,
                }
            }
            lag
        };
        let (mut drawn, mut jittered, mut unmatched, mut skipped) = (0, 0, 0, 0);
        let (mut ours, mut theirs) = (Frame::new(), Frame::new());
        for tick in 0..3000 {
            play.step(0);
            let mut log = m.run_frame(0);
            while lagged(&mut m) || log.nmis == 0 {
                skipped += 1;
                log = m.run_frame(0);
            }
            if log.lines.is_empty() {
                continue;
            }
            let mut lines: Vec<Line> = log.lines.iter().map(line).collect();
            let mut compared = [true; 240];
            for (y, mine) in play.lines.iter().enumerate() {
                let states = std::iter::once(lines[y]).chain(
                    log.splits
                        .iter()
                        .filter(|s| s.0 == y)
                        .flat_map(|s| [line(&s.1), line(&s.2)]),
                );
                let blank = |l: &Line| l.mask & mask::RENDERING == 0;
                match states
                    .into_iter()
                    .find(|s| s == mine || blank(s) && blank(mine))
                {
                    Some(state) => lines[y] = state,
                    None => compared[y] = false,
                }
            }
            jittered += usize::from(compared.contains(&false));
            unmatched += compared.iter().filter(|&&c| !c).count();
            if lines.iter().all(|l| l.mask & mask::RENDERING == 0) {
                continue;
            }
            play.compose(&mut ours, MAX_SPRITES_PER_LINE, false);
            theirs.render(
                &Picture {
                    lines: &lines,
                    vram: &m.bus.ppu.vram,
                    palette: &m.bus.ppu.palette,
                    oam: &m.bus.ppu.oam,
                    chr: &m.bus.cart.chr,
                },
                MAX_SPRITES_PER_LINE,
            );
            for y in (0..240).filter(|&y| compared[y]) {
                let row = y * SCREEN_W..(y + 1) * SCREEN_W;
                assert_eq!(
                    ours.indices()[row.clone()],
                    theirs.indices()[row],
                    "tick {tick} (oracle frame {}): line {y}",
                    m.frame
                );
            }
            drawn += 1;
        }
        assert_eq!((drawn, jittered, unmatched, skipped), (2937, 42, 319, 15));
    }
}
