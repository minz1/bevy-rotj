//! The ported sound driver against the game's own, run side by side from power-on.
//!
//! Every driver call the game makes (bank 10 `$8000` init, `$8003` reset,
//! `$8009` tick with the `$0040-$0043` requests it drains) is mirrored into
//! `rotj_music::Music`, and each frame's driver register writes must be equal.

pub mod common;

use std::collections::BTreeSet;
use std::path::Path;

use rotj_music::Music;
use rotj_oracle::{Machine, PeekEvent, physics_input, plan_input, power_on, read_inputs};

const DRIVER_BANK: u8 = 10;
const INIT: u16 = 0x8000;
const RESET: u16 = 0x8003;
const COMMAND: u16 = 0x8006;
const TICK: u16 = 0x8009;
const REQUESTS: std::ops::Range<usize> = 0x40..0x44;

type Writes = Vec<(u16, u8)>;
/// The driver paths reached, each with the opcode that reached it.
type Coverage = BTreeSet<(&'static str, u8)>;

/// `$8288`: the `$E0`-`$FF` command dispatch, A = the opcode.
const COMMAND_DISPATCH: u16 = 0x8288;
/// `$82DD`: a `$00`-`$7F` note, A = the opcode.
const NOTE: u16 = 0x82DD;
/// `$8A7B`: an `$80`-`$DF` instrument note, A = the opcode.
const INSTRUMENT_NOTE: u16 = 0x8A7B;
/// `$82C4`: the mid-note gate, on the stream byte; only `$F5` and up act.
const MID_NOTE: u16 = 0x82C4;
/// `$88DE`: the SFX engine's next event, on the stream byte.
const SFX_EVENT: u16 = 0x88DE;
/// The `$FD`/`$FE` repeat handlers: (PC, path, opcode, counter array). A repeat runs out
/// when its counter enters at 1.
const REPEATS: [(u16, &str, u8, usize); 6] = [
    (0x86DF, "repeat exhausted", 0xFD, 0x064D),
    (0x870A, "repeat exhausted", 0xFE, 0x0645),
    (0x87B3, "mid-note repeat exhausted", 0xFD, 0x064D),
    (0x87D7, "mid-note repeat exhausted", 0xFE, 0x0645),
    (0x8A24, "sfx repeat exhausted", 0xFD, 0x06F5),
    (0x8A4F, "sfx repeat exhausted", 0xFE, 0x06F3),
];

/// The stream byte the driver reads with `LDA ($45),y`, through the window it is in.
fn stream_byte(prg: &[u8], peek: &PeekEvent) -> u8 {
    let ram = &peek.ram;
    let addr = u16::from_le_bytes([ram[0x45], ram[0x46]]).wrapping_add(u16::from(peek.cpu.y));
    let (bank, base) = if addr >= 0xA000 {
        (ram[0x600] & 0x0F, 0xA000)
    } else {
        (DRIVER_BANK, 0x8000)
    };
    prg[usize::from(bank) * 0x2000 + usize::from(addr - base)]
}

/// The path a coverage peek reached, if it reached one.
fn path_of(prg: &[u8], peek: &PeekEvent) -> Option<(&'static str, u8)> {
    let opcode = peek.cpu.a;
    match peek.pc {
        COMMAND_DISPATCH => Some(("command", opcode)),
        NOTE => Some(("note", opcode)),
        INSTRUMENT_NOTE => Some(("instrument note", opcode)),
        MID_NOTE => Some(("mid-note", stream_byte(prg, peek))).filter(|&(_, op)| op >= 0xF5),
        SFX_EVENT => Some(("sfx", stream_byte(prg, peek))),
        pc => REPEATS
            .iter()
            .find(|&&(at, ..)| at == pc)
            .filter(|&&(.., counters)| peek.ram[counters + usize::from(peek.cpu.x)] == 1)
            .map(|&(_, path, opcode, _)| (path, opcode)),
    }
}

#[derive(Clone)]
struct SideBySide {
    machine: Machine,
    music: Music,
    calls: Vec<(u64, u16, [u8; 4])>,
    game_writes: Vec<(u64, u16, u8)>,
    differences: Vec<(u64, Writes, Writes)>,
    coverage: Coverage,
}

impl SideBySide {
    fn power_on() -> Option<Self> {
        let mut machine = power_on()?;
        machine.bus.trace.peeks = vec![INIT, RESET, COMMAND, TICK];
        let music = Music::from_prg(&machine.bus.cart.prg);
        Some(Self {
            machine,
            music,
            calls: Vec::new(),
            game_writes: Vec::new(),
            differences: Vec::new(),
            coverage: Coverage::new(),
        })
    }

    /// Records [`Self::coverage`] from here on.
    fn record_coverage(&mut self) {
        let repeats = REPEATS.iter().map(|&(pc, ..)| pc);
        self.machine.bus.trace.peeks.extend(
            [COMMAND_DISPATCH, NOTE, INSTRUMENT_NOTE, MID_NOTE, SFX_EVENT]
                .into_iter()
                .chain(repeats),
        );
    }

    fn run_frame(&mut self, pad: u8) {
        let log = self.machine.run_frame(pad);
        let frame = self.machine.frame;
        let mut port = Writes::new();
        for call in std::mem::take(&mut self.machine.bus.trace.peeked) {
            if call.bank != Some(DRIVER_BANK) {
                continue;
            }
            if ![INIT, RESET, COMMAND, TICK].contains(&call.pc) {
                self.coverage
                    .extend(path_of(&self.machine.bus.cart.prg, &call));
                continue;
            }
            let requests: [u8; 4] = call.ram[REQUESTS].try_into().expect("four slots");
            self.calls.push((frame, call.pc, requests));
            let writes = match call.pc {
                INIT => self.music.init(),
                RESET => self.music.reset_voices(),
                TICK => {
                    for id in requests.into_iter().filter(|&id| id != 0) {
                        self.music.request(id);
                    }
                    self.music.tick()
                }
                _ => panic!("direct driver command ${:02X}: not modelled", call.cpu.a),
            };
            port.extend(writes.iter().map(|w| (w.addr, w.value)));
        }
        let game: Writes = log
            .apu
            .iter()
            .filter(|w| (0x8000..0xA000).contains(&w.pc))
            .map(|w| (w.addr, w.value))
            .collect();
        self.game_writes
            .extend(game.iter().map(|&(addr, value)| (frame, addr, value)));
        if game != port {
            self.differences.push((frame, game, port));
        }
    }

    fn assert_matched(&self, name: &str) {
        println!(
            "{name}: {} driver writes, {} frames differ",
            self.game_writes.len(),
            self.differences.len()
        );
        if let Some((frame, game, port)) = self.differences.first() {
            panic!("{name}: frame {frame} differs\n  game {game:02X?}\n  port {port:02X?}");
        }
    }
}

/// The game's driver writes from frame 600, the window fceux's capture covers.
fn physics_writes(run: &SideBySide) -> Vec<(u64, u16, u8)> {
    run.game_writes
        .iter()
        .copied()
        .filter(|&(frame, _, _)| frame >= 600)
        .collect()
}

/// The physics run reproduces fceux's capture of it (`tools/emu/stage`, frames 600-1200,
/// by its digests) and the port's driver matches the game's on every frame.
#[test]
fn physics_run_is_the_acceptance_capture() {
    const LAST: usize = 1005;
    let Some(mut run) = SideBySide::power_on() else {
        return;
    };
    for frame in 1..=LAST {
        run.run_frame(physics_input(frame));
    }
    run.assert_matched("physics");
    common::assert_write_digests("physics-apu", &physics_writes(&run));
}

#[test]
fn port_matches_the_driver_through_run1() {
    let Some(mut run) = SideBySide::power_on() else {
        return;
    };
    for frame in 1..3000 {
        run.run_frame(plan_input(frame));
    }
    run.assert_matched("run1");
}

/// Every song-table id, requested over stage 1-1's song for `play` frames, plus the fade-out
/// followed by a new song (whose DMC channel the fade must not mute), and a pause then resume.
/// With `coverage`, returns the driver paths reached.
fn every_song_and_effect(play: usize, coverage: bool) -> Option<Coverage> {
    const SETTLE: usize = 640;
    let mut run = SideBySide::power_on()?;
    if coverage {
        run.record_coverage();
    }
    for frame in 1..=SETTLE {
        run.run_frame(physics_input(frame));
    }
    run.assert_matched("stage start");
    let fade_then_song = [(0, 0xFC), (100, 0x04)];
    let pause_and_resume = [(0, 0x04), (60, 0xFE), (120, 0xFD)];
    let cases = (0..0x30_u8)
        .map(|id| vec![(0, id)])
        .chain([fade_then_song.to_vec(), pause_and_resume.to_vec()]);
    let mut reached = Coverage::new();
    for requests in cases {
        let mut case = run.clone();
        for frame in 0..play {
            for &(_, id) in requests.iter().filter(|&&(at, _)| at == frame) {
                case.machine.bus.ram[0x42] = id;
            }
            case.run_frame(0);
        }
        case.assert_matched(&format!("requests {requests:02X?}"));
        reached.extend(case.coverage);
    }
    Some(reached)
}

/// The sweep at 480 frames per song.
#[test]
fn port_matches_the_driver_for_every_song_and_effect() {
    every_song_and_effect(480, false);
}

/// The (path, opcode) pairs the long sweep reaches in the game's driver.
const COVERAGE: &[(&str, u8)] = &[
    ("command", 0xE2),
    ("command", 0xE5),
    ("command", 0xE6),
    ("command", 0xE7),
    ("command", 0xE9),
    ("command", 0xEA),
    ("command", 0xEB),
    ("command", 0xEC),
    ("command", 0xED),
    ("command", 0xEE),
    ("command", 0xEF),
    ("command", 0xF0),
    ("command", 0xF1),
    ("command", 0xF2),
    ("command", 0xF5),
    ("command", 0xF6),
    ("command", 0xF8),
    ("command", 0xF9),
    ("command", 0xFA),
    ("command", 0xFB),
    ("command", 0xFC),
    ("command", 0xFD),
    ("command", 0xFE),
    ("instrument note", 0xC0),
    ("instrument note", 0xC1),
    ("instrument note", 0xC2),
    ("instrument note", 0xC3),
    ("instrument note", 0xC4),
    ("instrument note", 0xC5),
    ("instrument note", 0xC6),
    ("instrument note", 0xC7),
    ("mid-note", 0xF5),
    ("mid-note", 0xF6),
    ("mid-note", 0xF7),
    ("mid-note", 0xF8),
    ("mid-note", 0xF9),
    ("mid-note", 0xFA),
    ("mid-note", 0xFB),
    ("mid-note", 0xFC),
    ("mid-note", 0xFD),
    ("mid-note", 0xFE),
    ("mid-note repeat exhausted", 0xFD),
    ("mid-note repeat exhausted", 0xFE),
    ("note", 0x01),
    ("note", 0x02),
    ("note", 0x03),
    ("note", 0x04),
    ("note", 0x05),
    ("note", 0x06),
    ("note", 0x07),
    ("note", 0x08),
    ("note", 0x09),
    ("note", 0x0A),
    ("note", 0x0B),
    ("note", 0x0C),
    ("note", 0x0D),
    ("note", 0x0E),
    ("note", 0x0F),
    ("note", 0x10),
    ("note", 0x11),
    ("note", 0x12),
    ("note", 0x13),
    ("note", 0x14),
    ("note", 0x15),
    ("note", 0x16),
    ("note", 0x17),
    ("note", 0x18),
    ("note", 0x19),
    ("note", 0x1A),
    ("note", 0x1B),
    ("note", 0x1C),
    ("note", 0x1D),
    ("note", 0x1E),
    ("note", 0x1F),
    ("note", 0x20),
    ("note", 0x21),
    ("note", 0x22),
    ("note", 0x23),
    ("note", 0x24),
    ("note", 0x25),
    ("note", 0x26),
    ("note", 0x27),
    ("note", 0x28),
    ("note", 0x29),
    ("note", 0x2A),
    ("note", 0x2B),
    ("note", 0x2C),
    ("note", 0x2D),
    ("note", 0x2E),
    ("note", 0x2F),
    ("note", 0x30),
    ("note", 0x31),
    ("note", 0x32),
    ("note", 0x33),
    ("note", 0x34),
    ("note", 0x35),
    ("note", 0x36),
    ("note", 0x37),
    ("note", 0x38),
    ("note", 0x39),
    ("note", 0x3A),
    ("note", 0x3B),
    ("note", 0x3C),
    ("note", 0x3D),
    ("note", 0x3E),
    ("note", 0x3F),
    ("note", 0x40),
    ("note", 0x41),
    ("note", 0x42),
    ("note", 0x43),
    ("note", 0x44),
    ("note", 0x45),
    ("note", 0x46),
    ("note", 0x47),
    ("note", 0x48),
    ("note", 0x4A),
    ("note", 0x4B),
    ("note", 0x4C),
    ("note", 0x4F),
    ("repeat exhausted", 0xFD),
    ("repeat exhausted", 0xFE),
    ("sfx", 0x02),
    ("sfx", 0x03),
    ("sfx", 0x04),
    ("sfx", 0x05),
    ("sfx", 0x06),
    ("sfx", 0x07),
    ("sfx", 0x08),
    ("sfx", 0x09),
    ("sfx", 0x0A),
    ("sfx", 0x0B),
    ("sfx", 0x0C),
    ("sfx", 0x0D),
    ("sfx", 0x0E),
    ("sfx", 0x0F),
    ("sfx", 0x10),
    ("sfx", 0x11),
    ("sfx", 0x12),
    ("sfx", 0x13),
    ("sfx", 0x14),
    ("sfx", 0x15),
    ("sfx", 0x16),
    ("sfx", 0x17),
    ("sfx", 0x19),
    ("sfx", 0x1A),
    ("sfx", 0x1C),
    ("sfx", 0x20),
    ("sfx", 0x21),
    ("sfx", 0x23),
    ("sfx", 0x25),
    ("sfx", 0x26),
    ("sfx", 0x28),
    ("sfx", 0x2A),
    ("sfx", 0x2B),
    ("sfx", 0x2C),
    ("sfx", 0x2D),
    ("sfx", 0x2F),
    ("sfx", 0x30),
    ("sfx", 0x31),
    ("sfx", 0xF0),
    ("sfx", 0xF1),
    ("sfx", 0xF2),
    ("sfx", 0xF3),
    ("sfx", 0xF6),
    ("sfx", 0xF7),
    ("sfx", 0xFA),
    ("sfx", 0xFB),
    ("sfx", 0xFC),
    ("sfx", 0xFD),
    ("sfx", 0xFE),
    ("sfx repeat exhausted", 0xFD),
    ("sfx repeat exhausted", 0xFE),
];

/// Every song played to its loop (3600 frames each), and the driver paths it reaches: the
/// committed list (a longer horizon, 7200 frames, reaches no more).
#[test]
#[ignore = "plays every song for a minute (minutes in a debug build); run with --ignored"]
fn every_song_to_its_loop() {
    let Some(reached) = every_song_and_effect(3600, true) else {
        return;
    };
    let listed: Coverage = COVERAGE.iter().copied().collect();
    let source: Vec<String> = reached
        .iter()
        .map(|(path, opcode)| format!("(\"{path}\", 0x{opcode:02X}),"))
        .collect();
    assert!(
        reached == listed,
        "reached {} pairs:\n{}",
        reached.len(),
        source.join("\n")
    );
}

#[test]
#[ignore = "plays the whole TAS (minutes in a debug build); run with --ignored"]
fn port_matches_the_driver_through_the_tas() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../emu/tas/inputs.csv");
    let Some(mut run) = SideBySide::power_on() else {
        return;
    };
    for pad in read_inputs(&path).into_iter().skip(1) {
        run.run_frame(pad);
    }
    run.assert_matched("tas");
}
