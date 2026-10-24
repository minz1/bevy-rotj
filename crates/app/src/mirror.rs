//! The read-only mirror: the game's drawers as entities, for the Modern picture, the
//! inspector and the editor (`docs/specs/2026-10-18-phase-c-mirror-design.md`, C2).

use bevy::prelude::*;
use rotj_core::actor::Game;
use rotj_core::actor::oam::{DrawRecord, Mark};
use rotj_core::render::Line;

use crate::pacing::Step;
use crate::play::Play;

/// The drawer an entity mirrors: a slot or Batman.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Drawer(pub Mark);

/// The slot's type byte, 0 when it is empty.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Kind(pub u8);

/// Where the drawer is in the level, in pixels, y down.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq)]
pub struct LevelPosition(pub Vec2);

/// The drawer's records for the frame on screen, in draw order.
#[derive(Component, Debug, Default, Clone, PartialEq)]
pub struct Draws(pub Vec<DrawRecord>);

/// The view's top-left in the level, in pixels, y down.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct MirrorCamera(pub Vec2);

/// The frame's records whose drawer is no entity: the scene's, the HUD's and the screens'.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct OtherDraws(pub Vec<DrawRecord>);

/// One drawer's state in a [`Snapshot`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawerState {
    pub drawer: Mark,
    /// `None` for Batman, who has no type byte.
    pub kind: Option<u8>,
    pub position: Vec2,
}

/// The game as the mirror shows it, taken when `Play` copies the page it shows.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Snapshot {
    /// The page's draw records.
    pub records: Vec<DrawRecord>,
    /// Every drawer, the slots in order then Batman.
    pub drawers: Vec<DrawerState>,
    pub camera: Vec2,
    /// What the Modern picture draws of the frame, when it draws it.
    pub modern: Option<crate::modern::ModernFrame>,
}

impl Snapshot {
    /// The game at its NMI, whose frame `lines` draws.
    #[must_use]
    pub fn of(game: &Game, lines: &[Line]) -> Snapshot {
        let slot = |drawer, kind, x, y| DrawerState {
            drawer,
            kind: Some(kind),
            position: level_pixels(x, y),
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
            position: level_pixels(player.x, player.y),
        };
        Snapshot {
            records: game.oam.shown.clone(),
            drawers: actors
                .chain(seconds)
                .chain(projectiles)
                .chain([batman])
                .collect(),
            camera: level_pixels(game.world.camera_x, game.world.camera_y),
            modern: crate::modern::ModernFrame::of(game, lines),
        }
    }
}

/// A position in the game's 1/16 px, in pixels.
fn level_pixels(x: u16, y: u16) -> Vec2 {
    Vec2::new(f32::from(x), f32::from(y)) / 16.0
}

/// Whether an entity mirrors `mark`'s draws.
fn has_entity(mark: Mark) -> bool {
    matches!(
        mark,
        Mark::Actor(_) | Mark::Second(_) | Mark::Projectile(_) | Mark::Player
    )
}

/// Spawns one entity per drawer, kept for the app's life so ids stay stable.
fn spawn(mut commands: Commands<'_, '_>) {
    let slots = (0..24)
        .map(Mark::Actor)
        .chain((0..8).map(Mark::Second))
        .chain((0..16).map(Mark::Projectile));
    for mark in slots {
        commands.spawn((
            Drawer(mark),
            Kind::default(),
            LevelPosition::default(),
            Draws::default(),
            Transform::default(),
            Visibility::default(),
        ));
    }
    commands.spawn((
        Drawer(Mark::Player),
        LevelPosition::default(),
        Draws::default(),
        Transform::default(),
        Visibility::default(),
    ));
}

/// Copies the snapshot `Play` took into the entities and resources, changing only what changed.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
fn copy(
    play: Res<'_, Play>,
    mut drawers: Query<'_, '_, (&Drawer, Option<&mut Kind>, &mut LevelPosition, &mut Draws)>,
    mut camera: ResMut<'_, MirrorCamera>,
    mut other: ResMut<'_, OtherDraws>,
) {
    let snapshot = &play.shown;
    for (drawer, kind, mut position, mut draws) in &mut drawers {
        let Some(state) = snapshot.drawers.iter().find(|s| s.drawer == drawer.0) else {
            continue;
        };
        if let (Some(mut kind), Some(byte)) = (kind, state.kind) {
            kind.set_if_neq(Kind(byte));
        }
        position.set_if_neq(LevelPosition(state.position));
        let records = snapshot.records.iter().filter(|r| r.mark == drawer.0);
        draws.set_if_neq(Draws(records.cloned().collect()));
    }
    camera.set_if_neq(MirrorCamera(snapshot.camera));
    let records = snapshot.records.iter().filter(|r| !has_entity(r.mark));
    other.set_if_neq(OtherDraws(records.cloned().collect()));
}

