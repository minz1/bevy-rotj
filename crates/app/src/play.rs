//! The app running the port's game (`rotj_core::actor::Game`) from power-on: the
//! boot, intro and title, then the game loop, one `Game::frame` per 60 Hz tick, the
//! NMI's queue drained into the nametables and the palette, its sound requests handed
//! to the music driver, and the picture composed from the game's own PPU state.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use rotj_core::actor::scroll::Scroll;
use rotj_core::actor::world::Ppu;
use rotj_core::actor::{Game, Script, cutscene};
use rotj_core::render::{Background, Frame, Mirroring, SCREEN_H, Sprites};
use std::ops::Range;

use crate::input::NesInput;
use crate::screen::{Screen, screen_frame, screen_image};
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
    prg: Vec<u8>,
    chr: Vec<u8>,
    ppu: Ppu,
    /// The NMI armed the raster IRQ (`$D0` bit 7), so this frame is split.
    raster: bool,
    /// The level forced at `$D3D8`, kept for the boot after a reset.
    forced: Option<u8>,
}

impl Play {
    /// The game at power-on; `forced` overrides the level the title starts.
    #[must_use]
    pub fn power_on(prg: Vec<u8>, chr: Vec<u8>, forced: Option<u8>) -> Play {
        Play {
            game: Game::power_on(forced),
            prg,
            chr,
            ppu: Ppu::new(),
            raster: false,
            forced,
        }
    }

    /// The game at `level`'s first loop frame, its first page already in the nametables.
    #[cfg(test)]
    #[must_use]
    pub fn start(prg: Vec<u8>, chr: Vec<u8>, level: usize) -> Play {
        let mut game = rotj_core::actor::room::start(&prg, &chr, level);
        let mut ppu = Ppu::new();
        let page = std::mem::take(&mut game.effects.vram.direct);
        ppu.apply(&page, game.scroll.mirroring);
        ppu.palette = game.effects.palette;
        Play {
            game,
            prg,
            chr,
            ppu,
            raster: false,
            forced: None,
        }
    }

    /// One frame of the game and its NMI, with joypad 1 held as `pad`; the sound
    /// requests the NMI hands the driver (`$40-$43`, 0 for none). A reset (GAME OVER,
    /// `JMP ($FFFC)`) runs on into the boot's first frame.
    pub fn step(&mut self, pad: u8) -> [u8; 4] {
        self.game.frame(&self.prg, &self.chr, &mut |_, _| {});
        if self.game.reset() {
            self.game = Game::power_on(self.forced);
            self.raster = false;
            self.game.frame(&self.prg, &self.chr, &mut |_, _| {});
        }
        if self.raster {
            cutscene::raster_irqs(&mut self.game);
        }
        let vblank = self.game.nmi(pad);
        self.raster = self.game.scroll.room[0] & 0x80 != 0;
        self.ppu.apply(&vblank.writes, self.game.scroll.mirroring);
        vblank.sound
    }

