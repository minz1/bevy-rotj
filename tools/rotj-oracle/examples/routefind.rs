//! Oracle-driven route finder: a beam search over macro-actions of controller
//! input, scored by progress into stage 1-1, that writes an `inputs.csv` fceux
//! and `rotj_oracle` both replay (see `dump.lua`'s `INPUT=replay`).
//!
//! Run with `cargo run -p rotj-oracle --release --example routefind -- --help`.

use std::collections::HashSet;
use std::fmt::Write as _;

use rotj_oracle::{Machine, load_rom};

/// The Start press that leaves the title (dump.lua `route`/`route2`).
const START_FROM: u64 = 300;
const START_TO: u64 = 306;
/// Controller bits (NES order, A first).
const A: u8 = 0x01;
const B: u8 = 0x02;
const START: u8 = 0x08;
const UP: u8 = 0x10;
const DOWN: u8 = 0x20;
const LEFT: u8 = 0x40;
const RIGHT: u8 = 0x80;

/// Progress variables in the game's CPU RAM.
const ROOM: usize = 0x0402;
const LEVEL_X: usize = 0x001E;
const LEVEL_X_HI: usize = 0x001F;
/// `$00B0`: the player's x in pixels (walking moves it; a wall pins it).
const PLAYER_X: usize = 0x00B0;
const CAM_LO: usize = 0x00CC;
const CAM_HI: usize = 0x00CD;
const STATE: usize = 0x046E;
const ENERGY: usize = 0x0478;
const LIVES: usize = 0x0482;
const FLAG: usize = 0x00BA;
const Y: usize = 0x00B1;
const LEVEL_INDEX: usize = 0x0020;
/// `$0477`: the meter the red-brick room's state-4 actor fills (+2 per hit,
/// level end armed at `$0E`; bank 13 `$AB69`).
const METER: usize = 0x0477;

/// Parsed command-line knobs.
struct Knobs {
    width: usize,
    chunk: u64,
    depth: usize,
    from: u64,
    out: String,
    probe: bool,
    trace: Option<String>,
    prefix: Option<String>,
    stop_room: u8,
}

/// One beam node: a machine and the macro-actions that reached it.
struct State {
    machine: Machine,
    /// One pad per macro-action, `chunk` frames each.
    pads: Vec<u8>,
    score: (u8, i32, i32),
}

/// The macro-actions the search picks from: a pad held for `chunk` frames
/// (`Right+A` holds A the whole chunk, the variable-height jump).
const ACTIONS: [(u8, &str); 12] = [
    (RIGHT, "right"),
    (RIGHT | A, "right+A"),
    (A, "A"),
    (LEFT, "left"),
    (LEFT | A, "left+A"),
    (0, "wait"),
    (UP | A, "up+A"),
    (RIGHT | B, "right+B"),
    (B, "B"),
    (DOWN | A, "down+A"),
    (START, "start"),
    (0x04, "select"),
];

fn pad_for(frame: u64) -> u8 {
    if (START_FROM..=START_TO).contains(&frame) {
        START
    } else {
        0
    }
}

/// The game's level x (`$001E/$001F`, little-endian).
fn level_x(m: &Machine) -> i32 {
    i32::from(m.bus.ram[LEVEL_X]) | (i32::from(m.bus.ram[LEVEL_X_HI]) << 8)
}

/// The camera scroll in level pixels (`$CC:$CD >> 4`), monotonic inside room 1.
fn camera(m: &Machine) -> i32 {
    let v = (i32::from(m.bus.ram[CAM_LO]) << 8) | i32::from(m.bus.ram[CAM_HI]);
    v >> 4
}

/// Progress: room, then a position term, then a damage penalty so a clean
/// state wins ties. In room 1 the position term is the camera plus a bias to
/// the higher (airborne) state, which is what lands the player on room 2's
/// upper platform. In the red-brick room (room >= 2) the exit looked up (the
/// crate/door), so reward height first and the player's x (`$00B0`) as a
/// tiebreak; `$0477` is kept in the key only (the state-4 actor that would fill
/// it never spawns there, see `docs/SUBLEVELS.md` §7d-iii).
fn score_of(m: &Machine) -> (u8, i32, i32) {
    let room = m.bus.ram[ROOM];
    let damage = i32::from(2u8.wrapping_sub(m.bus.ram[LIVES])) * 100
        + i32::from(0x0E_u8.wrapping_sub(m.bus.ram[ENERGY]));
    let aux = if room < 2 {
        camera(m) * 4096 + (255 - i32::from(m.bus.ram[Y])) - damage * 2000
    } else {
        // The red-brick room: the exit is up (the crate/door), so weight height
        // first and the player's x ($00B0) as a tiebreak.
        (255 - i32::from(m.bus.ram[Y])) * 4096 + i32::from(m.bus.ram[PLAYER_X]) - damage * 2000
    };
    (room, aux, -damage)
}

