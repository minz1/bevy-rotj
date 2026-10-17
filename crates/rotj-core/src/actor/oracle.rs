//! Oracle diffs: bracket a game routine with peeks and compare every byte either side changed.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use rotj_oracle::{
    Cpu, Machine, PeekEvent, Playtest, StartBytes, data_path, power_on, read_inputs,
};

use super::engine::Work;
use super::families::{Brain, SecondBrain};
use super::oam::OamBuilder;
use super::slots::{Slot, Slots};
use super::step_actors;
use super::tape::{Event, MEMORY, Tape, Writes, loop_hash, script_hash};
use super::world::{Cx, Effects, Ppu, Schedule, World};
use crate::actors::{CELL_PENDING, CELL_RECORD, SpawnRecord, spawn_record, spawn_record_pointer};
use crate::render::Line;

/// The ported paths that ran in any test so far, by their 6502 address.
static SEEN: Mutex<BTreeSet<u16>> = Mutex::new(BTreeSet::new());

/// Records that the port ran the path at `addr`.
pub(crate) fn seen(addr: u16) {
    SEEN.lock().expect("seen set").insert(addr);
}

thread_local! {
    /// The overflow flag the port held at each of [`OVERFLOW_PEEKS`] this frame, in order.
    static OVERFLOW: std::cell::RefCell<Vec<(u16, bool)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// After the player's animate-and-draw returns (`$84AB` -> `$D42F`).
const AFTER_PLAYER_DRAW: u16 = 0xD42F;
/// After the second-class draw returns (`$DC87` -> `$D43B`).
const AFTER_SECOND_CLASS_DRAW: u16 = 0xD43B;
/// Weapon 5's charge test (`$9790` in bank 5), which reads the overflow flag.
const WEAPON5_CHARGE: u16 = 0x9790;

/// Where the lockstep compares the overflow flag: after `$84AB` and `$DC87`, and at
/// weapon 5's charge, which reads it.
const OVERFLOW_PEEKS: [u16; 3] = [AFTER_PLAYER_DRAW, AFTER_SECOND_CLASS_DRAW, WEAPON5_CHARGE];

/// Records the overflow flag the port holds where the game reaches `pc`.
pub(crate) fn overflow_at(pc: u16, overflow: bool) {
    OVERFLOW.with_borrow_mut(|log| log.push((pc, overflow)));
}

/// Has the port run the path at `addr` in any test so far?
pub(crate) fn was_seen(addr: u16) -> bool {
    SEEN.lock().expect("seen set").contains(&addr)
}

/// The actor loop's `JSR $CED5` and the instruction after it.
pub(crate) const ACTOR_LOOP: (u16, u16) = (0xD450, 0xD453);

pub(crate) const SCRATCH: [std::ops::RangeInclusive<u16>; 7] = [
    0x10..=0x1F,
    0x20..=0x3F,
    0xDF..=0xDF,
    0xE9..=0xEA,
    0x100..=0x102,
    0x182..=0x1FF,
    0x4A0..=0x4DF,
];

/// Two peeks around one run of a routine, with no interrupt between them.
pub(crate) struct Bracket {
    pub begin: PeekEvent,
    pub end: PeekEvent,
}

/// Every `begin` peek immediately followed by one of `ends` with no interrupt between.
pub(crate) fn brackets(events: &[PeekEvent], begin: u16, ends: &[u16]) -> Vec<Bracket> {
    events
        .windows(2)
        .filter(|w| {
            w[0].pc == begin
                && ends.contains(&w[1].pc)
                && (w[0].nmis, w[0].irqs) == (w[1].nmis, w[1].irqs)
        })
        .map(|w| Bracket {
            begin: w[0].clone(),
            end: w[1].clone(),
        })
        .collect()
}

/// The first CPU address either side changed whose values differ: `(addr, port, oracle)`.
pub(crate) fn diff(
    before: &PeekEvent,
    port: (&[u8; 2048], &[u8; 8192]),
    oracle: &PeekEvent,
) -> Option<(u16, u8, u8)> {
    diff_claiming(before, port, oracle, &[])
}

/// [`diff`], also comparing the scratch addresses in `claimed` (a routine's outputs).
pub(crate) fn diff_claiming(
    before: &PeekEvent,
    port: (&[u8; 2048], &[u8; 8192]),
    oracle: &PeekEvent,
    claimed: &[u16],
) -> Option<(u16, u8, u8)> {
    let ram = (0..0x800u16).map(|a| {
        let i = usize::from(a);
        (a, before.ram[i], port.0[i], oracle.ram[i])
    });
    let wram = (0..0x2000u16).map(|a| {
        let i = usize::from(a);
        (0x6000 + a, before.wram[i], port.1[i], oracle.wram[i])
    });
    ram.chain(wram)
        .filter(|(a, ..)| claimed.contains(a) || !SCRATCH.iter().any(|r| r.contains(a)))
        .find(|&(a, b, p, o)| (claimed.contains(&a) || p != b || o != b) && p != o)
        .map(|(a, _, p, o)| (a, p, o))
}

/// Everything the game loop reads or writes, lifted from a peek.
pub(crate) type State = super::Game;

impl State {
    pub(crate) fn lift(peek: &PeekEvent) -> State {
        let mut state = State::lift_memory(&peek.ram, &peek.wram);
        state.work.cpu_overflow = peek.cpu.p & 0x40 != 0;
        state
    }

    pub(crate) fn lift_memory(ram: &[u8; 2048], wram: &[u8; 8192]) -> State {
        State {
            slots: Slots::lift(wram),
            world: World::lift(ram),
            schedule: Schedule::lift(ram, wram),
            effects: Effects::lift(ram, wram),
            oam: OamBuilder::lift(ram),
            work: Work::lift(ram),
            scroll: super::scroll::Scroll::lift(ram),
            display: super::frame::Display::lift(ram),
            script: None,
            probe: super::collision::Probe::lift(ram),
        }
    }

    /// The peek's memory with this state written over it.
    pub(crate) fn project(&self, peek: &PeekEvent) -> (Box<[u8; 2048]>, Box<[u8; 8192]>) {
        let (mut ram, mut wram) = (peek.ram.clone(), peek.wram.clone());
        self.project_into(&mut ram, &mut wram);
        (ram, wram)
    }

    /// Writes this state over `ram` and `wram`.
    pub(crate) fn project_into(&self, ram: &mut [u8; 2048], wram: &mut [u8; 8192]) {
        self.slots.project(wram);
        self.schedule.project(ram, wram);
        self.effects.project(ram, wram);
        self.oam.project(ram);
        self.work.project(ram);
        self.world.project(ram);
        self.scroll.project(ram);
        self.display.project(ram);
        for (addr, value) in self.probe.writes() {
            ram[usize::from(addr)] = value;
        }
    }
}

const BOOT: u64 = 640;

/// Where the level start reads the level (`$20`), which [`Scene::level`] forces.
const FORCE_LEVEL: u16 = 0xD3D8;

/// The frame each [`Scene::level`] reaches the NMI wait before its first loop frame
/// (`$D3F9`), found by running each level to its first `$D3F9`.
const FRESH_START: [u64; 13] = [
    640, 640, 641, 634, 641, 643, 639, 641, 641, 634, 641, 639, 643,
];

/// A running oracle and the pads it replays.
#[derive(Clone)]
pub(crate) struct Scene {
    pub machine: Machine,
    pads: Vec<u8>,
    /// The last frames' pictures: the lines the PPU drew them with and their split lines.
    pictures: std::collections::VecDeque<Picture>,
}

/// One oracle frame's lines (`FrameLog::lines`, `splits`).
#[derive(Clone, Debug)]
pub(crate) struct Picture {
    pub frame: u64,
    pub lines: Vec<Line>,
    pub splits: Vec<(usize, Line, Line)>,
}

/// What a line draws with: the background's scroll, table, banks and mirroring only
/// while the background shows, the sprites' size, table and banks only while they show.
fn drawn(l: &Line) -> Line {
    let (background, sprites) = (l.mask & 0x08 != 0, l.mask & 0x10 != 0);
    Line {
        v: if background { l.v } else { 0 },
        fine_x: if background { l.fine_x } else { 0 },
        ctrl: l.ctrl & (if background { 0x10 } else { 0 } | if sprites { 0x28 } else { 0 }),
        mask: l.mask,
        chr: if background { l.chr } else { [0; 8] },
        sprite_chr: if sprites { l.sprite_chr } else { [0; 8] },
        mirroring: if background { l.mirroring & 3 } else { 0 },
    }
}

/// The most lines in one frame the IRQs' jitter may explain: one per band edge.
const MOST_JITTERED: usize = 4;

/// Every drawn field of `line` is one of `near`'s (each CHR bank on its own), its
/// vertical scroll one of `own`'s; the nametable bit and coarse X come from different
/// writes (`$2000`, `$2005`), so either may land first.
fn mixes(line: &Line, own: &[Line], near: &[Line]) -> bool {
    let any = |f: &dyn Fn(&Line) -> bool| near.iter().any(f);
    own.iter().any(|o| o.v & 0x7BE0 == line.v & 0x7BE0)
        && any(&|n| n.v & 0x0400 == line.v & 0x0400)
        && any(&|n| n.v & 0x001F == line.v & 0x001F)
        && any(&|n| n.fine_x == line.fine_x)
        && any(&|n| n.ctrl == line.ctrl)
        && any(&|n| n.mask == line.mask)
        && any(&|n| n.mirroring == line.mirroring)
        && (0..8).all(|slot| any(&|n| n.chr[slot] == line.chr[slot]))
        && (0..8).all(|slot| any(&|n| n.sprite_chr[slot] == line.sprite_chr[slot]))
}

/// The oracle's line as the renderer's.
fn render_line(l: &rotj_oracle::Line) -> Line {
    Line {
        v: l.v,
        fine_x: l.fine_x,
        ctrl: l.ctrl,
        mask: l.mask,
        chr: l.chr,
        sprite_chr: l.sprite_chr,
        mirroring: l.mirroring,
    }
}

impl Scene {
    /// Power-on, then the route's recorded frames 1 through `frame`.
    pub(crate) fn route(frame: u64) -> Option<Scene> {
        Scene::replay("route-inputs.csv", frame)
    }

    /// Power-on, then frames 1 through `frame` of an input file in the oracle's `tests/data`.
    pub(crate) fn replay(inputs: &str, frame: u64) -> Option<Scene> {
        Scene::replay_from(inputs, &data_path(inputs), frame)
    }

    /// Power-on, then frames 1 through `frame` of the published TAS
    /// (`tools/emu/tas/inputs.csv`, fetched by `tools/emu/fetch-tas.sh`; area 1 from 2047).
    pub(crate) fn tas(frame: u64) -> Option<Scene> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/emu/tas/inputs.csv");
        if !path.exists() {
            assert!(
                std::env::var_os("ROTJ_REQUIRE_ROM").is_none(),
                "TAS required but not found at {}",
                path.display()
            );
            println!("skipped: TAS not found at {}", path.display());
            return None;
        }
        Scene::replay_from("tas", &path, frame)
    }

    fn replay_from(key: &str, path: &Path, frame: u64) -> Option<Scene> {
        static REPLAYED: OnceLock<Mutex<HashMap<String, BTreeMap<u64, Scene>>>> = OnceLock::new();
        let replayed = REPLAYED.get_or_init(Mutex::default);
        let start = replayed
            .lock()
            .expect("scene cache")
            .get(key)
            .and_then(|scenes| scenes.range(..=frame).next_back().map(|(_, s)| s.clone()));
        let mut scene = match start {
            Some(scene) => scene,
            None => Scene {
                machine: power_on()?,
                pads: read_inputs(path),
                pictures: std::collections::VecDeque::new(),
            },
        };
        let behind = frame - scene.machine.frame;
        scene.run(behind, &[]);
        if frame >= BOOT {
            replayed
                .lock()
                .expect("scene cache")
                .entry(key.to_owned())
                .or_default()
                .insert(frame, scene.clone());
        }
        Some(scene)
    }

    /// Power-on, `pads[f]` (the controller's shift order) to be held in frame `f`.
    pub(crate) fn power_on(pads: Vec<u8>) -> Option<Scene> {
        Some(Scene {
            machine: power_on()?,
            pads,
            pictures: std::collections::VecDeque::new(),
        })
    }

    /// Power-on with level `level` forced at `$D3D8` and Start pressed on frames 300-306.
    pub(crate) fn level(level: u8, frame: u64) -> Option<Scene> {
        let mut machine = power_on()?;
        machine.bus.trace.pokes.push((FORCE_LEVEL, 0x0020, level));
        let pads = (0..=frame)
            .map(|f| if (300..=306).contains(&f) { 0x08 } else { 0 })
            .collect();
        let mut scene = Scene {
            machine,
            pads,
            pictures: std::collections::VecDeque::new(),
        };
        scene.run(frame, &[]);
        Some(scene)
    }

    /// The PRG image.
    pub(crate) fn prg(&self) -> Vec<u8> {
        self.machine.bus.cart.prg.clone()
    }

    /// Frees every slot whose type is not in `kinds` and drops their pending spawns.
    pub(crate) fn isolate(&mut self, kinds: &[u8]) {
        let prg = self.prg();
        let (area, room) = (self.machine.bus.ram[0x400], self.machine.bus.ram[0x402]);
        let wram = &mut self.machine.bus.wram;
        for slot in 0..32 {
            let kind = &mut wram[0x1CC0 + slot];
            if !kinds.contains(kind) {
                *kind = 0;
            }
        }
        for cell in &mut wram[0x1410..0x1510] {
            if *cell & CELL_PENDING == 0 {
                continue;
            }
            let owned = spawn_record_pointer(&prg, area, room, *cell & CELL_RECORD)
                .and_then(|addr| spawn_record(&prg, addr))
                .is_ok_and(|record| kinds.contains(&record.kind));
            if !owned {
                *cell &= CELL_RECORD;
            }
        }
    }

    /// Spawns `record` into main slot `slot` (0-15) the way the spawners write it.
    pub(crate) fn place(&mut self, slot: usize, record: &SpawnRecord, cell: u8) {
        let wram = &mut self.machine.bus.wram;
        let mut slots = Slots::lift(wram);
        let target = &mut slots.main[slot];
        target.kind = record.kind;
        target.body.spawn(record, cell);
        target.brain = Brain::new(target.kind);
        slots.project(wram);
    }

    /// Sets the camera (level x, 1/16 px).
    pub(crate) fn camera(&mut self, x: u16) {
        self.machine.bus.ram[0xCC..0xCE].copy_from_slice(&x.to_be_bytes());
    }

    /// Sets the player's level position (1/16 px).
    pub(crate) fn player(&mut self, x: u16, y: u16) {
        self.machine.bus.ram[0x460..0x462].copy_from_slice(&x.to_be_bytes());
        self.machine.bus.ram[0x462..0x464].copy_from_slice(&y.to_be_bytes());
    }

    /// Runs `frames` more frames holding `pad`, with `peeks` set.
    pub(crate) fn play(&mut self, frames: u64, pad: u8, peeks: &[u16]) -> Vec<PeekEvent> {
        self.script(frames, |_| pad);
        self.run(frames, peeks)
    }

    /// Sets the pads of the next `frames` frames, `pad(i)` for the i-th.
    pub(crate) fn script(&mut self, frames: u64, pad: impl Fn(u64) -> u8) {
        let from = usize::try_from(self.machine.frame + 1).expect("frame fits");
        let to = from + usize::try_from(frames).expect("frames fit");
        if self.pads.len() < to {
            self.pads.resize(to, 0);
        }
        for (i, slot) in self.pads[from..to].iter_mut().enumerate() {
            *slot = pad(u64::try_from(i).expect("small"));
        }
    }

    /// Puts the player `dx` left of `slot`'s actor, walking left and punching for `frames`.
    pub(crate) fn brawl(mut self, slot: usize, dx: u16, frames: u64) -> Scene {
        let wram = &self.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        self.player(x.wrapping_sub(dx), y);
        self.script(frames, |i| if i % 8 < 2 { 0x42 } else { 0x40 });
        self
    }

    /// Runs `frames` more frames with `peeks` set and returns what they recorded.
    pub(crate) fn run(&mut self, frames: u64, peeks: &[u16]) -> Vec<PeekEvent> {
        self.machine.bus.trace.peeks = peeks.to_vec();
        for _ in 0..frames {
            let next = usize::try_from(self.machine.frame + 1).expect("frame fits");
            let log = self
                .machine
                .run_frame(self.pads.get(next).copied().unwrap_or(0));
            if self.pictures.len() == 8 {
                self.pictures.pop_front();
            }
            self.pictures.push_back(Picture {
                frame: self.machine.frame,
                lines: log.lines.iter().map(render_line).collect(),
                splits: log
                    .splits
                    .iter()
                    .map(|(y, a, b)| (*y, render_line(a), render_line(b)))
                    .collect(),
            });
        }
        self.machine.bus.trace.peeks.clear();
        std::mem::take(&mut self.machine.bus.trace.peeked)
    }
}

