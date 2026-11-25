//! The player's settings, saved as `settings.ron` beside the exe. Every name here is
//! public interface (`docs/PLAN.md` "Versioning"): a later release must read this file.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use ron::value::RawValue;
use serde::{Deserialize, Serialize};

const FILE: &str = "settings.ron";

/// Presentation settings; the defaults are the NES's behaviour, or 1.0's where it had none.
#[derive(Resource, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Settings {
    /// Master volume, 0-100.
    pub volume: u8,
    /// Whether the 8 lines a TV hides at the top and bottom show.
    pub overscan: Overscan,
    /// The colours the PPU's 64 values show as.
    pub palette: PaletteChoice,
    /// The PPU's 8 sprites per line, whose overflow is the game's flicker.
    pub sprite_limit: bool,
    /// Present on the display's vertical blank.
    pub vsync: bool,
    /// How the game's own UI (HUD, pause card, title cursor, password entry) is drawn.
    pub ui: UiStyle,
    /// How the picture is drawn.
    pub picture: Picture,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            volume: 100,
            overscan: Overscan::Hidden,
            palette: PaletteChoice::CompositeDirect,
            sprite_limit: true,
            vsync: true,
            ui: UiStyle::Classic,
            picture: Picture::Classic,
        }
    }
}

/// How the picture is drawn.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Picture {
    /// The PPU's picture, line by line.
    Classic,
    /// In play, the level's tilemap and the game's sprites as Bevy entities.
    Modern,
}

/// How the game's own UI is drawn.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UiStyle {
    /// As the game's sprites, flicker and all.
    Classic,
    /// The game's own UI sprites as Bevy UI, steady.
    Faithful,
}

/// Which of the 240 lines show.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Overscan {
    /// Lines 8-231, as a TV shows them.
    Hidden,
    /// All 240.
    Shown,
}

/// The colour table.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaletteChoice {
    /// FirebrandX's Composite Direct, a NES's composite output.
    CompositeDirect,
    /// The 2C03 RGB PPU.
    RgbPpu,
    /// The composite signal decoded.
    Ntsc,
    /// `palette.pal` beside the exe.
    File,
}

/// Where `settings.ron` lives; `None` when the exe's directory is unknown.
#[derive(Resource)]
pub(crate) struct SettingsDir(pub Option<PathBuf>);

/// The exe's directory.
#[must_use]
pub(crate) fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}

/// `dir`'s settings and any non-fatal warnings: the defaults without a file; a file that
/// does not parse is kept as `settings.ron.bak` and the defaults used.
#[must_use]
pub(crate) fn load_with_warnings(dir: &Path) -> (Settings, Vec<String>) {
    let path = dir.join(FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return (Settings::default(), vec![]),
        Err(error) => {
            return (
                Settings::default(),
                vec![format!("{}: {error}; using the defaults", path.display())],
            );
        }
    };
    match parse(&text) {
        Ok((settings, field_warnings)) => {
            let warnings = field_warnings
                .into_iter()
                .map(|message| format!("{}: {message}", path.display()))
                .collect();
            (settings, warnings)
        }
        Err(error) => {
            let backup = dir.join(format!("{FILE}.bak"));
            let mut warnings = vec![format!(
                "{}: {error}; using the defaults, the file kept as {}",
                path.display(),
                backup.display()
            )];
            if let Err(error) = fs::rename(&path, &backup) {
                warnings.push(format!("{}: {error}", backup.display()));
            }
            (Settings::default(), warnings)
        }
    }
}

/// Each setting's raw value, so one this release cannot read leaves the others.
#[derive(Deserialize)]
struct Fields {
    volume: Option<Box<RawValue>>,
    overscan: Option<Box<RawValue>>,
    palette: Option<Box<RawValue>>,
    sprite_limit: Option<Box<RawValue>>,
    vsync: Option<Box<RawValue>>,
    ui: Option<Box<RawValue>>,
    picture: Option<Box<RawValue>>,
}

