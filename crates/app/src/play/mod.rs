//! The app running the port's game (`rotj_core::Session`) from power-on: the
//! boot, intro and title, then the game loop, one step per 60 Hz tick, its sound
//! requests handed to the music driver, and the picture composed from the session's
//! PPU state.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use rotj_core::levels::Levels;
use rotj_core::prg::Prg;
use rotj_core::render::{Frame, Line, MAX_SPRITES_PER_LINE, OAM_SPRITES};
use rotj_core::sim::level_area;
use rotj_core::sim::oam::{Mark, OamEntry};
use rotj_core::{Buttons, Session, SoundId};

use crate::input::NesInput;
use crate::pacing::Step;
use crate::screen::{Screen, screen_frame, screen_image};
use crate::settings::Settings;
use crate::{GameLevels, GameState, RomData};

/// A level to start on instead of the title's, from `ROTJ_LEVEL` (a checkpoint of
/// `levels`, 0-12 in the original game), forced where the game reads it (`$D3D8`), as a
/// developer's poke would; the env var follows `ROTJ_RENDER_BACKEND`. 0 is the title's own
/// start and an unparsable or out-of-range value forces nothing, so a recording's `level0`
/// always means "as the title chose".
fn forced_level(levels: &Levels) -> Option<u8> {
    let value = std::env::var("ROTJ_LEVEL").ok()?;
    match value.parse::<u8>() {
        Ok(0) => None,
        Ok(level) if level_area(levels, usize::from(level)).is_some() => Some(level),
        _ => {
            warn!("ROTJ_LEVEL {value:?} is not one of the levels' checkpoints; the title chooses");
            None
        }
    }
}

/// The HUD label's sprites from the previous step, carried across even frames when the
/// game omits the odd-frame L/E label.
#[derive(Default)]
pub(crate) struct LabelHold(Vec<OamEntry>);

impl LabelHold {
    /// Captures the outgoing page's `Mark::Label` sprites before the next step overwrites it.
    fn update(&mut self, oam: &[u8; 256], marks: &[Mark; 64]) {
        self.0 = marked(oam, marks, Mark::Label);
    }

    /// The carried label sprites when this frame drew none, or empty otherwise.
    #[must_use]
    pub(crate) fn carried(&self, marks: &[Mark; 64]) -> &[OamEntry] {
        if marks.contains(&Mark::Label) {
            &[]
        } else {
            &self.0
        }
    }

    #[cfg(test)]
    fn entries(&self) -> &[OamEntry] {
        &self.0
    }
}

/// The Bevy resource wrapping the core [`Session`], its ROM data, and the carried HUD label.
#[derive(Resource)]
pub(crate) struct Play {
    session: Session,
    prg: Prg,
    pub(crate) chr: Vec<u8>,
    /// The HUD label's sprites carried from the step before, which the game draws every other frame.
    pub(crate) label: LabelHold,
}

impl Play {
    /// The game at power-on; `forced` overrides the level the title starts.
    #[must_use]
    pub(crate) fn power_on(prg: Prg, chr: Vec<u8>, forced: Option<u8>) -> Play {
        let session = Session::power_on(&prg, &chr, forced);
        Play {
            session,
            prg,
            chr,
            label: LabelHold::default(),
        }
    }

    /// The game at the end of `level`'s first loop frame, its first page already in the
    /// nametables.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn start(prg: Prg, chr: Vec<u8>, level: usize) -> Play {
        let session = Session::start(&prg, &chr, level);
        Play {
            session,
            prg,
            chr,
            label: LabelHold::default(),
        }
    }

    /// Steps one 60 Hz frame with joypad 1 held as `pad` and returns the sound requests
    /// the NMI hands the music driver (`$40-$43`).
    pub(crate) fn step(&mut self, pad: Buttons) -> [Option<SoundId>; 4] {
        self.label.update(self.session.oam(), self.session.marks());
        self.session.step(pad, &self.prg, &self.chr)
    }

    /// The underlying simulation session.
    #[must_use]
    pub(crate) const fn session(&self) -> &Session {
        &self.session
    }

    /// Mutable access to the simulation session (scene setup in tests).
    #[cfg(test)]
    pub(crate) fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    /// Triggers death on the first live final-boss part (scene setup in tests).
    #[cfg(test)]
    pub(crate) fn destroy_final_boss_part(
        &mut self,
    ) -> Option<(u16, rotj_core::render::Mirroring)> {
        self.session.destroy_final_boss_part(&self.prg)
    }

    /// True until the boot reaches the first loop frame.
    #[must_use]
    pub(crate) fn booting(&self) -> bool {
        self.session.booting()
    }

    /// True while the game's own pause is up (Start in the loop, `$9C37`).
    #[must_use]
    pub(crate) fn paused(&self) -> bool {
        self.session.paused()
    }

    /// Composes the picture the PPU draws from the session's state.
    pub(crate) fn compose(&self, frame: &mut Frame, sprites_per_line: usize, park: bool) {
        self.session
            .compose(&self.chr, frame, sprites_per_line, park);
    }
}

/// The entries of `oam` that `mark` placed.
fn marked(oam: &[u8; 256], marks: &[Mark; 64], mark: Mark) -> Vec<OamEntry> {
    rotj_core::sim::oam::entries(oam)
        .enumerate()
        .filter(|&(entry, _)| marks[entry] == mark)
        .map(|(_, sprite)| sprite)
        .collect()
}

