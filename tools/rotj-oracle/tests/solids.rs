//! The port's collision rule (`rotj_core::collision`, the game's own metatile
//! class table at WRAM `$7340` and the two probe handler tables at bank 14
//! `$CADC`/`$C97D`) against the oracle's own answer at every collision probe the
//! stage-1 route makes. ROM-gated like the other oracle tests.

use std::collections::BTreeMap;
use std::path::PathBuf;

use rotj_core::collision::{CLASS_COUNT, IN_ORDER, Level, class_table_for_level, collision_of};
use rotj_core::levelmap::map_for_level;
use rotj_oracle::{Machine, load_rom};

/// The dispatch `$C99D` jumps through: `(frame, pc, bank, handler)`. The body
/// probe writes the resolved handler pointer to `$2C/$2D` before `JMP ($2C)`.
const DISPATCH: u16 = 0xC9D7;

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

/// `frame,pad` rows of the stage-1 route (`examples/routefind.rs`).
fn route() -> Vec<u8> {
    let text = std::fs::read_to_string(data("stage1-inputs.csv")).expect("stage1-inputs.csv");
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

/// `dump.lua`'s `INPUT=physics` script: Start 300-306, Right 660-780, A 800, a
/// running jump from 900.
fn physics_pad(frame: u64) -> u8 {
    let mut pad = 0;
    if (300..=306).contains(&frame) {
        pad |= 0x08;
    }
    if (660..=780).contains(&frame) {
        pad |= 0x80;
    }
    if frame == 800 {
        pad |= 0x01;
    }
    if frame >= 900 {
        pad |= 0x80;
        if frame == 905 {
            pad |= 0x01;
        }
    }
    pad
}

/// The terrain built the way `Stage::for_level` builds it (the ROM's map and
/// the level's class table).
fn terrain(cart: &rotj_rom::Rom, level: usize) -> Level {
    let map_rom = map_for_level(&cart.prg, &cart.chr, level).expect("the ROM holds the map");
    let mut map = [0u8; 4096];
    for (row, bytes) in map_rom.iter().enumerate() {
        map[row * 256..row * 256 + 256].copy_from_slice(bytes);
    }
    let classes = class_table_for_level(&cart.prg, level).expect("the level's classes");
    Level {
        map,
        classes: classes.map(|class| class * 2),
        ..Level::default()
    }
}

/// The body probe's handlers that push the actor back out of the tile (end in
/// the position revert `$C9E2`/`$CA1D`), read from bank 14 `$CADC`.
fn pushes(handler: u16) -> bool {
    matches!(handler, 0xC9DA | 0xCA2C | 0xCA3D | 0xCA66 | 0xCA86 | 0xCAA9)
}

/// The port's collision replayed against every solidity probe the stage-1
/// route's player movement makes, in the oracle, frame by frame: each `$C99D`
/// body probe's map cell (its position) and the handler the game dispatched to
/// (its result), against `collision_at` on the ROM-decoded map and classes. The
/// first divergence fails with its frame and position; the run must cover the
/// whole route.
#[test]
fn the_rules_solidity_matches_every_probe_the_route_makes() {
    let Some(cart) = load_rom() else {
        return;
    };
    let pads = route();
    let mut machine = Machine::power_on(cart);
    machine.bus.trace.peeks.push(DISPATCH);
    for &pad in pads.iter().skip(1) {
        machine.run_frame(pad);
    }

    let map = terrain(&machine.bus.cart, 0);
    let classes = class_table_for_level(&machine.bus.cart.prg, 0).expect("level 0 classes");
    let mut probes = 0usize;
    let mut cells: BTreeMap<(u8, u8), (bool, usize)> = BTreeMap::new();
    for p in &machine.bus.trace.peeked {
        if p.pc != DISPATCH {
            continue;
        }
        let r = &p.ram;
        let handler = u16::from(r[0x2C]) | (u16::from(r[0x2D]) << 8);
        let addr = u16::from(r[0x2A]) | (u16::from(r[0x2B]) << 8);
        if !(0x6000..0x7000).contains(&addr) {
            continue;
        }
        let row = u8::try_from((addr - 0x6000) >> 8).expect("row");
        let col = u8::try_from(addr & 0xFF).expect("col");
        let metatile = p.wram[usize::from(addr - 0x6000)];
        if usize::from(metatile) >= CLASS_COUNT {
            continue;
        }
        let game = pushes(handler);
        let ours = map
            .collision_at(
                &machine.bus.cart.prg,
                i32::from(col) * 16,
                i32::from(row) * 16,
            )
            .wall;
        assert_eq!(
            ours, game,
            "frame {} probe at metatile ({col}, {row}) = ${metatile:02X}: \
             the game's handler ${handler:04X} pushes={game}, the rule says {ours}",
            p.frame
        );
        // The live class table must match the ROM-decoded one too.
        assert_eq!(
            usize::from(p.wram[0x1340 + usize::from(metatile)]) / 2,
            usize::from(classes[usize::from(metatile)]),
            "frame {}: live $7340 class for metatile ${metatile:02X}",
            p.frame
        );
        probes += 1;
        cells
            .entry((row, col))
            .and_modify(|e| e.1 += 1)
            .or_insert((game, 1));
    }
    println!(
        "-- collision probe agreement: {probes} $C99D probes over {} map cells, \
         0 divergences (solids: {:?})",
        cells.len(),
        cells
            .iter()
            .filter(|&(_, &(solid, _))| solid)
            .map(|(&cell, _)| cell)
            .collect::<Vec<_>>()
    );
    assert!(probes > 500, "the route made only {probes} probes");
}

/// The physical consequence, on the oracle's own replay of `INPUT=physics`: the
/// actor walks right until the body probe (`$8184`, x + 10 px) first reads the
/// class-0 wall at metatile column 12, and his x then stops advancing. The
/// port's rule reports the same wall.
#[test]
fn the_physics_walk_pins_on_the_class_zero_wall() {
    let Some(cart) = load_rom() else {
        return;
    };
    let mut machine = Machine::power_on(cart);
    // The bank-4 body probe `$8184` returns to `$8187`; there `$20/$21` is the
    // probed x (the actor's x + 10) and `$22/$23` the probed y.
    machine.bus.trace.peeks.push(0x8187);
    for frame in 1..=850 {
        machine.run_frame(physics_pad(frame));
    }

    let map = terrain(&machine.bus.cart, 0);
    let classes = class_table_for_level(&machine.bus.cart.prg, 0).expect("level 0 classes");
    let probed: Vec<(u64, i32, i32)> = machine
        .bus
        .trace
        .peeked
        .iter()
        .filter(|p| p.pc == 0x8187)
        .map(|p| {
            (
                p.frame,
                i32::from((u16::from(p.ram[0x21]) << 8 | u16::from(p.ram[0x20])) / 16),
                i32::from((u16::from(p.ram[0x23]) << 8 | u16::from(p.ram[0x22])) / 16),
            )
        })
        .collect();

    let wall = probed
        .iter()
        .filter(|&&(f, _, _)| f >= 700)
        .find(|&&(_, x, y)| map.collision_at(&machine.bus.cart.prg, x, y).wall)
        .copied();
    let (wall_frame, wall_x, wall_y) = wall.expect("the walk reaches the class-0 wall");
    // The wall probe is the structural metatile column 12 (level x 192+).
    assert!(
        (190..208).contains(&wall_x),
        "the wall probe is at metatile column 12, got x={wall_x}"
    );
    // From the wall frame on, the probed x never advances past the wall.
    let last = probed.last().expect("probes");
    let max_after: i32 = probed
        .iter()
        .filter(|&&(f, _, _)| f >= wall_frame)
        .map(|&(_, x, _)| x)
        .max()
        .expect("probes after the wall");
    assert!(
        max_after < 208,
        "the actor walked through the wall: probe reached x={max_after}"
    );
    // The metatile under the probe is class 0's wall.
    let metatile = map.metatile(
        u8::try_from(wall_x / 16).expect("col"),
        u8::try_from(wall_y / 16).expect("row"),
        &IN_ORDER,
        false,
    );
    println!(
        "-- physics walk: first wall probe frame {wall_frame} at ({wall_x},{wall_y}), \
         metatile ${metatile:02X} class {} -> wall={}; last probe {last:?}",
        classes[usize::from(metatile)],
        map.collision_at(&machine.bus.cart.prg, wall_x, wall_y).wall,
    );
    assert!(
        collision_of(&machine.bus.cart.prg, classes[usize::from(metatile)]).wall,
        "metatile ${metatile:02X} must be a wall"
    );
    // The tiles he walked over before the wall were clear: column 11 at the
    // same height must not block.
    assert!(
        !map.collision_at(&machine.bus.cart.prg, wall_x - 16, wall_y)
            .wall,
        "the tile before the wall must be clear"
    );
}
