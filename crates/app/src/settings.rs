//! The player's settings, saved as `settings.ron` beside the exe. Every name here is
//! public interface (`docs/PLAN.md` "Versioning"): a later release must read this file.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

const FILE: &str = "settings.ron";

/// Presentation settings; the defaults are the NES's behaviour, or 1.0's where it had none.
#[derive(Resource, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            volume: 100,
            overscan: Overscan::Hidden,
            palette: PaletteChoice::CompositeDirect,
            sprite_limit: true,
            vsync: true,
        }
    }
}

/// Which of the 240 lines show.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overscan {
    /// Lines 8-231, as a TV shows them.
    Hidden,
    /// All 240.
    Shown,
}

/// The colour table.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteChoice {
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
pub struct SettingsDir(pub Option<PathBuf>);

/// The exe's directory.
#[must_use]
pub fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}

/// `dir`'s settings: the defaults without a file; a file that does not parse is kept as
/// `settings.ron.bak` and the defaults used.
#[must_use]
pub fn load(dir: &Path) -> Settings {
    let path = dir.join(FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Settings::default(),
        Err(error) => {
            warn!("{}: {error}; using the defaults", path.display());
            return Settings::default();
        }
    };
    match ron::from_str::<Settings>(&text) {
        Ok(mut settings) => {
            settings.volume = settings.volume.min(100);
            settings
        }
        Err(error) => {
            let backup = dir.join(format!("{FILE}.bak"));
            warn!(
                "{}: {error}; using the defaults, the file kept as {}",
                path.display(),
                backup.display()
            );
            if let Err(error) = fs::rename(&path, &backup) {
                warn!("{}: {error}", backup.display());
            }
            Settings::default()
        }
    }
}

/// Writes `settings` to `dir` through a temporary file, so a crash never leaves half a file.
pub fn save(dir: &Path, settings: &Settings) -> std::io::Result<()> {
    let text = ron::ser::to_string_pretty(settings, ron::ser::PrettyConfig::default())
        .map_err(std::io::Error::other)?;
    let temporary = dir.join(format!("{FILE}.tmp"));
    fs::write(&temporary, text)?;
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
    }

    #[test]
    fn the_1_1_0_file_parses() {
        let settings: Settings =
            ron::from_str(include_str!("../settings-1.1.0.ron")).expect("1.1.0's file");
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn an_unwritable_dir_is_an_error_not_a_panic() {
        let dir = dir("unwritable");
        let file = dir.join("not-a-dir");
        fs::write(&file, "").expect("write");
        assert!(save(&file, &Settings::default()).is_err());
    }
}
