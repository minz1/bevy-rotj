//! The tilesets a level pack's maps name, written beside the settings file for Tiled.

use std::fs;
use std::io::{self, Cursor};
use std::path::Path;

use bevy::log::warn;
use rotj_core::levels::{TILESET_PX, custom_picture, tilesets};

/// Writes each tileset's `.tsj` and `.png` into `dir/tilesets/`, and each pack's
/// `dir/packs/<pack>/custom.tsj` picture into `dir/tilesets/packs/<pack>/custom.png`, where
/// the file is missing or differs from what the ROM makes now; a pack whose picture fails is
/// logged and skipped.
pub fn write(dir: &Path, prg: &[u8], chr: &[u8]) -> io::Result<()> {
    let folder = dir.join("tilesets");
    fs::create_dir_all(&folder)?;
    let tilesets =
        tilesets(prg, chr).ok_or_else(|| io::Error::other("the ROM's levels do not import"))?;
    for tileset in tilesets {
        update(
            &folder.join(format!("{}.tsj", tileset.name)),
            tileset.text.as_bytes(),
        )?;
        update(
            &folder.join(format!("{}.png", tileset.name)),
            &png(tileset.picture)?,
        )?;
    }
    let Ok(packs) = fs::read_dir(dir.join("packs")) else {
        return Ok(());
    };
    for pack in packs.flatten() {
        let custom = pack.path().join("custom.tsj");
        let Ok(text) = fs::read_to_string(&custom) else {
            continue;
        };
        let out = folder.join("packs").join(pack.file_name());
        let written = custom_picture(prg, chr, &text)
            .map_err(io::Error::other)
            .and_then(png)
            .and_then(|png| {
                fs::create_dir_all(&out)?;
                update(&out.join("custom.png"), &png)
            });
        if let Err(error) = written {
            warn!("{}: {error}", custom.display());
        }
    }
    Ok(())
}

/// `picture`, RGBA and [`TILESET_PX`] wide, as a PNG.
fn png(picture: Vec<u8>) -> io::Result<Vec<u8>> {
    let height = u32::try_from(picture.len() / 4).expect("a small picture") / TILESET_PX;
    let picture =
        image::RgbaImage::from_raw(TILESET_PX, height, picture).expect("a picture is its size");
    let mut png = Cursor::new(Vec::new());
    picture
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(io::Error::other)?;
    Ok(png.into_inner())
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
        let first = write(&dir, &rom.prg, &rom.chr);
        let folder = dir.join("tilesets");
        let count = fs::read_dir(&folder).map(Iterator::count);
        let (stale, equal) = (folder.join("area-01.tsj"), folder.join("area-01.png"));
        let fresh = fs::read(&stale);
        let spoiled = fs::write(&stale, "stale");
        let long_ago = SystemTime::UNIX_EPOCH + Duration::from_hours(24);
        let aged = fs::File::options()
            .write(true)
            .open(&equal)
            .and_then(|file| file.set_modified(long_ago));
        let second = write(&dir, &rom.prg, &rom.chr);
        let modified = fs::metadata(&equal).and_then(|meta| meta.modified());
        let rewritten = fs::read(&stale);
        fs::remove_dir_all(&dir).expect("removes the folder");
        first.and(spoiled).and(aged).and(second).expect("writes");
        assert_eq!(count.expect("the folder"), 54);
        assert!(
            rewritten.expect("area-01.tsj") == fresh.expect("area-01.tsj"),
            "the stale file is rewritten"
        );
        assert_eq!(
            modified.expect("a time"),
            long_ago,
            "the equal file is left"
        );
    }

    #[test]
    fn draws_a_packs_custom_png_and_survives_a_bad_one() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("rotj-tilesets-custom-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let custom = |columns: u32| {
            format!(
                r#"{{"columns": {columns}, "tilecount": 17, "tiles": [{{"id": 16, "properties": [
                    {{"name": "palette", "type": "int", "value": 0}},
                    {{"name": "surface", "type": "string", "value": "solid"}},
                    {{"name": "tiles", "type": "list", "value": [
                        {{"type": "string", "value": "2c:05"}}, {{"type": "string", "value": "2c:06"}},
                        {{"type": "string", "value": "02:3f"}}, {{"type": "string", "value": "03:00"}}]}}]}}]}}"#
            )
        };
        for (pack, columns) in [("a", 16), ("b", 8)] {
            fs::create_dir_all(dir.join("packs").join(pack)).expect("a pack folder");
            fs::write(
                dir.join("packs").join(pack).join("custom.tsj"),
                custom(columns),
            )
            .expect("custom.tsj");
        }
        let written = write(&dir, &rom.prg, &rom.chr);
        let folder = dir.join("tilesets").join("packs");
        let good =
            image::open(folder.join("a").join("custom.png")).map(|png| (png.width(), png.height()));
        let bad = folder.join("b").join("custom.png").exists();
        fs::remove_dir_all(&dir).expect("removes the folder");
        written.expect("writes");
        assert_eq!(good.expect("a's custom.png"), (256, 32));
        assert!(!bad, "b's bad custom.tsj draws nothing");
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