/// One bracketed call of a routine, for the port to reproduce.
pub(crate) struct Routine<'a> {
    pub begin: &'a PeekEvent,
    pub prg: &'a [u8],
    pub state: State,
    /// Scratch outputs the port writes (`$20-$3F` results), compared too.
    pub writes: Vec<(u16, u8)>,
    /// The accumulator the routine returns, when it returns one.
    pub a: Option<u8>,
    /// The Y register the routine returns, when it returns one.
    pub y: Option<u8>,
    /// The N flag the routine returns, when its callers branch on it.
    pub negative: Option<bool>,
}

impl Routine<'_> {
    /// The slot the routine runs for (`$10`).
    pub(crate) fn slot(&self) -> usize {
        usize::from(self.begin.ram[0x10])
    }

    /// Runs `f` on slot `$10` as the actor loop would (a blank one for the player's calls).
    pub(crate) fn in_slot<T>(
        &mut self,
        f: impl FnOnce(&mut Slot, &mut Work, &mut Cx<'_>) -> T,
    ) -> T {
        let index = self.slot();
        let state = &mut self.state;
        let mut slot = state
            .slots
            .main
            .get_mut(index)
            .map(|slot| {
                let taken = std::mem::take(slot);
                slot.kind = taken.kind;
                taken
            })
            .unwrap_or_default();
        if index < 24 {
            super::slots::load_aliases(
                &mut slot.body,
                index,
                &state.slots,
                &state.effects.projectiles,
            );
        }
        let (mut cx, work) = state.parts(self.prg);
        let result = f(&mut slot, work, &mut cx);
        if index < 24 {
            super::slots::store_aliases(
                &slot.body,
                index,
                &mut state.slots,
                &mut state.effects.projectiles,
            );
            state.slots.main[index] = slot;
        }
        result
    }
}