/// A session's pads for a replay against the oracle: one byte per [`Play::step`], the
/// pad it was given, to `<dir>/<unix millis>-level<N>.pads` (`ROTJ_RECORD_INPUT=<dir>`).
#[derive(Resource)]
pub(crate) enum Recorder {
    /// Actively appending one `$F6` byte per tick to the open file.
    Recording(File),
    /// Recording stopped after a write error.
    Stopped,
}

impl Recorder {
    /// A recording of a session starting at `level`, named by the clock.
    #[must_use]
    fn open(dir: &Path, level: usize) -> Option<Recorder> {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis());
        Recorder::open_at(dir, millis, level)
    }

    /// A new file `<millis>-level<N>.pads`, or `<millis>-2-level<N>.pads` if a session
    /// started in the same millisecond took that; never an existing file or link.
    #[must_use]
    fn open_at(dir: &Path, millis: u128, level: usize) -> Option<Recorder> {
        let create = |name: String| {
            let path = dir.join(name);
            OpenOptions::new().write(true).create_new(true).open(&path)
        };
        match create(format!("{millis}-level{level}.pads")).or_else(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                create(format!("{millis}-2-level{level}.pads"))
            } else {
                Err(error)
            }
        }) {
            Ok(file) => Some(Recorder::Recording(file)),
            Err(error) => {
                warn!("not recording input to {}: {error}", dir.display());
                None
            }
        }
    }

    /// Appends `pad`; after a failed write, warns once and records nothing more.
    fn record(&mut self, pad: Buttons) {
        if let Recorder::Recording(file) = self
            && let Err(error) = file.write_all(&[pad.bits()])
        {
            warn!("input recording stopped: {error}");
            *self = Recorder::Stopped;
        }
    }
}

/// The app's game plugin.
pub(crate) struct PlayPlugin;

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
    !menu.is_open()
}

/// Powers the game on, records its pads if asked (`ROTJ_RECORD_INPUT`) and shows it.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
fn power_on(
    mut commands: Commands<'_, '_>,
    screen: Res<'_, Screen>,
    rom: Res<'_, RomData>,
    levels: Res<'_, GameLevels>,
) {
    let forced = forced_level(&levels.0);
    let dir = std::env::var_os("ROTJ_RECORD_INPUT").filter(|dir| !dir.is_empty());
    let level = usize::from(forced.unwrap_or(0));
    if let Some(recorder) = dir.and_then(|dir| Recorder::open(Path::new(&dir), level)) {
        commands.insert_resource(recorder);
    }
    let prg = Prg::new(rom.0.prg.clone(), levels.0.clone());
    commands.insert_resource(Play::power_on(prg, rom.0.chr.clone(), forced));
    crate::screen::spawn_screen_quad(&mut commands, &screen_image(&screen));
}

/// Bundles the optional host services updated on each 60 Hz simulation tick.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct TickServices<'w> {
    clock: Option<Res<'w, crate::pacing::Clock>>,
    recorder: Option<ResMut<'w, Recorder>>,
    music: Option<ResMut<'w, crate::audio::Music>>,
    history: Option<ResMut<'w, crate::mirror::InterpolationHistory>>,
}

/// One or more 60 Hz frames of the game; the state follows it: the title while it boots
/// (again after GAME OVER's reset), gameplay from its first loop frame.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
pub(crate) fn tick(
    mut input: ResMut<'_, NesInput>,
    mut play: ResMut<'_, Play>,
    mut services: TickServices<'_>,
    state: Res<'_, State<GameState>>,
    mut next: ResMut<'_, NextState<GameState>>,
) {
    let ticks = services.clock.as_ref().map_or(1, |c| c.ticks_due().max(1));
    for _ in 0..ticks {
        let pad = input.take_tick_buttons();
        if let Some(recorder) = services.recorder.as_deref_mut() {
            recorder.record(pad);
        }
        let sound = play.step(pad);
        if let Some(history) = services.history.as_deref_mut() {
            history.push(crate::mirror::TickSnapshot {
                room: play.session().room_id(),
                paused: play.paused(),
                snapshot: play.session().snapshot().clone(),
            });
        }
        if let Some(music) = services.music.as_deref_mut() {
            music.request_frame_with_all(sound, play.session().tick_sounds());
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
fn draw(
    play: Res<'_, Play>,
    settings: Res<'_, Settings>,
    mut screen: ResMut<'_, Screen>,
    mut last_picture: Local<'_, Option<crate::modern::ActivePicture>>,
) {
    let active = crate::modern::ActivePicture::for_frame(&settings, &play);
    if active == crate::modern::ActivePicture::Modern {
        *last_picture = Some(active);
        return;
    }
    let changed = play.is_changed()
        || settings.is_changed()
        || *last_picture != Some(crate::modern::ActivePicture::Classic);
    *last_picture = Some(active);
    if !changed {
        return;
    }
    let limit = if settings.sprite_limit {
        MAX_SPRITES_PER_LINE
    } else {
        OAM_SPRITES
    };
    let faithful = settings.ui == crate::settings::UiStyle::Faithful;
    play.compose(screen_frame(&mut screen), limit, faithful);
    let hidden = settings.overscan == crate::settings::Overscan::Hidden;
    let lines = play.session().lines();
    let edges = std::array::from_fn(|y| hidden && lines.get(y).is_some_and(Line::scrolled));
    screen.edges = edges;
    screen.blend = settings.intro_blend && play.session().intro_flickering();
}

#[cfg(test)]
pub(crate) mod tests;
