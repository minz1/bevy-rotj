//! Pins a few rows of the actor-type inventory produced by
//! `examples/inventory.rs`: the bank-14 dispatch table (the game's own
//! `$7CC0` -> handler map) and the types level 0 spawns under the walk script.

use std::collections::BTreeMap;

use rotj_oracle::{Machine, load_rom};

/// PRG bank holding the actor dispatch loop and its tables.
const DISPATCH_BANK: usize = 0x0E;
const DISPATCH_WINDOW: u16 = 0xC000;
const BANK_SIZE: usize = 0x2000;
const BANK_TABLE: u16 = 0xCF0D;
const HANDLER_TABLE: u16 = 0xCF72;
const MAX_TYPE: u16 = 0xC8;
const NOT_SPAWNS: [u16; 3] = [0x8A9F, 0x8AED, 0x98FB];

fn dispatch_byte(prg: &[u8], cpu: u16) -> u8 {
    prg[DISPATCH_BANK * BANK_SIZE + usize::from(cpu - DISPATCH_WINDOW)]
}

/// The game's `$7CC0` -> `(handler, bank)` map, read from `$CF0D`/`$CF72`.
fn dispatch_table(prg: &[u8], banks: usize) -> Vec<Option<(u16, u8)>> {
    let mut table = vec![None; 256];
    for t in (0..=MAX_TYPE).step_by(2) {
        let raw = dispatch_byte(prg, BANK_TABLE + t / 2);
        let bank = u8::try_from(usize::from(raw & 0x3F) % banks).expect("bank fits");
        let lo = dispatch_byte(prg, HANDLER_TABLE + t);
        let hi = dispatch_byte(prg, HANDLER_TABLE + t + 1);
        table[usize::from(t)] = Some((u16::from(lo) | (u16::from(hi) << 8), bank));
    }
    table
}

fn walk_pad(frame: u64) -> u8 {
    let mut pad = 0;
    if (300..=306).contains(&frame) {
        pad |= 0x08;
    }
    if frame >= 610 {
        pad |= 0x80;
        if (frame - 610) % 72 < 26 {
            pad |= 0x01;
        }
    }
    pad
}

/// Play level 0 with the walk script and return `type -> (handler, bank)` for
/// every actor the level spawned.
fn level_0_types() -> BTreeMap<u8, (u16, u8)> {
    let Some(cart) = load_rom() else {
        panic!("ROM missing: the inventory test needs raw/");
    };
    let table = dispatch_table(&cart.prg, cart.prg.len() / BANK_SIZE);
    let mut machine = Machine::power_on(cart);
    machine.bus.trace.pokes.push((0xD3D8, 0x0020, 0));
    machine.bus.trace.watch.push(0x7CC0..=0x7CDF);
    let mut types = BTreeMap::new();
    for frame in 1..=3000 {
        machine.bus.ram[0x0477] = 2; // keep the player alive
        machine.run_frame(walk_pad(frame));
        for w in &machine.bus.trace.writes {
            if (0x7CC0..=0x7CDF).contains(&w.addr)
                && w.value != 0
                && w.value % 2 == 0
                && !NOT_SPAWNS.contains(&w.pc)
            {
                types
                    .entry(w.value)
                    .or_insert_with(|| table[usize::from(w.value)].unwrap());
            }
        }
        machine.bus.trace.writes.clear();
    }
    types
}

/// The dispatch table is the game's own: `$CF0D[$7CC0 / 2]` selects the bank
/// mapped at `$8000` and `$CF72[$7CC0]` the handler. These rows are the types
/// the inventory observes, plus the two `$56`/`$54` types stages 2-2 and 5-2
/// spawn (`docs/STATUS.md`).
#[test]
fn dispatch_table_pins_the_per_type_handlers() {
    let Some(cart) = load_rom() else {
        return;
    };
    let table = dispatch_table(&cart.prg, cart.prg.len() / BANK_SIZE);
    let expected: [(u8, u16, u8); 11] = [
        (0x06, 0x823C, 8),
        (0x0A, 0x85FD, 8),
        (0x22, 0x9555, 8),
        (0xB2, 0x8C4E, 12),
        (0xC6, 0x9953, 12),
        (0x54, 0x9D49, 11),
        (0x56, 0x9DF9, 11),
        (0x5C, 0x9EC2, 11),
        (0x78, 0x8D4F, 0),
        (0x84, 0x9361, 0),
        (0xC8, 0x9A26, 12),
    ];
    for (ty, handler, bank) in expected {
        assert_eq!(
            table[usize::from(ty)],
            Some((handler, bank)),
            "type ${ty:02X}"
        );
    }
}

/// Level 0 (stage 1-1) spawns exactly these types under the walk script; their
/// handlers are the dispatch table's. The first three (frame 607) match the
/// captured opening screen (`docs/ACTORS.md` §7d/§7f).
#[test]
fn level_0_spawns_the_expected_types() {
    if load_rom().is_none() {
        return;
    }
    let types = level_0_types();
    let observed: Vec<(u8, u16, u8)> = types.iter().map(|(t, (h, b))| (*t, *h, *b)).collect();
    let expected: [(u8, u16, u8); 9] = [
        (0x02, 0x81D4, 8),
        (0x04, 0x823C, 8),
        (0x06, 0x823C, 8),
        (0x08, 0x8506, 8),
        (0x0A, 0x85FD, 8),
        (0x22, 0x9555, 8),
        (0x24, 0x9823, 8),
        (0xB2, 0x8C4E, 12),
        (0xC6, 0x9953, 12),
    ];
    assert_eq!(observed, expected);
}
