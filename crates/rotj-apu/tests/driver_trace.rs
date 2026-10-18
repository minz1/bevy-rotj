//! Replay the running game's own sound-driver register writes through the port's
//! APU.
//!
//! The writes are the game's own, from the oracle running fceux's `tools/emu/stage`
//! input (`physics_input`) to frame 1005: every write its music driver made to
//! `$4000-$4017` from frame 600 on, the run `rotj-oracle`'s `music.rs` pins to fceux's
//! capture by digest. Feeding them to [`rotj_apu::apu::Apu`] exercises the real
//! driver's register usage against our cores and proves all four channels make sound.
//!
//! Known gap: the DMC registers are written but the core does not play samples
//! yet, so the mixed output here has no sampled bass (see `dmc` module docs).

use std::collections::BTreeSet;

use rotj_apu::apu::Apu;
use rotj_apu::dmc::{self, SampleRom};
use rotj_apu::{STATUS, noise, pulse1, pulse2, triangle};

/// APU ticks per NTSC frame (1,789,773 Hz / 60.0988 Hz).
const TICKS_PER_FRAME: u32 = 29_781;

/// A write the game's driver made: frame, register, value.
struct Write {
    frame: u32,
    addr: u16,
    value: u8,
}

/// The game's APU writes for frames 600-1005 of the physics run, or `None` without the ROM.
fn trace() -> Option<Vec<Write>> {
    let mut machine = rotj_oracle::power_on()?;
    let mut writes = Vec::new();
    for frame in 1..=1005 {
        let log = machine.run_frame(rotj_oracle::physics_input(frame));
        let frame = u32::try_from(machine.frame).expect("a short run");
        if frame >= 600 {
            writes.extend(log.apu.iter().map(|w| Write {
                frame,
                addr: w.addr,
                value: w.value,
            }));
        }
    }
    Some(writes)
}

/// The register families the driver wrote to, for the coverage assertion.
fn families(writes: &[Write]) -> BTreeSet<&'static str> {
    writes
        .iter()
        .filter_map(|write| match write.addr {
            pulse1::CTRL..=pulse1::TIMER_HI => Some("pulse1"),
            pulse2::CTRL..=pulse2::TIMER_HI => Some("pulse2"),
            triangle::CTRL | triangle::TIMER_LO | triangle::TIMER_HI => Some("triangle"),
            noise::CTRL | noise::TIMER | noise::LENGTH => Some("noise"),
            dmc::CTRL..=dmc::LENGTH => Some("dmc"),
            STATUS => Some("enable"),
            _ => None,
        })
        .collect()
}

#[test]
fn replaying_the_driver_produces_sound_on_every_channel() {
    let Some(writes) = trace() else {
        return;
    };
    let rom = rotj_rom::for_test().expect("the oracle ran it");
    assert_eq!(
        writes.len(),
        3468,
        "the capture's writes without $4014/$4016"
    );

    let families = families(&writes);
    for expected in ["pulse1", "pulse2", "triangle", "noise", "dmc", "enable"] {
        assert!(families.contains(expected), "driver never wrote {expected}");
    }

    // The game's samples live in the fixed $E000 window, PRG bank 15.
    let mut apu = Apu::new();
    apu.set_sample_rom(SampleRom::new(0xe000, rom.prg[0x1E000..0x20000].to_vec()));
    let mut peak = 0.0_f32;
    let mut sum_squares = 0.0_f64;
    let mut samples = 0_u32;
    let mut channel_peaks = [0_u8; 5];

    let last_frame = writes.last().expect("non-empty").frame;
    let first_frame = writes.first().expect("non-empty").frame;
    let mut next = 0;
    for frame in first_frame..=last_frame {
        // Apply this frame's writes in order, then run the frame's APU ticks.
        while next < writes.len() && writes[next].frame == frame {
            let write = &writes[next];
            apu.write(write.addr, write.value);
            next += 1;
        }
        for _ in 0..TICKS_PER_FRAME {
            apu.tick();
            let out = apu.output();
            peak = peak.max(out);
            sum_squares += f64::from(out) * f64::from(out);
            samples += 1;
            let levels = apu.channel_levels();
            for (peak_of, level) in channel_peaks.iter_mut().zip(levels) {
                *peak_of = (*peak_of).max(level);
            }
        }
    }

    let rms = (sum_squares / f64::from(samples)).sqrt();
    println!("peak {peak:.4}, rms {rms:.4}, channel peaks {channel_peaks:?}");
    assert!(peak > 0.05, "output was near-silent (peak {peak})");
    assert!(peak <= 1.0, "output exceeded full scale (peak {peak})");
    assert!(rms > 0.005, "output had almost no energy (rms {rms})");
    for (i, peak_of) in channel_peaks.iter().enumerate() {
        assert!(*peak_of > 0, "channel {i} never produced sound");
    }
}
