//! The host-facing simulation session: owns [`Game`] and [`Ppu`], steps one 60 Hz tick at a
//! time, composes the Classic PPU picture, and exposes read-only snapshots for the Modern
//! renderer, faithful UI overlay, and ECS mirror.

use super::button::Buttons;
use super::modern::ModernFrame;
use super::oam::{self, DrawRecord, Mark};
use super::stage::StageEnd;
use super::world::{Motion, Ppu, SoundId, Transition};
use super::{Game, Script, raster, room};
use crate::levels::Levels;
use crate::map::Chain;
use crate::prg::Prg;
use crate::render::{Frame, Line, Mirroring, Picture};

/// One drawer's state in a [`FrameSnapshot`], in the game's 1/16 px level coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawerState {
    /// Which drawer this entry represents (an actor/second/projectile slot or Batman).
    pub drawer: Mark,
    /// The slot's type byte (`0` when empty), or `None` for Batman.
    pub kind: Option<u8>,
    /// Level x in 1/16 px.
    pub x: u16,
    /// Level y in 1/16 px.
    pub y: u16,
}

/// Read-only view of the frame the NMI just presented, for the mirror and Modern renderer.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FrameSnapshot {
    /// The page's draw records.
    pub records: Vec<DrawRecord>,
    /// Every drawer: main slots, second-class slots, projectiles, then Batman.
    pub drawers: Vec<DrawerState>,
    /// Camera top-left `(x, y)` in 1/16 px.
    pub camera: (u16, u16),
    /// What the Modern picture draws of this frame, or `None` when it falls back to Classic.
    pub modern: Option<ModernFrame>,
}

impl FrameSnapshot {
    fn of(game: &Game, levels: &Levels, lines: &[Line]) -> FrameSnapshot {
        let slot = |drawer, kind, x, y| DrawerState {
            drawer,
            kind: Some(kind),
            x,
            y,
        };
        let actors = (0u8..)
            .zip(&game.slots.main)
            .map(|(index, actor)| slot(Mark::Actor(index), actor.kind, actor.body.x, actor.body.y));
        let seconds = (0u8..).zip(&game.slots.second).map(|(index, second)| {
            slot(
                Mark::Second(index),
                second.kind,
                second.body.x,
                second.body.y,
            )
        });
        let projectiles = (0u8..)
            .zip(&game.effects.projectiles)
            .map(|(index, shot)| slot(Mark::Projectile(index), shot.kind, shot.x, shot.y));
        let player = &game.effects.player;
        let batman = DrawerState {
            drawer: Mark::Player,
            kind: None,
            x: player.x,
            y: player.y,
        };
        FrameSnapshot {
            records: game.oam.shown.clone(),
            drawers: actors
                .chain(seconds)
                .chain(projectiles)
                .chain([batman])
                .collect(),
            camera: (game.world.camera_x, game.world.camera_y),
            modern: ModernFrame::of(game, levels, lines),
        }
    }
}

/// A live simulation session owning [`Game`] and [`Ppu`] and stepping frame-by-frame.
#[derive(Debug, Clone)]
pub struct Session {
    game: Game,
    ppu: Ppu,
    /// The NMI armed the raster IRQ (`$D0` bit 7), so this frame is split.
    raster: bool,
    /// The scanlines the PPU draws this frame with.
    lines: Vec<Line>,
    /// The OAM page the NMI copied.
    oam: [u8; 256],
    /// Which drawer placed each OAM sprite.
    marks: [Mark; 64],
    /// Snapshot of the drawers and Modern view taken at the NMI.
    snapshot: FrameSnapshot,
    /// The level forced at `$D3D8`, kept across GAME OVER resets.
    forced: Option<u8>,
}

impl Session {
    /// Powers the game on and advances to its first NMI wait; `forced` overrides the level
    /// the title starts.
    #[must_use]
    pub fn power_on(prg: &Prg, chr: &[u8], forced: Option<u8>) -> Session {
        Session::running(Game::power_on(forced), prg, chr, Ppu::new(), forced)
    }

