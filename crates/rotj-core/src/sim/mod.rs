//! The game simulation: [`Session`], slot state, spawning, the actor loop, and drawing.

mod attract;
pub(crate) mod boot;
pub(crate) mod button;
pub(crate) mod collision;
pub(crate) mod combat;
pub(crate) mod cutscene;
pub(crate) mod ending;
pub(crate) mod engine;
pub(crate) mod families;
pub(crate) mod frame;
pub mod modern;
pub mod oam;
#[cfg(test)]
pub(crate) mod oracle;
pub(crate) mod pause;
pub(crate) mod player;
pub(crate) mod raster;
pub(crate) mod room;
pub(crate) mod script;
pub(crate) mod scroll;
pub mod session;
pub(crate) mod slots;
pub(crate) mod sound_test;
pub(crate) mod spawn;
pub(crate) mod stage;
#[cfg(test)]
mod tape;
pub(crate) mod tile_block;
pub(crate) mod weapons;
pub(crate) mod world;

pub use button::Buttons;
pub use session::{DrawerState, FrameSnapshot, Session};
pub use stage::level_area;
pub use world::{Motion, Ppu, SoundId, Transition};

use crate::prg::Prg;
use crate::render::ctrl;
use engine::{Work, anchor};
use oam::OamBuilder;
use slots::Slots;
use world::{Cx, Effects, Schedule, World, busy, stage_script};

/// Everything the game loop reads and writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Game {
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
pub(crate) const RNG_SPINS: usize = 415;

/// A script that runs in place of the loop body, one frame per [`Game::frame`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Script {
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
            Transition::None => None,
            Transition::RoomChange => Some(Script::RoomChange(room::RoomChange::new(
                self.effects.transition_arg,
            ))),
            Transition::StageEnd => stage::stage_end(self, prg).then_some(Script::StageEnded),
            Transition::NextArea => Some(Script::NextArea(stage::NextArea::default())),
            Transition::LifeLost => Some(Script::NextArea(stage::NextArea::life_lost())),
            Transition::PowerUp => Some(Script::PowerUp(stage::PowerUp::default())),
            Transition::Ending => Some(Script::Ending(ending::Ending::default())),
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
    pub fn nmi(&mut self, pad: Buttons) -> Vblank {
        let mut rng = world::Rng(self.world.rng);
        for _ in 0..RNG_SPINS {
            rng.next_frame();
        }
        self.world.rng = rng.0;
        let world = &mut self.world;
        world.frame = world.frame.wrapping_add(1);
        world.frame_odd = !world.frame_odd;
        let mut held = pad;
        if held.contains(Buttons::SIDEWAYS) {
            held &= !Buttons::RIGHT;
        }
        if held.contains(Buttons::UP_OR_DOWN) {
            held &= !Buttons::UP;
        }
        world.pressed = Buttons::from_bits((held.bits() ^ world.pad.bits()) & pad.bits());
        world.pad = held;
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
pub(crate) struct Vblank {
    /// PPU writes, in order.
    pub writes: Vec<world::VramWrite>,
    /// The sound driver's requests, `$40-$43`.
    pub sound: [Option<SoundId>; 4],
}

/// One pass of the actor loop over slots 23 down to 0 (bank 14 `$CED5`).
pub(crate) fn step_actors(prg: &Prg, game: &mut Game) {
    let marking = game.oam.marking;
    for index in (0..game.slots.main.len()).rev() {
        if game.slots.main[index].kind == 0 {
            continue;
        }
        let mut slot = std::mem::take(&mut game.slots.main[index]);
        game.slots.main[index].kind = slot.kind;
        let loaded = slots::load_aliases(
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
            &loaded,
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
pub(crate) fn step_second(prg: &Prg, game: &mut Game) {
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
pub(crate) fn draw_second(
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
mod tests;