/// Checks `port` against every call of `entry` (in `bank`) to one of `exits`; returns the count.
pub(crate) fn assert_routine(
    scene: &mut Scene,
    frames: u64,
    (entry, bank): (u16, u8),
    exits: &[u16],
    mut port: impl FnMut(&mut Routine<'_>),
) -> usize {
    let peeks: Vec<u16> = std::iter::once(entry)
        .chain(exits.iter().copied())
        .collect();
    let prg = scene.prg();
    let events = scene.run(frames, &peeks);
    let mut checked = 0;
    for Bracket { begin, end } in brackets(&events, entry, exits) {
        if begin.bank != Some(bank) {
            continue;
        }
        let mut call = Routine {
            begin: &begin,
            prg: &prg,
            state: State::lift(&begin),
            writes: Vec::new(),
            a: None,
            y: None,
            negative: None,
        };
        port(&mut call);
        let (mut ram, wram) = call.state.project(&begin);
        for &(addr, value) in &call.writes {
            ram[usize::from(addr)] = value;
        }
        let claimed: Vec<u16> = call.writes.iter().map(|&(addr, _)| addr).collect();
        if let Some((addr, port, oracle)) = diff_claiming(&begin, (&ram, &wram), &end, &claimed) {
            panic!(
                "${entry:04X} at frame {} (slot {}): ${addr:04X} port {port:02X} oracle {oracle:02X}",
                begin.frame,
                call.slot()
            );
        }
        if let Some(a) = call.a {
            assert_eq!(a, end.cpu.a, "${entry:04X} at frame {}: A", begin.frame);
        }
        if let Some(y) = call.y {
            assert_eq!(y, end.cpu.y, "${entry:04X} at frame {}: Y", begin.frame);
        }
        if let Some(negative) = call.negative {
            assert_eq!(
                negative,
                end.cpu.p & 0x80 != 0,
                "${entry:04X} at frame {}: N",
                begin.frame
            );
        }
        checked += 1;
    }
    checked
}

/// The loop's NMI wait (`$D45C`), the wait's entry and its return after the NMI.
const LOOP_WAIT: u16 = 0xD45C;
/// The loop's first instruction (`$D3F9`), where a run from a port start begins.
const LOOP_START: u16 = 0xD3F9;
/// The IRQ handler's entry and its `RTI`, which the NMI shares.
const IRQ: (u16, u16) = (0xE2A9, 0xE2A8);
/// The NMI handler's entry.
const NMI: u16 = 0xE1D0;
const WAIT: u16 = 0xC232;
const WOKE: u16 = 0xC23F;
/// The reset vector's target (`$E2E6`), which GAME OVER jumps to.
const RESET: u16 = 0xE2E6;

/// The brackets of the routines [`Game::frame`](super::Game::frame) hands out, injected from the oracle.
fn outside_bracket(outside: super::Outside) -> (u16, u16) {
    use super::Outside;
    match outside {
        Outside::SecondClass => SECOND,
    }
}

/// The second-class update's call in the loop (`JSR $CE7F`) and the instruction after it.
const SECOND: (u16, u16) = (0xD3FC, 0xD3FF);
/// Peeks the lockstep passes over unless an [`super::Outside`] bracket asks for them.
const ON_DEMAND: [u16; 2] = [SECOND.0, SECOND.1];

/// The port's whole memory between frames: what its systems project, and the bytes no
/// system holds, which only injections and lifts write.
struct Memory {
    ram: Box<[u8; 2048]>,
    wram: Box<[u8; 8192]>,
}

/// The RAM half of a `tape::MEMORY` snapshot.
const RAM: usize = 0x800;

impl Memory {
    fn zeroed() -> Memory {
        Memory {
            ram: Box::new([0; 2048]),
            wram: Box::new([0; 8192]),
        }
    }

    fn of(peek: &PeekEvent) -> Memory {
        Memory {
            ram: peek.ram.clone(),
            wram: peek.wram.clone(),
        }
    }

    fn restore(snapshot: &[u8; MEMORY]) -> Memory {
        let (ram, wram) = snapshot.split_at(RAM);
        Memory {
            ram: Box::new(ram.try_into().expect("2 KiB of RAM")),
            wram: Box::new(wram.try_into().expect("8 KiB of WRAM")),
        }
    }

    fn snapshot(&self) -> Box<[u8; MEMORY]> {
        let mut snapshot = Box::new([0; MEMORY]);
        snapshot[..RAM].copy_from_slice(&self.ram[..]);
        snapshot[RAM..].copy_from_slice(&self.wram[..]);
        snapshot
    }

    fn lift(&self) -> State {
        State::lift_memory(&self.ram, &self.wram)
    }

    /// Projects `game`, applies `writes`, lifts it back.
    fn inject(&mut self, game: &mut State, writes: &[(u16, u8)]) {
        game.project_into(&mut self.ram, &mut self.wram);
        self.apply(game, writes);
    }

    /// Applies `writes` over the already-projected memory and lifts `game` back, keeping
    /// what memory does not hold: the script and the OAM's marks.
    fn apply(&mut self, game: &mut State, writes: &[(u16, u8)]) {
        for &(at, value) in writes {
            self.write(at, value);
        }
        let script = game.script.take();
        let marks = game.oam.marks;
        *game = self.lift();
        game.script = script;
        game.oam.marks = marks;
    }

    /// [`Memory::inject`] for interrupt bytes, except their stack traffic where the VRAM
    /// queue now holds entries (pushed over it since). The frame's direct VRAM writes,
    /// which memory does not hold, are kept for the port's NMI.
    fn inject_irqs(&mut self, game: &mut State, writes: &[(u16, u8)]) {
        if writes.is_empty() {
            return;
        }
        game.project_into(&mut self.ram, &mut self.wram);
        let queued = 0x101 + u16::from(self.ram[0x181])..0x180;
        let kept: Vec<_> = writes
            .iter()
            .copied()
            .filter(|(at, _)| !queued.contains(at))
            .collect();
        let direct = std::mem::take(&mut game.effects.vram.direct);
        self.apply(game, &kept);
        game.effects.vram.direct = direct;
    }

    fn read(&self, at: u16) -> u8 {
        let at = usize::from(at);
        if at < 0x800 {
            self.ram[at]
        } else {
            self.wram[at - 0x800]
        }
    }

    fn write(&mut self, at: u16, value: u8) {
        let at = usize::from(at);
        if at < 0x800 {
            self.ram[at] = value;
        } else {
            self.wram[at - 0x800] = value;
        }
    }
}

/// The byte at `addr` in a peek's memory.
fn peek_byte(at: &PeekEvent, addr: u16) -> u8 {
    let index = usize::from(addr);
    if index < 0x800 {
        at.ram[index]
    } else {
        at.wram[index - 0x800]
    }
}

/// Keeps the interrupt bytes the loop has not rewritten by `at`, and every stack-page
/// byte: [`Lockstep::inject_irqs_at`]'s rule.
fn drop_rewritten_irqs(writes: &mut Writes, at: &PeekEvent) {
    let last: BTreeMap<u16, u8> = writes.iter().copied().collect();
    writes
        .retain(|&(addr, _)| (0x100..0x200).contains(&addr) || peek_byte(at, addr) == last[&addr]);
}

/// The bytes the raster handlers own.
const OWNED: [u16; 12] = [
    0xD1, 0xD2, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xFC, 0xFD, 0xFF,
];

/// The last value `writes` leaves in each byte the raster handlers own.
fn owned(writes: &[(u16, u8)]) -> BTreeMap<u16, u8> {
    writes
        .iter()
        .copied()
        .filter(|(addr, _)| OWNED.contains(addr))
        .collect()
}

/// The bytes that differ from `begin` to `end`, as tape writes.
fn changes(begin: &PeekEvent, end: &PeekEvent) -> Writes {
    let ram = begin.ram.iter().zip(end.ram.iter());
    let wram = begin.wram.iter().zip(end.wram.iter());
    (0u16..)
        .zip(ram.chain(wram))
        .filter(|(_, (before, after))| before != after)
        .map(|(at, (_, &after))| (at, after))
        .collect()
}

/// What [`run_game`] asks of the oracle, or of a tape recorded from it. Failures panic.
pub(crate) trait Environment {
    /// The first state, and whether its frame's NMI has already run.
    fn start(&mut self) -> (State, bool);
    /// Whether another frame runs.
    fn running(&mut self) -> bool;
    /// The oracle frame of the last event consumed.
    fn frame(&self) -> u64;
    /// The NMI wait's entry (`$C232`).
    fn nmi_wait(&mut self);
    /// Writes the interrupts' bytes since the last injection over `game`.
    fn inject_irqs(&mut self, game: &mut State);
    /// Compares a script frame's RAM at the NMI wait.
    fn check_script(&mut self, game: &State, checked: usize);
    /// Writes the NMI's bytes over `game` (`$C232` to `$C23F`).
    fn nmi(&mut self, game: &mut State);
    /// Runs a routine the port does not on `game`.
    fn outside(&mut self, game: &mut State, outside: super::Outside);
    /// Whether an outside routine waited for the NMI itself since the last call.
    fn waited(&mut self) -> bool;
    /// The next loop frame, lifted.
    fn relift(&mut self) -> State;
    /// GAME OVER's reset (`$E2E6`).
    fn reset(&mut self);
    /// The port's draw hung; the game must hang too.
    fn hung(&mut self);
    /// The loop frame's NMI wait (`$D45C`).
    fn loop_wait(&mut self);
    /// Compares a loop frame's RAM and WRAM.
    fn check(&mut self, game: &State, checked: usize);
    /// The routines injected, by name, with their call counts.
    fn injected(&mut self) -> BTreeMap<&'static str, usize>;
    /// Frames whose lines were compared with the oracle's, and how many of them had a
    /// line only the IRQs' jitter explains.
    fn pictures(&self) -> Pictures {
        Pictures::default()
    }
}

/// The oracle's peeks in order, run a frame at a time, and the port's whole memory.
struct Lockstep {
    scene: Scene,
    events: std::collections::VecDeque<PeekEvent>,
    memory: Memory,
    injected: BTreeMap<&'static str, usize>,
    /// An injected routine waited for an NMI itself, so the port's frame is not the oracle's.
    waited: bool,
    /// An [`super::Outside`] bracket is being read, so the [`ON_DEMAND`] peeks count.
    on_demand: bool,
    /// The bytes the IRQ handler (`$E2A9-$E2A8`), or an NMI outside the wait, changed
    /// since they were last injected; an injected bracket covers the ones inside it.
    irq_writes: Writes,
    /// Which interrupts `irq_writes` holds: the raster IRQs', a lag frame's NMI's.
    irq_kinds: (bool, bool),
    /// How many of `irq_writes` came before the waited NMI: an NMI that runs long lets
    /// the next frame's first IRQ in before the loop wakes.
    irq_at_nmi: Option<usize>,
    /// Each byte those early IRQs changed, as it was before them: the NMI's writes are
    /// the wait-to-wake changes without them.
    early_irq: BTreeMap<u16, u8>,
    irq_check: Option<IrqCheck>,
    /// The last NMI's registers, the port's state then and the oracle's picture of its
    /// frame, compared at its loop's end.
    registers: Option<(super::raster::Registers, State, Picture)>,
    /// Between the NMI wait's entry and its return, where the NMI is the frame's own.
    waiting: bool,
    /// The port's own state and the oracle's peek at `$D3F9`, for a run that starts there.
    first: Option<(State, PeekEvent)>,
    /// The port's power-on state, for a run that starts at the first NMI wait.
    power_on: Option<State>,
    /// The first NMI wait, reached before the run asks for it.
    primed: Option<PeekEvent>,
    /// The oracle frame the run ends at.
    end: u64,
    /// The last NMI wait reached, which the next compare checks against.
    at: Option<PeekEvent>,
    /// The oracle frame of the last peek.
    frame: u64,
    tape: Option<Tape>,
    /// The oracle's overflow flag at each of [`OVERFLOW_PEEKS`] this frame.
    overflow: Vec<(u16, bool)>,
    /// NMI waits reached (`$C232`), which a playtest feeds one pad each.
    waits: usize,
    /// The port's PPU memory, applied from each [`super::Game::nmi`] as the app does.
    ppu: Ppu,
    /// Loop frames compared so far.
    checked: usize,
    /// XORed into the first byte of the next NMI's direct VRAM writes, to prove the check.
    flip_direct: u8,
    pictures: Pictures,
    /// XORed into the port's `$D1` after its raster IRQs, to prove [`Lockstep::compare_irq_ram`].
    flip_irq: u8,
}

/// At the NMI wait: the RAM the port's own raster IRQs leave, the bytes they own as the
/// oracle's IRQs before the wait left them (the rest run during the wait), and whether
/// any raster IRQ ran before it.
struct IrqCheck {
    port: Box<[u8; 2048]>,
    oracle: BTreeMap<u16, u8>,
    raster: bool,
}

/// The PPU memory a peek holds.
fn ppu_of(peek: &PeekEvent) -> Ppu {
    let mut ppu = Ppu::new();
    ppu.nametables[0].copy_from_slice(&peek.vram[..0x400]);
    ppu.nametables[1].copy_from_slice(&peek.vram[0x400..]);
    ppu.palette = peek.palette;
    ppu
}

impl Lockstep {
    const PEEKS: [u16; 12] = [
        LOOP_WAIT,
        WAIT,
        WOKE,
        IRQ.0,
        IRQ.1,
        NMI,
        RESET,
        SECOND.0,
        SECOND.1,
        AFTER_PLAYER_DRAW,
        AFTER_SECOND_CLASS_DRAW,
        WEAPON5_CHARGE,
    ];

    /// A lockstep over `scene` for `frames` frames, from a lift or from `start`;
    /// the scene stops inside the NMI wait like every scene.
    fn new(scene: Scene, frames: u64, start: Option<State>) -> Lockstep {
        let first = start.map(|game| {
            let peek = scene.clone().run(1, &[LOOP_START]).remove(0);
            (game, peek)
        });
        Lockstep {
            end: scene.machine.frame + frames,
            frame: scene.machine.frame,
            scene,
            events: std::collections::VecDeque::new(),
            memory: Memory::zeroed(),
            injected: BTreeMap::new(),
            waited: false,
            on_demand: false,
            irq_writes: Vec::new(),
            irq_kinds: (false, false),
            irq_at_nmi: None,
            early_irq: BTreeMap::new(),
            irq_check: None,
            registers: None,
            waiting: true,
            first,
            power_on: None,
            primed: None,
            at: None,
            tape: None,
            overflow: Vec::new(),
            waits: 0,
            ppu: Ppu::new(),
            checked: 0,
            flip_direct: 0,
            pictures: Pictures::default(),
            flip_irq: 0,
        }
    }

    /// The port's lines for the frame `registers`' NMI began, from its loop's end, equal
    /// the oracle's, line by line. Where they do not, the line must be one of two
    /// others the game shows there, and the frame is counted in `jittered`: the line
    /// from the state at the NMI (`before`), when the loop wrote a byte an IRQ reads
    /// only after the IRQ read it (the port has no CPU time to order them by), or one
    /// whose every drawn field is the line's or a neighbour's (`mixes`), which a write
    /// the IRQs' jitter carries across a line boundary, or a group of writes it splits,
    /// leaves. A mirroring the main thread wrote to the mapper itself before line 0
    /// (`$618D`, the intro's end) is not compared.
    fn compare_lines(
        &mut self,
        registers: &super::raster::Registers,
        before: &State,
        game: &State,
        oracle: &Picture,
    ) {
        let frame = oracle.frame;
        assert_eq!(
            self.scene.machine.bus.ppu.unmodelled,
            0,
            "frame {frame}: the oracle's PPU was written mid-frame in a way its lines do not show (OAM, VRAM or palette, or colour emphasis){}",
            self.context()
        );
        if oracle.lines.is_empty() {
            return;
        }
        let port = super::raster::lines(registers, game);
        if let Some(why) = super::oam::undrawable(&before.oam.page, &before.oam.marks, &port) {
            panic!("frame {frame}: {why}{}", self.context());
        }
        let shown = |mark| before.oam.marks.iter().filter(|&&m| m == mark).count();
        self.pictures.hud += shown(super::oam::Mark::Hud);
        self.pictures.label += shown(super::oam::Mark::Label);
        let direct_mirroring = oracle.lines[0].mirroring != port[0].mirroring;
        let normal = |l: &Line| {
            let mut l = drawn(l);
            if direct_mirroring {
                l.mirroring = 0;
            }
            l
        };
        let port: Vec<Line> = port.iter().map(normal).collect();
        let before: Vec<Line> = super::raster::lines(registers, before)
            .iter()
            .map(normal)
            .collect();
        let states = |y: usize| -> Vec<Line> {
            std::iter::once(&oracle.lines[y])
                .chain(
                    oracle
                        .splits
                        .iter()
                        .filter(|s| s.0 == y)
                        .flat_map(|s| [&s.1, &s.2]),
                )
                .map(normal)
                .collect()
        };
        let (mut jittered, mut raced) = (0, false);
        for (y, line) in port.iter().enumerate() {
            let own = states(y);
            if own.contains(line) {
                continue;
            }
            let near: Vec<Line> = [y.wrapping_sub(1), y, y + 1]
                .into_iter()
                .filter(|&n| n < 240)
                .flat_map(states)
                .collect();
            if own.contains(&before[y]) {
                raced = true;
                continue;
            }
            if mixes(line, &own, &near) {
                jittered += 1;
                continue;
            }
            panic!(
                "frame {frame} ({} loop frames in): line {y}: port {line:02X?} oracle {:02X?} splits {:02X?}{}",
                self.checked,
                own[0],
                &own[1..],
                self.context()
            );
        }
        assert!(
            jittered <= MOST_JITTERED,
            "frame {frame}: {jittered} lines only the IRQs' jitter explains{}",
            self.context()
        );
        let pictures = &mut self.pictures;
        pictures.frames += 1;
        pictures.jittered += usize::from(jittered > 0);
        pictures.lines += jittered;
        pictures.most = pictures.most.max(jittered);
        pictures.raced += usize::from(raced);
    }

    /// The bytes the raster handlers own, as the port's own [`super::raster::run_irqs`]
    /// left them from the NMI wait, equal what the oracle's IRQs of the frame left: an
    /// IRQ the port does not model fails here.
    fn compare_irq_ram(&mut self, game: &State, woke: &PeekEvent, before_nmi: usize) {
        let Some(IrqCheck {
            port,
            oracle: mut want,
            raster,
        }) = self.irq_check.take()
        else {
            return;
        };
        let (during, lag) = self.irq_kinds;
        if lag || !(raster || during) {
            return;
        }
        want.extend(owned(&self.irq_writes[..before_nmi]));
        let mut port = port;
        port[0xD1] ^= std::mem::take(&mut self.flip_irq);
        self.pictures.irqs += 1;
        for (&addr, &value) in &want {
            assert_eq!(
                port[usize::from(addr)],
                value,
                "frame {}: ${addr:04X} after the raster IRQs ($D3 = {:02X}): port {:02X} oracle {value:02X}{}",
                woke.frame,
                game.scroll.room[3],
                port[usize::from(addr)],
                self.context()
            );
        }
    }

    /// The port's PPU memory against the oracle's at the NMI's return, palette
    /// entries masked `& 0x3F` (the oracle stores them so).
    fn compare_vram(&self, woke: &PeekEvent) {
        let port = self.ppu.nametables.as_flattened();
        if let Some(at) = (0..port.len()).find(|&at| port[at] != woke.vram[at]) {
            panic!(
                "frame {} ({} loop frames in): VRAM table {} offset ${:03X} port {:02X} oracle {:02X}{}",
                woke.frame,
                self.checked,
                ["A", "B"][at / 0x400],
                at % 0x400,
                port[at],
                woke.vram[at],
                self.context()
            );
        }
        let palette = self.ppu.palette.iter().zip(&woke.palette);
        if let Some((at, (port, oracle))) = palette
            .enumerate()
            .find(|(_, (port, oracle))| *port & 0x3F != *oracle & 0x3F)
        {
            panic!(
                "frame {} ({} loop frames in): palette ${at:02X} port {:02X} oracle {:02X}{}",
                woke.frame,
                self.checked,
                port & 0x3F,
                oracle & 0x3F,
                self.context()
            );
        }
    }

    /// The pads a playtest feeds the oracle, in the controller's shift order, if one runs.
    fn playtest(&self) -> Option<&[u8]> {
        let playtest = self.scene.machine.bus.trace.playtest.as_ref();
        playtest.map(|playtest| &playtest.pads[..])
    }

    /// Where a playtest is when a compare fails: its time and the lag NMIs so far.
    fn context(&self) -> String {
        if self.playtest().is_none() {
            return String::new();
        }
        let lag = self
            .injected
            .get("NMI $E1D0 outside the wait (a lag frame)")
            .copied()
            .unwrap_or(0);
        let (waits, seconds) = (self.waits, self.waits / 60);
        format!(" at {seconds}s of play (pad {waits}), {lag} lag NMIs so far")
    }

    fn record(&mut self, event: Event) {
        if let Some(tape) = &mut self.tape {
            tape.events.push((self.frame, event));
        }
    }

    fn next(&mut self) -> PeekEvent {
        loop {
            let event = self.pop();
            if OVERFLOW_PEEKS.contains(&event.pc) {
                if event.pc != WEAPON5_CHARGE || event.bank == Some(5) {
                    self.overflow.push((event.pc, event.cpu.p & 0x40 != 0));
                }
                continue;
            }
            if !self.on_demand && ON_DEMAND.contains(&event.pc) {
                continue;
            }
            if [IRQ.0, NMI].contains(&event.pc) {
                self.interrupt(&event);
            } else if event.pc != IRQ.1 {
                self.waiting = (self.waiting || event.pc == WAIT) && event.pc != WOKE;
                self.waits += usize::from(event.pc == WAIT);
                return event;
            }
        }
    }

    /// Runs the oracle through the interrupt handler entered at `entry` (and any nested
    /// in it) and keeps what it changed, unless it is the NMI the wait expects.
    fn interrupt(&mut self, entry: &PeekEvent) {
        let exit = loop {
            let event = self.pop();
            if event.pc == IRQ.1 {
                break event;
            }
            assert!(
                [IRQ.0, NMI].contains(&event.pc),
                "frame {}: ${:04X} inside an interrupt",
                entry.frame,
                event.pc
            );
            self.interrupt(&event);
        };
        let name = match entry.pc {
            NMI if self.waiting => {
                self.irq_at_nmi.get_or_insert(self.irq_writes.len());
                return;
            }
            NMI => {
                self.irq_kinds.1 = true;
                "NMI $E1D0 outside the wait (a lag frame)"
            }
            _ => {
                if self.irq_at_nmi.is_some() {
                    for (addr, _) in changes(entry, &exit) {
                        self.early_irq
                            .entry(addr)
                            .or_insert_with(|| peek_byte(entry, addr));
                    }
                } else {
                    self.irq_kinds.0 = true;
                }
                "IRQ handler $E2A9 (raster splits)"
            }
        };
        let pushed = (1..=3).map(|i| 0x100 + u16::from(entry.cpu.s.wrapping_add(i)));
        let pushed = pushed.map(|at| (at, entry.ram[usize::from(at)]));
        self.irq_writes.extend(pushed.chain(changes(entry, &exit)));
        *self.injected.entry(name).or_default() += 1;
    }

    /// Writes the interrupts' bytes over `game`, skipping a byte whose value at the injection point differs from the last value an interrupt wrote there; the stack page is always injected.
    fn inject_irqs_at(&mut self, game: &mut State, at: &PeekEvent) {
        if let Some((registers, before, picture)) = self.registers.take() {
            self.compare_lines(&registers, &before, game, &picture);
        }
        let mut writes = std::mem::take(&mut self.irq_writes);
        self.irq_at_nmi = None;
        self.early_irq.clear();
        drop_rewritten_irqs(&mut writes, at);
        let (raster, lag) = std::mem::take(&mut self.irq_kinds);
        self.irq_check = (!lag).then(|| {
            let (mut ram, mut wram) = (self.memory.ram.clone(), self.memory.wram.clone());
            game.project_into(&mut ram, &mut wram);
            let mut oracle: BTreeMap<u16, u8> = OWNED
                .iter()
                .map(|&addr| (addr, ram[usize::from(addr)]))
                .collect();
            oracle.extend(owned(&writes));
            let mut port = game.clone();
            super::raster::run_irqs(&mut port);
            port.project_into(&mut ram, &mut wram);
            IrqCheck {
                port: ram,
                oracle,
                raster,
            }
        });
        self.memory.inject_irqs(game, &writes);
        self.record(Event::Irq(writes));
    }

    fn pop(&mut self) -> PeekEvent {
        let from = self.scene.machine.frame;
        while self.events.is_empty() {
            assert!(
                self.scene.machine.frame - from < 300,
                "the oracle reached no peek in frames {from}-{}: hung?",
                self.scene.machine.frame
            );
            self.events.extend(self.scene.run(1, &Self::PEEKS));
        }
        let event = self.events.pop_front().expect("an event");
        self.frame = event.frame;
        event
    }

    fn expect(&mut self, pc: u16) -> PeekEvent {
        let event = self.next();
        assert_eq!(
            event.pc,
            pc,
            "frame {}: the oracle reached ${:04X} where the port expects ${pc:04X}{}",
            event.frame,
            event.pc,
            self.context()
        );
        event
    }

    /// The oracle's next loop frame: the memory there, lifted.
    fn lift_next(&mut self) -> State {
        let mut event = self.next();
        while event.pc != LOOP_WAIT {
            event = self.next();
        }
        self.memory = Memory::of(&event);
        self.ppu = ppu_of(&event);
        self.irq_writes.clear();
        self.irq_at_nmi = None;
        self.early_irq.clear();
        self.irq_kinds = (false, false);
        self.registers = None;
        self.memory.lift()
    }
}

impl Environment for Lockstep {
    fn start(&mut self) -> (State, bool) {
        if let Some(game) = self.power_on.take() {
            let mut wait = self.next();
            while wait.pc != WAIT {
                wait = self.next();
            }
            self.memory = Memory::of(&wait);
            self.ppu = ppu_of(&wait);
            self.primed = Some(wait);
            return (game, true);
        }
        if let Some((game, peek)) = self.first.take() {
            while self.next().pc != WOKE {}
            self.memory = Memory::of(&peek);
            self.ppu = ppu_of(&peek);
            return (game, true);
        }
        let game = self.lift_next();
        self.scene.machine.bus.trace.pokes.clear();
        self.record(Event::Start(self.memory.snapshot()));
        (game, false)
    }

    fn running(&mut self) -> bool {
        let running = self.scene.machine.frame < self.end;
        if !running {
            self.record(Event::End);
        }
        running
    }

    fn frame(&self) -> u64 {
        self.frame
    }

    fn nmi_wait(&mut self) {
        let wait = match self.primed.take() {
            Some(wait) => wait,
            None => self.expect(WAIT),
        };
        self.at = Some(wait);
    }

    fn inject_irqs(&mut self, game: &mut State) {
        let at = self.at.take().expect("an NMI wait to inject at");
        self.inject_irqs_at(game, &at);
        self.at = Some(at);
    }

    fn check_script(&mut self, game: &State, checked: usize) {
        let wait = self.at.take().expect("the NMI wait");
        game.project_into(&mut self.memory.ram, &mut self.memory.wram);
        if let Some((addr, port, want)) = diff(&wait, (&self.memory.ram, &wait.wram), &wait) {
            panic!(
                "script frame {} ({checked} in): ${addr:04X} port {port:02X} oracle {want:02X}{}",
                wait.frame,
                self.context()
            );
        }
        self.record(Event::ScriptCheck(script_hash(&wait.ram)));
        self.at = Some(wait);
    }

    fn nmi(&mut self, game: &mut State) {
        let wait = self.at.take().expect("the NMI wait");
        let woke = self.expect(WOKE);
        let before_nmi = self.irq_at_nmi.take().unwrap_or(self.irq_writes.len());
        self.compare_irq_ram(game, &woke, before_nmi);
        self.overflow.clear();
        OVERFLOW.with_borrow_mut(Vec::clear);
        self.irq_writes.drain(..before_nmi);
        self.irq_kinds = (!self.irq_writes.is_empty(), false);
        let mut writes: BTreeMap<u16, u8> = changes(&wait, &woke).into_iter().collect();
        for (addr, before) in std::mem::take(&mut self.early_irq) {
            if peek_byte(&wait, addr) == before {
                writes.remove(&addr);
            } else {
                writes.insert(addr, before);
            }
        }
        let writes: Writes = writes.into_iter().collect();
        let pads = self.playtest();
        let pad = pads.and_then(|pads| pads.get(self.waits - 1));
        let mut app = game.clone();
        let direct = !app.effects.vram.direct.is_empty();
        let mut vblank = app.nmi(pad.map_or(0, |pad| pad.reverse_bits()));
        if direct {
            vblank.writes[0].bytes[0] ^= std::mem::take(&mut self.flip_direct);
        }
        self.ppu.apply(&vblank.writes, app.scroll.mirroring);
        self.memory.inject(game, &writes);
        self.record(Event::Nmi(writes));
        if self.playtest().is_some() {
            let (app, world) = (&app.world, &game.world);
            for (field, same) in [
                ("rng", app.rng == world.rng),
                ("pad", app.pad == world.pad),
                ("pressed", app.pressed == world.pressed),
                ("frame", app.frame == world.frame),
                ("frame_odd", app.frame_odd == world.frame_odd),
            ] {
                assert!(
                    same,
                    "frame {}: the app's NMI differs: {field}{}",
                    woke.frame,
                    self.context()
                );
            }
        }
        self.compare_vram(&woke);
        let picture = self
            .scene
            .pictures
            .iter()
            .find(|p| p.frame == woke.frame)
            .expect("the NMI's frame among the oracle's last pictures")
            .clone();
        let unread =
            self.registers
                .replace((super::raster::Registers::of(game), game.clone(), picture));
        assert!(
            unread.is_none(),
            "frame {}: the last NMI's lines were never compared",
            woke.frame
        );
    }

    fn outside(&mut self, game: &mut State, outside: super::Outside) {
        let (entry, exit) = outside_bracket(outside);
        self.on_demand = true;
        let begin = self.expect(entry);
        self.inject_irqs_at(game, &begin);
        let mut waited = false;
        let mut end = self.next();
        while end.pc != exit {
            waited |= end.pc == WAIT;
            end = self.next();
        }
        self.on_demand = false;
        self.irq_writes.clear();
        self.irq_at_nmi = None;
        self.early_irq.clear();
        self.irq_kinds = (false, false);
        self.waited |= waited;
        let writes = changes(&begin, &end);
        self.memory.inject(game, &writes);
        self.record(Event::Outside {
            kind: outside as u8,
            waited,
            writes,
        });
        *self.injected.entry(name(outside)).or_default() += 1;
    }

    fn waited(&mut self) -> bool {
        std::mem::take(&mut self.waited)
    }

    fn relift(&mut self) -> State {
        let game = self.lift_next();
        self.record(Event::Relift(self.memory.snapshot()));
        game
    }

    fn reset(&mut self) {
        self.expect(RESET);
        self.record(Event::Reset);
    }

    /// Runs the oracle to the end and checks it never waits for an NMI again.
    fn hung(&mut self) {
        let frames = self.end - self.scene.machine.frame;
        let later = self.scene.run(frames, &[LOOP_WAIT, WAIT]);
        let waited = self
            .events
            .iter()
            .chain(&later)
            .find(|e| [LOOP_WAIT, WAIT].contains(&e.pc));
        assert!(
            waited.is_none(),
            "the port hung, the oracle went on: {:?}",
            waited.map(|e| (e.frame, e.pc))
        );
        self.record(Event::Hung);
    }

    fn loop_wait(&mut self) {
        self.at = Some(self.expect(LOOP_WAIT));
    }

    fn check(&mut self, game: &State, checked: usize) {
        let oracle = self.at.take().expect("the loop's NMI wait");
        let memory = &mut self.memory;
        game.project_into(&mut memory.ram, &mut memory.wram);
        if let Some((addr, port, want)) = diff(&oracle, (&memory.ram, &memory.wram), &oracle) {
            panic!(
                "frame {} ({checked} loop frames in): ${addr:04X} port {port:02X} oracle {want:02X}{}",
                oracle.frame,
                self.context()
            );
        }
        let port = OVERFLOW.with_borrow_mut(std::mem::take);
        assert_eq!(
            port,
            std::mem::take(&mut self.overflow),
            "frame {} ({checked} loop frames in): the overflow flag at (pc, V), port then oracle",
            oracle.frame
        );
        self.record(Event::Check(loop_hash(&oracle.ram, &oracle.wram)));
        self.checked += 1;
    }

    fn injected(&mut self) -> BTreeMap<&'static str, usize> {
        std::mem::take(&mut self.injected)
    }

    fn pictures(&self) -> Pictures {
        self.pictures
    }
}

