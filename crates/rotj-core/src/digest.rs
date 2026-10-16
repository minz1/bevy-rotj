//! FNV-1a 64 digests: the tape's state hashes, and the committed fingerprints of fceux
//! captures (`digests.txt`, written by `tools/emu/digest.py`) that pin the oracle to the
//! real emulator without distributing the captures.

use std::collections::BTreeMap;

/// FNV-1a 64 (`std`'s hasher is not stable across Rust releases).
#[must_use]
pub fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3)
    })
}

/// The `unit -> digest` entries named `name` in a `digests.txt` table (`name unit digest`
/// lines, `#` comments).
#[must_use]
pub fn table(text: &str, name: &str) -> BTreeMap<String, u64> {
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            (fields.next()? == name).then_some(())?;
            let unit = fields.next()?.to_owned();
            let digest = u64::from_str_radix(fields.next()?, 16).ok()?;
            Some((unit, digest))
        })
        .collect()
}

/// A write stream's digest per frame: each `(frame, addr, value)` hashed as `addr`
/// (little-endian) then `value`, in order; frames without writes have no entry.
#[must_use]
pub fn write_frames(writes: &[(u64, u16, u8)]) -> BTreeMap<String, u64> {
    let mut bytes: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
    for &(frame, addr, value) in writes {
        let frame = bytes.entry(frame).or_default();
        frame.extend_from_slice(&addr.to_le_bytes());
        frame.push(value);
    }
    bytes
        .into_iter()
        .map(|(frame, bytes)| (frame.to_string(), fnv1a(&bytes)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_matches_the_reference_vectors() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn a_table_reads_its_entries() {
        let text = "# from tools/emu/x\nram 1 00000000000000ff\nram 2 af63dc4c8601ec8c\napu - 1\n";
        let ram = table(text, "ram");
        assert_eq!(ram.len(), 2);
        assert_eq!(ram["2"], 0xaf63_dc4c_8601_ec8c);
        assert_eq!(table(text, "apu")["-"], 1);
    }

    #[test]
    fn writes_digest_per_frame() {
        let frames = write_frames(&[(3, 0x4000, 0x10), (3, 0x4001, 0x08), (5, 0x4015, 0x0F)]);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames["3"], fnv1a(&[0x00, 0x40, 0x10, 0x01, 0x40, 0x08]));
        assert_eq!(frames["5"], fnv1a(&[0x15, 0x40, 0x0F]));
    }
}
