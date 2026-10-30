//! The game's actors: slot state, spawning, the actor loop and drawing.

mod attract;
pub mod boot;
pub mod button;
pub mod collision;
pub mod cutscene;
pub mod ending;
pub mod engine;
pub mod families;
pub mod frame;
pub mod modern;
pub mod oam;
#[cfg(test)]
pub(crate) mod oracle;
pub mod pause;
pub mod player;
pub mod raster;
pub mod room;
pub mod script;
pub mod scroll;
pub mod slots;
mod sound_test;
pub mod spawn;
pub mod stage;
#[cfg(test)]
mod tape;
pub mod weapons;
pub mod world;

use crate::prg::Prg;
use crate::render::ctrl;
use button::Buttons;
use engine::{Work, anchor};
use oam::OamBuilder;
use slots::Slots;
use world::{Cx, Effects, Schedule, SoundId, World, busy, stage_script};

/// Everything the game loop reads and writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    /// The actor slots.
    pub slots: Slots,
    /// Camera, scroll and frame state.
    pub world: World,
    /// The live spawn schedule.
    pub schedule: Schedule,
    /// The player, projectiles, terrain and everything else handlers change.
    pub effects: Effects,
    /// The OAM page being built.
    pub oam: OamBuilder,
    /// The engine's working registers.
    pub work: Work,
    /// Scroll state beside the camera.
    pub scroll: scroll::Scroll,
    /// Palette, HUD and CHR animation state.
    pub display: frame::Display,
    /// The script running in place of the loop, if any.
    pub script: Option<Script>,
    /// The collision probe's working bytes, which the player's movement reads back.
    // shortcut: only the player's probes carry over; actors' probes write `$20-$28` too.
    pub probe: collision::Probe,
}

/// How many times the NMI wait steps the random bytes (`$C21C`) in a shipped frame:
/// the median over the TAS's stage 1-1 loop frames (44-493 measured).
pub const RNG_SPINS: usize = 415;

/// A script that runs in place of the loop body, one frame per [`Game::frame`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Script {
    /// The room change, `$BA = 1` (bank 13 `$B7EC`).
    RoomChange(room::RoomChange),
    /// The next area's load, `$BA = 3` (`$D513`), or the area's restart after a life lost, `$BA = 4` (`$D527`).
    NextArea(stage::NextArea),
    /// The stage end waited for the NMI itself; the loop resumes at its pause check (`$D46C`).
    StageEnded,
    /// The full meter's power-up, `$BA = 5` (`$D58F`).
    PowerUp(stage::PowerUp),
    /// The pause, Start pressed in the loop (bank 8 `$9C37`).
    Pause(pause::Pause),
    /// The ending, `$BA = 6` (`$D967`), which never returns.
    Ending(ending::Ending),
    /// Power-on, the intro and the title to the new game's first loop frame (`$D3F9`).
    Boot(Box<boot::Boot>),
}

impl Script {
    /// True once the script's last frame has run and the loop resumes (`$D46C`).
    #[must_use]
    pub fn done(&self) -> bool {
        match self {
            Script::RoomChange(change) => change.done(),
            Script::NextArea(load) => load.done(),
            Script::StageEnded => true,
            Script::PowerUp(power) => power.done(),
            Script::Pause(pause) => pause.done(),
            Script::Ending(_) => false,
            Script::Boot(boot) => boot.done(),
        }
    }

    fn step(&mut self, game: &mut Game, prg: &Prg, chr: &[u8]) {
        match self {
            Script::RoomChange(change) => change.step(game, prg),
            Script::NextArea(load) => load.step(game, prg, chr),
            Script::StageEnded => {}
            Script::PowerUp(power) => power.step(game, prg),
            Script::Pause(pause) => pause.step(game, prg),
            Script::Ending(ending) => ending.step(game, prg, chr),
            Script::Boot(boot) => boot.step(game, prg, chr),
        }
    }
}

/// Where a frame resumes after the script's part (`$D45F-$D46C`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScriptFrame {
    /// A script ran this frame; the frame is over.
    Stepped,
    /// The pause check, then the gameplay loop.
    AtPauseCheck,
    /// Past the pause check (a pause or the boot has just ended): the gameplay loop.
    PastPauseCheck,
}

impl Game {
    /// The handler context over this state, and the working registers beside it.
    pub fn parts<'a>(&'a mut self, prg: &'a Prg) -> (Cx<'a>, &'a mut Work) {
        let cx = Cx {
            prg,
            world: &self.world,
            slots: &mut self.slots,
            schedule: &mut self.schedule,
            effects: &mut self.effects,
            scroll: &mut self.scroll,
            oam: &mut self.oam,
        };
        (cx, &mut self.work)
    }

    /// Runs the game from one NMI wait to the next (bank 14 `$C23F` -> `$C232`): the end
    /// of the last frame (`$D45F-$D46C`: the speeds, the `$BA` dispatch `$D4D3`, the
    /// pause) and the gameplay loop `$D3F9`, or one frame of a [`Script`]. Once a draw
    /// has hung ([`OamBuilder::hung`]) the game is frozen and this does nothing.
    ///
    /// # Panics
    /// On a `$BA` past the dispatch table's seven entries (`$D60D`).
    pub fn frame(&mut self, prg: &Prg, chr: &[u8]) {
        if self.oam.hung {
            return;
        }
        self.scroll.ppu_ctrl &= !ctrl::VERTICAL_INCREMENT;
        match self.script_frame(prg, chr) {
            ScriptFrame::Stepped => return,
            ScriptFrame::AtPauseCheck if self.pause_requested() => {
                self.start_script(Script::Pause(pause::Pause::default()), prg, chr);
                return;
            }
            ScriptFrame::AtPauseCheck | ScriptFrame::PastPauseCheck => {}
        }
        self.loop_body(prg);
    }

    /// The script's part of the frame: a running script's step, a finished one's end, or
    /// the `$BA` dispatch (`$D4D3`) and a new script's first step.
    fn script_frame(&mut self, prg: &Prg, chr: &[u8]) -> ScriptFrame {
        match self.script.take() {
            Some(Script::PowerUp(power)) if power.done() => {
                stage::PowerUp::finish(self, prg);
                ScriptFrame::AtPauseCheck
            }
            Some(Script::Pause(pause)) if pause.done() => {
                self.scroll.gate = 0xFF;
                ScriptFrame::PastPauseCheck
            }
            Some(Script::Boot(boot)) if boot.done() => ScriptFrame::PastPauseCheck,
            Some(script) if script.done() => ScriptFrame::AtPauseCheck,
            Some(script) => {
                self.start_script(script, prg, chr);
                ScriptFrame::Stepped
            }
            None => {
                self.scroll.speed_x = 0;
                self.scroll.speed_y = 0;
                match self.dispatch_transition(prg) {
                    Some(script) => {
                        self.start_script(script, prg, chr);
                        ScriptFrame::Stepped
                    }
                    None => ScriptFrame::AtPauseCheck,
                }
            }
        }
    }

    /// The script `$BA` asks for (`$D4D3`), if any.
    fn dispatch_transition(&mut self, prg: &Prg) -> Option<Script> {
        match self.effects.transition {
            0 => None,
            1 => Some(Script::RoomChange(room::RoomChange::new(
                self.effects.transition_arg,
            ))),
            2 => stage::stage_end(self, prg).then_some(Script::StageEnded),
            3 => Some(Script::NextArea(stage::NextArea::default())),
            4 => Some(Script::NextArea(stage::NextArea::life_lost())),
            5 => Some(Script::PowerUp(stage::PowerUp::default())),
            6 => Some(Script::Ending(ending::Ending::default())),
            other => panic!("$BA = {other} ($D4D3) is not ported"),
        }
    }

    /// Runs `script`'s frame and keeps it running.
    fn start_script(&mut self, mut script: Script, prg: &Prg, chr: &[u8]) {
        script.step(self, prg, chr);
        self.script = Some(script);
    }

    /// Start pressed while the player may pause (`$0476`'s `$AF` busy bit clear).
    fn pause_requested(&self) -> bool {
        self.world.pressed.contains(Buttons::START) && self.effects.busy & busy::SCRIPTED == 0
    }

    /// The gameplay loop `$D3F9`, one step a line.
    fn loop_body(&mut self, prg: &Prg) {
        self.oam.marking = oam::Mark::Hud;
        frame::draw_hud(self, prg);
        self.oam.marking = oam::Mark::Scene;
        step_second(prg, self);
        #[cfg(test)]
        assert!(
            !self
                .slots
                .second
                .iter()
                .any(|second| second.kind != 0
                    && matches!(second.brain, families::SecondBrain::Empty)),
            "a live second-class slot's kind has no $CEC3 entry"
        );
        self.move_player(prg);
        scroll::speed(self, prg);
        scroll::step(self, prg);
        let overflow = self.draw_player(prg, true);
        #[cfg(test)]
        oracle::overflow_at(0xD42F, overflow);
        let overflow = draw_second(
            prg,
            &self.world,
            &self.slots,
            &mut self.oam,
            &mut self.work,
            overflow,
        );
        #[cfg(test)]
        oracle::overflow_at(0xD43B, overflow);
        let (mut cx, work) = self.parts(prg);
        weapons::step(work, &mut cx, overflow);
        step_actors(prg, self);
        self.oam.finish();
        frame::animate_chr(self);
        frame::queue_palette(self);
    }

    /// Batman's move (bank 4 `$8000`), then the pads cleared while a stage script holds
    /// him (`$D40E`: `$0481` bits 7 and 5 = `10`).
    fn move_player(&mut self, prg: &Prg) {
        let mut probe = self.probe;
        let (mut cx, work) = self.parts(prg);
        player::step_move(work, &mut probe, &mut cx);
        self.probe = probe;
        if self.effects.player.stage_script & (stage_script::FROZEN | stage_script::KEEP_PADS)
            == stage_script::FROZEN
        {
            (self.world.pad, self.world.pressed) = (Buttons::NONE, Buttons::NONE);
        }
    }

    /// True once GAME OVER has run its course, or the password screen's Select or the
    /// sound test's exit was chosen, and the game resets (`JMP ($FFFC)`).
    #[must_use]
    pub fn reset(&self) -> bool {
        match &self.script {
            Some(Script::NextArea(load)) => load.reset(),
            Some(Script::Boot(boot)) => boot.reset(),
            _ => false,
        }
    }

    /// Batman's animate-and-draw (bank 4 `$84AB`), or the draw alone (`$94A6`); the
    /// 6502 overflow flag the animation leaves for weapon 5's charge, `false` for the draw
    /// alone, whose flag nothing reads (the room change's next loop frame runs `$84AB`).
    pub fn draw_player(&mut self, prg: &Prg, animate: bool) -> bool {
        let (mut pressed, mut pad) = (self.world.pressed, self.world.pad);
        let marking = std::mem::replace(&mut self.oam.marking, oam::Mark::Player);
        let (mut cx, work) = self.parts(prg);
        let overflow = if animate {
            player::step_draw(&mut pressed, &mut pad, work, &mut cx)
        } else {
            player::draw_player(work, &mut cx);
            false
        };
        self.oam.marking = marking;
        (self.world.pressed, self.world.pad) = (pressed, pad);
        overflow
    }

    /// The NMI wait and the NMI (`$C232`, bank 15 `$E1D0`): the random bytes spun
    /// [`RNG_SPINS`] times, the frame counters, the pads read from `pad` (`$C1DB`, the
    /// game's bit order `$F6`), the VRAM queue drained after the frame's direct writes,
    /// last pushed first as `$C084` pulls it off the stack, and the sound requests the
    /// driver takes (`$40-$43`, cleared).
    pub fn nmi(&mut self, pad: u8) -> Vblank {
        let mut rng = world::Rng(self.world.rng);
        for _ in 0..RNG_SPINS {
            rng.next_frame();
        }
        self.world.rng = rng.0;
        let world = &mut self.world;
        world.frame = world.frame.wrapping_add(1);
        world.frame_odd = !world.frame_odd;
        let mut held = pad;
        if held & 3 == 3 {
            held &= 0xFE;
        }
        if held & 0x0C == 0x0C {
            held &= 0xF7;
        }
        world.pressed = Buttons::from_bits((held ^ world.pad.bits()) & pad);
        world.pad = Buttons::from_bits(held);
        let effects = &mut self.effects;
        let sound = [
            std::mem::take(&mut effects.music),
            std::mem::take(&mut effects.song),
            std::mem::take(&mut effects.sound[0]),
            std::mem::take(&mut effects.sound[1]),
        ]
        .map(SoundId::from_byte);
        let vram = &mut effects.vram;
        vram.top = 0x7F;
        let mut writes = std::mem::take(&mut vram.direct);
        writes.extend(vram.writes.drain(..).rev());
        Vblank { writes, sound }
    }
}

