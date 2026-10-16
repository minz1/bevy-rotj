//! Whole-game actor inventory, taken along the published TAS.
//!
//! Replays `tools/emu/tas/inputs.csv` (TASVideos 1087M, fetched by
//! `tools/emu/fetch-tas.sh`) once from power-on with a write watch on the two
//! slot banks `$7CC0-$7DFF`, and records per actor type (`$7CC0`, the even
//! occupied byte the dispatcher `$CF0D`/`$CF72` indexes) where it appears, when
//! it is alive, which spawner wrote it, the `$7D50` states and `$7DD0` phases it
//! passes through, and whether it enters the player-contact path (bank 13
//! `$A9C4`) or the weapon scan (bank 13 `$AA57`, signalled by `$AA91`) for its
//! own slot.
//!
//! Slot -> type attribution for the engine peeks: `$10` is the slot cursor the
//! actor loop (bank 14 `$CED5`) sets before running a handler, and every peeked
//! PC below reads it.
//!
//! Run: `cargo run -p rotj-oracle --release --example inventory_tas`
//! Env `DEBUG_FRAME=<n>` dumps the slot arrays at that frame for calibration.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use rotj_oracle::{Cart, Machine, WriteEvent, load_rom};

/// `frame,pad` rows; row N drives oracle frame N.
fn tas_inputs() -> Option<Vec<u8>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../emu/tas/inputs.csv");
    if !path.exists() {
        eprintln!(
            "skipped: TAS not found at {} (run tools/emu/fetch-tas.sh)",
            path.display()
        );
        return None;
    }
    Some(rotj_oracle::read_inputs(&path))
}

/// Slot arrays. `$7CC0` is the occupied byte (always even; the type); `$7D50`
/// the state byte `$ABA8` dispatches on; `$7DD0` the per-type phase byte.
const OCCUPIED: u16 = 0x7CC0;
const STATE: u16 = 0x7D50;
const PHASE: u16 = 0x7DD0;
/// Batman's weapon slots (the `$7BD0` class the scan at bank 13 `$AA57` walks).
const WEAPONS: u16 = 0x7BD0;
/// The level-end flag; bank 13 `$AB69` sets it to `$05` when the boss meter
/// passes `$0E`, and bank 14 `$D4D3` dispatches on it (`$D58F` is the handler).
const LEVEL_END: u16 = 0x00BA;
/// The bank-5 handler selector (`$AB5A` sets it to `state + 1` on a hit).
const WEAPON_SELECT: u16 = 0x0476;

/// Engine PCs peeked (bank 13, `$A000` window). `$10` is the slot in every case.
const CONTACT_PC: u16 = 0xA9C4;
const DAMAGE_PC: u16 = 0xAA4B;
const WEAPON_HIT_PC: u16 = 0xAA91;
const ONSCREEN_PC: u16 = 0xA8AD;
/// `sta $AA` in the boss-HP setup (bank 13 `$BE0A`); fires once per boss fight.
const BOSS_HP_PC: u16 = 0xBE12;

/// PCs that write `$7CC0-$7CDF` without spawning an actor: the bank-2
/// level/screen loader copying the map (`$8A9F`/`$8AED`) and the boot `$FF`
/// fill (`$98FB`).
const NOT_SPAWNS: [u16; 3] = [0x8A9F, 0x8AED, 0x98FB];

/// On-screen bounds for the `$A852` projection result `$94`/`$95`.
const SCREEN_X: u8 = 240;
const SCREEN_Y: u8 = 240;
/// The fan-out scene window.
const SCENE_WINDOW: u64 = 300;
/// The five stage-end areas: the boss-HP setup (`$BE12`, bank 13) fires in
/// exactly these `$0400` values (frames 6281, 14648, 26280, 36845, 39700),
/// matching the manual ("bosses at the ends of stages 1, 3, 4, 6, 7").
const BOSS_AREAS: [u8; 5] = [0x04, 0x0E, 0x14, 0x1E, 0x22];