/// Dedup key: room + position; never collapse a state to one with a different
/// player x, y or hit counter.
fn key_of(m: &Machine) -> (u8, i32, i32, i32) {
    (
        m.bus.ram[ROOM],
        i32::from(m.bus.ram[METER]) * 65536 + i32::from(m.bus.ram[PLAYER_X]),
        i32::from(m.bus.ram[STATE]),
        i32::from(m.bus.ram[Y]),
    )
}

fn hex(v: u8) -> i32 {
    i32::from(v)
}

fn describe(m: &Machine) -> String {
    format!(
        "room={} levelx={} px={} py={} cam={} st={} en={:02X} lives={} ba={} meter={:02X} idx={:02X}",
        hex(m.bus.ram[ROOM]),
        level_x(m),
        hex(m.bus.ram[PLAYER_X]),
        hex(m.bus.ram[Y]),
        camera(m),
        hex(m.bus.ram[STATE]),
        hex(m.bus.ram[ENERGY]),
        hex(m.bus.ram[LIVES]),
        hex(m.bus.ram[FLAG]),
        hex(m.bus.ram[METER]),
        hex(m.bus.ram[LEVEL_INDEX]),
    )
}

fn usage() {
    println!(
        "routefind — beam-search controller input for stage 1-1\n\
         \n\
         cargo run -p rotj-oracle --release --example routefind -- [options]\n\
         \n\
         --width N      beam width (default 48)\n\
         --chunk N      frames per macro-action (default 8)\n\
         --depth N      max macro-actions (default 400)\n\
         --from F       first frame the search controls (default 700)\n\
         --out FILE     write the route as frame,pad (default tests/data/stage1-inputs.csv)\n\
         --prefix FILE  warm start: replay this frame,pad csv, then search from there\n\
         --stop-room N  stop when $0402 >= N (default 9 = run to the level end)\n\
         --probe        run the prefix, then 200 idle frames, printing state\n\
         --trace FILE   replay a frame,pad csv and print state every frame\n\
         --help         this text"
    );
}

fn value<T: std::str::FromStr>(args: &mut impl Iterator<Item = String>) -> T {
    args.next()
        .expect("a value")
        .parse()
        .unwrap_or_else(|_| panic!("not a value"))
}

fn parse() -> Knobs {
    let mut k = Knobs {
        width: 48,
        chunk: 8,
        depth: 400,
        from: 700,
        out: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/stage1-inputs.csv")
            .to_string_lossy()
            .into_owned(),
        probe: false,
        trace: None,
        prefix: None,
        stop_room: 9,
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--width" => k.width = value(&mut args),
            "--chunk" => k.chunk = value(&mut args),
            "--depth" => k.depth = value(&mut args),
            "--from" => k.from = value(&mut args),
            "--out" => k.out = args.next().expect("a path"),
            "--prefix" => k.prefix = Some(args.next().expect("a path")),
            "--stop-room" => k.stop_room = value(&mut args),
            "--probe" => k.probe = true,
            "--trace" => k.trace = Some(args.next().expect("a path")),
            "--help" | "-h" => {
                usage();
                std::process::exit(0);
            }
            other => {
                eprintln!("unknown argument {other}\n");
                usage();
                std::process::exit(2);
            }
        }
    }
    k
}

fn main() {
    let k = parse();
    let Some(cart) = load_rom() else {
        std::process::exit(1);
    };
    if let Some(path) = &k.trace {
        trace(path, cart);
        return;
    }
    run_route(&k, cart);
}