/// What one NMI hands the hardware.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vblank {
    /// PPU writes, in order.
    pub writes: Vec<world::VramWrite>,
    /// The sound driver's requests, `$40-$43`.
    pub sound: [Option<SoundId>; 4],
}

/// One pass of the actor loop over slots 23 down to 0 (bank 14 `$CED5`).
pub fn step_actors(prg: &Prg, game: &mut Game) {
    let marking = game.oam.marking;
    for index in (0..game.slots.main.len()).rev() {
        if game.slots.main[index].kind == 0 {
            continue;
        }
        let mut slot = std::mem::take(&mut game.slots.main[index]);
        game.slots.main[index].kind = slot.kind;
        slots::load_aliases(
            &mut slot.body,
            index,
            &game.slots,
            &game.effects.projectiles,
        );
        game.work.slot = u8::try_from(index).expect("24 main slots");
        game.oam.marking = oam::Mark::Actor(game.work.slot);
        let running = slot.kind;
        let (mut cx, work) = game.parts(prg);
        families::step(&mut slot, work, &mut cx);
        slots::store_aliases(
            &slot.body,
            index,
            &mut game.slots,
            &mut game.effects.projectiles,
        );
        if slot.kind != 0 || game.slots.main[index].kind == running {
            game.slots.main[index] = slot;
        }
    }
    game.oam.marking = marking;
}

/// One pass of the second-class loop over slots 31 down to 24 (bank 14 `$CE7F`).
pub fn step_second(prg: &Prg, game: &mut Game) {
    for index in (0..game.slots.second.len()).rev() {
        if game.slots.second[index].kind == 0 {
            continue;
        }
        let mut second = std::mem::take(&mut game.slots.second[index]);
        game.slots.second[index].kind = second.kind;
        let (mut cx, work) = game.parts(prg);
        families::step_second(index, &mut second, work, &mut cx);
        game.slots.second[index] = second;
    }
}