/// WRAM byte at a `$6000-$7FFF` address.
fn w(m: &Machine, addr: u16) -> u8 {
    m.bus.wram[usize::from(addr - 0x6000)]
}

/// The level index whose start area is the one `area` ($0400) belongs to.
///
/// `$D94E[$20]` is the 13-row level -> start-`$0400` table and `$D57D[$0400/2]`
/// maps any area to its stage's start area (bank 14 `$D527`). Both are read from
/// the ROM, so the mapping is the game's own.
fn level_of(prg: &[u8], area: u8) -> Option<u8> {
    let bank14 = &prg[14 * 0x2000..15 * 0x2000];
    let levels = &bank14[0x194E..0x194E + 13];
    let starts = &bank14[0x157D..0x157D + 19];
    let start = starts[usize::from(area) / 2];
    levels
        .iter()
        .position(|&v| v == start)
        .map(|i| u8::try_from(i).expect("13 levels"))
}

/// The `$7CC0` -> `(handler, bank)` map the dispatcher `$CED5` builds from the
/// bank-14 tables `$CF0D` (bank) and `$CF72` (handler).
fn handler_of(prg: &[u8], ty: u8) -> (u16, u8) {
    let bank14 = &prg[14 * 0x2000..15 * 0x2000];
    let table = |cpu: usize| bank14[cpu - 0xC000];
    let banks = prg.len() / 0x2000;
    let bank = u8::try_from(usize::from(table(0xCF0D + usize::from(ty) / 2) & 0x3F) % banks)
        .expect("bank");
    let lo = table(0xCF72 + usize::from(ty));
    let hi = table(0xCF73 + usize::from(ty));
    (u16::from(lo) | (u16::from(hi) << 8), bank)
}

/// One actor type, aggregated over the whole TAS.
#[derive(Default)]
struct Agg {
    first: u64,
    last: u64,
    /// (area `$0400`, room `$0402`) -> first/last frame alive there.
    rooms: BTreeMap<(u8, u8), (u64, u64)>,
    slots: BTreeSet<u8>,
    spawns: BTreeSet<(u16, Option<u8>)>,
    states: BTreeSet<u8>,
    phases: BTreeSet<u8>,
    contact: bool,
    damage: bool,
    weapon_hit: bool,
    /// (frame, phase) while the type was alive, for the scene search.
    alive: Vec<(u64, u8)>,
    /// Frames it was on screen, with its projected x/y.
    onscreen: Vec<(u64, u8, u8)>,
    scene: Option<u64>,
}

impl Agg {
    fn touch(&mut self, frame: u64) {
        if self.first == 0 || frame < self.first {
            self.first = frame;
        }
        self.last = self.last.max(frame);
    }

    fn in_room(&mut self, area: u8, room: u8, frame: u64) {
        let entry = self.rooms.entry((area, room)).or_insert((frame, frame));
        entry.0 = entry.0.min(frame);
        entry.1 = entry.1.max(frame);
    }
}

/// A `$BA` transition event: the room whose clear armed this screen/area change.
struct LevelEnd {
    frame: u64,
    value: u8,
    area: u8,
    room: u8,
    /// Actor types alive in slots when the flag was written.
    types: Vec<u8>,
}

/// Everything the replay collects.
#[derive(Default)]
struct Acc {
    types: BTreeMap<u8, Agg>,
    cur: [Option<u8>; 32],
    level_ends: Vec<LevelEnd>,
    flag_writes: Vec<(u64, u16, u8)>,
    hp_writes: Vec<(u64, u16, u16, u8)>,
    player_state: Vec<(u8, u16, u8)>,
    weapon_writes: Vec<(u64, u8, u8, u8, u16)>,
    weapon_select: Vec<(u64, u16, u8)>,
    execs: BTreeMap<(u8, u16), u64>,
    boss_hp: Vec<(u64, u8, Option<u8>)>,
}

