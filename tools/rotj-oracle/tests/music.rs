//! The ported sound driver against the game's own, run side by side from power-on.
//!
//! Every driver call the game makes (bank 10 `$8000` init, `$8003` reset,
//! `$8009` tick with the `$0040-$0043` requests it drains) is mirrored into
//! `rotj_music::Music`, and each frame's driver register writes must be equal.

pub mod common;

use std::path::Path;

use rotj_music::Music;
use rotj_oracle::{Machine, physics_input, plan_input, power_on, read_inputs};

const DRIVER_BANK: u8 = 10;
const INIT: u16 = 0x8000;
const RESET: u16 = 0x8003;
const COMMAND: u16 = 0x8006;
const TICK: u16 = 0x8009;
const REQUESTS: std::ops::Range<usize> = 0x40..0x44;

type Writes = Vec<(u16, u8)>;

#[derive(Clone)]
struct SideBySide {
    machine: Machine,
    music: Music,
    calls: Vec<(u64, u16, [u8; 4])>,
    game_writes: Vec<(u64, u16, u8)>,
    differences: Vec<(u64, Writes, Writes)>,
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
        })
    }

    fn run_frame(&mut self, pad: u8) {
        let log = self.machine.run_frame(pad);
        let frame = self.machine.frame;
        let mut port = Writes::new();
        for call in std::mem::take(&mut self.machine.bus.trace.peeked) {
            if call.bank != Some(DRIVER_BANK) {
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

/// Every song-table id, requested over stage 1-1's song, plus the fade-out
/// followed by a new song (whose DMC channel the fade must not mute).
#[test]
fn port_matches_the_driver_for_every_song_and_effect() {
    const SETTLE: usize = 640;
    const PLAY: usize = 480;
    let Some(mut run) = SideBySide::power_on() else {
        return;
    };
    for frame in 1..=SETTLE {
        run.run_frame(physics_input(frame));
    }
    run.assert_matched("stage start");
    let fade_then_song = [(0, 0xFC), (100, 0x04)];
    let cases = (0..0x30_u8)
        .map(|id| vec![(0, id)])
        .chain([fade_then_song.to_vec()]);
    for requests in cases {
        let mut case = run.clone();
        for frame in 0..PLAY {
            for &(_, id) in requests.iter().filter(|&&(at, _)| at == frame) {
                case.machine.bus.ram[0x42] = id;
            }
            case.run_frame(0);
        }
        case.assert_matched(&format!("requests {requests:02X?}"));
    }
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