/// A recorded run fed back through the injections the lockstep made, each compare
/// checked against its recorded hash.
struct TapePlayer<'a> {
    name: &'a str,
    tape: &'a Tape,
    /// The index of the next event.
    next: usize,
    memory: Memory,
    injected: BTreeMap<&'static str, usize>,
    waited: bool,
}

impl<'a> TapePlayer<'a> {
    fn take(&mut self) -> &'a Event {
        let tape = self.tape;
        let Some((_, event)) = tape.events.get(self.next) else {
            panic!(
                "tape {} is stale at event {}: it ends there; re-record with ROTJ_RECORD_TAPE=1",
                self.name, self.next
            );
        };
        self.next += 1;
        event
    }

    fn frame(&self) -> u64 {
        self.tape.events[self.next - 1].0
    }

    fn stale(&self, wanted: &str, found: &Event) -> ! {
        panic!(
            "tape {} is stale at event {} (frame {}): expected {wanted}, found {}; \
             re-record with ROTJ_RECORD_TAPE=1",
            self.name,
            self.next - 1,
            self.frame(),
            found.name()
        );
    }

    fn differs(&self, what: &str, checked: usize) -> ! {
        panic!(
            "tape {}: {what} {checked} (TAS frame {}): state hash differs; \
             byte diff: ROTJ_REQUIRE_ROM=1 cargo test -p rotj-core {}",
            self.name,
            self.frame(),
            self.name
        );
    }
}

