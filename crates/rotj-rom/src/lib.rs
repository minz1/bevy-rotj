//! The user's own Batman: Return of the Joker (USA) ROM: where it is found, how it is
//! checked, and its PRG and CHR. Nothing derived from the ROM is distributed with the
//! port; the app's build script embeds it (an exe built without it loads it at start-up)
//! and every test reads it through this crate.

mod md5;

pub use md5::md5_hex;

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// MD5 of PRG followed by CHR, without the iNES header, of the one dump the port is
/// verified against (the whole iNES file is `d229eab8a56783be1af66d1fec169f92`).
pub const PRG_CHR_MD5: &str = "830111df30b8511562d4faca6aeb792d";

/// The ROM's file name in a development checkout's `raw/`.
const RAW_NAME: &str = "Batman - Return of the Joker (USA).nes";

/// PRG and CHR ROM of the cartridge (FME-7, 128 KiB + 256 KiB).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rom {
    /// PRG ROM.
    pub prg: Vec<u8>,
    /// CHR ROM.
    pub chr: Vec<u8>,
}

impl Rom {
    /// Parses an iNES image of mapper 69 (FME-7) without checking which game it is.
    pub fn parse_ines(bytes: &[u8]) -> Result<Rom, String> {
        let header = bytes.get(..16).ok_or("shorter than an iNES header")?;
        if &header[..4] != b"NES\x1a" {
            return Err("not an iNES image".into());
        }
        let mapper = (header[6] >> 4) | (header[7] & 0xF0);
        if mapper != 69 {
            return Err(format!("mapper {mapper}, expected 69 (FME-7)"));
        }
        if header[4] == 0 {
            return Err("no PRG ROM".into());
        }
        let prg_start = if header[6] & 0x04 == 0 { 16 } else { 16 + 512 };
        let prg_end = prg_start + usize::from(header[4]) * 0x4000;
        let chr_end = prg_end + usize::from(header[5]) * 0x2000;
        let prg = bytes.get(prg_start..prg_end).ok_or("PRG truncated")?;
        let chr = bytes.get(prg_end..chr_end).ok_or("CHR truncated")?;
        Ok(Rom {
            prg: prg.to_vec(),
            chr: chr.to_vec(),
        })
    }

    /// Parses an iNES image and checks it is the dump the port is verified against.
    pub fn from_ines(bytes: &[u8]) -> Result<Rom, String> {
        let rom = Rom::parse_ines(bytes)?;
        let md5 = md5_hex(&[rom.prg.as_slice(), rom.chr.as_slice()].concat());
        if md5 != PRG_CHR_MD5 {
            return Err(format!(
                "not Batman: Return of the Joker (USA): PRG+CHR MD5 {md5}, expected {PRG_CHR_MD5}"
            ));
        }
        Ok(rom)
    }

    /// The ROM of an exe built without one: `arg` (a file dropped on the exe), `ROTJ_ROM`,
    /// then a `.nes` beside the exe.
    pub fn load(arg: Option<&Path>) -> Result<Rom, String> {
        let env = std::env::var_os("ROTJ_ROM").filter(|env| !env.is_empty());
        let exe = std::env::current_exe().ok();
        search(arg, env.as_deref(), exe.as_deref().and_then(Path::parent)).map(|(rom, _)| rom)
    }
}

/// The workspace's `raw/` ROM path (it exists only in a development checkout).
#[must_use]
pub fn raw_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../raw")
        .join(RAW_NAME)
}