    /// True until the boot reaches the first loop frame.
    #[must_use]
    pub fn booting(&self) -> bool {
        matches!(&self.game.script, Some(Script::Boot(boot)) if !boot.done())
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

    /// Composes the picture from the game's PPU state.
    pub fn compose(&self, frame: &mut Frame) {
        let scroll = &self.game.scroll;
        let mut pattern = vec![0u8; 8 * 1024];
        for (i, &bank) in self.game.oam.chr.iter().enumerate() {
            let src = usize::from(bank) * 1024;
            pattern[i * 1024..(i + 1) * 1024].copy_from_slice(&self.chr[src..src + 1024]);
        }
        let mut tables = Vec::with_capacity(2048);
        let (first, second, mirroring) = match scroll.mirroring & 3 {
            0 => (0, 1, Mirroring::Vertical),
            1 => (0, 1, Mirroring::Horizontal),
            2 => (0, 0, Mirroring::Vertical),
            _ => (1, 1, Mirroring::Vertical),
        };
        tables.extend_from_slice(&self.ppu.nametables[first]);
        tables.extend_from_slice(&self.ppu.nametables[second]);
        frame.clear();
        let scroll_y = u16::from((scroll.ppu_ctrl >> 1) & 1) * 240 + u16::from(scroll.scroll_y);
        let scroll_x = u16::from(scroll.ppu_ctrl & 1) * 256 + u16::from(scroll.scroll_x);
        let Split {
            bands,
            sprite_lines,
            row_from,
        } = split(scroll, scroll_x, self.raster);
        for (lines, scroll_x) in bands {
            let scroll_y = match row_from {
                Some((at, row)) if lines.start >= at => {
                    let at = u16::try_from(at).expect("a scanline");
                    (row + 480 - at) % 480
                }
                _ => scroll_y,
            };
            frame.background(
                lines,
                &Background {
                    nametables: &tables,
                    pattern: &pattern,
                    pattern_base: 0,
                    palette: &self.ppu.palette,
                    mirroring,
                    scroll_x,
                    scroll_y,
                },
            );
        }
        frame.sprites(
            sprite_lines,
            &Sprites {
                oam: &self.game.oam.page,
                pattern: &pattern,
                pattern_base: 0x1000,
                tall: true,
                palette: &self.ppu.palette,
            },
        );
    }
}

/// How the raster split divides the picture: the background's bands `(lines, x
/// scroll)`, the lines sprites show on, and the line from which a `$2006` write points
/// the background at a nametable row.
struct Split {
    bands: Vec<(Range<usize>, u16)>,
    sprite_lines: Range<usize>,
    row_from: Option<(usize, u16)>,
}

/// The split the NMI armed (`raster`) for this picture: the intro's (`$D3 = 9`), the
/// cutscene's (`$0A`) and the ending's (`$0C`, `$0D`), or one band at `scroll_x`.
fn split(scroll: &Scroll, scroll_x: u16, raster: bool) -> Split {
    let mut bands = vec![(0..SCREEN_H, scroll_x)];
    let mut sprite_lines = 0..SCREEN_H;
    // From this line on the background shows this nametable row (a `$2006` write).
    let mut row_from: Option<(usize, u16)> = None;
    if raster {
        let room = &scroll.room;
        let table = |bit: u8| u16::from((scroll.ppu_ctrl | bit) & 1) * 256;
        let first = usize::from(u16::from_le_bytes([room[1], room[2]]));
        let line = |cycles: usize| split_line(first + cycles);
        // Each band runs from one `(start line, x scroll)` breakpoint to the next.
        let bands_of = |points: &[(usize, u16)]| -> Vec<(Range<usize>, u16)> {
            (0..points.len())
                .map(|i| {
                    let (start, scroll) = points[i];
                    let end = points.get(i + 1).map_or(SCREEN_H, |point| point.0);
                    (start..end, scroll)
                })
                .collect()
        };
        match room[3] {
            0x09 => {
                let start = line(0);
                let band = |shift: u8, x: u8| table(room[10] >> shift) + u16::from(x);
                bands = bands_of(&[
                    (0, scroll_x),
                    (start, u16::from(room[5])),
                    (line(0x480), band(1, room[6])),
                    (line(0x780), band(2, room[7])),
                    (line(0xA80), band(3, room[8])),
                    (line(0xD80), band(4, room[9])),
                    (line(0x1080), band(3, room[8])),
                    (line(0x1380), band(2, room[7])),
                    (line(0x1680), band(1, room[6])),
                    (line(0x1980), band(0, room[5])),
                    (line(0x1F80), table(1)),
                ]);
                row_from = Some((start, INTRO_ROW));
            }
            0x0A => {
                bands = bands_of(&[
                    (0, scroll_x),
                    (line(0), table(room[10]) + u16::from(room[5])),
                    (line(0x1000), table(room[10] >> 1) + u16::from(room[6])),
                    (line(0x2000), 0),
                ]);
            }
            0x0C => {
                let end = line(0x4000);
                bands = bands_of(&[
                    (0, scroll_x),
                    (line(0), table(1) + u16::from(room[5])),
                    (end, table(0)),
                ]);
                sprite_lines = 0..end;
            }
            0x0D => {
                bands = bands_of(&[
                    (0, scroll_x),
                    (line(0), table(0) + u16::from(room[8])),
                    (line(0x1000), table(0) + u16::from(room[10])),
                    (line(0x1700), table(0) + u16::from(room[8])),
                    (line(0x2700), table(0)),
                ]);
            }
            _ => {}
        }
    }
    Split {
        bands,
        sprite_lines,
        row_from,
    }
}

/// The nametable row the intro's first raster IRQ points the PPU at (`$2006 = $2240`,
/// bank 2 `$95FA`): row 18, fine y 2.
const INTRO_ROW: u16 = 146;

/// The scanline a raster IRQ splits at `cycles` CPU cycles after the NMI: the first
/// at `$D1:$D2`, each next as its handler arms it (bank 2 `$98AD`). The cutscene's
/// three come `$1000` apart; the ending's title split (`$0C`) hides the sprites below
/// its second, and its credits' parallax (`$0D`) splits four bands.
fn split_line(cycles: usize) -> usize {
    // shortcut: a fixed 60-cycle NMI and IRQ entry latency, not cycle-timed.
    ((cycles + 60) * 3)
        .div_ceil(341)
        .saturating_sub(21)
        .min(SCREEN_H)
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
            .add_systems(FixedUpdate, tick)
            .add_systems(Update, (draw, crate::screen::upload_frame).chain());
    }
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
    commands.insert_resource(Play::power_on(rom.0.prg.clone(), rom.0.chr.clone(), forced));
    crate::screen::spawn_screen_quad(&mut commands, &screen_image(&screen));
}

