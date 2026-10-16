//! Replays `tests/data/stage1-inputs.csv`, the route `examples/routefind.rs`
//! finds, and pins the progress it reaches.
//!
//! The route plays stage 1-1 from the title into the game's own mid-level
//! transition: the symmetric "stage" counter `$0402` goes `0 -> 2` (the
//! red-brick room), exactly where the scripted `route2` capture stalls
//! (`docs/SUBLEVELS.md` §7d). It does **not** reach stage 1-2, so the level
//! index `$20` is never advanced; §7d records the limit and the payload that
//! would advance `$20` (`INC $20` at WRAM `$63D0`).

use std::path::PathBuf;

use rotj_oracle::{Machine, load_rom};

/// `$0402`, the room counter (`docs/SUBLEVELS.md` §7d).
const ROOM: usize = 0x0402;
/// `$0400`, the area index / CHR-set selector (`$D94E` output).
const AREA: usize = 0x0400;
/// `$20`, the level index (`$D94E` selector).
const LEVEL_INDEX: usize = 0x0020;

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

/// `frame,pad` rows; row N drives oracle frame N (row 0's frameadvance runs none).
fn inputs() -> Vec<u8> {
    let text = std::fs::read_to_string(data("stage1-inputs.csv")).expect("stage1-inputs.csv");
    let mut pads = Vec::new();
    for (row, line) in text.lines().skip(1).enumerate() {
        let (frame, pad) = line.split_once(',').expect("frame,pad");
        assert_eq!(frame.parse::<usize>().expect("frame"), row);
        pads.push(pad.parse().expect("pad"));
    }
    pads
}

#[test]
fn stage1_route_reaches_the_red_brick_room_change() {
    let Some(cart) = load_rom() else {
        return;
    };
    let pads = inputs();
    let mut machine = Machine::power_on(cart);

    // The room counter (ignoring the title screen's $FF) and the first frame
    // each new room appears on; the area and the resident map either side of the
    // $0402 change are kept for the relationship asserts.
    let mut rooms: Vec<(u64, u8)> = Vec::new();
    let mut change: Option<(u8, Vec<u8>, u8, Vec<u8>)> = None;
    for (frame, &pad) in pads.iter().enumerate().skip(1) {
        let area_before = machine.bus.ram[AREA];
        let map_before = machine.bus.wram[..0x1000].to_vec();
        machine.run_frame(pad);
        let room = machine.bus.ram[ROOM];
        if room != 0xFF && rooms.last().map(|&(_, r)| r) != Some(room) {
            rooms.push((frame as u64, room));
        }
        if room == 2 && change.is_none() {
            change = Some((
                area_before,
                map_before,
                machine.bus.ram[AREA],
                machine.bus.wram[..0x1000].to_vec(),
            ));
        }
    }

    let sequence: Vec<u8> = rooms.iter().map(|&(_, r)| r).collect();
    assert_eq!(
        sequence,
        vec![0, 2],
        "the route reaches the room change 0 -> 2 (rooms seen: {rooms:?})"
    );
    assert_eq!(
        machine.bus.ram[ROOM], 2,
        "the final state is the red-brick room"
    );
    // The room change advances $0402 without re-selecting the level's area: $0400
    // stays 0 and the resident map $6000-$6FFF is byte-identical across it (the
    // scripted `route2` capture's own relationship, `docs/SUBLEVELS.md` §7d).
    let (area_before, map_before, area_after, map_after) =
        change.expect("the route reaches the red-brick room change");
    assert_eq!(area_before, 0, "area before the room change");
    assert_eq!(
        area_after, 0,
        "sub_D3D8 did not re-select the level at the room change"
    );
    assert_eq!(
        map_before, map_after,
        "the room change does not reload the map"
    );
    // The route, like the scripted capture, stops here: no level end ($BA = 5)
    // is armed and the level index is not the stage-1-2 value. The stall is
    // recorded in docs/SUBLEVELS.md §7d.
    assert_ne!(machine.bus.ram[0x00BA], 5, "the level end was not reached");
    assert_ne!(machine.bus.ram[LEVEL_INDEX], 1, "stage 1-2 was not reached");
}