impl Acc {
    /// Fold one finished frame's trace into the aggregates.
    fn frame(&mut self, m: &mut Machine, frame: u64, prg: &[u8]) {
        let (area, room) = (m.bus.ram[0x0400], m.bus.ram[0x0402]);
        let mut trans: Option<(u64, u8)> = None;

        for event in std::mem::take(&mut m.bus.trace.writes) {
            let addr = event.addr;
            if (OCCUPIED..OCCUPIED + 32).contains(&addr) {
                self.occupied(&event, area, room);
            } else if (STATE..STATE + 32).contains(&addr) {
                if let Some(t) = self.cur[usize::from(addr - STATE)] {
                    self.types.entry(t).or_default().states.insert(event.value);
                }
            } else if (PHASE..PHASE + 32).contains(&addr) {
                if let Some(t) = self.cur[usize::from(addr - PHASE)] {
                    self.types.entry(t).or_default().phases.insert(event.value);
                }
            } else if addr == LEVEL_END {
                self.flag_writes.push((frame, event.pc, event.value));
                if matches!(event.value, 1 | 3 | 5) {
                    trans = Some((frame, event.value));
                }
            } else if addr == WEAPON_SELECT {
                self.weapon_select.push((frame, event.pc, event.value));
            } else if addr == 0x00AA || addr == 0x00F1 {
                self.hp_writes.push((frame, addr, event.pc, event.value));
            } else if (0x0460..=0x04FF).contains(&addr) {
                let level = level_of(prg, area).unwrap_or(0xFF);
                self.player_state.push((level, addr - 0x0460, event.value));
            } else if (WEAPONS..WEAPONS + 16).contains(&addr) {
                let level = level_of(prg, area).unwrap_or(0xFF);
                self.weapon_writes.push((
                    frame,
                    level,
                    u8::try_from(addr - WEAPONS).expect("16 slots"),
                    event.value,
                    event.pc,
                ));
            }
        }

        if let Some((tframe, value)) = trans {
            let mut types: Vec<u8> = self.cur.iter().flatten().copied().collect();
            types.sort_unstable();
            types.dedup();
            self.level_ends.push(LevelEnd {
                frame: tframe,
                value,
                area,
                room,
                types,
            });
        }

        for event in std::mem::take(&mut m.bus.trace.execs) {
            if let Some(bank) = event.bank {
                *self.execs.entry((bank, event.pc)).or_default() += 1;
            }
        }

        for peek in std::mem::take(&mut m.bus.trace.peeked) {
            let slot = peek.ram[0x10];
            if peek.pc == BOSS_HP_PC {
                self.boss_hp.push((
                    frame,
                    slot,
                    self.cur.get(usize::from(slot)).copied().flatten(),
                ));
            }
            let Some(t) = self.cur.get(usize::from(slot)).copied().flatten() else {
                continue;
            };
            let agg = self.types.entry(t).or_default();
            match peek.pc {
                CONTACT_PC => agg.contact = true,
                DAMAGE_PC => agg.damage = true,
                WEAPON_HIT_PC => agg.weapon_hit = true,
                ONSCREEN_PC => {
                    let (x, y) = (peek.ram[0x94], peek.ram[0x95]);
                    if x < SCREEN_X && y < SCREEN_Y {
                        agg.onscreen.push((frame, x, y));
                    }
                }
                _ => {}
            }
        }

        for slot in 0..32u8 {
            if let Some(t) = self.cur[usize::from(slot)] {
                let state = w(m, STATE + u16::from(slot));
                let phase = w(m, PHASE + u16::from(slot));
                let agg = self.types.entry(t).or_default();
                agg.touch(frame);
                agg.states.insert(state);
                agg.phases.insert(phase);
                agg.alive.push((frame, phase));
                agg.in_room(area, room, frame);
            }
        }
    }