/// Builds the prefix, runs the search and writes the route.
fn run_route(k: &Knobs, cart: rotj_oracle::Cart) {
    let mut machine = Machine::power_on(cart);
    if k.probe {
        for frame in 0..k.from.max(900) {
            machine.run_frame(pad_for(frame));
            if frame % 20 == 0 {
                println!("f={} {}", machine.frame, describe(&machine));
            }
        }
        return;
    }

    // Rows are indexed by frame: row N is the pad for oracle frame N; row 0 is
    // unused (fceux's first `frameadvance` runs no frame).
    let mut rows: Vec<u8> = match &k.prefix {
        Some(path) => load_pads(path),
        None => (0..=k.from).map(pad_for).collect(),
    };
    for &pad in &rows[1..] {
        machine.run_frame(pad);
    }
    println!(
        "snapshot at frame {}: {}",
        rows.len() - 1,
        describe(&machine)
    );

    let winner = search(k, machine);
    println!("winner: {}", describe(&winner.machine));
    for pad in &winner.pads {
        rows.extend(std::iter::repeat_n(
            *pad,
            usize::try_from(k.chunk).expect("small"),
        ));
    }
    let mut csv = String::from("frame,pad\n");
    for (frame, pad) in rows.iter().enumerate() {
        let _ = writeln!(csv, "{frame},{pad}");
    }
    std::fs::write(&k.out, csv).expect("write route");
    println!("wrote {} ({} frames)", k.out, rows.len());
}

/// The beam search: expand every state with every action, keep the best `width`
/// distinct positions, stop on the level end or `stop_room`.
fn search(k: &Knobs, machine: Machine) -> State {
    let mut beam = vec![State {
        score: score_of(&machine),
        pads: Vec::new(),
        machine,
    }];
    for step in 0..k.depth {
        let next = trim(expand(&beam, k), k.width);
        if next.is_empty() {
            eprintln!("beam emptied at step {step}");
            return beam.remove(0);
        }
        println!(
            "step {step:3}: {} | {}",
            describe(&next[0].machine),
            summary(&next)
        );
        let top = &next[0].machine;
        let done = top.bus.ram[FLAG] >= 5 || top.bus.ram[ROOM] >= k.stop_room;
        beam = next;
        if done {
            println!("reached the stop condition at step {step}");
            break;
        }
    }
    beam.remove(0)
}

/// One macro-action forward for every state.
fn expand(beam: &[State], k: &Knobs) -> Vec<State> {
    let mut candidates: Vec<State> = Vec::new();
    for parent in beam {
        for (pad, _) in &ACTIONS {
            let mut m = parent.machine.clone();
            for _ in 0..k.chunk {
                m.run_frame(*pad);
            }
            let mut pads = parent.pads.clone();
            pads.push(*pad);
            candidates.push(State {
                score: score_of(&m),
                pads,
                machine: m,
            });
        }
    }
    candidates
}

/// Keeps the best-scoring `width` candidates with distinct positions.
fn trim(mut candidates: Vec<State>, width: usize) -> Vec<State> {
    candidates.sort_by_key(|c| std::cmp::Reverse(c.score));
    let mut seen: HashSet<(u8, i32, i32, i32)> = HashSet::new();
    let mut next: Vec<State> = Vec::new();
    for c in candidates {
        if seen.insert(key_of(&c.machine)) {
            next.push(c);
        }
        if next.len() >= width {
            break;
        }
    }
    next
}

/// A compact range of what the beam currently covers.
fn summary(beam: &[State]) -> String {
    let ys: Vec<i32> = beam
        .iter()
        .map(|s| i32::from(s.machine.bus.ram[Y]))
        .collect();
    let xs: Vec<i32> = beam
        .iter()
        .map(|s| i32::from(s.machine.bus.ram[PLAYER_X]))
        .collect();
    format!(
        "beam min_y={} px=[{}..{}]",
        ys.iter().min().copied().unwrap_or(0),
        xs.iter().min().copied().unwrap_or(0),
        xs.iter().max().copied().unwrap_or(0),
    )
}

/// Reads a `frame,pad` csv into a pad per frame.
fn load_pads(path: &str) -> Vec<u8> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
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

/// Replays `<frame,pad>` rows, driving oracle frame N with row N's pad.
fn trace(path: &str, cart: rotj_oracle::Cart) {
    let pads = load_pads(path);
    let mut machine = Machine::power_on(cart);
    for (frame, &pad) in pads.iter().enumerate().skip(1) {
        machine.run_frame(pad);
        println!("f={frame} {}", describe(&machine));
    }
    println!("final: {}", describe(&machine));
}
