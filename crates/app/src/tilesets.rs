//! The tilesets a level pack's maps name, written beside the settings file for Tiled.

use std::fs;
use std::io::{self, Cursor};
use std::path::Path;

use rotj_core::levels::{TILESET_PX, tilesets};

/// Writes each tileset's `.tsj` and `.png` into `dir/tilesets/` where the file is missing or
/// differs from what the ROM makes now.
pub fn write(dir: &Path, prg: &[u8], chr: &[u8]) -> io::Result<()> {
    let folder = dir.join("tilesets");
    fs::create_dir_all(&folder)?;
    let tilesets =
        tilesets(prg, chr).ok_or_else(|| io::Error::other("the ROM's levels do not import"))?;
    for tileset in tilesets {
        let picture = image::RgbaImage::from_raw(TILESET_PX, TILESET_PX, tileset.picture)
            .expect("a picture is its size");
        let mut png = Cursor::new(Vec::new());
        picture
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(io::Error::other)?;
        update(
            &folder.join(format!("{}.tsj", tileset.name)),
            tileset.text.as_bytes(),
        )?;
        update(&folder.join(format!("{}.png", tileset.name)), png.get_ref())?;
    }
    Ok(())
}

fn update(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if fs::read(path).is_ok_and(|held| held == bytes) {
        return Ok(());
    }
    fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;

    #[test]
    fn writes_missing_and_stale_files_and_leaves_equal_ones() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("rotj-tilesets-app-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        write(&dir, &rom.prg, &rom.chr).expect("writes");
        let folder = dir.join("tilesets");
        assert_eq!(fs::read_dir(&folder).expect("the folder").count(), 54);
        let (stale, equal) = (folder.join("area-01.tsj"), folder.join("area-01.png"));
        let fresh = fs::read(&stale).expect("area-01.tsj");
        fs::write(&stale, "stale").expect("write");
        let long_ago = SystemTime::UNIX_EPOCH + Duration::from_hours(24);
        fs::File::options()
            .write(true)
            .open(&equal)
            .and_then(|file| file.set_modified(long_ago))
            .expect("set the time");
        write(&dir, &rom.prg, &rom.chr).expect("writes");
        let modified = fs::metadata(&equal).and_then(|meta| meta.modified());
        let rewritten = fs::read(&stale).expect("area-01.tsj");
        fs::remove_dir_all(&dir).expect("removes the folder");
        assert!(rewritten == fresh, "the stale file is rewritten");
        assert_eq!(
            modified.expect("a time"),
            long_ago,
            "the equal file is left"
        );
    }

    #[test]
    fn a_folder_it_cannot_create_is_an_error_not_a_panic() {
        let dir = std::env::temp_dir().join(format!("rotj-tilesets-file-{}", std::process::id()));
        fs::write(&dir, "").expect("write");
        let result = write(&dir, &[], &[]);
        fs::remove_file(&dir).expect("removes the file");
        assert!(result.is_err());
    }
}