    /// A write to `$7CC0-$7CDF`: a spawn, a free, or a filtered non-spawn.
    fn occupied(&mut self, event: &WriteEvent, area: u8, room: u8) {
        let slot = usize::from(event.addr - OCCUPIED);
        if event.value == 0 {
            self.cur[slot] = None;
            return;
        }
        if !event.value.is_multiple_of(2) || NOT_SPAWNS.contains(&event.pc) {
            return;
        }
        self.cur[slot] = Some(event.value);
        let agg = self.types.entry(event.value).or_default();
        agg.touch(event.frame);
        agg.slots.insert(u8::try_from(slot).expect("0..31"));
        agg.spawns.insert((event.pc, event.bank));
        agg.in_room(area, room, event.frame);
    }
}

/// Replay the TAS and print the inventory. (`$BA` = 5 and the `$D58F` level-end
/// path never fire in this TAS; the boss areas come from the HP setup instead —
/// see `docs/ACTORS.md` §8b.1.)
fn main() {
    let Some(inputs) = tas_inputs() else {
        return;
    };
    let Some(cart) = load_rom() else {
        return;
    };
    let debug_frame: Option<u64> = std::env::var("DEBUG_FRAME")
        .ok()
        .and_then(|s| s.parse().ok());
    let mut acc = replay(&cart, &inputs, debug_frame);
    for agg in acc.types.values_mut() {
        agg.alive.sort_unstable();
        agg.onscreen.sort_unstable();
        agg.scene = best_scene(&agg.onscreen, &agg.alive);
    }
    print_table(&cart.prg, &acc);
    print_evidence(&acc);
    print_state(&acc);
}

fn replay(cart: &Cart, inputs: &[u8], debug_frame: Option<u64>) -> Acc {
    let mut m = Machine::power_on(cart.clone());
    m.bus.trace.watch.push(OCCUPIED..=0x7DFF);
    m.bus.trace.watch.push(LEVEL_END..=LEVEL_END);
    m.bus.trace.watch.push(WEAPON_SELECT..=WEAPON_SELECT);
    m.bus.trace.watch.push(WEAPONS..=WEAPONS + 15);
    // RE-NOTES leads: `$00AA` and `$00F1` are labelled "boss HP".
    m.bus.trace.watch.push(0x00AA..=0x00AA);
    m.bus.trace.watch.push(0x00F1..=0x00F1);
    // Player/level state block, for a per-stage weapon selector.
    m.bus.trace.watch.push(0x0460..=0x04FF);
    for pc in [
        CONTACT_PC,
        DAMAGE_PC,
        WEAPON_HIT_PC,
        ONSCREEN_PC,
        BOSS_HP_PC,
    ] {
        m.bus.trace.peeks.push(pc);
    }
    // The $7D50 hit/level-end dispatch (bank 13 `$AB48`) and the bank-14
    // level-end handler `$D58F`: executed PCs prove whether the boss path ran.
    m.bus.trace.exec.push(0xAB39..=0xAB75);
    m.bus.trace.exec.push(0xD58F..=0xD5B5);

    let mut acc = Acc::default();
    for (frame, &pad) in inputs.iter().enumerate().skip(1) {
        let frame = u64::try_from(frame).expect("frame fits");
        m.run_frame(pad);
        acc.frame(&mut m, frame, &cart.prg);
        if debug_frame == Some(frame) {
            dump_frame(&m, frame);
        }
    }
    acc
}

/// A frame where the type is on screen and covers the most `$7DD0` phases in the
/// next [`SCENE_WINDOW`] frames.
fn best_scene(onscreen: &[(u64, u8, u8)], alive: &[(u64, u8)]) -> Option<u64> {
    let mut best: Option<(usize, u64)> = None;
    for &(f, _, _) in onscreen {
        let start = alive.partition_point(|&(g, _)| g < f);
        let mut set = BTreeSet::new();
        for &(g, p) in &alive[start..] {
            if g >= f + SCENE_WINDOW {
                break;
            }
            set.insert(p);
        }
        if best.is_none_or(|(n, _)| set.len() > n) {
            best = Some((set.len(), f));
        }
    }
    best.map(|(_, f)| f)
}

