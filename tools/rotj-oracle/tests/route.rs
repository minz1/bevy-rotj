//! The oracle against `tools/emu/oracle-route` (by its digests), a stage 1-1 capture with
//! recorded input.

pub mod common;

use rotj_core::digest::fnv1a;
use rotj_oracle::{data_path as data, power_on, read_inputs};

fn inputs() -> Vec<u8> {
    read_inputs(&data("route-inputs.csv"))
}

#[test]
fn route_replay_matches_frame_900() {
    let Some(mut machine) = power_on() else {
        return;
    };
    let pads = inputs();
    assert!(pads.len() > 1000, "route-inputs.csv covers frames 0-1000");

    let mut apu = Vec::new();
    let mut run = |machine: &mut rotj_oracle::Machine, frame: usize| {
        let log = machine.run_frame(pads[frame]);
        if frame >= 600 {
            apu.extend(log.apu.iter().map(|w| (machine.frame, w.addr, w.value)));
        }
    };
    for frame in 1..=899 {
        run(&mut machine, frame);
    }
    assert_eq!(
        fnv1a(&machine.bus.ram[..0x800]),
        common::digest("route-900-ram"),
        "RAM differs from fceux's ram-900.bin"
    );
    assert_eq!(
        fnv1a(&machine.bus.ppu.oam[..]),
        common::digest("route-900-oam"),
        "OAM differs from fceux's oam-900.bin"
    );
    for frame in 900..=1000 {
        run(&mut machine, frame);
    }
    common::assert_write_digests("route-apu", &apu);
}

#[test]
fn the_actor_loop_is_bracketed_once_per_frame() {
    let Some(mut machine) = power_on() else {
        return;
    };
    let pads = inputs();
    for &pad in &pads[1..=640] {
        machine.run_frame(pad);
    }
    machine.bus.trace.peeks = vec![0xD450, 0xD453];
    for &pad in &pads[641..=999] {
        machine.run_frame(pad);
    }
    let peeked = &machine.bus.trace.peeked;
    assert_eq!(peeked.len(), 2 * 359);
    for (i, pair) in peeked.chunks(2).enumerate() {
        let frame = 641 + u64::try_from(i).expect("small");
        assert_eq!((pair[0].pc, pair[1].pc), (0xD450, 0xD453));
        assert_eq!((pair[0].frame, pair[1].frame), (frame, frame));
        assert_eq!((pair[0].bank, pair[1].bank), (Some(14), Some(14)));
        assert_eq!(pair[0].nmis, pair[1].nmis);
    }
}