    /// Starts directly at `level`'s first loop frame, with its first page already in the
    /// nametables.
    #[must_use]
    pub fn start(prg: &Prg, chr: &[u8], level: usize) -> Session {
        let mut game = room::start(prg, chr, level);
        let mut ppu = Ppu::new();
        let page = std::mem::take(&mut game.effects.vram.direct);
        ppu.apply(&page, game.scroll.mirroring);
        ppu.palette = game.effects.palette;
        Session::running(game, prg, chr, ppu, None)
    }

    fn running(game: Game, prg: &Prg, chr: &[u8], ppu: Ppu, forced: Option<u8>) -> Session {
        let mut session = Session {
            game,
            ppu,
            raster: false,
            lines: Vec::new(),
            oam: [0; 256],
            marks: [Mark::Scene; 64],
            snapshot: FrameSnapshot::default(),
            forced,
        };
        let registers = raster::Registers::of(&session.game);
        session.copy_page(&registers, &prg.levels);
        session.run_frame(registers, prg, chr);
        session
    }

    fn run_frame(&mut self, registers: raster::Registers, prg: &Prg, chr: &[u8]) {
        let mut registers = registers;
        let split_at_nmi = self.game.scroll.split.armed.then(|| self.game.clone());
        self.game.frame(prg, chr);
        if self.game.reset() {
            self.game = Game::power_on(self.forced);
            self.raster = false;
            self.game.frame(prg, chr);
            registers = raster::Registers::of(&self.game);
        }
        let direct = std::mem::take(&mut self.game.effects.vram.direct);
        self.ppu.apply(&direct, self.game.scroll.mirroring);
        self.lines = raster::lines(&registers, &self.game);
        if let Some(game) = split_at_nmi {
            self.snapshot.modern = ModernFrame::of(&game, &prg.levels, &self.lines);
        }
    }

    fn copy_page(&mut self, registers: &raster::Registers, levels: &Levels) {
        self.oam = self.game.oam.page;
        self.marks = self.game.oam.marks;
        self.snapshot =
            FrameSnapshot::of(&self.game, levels, &raster::lines(registers, &self.game));
    }

    /// Runs the last frame's raster IRQs, an NMI with joypad 1 held as `pad`, and the next
    /// frame of the game; returns the sound requests the NMI hands the music driver (`$40-$43`).
    pub fn step(&mut self, pad: Buttons, prg: &Prg, chr: &[u8]) -> [Option<SoundId>; 4] {
        if self.raster {
            raster::run_irqs(&mut self.game);
        }
        let vblank = self.game.nmi(pad);
        self.raster = self.game.scroll.split.armed;
        self.ppu.apply(&vblank.writes, self.game.scroll.mirroring);
        let registers = raster::Registers::of(&self.game);
        self.copy_page(&registers, &prg.levels);
        self.run_frame(registers, prg, chr);
        vblank.sound
    }

    /// Composes the PPU picture into `frame`; with `park`, moves UI-marked sprites off screen.
    pub fn compose(&self, chr: &[u8], frame: &mut Frame, sprites_per_line: usize, park: bool) {
        frame.render(
            &Picture {
                lines: &self.lines,
                vram: self.ppu.nametables.as_flattened(),
                palette: &self.ppu.palette,
                oam: &self.display_oam(park),
                chr,
            },
            sprites_per_line,
        );
    }

    /// The sprites the picture shows: with `park`, the game's UI sprites moved off screen.
    #[must_use]
    pub fn display_oam(&self, park: bool) -> [u8; 256] {
        let mut oam = self.oam;
        if park {
            for (entry, mark) in self.marks.iter().enumerate() {
                if mark.is_ui() {
                    oam::park(&mut oam, entry);
                }
            }
        }
        oam
    }

    /// True until the boot reaches the first loop frame.
    #[must_use]
    pub fn booting(&self) -> bool {
        matches!(&self.game.script, Some(Script::Boot(boot)) if !boot.done())
    }

