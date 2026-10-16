//! Pins a few rows of the whole-game TAS actor inventory (`docs/ACTORS.md` §8b,
//! `tools/rotj-oracle/examples/inventory_tas.rs`): stage 1-1 room 0's types and
//! their first frames, the on-screen projection, and the boss fights' areas.
//!
//! Skips when the TAS or ROM is absent, like `tests/tas.rs`.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// `frame,pad` rows of the published TAS; `None` when it has not been fetched.
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

/// The bank-2 level/screen loader and boot `$FF` fill write `$7CC0` without
/// spawning an actor.
const NOT_SPAWNS: [u16; 3] = [0x8A9F, 0x8AED, 0x98FB];
const OCCUPIED: u16 = 0x7CC0;
/// The on-screen return of the bank-13 `$A852` projection; `$10` is the slot.
const ONSCREEN_PC: u16 = 0xA8AD;
/// The boss-HP setup `sta $AA` (bank 13 `$BE12`).
const BOSS_HP_PC: u16 = 0xBE12;

/// `type -> (first frame, last frame)` the TAS spawns alive in `(area, room)`.
fn types_in(inputs: &[u8], area: u8, room: u8, frames: usize) -> BTreeMap<u8, (u64, u64)> {
    let mut m = rotj_oracle::power_on().expect("ROM");
    m.bus.trace.watch.push(OCCUPIED..=OCCUPIED + 31);
    let mut seen: BTreeMap<u8, (u64, u64)> = BTreeMap::new();
    for (frame, &pad) in inputs.iter().enumerate().skip(1).take(frames) {
        let frame = u64::try_from(frame).expect("frame fits");
        m.run_frame(pad);
        for event in std::mem::take(&mut m.bus.trace.writes) {
            let spawned = event.addr <= OCCUPIED + 31
                && event.value != 0
                && event.value % 2 == 0
                && !NOT_SPAWNS.contains(&event.pc);
            if spawned && m.bus.ram[0x0400] == area && m.bus.ram[0x0402] == room {
                seen.entry(event.value)
                    .and_modify(|e| e.1 = frame)
                    .or_insert((frame, frame));
            }
        }
    }
    seen
}

/// Stage 1-1 room 0 (`docs/ACTORS.md` §8b): eight types, three of them from the
/// frame-317 level-start records (`$B2`, `$C6`, `$06`), the rest the runtime
/// spawner as the screen scrolls.
#[test]
fn tas_stage_1_1_room_0_types() {
    let Some(inputs) = tas() else {
        return;
    };
    if rotj_oracle::power_on().is_none() {
        return;
    }
    let types = types_in(&inputs, 0, 0, 700);
    let observed: Vec<(u8, u64)> = types.iter().map(|(&t, &(f, _))| (t, f)).collect();
    let expected: [(u8, u64); 8] = [
        (0x02, 565),
        (0x06, 317),
        (0x08, 429),
        (0x0A, 492),
        (0x10, 442),
        (0x22, 455),
        (0xB2, 317),
        (0xC6, 317),
    ];
    assert_eq!(observed, expected);
}

/// The `$A852` projection reports a stage-1-1 actor on screen with `$94`/`$95`
/// inside the 256x240 window: at frame 494 type `$0A` is in slot 2.
#[test]
fn tas_projection_reports_an_on_screen_actor() {
    let Some(inputs) = tas() else {
        return;
    };
    let Some(mut m) = rotj_oracle::power_on() else {
        return;
    };
    m.bus.trace.peeks.push(ONSCREEN_PC);
    let mut hits: Vec<(u64, u8)> = Vec::new();
    for (frame, &pad) in inputs.iter().enumerate().skip(1).take(500) {
        let frame = u64::try_from(frame).expect("frame fits");
        m.run_frame(pad);
        for peek in std::mem::take(&mut m.bus.trace.peeked) {
            let (x, y) = (peek.ram[0x94], peek.ram[0x95]);
            if peek.pc == ONSCREEN_PC && x < 240 && y < 240 {
                hits.push((frame, peek.ram[0x10]));
            }
        }
    }
    assert!(
        hits.iter().any(|&(f, slot)| f == 494 && slot == 2),
        "frame 494, slot 2: {hits:?}"
    );
}

/// The five boss fights: the HP setup `$BE12` writes `$00AA` once per
/// stage-end area, `$0400` = `$04`, `$0E`, `$14`, `$1E`, `$22`
/// (`docs/ACTORS.md` §8b.1).
#[test]
#[ignore = "plays the whole game (about a minute in a debug build); run with --ignored"]
fn tas_records_five_boss_fights() {
    let Some(inputs) = tas() else {
        return;
    };
    let Some(mut m) = rotj_oracle::power_on() else {
        return;
    };
    m.bus.trace.watch.push(0x00AA..=0x00AA);
    let mut areas: Vec<u8> = Vec::new();
    let mut frames: Vec<u64> = Vec::new();
    for (frame, &pad) in inputs.iter().enumerate().skip(1) {
        let frame = u64::try_from(frame).expect("frame fits");
        m.run_frame(pad);
        for event in std::mem::take(&mut m.bus.trace.writes) {
            if event.pc == BOSS_HP_PC && areas.last() != Some(&m.bus.ram[0x0400]) {
                areas.push(m.bus.ram[0x0400]);
                frames.push(frame);
            }
        }
    }
    assert_eq!(areas, [0x04, 0x0E, 0x14, 0x1E, 0x22]);
    assert_eq!(frames, [6281, 14648, 26280, 36845, 39700]);
}