impl Environment for TapePlayer<'_> {
    fn start(&mut self) -> (State, bool) {
        match self.take() {
            Event::Start(snapshot) => {
                self.memory = Memory::restore(snapshot);
                (self.memory.lift(), false)
            }
            other => self.stale("Start", other),
        }
    }

    fn running(&mut self) -> bool {
        let end = matches!(self.tape.events.get(self.next), Some((_, Event::End)));
        self.next += usize::from(end);
        !end
    }

    fn frame(&self) -> u64 {
        TapePlayer::frame(self)
    }

    fn nmi_wait(&mut self) {}

    fn inject_irqs(&mut self, game: &mut State) {
        match self.take() {
            Event::Irq(writes) => self.memory.inject_irqs(game, writes),
            other => self.stale("Irq", other),
        }
    }

    fn check_script(&mut self, game: &State, checked: usize) {
        let hash = match self.take() {
            Event::ScriptCheck(hash) => *hash,
            other => self.stale("ScriptCheck", other),
        };
        game.project_into(&mut self.memory.ram, &mut self.memory.wram);
        if script_hash(&self.memory.ram) != hash {
            self.differs("script frame", checked);
        }
    }

    fn nmi(&mut self, game: &mut State) {
        match self.take() {
            Event::Nmi(writes) => self.memory.inject(game, writes),
            other => self.stale("Nmi", other),
        }
    }

    fn outside(&mut self, game: &mut State, outside: super::Outside) {
        self.inject_irqs(game);
        match self.take() {
            Event::Outside {
                kind,
                waited,
                writes,
            } if *kind == outside as u8 => {
                self.waited |= waited;
                self.memory.inject(game, writes);
            }
            other => self.stale(name(outside), other),
        }
        *self.injected.entry(name(outside)).or_default() += 1;
    }

    fn waited(&mut self) -> bool {
        std::mem::take(&mut self.waited)
    }

    fn relift(&mut self) -> State {
        match self.take() {
            Event::Relift(snapshot) => {
                self.memory = Memory::restore(snapshot);
                self.memory.lift()
            }
            other => self.stale("Relift", other),
        }
    }

    fn reset(&mut self) {
        match self.take() {
            Event::Reset => {}
            other => self.stale("Reset", other),
        }
    }

    fn hung(&mut self) {
        match self.take() {
            Event::Hung => {}
            other => self.stale("Hung", other),
        }
    }

    fn loop_wait(&mut self) {}

    fn check(&mut self, game: &State, checked: usize) {
        let hash = match self.take() {
            Event::Check(hash) => *hash,
            other => self.stale("Check", other),
        };
        game.project_into(&mut self.memory.ram, &mut self.memory.wram);
        if loop_hash(&self.memory.ram, &self.memory.wram) != hash {
            self.differs("loop frame", checked);
        }
    }

    fn injected(&mut self) -> BTreeMap<&'static str, usize> {
        std::mem::take(&mut self.injected)
    }
}

