//! Per-level actor-type inventory, driven by the 6502 oracle.
//!
//! For every level index 0..12 this forces `$20` at the game's own level-start
//! instruction (`$D3D8`), plays with a fixed input while keeping the player
//! alive, and logs every write to the slot window `$7CC0-$7DEF` and every
//! instruction executed in the actor handler window `$8000-$9FFF`.
//!
//! The per-type handler is the dispatch the game itself runs: the loop at bank
//! 14 `$CED5` reads `$7CC0,x`, maps bank `$CF0D[$7CC0 / 2]` into `$8000` (a
//! 101-entry bank table), then `JMP ($CF72[$7CC0])` (the even occupied byte
//! indexes a stride-2 pointer table). Both tables are read from the ROM here,
//! so the mapping is the game's own, not a guess.
//!
//! Run: `cargo run -p rotj-oracle --release --example inventory`

use std::collections::BTreeMap;

use rotj_oracle::{Cart, Machine, load_rom};

/// Level indices the bank-14 `$D94E` table names.
const LEVELS: u8 = 13;
/// Frames played per level (gameplay starts around frame 607).
const FRAMES: u64 = 3000;
/// First frame Right is held.
const PLAY_FROM: u64 = 610;

/// PRG bank holding the actor update loop and its dispatch tables.
const DISPATCH_BANK: usize = 0x0E;
/// CPU window the dispatcher and its tables are mapped at.
const DISPATCH_WINDOW: u16 = 0xC000;
const BANK_SIZE: usize = 0x2000;
/// CPU `$CF0D`: bank select, indexed by `$7CC0 / 2`.
const BANK_TABLE: u16 = 0xCF0D;
/// CPU `$CF72`: 16-bit handler pointer, indexed by the occupied byte `$7CC0`.
const HANDLER_TABLE: u16 = 0xCF72;
/// Highest occupied byte the bank table describes (one entry per two bytes).
const MAX_TYPE: u16 = 0xC8;

/// PCs that write `$7CC0-$7CDF` without spawning: the bank-2 level/screen loader
/// copying the map (`$8A9F`/`$8AED`) and the `$FF` fill at level load (`$98FB`).
/// A real spawn writes the record's state/behaviour/kind as well.
const NOT_SPAWNS: [u16; 3] = [0x8A9F, 0x8AED, 0x98FB];

/// What `$0477` is held at each frame to keep the player alive.
///
/// `dump.lua`'s `ROUTE_INVIN` writes 1; the oracle panics on 1 because bank 14
/// `$DBE6` then indexes the stride-2 pointer table `$DC0F[$0477]` with an odd
/// index and reads a PPU mirror (`$28D0`), which the oracle refuses (fceux
/// returns open bus). Any other value works, so this uses the smallest even one.
const KEEP_ALIVE: u8 = 2;

/// One actor type seen in a level, with the dispatch the ROM gives it.
#[derive(Clone)]
struct TypeRow {
    state: u8,
    behaviour: u8,
    kind: u8,
    first_frame: u64,
    spawn_pc: u16,
    spawn_bank: Option<u8>,
    handler: u16,
    handler_bank: u8,
}

/// Read a byte of `prg` at a CPU address in the dispatcher's window.
fn dispatch_byte(prg: &[u8], cpu: u16) -> u8 {
    let off = DISPATCH_BANK * BANK_SIZE + usize::from(cpu - DISPATCH_WINDOW);
    prg[off]
}

/// The occupied byte (`$7CC0`) -> `(handler, bank)` map the game builds from
/// `$CF0D`/`$CF72`. Only even bytes up to [`MAX_TYPE`] have a valid bank.
fn dispatch_table(prg: &[u8], banks: usize) -> Vec<Option<(u16, u8)>> {
    let mut table = vec![None; 256];
    for t in (0..=MAX_TYPE).step_by(2) {
        let raw = dispatch_byte(prg, BANK_TABLE + t / 2);
        let bank = usize::from(raw & 0x3F) % banks;
        let bank = u8::try_from(bank).expect("bank count fits a byte");
        let lo = dispatch_byte(prg, HANDLER_TABLE + t);
        let hi = dispatch_byte(prg, HANDLER_TABLE + t + 1);
        table[usize::from(t)] = Some((u16::from(lo) | (u16::from(hi) << 8), bank));
    }
    table
}