/// The settings `text` holds that this release reads, each on its own: a value it does
/// not know (a newer release's) keeps that setting's default, not every setting's.
fn parse(text: &str) -> ron::error::SpannedResult<(Settings, Vec<String>)> {
    fn read<T: serde::de::DeserializeOwned>(
        name: &str,
        raw: Option<Box<RawValue>>,
        into: &mut T,
        warnings: &mut Vec<String>,
    ) {
        let Some(raw) = raw else { return };
        match raw.into_rust() {
            Ok(value) => *into = value,
            Err(error) => warnings.push(format!("ignoring `{name}` ({error}); using the default")),
        }
    }
    let fields: Fields = ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(text)?;
    let mut warnings = Vec::new();
    let mut settings = Settings::default();
    let mut volume = u16::from(settings.volume);
    read("volume", fields.volume, &mut volume, &mut warnings);
    settings.volume = u8::try_from(volume.min(100)).expect("at most 100");
    read(
        "overscan",
        fields.overscan,
        &mut settings.overscan,
        &mut warnings,
    );
    read(
        "palette",
        fields.palette,
        &mut settings.palette,
        &mut warnings,
    );
    read(
        "sprite_limit",
        fields.sprite_limit,
        &mut settings.sprite_limit,
        &mut warnings,
    );
    read("vsync", fields.vsync, &mut settings.vsync, &mut warnings);
    read("ui", fields.ui, &mut settings.ui, &mut warnings);
    read(
        "picture",
        fields.picture,
        &mut settings.picture,
        &mut warnings,
    );
    Ok((settings, warnings))
}

/// Writes `settings` to `dir` through a temporary file, so a crash never leaves half a file.
pub(crate) fn save(dir: &Path, settings: &Settings) -> std::io::Result<()> {
    let text = ron::ser::to_string_pretty(settings, ron::ser::PrettyConfig::default())
        .map_err(std::io::Error::other)?;
    let temporary = dir.join(format!("{FILE}.tmp"));
    let mut file = fs::File::create(&temporary)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    fs::rename(&temporary, dir.join(FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty directory of its own for each test.
    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rotj-settings-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn load(dir: &Path) -> Settings {
        load_with_warnings(dir).0
    }

    fn load_text(name: &str, text: &str) -> (PathBuf, Settings) {
        let dir = dir(name);
        fs::write(dir.join(FILE), text).expect("write");
        let settings = load(&dir);
        (dir, settings)
    }

    #[test]
    fn no_file_is_the_defaults() {
        let dir = dir("none");
        assert_eq!(load(&dir), Settings::default());
        assert_eq!(fs::read_dir(&dir).expect("dir").count(), 0);
    }

    #[test]
    fn settings_round_trip() {
        let dir = dir("round-trip");
        let settings = Settings {
            volume: 35,
            overscan: Overscan::Shown,
            palette: PaletteChoice::Ntsc,
            sprite_limit: false,
            vsync: false,
            ui: UiStyle::Faithful,
            picture: Picture::Modern,
        };
        save(&dir, &settings).expect("save");
        assert_eq!(load(&dir), settings);
        assert!(!dir.join("settings.ron.tmp").exists());
    }

    #[test]
    fn a_missing_key_is_its_default() {
        let expected = Settings {
            volume: 40,
            ..Settings::default()
        };
        assert_eq!(load_text("missing", "(volume: 40)").1, expected);
    }

    #[test]
    fn an_unknown_key_is_ignored() {
        let (_, settings) = load_text("unknown", "(volume: 40, frame_rate: Display)");
        assert_eq!(settings.volume, 40);
    }

    #[test]
    fn a_bad_file_is_kept_as_bak() {
        let (dir, settings) = load_text("bad", "(volume: ");
        assert_eq!(settings, Settings::default());
        assert_eq!(
            fs::read_to_string(dir.join("settings.ron.bak")).expect("bak"),
            "(volume: "
        );
    }

    #[test]
    fn volume_is_clamped() {
        assert_eq!(load_text("loud", "(volume: 250)").1.volume, 100);
        assert_eq!(load_text("louder", "(volume: 300)").1.volume, 100);
    }

    #[test]
    fn a_value_this_release_does_not_know_keeps_the_rest() {
        let dir = dir("newer");
        fs::write(
            dir.join(FILE),
            "(volume: 40, palette: Hdr, overscan: Shown, vsync: maybe, ui: Modern)",
        )
        .expect("write");
        let (settings, warnings) = load_with_warnings(&dir);
        let expected = Settings {
            volume: 40,
            overscan: Overscan::Shown,
            ..Settings::default()
        };
        assert_eq!(settings, expected);
        assert_eq!(warnings.len(), 3, "each invalid field warned: {warnings:?}");
        assert!(!dir.join("settings.ron.bak").exists(), "the file is kept");
    }

    #[test]
    fn the_1_1_0_file_parses() {
        let (settings, warnings) =
            parse(include_str!("../settings-1.1.0.ron")).expect("1.1.0's file");
        assert_eq!(settings, Settings::default());
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn an_unwritable_dir_is_an_error_not_a_panic() {
        let dir = dir("unwritable");
        let file = dir.join("not-a-dir");
        fs::write(&file, "").expect("write");
        assert!(save(&file, &Settings::default()).is_err());
    }
}