    /// True while the game's own pause is up (`$9C37`).
    #[must_use]
    pub fn paused(&self) -> bool {
        matches!(self.game.script, Some(Script::Pause(_)))
    }

    /// True while the boot script is sitting on the title screen.
    #[must_use]
    pub fn on_title(&self) -> bool {
        matches!(&self.game.script, Some(Script::Boot(boot)) if boot.on_title())
    }

    /// Current `(area, room)` identifier, used by the host to detect room changes without
    /// inspecting simulation internals.
    #[must_use]
    pub const fn room_id(&self) -> (u8, u8) {
        (self.game.world.area, self.game.world.room)
    }

    /// True when any script is running instead of the gameplay loop.
    #[must_use]
    pub fn in_script(&self) -> bool {
        self.game.script.is_some()
    }

    /// True when the ending script is running and has reached its final screen.
    #[must_use]
    pub fn ending_over(&self) -> bool {
        matches!(&self.game.script, Some(Script::Ending(ending)) if ending.over())
    }

    /// Current area byte (`$0400`).
    #[must_use]
    pub const fn area(&self) -> u8 {
        self.game.world.area
    }

    /// Current room byte (`$0402`).
    #[must_use]
    pub const fn room(&self) -> u8 {
        self.game.world.room
    }

    /// Current room orientation byte (`$DD`).
    #[must_use]
    pub const fn orientation(&self) -> u8 {
        self.game.world.orientation
    }

    /// Frame counter (`$06`).
    #[must_use]
    pub const fn frame(&self) -> u8 {
        self.game.world.frame
    }

    /// Remaining lives (`$0413`).
    #[must_use]
    pub const fn lives(&self) -> u8 {
        self.game.effects.lives
    }

    /// Current camera top-left `(x, y)` in 1/16 px.
    #[must_use]
    pub const fn camera(&self) -> (u16, u16) {
        (self.game.world.camera_x, self.game.world.camera_y)
    }

    /// Player level x in 1/16 px.
    #[must_use]
    pub const fn player_x(&self) -> u16 {
        self.game.effects.player.x
    }

    /// Player level y in 1/16 px.
    #[must_use]
    pub const fn player_y(&self) -> u16 {
        self.game.effects.player.y
    }

    /// Player health/energy byte.
    #[must_use]
    pub const fn player_energy(&self) -> u8 {
        self.game.effects.player.energy
    }

    /// Player motion state.
    #[must_use]
    pub const fn player_motion(&self) -> Motion {
        self.game.effects.player.motion
    }

    /// Armed raster split chain on this frame, if any.
    #[must_use]
    pub const fn raster_chain(&self) -> Option<Chain> {
        if self.raster {
            Some(self.game.scroll.split.chain)
        } else {
            None
        }
    }

    /// Whether any player projectile slot is active.
    #[must_use]
    pub fn has_active_projectile(&self) -> bool {
        self.game
            .effects
            .projectiles
            .iter()
            .any(|shot| shot.kind != 0)
    }

    /// Current PPU nametable and palette state.
    #[must_use]
    pub const fn ppu(&self) -> &Ppu {
        &self.ppu
    }

    /// Current frame's per-scanline PPU state.
    #[must_use]
    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    /// Current frame's copied OAM page.
    #[must_use]
    pub const fn oam(&self) -> &[u8; 256] {
        &self.oam
    }

    /// Which drawer placed each entry in [`Self::oam`].
    #[must_use]
    pub const fn marks(&self) -> &[Mark; 64] {
        &self.marks
    }

    /// Read-only snapshot taken at the NMI for the ECS mirror and Modern renderer.
    #[must_use]
    pub const fn snapshot(&self) -> &FrameSnapshot {
        &self.snapshot
    }

    /// Arms a `$BA`/`$BB` transition on the next frame (scene-setup helper for tests).
    #[doc(hidden)]
    pub fn request_transition(&mut self, kind: Transition, arg: u8) {
        self.game.effects.transition = kind;
        self.game.effects.transition_arg = arg;
    }

