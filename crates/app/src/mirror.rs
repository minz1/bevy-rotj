//! The read-only mirror: the game's drawers as entities, for the Modern picture, the
//! inspector and the editor (`docs/specs/2026-10-18-phase-c-mirror-design.md`, C2).

use bevy::prelude::*;
use rotj_core::sim::oam::{DrawRecord, Mark};

use crate::pacing::Step;
use crate::play::Play;

/// The drawer an entity mirrors: a slot or Batman.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Drawer(pub Mark);

/// The slot's type byte (`0` when it is empty).
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Kind(u8);

/// Where the drawer is in the level, in pixels, y down.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq)]
struct LevelPosition(Vec2);

/// The drawer's records for the frame on screen, in draw order.
#[derive(Component, Debug, Default, Clone, PartialEq)]
struct Draws(Vec<DrawRecord>);

/// The view's top-left in the level, in pixels, y down.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
struct MirrorCamera(Vec2);

/// The frame's records whose drawer is no entity: the scene's, the HUD's and the screens'.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
struct OtherDraws(Vec<DrawRecord>);

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
    let snapshot = play.session().snapshot();
    for (drawer, kind, mut position, mut draws) in &mut drawers {
        let Some(state) = snapshot.drawers.iter().find(|s| s.drawer == drawer.0) else {
            continue;
        };
        if let (Some(mut kind), Some(byte)) = (kind, state.kind) {
            kind.set_if_neq(Kind(byte));
        }
        position.set_if_neq(LevelPosition(level_pixels(state.x, state.y)));
        let records = snapshot.records.iter().filter(|r| r.mark == drawer.0);
        draws.set_if_neq(Draws(records.cloned().collect()));
    }
    camera.set_if_neq(MirrorCamera(level_pixels(
        snapshot.camera.0,
        snapshot.camera.1,
    )));
    let records = snapshot.records.iter().filter(|r| !has_entity(r.mark));
    other.set_if_neq(OtherDraws(records.cloned().collect()));
}

/// The mirror.
pub(crate) struct MirrorPlugin;

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
            app.world_mut()
                .resource_mut::<Play>()
                .step(rotj_core::Buttons::from_bits(pad(frame)));
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
            let snapshot = play.session().snapshot();
            for state in &snapshot.drawers {
                let (_, kind, position, _) = entities
                    .iter()
                    .find(|entity| entity.0 == state.drawer)
                    .expect("an entity per drawer");
                assert_eq!(kind.map(|kind| kind.0), state.kind, "{:?}", state.drawer);
                assert_eq!(
                    position.0,
                    level_pixels(state.x, state.y),
                    "{:?}",
                    state.drawer
                );
            }
            let draws: Vec<(Mark, Draws)> = entities
                .into_iter()
                .map(|(mark, _, _, draws)| (mark, draws))
                .collect();
            for (mark, records) in &draws {
                let shown = crate::play::tests::shown(play, *mark);
                let pieces: usize = records.0.iter().map(|r| r.sprite.pieces.len()).sum();
                assert!(
                    shown <= pieces,
                    "frame {frame}: {mark:?} shows {shown} sprites from {pieces} pieces"
                );
                drew += shown;
            }
            let records = &snapshot.records;
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
