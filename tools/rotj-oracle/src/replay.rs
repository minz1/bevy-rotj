//! Helpers for tests that replay the game: the ROM gate and recorded input.

use std::path::{Path, PathBuf};

use crate::cart::load_rom;
use crate::machine::Machine;

/// A powered-on machine, or `None` (skip) without the ROM unless `ROTJ_REQUIRE_ROM` is set.
#[must_use]
pub fn power_on() -> Option<Machine> {
    Some(Machine::power_on(load_rom()?))
}

/// The pad of each frame from a `dump.lua` `inputs.csv` (`frame,pad` rows, row N = frame N).
#[must_use]
pub fn read_inputs(path: &Path) -> Vec<u8> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
    text.lines()
        .skip(1)
        .enumerate()
        .map(|(row, line)| {
            let (frame, pad) = line.split_once(',').expect("frame,pad");
            assert_eq!(frame.parse::<usize>().expect("frame"), row, "row {row}");
            pad.parse().expect("pad")
        })
        .collect()
}

/// A file in this crate's `tests/data`.
#[must_use]
pub fn data_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

/// `tools/emu/dump.lua` `INPUT=physics`, fceux's `tools/emu/stage` run (frame 700 is the stage 1-1 level capture).
#[must_use]
pub fn physics_input(frame: usize) -> u8 {
    let mut pad = 0;
    if (300..=306).contains(&frame) {
        pad |= 0x08;
    }
    if (660..=780).contains(&frame) || frame >= 900 {
        pad |= 0x80;
    }
    if frame == 800 || frame == 905 {
        pad |= 0x01;
    }
    pad
}

/// run1's input, `dump.lua` `INPUT=plan`, as the pad shifts out (A bit 0 .. Right bit 7).
#[must_use]
pub fn plan_input(frame: usize) -> u8 {
    let mut pad = 0;
    if (100..=900).contains(&frame) && frame % 20 < 3 {
        pad |= 0x08;
    }
    if frame > 900 {
        pad |= 0x80;
        if frame % 60 < 3 {
            pad |= 0x01;
        }
        if frame % 137 < 3 {
            pad |= 0x02;
        }
    }
    pad
}
