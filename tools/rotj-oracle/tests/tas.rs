//! The published TAS (TASVideos 1087M), fetched by `tools/emu/fetch-tas.sh`, replayed from power-on.

use std::path::PathBuf;

fn tas() -> Option<Vec<u8>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../emu/tas/inputs.csv");
    if !path.exists() {
        assert!(
            std::env::var_os("ROTJ_REQUIRE_ROM").is_none(),
            "TAS required but not found at {} (run tools/emu/fetch-tas.sh)",
            path.display()
        );
        println!("skipped: TAS not found at {}", path.display());
        return None;
    }
    Some(rotj_oracle::read_inputs(&path))
}

fn areas_and_rooms(frames: usize) -> Option<Vec<(u8, u8)>> {
    let inputs = tas()?;
    let mut machine = rotj_oracle::power_on()?;
    let mut seen = Vec::new();
    for &pad in inputs.iter().skip(1).take(frames) {
        machine.run_frame(pad);
        let ram = &machine.bus.ram;
        let (area, room) = (ram[0x400], ram[0x402]);
        if ram[0x482] == 2 && seen.last() != Some(&(area, room)) && room != 0xFF {
            seen.push((area, room));
        }
    }
    Some(seen)
}

#[test]
fn tas_reaches_stage_1_2() {
    let Some(seen) = areas_and_rooms(2100) else {
        return;
    };
    assert_eq!(seen, [(0, 0), (0, 2), (0, 4), (2, 0)]);
}

#[test]
#[ignore = "plays the whole game (about a minute in a debug build); run with --ignored"]
fn tas_plays_every_area() {
    let Some(seen) = areas_and_rooms(usize::MAX) else {
        return;
    };
    let mut areas: Vec<u8> = seen.iter().map(|&(area, _)| area).collect();
    areas.dedup();
    assert_eq!(areas, (0..=0x22).step_by(2).collect::<Vec<u8>>());
}