fn read(path: &Path) -> Result<Rom, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Rom::from_ines(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// The ROM the build embeds and the tests read: `ROTJ_ROM`, else the workspace's `raw/`,
/// else `None`. A named or present file that does not load is an error naming it.
pub fn locate() -> Result<Option<(PathBuf, Rom)>, String> {
    let env = std::env::var_os("ROTJ_ROM").filter(|env| !env.is_empty());
    find(env.as_deref(), &raw_path())
}

/// [`locate`] with the environment passed in.
fn find(env: Option<&OsStr>, raw: &Path) -> Result<Option<(PathBuf, Rom)>, String> {
    let path = match env {
        Some(env) if Path::new(env).is_absolute() => PathBuf::from(env),
        Some(env) => {
            return Err(format!(
                "ROTJ_ROM must be an absolute path, not {}",
                Path::new(env).display()
            ));
        }
        None if raw.exists() => raw.to_path_buf(),
        None => return Ok(None),
    };
    let rom = read(&path)?;
    Ok(Some((path, rom)))
}

/// The search behind [`Rom::load`], with the environment passed in. An argument or
/// `ROTJ_ROM` that does not load is an error; a bad file beside the exe is noted and the
/// search goes on.
fn search(
    arg: Option<&Path>,
    env: Option<&OsStr>,
    exe_dir: Option<&Path>,
) -> Result<(Rom, PathBuf), String> {
    if let Some(path) = arg.or(env.map(Path::new)) {
        return read(path).map(|rom| (rom, path.to_path_buf()));
    }
    let dir = exe_dir.ok_or("Looked nowhere: the exe's folder is unknown")?;
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("Looked in:\n  {}: {e}", dir.display()))?;
    let mut beside: Vec<PathBuf> = entries
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("nes"))
        })
        .collect();
    beside.sort();
    let mut looked = Vec::new();
    for path in beside {
        match read(&path) {
            Ok(rom) => return Ok((rom, path)),
            Err(error) => looked.push(error),
        }
    }
    if looked.is_empty() {
        looked.push(format!("{}: no .nes file", dir.display()));
    }
    Err(format!("Looked in:\n  {}", looked.join("\n  ")))
}