/// The scripted inputs. `Walk` is a plain walk with a held jump every 72 frames;
/// `Prog`/`Route` are the project's `dump.lua` scripts; `Adaptive` jumps when
/// the camera stops advancing. Different scripts reach different screens, so the
/// inventory is their union.
#[derive(Clone, Copy, Debug)]
enum Script {
    Walk,
    Prog,
    Route,
    Adaptive,
    Sweep,
}

/// Every script the inventory plays each level with.
const SCRIPTS: [Script; 5] = [
    Script::Walk,
    Script::Prog,
    Script::Route,
    Script::Adaptive,
    Script::Sweep,
];

/// Start is held to leave the title in every script.
fn pad_for(frame: u64, script: Script, stuck: u64) -> u8 {
    let mut pad = 0;
    if (300..=306).contains(&frame) {
        pad |= 0x08; // Start
    }
    match script {
        Script::Walk => {
            if frame >= PLAY_FROM {
                pad |= 0x80; // Right
                if (frame - PLAY_FROM) % 72 < 26 {
                    pad |= 0x01; // held A
                }
            }
        }
        // `dump.lua` INPUT=prog: Right with a short jump every 40 frames and B
        // every 120.
        Script::Prog => {
            if frame >= 600 {
                pad |= 0x80;
                if (frame - 600) % 40 < 4 {
                    pad |= 0x01;
                }
                if (frame - 600) % 120 < 3 {
                    pad |= 0x02;
                }
            }
        }
        // `dump.lua` INPUT=route: the two held jumps that clear stage 1-1's
        // steps (`docs/ACTORS.md` §7f).
        Script::Route => {
            if frame >= PLAY_FROM {
                pad |= 0x80;
            }
            if (735..=760).contains(&frame) || (792..=817).contains(&frame) {
                pad |= 0x01;
            }
        }
        Script::Sweep => {
            if frame >= PLAY_FROM {
                pad |= 0x80;
            }
        }
        // Walk right, and hold A while the camera has not advanced, to clear
        // whatever stopped the walk; B every 90 frames to clear enemies.
        Script::Adaptive => {
            if frame >= PLAY_FROM {
                pad |= 0x80;
                if stuck >= 12 {
                    pad |= 0x01;
                }
                if (frame - PLAY_FROM) % 90 < 2 {
                    pad |= 0x02;
                }
            }
        }
    }
    pad
}

/// Run one level and return the types it spawned plus the distinct
/// `(bank, pc)` instructions the actor window executed.
fn run_level(
    cart: &Cart,
    level: u8,
    dispatch: &[Option<(u16, u8)>],
    script: Script,
) -> (BTreeMap<u8, TypeRow>, BTreeMap<(u8, u16), u64>) {
    let mut machine = Machine::power_on(cart.clone());
    machine.bus.trace.pokes.push((0xD3D8, 0x0020, level));
    machine.bus.trace.watch.push(0x7CC0..=0x7DEF);
    machine.bus.trace.exec.push(0x8000..=0x9FFF);
    let mut rows: BTreeMap<u8, TypeRow> = BTreeMap::new();
    let mut execs: BTreeMap<(u8, u16), u64> = BTreeMap::new();
    let mut last_progress = (0u8, 0u8);
    let mut stuck = 0u64;
    for frame in 1..=FRAMES {
        let progress = (machine.bus.ram[0x0400], machine.bus.ram[0x00AC]);
        if progress == last_progress && frame > PLAY_FROM {
            stuck += 1;
        } else {
            stuck = 0;
        }
        last_progress = progress;
        machine.bus.ram[0x0477] = KEEP_ALIVE;
        if matches!(script, Script::Sweep) && frame >= PLAY_FROM {
            // Advance the level's spawn cursor so every schedule slot is used.
            machine.bus.ram[0x00CC] = u8::try_from((frame - PLAY_FROM) / 8 % 256).expect("byte");
        }
        machine.run_frame(pad_for(frame, script, stuck));
        for event in machine.bus.trace.execs.drain(..) {
            if let Some(bank) = event.bank {
                *execs.entry((bank, event.pc)).or_default() += 1;
            }
        }
        for w in &machine.bus.trace.writes {
            let occupied = (0x7CC0..=0x7CDF).contains(&w.addr);
            if !occupied || w.value == 0 || w.value % 2 != 0 || NOT_SPAWNS.contains(&w.pc) {
                continue;
            }
            let slot = usize::from(w.addr - 0x7CC0);
            rows.entry(w.value).or_insert_with(|| TypeRow {
                state: machine.bus.wram[0x1D50 + slot],
                behaviour: machine.bus.wram[0x1D90 + slot],
                kind: machine.bus.wram[0x1DB0 + slot],
                first_frame: frame,
                spawn_pc: w.pc,
                spawn_bank: w.bank,
                handler: dispatch[usize::from(w.value)].map_or(0, |(h, _)| h),
                handler_bank: dispatch[usize::from(w.value)].map_or(0, |(_, b)| b),
            });
        }
        machine.bus.trace.writes.clear();
    }
    (rows, execs)
}