/// Replays the tape `name` through the port alone: the game [`assert_game`] recorded it
/// from, checked at every compare. The interrupts' counts are not on the tape, so
/// `injected` holds only the [`super::Outside`] routines.
pub(crate) fn replay(name: &str, tape: &Tape, prg: &[u8], chr: &[u8]) -> GameRun {
    let mut player = TapePlayer {
        name,
        tape,
        next: 0,
        memory: Memory::zeroed(),
        injected: BTreeMap::new(),
        waited: false,
    };
    let run = run_game(&mut player, prg, chr);
    if let Some((frame, event)) = tape.events.get(player.next) {
        panic!(
            "tape {name} is stale at event {} (frame {frame}): the run ended, found {}; \
             re-record with ROTJ_RECORD_TAPE=1",
            player.next,
            event.name()
        );
    }
    run
}

fn name(outside: super::Outside) -> &'static str {
    use super::Outside;
    match outside {
        Outside::SecondClass => "unported second-class update $CE7F",
    }
}

/// Which script a [`Start`] follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartKind {
    /// A room change (`$BA = 1`, bank 13 `$B7EC`).
    RoomChange,
    /// A next-area load (`$BA = 3`, `$D513`), boss fights included.
    NextArea,
}

/// Where a random-play run may begin: the oracle frame of the first loop frame
/// after a room change or next-area load, and the port's `$0400`/`$0402` there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Start {
    pub kind: StartKind,
    pub frame: u64,
    pub area: u8,
    pub room: u8,
}

/// What [`assert_game`] compared.
#[derive(Debug, Default)]
pub(crate) struct GameRun {
    /// The room changes and next-area loads the port ran, each as a start point.
    pub starts: Vec<Start>,
    /// Loop frames compared at their NMI wait.
    pub frames: usize,
    /// Room changes run by the port and compared at the next loop frame.
    pub room_changes: usize,
    /// Next-area loads (`$BA = 3`, `$D513`) run by the port and compared at the next loop frame.
    pub next_area: usize,
    /// Stage ends (`$940C`'s last frame, which waits for the NMI itself) run by the port.
    pub stage_ends: usize,
    /// Full-meter power-ups (`$BA = 5`, `$D58F`) run by the port.
    pub power_ups: usize,
    /// Pauses (bank 8 `$9C37`) run by the port and compared at the next loop frame.
    pub pauses: usize,
    /// Endings (`$BA = 6`, `$D967`) run by the port.
    pub endings: usize,
    /// Frames of the cutscene (`$98BD`), the pause and the ending whose RAM was compared
    /// at their NMI wait.
    pub script_frames: usize,
    /// Frames re-lifted because an injected routine waited for an NMI itself.
    pub relifted: usize,
    /// Routines injected from the oracle, by name, with their call counts.
    pub injected: BTreeMap<&'static str, usize>,
    /// The port reset the game after GAME OVER, at the oracle's own reset; the run ends there.
    pub reset: bool,
    /// A draw hung the port's game and the oracle's loop never waited for an NMI again.
    pub hung: bool,
    pub pictures: Pictures,
}

/// What a lockstep's picture checks compared.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Pictures {
    /// Frames whose 240 lines matched the oracle's ([`Lockstep::compare_lines`]).
    pub frames: usize,
    /// Of those, frames with a line only the IRQs' jitter explains, those lines, and the
    /// most in one frame.
    pub jittered: usize,
    pub lines: usize,
    pub most: usize,
    /// Frames with a line drawn from the NMI's state, a byte the loop wrote after an IRQ read it.
    pub raced: usize,
    /// Frames whose raster IRQ bytes were compared ([`Lockstep::compare_irq_ram`]).
    pub irqs: usize,
    /// The HUD's marked gauge and label entries the compared frames showed.
    pub hud: usize,
    pub label: usize,
}

/// Runs the port's [`Game::frame`](super::Game::frame) chained from one lift at the
/// loop's NMI wait, `frames` oracle frames on: every loop frame's RAM and WRAM must
/// equal the oracle's at `$D45C` (scratch aside). The NMI's writes and the routines
/// in [`super::Outside`] are injected; a [`super::Script`] is compared once, at the
/// next loop frame. The scene's pokes apply once, at the lift.
pub(crate) fn assert_game(scene: Scene, frames: u64) -> GameRun {
    assert_game_scene(scene, frames).0
}

/// [`assert_game`] and the lockstep's own final [`Scene`], for reading the state it ended in.
pub(crate) fn assert_game_scene(scene: Scene, frames: u64) -> (GameRun, Scene) {
    let (prg, chr) = (scene.prg(), scene.machine.bus.cart.chr.clone());
    let mut lock = Lockstep::new(scene, frames, None);
    let run = run_game(&mut lock, &prg, &chr);
    (run, lock.scene)
}

/// [`assert_game`], and the tape of what it injected and compared.
pub(crate) fn assert_game_recording(scene: Scene, frames: u64) -> (GameRun, Tape) {
    let (prg, chr) = (scene.prg(), scene.machine.bus.cart.chr.clone());
    let mut lock = Lockstep::new(scene, frames, None);
    lock.tape = Some(Tape::default());
    let run = run_game(&mut lock, &prg, &chr);
    (run, lock.tape.take().expect("the tape"))
}

/// [`assert_game`] with the first byte of the first direct VRAM write the port's NMI
/// hands out flipped, which the VRAM compare must catch.
/// [`assert_game`] with the port's `$D1` flipped after its first raster IRQs, which
/// the IRQ compare must catch.
pub(crate) fn assert_game_with_a_flipped_raster_byte(scene: Scene, frames: u64) -> GameRun {
    let (prg, chr) = (scene.prg(), scene.machine.bus.cart.chr.clone());
    let mut lock = Lockstep::new(scene, frames, None);
    lock.flip_irq = 0xFF;
    run_game(&mut lock, &prg, &chr)
}

pub(crate) fn assert_game_with_a_flipped_direct_write(scene: Scene, frames: u64) -> GameRun {
    let (prg, chr) = (scene.prg(), scene.machine.bus.cart.chr.clone());
    let mut lock = Lockstep::new(scene, frames, None);
    lock.flip_direct = 0xFF;
    run_game(&mut lock, &prg, &chr)
}

/// [`assert_game`] from `start` instead of a lift: the port's own state at the
/// scene's next `$D3F9`, the oracle's memory there under it.
pub(crate) fn assert_game_from(scene: Scene, frames: u64, start: Option<State>) -> GameRun {
    let (prg, chr) = (scene.prg(), scene.machine.bus.cart.chr.clone());
    run_game(&mut Lockstep::new(scene, frames, start), &prg, &chr)
}

/// The port from power-on ([`State::power_on`], `forced` as a level poked at `$D3D8`)
/// against the oracle's `scene`, also at power-on, for `frames` frames: every NMI wait of
/// the boot compared as a script frame (RAM, the driver's init and the PPU mask taken from
/// the oracle), then the loop as [`assert_game`].
pub(crate) fn assert_boot(scene: Scene, frames: u64, forced: Option<u8>) -> GameRun {
    let (prg, chr) = (scene.prg(), scene.machine.bus.cart.chr.clone());
    let mut lock = Lockstep::new(scene, frames, None);
    lock.power_on = Some(State::power_on(forced));
    run_game(&mut lock, &prg, &chr)
}

