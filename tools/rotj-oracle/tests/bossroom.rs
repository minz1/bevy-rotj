//! What the red-brick room (stage 1-1 room `$0402 = 2`) contains, read with the
//! oracle from the committed `tests/data/stage1-inputs.csv` route.
//!
//! Evidence for the room's two actors and the player's motion addresses
//! (`$00B0` x, `$00B1` y), recorded in `docs/SUBLEVELS.md` §7d and
//! `docs/ACTORS.md` §10.

use std::path::PathBuf;

use rotj_oracle::{Machine, load_rom};

/// `frame,pad` rows of the committed route; row N drives oracle frame N.
fn route() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/stage1-inputs.csv");
    let text = std::fs::read_to_string(path).expect("stage1-inputs.csv");
    text.lines()
        .skip(1)
        .map(|line| {
            line.split_once(',')
                .expect("frame,pad")
                .1
                .parse()
                .expect("pad")
        })
        .collect()
}

/// Boots the route to its last frame (the player is in the red-brick room).
fn in_room() -> Option<Machine> {
    let mut m = Machine::power_on(load_rom()?);
    for &pad in route().iter().skip(1) {
        m.run_frame(pad);
    }
    Some(m)
}

/// WRAM byte at a `$6000-$7FFF` address.
fn w(m: &Machine, addr: usize) -> u8 {
    m.bus.wram[addr - 0x6000]
}

/// The room spawns two actors at frame 1058 (level-start init, bank 14
/// `$D7A8`-`$D821`): slot 24 (type `$04`) and slot 0 (type `$06`).
#[test]
fn red_brick_room_actors() {
    let Some(mut m) = in_room() else {
        return;
    };
    // The route ends at frame 1026; the room init runs at 1058.
    for _ in 0..(1058 - 1026) {
        m.run_frame(0);
    }
    assert_eq!(m.frame, 1058, "the level-start spawn frame");
    assert_eq!(m.bus.ram[0x0402], 2, "still the red-brick room");

    // Slot 24: type `$04`, state `$08`, record id 1 of the room's stage table.
    assert_eq!(w(&m, 0x7CC0 + 24), 0x04, "slot 24 occupied byte (type)");
    assert_eq!(w(&m, 0x7D50 + 24), 0x08, "slot 24 state");
    assert_eq!(w(&m, 0x7D90 + 24), 0x00, "slot 24 behaviour");
    assert_eq!(w(&m, 0x7DB0 + 24), 0x00, "slot 24 kind");
    assert_eq!(w(&m, 0x7CE0 + 24), 0x30, "slot 24 spawn-schedule slot");

    // Slot 0: type `$06`, state `$00`, the crate/door actor (drawn up-left).
    assert_eq!(w(&m, 0x7CC0), 0x06, "slot 0 occupied byte (type)");
    assert_eq!(w(&m, 0x7D50), 0x00, "slot 0 state");
    assert_eq!(w(&m, 0x7D90), 0x33, "slot 0 behaviour");
    assert_eq!(w(&m, 0x7DB0), 0x61, "slot 0 kind");
    assert_eq!(w(&m, 0x7CE0), 0x31, "slot 0 spawn-schedule slot");

    // Slot 0's hit lock (`$7EE0`, record byte 11 `$48`) keeps the actor-vs-player
    // contact path (bank 13 `$A9C4`) from resolving it; slot 24's is clear.
    assert_eq!(w(&m, 0x7EE0), 0x48, "slot 0 hit lock");
    assert_eq!(w(&m, 0x7EE0 + 24), 0x00, "slot 24 hit lock");
}

/// The player's x is `$00B0` and y `$00B1`: holding Right walks him from the
/// room's left wall (x 20) to its right wall (x 214); a held A jumps him
/// (state `$046E` reaches 3 and y falls from 176 to ~122). So he is *not*
/// frozen in the room (`docs/SUBLEVELS.md` §7d corrects the earlier lead).
#[test]
fn red_brick_room_is_walkable_and_jumpable() {
    let Some(mut m) = in_room() else {
        return;
    };
    m.run_frame(0);
    let mut min_y = 255u8;
    let mut max_x = 0u8;
    let mut jumped = false;
    for i in 0..300 {
        // Walk right; hold A in bursts so the variable-height jump fires.
        let pad = if i % 40 < 25 { 0x81 } else { 0x80 };
        m.run_frame(pad);
        min_y = min_y.min(m.bus.ram[0x00B1]);
        max_x = max_x.max(m.bus.ram[0x00B0]);
        jumped |= m.bus.ram[0x046E] == 3;
    }
    assert!(
        max_x >= 214,
        "walked to the room's right wall (max_x {max_x})"
    );
    assert!(jumped, "the jump state $046E = 3 was reached");
    assert!(
        min_y <= 128,
        "the jump rose above the ground line (min_y {min_y})"
    );
    assert_eq!(m.bus.ram[0x0402], 2, "still in the red-brick room");
}
