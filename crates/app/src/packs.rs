//! Session-only level-pack discovery under `<exe_dir>/packs/` for the in-game `LOAD PACK` menu.

use std::path::{Path, PathBuf};

/// One discovered level-pack directory under `<exe_dir>/packs/`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PackEntry {
    /// Display name sanitized to the game's 8x16 NES font glyphs.
    pub label: String,
    /// Full path to the pack's folder containing `pack.json`.
    pub dir: PathBuf,
}

/// Converts a folder name into an uppercase string containing only glyphs supported by
/// [`crate::menu::glyph`], collapsed and truncated to at most `max_len` characters.
#[must_use]
pub(crate) fn sanitize_label(name: &str, max_len: usize) -> String {
    let mut out = String::new();
    let mut prev_space = true;
    for ch in name.chars() {
        let mapped = match ch {
            '_' => ' ',
            other => other.to_ascii_uppercase(),
        };
        let allowed = matches!(mapped, 'A'..='Z' | '0'..='9' | '-' | '.' | '?' | ' ');
        let c = if allowed { mapped } else { ' ' };
        if c == ' ' {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    let trimmed = out.trim();
    let truncated: String = trimmed.chars().take(max_len).collect();
    let truncated = truncated.trim_end();
    if truncated.is_empty() {
        "PACK".chars().take(max_len).collect()
    } else {
        truncated.to_string()
    }
}

/// True if `dir` contains an immediate file named `pack.json` (case-insensitive).
fn has_pack_json(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry.file_name().to_str().is_some_and(|name| {
            name.eq_ignore_ascii_case("pack.json") && entry.file_type().is_ok_and(|ft| ft.is_file())
        })
    })
}

/// Scans `<base_dir>/packs/` for immediate subdirectories containing `pack.json`, sorted
/// alphabetically by display label. Missing or unreadable folders return an empty list.
#[must_use]
pub(crate) fn discover(base_dir: &Path, max_label_len: usize) -> Vec<PackEntry> {
    let packs_dir = base_dir.join("packs");
    let Ok(entries) = std::fs::read_dir(packs_dir) else {
        return Vec::new();
    };
    let mut found: Vec<(String, String, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || !has_pack_json(&path) {
            continue;
        }
        let raw_name = entry.file_name().to_string_lossy().into_owned();
        let label = sanitize_label(&raw_name, max_label_len);
        found.push((label, raw_name, path));
    }
    found.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    found
        .into_iter()
        .map(|(label, _, dir)| PackEntry { label, dir })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_label_maps_to_menu_glyphs_and_truncates() {
        assert_eq!(sanitize_label("my_cool_pack!", 23), "MY COOL PACK");
        assert_eq!(sanitize_label("stage-1.2_remix", 23), "STAGE-1.2 REMIX");
        assert_eq!(
            sanitize_label("this_is_a_very_long_pack_folder_name", 12),
            "THIS IS A VE"
        );
        assert_eq!(sanitize_label("___***___", 12), "PACK");
        for sample in ["my_cool_pack!", "stage-1.2_remix", "___***___", "test (v2)"] {
            let label = sanitize_label(sample, 23);
            assert!(
                label.chars().all(|c| crate::menu::glyph(c).is_some()),
                "{label:?}"
            );
        }
    }

    #[test]
    fn discover_scans_immediate_subdirs_with_pack_json_sorted() {
        let root = std::env::temp_dir().join(format!("rotj-packs-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert!(discover(&root, 23).is_empty(), "missing folder is empty");

        let packs = root.join("packs");
        std::fs::create_dir_all(packs.join("zeta_pack")).expect("zeta");
        std::fs::write(packs.join("zeta_pack/pack.json"), "{}").expect("zeta json");
        std::fs::create_dir_all(packs.join("alpha-1")).expect("alpha");
        std::fs::write(packs.join("alpha-1/PACK.JSON"), "{}").expect("alpha json");
        std::fs::create_dir_all(packs.join("empty_folder")).expect("empty");
        std::fs::write(packs.join("stray_file.txt"), "hi").expect("stray");

        let entries = discover(&root, 23);
        let labels: Vec<&str> = entries.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["ALPHA-1", "ZETA PACK"]);
        assert_eq!(entries[0].dir, packs.join("alpha-1"));
        assert_eq!(entries[1].dir, packs.join("zeta_pack"));

        std::fs::remove_dir_all(&root).expect("cleanup");
    }
}
