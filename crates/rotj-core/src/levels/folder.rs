//! A pack's folder on disk: read into [`PackFiles`], and saved from them.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::PackFiles;

/// The extensions of the files a pack's import reads.
const PACK_EXTENSIONS: [&str; 3] = ["json", "tmj", "tsj"];
/// Maximum directory nesting when walking a pack folder (guards against symlink chains).
const MAX_DEPTH: usize = 32;

/// Why a level pack folder could not be read.
#[derive(Debug, Error)]
pub enum FolderError {
    /// The folder does not contain a `pack.json` file.
    #[error("no `pack.json`: not a level pack")]
    NoPackJson,
    /// An entry inside the folder has a non-UTF-8 file name.
    #[error("{dir}: a file name is not UTF-8")]
    NonUtf8Name {
        /// Relative directory path.
        dir: String,
    },
    /// Directory traversal hit a symlink loop.
    #[error("{path}: symlink loop")]
    SymlinkCycle {
        /// Relative directory path where the loop was detected.
        path: String,
    },
    /// Directory nesting exceeded [`MAX_DEPTH`].
    #[error("{path}: directory nesting is too deep")]
    TooDeep {
        /// Relative directory path where the limit was reached.
        path: String,
    },
    /// An underlying filesystem error.
    #[error("{path}: {source}")]
    Io {
        /// Relative file or directory path.
        path: String,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
}

/// The pack in `dir`: every `.json`, `.tmj` and `.tsj` file under it, keyed by its path
/// relative to `dir` with `/` separators. A folder without a `pack.json` is refused first.
///
/// # Errors
///
/// No `pack.json`, a symlink loop, a non-UTF-8 name, or a file or folder that does not read,
/// named relative to `dir`.
pub fn read(dir: &Path) -> Result<PackFiles, FolderError> {
    let mut visited = BTreeSet::new();
    let root = fs::canonicalize(dir).map_err(|source| FolderError::Io {
        path: ".".into(),
        source,
    })?;
    visited.insert(root);
    let mut files = PackFiles::new();
    read_under(dir, "", 0, &mut visited, &mut files)?;
    if !files.contains_key("pack.json") {
        return Err(FolderError::NoPackJson);
    }
    Ok(files)
}

fn read_under(
    dir: &Path,
    prefix: &str,
    depth: usize,
    visited: &mut BTreeSet<PathBuf>,
    files: &mut PackFiles,
) -> Result<(), FolderError> {
    let shown = if prefix.is_empty() { "." } else { prefix };
    if depth > MAX_DEPTH {
        return Err(FolderError::TooDeep { path: shown.into() });
    }
    let entries = fs::read_dir(dir).map_err(|source| FolderError::Io {
        path: shown.into(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| FolderError::Io {
            path: shown.into(),
            source,
        })?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| FolderError::NonUtf8Name { dir: shown.into() })?;
        let key = if prefix.is_empty() && name.eq_ignore_ascii_case("pack.json") {
            "pack.json".to_owned()
        } else if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}/{name}")
        };
        if path.is_dir() {
            let canonical = fs::canonicalize(&path).map_err(|source| FolderError::Io {
                path: key.clone(),
                source,
            })?;
            if !visited.insert(canonical.clone()) {
                return Err(FolderError::SymlinkCycle { path: key });
            }
            let result = read_under(&path, &key, depth + 1, visited, files);
            visited.remove(&canonical);
            result?;
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                PACK_EXTENSIONS
                    .iter()
                    .any(|&allowed| extension.eq_ignore_ascii_case(allowed))
            })
        {
            let text = fs::read_to_string(&path).map_err(|source| FolderError::Io {
                path: key.clone(),
                source,
            })?;
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
        let _ = fs::remove_dir_all(&dir);
        let files = PackFiles::from([
            ("pack.json".into(), "{}".into()),
            ("rooms/1-1-1.tmj".into(), "{\"a\": 1}".into()),
            ("custom.tsj".into(), "[]".into()),
        ]);
        save(&dir, &files).expect("saves");
        fs::write(dir.join("rooms/notes.txt"), "not a pack file").expect("writes");
        assert_eq!(read(&dir).expect("reads"), files);
        fs::remove_file(dir.join("pack.json")).expect("removes");
        assert!(matches!(read(&dir), Err(FolderError::NoPackJson)));
        fs::remove_dir_all(&dir).expect("cleans up");
    }

    #[test]
    fn uppercase_pack_json_and_extensions_are_read() {
        let dir = std::env::temp_dir().join(format!("rotj-pack-case-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("rooms")).expect("mkdir");
        fs::write(dir.join("PACK.JSON"), "{}").expect("write");
        fs::write(dir.join("rooms/1-1-1.TMJ"), "{\"a\": 1}").expect("write");
        let files = read(&dir).expect("reads case-insensitively");
        assert_eq!(files.get("pack.json").map(String::as_str), Some("{}"));
        assert_eq!(
            files.get("rooms/1-1-1.TMJ").map(String::as_str),
            Some("{\"a\": 1}")
        );
        fs::remove_dir_all(&dir).expect("cleans up");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_loop_is_refused_without_overflowing_the_stack() {
        let dir = std::env::temp_dir().join(format!("rotj-pack-loop-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        fs::write(dir.join("pack.json"), "{}").expect("write");
        std::os::unix::fs::symlink(&dir, dir.join("again")).expect("symlink");
        let result = read(&dir);
        fs::remove_dir_all(&dir).expect("cleans up");
        assert!(matches!(result, Err(FolderError::SymlinkCycle { .. })));
    }
}
