//! Shared helpers for the ROM-gated capture tests: the committed digests of fceux's
//! captures (`tests/data/digests.txt`, written by `tools/emu/digest.py`).

use std::collections::BTreeMap;

/// The committed digests named `name`, by unit (a frame number, or `-` for a whole file).
#[must_use]
pub fn digests(name: &str) -> BTreeMap<String, u64> {
    rotj_core::digest::table(include_str!("../data/digests.txt"), name)
}

/// The one committed digest of a whole capture.
#[must_use]
pub fn digest(name: &str) -> u64 {
    digests(name)["-"]
}

/// Asserts a write stream has the committed per-frame digests `name` (fceux's writes
/// without `$4014`/`$4016`), naming the first frame that differs.
pub fn assert_write_digests(name: &str, actual: &[(u64, u16, u8)]) {
    let expected = digests(name);
    let actual = rotj_core::digest::write_frames(actual);
    let mut frames: Vec<u64> = expected
        .keys()
        .chain(actual.keys())
        .map(|frame| frame.parse().expect("a frame"))
        .collect();
    frames.sort_unstable();
    frames.dedup();
    assert!(!expected.is_empty(), "no digests named {name}");
    if let Some(frame) = frames
        .into_iter()
        .find(|frame| expected.get(&frame.to_string()) != actual.get(&frame.to_string()))
    {
        panic!(
            "{name}: frame {frame}'s writes differ from fceux's (regenerate the capture named in digests.txt for the byte diff)"
        );
    }
}