/// Draws the second-class actors, slots 31 down to 24 (bank 14 `$DC87`); the 6502
/// overflow flag its last live slot leaves, `overflow` when none is live.
pub fn draw_second(
    prg: &[u8],
    world: &World,
    slots: &Slots,
    oam: &mut OamBuilder,
    work: &mut Work,
    mut overflow: bool,
) -> bool {
    let camera_x = world.camera_x.wrapping_add(8) & 0xFFF0;
    let camera_y = world.camera_y.wrapping_add(8);
    let marking = oam.marking;
    for (index, second) in slots.second.iter().enumerate().rev() {
        let body = &second.body;
        if second.kind == 0 {
            continue;
        }
        overflow = world.camera_x.cast_signed().overflowing_add(8).1;
        if body.anim & 0x80 != 0 {
            continue;
        }
        overflow = engine::sub_overflows16(body.x, camera_x);
        let Some(dx) = body.x.checked_sub(camera_x).filter(|&d| d < 0x1000) else {
            continue;
        };
        work.screen_x = (dx >> 4).to_le_bytes()[0];
        overflow = engine::sub_overflows16(body.y, camera_y);
        let Some(dy) = body.y.checked_sub(camera_y).filter(|&d| d < 0x1000) else {
            continue;
        };
        work.screen_y = (dy >> 4).to_le_bytes()[0];
        work.anchor = if work.screen_y >= 0xF0 {
            anchor::BOTTOM
        } else {
            0
        };
        work.chr_slot = body.chr_slot;
        work.tile = engine::rom_byte(prg, 14, 0xDD24 + u16::from(body.chr_slot));
        work.attr = body.flags;
        work.pointer = engine::rom_word(prg, 14, 0xDD2C + 2 * u16::from(body.sprite));
        let sprite =
            oam::Metasprite::from_rom(prg, 14, work.pointer).expect("second-class metasprite");
        let call = oam::DrawCall {
            x: i16::from(work.screen_x),
            y: i16::from(work.screen_y),
            attr: work.attr,
            tile: work.tile,
            chr_slot: work.chr_slot,
            bottom: work.anchor & anchor::BOTTOM != 0,
        };
        oam.marking = oam::Mark::Second(u8::try_from(index).expect("8 second-class slots"));
        overflow = oam.draw(&sprite, call, world.frame_odd);
    }
    oam.marking = marking;
    overflow
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::path::{Path, PathBuf};

    use super::oracle::{
        GameRun, Scene, StartKind, assert_boot, assert_game, assert_game_from,
        assert_game_recording, assert_playtest, fresh_start, playtest_scene, replay,
    };
    use super::tape::{Event, Tape};
    use crate::render::Mirroring;
    use rotj_rom::Rom;

    const CHAIN: &str = "the_loop_matches_the_tas_from_stage_1_to_the_ending";

    /// The tapes the release lockstep records: per checkout, never committed (they hold ROM bytes).
    fn tape_path(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/tapes/{name}.tape"))
    }

    /// The chain's recorded tape and the ROM, or `None` after a skip line. A missing tape
    /// fails under `ROTJ_REQUIRE_ROM`: the gates record it first.
    fn local_tape() -> Option<(Tape, &'static Rom)> {
        let rom = rotj_rom::for_test()?;
        if !tape_path(CHAIN).exists() {
            let record = "ROTJ_REQUIRE_ROM=1 cargo test --release -p rotj-core the_loop_matches_the_tas -- --ignored";
            assert!(
                std::env::var_os("ROTJ_REQUIRE_ROM").is_none(),
                "no tape at {}: record it with {record}",
                tape_path(CHAIN).display()
            );
            println!("skipped: no tape (record it with {record})");
            return None;
        }
        Some((recorded(CHAIN), rom))
    }

    /// The message `tape`'s replay stops with.
    fn rejection(tape: &Tape, rom: &Rom) -> String {
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            replay_chain(tape, rom);
        }))
        .expect_err("the tape was accepted");
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_default()
    }

    fn recorded(name: &str) -> Tape {
        let path = tape_path(name);
        let file = std::fs::File::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(file)
            .read_to_end(&mut bytes)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        Tape::decode(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    fn replay_chain(tape: &Tape, rom: &Rom) -> GameRun {
        replay(
            CHAIN,
            tape,
            crate::prg::for_test().expect("the test ROM"),
            &rom.chr,
        )
    }

    /// With no live second-class actor the overflow flag passes through `$DC87` untouched.
    #[test]
    fn draw_second_with_no_live_slot_keeps_the_overflow_flag() {
        let mut oam = super::oam::OamBuilder::new();
        let mut work = super::engine::Work::default();
        let (world, slots) = (super::World::default(), super::slots::Slots::default());
        for overflow in [false, true] {
            let kept = super::draw_second(&[], &world, &slots, &mut oam, &mut work, overflow);
            assert_eq!(kept, overflow);
        }
    }

    /// The chain's counts, which its tape's replay reproduces.
    #[derive(Debug, PartialEq, Eq)]
    struct ChainCounts {
        frames: usize,
        room_changes: usize,
        stage_ends: usize,
        next_area: usize,
        script_frames: usize,
        endings: usize,
    }

    /// The chain's counts as its recording ran them.
    const CHAIN_COUNTS: ChainCounts = ChainCounts {
        frames: 35299,
        room_changes: 12,
        stage_ends: 1,
        next_area: 17,
        script_frames: 8901,
        endings: 1,
    };

    fn chain_counts(run: &GameRun) -> ChainCounts {
        ChainCounts {
            frames: run.frames,
            room_changes: run.room_changes,
            stage_ends: run.stage_ends,
            next_area: run.next_area,
            script_frames: run.script_frames,
            endings: run.endings,
        }
    }

    /// Writes `tape` as `name`'s local tape, which the replay tests read.
    fn record(name: &str, tape: &Tape) {
        let path = tape_path(name);
        std::fs::create_dir_all(path.parent().expect("tapes/")).expect("tapes/");
        let file = std::fs::File::create(&path).expect("tape created");
        let mut gzip = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        gzip.write_all(&tape.encode()).expect("tape written");
        gzip.finish().expect("tape written");
    }

    /// The whole TAS, then the last boss's fall with no input (its last frame is 40896):
    /// type `$B8` arms the ending (`$BA = 6`, 41177), which runs through the credits to
    /// its last screen (`$6656`, 47938) and on into its hold.
    #[test]
    #[ignore = "the whole game in lockstep with the oracle (8-10 min in a debug build); the gates run it in release with --ignored"]
    fn the_loop_matches_the_tas_from_stage_1_to_the_ending() {
        let Some(scene) = Scene::tas(350) else {
            return;
        };
        let (run, tape) = assert_game_recording(scene, 47676);
        assert_eq!(chain_counts(&run), CHAIN_COUNTS, "{run:?}");
        let pictures = run.pictures;
        assert_eq!(
            (
                pictures.frames,
                pictures.jittered,
                pictures.lines,
                pictures.most,
                pictures.raced,
                pictures.irqs
            ),
            (47332, 2318, 2362, 2, 0, 31268),
            "{run:?}"
        );
        assert!(
            pictures.hud > 0 && pictures.label > 0,
            "the chain showed marked HUD sprites: {run:?}"
        );
        let exceptions = run.sprite_exceptions;
        assert_eq!(
            (
                pictures.modern,
                pictures.runs,
                pictures.from_nametables,
                pictures.classic_splits,
                pictures.row_15_palettes,
                exceptions.half_written,
                exceptions.unreached
            ),
            (34815, 25985, 23807, 486, 185, 0, 8),
            "the Modern frames and the gates' exceptions: {run:?}"
        );
        assert_eq!(pictures.crowded, 0, "no split past MAX_RUNS: {run:?}");
        let injected: Vec<_> = run.injected.keys().copied().collect();
        assert_eq!(
            injected,
            [
                "IRQ handler $E2A9 (raster splits)",
                "NMI $E1D0 outside the wait (a lag frame)"
            ],
            "{run:?}"
        );
        record(CHAIN, &tape);
    }

    #[test]
    fn the_tape_replays_the_chain() {
        let Some((tape, rom)) = local_tape() else {
            return;
        };
        let run = replay_chain(&tape, rom);
        assert_eq!(chain_counts(&run), CHAIN_COUNTS, "{run:?}");
        assert!(run.injected.is_empty(), "{run:?}");
    }

    /// The chain's starts, which the sweep replays from. Their `frame` is the TAS
    /// frame of the first loop frame after each room change and next-area load.
    #[test]
    fn the_chain_lists_its_starts() {
        let Some((tape, rom)) = local_tape() else {
            return;
        };
        let run = replay_chain(&tape, rom);
        let kind_count = |kind: StartKind| run.starts.iter().filter(|s| s.kind == kind).count();
        assert_eq!(
            (
                kind_count(StartKind::RoomChange),
                kind_count(StartKind::NextArea)
            ),
            (12, 17),
            "{:?}",
            run.starts
        );
        assert!(
            run.starts.windows(2).all(|w| w[0].frame < w[1].frame),
            "{:?}",
            run.starts
        );
        for start in &run.starts {
            println!(
                "{:?} {} {} {}",
                start.kind, start.frame, start.area, start.room
            );
        }
    }

    #[test]
    fn the_replay_is_deterministic() {
        let Some((tape, rom)) = local_tape() else {
            return;
        };
        let first = format!("{:?}", replay_chain(&tape, rom));
        assert_eq!(first, format!("{:?}", replay_chain(&tape, rom)));
    }

    /// Indexes of `tape`'s loop-frame checks.
    fn checks(tape: &Tape) -> Vec<usize> {
        (0..tape.events.len())
            .filter(|&i| matches!(tape.events[i].1, Event::Check(_)))
            .collect()
    }

    #[test]
    fn a_changed_hash_names_its_frame() {
        let Some((mut tape, rom)) = local_tape() else {
            return;
        };
        let at = checks(&tape)[99];
        if let Event::Check(hash) = &mut tape.events[at].1 {
            *hash ^= 1;
        }
        let message = rejection(&tape, rom);
        assert!(message.contains("loop frame 99"), "{message}");
    }

    #[test]
    fn a_changed_script_hash_names_its_frame() {
        let Some((mut tape, rom)) = local_tape() else {
            return;
        };
        let at = (0..tape.events.len())
            .find(|&i| matches!(tape.events[i].1, Event::ScriptCheck(_)))
            .expect("the chain has script frames");
        if let Event::ScriptCheck(hash) = &mut tape.events[at].1 {
            *hash ^= 1;
        }
        let message = rejection(&tape, rom);
        assert!(message.contains("script frame"), "{message}");
    }

    #[test]
    fn a_stale_tape_is_rejected() {
        let Some((mut tape, rom)) = local_tape() else {
            return;
        };
        let at = checks(&tape)[9];
        let frame = tape.events[at].0;
        let extra = Event::Nmi(Vec::new());
        tape.events.insert(at, (frame, extra));
        let message = rejection(&tape, rom);
        assert!(message.contains("stale"), "{message}");
    }

    #[test]
    fn a_tape_cut_short_mid_run_is_rejected() {
        let Some((mut tape, rom)) = local_tape() else {
            return;
        };
        tape.events.truncate(tape.events.len() / 2);
        let message = rejection(&tape, rom);
        assert!(message.contains("stale"), "{message}");
    }

    #[test]
    fn an_event_after_end_is_rejected() {
        let Some((mut tape, rom)) = local_tape() else {
            return;
        };
        let frame = tape.events.last().expect("the tape ends").0;
        tape.events.push((frame, Event::End));
        let message = rejection(&tape, rom);
        assert!(message.contains("stale"), "{message}");
    }

    #[test]
    fn the_port_starts_stage_1_1_and_plays_it_like_the_tas() {
        let Some(scene) = Scene::tas(350) else {
            return;
        };
        let prg = Scene::prg();
        let chr = scene.machine.bus.cart.chr.clone();
        let peek = scene.clone().run(1, &[0xD3F9]).remove(0);
        let mut start = super::room::start(prg, &chr, 0);
        assert_eq!(start.effects.vram.direct.len(), 16);
        start.effects.vram.direct.clear();
        let oracle = &super::oracle::State::lift(&peek).world;
        start.world.rng = oracle.rng;
        start.world.frame = oracle.frame;
        start.world.frame_odd = oracle.frame_odd;
        start.world.pad = oracle.pad;
        start.world.pressed = oracle.pressed;
        start.probe = super::collision::Probe::lift(&peek.ram);
        let chr = start.oam.chr;
        start.oam = super::oracle::State::lift(&peek).oam;
        assert_eq!(start.oam.chr, chr);
        let lifted = super::oracle::State::lift(&peek);
        start.work = lifted.work;
        let run = assert_game_from(scene, 1800, Some(start));
        assert_eq!(
            (run.frames, run.room_changes, run.stage_ends, run.next_area),
            (1550, 2, 1, 1),
            "{run:?}"
        );
    }

    #[test]
    fn standing_in_stage_1_2_dies_in_the_pit_and_restarts_the_area() {
        let Some(mut scene) = Scene::tas(2150) else {
            return;
        };
        scene.script(1500, |_| 0);
        let run = assert_game(scene, 1500);
        assert_eq!((run.frames, run.next_area), (1135, 2), "{run:?}");
        assert_eq!(run.injected.len(), 1, "{run:?}");
    }

    #[test]
    fn start_on_the_continue_screen_restarts_the_stage() {
        let Some(mut scene) = Scene::tas(2150) else {
            return;
        };
        scene.script(2700, |i| if (2250..2254).contains(&i) { 0x08 } else { 0 });
        let run = assert_game(scene, 2700);
        assert_eq!((run.frames, run.next_area), (1909, 3), "{run:?}");
    }

    #[test]
    fn the_countdown_running_out_shows_game_over_and_resets() {
        let Some(mut scene) = Scene::tas(2150) else {
            return;
        };
        scene.script(3400, |_| 0);
        let run = assert_game(scene, 3400);
        assert_eq!(
            (run.frames, run.next_area, run.reset),
            (1627, 3, true),
            "{run:?}"
        );
    }

    #[test]
    fn start_pauses_the_game_and_start_again_resumes_it() {
        let Some(mut scene) = Scene::tas(470) else {
            return;
        };
        scene.script(400, |i| match i {
            20..24 | 150..154 | 300..304 => 0x08,
            _ => 0x80,
        });
        let run = assert_game(scene, 400);
        assert_eq!(
            (run.frames, run.pauses, run.script_frames),
            (168, 2, 229),
            "{run:?}"
        );
    }

    #[test]
    fn a_full_meter_powers_batman_up() {
        let Some(mut scene) = Scene::tas(470) else {
            return;
        };
        scene.machine.bus.trace.pokes.push((0xD45C, 0xBA, 5));
        scene.script(400, |i| if i >= 120 && i % 16 < 2 { 0x02 } else { 0 });
        let run = assert_game(scene, 400);
        assert_eq!((run.frames, run.power_ups), (361, 1), "{run:?}");
        assert!(super::oracle::was_seen(0x9668), "no weapon 5 shot flew");
    }

    #[test]
    fn running_out_of_energy_loses_a_life() {
        let Some(mut scene) = Scene::tas(470) else {
            return;
        };
        for (addr, value) in [(0x478, 0xFE), (0x47A, 0xC0), (0x479, 0x02)] {
            scene.machine.bus.trace.pokes.push((0xD45C, addr, value));
        }
        let run = assert_game(scene, 400);
        assert_eq!((run.frames, run.next_area), (263, 1), "{run:?}");
    }

    /// Seeded random pads in the oracle's bit order: each held 2-21 frames, mostly
    /// heading right, B held three times in four, A one in four, a rare Select, and
    /// a rare Start that pauses for three holds.
    fn random_pads(seed: u32, frames: usize) -> Vec<u8> {
        const DIRECTIONS: [u8; 9] = [0x80, 0x80, 0x80, 0x90, 0xA0, 0x40, 0x10, 0x20, 0];
        let mut state = seed;
        let mut pads = Vec::with_capacity(frames);
        let mut unpause = None;
        while pads.len() < frames {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let [length, direction, buttons, extra] = state.to_le_bytes();
            let mut pad = DIRECTIONS[usize::from(direction % 9)]
                | u8::from(buttons.is_multiple_of(4))
                | if buttons & 0x0C == 0 { 0 } else { 0x02 }
                | if extra == 0 { 0x04 } else { 0 };
            unpause = match unpause {
                Some(0) => {
                    pad |= 0x08;
                    None
                }
                Some(holds) => Some(holds - 1),
                None if extra == 1 => {
                    pad |= 0x08;
                    Some(3)
                }
                None => None,
            };
            pads.extend(std::iter::repeat_n(pad, usize::from(2 + length % 20)));
        }
        pads.truncate(frames);
        pads
    }

    /// From TAS frame `start` with `$0476` poked to `weapon` and `poke`'s RAM bytes set,
    /// chains 2000 frames of [`random_pads`] from `seed` through the whole loop.
    fn random_play(start: u64, seed: u32, weapon: u8, poke: &[(u16, u8)]) -> Option<GameRun> {
        let mut scene = Scene::tas(start)?;
        scene.machine.bus.ram[0x476] = weapon;
        for &(addr, value) in poke {
            scene.machine.bus.ram[usize::from(addr)] = value;
        }
        let pads = random_pads(seed, 2000);
        scene.script(2000, |i| pads[usize::try_from(i).expect("2000 frames")]);
        Some(assert_game(scene, 2000))
    }

    /// [`random_play`], catching a divergence so the sweep can go on: `Err` holds the
    /// panic's message.
    fn caught<T>(run: impl FnOnce() -> T) -> Result<T, String> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).map_err(|payload| {
            payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panic without a message".to_owned())
        })
    }

    /// Each `(start, seed, weapon, loop frames)` with `poke` plays randomly to the end
    /// without a divergence.
    fn plays_randomly(poke: &[(u16, u8)], cases: &[(u64, u32, u8, usize)]) {
        let mut played = Vec::new();
        for &(start, seed, weapon, _) in cases {
            let Some(run) = random_play(start, seed, weapon, poke) else {
                return;
            };
            assert!(!run.hung, "TAS {start}, seed {seed}: {run:?}");
            played.push((start, seed, weapon, run.frames));
        }
        assert_eq!(played, cases);
    }

    #[test]
    fn random_play_in_stage_1_1_matches_the_oracle() {
        plays_randomly(
            &[],
            &[
                (350, 8269, 1, 1967),
                (1000, 8919, 1, 1894),
                (1000, 16838, 2, 1999),
                (1000, 24757, 3, 1951),
                (1000, 64352, 2, 1999),
            ],
        );
    }

    #[test]
    fn random_play_in_stage_1_2_matches_the_oracle() {
        plays_randomly(
            &[],
            &[
                (2150, 10069, 1, 1776),
                (2150, 33826, 4, 1669),
                (3000, 26757, 3, 1772),
                (4500, 20338, 2, 1826),
            ],
        );
    }

    #[test]
    fn random_play_in_the_boss_fight_matches_the_oracle() {
        plays_randomly(
            &[],
            &[
                (6300, 22138, 2, 1961),
                (6300, 37976, 4, 1901),
                (6300, 53814, 0, 1951),
                (6300, 180_518, 4, 1993),
            ],
        );
    }

    /// Weapon 5's charge waits on the overflow flag (`$9790` `LDA $B8; BVC`), which the
    /// routines before it leave: seeded runs where it differs from `$B8` bit 6 at `$9790`,
    /// left by the draw's `ADC $94` (`$C288`), the second-class draw's `SBC $31`
    /// (`$DCDF`), motion 0's `BIT $F5` (`$85A4`) and the solid probe's `BIT $DD` (`$C93D`).
    #[test]
    fn weapon_5s_charge_waits_on_the_overflow_flag() {
        let cases = [
            (1000, 40595, 120, 119),
            (1000, 72271, 260, 259),
            (2150, 81340, 200, 199),
            (4500, 28257, 420, 419),
        ];
        for (start, seed, frames, loop_frames) in cases {
            let Some(mut scene) = Scene::tas(start) else {
                return;
            };
            scene.machine.bus.ram[0x476] = 5;
            let pads = random_pads(seed, usize::try_from(frames).expect("small"));
            scene.script(frames, |i| pads[usize::try_from(i).expect("small")]);
            let differs = scene.clone().run(frames, &[0x9790]).iter().any(|peek| {
                peek.bank == Some(5) && (peek.cpu.p & 0x40 != 0) != (peek.ram[0xB8] & 0x40 != 0)
            });
            assert!(
                differs,
                "TAS {start}, seed {seed}: V equals $B8 bit 6 at every $9790"
            );
            let run = assert_game(scene, frames);
            assert_eq!(run.frames, loop_frames, "TAS {start}, seed {seed}: {run:?}");
        }
    }

    /// The area starts of stage 1, every weapon from its first passing seed, then a
    /// death run from each and the GAME OVER runs that reach the reset.
    #[test]
    #[ignore = "seeded random play in stage 1 (release); the gates run it in release with --ignored"]
    fn random_play_in_stage_1_matches_the_oracle() {
        plays_randomly(
            &[],
            &[
                (350, 1, 0, 1999),
                (350, 1, 1, 1999),
                (350, 1, 2, 1999),
                (350, 1, 3, 1999),
                (350, 1, 4, 1999),
                (350, 1, 5, 1999),
                (2150, 1, 0, 1722),
                (2150, 1, 1, 1722),
                (2150, 1, 2, 1722),
                (2150, 1, 3, 1722),
                (2150, 1, 4, 1722),
                (2150, 1, 5, 1722),
                (6282, 1, 0, 1999),
                (6282, 1, 1, 1999),
                (6282, 1, 2, 1999),
                (6282, 1, 3, 1999),
                (6282, 1, 4, 1999),
                (6282, 1, 5, 1796),
            ],
        );
        plays_randomly(
            DEATH,
            &[(350, 1, 1, 1856), (2150, 1, 1, 1722), (6282, 1, 1, 1999)],
        );
        plays_randomly(GAME_OVER, &[(2150, 1, 1, 541)]);
    }

    /// The area starts of stage 2, every weapon from its first passing seed, then a
    /// death run from each and the GAME OVER runs that reach the reset.
    #[test]
    #[ignore = "seeded random play in stage 2 (release); the gates run it in release with --ignored"]
    fn random_play_in_stage_2_matches_the_oracle() {
        // The runs that found type $1E's ungated turn (7239) and $9D49's weapon clear (8634).
        plays_randomly(
            &[],
            &[
                (7239, 1, 0, 1999),
                (7239, 1, 1, 1999),
                (7239, 1, 2, 1704),
                (7239, 1, 3, 1999),
                (7239, 1, 4, 1999),
                (7239, 1, 5, 1999),
                (8634, 1, 0, 1882),
                (8634, 1, 1, 1882),
                (8634, 1, 2, 1882),
                (8634, 1, 3, 1882),
                (8634, 1, 4, 1882),
                (8634, 1, 5, 1999),
            ],
        );
        plays_randomly(DEATH, &[(7239, 1, 1, 1860), (8634, 1, 1, 1871)]);
        plays_randomly(GAME_OVER, &[(8634, 1, 1, 860)]);
    }

    /// The area starts of stage 3, every weapon from its first passing seed, then a
    /// death run from each.
    #[test]
    #[ignore = "seeded random play in stage 3 (release); the gates run it in release with --ignored"]
    fn random_play_in_stage_3_matches_the_oracle() {
        plays_randomly(
            &[],
            &[
                (11959, 1, 0, 1849),
                (11959, 1, 1, 1849),
                (11959, 1, 2, 1999),
                (11959, 1, 3, 1999),
                (11959, 1, 4, 1849),
                (11959, 1, 5, 1863),
                (13215, 1, 0, 1999),
                (13215, 1, 1, 1999),
                (13215, 1, 2, 1999),
                (13215, 1, 3, 1999),
                (13215, 1, 4, 1999),
                (13215, 1, 5, 1999),
                (14649, 3, 0, 1999),
                (14649, 1, 1, 1999),
                (14649, 1, 2, 1999),
                (14649, 1, 3, 1999),
                (14649, 1, 4, 1999),
                (14649, 1, 5, 1523),
            ],
        );
        plays_randomly(
            DEATH,
            &[
                (11959, 1, 1, 1849),
                (13215, 1, 1, 1912),
                (14649, 1, 1, 1999),
            ],
        );
    }

    /// The area starts of stage 4, every weapon from its first passing seed, then a
    /// death run from each and the GAME OVER runs that reach the reset.
    #[test]
    #[ignore = "seeded random play in stage 4 (release); the gates run it in release with --ignored"]
    fn random_play_in_stage_4_matches_the_oracle() {
        plays_randomly(
            &[],
            &[
                (15711, 1, 0, 1692),
                (15711, 1, 1, 1692),
                (15711, 1, 2, 1753),
                (15711, 1, 3, 1692),
                (15711, 1, 4, 1692),
                (15711, 1, 5, 1861),
                (20119, 1, 0, 1853),
                (20119, 1, 1, 1853),
                (20119, 1, 2, 1841),
                (20119, 1, 3, 1853),
                (20119, 1, 4, 1853),
                (20119, 1, 5, 1853),
                (26281, 1, 0, 1999),
                (26281, 1, 1, 1999),
                (26281, 1, 2, 1999),
                (26281, 1, 3, 1999),
                (26281, 1, 4, 1998),
                (26281, 1, 5, 1472),
            ],
        );
        plays_randomly(
            DEATH,
            &[
                (15711, 1, 1, 1747),
                (20119, 1, 1, 1853),
                (26281, 1, 1, 1999),
            ],
        );
        plays_randomly(GAME_OVER, &[(15711, 1, 1, 964)]);
    }

    /// The area starts of stage 5, every weapon from its first passing seed, then a
    /// death run from each and the GAME OVER runs that reach the reset.
    #[test]
    #[ignore = "seeded random play in stage 5 (release); the gates run it in release with --ignored"]
    fn random_play_in_stage_5_matches_the_oracle() {
        // 28821 seed 1 found $8BEA's full-slot return; seed 3 weapon 4 an NMI that ran
        // past the first IRQ's counter.
        plays_randomly(
            &[],
            &[
                (27554, 1, 0, 901),
                (27554, 1, 1, 901),
                (27554, 1, 2, 901),
                (27554, 1, 3, 901),
                (27554, 1, 4, 901),
                (27554, 1, 5, 901),
                (28821, 1, 0, 1884),
                (28821, 1, 1, 1884),
                (28821, 1, 2, 1884),
                (28821, 1, 3, 1884),
                (28821, 1, 4, 1874),
                (28821, 1, 5, 1883),
                (28821, 3, 4, 1838),
            ],
        );
        plays_randomly(DEATH, &[(27554, 1, 1, 901), (28821, 1, 1, 1811)]);
        plays_randomly(GAME_OVER, &[(27554, 1, 1, 324), (28821, 1, 1, 528)]);
    }

    /// The area starts of stage 6, every weapon from its first passing seed, then a
    /// death run from each and the GAME OVER runs that reach the reset.
    #[test]
    #[ignore = "seeded random play in stage 6 (release); the gates run it in release with --ignored"]
    fn random_play_in_stage_6_matches_the_oracle() {
        plays_randomly(
            &[],
            &[
                (32043, 1, 0, 1999),
                (32043, 1, 1, 1999),
                (32043, 1, 2, 1999),
                (32043, 1, 3, 1999),
                (32043, 1, 4, 1999),
                (32043, 1, 5, 1999),
                (33632, 1, 0, 825),
                (33632, 1, 1, 825),
                (33632, 1, 2, 825),
                (33632, 1, 3, 825),
                (33632, 1, 4, 825),
                (33632, 1, 5, 825),
                (36846, 1, 0, 1999),
                (36846, 1, 1, 1999),
                (36846, 1, 2, 1999),
                (36846, 1, 3, 1999),
                (36846, 1, 4, 1999),
                (36846, 1, 5, 1470),
            ],
        );
        plays_randomly(
            DEATH,
            &[(32043, 1, 1, 1999), (33632, 1, 1, 825), (36846, 1, 1, 1999)],
        );
        plays_randomly(GAME_OVER, &[(33632, 1, 1, 289)]);
    }

    /// The area starts of stage 7, every weapon from its first passing seed, then a
    /// death run from each.
    #[test]
    #[ignore = "seeded random play in stage 7 (release); the gates run it in release with --ignored"]
    fn random_play_in_stage_7_matches_the_oracle() {
        // 39200 (a room change) found the spikes' lost $A8; 38120 seed 2 weapon 4 the lag
        // NMI's $42 injected over a later request.
        plays_randomly(
            &[],
            &[
                (38120, 1, 0, 1937),
                (38120, 1, 1, 1937),
                (38120, 1, 2, 1937),
                (38120, 1, 3, 1937),
                (38120, 1, 4, 1937),
                (38120, 1, 5, 1999),
                (39701, 1, 0, 1999),
                (39701, 1, 1, 1999),
                (39701, 1, 2, 1994),
                (39701, 1, 3, 1999),
                (39701, 1, 4, 1996),
                (39701, 1, 5, 1037),
                (39200, 1, 0, 1832),
                (38120, 2, 4, 1714),
            ],
        );
        plays_randomly(DEATH, &[(38120, 1, 1, 1869), (39701, 1, 1, 1999)]);
    }

    /// Weapon 0's charge lands in a slot a spent shot left with `$7CB0 = $FF`, draws
    /// `$814E[$FE]` (`$D020`, code in bank 14) and `$C247` never returns.
    #[test]
    fn weapon_0s_charge_in_a_spent_slot_freezes_the_game() {
        let Some(run) = random_play(1000, 48514, 0, &[]) else {
            return;
        };
        assert_eq!((run.frames, run.hung), (674, true), "{run:?}");
    }

    /// The TAS frame of the first loop frame after each room change (`RoomChange`) and
    /// next-area load (`NextArea`) the whole-game chain runs, with the port's
    /// `$0400`/`$0402` there: the sweep's start points. Generated once from
    /// `the_chain_lists_its_starts`'s output, frame 350 (stage 1-1) first.
    const STARTS: [(u64, StartKind, u8, u8); 30] = [
        (350, StartKind::NextArea, 0, 0),
        (663, StartKind::RoomChange, 0, 2),
        (1323, StartKind::RoomChange, 0, 4),
        (2150, StartKind::NextArea, 2, 0),
        (6282, StartKind::NextArea, 4, 0),
        (7239, StartKind::NextArea, 6, 0),
        (7638, StartKind::RoomChange, 6, 2),
        (8140, StartKind::RoomChange, 6, 4),
        (8634, StartKind::NextArea, 8, 0),
        (11959, StartKind::NextArea, 10, 0),
        (13215, StartKind::NextArea, 12, 0),
        (14047, StartKind::RoomChange, 12, 2),
        (14649, StartKind::NextArea, 14, 0),
        (15711, StartKind::NextArea, 16, 0),
        (20119, StartKind::NextArea, 18, 0),
        (20516, StartKind::RoomChange, 18, 2),
        (23653, StartKind::RoomChange, 18, 4),
        (23857, StartKind::RoomChange, 18, 6),
        (26281, StartKind::NextArea, 20, 0),
        (27554, StartKind::NextArea, 22, 0),
        (28821, StartKind::NextArea, 24, 0),
        (32043, StartKind::NextArea, 26, 0),
        (32674, StartKind::RoomChange, 26, 2),
        (33322, StartKind::RoomChange, 26, 4),
        (33632, StartKind::NextArea, 28, 0),
        (36846, StartKind::NextArea, 30, 0),
        (38120, StartKind::NextArea, 32, 0),
        (38667, StartKind::RoomChange, 32, 2),
        (39200, StartKind::RoomChange, 32, 4),
        (39701, StartKind::NextArea, 34, 0),
    ];

    /// The death run's poke: energy `$0478` = 2, the lowest the game's even damage
    /// leaves, so the first hit kills. An odd energy points the HUD gauge (`$DC0F`,
    /// word-indexed) into the PPU registers, which the oracle cannot answer mid-frame.
    const DEATH: &[(u16, u8)] = &[(0x478, 2)];

    /// [`DEATH`] on the last life (`$0482` = 0): the death reaches GAME OVER.
    const GAME_OVER: &[(u16, u8)] = &[(0x478, 2), (0x482, 0)];

    /// Prints one sweep line: `kind start area room seed weapon pokes -> outcome`.
    fn sweep_report(
        start: u64,
        kind: StartKind,
        area: u8,
        room: u8,
        seed: u32,
        weapon: u8,
        poke: &[(u16, u8)],
    ) {
        let pokes = if poke.is_empty() {
            "-".to_owned()
        } else {
            poke.iter()
                .map(|(addr, value)| format!("${addr:04X}={value:02X}"))
                .collect::<Vec<_>>()
                .join(",")
        };
        let outcome = match caught(|| random_play(start, seed, weapon, poke)) {
            Ok(None) => "skipped (no ROM/TAS)".to_owned(),
            Ok(Some(run)) if run.hung => "hung".to_owned(),
            Ok(Some(run)) if run.reset => format!("reset at {} frames", run.frames),
            Ok(Some(run)) => format!("{} frames", run.frames),
            Err(message) => format!("DIVERGES {message}"),
        };
        println!(
            "{kind:?} {start} {area} {room} seed {seed} weapon {weapon} pokes {pokes} -> {outcome}"
        );
    }

    /// Seeded random play from every chain start, every weapon 0-5, K seeds each plus one
    /// death and one GAME OVER run per start; prints one line per run and never fails
    /// itself. Run it alone:
    /// it installs a quiet panic hook. `ROTJ_SWEEP_SEEDS` (3) and `ROTJ_SWEEP_BASE` (1)
    /// choose the seed range; seed 0 is skipped (xorshift would give all-zero pads).
    #[test]
    #[ignore = "the seeded random-play sweep from every start (release, on demand); run with --ignored"]
    fn random_play_sweep() {
        let seeds = std::env::var("ROTJ_SWEEP_SEEDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3);
        let base: u32 = std::env::var("ROTJ_SWEEP_BASE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        std::panic::set_hook(Box::new(|_| {}));
        for &(start, kind, area, room) in &STARTS {
            for seed in base..base + seeds {
                if seed == 0 {
                    continue;
                }
                for weapon in 0..=5u8 {
                    sweep_report(start, kind, area, room, seed, weapon, &[]);
                }
            }
            sweep_report(start, kind, area, room, base, 1, DEATH);
            sweep_report(start, kind, area, room, base, 1, GAME_OVER);
        }
    }

    #[test]
    #[ignore = "the release random-play gate; the gates run it in release with --ignored"]
    fn caught_reports_a_divergence() {
        let Some(scene) = Scene::tas(350) else {
            return;
        };
        let peek = scene.clone().run(1, &[0xD3F9]).remove(0);
        let mut start = super::oracle::State::lift(&peek);
        start.effects.player.energy ^= 1;
        let message = caught(move || Some(assert_game_from(scene, 1800, Some(start)))).unwrap_err();
        // The energy difference reaches the HUD gauge's CHR shadow ($00E0) in the same
        // frame's draw, before the compare's address order reaches $0478.
        assert!(message.contains("$00E0"), "{message}");
        let run = caught(|| random_play(350, 8269, 1, &[]))
            .expect("a clean run does not panic")
            .expect("the TAS");
        assert_eq!(run.frames, 1967, "{run:?}");
    }

    /// A run that hangs the port is reported, not stalled: `caught` returns it whole.
    #[test]
    #[ignore = "the release random-play gate; the gates run it in release with --ignored"]
    fn caught_reports_a_hang() {
        let run = caught(|| random_play(1000, 48514, 0, &[]))
            .expect("a hang does not panic")
            .expect("the TAS");
        assert!(run.hung, "{run:?}");
    }

    /// The death on the last life ([`GAME_OVER`]) reaches GAME OVER and the game's reset.
    #[test]
    #[ignore = "the release random-play gate; the gates run it in release with --ignored"]
    fn lives_at_zero_reach_game_over() {
        let Some(run) = random_play(2150, 1, 1, GAME_OVER) else {
            return;
        };
        assert_eq!((run.frames, run.reset), (541, true), "{run:?}");
    }

    use std::collections::BTreeSet;

    /// A novelty cell: area `$0400`, room `$0402`, camera page (`$CC:$CD`, 1/16 px, `>> 12`
    /// is 256 px) and player motion `$046E`.
    type Cell = (u8, u8, u16, u8);

    /// The novelty keys a run reaches: its cells and `(type, phase)` pairs.
    type Keys = (BTreeSet<Cell>, BTreeSet<(u8, u8)>);

    /// What a run of [`explore_branches`] found: the archive, the keys reached and the
    /// divergences.
    type Exploration = (
        Vec<ExplorePath>,
        BTreeSet<Cell>,
        BTreeSet<(u8, u8)>,
        Vec<Divergence>,
    );

    /// The novelty key of a state: its [`Cell`] and, over the occupied slots (as
    /// [`Scene::isolate`] scans them), each slot's `(type $7CC0 & $7F, phase $7DD0 >> 4)`.
    fn features(ram: &[u8; 2048], wram: &[u8; 8192]) -> (Cell, BTreeSet<(u8, u8)>) {
        let camera = u16::from_be_bytes([ram[0xCC], ram[0xCD]]);
        let cell = (ram[0x400], ram[0x402], camera >> 12, ram[0x46E]);
        let pairs = (0..32)
            .filter(|&slot| wram[0x1CC0 + slot] != 0)
            .map(|slot| (wram[0x1CC0 + slot] & 0x7F, wram[0x1DD0 + slot] >> 4))
            .collect();
        (cell, pairs)
    }

    /// A branch's path: a [`STARTS`] frame, the RAM pokes made there and the pads from
    /// there on (the oracle's bit order, as [`random_pads`]). Named `ExplorePath` because
    /// this module already imports `std::path::Path`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct ExplorePath {
        start: u64,
        pokes: Vec<(u16, u8)>,
        pads: Vec<u8>,
    }

    /// A branch that diverged from the oracle: the archive index it began at, the branch's
    /// own path and the lockstep's message.
    struct Divergence {
        index: usize,
        path: ExplorePath,
        message: String,
    }

    /// A branch's new segment length in frames, and the depth cap in segments.
    const SEGMENT: usize = 300;
    const MAX_SEGMENTS: usize = 8;

    /// A xorshift32 step: the archive's selection and its segments' seeds.
    fn xorshift(state: &mut u32) -> u32 {
        *state ^= *state << 13;
        *state ^= *state >> 17;
        *state ^= *state << 5;
        *state
    }

    /// Replays `branch` in one lockstep from `Scene::tas(branch.start)` with its pokes set,
    /// inside [`caught`]: `Ok(None)` when the TAS is missing, `Ok` with the run and the
    /// lockstep's final scene, `Err` with a divergence's message.
    fn explore_branch(branch: &ExplorePath) -> Result<Option<(GameRun, Scene)>, String> {
        let replay = branch.clone();
        caught(move || {
            let mut scene = Scene::tas(replay.start)?;
            for &(addr, value) in &replay.pokes {
                scene.machine.bus.ram[usize::from(addr)] = value;
            }
            let frames = u64::try_from(replay.pads.len()).expect("pads fit in u64");
            let pads = replay.pads.clone();
            scene.script(frames, |i| pads[usize::try_from(i).expect("small")]);
            let (run, scene) = super::oracle::assert_game_scene(scene, frames);
            Some((run, scene))
        })
    }

    /// `branches` novelty-guided branches from `seed`: the archive seeded with [`STARTS`]
    /// and weapons 0-5, each branch extending one of the 16 newest entries by one
    /// [`SEGMENT`] (depth-capped at [`MAX_SEGMENTS`]) and joining the archive when it
    /// reaches a new cell or `(type, phase)` pair. Returns the archive, the cells and
    /// pairs reached, and the divergences; `None` when the TAS is missing.
    fn explore_branches(branches: usize, seed: u32) -> Option<Exploration> {
        let mut stream = if seed == 0 { 1 } else { seed };
        let mut archive: Vec<(ExplorePath, usize)> = STARTS
            .iter()
            .flat_map(|&(start, ..)| {
                let weapons = (0..=5u8).map(|weapon| vec![(0x476, weapon)]);
                let endings = [DEATH, GAME_OVER].map(|pokes| [&[(0x476, 1)], pokes].concat());
                weapons.chain(endings).map(move |pokes| {
                    (
                        ExplorePath {
                            start,
                            pokes,
                            pads: Vec::new(),
                        },
                        0,
                    )
                })
            })
            .collect();
        let (mut cells, mut pairs) = (BTreeSet::new(), BTreeSet::new());
        let mut divergences = Vec::new();
        for index in 0..branches {
            let open: Vec<usize> = (0..archive.len())
                .filter(|&i| archive[i].0.pads.len() < MAX_SEGMENTS * SEGMENT)
                .collect();
            let candidates = if index % 4 == 3 {
                &open[..]
            } else {
                &open[open.len().saturating_sub(16)..]
            };
            if candidates.is_empty() {
                continue;
            }
            let pick =
                candidates[usize::try_from(xorshift(&mut stream)).unwrap_or(0) % candidates.len()];
            let (parent, parent_frames) = archive[pick].clone();
            let segment = xorshift(&mut stream).max(1);
            let branch = ExplorePath {
                start: parent.start,
                pokes: parent.pokes.clone(),
                pads: [parent.pads.as_slice(), &random_pads(segment, SEGMENT)].concat(),
            };
            match explore_branch(&branch) {
                Ok(None) => return None,
                Ok(Some((run, scene))) => {
                    if run.frames <= parent_frames {
                        continue;
                    }
                    let (cell, found) = features(&scene.machine.bus.ram, &scene.machine.bus.wram);
                    let mut novel = cells.insert(cell);
                    for pair in found {
                        novel |= pairs.insert(pair);
                    }
                    if novel {
                        archive.push((branch, run.frames));
                    }
                }
                Err(message) => divergences.push(Divergence {
                    index,
                    path: branch,
                    message,
                }),
            }
        }
        let archive = archive.into_iter().map(|(path, _)| path).collect();
        Some((archive, cells, pairs, divergences))
    }

    /// The TAS's own novelty keys: every frame's [`features`] from the stage 1-1 start to
    /// 2000 frames past the last [`STARTS`] start, so "how far off-route" is a number.
    fn tas_features() -> Option<Keys> {
        let mut scene = Scene::tas(350)?;
        let last = STARTS.last().expect("the starts").0 + 2000;
        let (mut cells, mut pairs) = (BTreeSet::new(), BTreeSet::new());
        while scene.machine.frame < last {
            scene.run(1, &[]);
            let (cell, found) = features(&scene.machine.bus.ram, &scene.machine.bus.wram);
            cells.insert(cell);
            pairs.extend(found);
        }
        Some((cells, pairs))
    }

    /// Novelty-guided exploration: seeds the archive with [`STARTS`] x weapons 0-5 and
    /// extends the 16 newest entries toward states no run has seen (spec section 5).
    /// `ROTJ_EXPLORE_BRANCHES` (2000) and `ROTJ_EXPLORE_SEED` (1) choose the run.
    #[test]
    #[ignore = "novelty-guided exploration (release, on demand); its pins are the gates"]
    fn explore() {
        let branches: usize = std::env::var("ROTJ_EXPLORE_BRANCHES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(2000);
        let seed: u32 = std::env::var("ROTJ_EXPLORE_SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        // The explorer runs alone (ignored) and catches each branch's divergence itself.
        std::panic::set_hook(Box::new(|_| {}));
        let Some((archive, cells, pairs, divergences)) = explore_branches(branches, seed) else {
            println!("skipped: no ROM/TAS");
            return;
        };
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/explore");
        if !divergences.is_empty() {
            std::fs::create_dir_all(&dir).expect("target/explore");
        }
        for divergence in &divergences {
            let pokes = if divergence.path.pokes.is_empty() {
                "none".to_owned()
            } else {
                divergence
                    .path
                    .pokes
                    .iter()
                    .map(|(addr, value)| format!("{addr:04X}={value:02X}"))
                    .collect::<Vec<_>>()
                    .join("_")
            };
            let file = dir.join(format!(
                "{}-start{}-pokes-{pokes}.pads",
                divergence.index, divergence.path.start
            ));
            std::fs::write(&file, &divergence.path.pads).expect("write the pads");
            println!(
                "DIVERGES start {} pokes {:?} pads {}: {}",
                divergence.path.start,
                divergence.path.pokes,
                file.display(),
                divergence.message
            );
        }
        println!(
            "explored {branches} branches (seed {seed}): archive {}, {} cells, {} pairs, {} divergences",
            archive.len(),
            cells.len(),
            pairs.len(),
            divergences.len()
        );
        if let Some((tas_cells, tas_pairs)) = tas_features() {
            println!(
                "the TAS reaches {} cells and {} pairs; beyond it: {} cells, {} pairs",
                tas_cells.len(),
                tas_pairs.len(),
                cells.difference(&tas_cells).count(),
                pairs.difference(&tas_pairs).count()
            );
        }
    }

    /// [`assert_game_scene`] hands back the lockstep's own final scene: the state it ended in.
    #[test]
    fn the_final_scene_is_the_oracles() {
        let Some(scene) = Scene::tas(350) else {
            return;
        };
        let (_run, final_scene) = super::oracle::assert_game_scene(scene, 120);
        assert_eq!(final_scene.machine.frame, 470);
        let Some(mut plain) = Scene::tas(350) else {
            return;
        };
        plain.run(120, &[]);
        assert_eq!(final_scene.machine.bus.ram, plain.machine.bus.ram);
        assert_eq!(final_scene.machine.bus.wram, plain.machine.bus.wram);
    }

    /// [`features`] on a state the TAS reaches: the pinned cell and `(type, phase)` pairs.
    #[test]
    fn features_of_a_known_frame() {
        let Some(scene) = Scene::tas(900) else {
            return;
        };
        let (cell, pairs) = features(&scene.machine.bus.ram, &scene.machine.bus.wram);
        assert_eq!(cell, (0, 2, 0, 2), "{pairs:?}");
        assert_eq!(pairs, BTreeSet::from([(4, 0)]), "{cell:?}");
    }

    /// The same seed and branch count explore the same way.
    #[test]
    #[ignore = "20 branches twice (release); the gates run it in release with --ignored"]
    fn the_same_seed_explores_the_same_way() {
        let Some((first, ..)) = explore_branches(20, 1) else {
            return;
        };
        let Some((second, ..)) = explore_branches(20, 1) else {
            return;
        };
        assert_eq!(first, second);
    }

    /// A branch that ends in a cutscene compares no loop frame past its parent and is
    /// skipped: [`explore_branch`] returns, never panics.
    #[test]
    #[ignore = "a branch into the banner cutscene (release); the gates run it in release with --ignored"]
    fn a_branch_into_a_cutscene_is_skipped() {
        // The stage 1 boss's ending runs into the stage 2 banner cutscene (`$98BD`): a
        // 300-frame segment begun at TAS 6900 is all script, so no loop frame is compared.
        let branch = ExplorePath {
            start: 6900,
            pokes: Vec::new(),
            pads: random_pads(1, SEGMENT),
        };
        let Some(_) = Scene::tas(branch.start) else {
            return;
        };
        let result = explore_branch(&branch);
        match result {
            Ok(Some((run, ..))) => {
                // The whole segment is inside the cutscene: no loop frame is compared.
                assert_eq!(run.frames, 0, "{run:?}");
            }
            Ok(None) => {}
            Err(message) => panic!("explore_branch panicked into a cutscene: {message}"),
        }
    }

    #[test]
    fn the_nmi_wait_spins_the_rng_415_times_at_the_median() {
        let Some(mut scene) = Scene::tas(351) else {
            return;
        };
        let events = scene.run(1700, &[0xD45C, 0xC23F]);
        let mut spins: Vec<usize> = events
            .windows(2)
            .filter(|w| (w[0].pc, w[1].pc) == (0xD45C, 0xC23F))
            .filter_map(|w| {
                let mut rng = super::world::Rng([w[0].ram[0xF9], w[0].ram[0xFA]]);
                (0..5000).find(|_| {
                    let found = rng.0 == [w[1].ram[0xF9], w[1].ram[0xFA]];
                    rng.next_frame();
                    found
                })
            })
            .collect();
        spins.sort_unstable();
        assert_eq!(spins.len(), 1548);
        assert_eq!(spins[spins.len() / 2], super::RNG_SPINS);
    }

    /// A recording as the app writes it: [`random_pads`] in the game's bit order (`$F6`).
    fn recording(seed: u32, frames: usize) -> Vec<u8> {
        let pads = random_pads(seed, frames);
        pads.iter().map(|pad| pad.reverse_bits()).collect()
    }

    /// Every level's start as `room::start` builds it (the scenes the app's tests start
    /// from, `Play::start`) plays 300 frames of no input like the game started from the
    /// title with Start on frame 300.
    #[test]
    #[ignore = "13 fresh starts in lockstep (10 s in release); the gates run it in release with --ignored"]
    fn every_fresh_level_start_matches() {
        for level in 0..13 {
            let Some((scene, start)) = fresh_start(level, &[0; 300]) else {
                return;
            };
            let peek = scene.clone().run(1, &[0xD3F9]).remove(0);
            let world = &start.world;
            assert_eq!(
                (
                    peek.ram[0xF9],
                    peek.ram[0xFA],
                    peek.ram[0xF1],
                    peek.ram[0xF7] >> 7
                ),
                (
                    world.rng[0],
                    world.rng[1],
                    world.frame,
                    u8::from(world.frame_odd)
                ),
                "level {level}"
            );
            let run = assert_game_from(scene, 300, Some(start));
            assert!(run.frames >= 250, "level {level}: {run:?}");
        }
    }

    /// A recording from power-on: the title left alone, Start, the stage start, then
    /// `pads` random pads.
    fn from_the_title(pads: usize) -> Vec<u8> {
        let recording_of = |frames| recording(2, frames);
        let mut recording = vec![0; 300];
        recording.push(0x10);
        recording.extend([0; 400]);
        recording.extend(recording_of(pads));
        recording
    }

    /// Level 0 through a room change and two pauses, and level 5 (forced) into its next
    /// area, each from power-on through the title.
    #[test]
    #[ignore = "two recordings in lockstep; the gates run it in release with --ignored"]
    fn a_recording_replays() {
        let Some(run) = assert_playtest(0, &from_the_title(2000)) else {
            return;
        };
        assert_eq!(
            (run.frames, run.room_changes, run.pauses),
            (1880, 1, 2),
            "{run:?}"
        );
        let Some(run) = assert_playtest(5, &from_the_title(2000)) else {
            return;
        };
        assert_eq!((run.frames, run.next_area), (1782, 1), "{run:?}");
    }

    #[test]
    #[ignore = "a recording in lockstep; the gates run it in release with --ignored"]
    fn the_app_nmi_is_checked() {
        let Some(mut scene) = playtest_scene(0, &recording(1, 200)) else {
            return;
        };
        let playtest = scene
            .machine
            .bus
            .trace
            .playtest
            .as_mut()
            .expect("a playtest");
        playtest.spins += 1;
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assert_boot(scene, 200, None)
        }));
        let message = caught.expect_err("the oracle's RNG is off by one spin");
        let message = message.downcast_ref::<String>().expect("a formatted panic");
        assert!(
            message.contains("the app's NMI differs: rng at 0s of play (pad 1)"),
            "{message}"
        );
    }

    /// Too short to reach the first loop frame's compare: ends cleanly at 0 frames.
    #[test]
    #[ignore = "power-on starts in lockstep; the gates run it in release with --ignored"]
    fn an_empty_or_one_pad_recording_replays() {
        for pads in [&[][..], &[0x01]] {
            let Some(run) = assert_playtest(0, pads) else {
                return;
            };
            assert_eq!(run.frames, 0, "{run:?}");
        }
    }

    /// The level in a recording's name, `<millis>[-2]-level<N>.pads`.
    fn playtest_level(path: &Path) -> Option<u8> {
        let name = path.file_name()?.to_str()?.strip_suffix(".pads")?;
        let level = name.rsplit_once("-level")?.1.parse().ok()?;
        (level < 13).then_some(level)
    }

    #[test]
    fn a_recordings_name_carries_its_level() {
        let level = |name: &str| playtest_level(Path::new(name));
        assert_eq!(level("/tmp/1760000000000-level3.pads"), Some(3));
        assert_eq!(level("1760000000000-2-level12.pads"), Some(12));
        assert_eq!(level("1760000000000-level13.pads"), None);
        assert_eq!(level("1760000000000-level3.bin"), None);
    }

    /// Replays the session the app recorded at `ROTJ_PLAYTEST` (`ROTJ_RECORD_INPUT`) from
    /// power-on: the first byte the port gets wrong, or a pass.
    #[test]
    #[ignore = "on demand: ROTJ_PLAYTEST=<file> replays an app recording against the oracle"]
    fn the_playtest_matches_the_oracle() {
        let Some(path) = std::env::var_os("ROTJ_PLAYTEST") else {
            return;
        };
        let path = PathBuf::from(path);
        let pads = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(
            pads.len() <= 216_000,
            "playtest too long: {} frames, an hour is 216000",
            pads.len()
        );
        let level = playtest_level(&path)
            .unwrap_or_else(|| panic!("{}: not <millis>-level<N>.pads", path.display()));
        let Some(run) = assert_playtest(level, &pads) else {
            return;
        };
        let end = match (run.reset, run.hung) {
            (true, _) => ", ending at GAME OVER's reset",
            (_, true) => ", ending in a draw that hangs the game",
            _ => "",
        };
        println!(
            "playtest {}: {} loop frames match{end} ({} lag NMIs)",
            path.display(),
            run.frames,
            run.injected
                .get("NMI $E1D0 outside the wait (a lag frame)")
                .unwrap_or(&0)
        );
    }

    /// The stage 1 boss's end into stage 2's banner cutscene: the first direct VRAM write
    /// flipped in the port's NMI is caught by the VRAM compare.
    #[test]
    #[ignore = "a lockstep through the banner cutscene; the gates run it in release with --ignored"]
    fn a_changed_direct_write_is_caught() {
        let Some(scene) = Scene::tas(6700) else {
            return;
        };
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            super::oracle::assert_game_with_a_flipped_direct_write(scene, 900)
        }));
        let message = caught.expect_err("the flipped byte is caught");
        let message = message.downcast_ref::<String>().expect("a formatted panic");
        assert!(message.contains("VRAM table"), "{message}");
    }

    /// The port's `$D1` flipped after its first raster IRQs in stage 1-2 (area 2, `$D3`
    /// 0) is caught by the IRQ compare.
    #[test]
    #[ignore = "a lockstep into stage 1-2; the gates run it in release with --ignored"]
    fn a_changed_raster_byte_is_caught() {
        let Some(scene) = Scene::tas(2150) else {
            return;
        };
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            super::oracle::assert_game_with_a_flipped_raster_byte(scene, 300)
        }));
        let message = caught.expect_err("the flipped byte is caught");
        let message = message.downcast_ref::<String>().expect("a formatted panic");
        assert!(message.contains("$00D1 after the raster IRQs"), "{message}");
    }

    /// `$3F10`/`$3F14` land on entries 0 and 4 in the port's `Ppu` as in the oracle's
    /// PPU, compared masked `& 0x3F` (the oracle stores entries masked).
    #[test]
    fn the_palette_mirror_agrees() {
        let writes = [(0x10, 0xC1), (0x14, 0xC2), (0x1D, 0xC3)];
        let mut program = Vec::new();
        for (low, value) in writes {
            program.extend([0xA9, 0x3F, 0x8D, 0x06, 0x20, 0xA9, low, 0x8D, 0x06, 0x20]);
            program.extend([0xA9, value, 0x8D, 0x07, 0x20]);
        }
        program.extend([0x4C, 0x2D, 0xE0]);
        let mut prg = vec![0; 0x20000];
        let last = prg.len() - 0x2000;
        prg[last..last + program.len()].copy_from_slice(&program);
        let end = prg.len();
        prg[end - 4..end - 2].copy_from_slice(&0xE000u16.to_le_bytes());
        let mut machine = rotj_oracle::Machine::power_on(rotj_oracle::Cart {
            prg,
            chr: Vec::new(),
        });
        machine.run_frame(0);
        let mut ppu = super::world::Ppu::new();
        let queued: Vec<_> = writes
            .iter()
            .map(|&(low, value)| super::world::VramWrite {
                addr: 0x3F00 | u16::from(low),
                flags: 0,
                bytes: vec![value],
            })
            .collect();
        ppu.apply(&queued, Mirroring::default());
        let port = ppu.palette.map(|entry| entry & 0x3F);
        let oracle = machine.bus.ppu.palette;
        assert_eq!((port[0], port[4], port[0x1D]), (0x01, 0x02, 0x03));
        assert_eq!(
            (oracle[0], oracle[4], oracle[0x1D]),
            (port[0], port[4], port[0x1D])
        );
    }

    /// The entries `mark` holds, sorted by `(y, tile, attr, x)`.
    fn marked(oam: &super::oam::OamBuilder, mark: super::oam::Mark) -> Vec<super::oam::OamEntry> {
        let chunks = oam.page.as_chunks::<4>().0;
        let mut entries: Vec<super::oam::OamEntry> = (0..64)
            .filter(|&entry| oam.marks[entry] == mark)
            .map(|entry| super::oam::OamEntry::from(chunks[entry]))
            .collect();
        entries.sort_unstable();
        entries
    }

    /// The HUD is a loop frame's first draw, so `draw_hud` alone on the frame's start
    /// (its OAM cleared) places what the frame marks `Hud` and `Label`.
    #[test]
    fn the_hud_marks_exactly_its_pieces() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let prg = crate::prg::for_test().expect("the test ROM");
        let mut game = super::room::start(prg, &rom.chr, 0);
        let mut labels = 0;
        for _ in 0..8 {
            game.nmi(0);
            let mut alone = game.clone();
            alone.oam.clear();
            alone.oam.marking = super::oam::Mark::Hud;
            super::frame::draw_hud(&mut alone, prg);
            game.frame(prg, &rom.chr);
            for mark in [super::oam::Mark::Hud, super::oam::Mark::Label] {
                assert_eq!(
                    marked(&game.oam, mark),
                    marked(&alone.oam, mark),
                    "{mark:?}"
                );
            }
            let label = marked(&game.oam, super::oam::Mark::Label);
            assert_eq!(!label.is_empty(), alone.world.frame_odd);
            labels += usize::from(!label.is_empty());
            assert_ne!(marked(&game.oam, super::oam::Mark::Hud).len(), 0);
        }
        assert_eq!(labels, 4, "the label on every other frame");
    }

    /// The stage 1 boss card queues the palette, then its first fade (`$BB65`, `$BBF8`);
    /// the game's NMI at TAS 6116 writes the fade, then the palette.
    #[test]
    fn the_nmi_drains_the_queue_last_pushed_first() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let mut game =
            super::room::start(crate::prg::for_test().expect("the test ROM"), &rom.chr, 0);
        game.effects.vram.direct.clear();
        game.effects.vram.push_palette(&[0x0F, 0x26]);
        game.effects.vram.push_palette(&[0x0F, 0x06]);
        let writes = game.nmi(0).writes;
        let bytes: Vec<_> = writes.iter().map(|write| write.bytes[1]).collect();
        assert_eq!(bytes, [0x06, 0x26]);
    }

    /// `$C1DB` masks Left+Right (and Up+Down) out of the held pad but ANDs the new
    /// presses with the raw read: at playtest frame 9667 Right comes back as pressed.
    #[test]
    fn opposite_directions_press_from_the_raw_read() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let mut game =
            super::room::start(crate::prg::for_test().expect("the test ROM"), &rom.chr, 0);
        game.nmi(0xC1);
        game.nmi(0xC3);
        assert_eq!(
            (game.world.pad.bits(), game.world.pressed.bits()),
            (0xC2, 0x03)
        );
    }
}