/// Calibration: the slot arrays at one frame.
fn dump_frame(m: &Machine, frame: u64) {
    let (area, room) = (m.bus.ram[0x0400], m.bus.ram[0x0402]);
    println!("# debug frame {frame} $0400={area:02X} $0402={room:02X}");
    for slot in 0..32u16 {
        let occupied = w(m, OCCUPIED + slot);
        if occupied != 0 {
            println!(
                "slot {slot:2} $7CC0={occupied:02X} $7D50={:02X} $7DD0={:02X} $7D90={:02X} $7DB0={:02X}",
                w(m, STATE + slot),
                w(m, PHASE + slot),
                w(m, 0x7D90 + slot),
                w(m, 0x7DB0 + slot),
            );
        }
    }
}

fn join_hex(set: &BTreeSet<u8>) -> String {
    set.iter()
        .map(|v| format!("{v:02X}"))
        .collect::<Vec<_>>()
        .join("+")
}

fn join_str(it: impl Iterator<Item = String>) -> String {
    it.collect::<Vec<_>>().join("+")
}

/// Print the inventory as TSV: one row per (type, level, area, room).
fn print_table(prg: &[u8], acc: &Acc) {
    println!(
        "# inventory: type,level,area($0400),room($0402),first,last,slots,spawn(PC@bank),states,phases,contact,damage,weapon_hit,scene,onscreen,handler@bank,boss_only"
    );
    for (&t, agg) in &acc.types {
        let (handler, bank) = handler_of(prg, t);
        let spawns = join_str(agg.spawns.iter().map(|(pc, b)| {
            format!(
                "{pc:04X}@{}",
                b.map_or("-".to_string(), |b| format!("{b:02X}"))
            )
        }));
        let slots = join_str(agg.slots.iter().map(u8::to_string));
        let states = join_hex(&agg.states);
        let phases = join_hex(&agg.phases);
        let boss_only = u8::from(agg.rooms.keys().all(|(area, _)| BOSS_AREAS.contains(area)));
        for (&(area, room), &(first, last)) in &agg.rooms {
            let level = level_of(prg, area).map_or("-".to_string(), |l| l.to_string());
            println!(
                "TYPE\t{t:02X}\t{level}\t{area:02X}\t{room:02X}\t{first}\t{last}\t{slots}\t{spawns}\t{states}\t{phases}\t{}\t{}\t{}\t{}\t{}\t{handler:04X}@{bank}\t{boss_only}",
                u8::from(agg.contact),
                u8::from(agg.damage),
                u8::from(agg.weapon_hit),
                agg.scene.map_or("-".to_string(), |f| f.to_string()),
                agg.onscreen.len(),
            );
        }
    }
}

/// `$BA` transitions and the weapon families.
fn print_evidence(acc: &Acc) {
    println!("# $BA transitions: value, frame, area, room, types alive");
    for ev in &acc.level_ends {
        let types = join_hex(&ev.types.iter().copied().collect());
        println!(
            "TRANS\t{:02X}\t{}\t{:02X}\t{:02X}\t{types}",
            ev.value, ev.frame, ev.area, ev.room
        );
    }

    println!("# weapon slots $7BD0-$7BDF: slot, first, last, values, writes");
    let mut per_slot: BTreeMap<u8, (u64, u64, BTreeSet<u8>, u64)> = BTreeMap::new();
    for &(frame, _level, slot, value, _pc) in &acc.weapon_writes {
        let e = per_slot
            .entry(slot)
            .or_insert((frame, frame, BTreeSet::new(), 0));
        e.0 = e.0.min(frame);
        e.1 = e.1.max(frame);
        e.2.insert(value);
        e.3 += 1;
    }
    for (slot, (a, b, vals, n)) in &per_slot {
        println!("WEAPON\t{slot}\t{a}\t{b}\t{}\t{n}", join_hex(vals));
    }

    println!("# weapon-slot $7BD0 writes per level: level, value, count, first, last, pc");
    let mut per_level: BTreeMap<(u8, u8, u16), (u64, u64, u64)> = BTreeMap::new();
    for &(frame, level, _slot, value, pc) in &acc.weapon_writes {
        if value == 0 {
            continue;
        }
        let e = per_level
            .entry((level, value, pc))
            .or_insert((frame, u64::MAX, 0));
        e.0 = e.0.min(frame);
        e.1 = e.1.min(frame);
        e.2 += 1;
    }
    for ((level, value, pc), (first, last, n)) in &per_level {
        println!("WPLEVEL\t{level}\t{value:02X}\t{n}\t{first}-{last}\t{pc:04X}");
    }
}

