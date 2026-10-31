//! A pack's folder on disk: read into [`PackFiles`], and saved from them.

use std::fs;
use std::io;
use std::path::Path;

use super::PackFiles;

/// The extensions of the files a pack's import reads.
const PACK_EXTENSIONS: [&str; 3] = ["json", "tmj", "tsj"];

/// The pack in `dir`: every `.json`, `.tmj` and `.tsj` file under it, keyed by its path
/// relative to `dir` with `/` separators. A folder without a `pack.json` is refused first.
///
/// # Errors
///
/// No `pack.json`, or a file or folder that does not read, named relative to `dir`.
pub fn read(dir: &Path) -> Result<PackFiles, String> {
    if !dir.join("pack.json").is_file() {
        return Err("no `pack.json`: not a level pack".into());
    }
    let mut files = PackFiles::new();
    read_under(dir, "", &mut files)?;
    Ok(files)
}

fn read_under(dir: &Path, prefix: &str, files: &mut PackFiles) -> Result<(), String> {
    let shown = if prefix.is_empty() { "." } else { prefix };
    let entries = fs::read_dir(dir).map_err(|error| format!("{shown}: {error}"))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("{shown}: {error}"))?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| format!("{shown}: a file name is not UTF-8"))?;
        let key = if prefix.is_empty() && name.eq_ignore_ascii_case("pack.json") {
            "pack.json".to_owned()
        } else if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}/{name}")
        };
        if path.is_dir() {
            read_under(&path, &key, files)?;
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| PACK_EXTENSIONS.contains(&extension))
        {
            let text = fs::read_to_string(&path).map_err(|error| format!("{key}: {error}"))?;
            files.insert(key, text);
        }
    }
    Ok(())
}

/// Writes `files` under `dir`, making the folders they need.
///
/// # Errors
///
/// A folder or file that does not write.
pub fn save(dir: &Path, files: &PackFiles) -> io::Result<()> {
    for (path, text) in files {
        let file = dir.join(path);
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&file, text)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_pack_reads_back_and_a_folder_without_pack_json_is_refused() {
        let dir = std::env::temp_dir().join(format!("rotj-pack-{}", std::process::id()));
        let files = PackFiles::from([
            ("pack.json".into(), "{}".into()),
            ("rooms/1-1-1.tmj".into(), "{\"a\": 1}".into()),
            ("custom.tsj".into(), "[]".into()),
        ]);
        save(&dir, &files).expect("saves");
        fs::write(dir.join("rooms/notes.txt"), "not a pack file").expect("writes");
        assert_eq!(read(&dir), Ok(files));
        fs::remove_file(dir.join("pack.json")).expect("removes");
        assert_eq!(read(&dir), Err("no `pack.json`: not a level pack".into()));
        fs::remove_dir_all(&dir).expect("cleans up");
    }
}