/// The start of `level` the app's tests set scenes up from (`Play::start`:
/// `room::start`, its first page taken for the nametables) and the oracle's:
/// [`Scene::level`] stopped in the NMI wait before the level's first loop frame, no
/// longer forced (the poke would force it at every next-area load), fed `pads` (what
/// `Play::step` takes, the game's bit order `$F6`), one per NMI wait. The bytes that
/// depend on when Start was pressed are pinned to the app's ([`StartBytes`]), and the
/// bytes the raster IRQ set since are injected into the app's, as the lockstep injects
/// the IRQ's.
pub(crate) fn fresh_start(level: u8, pads: &[u8]) -> Option<(Scene, State)> {
    const PRESSED: u64 = 300;
    let mut scene = Scene::level(level, PRESSED)?;
    let events = scene.run(
        FRESH_START[usize::from(level)] - PRESSED,
        &[IRQ.0, IRQ.1, NMI],
    );
    let pokes = &mut scene.machine.bus.trace.pokes;
    pokes.retain(|&(at, ..)| at != FORCE_LEVEL);
    let prg = scene.prg();
    let mut start = super::room::start(&prg, &scene.machine.bus.cart.chr, usize::from(level));
    start.effects.vram.direct.clear();
    let step = |rng| {
        let mut rng = super::world::Rng(rng);
        rng.next_frame();
        rng.0
    };
    let shifted = pads.iter().map(|pad| pad.reverse_bits()).collect();
    let mut playtest = Playtest::new(shifted, (WAIT, WOKE), super::RNG_SPINS, step);
    let world = &start.world;
    playtest.start = Some(StartBytes {
        rng: world.rng,
        frame: world.frame,
        frame_odd: world.frame_odd,
        oam: start.oam.page,
        oam_next: start.oam.next,
    });
    scene.machine.bus.trace.playtest = Some(playtest);
    let peek = scene.clone().run(1, &[LOOP_START]).remove(0);
    let mut memory = Memory::of(&peek);
    let irq: Writes = raster_irq_bytes(&events)
        .into_iter()
        .filter(|&(at, value)| !(0x100..0x200).contains(&at) && memory.read(at) == value)
        .collect();
    memory.inject(&mut start, &irq);
    Some((scene, start))
}

/// The bytes the raster IRQ (`$E2A9`, nested interrupts included) last changed in
/// `events`, peeked at the interrupts' entries and their `RTI`.
fn raster_irq_bytes(events: &[PeekEvent]) -> BTreeMap<u16, u8> {
    let mut entries = Vec::new();
    let mut last = BTreeMap::new();
    for event in events {
        if event.pc != IRQ.1 {
            entries.push(event);
        } else if let Some(entry) = entries.pop().filter(|entry| entry.pc == IRQ.0) {
            last.extend(changes(entry, event));
        }
    }
    last
}

/// A session the app recorded from power-on (`pads`, one per `Play::step`, the game's
/// bit order `$F6`), replayed: the oracle fed the same pad at each NMI wait, its random
/// bytes pinned to the app's ([`super::RNG_SPINS`] a wait), `level` (if not 0) forced
/// at `$D3D8` as the app forces `ROTJ_LEVEL`, then [`assert_boot`] over as many frames.
pub(crate) fn assert_playtest(level: u8, pads: &[u8]) -> Option<GameRun> {
    let scene = playtest_scene(level, pads)?;
    let frames = u64::try_from(pads.len()).expect("pads fit in u64");
    Some(assert_boot(scene, frames, (level != 0).then_some(level)))
}

/// The oracle at power-on with the playtest of [`assert_playtest`] set.
pub(crate) fn playtest_scene(level: u8, pads: &[u8]) -> Option<Scene> {
    let mut scene = Scene::power_on(Vec::new())?;
    if level != 0 {
        scene
            .machine
            .bus
            .trace
            .pokes
            .push((FORCE_LEVEL, 0x0020, level));
    }
    let step = |rng| {
        let mut rng = super::world::Rng(rng);
        rng.next_frame();
        rng.0
    };
    let shifted = pads.iter().map(|pad| pad.reverse_bits()).collect();
    let playtest = Playtest::new(shifted, (WAIT, WOKE), super::RNG_SPINS, step);
    scene.machine.bus.trace.playtest = Some(playtest);
    Some(scene)
}

/// The port's game loop from `env`'s start to its end, every injection and compare
/// asked of `env`.
pub(crate) fn run_game(env: &mut impl Environment, prg: &[u8], chr: &[u8]) -> GameRun {
    let (mut game, mut started) = env.start();
    let mut run = GameRun::default();
    let mut pending: Option<StartKind> = None;
    while env.running() {
        if !std::mem::take(&mut started) {
            env.nmi_wait();
            env.inject_irqs(&mut game);
            let per_frame = match &game.script {
                Some(super::Script::NextArea(load)) => load.in_cutscene(),
                Some(super::Script::Pause(pause)) => !pause.done(),
                Some(super::Script::Ending(_) | super::Script::Boot(_)) => true,
                _ => false,
            };
            if per_frame {
                env.check_script(&game, run.script_frames);
                run.script_frames += 1;
            }
            env.nmi(&mut game);
        }
        let scripted = game.script.is_some();
        game.frame(prg, chr, &mut |outside, game| env.outside(game, outside));
        if game.reset() {
            env.reset();
            run.reset = true;
            break;
        }
        if game.oam.hung {
            env.hung();
            run.hung = true;
            break;
        }
        if env.waited() {
            run.relifted += 1;
            game = env.relift();
            continue;
        }
        if let Some(script) = &game.script {
            if !scripted {
                pending = match script {
                    super::Script::RoomChange(_) => {
                        run.room_changes += 1;
                        Some(StartKind::RoomChange)
                    }
                    super::Script::NextArea(_) => {
                        run.next_area += 1;
                        Some(StartKind::NextArea)
                    }
                    super::Script::StageEnded => {
                        run.stage_ends += 1;
                        None
                    }
                    super::Script::PowerUp(_) => {
                        run.power_ups += 1;
                        None
                    }
                    super::Script::Pause(_) => {
                        run.pauses += 1;
                        None
                    }
                    super::Script::Ending(_) => {
                        run.endings += 1;
                        None
                    }
                    super::Script::Boot(_) => None,
                };
            }
            continue;
        }
        env.loop_wait();
        env.inject_irqs(&mut game);
        env.check(&game, run.frames);
        run.frames += 1;
        if let Some(kind) = pending.take() {
            run.starts.push(Start {
                kind,
                frame: env.frame(),
                area: game.world.area,
                room: game.world.room,
            });
        }
    }
    run.injected = env.injected();
    run.pictures = env.pictures();
    run
}

/// Routines each handler family calls on the stage 1-1 tour (`docs/ACTORS.md` §9).
pub(crate) fn census() -> Option<BTreeMap<u16, BTreeSet<u16>>> {
    let mut census = BTreeMap::new();
    let mut tour = |scene: &mut Scene, frames: u64| survey(scene, frames, &mut census);
    for (inputs, end) in [("route-inputs.csv", 1000), ("stage1-inputs.csv", 1027)] {
        let mut scene = Scene::replay(inputs, BOOT)?;
        while scene.machine.frame < end {
            tour(&mut scene, 40);
            if scene.machine.frame % 120 != 0 {
                continue;
            }
            for slot in 0..16 {
                let wram = &scene.machine.bus.wram;
                if wram[0x1CC0 + slot] == 0 {
                    continue;
                }
                let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
                let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
                for dx in [0, 0x200] {
                    let mut near = scene.clone();
                    near.player(x.wrapping_sub(dx), y);
                    tour(&mut near, 120);
                }
            }
        }
    }
    Some(census)
}

/// Adds to `census` the routines each handler family calls over the next `frames` frames:
/// every `JSR`/`JMP` to `$A000+` between a dispatch and the loop tail, bank switches aside.
pub(crate) fn survey(scene: &mut Scene, frames: u64, census: &mut BTreeMap<u16, BTreeSet<u16>>) {
    const DISPATCH: [u16; 2] = [0xCF0A, 0xCEAE];
    const TAILS: [u16; 2] = [0xCEDE, 0xCE88];
    const BANK_SWITCH: u16 = 0xC229;
    let prg = scene.prg();
    let vector = |at: usize| u16::from_le_bytes([prg[prg.len() - at], prg[prg.len() - at + 1]]);
    let interrupts = [vector(6), vector(2)];
    scene.machine.bus.trace.exec.push(0x0000..=0xFFFF);
    let mut depth = 0usize;
    for _ in 0..frames {
        scene.run(1, &[]);
        let execs = std::mem::take(&mut scene.machine.bus.trace.execs);
        let mut family = None;
        let mut entering = false;
        for event in &execs {
            if interrupts.contains(&event.pc) {
                depth += 1;
            }
            let opcode = event
                .bank
                .map(|bank| prg[usize::from(bank) * 0x2000 + usize::from(event.pc & 0x1FFF)]);
            if depth > 0 {
                if opcode == Some(0x40) {
                    depth -= 1;
                }
                continue;
            }
            if entering {
                family = Some(event.pc);
                entering = false;
            }
            if DISPATCH.contains(&event.pc) {
                entering = true;
                continue;
            }
            if TAILS.contains(&event.pc) {
                family = None;
            }
            let (Some(handler), Some(bank)) = (family, event.bank) else {
                continue;
            };
            let at = usize::from(bank) * 0x2000 + usize::from(event.pc & 0x1FFF);
            if matches!(prg[at], 0x20 | 0x4C) {
                let target = u16::from_le_bytes([prg[at + 1], prg[at + 2]]);
                if target >= 0xA000 && !TAILS.contains(&target) && target != BANK_SWITCH {
                    census.entry(handler).or_default().insert(target);
                }
            }
        }
    }
    scene.machine.bus.trace.exec.clear();
}

