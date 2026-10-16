//! The user's own Batman: Return of the Joker (USA) ROM: where it is found, how it is
//! checked, and its PRG and CHR. Nothing derived from the ROM is distributed with the
//! port; the app and every test read it through this crate.

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

/// Where [`search`] found the ROM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Arg,
    Env,
    Cache,
    BesideExe,
    Raw,
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

    /// The app's ROM: `arg` (a file dropped on the executable), `ROTJ_ROM`, the cached
    /// copy, a `.nes` beside the executable, then (debug builds) the workspace's `raw/`.
    /// A ROM found through `arg` or beside the executable is copied to the cache.
    pub fn load(arg: Option<&Path>) -> Result<Rom, String> {
        let env = std::env::var_os("ROTJ_ROM").filter(|env| !env.is_empty());
        let cache = cache_dir();
        let exe = std::env::current_exe().ok();
        let raw = cfg!(debug_assertions).then(raw_path);
        let (rom, source, path) = search(
            arg,
            env.as_deref(),
            cache.as_deref(),
            exe.as_deref().and_then(Path::parent),
            raw.as_deref(),
        )?;
        if matches!(source, Source::Arg | Source::BesideExe)
            && let Some(cache) = &cache
        {
            remember(&path, cache);
        }
        Ok(rom)
    }
}

/// The workspace's `raw/` ROM path (it exists only in a development checkout).
fn raw_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../raw")
        .join(RAW_NAME)
}

fn cache_dir() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
    } else if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME") {
        PathBuf::from(xdg)
    } else {
        PathBuf::from(std::env::var_os("HOME")?).join(".cache")
    };
    Some(base.join("batman-rotj"))
}

/// Copies the ROM into the cache through a temporary file, so a crash cannot leave half a
/// ROM behind; a cache that cannot be written only costs the next launch the search.
fn remember(path: &Path, cache: &Path) {
    let tmp = cache.join(format!("rom.nes.{}.tmp", std::process::id()));
    let copied = std::fs::create_dir_all(cache)
        .and_then(|()| std::fs::copy(path, &tmp))
        .and_then(|_| std::fs::rename(&tmp, cache.join("rom.nes")));
    if copied.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
}