    /// Arms a boss-room entry transition (`$0404 = $80`, `$BA = NextArea`, scene-setup helper).
    #[doc(hidden)]
    pub fn arm_boss_entry(&mut self) {
        self.game.effects.hud = 0x80;
        self.game.effects.transition = Transition::NextArea;
    }

    /// Arms a post-boss cutscene and next-area load (scene-setup helper).
    #[doc(hidden)]
    pub fn arm_cutscene_next_area(&mut self, cutscene_id: u8) {
        self.game.effects.cutscene_id = cutscene_id;
        self.game.effects.hud = 0;
        self.game.effects.transition = Transition::NextArea;
    }

    /// Arms the stage-end departure sequence at screen `(x, y)` (scene-setup helper).
    #[doc(hidden)]
    pub fn arm_stage_end_depart(&mut self, x: u8, y: u8) {
        self.game.effects.transition = Transition::StageEnd;
        self.game.effects.stage_end = StageEnd {
            phase: 2,
            x,
            y,
            ..StageEnd::default()
        };
    }

    /// Offsets the player's level y coordinate (scene-setup helper).
    #[doc(hidden)]
    pub fn nudge_player_y(&mut self, delta: i16) {
        self.game.effects.player.y = self.game.effects.player.y.wrapping_add_signed(delta);
    }

    /// Marks every non-solid metatile class in the current level as a hazard (scene-setup helper).
    #[doc(hidden)]
    pub fn make_non_solid_tiles_hazard(&mut self) {
        for class in &mut self.game.effects.level.classes {
            if *class != 0 {
                *class = 6;
            }
        }
    }

    /// Triggers death on the first live final-boss part (`$B6`) and returns its nametable
    /// death-tile destination and active mirroring (scene-setup helper).
    #[doc(hidden)]
    pub fn destroy_final_boss_part(&mut self, prg: &Prg) -> Option<(u16, Mirroring)> {
        let part = self
            .game
            .slots
            .main
            .iter_mut()
            .find(|slot| slot.kind == 0xB6)?;
        part.body.health |= 0x8000;
        let dest = prg.families.final_boss_part.death_dests[usize::from(part.body.subtype)];
        Some((dest, self.game.scroll.mirroring))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(frame: usize) -> Buttons {
        Buttons::from_bits(if frame % 4 < 2 { 0x41 } else { 0x01 })
    }

    #[test]
    fn the_snapshot_takes_each_drawers_kind_and_position_from_the_game() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let Some(prg) = crate::prg::for_test() else {
            return;
        };
        let mut session = Session::start(prg, &rom.chr, 0);
        let fired = (0..120).any(|frame| {
            session.step(pad(frame), prg, &rom.chr);
            session.has_active_projectile()
        });
        assert!(fired, "a shot is out");
        let game = &session.game;
        let snapshot = FrameSnapshot::of(game, &prg.levels, session.lines());
        let state = |mark| {
            snapshot
                .drawers
                .iter()
                .find(|state| state.drawer == mark)
                .copied()
                .expect("every drawer")
        };
        for (index, actor) in (0u8..).zip(&game.slots.main) {
            let actor_state = state(Mark::Actor(index));
            assert_eq!(actor_state.kind, Some(actor.kind));
            assert_eq!((actor_state.x, actor_state.y), (actor.body.x, actor.body.y));
        }
        for (index, second) in (0u8..).zip(&game.slots.second) {
            assert_eq!(state(Mark::Second(index)).kind, Some(second.kind));
        }
        for (index, shot) in (0u8..).zip(&game.effects.projectiles) {
            let shot_state = state(Mark::Projectile(index));
            assert_eq!(shot_state.kind, Some(shot.kind));
            assert_eq!((shot_state.x, shot_state.y), (shot.x, shot.y));
        }
        let player = &game.effects.player;
        assert_eq!(state(Mark::Player).kind, None);
        assert_eq!(
            (state(Mark::Player).x, state(Mark::Player).y),
            (player.x, player.y)
        );
        assert_eq!(snapshot.records, game.oam.shown);
        assert_eq!(snapshot.drawers.len(), 24 + 8 + 16 + 1);
    }
}