/// Distinct instruction PCs executed in `bank`, for the handler-size estimate.
fn bank_pcs(execs: &BTreeMap<(u8, u16), u64>, bank: u8) -> Vec<u16> {
    execs
        .keys()
        .filter(|(b, _)| *b == bank)
        .map(|(_, pc)| *pc)
        .collect()
}

/// Distinct PCs executed in `[entry, next)` of `bank`: the rough handler size.
fn handler_size(pcs: &[u16], entry: u16, next: u16) -> usize {
    pcs.iter().filter(|&&pc| pc >= entry && pc < next).count()
}

fn main() {
    let Some(cart) = load_rom() else {
        return;
    };
    let banks = cart.prg.len() / BANK_SIZE;
    let dispatch = dispatch_table(&cart.prg, banks);

    println!("# level,type,kind,behaviour,state,handler,bank,first_frame,spawn_pc,spawn_bank");
    let mut union_execs: BTreeMap<(u8, u16), u64> = BTreeMap::new();
    let mut seen_in: BTreeMap<u8, Vec<u8>> = BTreeMap::new();
    for level in 0..LEVELS {
        let mut rows: BTreeMap<u8, TypeRow> = BTreeMap::new();
        let mut execs: BTreeMap<(u8, u16), u64> = BTreeMap::new();
        for script in SCRIPTS {
            let (script_rows, script_execs) = run_level(&cart, level, &dispatch, script);
            for (ty, row) in script_rows {
                rows.entry(ty)
                    .and_modify(|old| {
                        if row.first_frame < old.first_frame {
                            *old = row.clone();
                        }
                    })
                    .or_insert(row);
            }
            for (key, count) in script_execs {
                *execs.entry(key).or_default() += count;
            }
        }
        for (key, count) in execs {
            *union_execs.entry(key).or_default() += count;
        }
        for (ty, row) in &rows {
            let levels = seen_in.entry(*ty).or_default();
            if !levels.contains(&level) {
                levels.push(level);
            }
            println!(
                "{level},{ty:02X},{:02X},{:02X},{:02X},{:04X},{},{},{:04X},{}",
                row.kind,
                row.behaviour,
                row.state,
                row.handler,
                row.handler_bank,
                row.first_frame,
                row.spawn_pc,
                row.spawn_bank.unwrap_or(0xFF),
            );
        }
    }

    println!("\n# types seen in more than one level");
    for (ty, levels) in &seen_in {
        if levels.len() > 1 {
            println!("{ty:02X} in levels {levels:?}");
        }
    }

    // Rough handler sizes: distinct PCs executed in [entry, next entry) of the
    // handler's own bank, over every level's run.
    let mut entries_by_bank: BTreeMap<u8, Vec<u16>> = BTreeMap::new();
    for slot in &dispatch {
        if let Some((handler, bank)) = *slot {
            entries_by_bank.entry(bank).or_default().push(handler);
        }
    }
    println!("\n# handler,bank,size (distinct instrs executed in [entry, next entry))");
    for (bank, mut entries) in entries_by_bank {
        entries.sort_unstable();
        entries.dedup();
        let pcs = bank_pcs(&union_execs, bank);
        for (i, &entry) in entries.iter().enumerate() {
            let next = entries.get(i + 1).copied().unwrap_or(0xA000);
            let size = handler_size(&pcs, entry, next);
            if entry >= 0x8000 && entry < next && size > 0 {
                println!("{entry:04X},{bank},{size}");
            }
        }
    }
}