/// `$BA`/`$0476` writers, the boss-HP setup, executed PCs and the player block.
fn print_state(acc: &Acc) {
    println!("# $BA writes: pc, value, count, first, last");
    let mut per_flag: BTreeMap<(u16, u8), (u64, u64, u64)> = BTreeMap::new();
    for &(frame, pc, value) in &acc.flag_writes {
        let e = per_flag.entry((pc, value)).or_insert((frame, u64::MAX, 0));
        e.0 = e.0.min(frame);
        e.1 = e.1.min(frame);
        e.2 += 1;
    }
    for ((pc, value), (first, last, n)) in &per_flag {
        println!("FLAG\t{pc:04X}\t{value:02X}\t{n}\t{first}-{last}");
    }

    println!("# boss HP setup ($BE12) events: frame, slot, type");
    for &(frame, slot, t) in &acc.boss_hp {
        println!(
            "BOSS\t{frame}\t{slot}\t{}",
            t.map_or("-".to_string(), |t| format!("{t:02X}"))
        );
    }

    println!("# $00AA/$00F1 writes: addr, pc, value, count, first, last");
    let mut per_hp: BTreeMap<(u16, u16, u8), (u64, u64, u64)> = BTreeMap::new();
    for &(frame, addr, pc, value) in &acc.hp_writes {
        let e = per_hp
            .entry((addr, pc, value))
            .or_insert((frame, u64::MAX, 0));
        e.0 = e.0.min(frame);
        e.1 = e.1.min(frame);
        e.2 += 1;
    }
    for ((addr, pc, value), (first, last, n)) in &per_hp {
        println!("HP\t{addr:04X}\t{pc:04X}\t{value:02X}\t{n}\t{first}-{last}");
    }

    println!("# executed PCs in the hit/level-end windows: pc@bank, count");
    for ((bank, pc), n) in &acc.execs {
        println!("EXEC\t{pc:04X}@{bank}\t{n}");
    }

    println!("# $0476 writes: pc, value, count, first, last");
    let mut per_pc: BTreeMap<(u16, u8), (u64, u64, u64)> = BTreeMap::new();
    for &(frame, pc, value) in &acc.weapon_select {
        let e = per_pc.entry((pc, value)).or_insert((frame, u64::MAX, 0));
        e.0 = e.0.min(frame);
        e.1 = e.1.min(frame);
        e.2 += 1;
    }
    for ((pc, value), (first, last, n)) in &per_pc {
        println!("SELECT\t{pc:04X}\t{value:02X}\t{n}\t{first}-{last}");
    }

    println!("# $0460-$04FF writes by level: addr, level, values, count");
    let mut per_addr: BTreeMap<(u16, u8), (BTreeSet<u8>, u64)> = BTreeMap::new();
    for &(level, addr, value) in &acc.player_state {
        let e = per_addr
            .entry((addr, level))
            .or_insert((BTreeSet::new(), 0));
        e.0.insert(value);
        e.1 += 1;
    }
    for ((addr, level), (vals, n)) in &per_addr {
        println!("PSTATE\t{addr:04X}\t{level}\t{}\t{n}", join_hex(vals));
    }
}
