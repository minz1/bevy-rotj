//! The oracle against fceux's `tools/emu/run1` capture (by its digests): 3000 frames from
//! power-on.

pub mod common;

use std::collections::BTreeMap;

use rotj_core::digest::fnv1a;
use rotj_oracle::{FrameLog, Machine};
use rotj_oracle::{plan_input, power_on};

const FRAMES: usize = 3000;
/// `ramseq-0.bin` entry N is the RAM after N - 1 frames (entry 0 is power-on).
const LAG: usize = 1;
const RAM: usize = 0x800;

/// fceux's RAM digests, entry N after N - 1 frames.
fn ramseq() -> BTreeMap<String, u64> {
    common::digests("run1-ram")
}

/// Asserts the oracle's RAM is fceux's `ramseq-0.bin` entry `entry`.
fn assert_fceux_ram(seq: &BTreeMap<String, u64>, entry: usize, ram: &[u8], what: &str) {
    assert_eq!(
        fnv1a(&ram[..RAM]),
        seq[&entry.to_string()],
        "{what}: RAM differs from fceux's (regenerate tools/emu/run1, dump.lua INPUT=plan, for the byte diff)"
    );
}

/// Runs frames `1..=last` with run1's input; `check` sees the machine after each one.
fn replay(machine: &mut Machine, last: usize, mut check: impl FnMut(usize, &Machine, FrameLog)) {
    for frame in 1..=last {
        let log = machine.run_frame(plan_input(frame));
        check(frame, machine, log);
    }
}

#[test]
fn matches_fceux_ram_for_3000_frames() {
    let Some(mut machine) = power_on() else {
        return;
    };
    let seq = ramseq();
    assert_eq!(seq.len(), FRAMES + 1, "fceux's 3001 RAM states");
    assert_fceux_ram(&seq, 0, &machine.bus.ram, "power-on");
    assert_fceux_ram(
        &seq,
        1,
        &machine.bus.ram,
        "fceux's first frameadvance runs nothing",
    );
    replay(&mut machine, FRAMES - LAG, |frame, machine, _| {
        assert_fceux_ram(
            &seq,
            frame + LAG,
            &machine.bus.ram,
            &format!("frame {frame}"),
        );
    });
}

#[test]
fn irq_chain_runs_in_the_intro() {
    let Some(mut machine) = power_on() else {
        return;
    };
    let mut armed = Vec::new();
    replay(&mut machine, 75, |frame, machine, _| {
        let ram = &machine.bus.ram;
        if frame + LAG == 61 {
            assert_eq!(u16::from_le_bytes([ram[0x08], ram[0x09]]), 0x96A1);
        }
        if ram[0xD0] & 0x80 != 0 {
            armed.push(frame + LAG);
        }
    });
    assert_eq!(armed, [60, 62, 64, 66, 68, 70, 72, 74]);
}

#[test]
fn dmc_sample_keeps_rng_in_step() {
    let Some(mut machine) = power_on() else {
        return;
    };
    let seq = ramseq();
    let mut sample_started = false;
    replay(&mut machine, 840, |frame, machine, _| {
        sample_started |= machine.bus.dmc.remaining > 0;
        if frame >= 801 {
            assert_fceux_ram(
                &seq,
                frame + LAG,
                &machine.bus.ram,
                &format!("frame {frame}"),
            );
        }
    });
    assert!(sample_started);
}

#[test]
fn apu_writes_match_the_capture() {
    let Some(mut machine) = power_on() else {
        return;
    };
    let mut actual = Vec::new();
    replay(&mut machine, FRAMES, |frame, _, log| {
        actual.extend(log.apu.iter().map(|w| (frame as u64, w.addr, w.value)));
    });
    common::assert_write_digests("run1-apu", &actual);
}