fn read(path: &Path) -> Result<Rom, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Rom::from_ines(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// The search behind [`Rom::load`], with the environment passed in. An argument or
/// `ROTJ_ROM` that does not load is an error; a bad cached or beside-exe file is noted
/// and the search goes on, so a corrupt cache cannot hide a good ROM.
fn search(
    arg: Option<&Path>,
    env: Option<&OsStr>,
    cache: Option<&Path>,
    exe_dir: Option<&Path>,
    raw: Option<&Path>,
) -> Result<(Rom, Source, PathBuf), String> {
    for (path, source) in [(arg, Source::Arg), (env.map(Path::new), Source::Env)] {
        if let Some(path) = path {
            return read(path).map(|rom| (rom, source, path.to_path_buf()));
        }
    }
    let mut looked = Vec::new();
    let mut candidates: Vec<(PathBuf, Source)> = Vec::new();
    if let Some(cache) = cache {
        candidates.push((cache.join("rom.nes"), Source::Cache));
    }
    if let Some(dir) = exe_dir {
        let mut beside: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(|entry| Some(entry.ok()?.path()))
            .filter(|path| {
                path.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("nes"))
            })
            .collect();
        beside.sort();
        if beside.is_empty() {
            looked.push(format!("{}: no .nes file", dir.display()));
        }
        candidates.extend(beside.into_iter().map(|path| (path, Source::BesideExe)));
    }
    if let Some(raw) = raw {
        candidates.push((raw.to_path_buf(), Source::Raw));
    }
    for (path, source) in candidates {
        if !path.exists() {
            looked.push(format!("{}: not found", path.display()));
            continue;
        }
        match read(&path) {
            Ok(rom) => return Ok((rom, source, path)),
            Err(error) => looked.push(error),
        }
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
        let env = std::env::var_os("ROTJ_ROM").filter(|env| !env.is_empty());
        let raw = raw_path();
        let path = match &env {
            Some(env) => PathBuf::from(env),
            None if raw.exists() => raw,
            None => return None,
        };
        Some(read(&path).unwrap_or_else(|e| panic!("{e}")))
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
        let path = std::env::var_os("ROTJ_ROM")
            .filter(|env| !env.is_empty())
            .map_or_else(raw_path, PathBuf::from);
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
        let raw = real_image().map(|image| {
            let raw = dir.join("raw.nes");
            std::fs::write(&raw, image).expect("write");
            raw
        });
        let error = search(None, Some(missing.as_os_str()), None, None, raw.as_deref())
            .expect_err("a missing ROTJ_ROM is an error");
        assert!(error.contains("missing.nes"), "{error}");
    }

    #[test]
    fn a_missing_arg_is_an_error() {
        let missing = scratch("arg").join("missing.nes");
        let error = search(Some(&missing), None, None, None, None).expect_err("missing arg");
        assert!(error.contains("missing.nes"), "{error}");
    }

    #[test]
    fn a_wrong_rom_names_its_file() {
        let bad = scratch("wrong").join("bad.nes");
        std::fs::write(&bad, wrong_game()).expect("write");
        let error = search(Some(&bad), None, None, None, None).expect_err("wrong game");
        assert!(
            error.contains("bad.nes") && error.contains("MD5"),
            "{error}"
        );
    }

    #[test]
    fn a_bad_cached_rom_falls_through() {
        let dir = scratch("cache");
        let (cache, exe) = (dir.join("cache"), dir.join("exe"));
        std::fs::create_dir_all(&cache).expect("cache");
        std::fs::create_dir_all(&exe).expect("exe");
        std::fs::write(cache.join("rom.nes"), wrong_game()).expect("write");
        let error =
            search(None, None, Some(&cache), Some(&exe), None).expect_err("only a bad cache");
        assert!(
            error.contains("rom.nes") && error.contains("MD5"),
            "{error}"
        );
        let Some(image) = real_image() else {
            return;
        };
        std::fs::write(exe.join("Batman.NES"), image).expect("write");
        let (_, source, _) = search(None, None, Some(&cache), Some(&exe), None).expect("beside");
        assert_eq!(source, Source::BesideExe);
    }

    #[test]
    fn the_search_order_holds() {
        let Some(image) = real_image() else {
            return;
        };
        let dir = scratch("order");
        let (cache, exe) = (dir.join("cache"), dir.join("exe"));
        std::fs::create_dir_all(&cache).expect("cache");
        std::fs::create_dir_all(&exe).expect("exe");
        let (arg, env, raw) = (
            dir.join("arg.nes"),
            dir.join("env.nes"),
            dir.join("raw.nes"),
        );
        for path in [&arg, &env, &raw, &cache.join("rom.nes"), &exe.join("a.nes")] {
            std::fs::write(path, &image).expect("write");
        }
        let source = |arg: Option<&Path>, env: Option<&Path>, cache: Option<&Path>, exe| {
            search(arg, env.map(Path::as_os_str), cache, exe, Some(&raw))
                .expect("found")
                .1
        };
        assert_eq!(
            source(Some(&arg), Some(&env), Some(&cache), Some(&exe)),
            Source::Arg
        );
        assert_eq!(
            source(None, Some(&env), Some(&cache), Some(&exe)),
            Source::Env
        );
        assert_eq!(source(None, None, Some(&cache), Some(&exe)), Source::Cache);
        assert_eq!(source(None, None, None, Some(&exe)), Source::BesideExe);
        assert_eq!(source(None, None, None, None), Source::Raw);
    }

    #[test]
    fn remember_replaces_a_bad_cache() {
        let Some(image) = real_image() else {
            return;
        };
        let dir = scratch("remember");
        let cache = dir.join("cache");
        std::fs::create_dir_all(&cache).expect("cache");
        std::fs::write(cache.join("rom.nes"), wrong_game()).expect("write");
        let source = dir.join("good.nes");
        std::fs::write(&source, &image).expect("write");
        remember(&source, &cache);
        assert_eq!(std::fs::read(cache.join("rom.nes")).expect("cached"), image);
        let leftovers = std::fs::read_dir(&cache).expect("cache").count();
        assert_eq!(leftovers, 1, "only rom.nes, no temporary file");
    }
}