/// The mirror.
pub struct MirrorPlugin;

impl Plugin for MirrorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MirrorCamera>()
            .init_resource::<OtherDraws>()
            .add_systems(Startup, spawn)
            .add_systems(Update, copy.after(Step::Game).before(Step::Picture));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rotj_core::actor::oam::OamEntry;

    /// Batman walking right and firing every other pair of frames.
    fn pad(frame: usize) -> u8 {
        if frame % 4 < 2 { 0x41 } else { 0x01 }
    }

    /// Level 0's first loop frame, or `None` (skipped) without the ROM.
    fn start() -> Option<Play> {
        let rom = rotj_rom::for_test()?;
        Some(Play::start(
            crate::play::tests::prg(rom),
            rom.chr.clone(),
            0,
        ))
    }

    #[test]
    fn the_snapshot_takes_each_drawers_kind_and_position_from_the_game() {
        let Some(mut play) = start() else {
            return;
        };
        let fired = (0..120).any(|frame| {
            play.step(pad(frame));
            play.game()
                .effects
                .projectiles
                .iter()
                .any(|shot| shot.kind != 0)
        });
        assert!(fired, "a shot is out");
        let game = play.game();
        let snapshot = Snapshot::of(game, &play.lines);
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
            assert_eq!(
                actor_state.position * 16.0,
                Vec2::new(f32::from(actor.body.x), f32::from(actor.body.y))
            );
        }
        for (index, second) in (0u8..).zip(&game.slots.second) {
            assert_eq!(state(Mark::Second(index)).kind, Some(second.kind));
        }
        for (index, shot) in (0u8..).zip(&game.effects.projectiles) {
            let shot_state = state(Mark::Projectile(index));
            assert_eq!(shot_state.kind, Some(shot.kind));
            assert_eq!(
                shot_state.position * 16.0,
                Vec2::new(f32::from(shot.x), f32::from(shot.y))
            );
        }
        let player = &game.effects.player;
        assert_eq!(state(Mark::Player).kind, None);
        assert_eq!(
            state(Mark::Player).position * 16.0,
            Vec2::new(f32::from(player.x), f32::from(player.y))
        );
        assert_eq!(snapshot.records, game.oam.shown);
        assert_eq!(snapshot.drawers.len(), 24 + 8 + 16 + 1);
    }

    /// Over level 0's first frames, every sprite the page shows from a mirrored drawer
    /// has that drawer's records, and the records split over the entities and
    /// [`OtherDraws`] add up to the page's.
    #[test]
    fn the_entities_carry_the_shown_pages_records() {
        let Some(play) = start() else {
            return;
        };
        let mut app = App::new();
        app.add_plugins(MirrorPlugin).insert_resource(play);
        let mut drew = 0;
        for frame in 0..300 {
            app.world_mut().resource_mut::<Play>().step(pad(frame));
            app.update();
            let entities: Vec<(Mark, Option<Kind>, LevelPosition, Draws)> = app
                .world_mut()
                .query::<(&Drawer, Option<&Kind>, &LevelPosition, &Draws)>()
                .iter(app.world())
                .map(|(drawer, kind, position, draws)| {
                    (drawer.0, kind.copied(), *position, draws.clone())
                })
                .collect();
            let world = app.world();
            let play = world.resource::<Play>();
            for state in &play.shown.drawers {
                let (_, kind, position, _) = entities
                    .iter()
                    .find(|entity| entity.0 == state.drawer)
                    .expect("an entity per drawer");
                assert_eq!(kind.map(|kind| kind.0), state.kind, "{:?}", state.drawer);
                assert_eq!(position.0, state.position, "{:?}", state.drawer);
            }
            let draws: Vec<(Mark, Draws)> = entities
                .into_iter()
                .map(|(mark, _, _, draws)| (mark, draws))
                .collect();
            for (mark, records) in &draws {
                let shown = play
                    .oam
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(play.marks)
                    .filter(|&(bytes, at)| at == *mark && OamEntry::from(*bytes).is_visible())
                    .count();
                let pieces: usize = records.0.iter().map(|r| r.sprite.pieces.len()).sum();
                assert!(
                    shown <= pieces,
                    "frame {frame}: {mark:?} shows {shown} sprites from {pieces} pieces"
                );
                drew += shown;
            }
            let records = &play.shown.records;
            for (mark, records_of) in &draws {
                let expected: Vec<&DrawRecord> =
                    records.iter().filter(|r| r.mark == *mark).collect();
                assert_eq!(
                    records_of.0.iter().collect::<Vec<_>>(),
                    expected,
                    "{mark:?}"
                );
            }
            let total = draws.iter().map(|(_, d)| d.0.len()).sum::<usize>()
                + world.resource::<OtherDraws>().0.len();
            assert_eq!(total, records.len(), "frame {frame}");
        }
        assert!(drew > 300, "{drew} mirrored sprites shown");
    }
}