/// The ROM for a test: `ROTJ_ROM`, else the workspace's `raw/`. Without one it prints the
/// skip line and returns `None`, or panics when `ROTJ_REQUIRE_ROM` is set; a ROM that
/// fails the check always panics.
#[must_use]
pub fn for_test() -> Option<&'static Rom> {
    static ROM: OnceLock<Option<Rom>> = OnceLock::new();
    let rom = ROM.get_or_init(|| {
        locate()
            .unwrap_or_else(|e| panic!("{e}"))
            .map(|(_, rom)| rom)
    });
    if rom.is_none() {
        assert!(
            std::env::var_os("ROTJ_REQUIRE_ROM").is_none(),
            "ROM required (ROTJ_REQUIRE_ROM) but not found: set ROTJ_ROM or put it in raw/"
        );
        println!("skipped: no ROM (set ROTJ_ROM or put it in raw/)");
    }
    rom.as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mapper-69 image of the right sizes that is not the game.
    fn wrong_game() -> Vec<u8> {
        let mut image = vec![0; 16 + 0x20000 + 0x40000];
        image[..8].copy_from_slice(b"NES\x1a\x08\x20\x50\x40");
        image
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rotj-rom-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn real_image() -> Option<Vec<u8>> {
        for_test()?;
        let (path, _) = locate().ok().flatten()?;
        Some(std::fs::read(path).expect("ROM"))
    }

    #[test]
    fn rejects_other_mappers() {
        let mut rom = vec![0; 16 + 0x4000];
        rom[..8].copy_from_slice(b"NES\x1a\x01\x00\x00\x00");
        assert!(Rom::parse_ines(&rom).is_err());
        rom[6] = 0x50;
        rom[7] = 0x40;
        let rom = Rom::parse_ines(&rom).expect("mapper 69");
        assert_eq!((rom.prg.len(), rom.chr.len()), (0x4000, 0));
    }

    #[test]
    fn rejects_a_truncated_image() {
        let image = wrong_game();
        assert!(Rom::parse_ines(&image[..16 + 0x20000 + 100]).is_err());
        assert!(Rom::parse_ines(&image[..10]).is_err());
    }

    #[test]
    fn rejects_the_wrong_game() {
        let error = Rom::from_ines(&wrong_game()).expect_err("zeros are not the game");
        assert!(error.contains(PRG_CHR_MD5), "{error}");
    }

    #[test]
    fn accepts_another_header() {
        let Some(mut image) = real_image() else {
            return;
        };
        let original = Rom::from_ines(&image).expect("the ROM");
        image[7] |= 0x08;
        image[8..16].copy_from_slice(&[0, 0, 0x07, 0, 0, 0, 0, 0x01]);
        assert_eq!(Rom::from_ines(&image), Ok(original));
    }

    #[test]
    fn a_missing_rom_env_is_an_error() {
        let dir = scratch("env");
        let missing = dir.join("missing.nes");
        let raw = dir.join("raw.nes");
        std::fs::write(&raw, wrong_game()).expect("write");
        let error = find(Some(missing.as_os_str()), &raw).expect_err("missing ROTJ_ROM");
        assert!(error.contains("missing.nes"), "{error}");
    }

    #[test]
    fn a_relative_rom_env_is_an_error() {
        let error = find(Some(OsStr::new("raw/rom.nes")), Path::new("/nonexistent"))
            .expect_err("relative ROTJ_ROM");
        assert!(error.contains("absolute"), "{error}");
    }

    #[test]
    fn a_wrong_rom_names_its_file() {
        let bad = scratch("wrong").join("bad.nes");
        std::fs::write(&bad, wrong_game()).expect("write");
        let error = find(None, &bad).expect_err("wrong game");
        assert!(
            error.contains("bad.nes") && error.contains("MD5"),
            "{error}"
        );
    }

    #[test]
    fn no_rom_is_none() {
        let missing = scratch("none").join("missing.nes");
        assert_eq!(find(None, &missing), Ok(None));
    }

    #[test]
    fn rom_env_wins_over_raw() {
        let Some(image) = real_image() else {
            return;
        };
        let dir = scratch("order");
        let (env, raw) = (dir.join("env.nes"), dir.join("raw.nes"));
        std::fs::write(&env, &image).expect("write");
        std::fs::write(&raw, wrong_game()).expect("write");
        let (path, _) = find(Some(env.as_os_str()), &raw)
            .expect("loads")
            .expect("found");
        assert_eq!(path, env);
    }

    fn exe_dir(name: &str) -> PathBuf {
        let dir = scratch(name).join("exe");
        std::fs::create_dir_all(&dir).expect("exe dir");
        dir
    }

    #[test]
    fn a_missing_arg_or_runtime_rom_env_is_an_error() {
        let exe = exe_dir("named");
        std::fs::write(exe.join("a.nes"), real_image().unwrap_or_else(wrong_game)).expect("write");
        let missing = exe.join("missing.nes");
        for error in [
            search(Some(&missing), None, Some(&exe)),
            search(None, Some(missing.as_os_str()), Some(&exe)),
        ] {
            let error = error.expect_err("a missing named ROM");
            assert!(error.contains("missing.nes"), "{error}");
        }
    }

    #[test]
    fn a_wrong_arg_names_its_file() {
        let bad = scratch("wrong-arg").join("bad.nes");
        std::fs::write(&bad, wrong_game()).expect("write");
        let error = search(Some(&bad), None, None).expect_err("wrong game");
        assert!(
            error.contains("bad.nes") && error.contains("MD5"),
            "{error}"
        );
    }

    #[test]
    fn an_unreadable_exe_folder_says_so() {
        let gone = scratch("gone").join("exe");
        let error = search(None, None, Some(&gone)).expect_err("no folder");
        assert!(error.contains(&gone.display().to_string()), "{error}");
    }

    #[test]
    fn no_rom_beside_the_exe_says_where_it_looked() {
        let exe = exe_dir("empty");
        let error = search(None, None, Some(&exe)).expect_err("nothing beside the exe");
        assert!(
            error.contains(&exe.display().to_string()) && error.contains("no .nes file"),
            "{error}"
        );
    }

    #[test]
    fn a_bad_rom_beside_the_exe_falls_through() {
        let exe = exe_dir("beside");
        std::fs::write(exe.join("a.nes"), wrong_game()).expect("write");
        let error = search(None, None, Some(&exe)).expect_err("only a bad file");
        assert!(error.contains("a.nes") && error.contains("MD5"), "{error}");
        let Some(image) = real_image() else {
            return;
        };
        std::fs::write(exe.join("b.NES"), image).expect("write");
        let (_, path) = search(None, None, Some(&exe)).expect("the good file");
        assert_eq!(path, exe.join("b.NES"));
    }

    #[test]
    fn the_search_order_holds() {
        let Some(image) = real_image() else {
            return;
        };
        let exe = exe_dir("search-order");
        let (arg, env) = (exe.join("../arg.nes"), exe.join("../env.nes"));
        for path in [&arg, &env, &exe.join("a.nes")] {
            std::fs::write(path, &image).expect("write");
        }
        let found = |arg: Option<&Path>, env: Option<&Path>| {
            search(arg, env.map(Path::as_os_str), Some(&exe))
                .expect("found")
                .1
        };
        assert_eq!(found(Some(&arg), Some(&env)), arg);
        assert_eq!(found(None, Some(&env)), env);
        assert_eq!(found(None, None), exe.join("a.nes"));
    }
}