/// One frame of the game; the state follows it: the title while it boots (again after
/// GAME OVER's reset), gameplay from its first loop frame.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
fn tick(
    input: Res<'_, NesInput>,
    play: Option<ResMut<'_, Play>>,
    recorder: Option<ResMut<'_, Recorder>>,
    music: Option<ResMut<'_, crate::audio::Music>>,
    state: Res<'_, State<GameState>>,
    mut next: ResMut<'_, NextState<GameState>>,
) {
    let Some(mut play) = play else {
        return;
    };
    let pad = input.buttons.game_byte();
    if let Some(mut recorder) = recorder {
        recorder.record(pad);
    }
    let sound = play.step(pad);
    if let Some(mut music) = music {
        for request in sound.into_iter().filter(|&request| request != 0) {
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

fn draw(play: Option<Res<'_, Play>>, mut screen: ResMut<'_, Screen>) {
    if let Some(play) = play {
        play.compose(screen_frame(&mut screen));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rotj_core::actor::stage::StageEnd;
    use rotj_core::digest::fnv1a;
    use rotj_core::render::SCREEN_W;

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
        Some(Play::power_on(rom.prg.clone(), rom.chr.clone(), forced))
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
            split += usize::from(play.raster && play.game().scroll.room[3] == 9);
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
            if sound[1] != 0 {
                songs.push(sound[1]);
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
        Some(Play::start(rom.prg.clone(), rom.chr.clone(), level))
    }

    /// Arms `$BA`/`$BB` and runs the script that follows to its end.
    fn transition(play: &mut Play, kind: u8, arg: u8) {
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
    fn enter_the_boss_room(play: &mut Play, area: u8) {
        play.game_mut().effects.hud = 0x80;
        transition(play, 3, 0);
        assert_eq!(play.game().world.area, area);
        for _ in 0..200 {
            if play.game().effects.player.motion != 9 {
                break;
            }
            play.step(0);
        }
        assert_ne!(play.game().effects.player.motion, 9, "Batman never entered");
    }

    fn picture(play: &Play) -> Vec<u8> {
        let mut frame = Frame::new();
        play.compose(&mut frame);
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
    /// streaming, the spawns, the actors and the shots run without an unported
    /// path, through the room change at the end of the first room.
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
        play.compose(&mut frame);
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
            died |= play.game().effects.player.motion == 7;
        }
        let game = play.game();
        assert!(died, "Batman never died");
        assert_eq!((game.effects.lives, game.world.area), (1, 2));
        assert!(game.script.is_none());
        assert_ne!(game.effects.player.motion, 7);
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
            entered |= play.game().effects.player.motion == 9;
        }
        assert!(entered, "Batman never entered");
        assert_eq!(play.game().world.area, 4);
        assert_ne!(play.game().effects.player.motion, 9);
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
            requests.extend(play.step(0).into_iter().filter(|&id| id != 0));
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
        play.compose(&mut frame);
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
            requests.extend(play.step(0).into_iter().filter(|&id| id != 0));
            if tick % 100 == 50 {
                play.compose(&mut frame);
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
            requests.extend(play.step(0).into_iter().filter(|&id| id != 0));
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
            .find(|sound| sound[2] == rotj_music::JUMP_SFX)
            .expect("the jump requested $42=$24");
        assert_eq!(request[2], 0x24);
        assert_eq!(request[3], 0, "the jump is slot $42");
        assert_eq!(
            play.game().effects.player.motion,
            3,
            "the sound is the take-off"
        );

        let mut driver = crate::audio::driver();
        driver.request(request[2]);
        let writes: Vec<(u16, u8)> = driver.tick().iter().map(|w| (w.addr, w.value)).collect();
        assert!(
            writes.contains(&(0x400C, 0x16)),
            "noise volume: {writes:04x?}"
        );
        assert!(
            writes.contains(&(0x400E, 0x0E)),
            "noise period: {writes:04x?}"
        );
        assert!(
            writes.contains(&(0x400F, 0x08)),
            "noise length: {writes:04x?}"
        );
        assert!(
            writes.contains(&(0x4004, 0x10)),
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
            .find(|sound| sound[2] == 0x2D)
            .expect("firing requested $42=$2D");
        assert_eq!(request[2], 0x2D);
        assert_eq!(request[3], 0, "the fire effect is slot $42");
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
            .find(|sound| sound[2] == 0x25)
            .expect("the damage requested $42=$25");
        assert_eq!(request[2], 0x25);
        assert_eq!(request[3], 0, "the damage effect is slot $42");
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
}