/// Isolates `kinds`, runs `frames` frames and checks the port's actor loop on each.
pub(crate) fn assert_actor_phases(mut scene: Scene, frames: u64, kinds: &[u8]) -> Coverage {
    scene.isolate(kinds);
    let prg = scene.prg();
    let events = scene.run(frames, &[ACTOR_LOOP.0, ACTOR_LOOP.1]);
    let pairs = brackets(&events, ACTOR_LOOP.0, &[ACTOR_LOOP.1]);
    let mut coverage = Coverage {
        frames: pairs.len(),
        ..Coverage::default()
    };
    for Bracket { begin, end } in &pairs {
        let mut state = State::lift(begin);
        let (ram, wram) = state.project(begin);
        assert!(
            *ram == *begin.ram && *wram == *begin.wram,
            "lift/project round trip at frame {}",
            begin.frame
        );
        let owned: Vec<_> = state
            .slots
            .main
            .iter()
            .filter(|slot| kinds.contains(&slot.kind))
            .collect();
        for slot in &owned {
            assert_ne!(slot.brain, Brain::Unported, "frame {}", begin.frame);
            *coverage.phases.entry(slot.body.phase).or_default() += 1;
        }
        coverage.occupied += usize::from(!owned.is_empty());
        coverage.drawn += usize::from(begin.ram[0x200..0x300] != end.ram[0x200..0x300]);
        step_actors(&prg, &mut state);
        let (ram, wram) = state.project(begin);
        if let Some((addr, port, oracle)) = diff(begin, (&ram, &wram), end) {
            panic!(
                "frame {}: ${addr:04X} port {port:02X} oracle {oracle:02X}",
                begin.frame
            );
        }
    }
    coverage
}

/// [`assert_actor_phases`] for the second class: the `$CE7F` update and the `$DC87` draw.
pub(crate) fn assert_second_phases(mut scene: Scene, frames: u64, kinds: &[u8]) -> Coverage {
    const UPDATE: (u16, u16) = (0xD3FC, 0xD3FF);
    const DRAW: (u16, u16) = (0xD438, 0xD43B);
    scene.isolate(kinds);
    let prg = scene.prg();
    let events = scene.run(frames, &[UPDATE.0, UPDATE.1, DRAW.0, DRAW.1]);
    let mut coverage = Coverage::default();
    for (pc, end) in [UPDATE, DRAW] {
        for Bracket { begin, end } in brackets(&events, pc, &[end]) {
            let mut state = State::lift(&begin);
            let owned: Vec<_> = state
                .slots
                .second
                .iter()
                .filter(|slot| kinds.contains(&slot.kind))
                .collect();
            if pc == UPDATE.0 {
                coverage.frames += 1;
                coverage.occupied += usize::from(!owned.is_empty());
                for slot in &owned {
                    assert_ne!(slot.brain, SecondBrain::Unported, "frame {}", begin.frame);
                    *coverage.phases.entry(slot.body.phase).or_default() += 1;
                }
                super::step_second(&prg, &mut state);
            } else {
                coverage.drawn += usize::from(begin.ram[0x200..0x300] != end.ram[0x200..0x300]);
                super::draw_second(
                    &prg,
                    &state.world,
                    &state.slots,
                    &mut state.oam,
                    &mut state.work,
                );
            }
            let (ram, wram) = state.project(&begin);
            if let Some((addr, port, oracle)) = diff(&begin, (&ram, &wram), &end) {
                panic!(
                    "${pc:04X} at frame {}: ${addr:04X} port {port:02X} oracle {oracle:02X}",
                    begin.frame
                );
            }
        }
    }
    coverage
}

/// What an actor-phase diff exercised.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Coverage {
    /// Actor loops compared.
    pub frames: usize,
    /// Loops where a slot of the tested kinds was occupied.
    pub occupied: usize,
    /// Loops that changed the OAM page.
    pub drawn: usize,
    /// Slot-frames per `$7DD0` phase of the tested kinds.
    pub phases: BTreeMap<u8, usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peek(pc: u16, nmis: u64) -> PeekEvent {
        PeekEvent {
            frame: 1,
            pc,
            bank: Some(14),
            nmis,
            irqs: 0,
            cpu: Cpu {
                a: 0,
                x: 0,
                y: 0,
                s: 0,
                p: 0,
                pc,
                cycles: 0,
            },
            ram: Box::new([0; 2048]),
            wram: Box::new([0; 8192]),
            vram: Box::new([0; 2048]),
            palette: [0; 32],
        }
    }

    #[test]
    fn lift_round_trips_the_route() {
        let Some(mut scene) = Scene::route(640) else {
            return;
        };
        let events = scene.run(359, &[ACTOR_LOOP.0]);
        assert_eq!(events.len(), 359);
        for event in &events {
            let (ram, wram) = State::lift(event).project(event);
            assert!(*ram == *event.ram, "RAM at frame {}", event.frame);
            assert!(*wram == *event.wram, "WRAM at frame {}", event.frame);
        }
    }

    #[test]
    fn an_empty_actor_loop_changes_nothing() {
        let Some(scene) = Scene::route(700) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 100, &[]);
        assert_eq!((coverage.frames, coverage.occupied), (100, 0));
    }

    #[test]
    fn freed_slots_keep_their_bytes() {
        let Some(mut scene) = Scene::route(900) else {
            return;
        };
        let event = scene.run(1, &[ACTOR_LOOP.0]).remove(0);
        let mut state = State::lift(&event);
        assert_ne!(state.slots.main[3].kind, 0);
        state.slots.main[3].kind = 0;
        let (_, wram) = state.project(&event);
        let changed: Vec<usize> = (0..0x2000).filter(|&i| wram[i] != event.wram[i]).collect();
        assert_eq!(changed, [0x1CC3]);
    }

    #[test]
    fn scenes_force_a_level_and_edit_ram() {
        let Some(mut scene) = Scene::level(2, 650) else {
            return;
        };
        let area = crate::levelmap::area_for_level(2).expect("level 2");
        assert_eq!(usize::from(scene.machine.bus.ram[0x400]), area * 2);
        scene.camera(0x1234);
        scene.player(0x0567, 0x0BFF);
        assert_eq!(scene.machine.bus.ram[0xCC..0xCE], [0x12, 0x34]);
        assert_eq!(
            scene.machine.bus.ram[0x460..0x464],
            [0x05, 0x67, 0x0B, 0xFF]
        );
        let record = SpawnRecord::from([
            0x1D, 0xFF, 0x05, 0xFF, 0x01, 0x05, 0x20, 0x00, 0x05, 0x00, 0xFF, 0x88,
        ]);
        scene.place(5, &record, 0x1D);
        let placed = &Slots::lift(&scene.machine.bus.wram).main[5];
        assert_eq!(
            (placed.kind, placed.body.x, placed.body.cell),
            (0x0A, 0x1DFF, 0x1D)
        );
    }

    /// The oracle's settled gameplay state for every level, pinned to its
    /// `stagearea-probe` capture by `digests.txt`: the frame-624 CHR set `$E0-$E7` (before
    /// sprite animation swaps `$E5`) and the frame-744 palette `$3F00-$3F1F`, read with the
    /// PPU's colour mirrors (`$3F10-$3F1C` read `$3F00-$3F0C`) the captured dump holds.
    #[test]
    fn every_level_settles_to_its_captured_chr_and_palette() {
        let digest = |name: &str| {
            crate::digest::table(include_str!("../../tests/data/digests.txt"), name)["-"]
        };
        for level in 0..13u8 {
            let Some(mut scene) = Scene::level(level, 623) else {
                return;
            };
            assert_eq!(
                crate::digest::fnv1a(&scene.machine.bus.ram[0xE0..0xE8]),
                digest(&format!("level-chr-{level}")),
                "level {level}: the oracle's frame-624 CHR set differs from its capture"
            );
            scene.run(743 - 623, &[]);
            let mut palette = scene.machine.bus.ppu.palette;
            for i in 0..4 {
                palette[0x10 + i * 4] = palette[i * 4];
            }
            assert_eq!(
                crate::digest::fnv1a(&palette),
                digest(&format!("level-palette-{level}")),
                "level {level}: the oracle's frame-744 palette differs from its capture"
            );
        }
    }

    #[test]
    fn room_pages_match_every_room_load() {
        let Some(mut scene) = Scene::tas(0) else {
            return;
        };
        let prg = scene.prg();
        let loads = scene.run(2400, &[0xB4F2]);
        assert!(loads.len() >= 4, "{} room loads", loads.len());
        for load in &loads {
            let (area, room) = (load.ram[0x400], load.ram[0x402]);
            let pages = crate::rooms::Room::import(&prg, area, room)
                .expect("the room")
                .pages;
            assert_eq!(
                pages.map(|page| page << 4)[..],
                load.ram[0x440..0x460],
                "area {area} room {room}"
            );
        }
    }

    /// The injection rule: a byte the loop rewrote after the interrupt is skipped, one
    /// equal to the interrupt's value is kept, and a stack-page byte is always kept.
    #[test]
    fn a_rewritten_interrupt_byte_is_skipped() {
        let mut at = peek(0, 0);
        at.ram[0x300] = 0xAA; // equal to the interrupt's value: kept
        at.ram[0x301] = 0x99; // differs: the loop rewrote it, skipped
        at.ram[0x105] = 0x00; // the stack page: always kept
        let mut writes: Writes = vec![(0x300, 0xAA), (0x301, 0xBB), (0x105, 0xCC)];
        drop_rewritten_irqs(&mut writes, &at);
        assert_eq!(writes, [(0x300, 0xAA), (0x105, 0xCC)]);
    }

    #[test]
    fn diff_names_the_first_changed_address() {
        let before = peek(0, 0);
        let oracle = peek(0, 0);
        let mut ram = [0; 2048];
        let wram = [0; 8192];
        ram[0x300] = 1;
        assert_eq!(diff(&before, (&ram, &wram), &oracle), Some((0x0300, 1, 0)));
        ram[0x300] = 0;
        ram[0x20] = 1;
        assert_eq!(diff(&before, (&ram, &wram), &oracle), None);
    }

    #[test]
    fn recording_counts_match_the_run() {
        let Some(scene) = Scene::tas(350) else {
            return;
        };
        let (run, tape) = assert_game_recording(scene, 600);
        let count =
            |wanted: fn(&Event) -> bool| tape.events.iter().filter(|(_, e)| wanted(e)).count();
        assert!(matches!(tape.events.first(), Some((_, Event::Start(_)))));
        assert!(matches!(tape.events.last(), Some((_, Event::End))));
        assert_eq!(count(|e| matches!(e, Event::Check(_))), run.frames);
        assert!(count(|e| matches!(e, Event::Irq(_))) >= run.frames);
        assert!(count(|e| matches!(e, Event::Nmi(_))) >= run.frames);
    }

    #[test]
    fn pairs_with_an_nmi_between_are_rejected() {
        let events = [
            peek(0xD450, 5),
            peek(0xD453, 6),
            peek(0xD450, 6),
            peek(0xD453, 6),
        ];
        let pairs = brackets(&events, 0xD450, &[0xD453]);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].begin.nmis, 6);
    }
}
