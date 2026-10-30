//! The Modern picture (`picture: Modern`): in play, the level's tilemap and the game's sprites
//! as Bevy entities, drawn into an image the screen quad shows instead of the PPU's
//! (`docs/specs/2026-10-18-modern-picture-design.md`).

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{RenderTarget, Viewport};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::sprite::Anchor;
use bevy::sprite_render::{
    TileData, TilemapChunk, TilemapChunkTileData, update_tilemap_chunk_indices,
};
use rotj_core::actor::Game;
use rotj_core::actor::modern::{MAX_RUNS, Run, View, run_tile, tile_at, view};
use rotj_core::actor::oam::attr;
use rotj_core::chr;
use rotj_core::palette::Rgb;
use rotj_core::render::{Line, SCREEN_H, SCREEN_W, mask};

use crate::mirror::Drawer;
use crate::pacing::Step;
use crate::play::Play;
use crate::screen::{Colours, Screen, ScreenSprite, pixels, screen_image};
use crate::settings::{Picture, Settings, UiStyle};
use crate::ui::{Key, sprite_key, tiles};

/// The tilemap's size in 8x8 tiles: the screen and one more each way, for the scroll.
pub const COLUMNS: usize = SCREEN_W / 8 + 1;
/// See [`COLUMNS`].
pub const ROWS: usize = SCREEN_H / 8 + 1;

/// What the Modern picture draws of the frame on screen, taken with the page it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModernFrame {
    /// Where the screen is in the level, and the background's banks.
    pub view: View,
    /// `$2001`'s greyscale bit.
    pub greyscale: bool,
    /// Each tile's (tile, palette), [`COLUMNS`] by [`ROWS`], from the screen's top-left tile.
    pub tiles: Vec<Option<(u8, u8)>>,
}

impl ModernFrame {
    /// The frame `lines` draws from `game`, or `None` when it is drawn Classic.
    #[must_use]
    pub fn of(game: &Game, lines: &[Line]) -> Option<ModernFrame> {
        let view = view(game, lines)?;
        let (left, top) = (
            view.left - view.left.rem_euclid(8),
            view.top - view.top.rem_euclid(8),
        );
        let end = view.top + i32::try_from(view.map_end).expect("a line");
        let tiles = (0..ROWS)
            .flat_map(|row| (0..COLUMNS).map(move |column| (column, row)))
            .map(|(column, row)| {
                let [x, y] =
                    [column, row].map(|at| i32::try_from(at * 8).expect("a screen's worth"));
                (top + y < end)
                    .then(|| tile_at(game, left + x, top + y))
                    .flatten()
            })
            .collect();
        Some(ModernFrame {
            view,
            greyscale: lines
                .first()
                .is_some_and(|line| line.mask & mask::GREYSCALE != 0),
            tiles,
        })
    }
}

/// A palette's four colours, as RGB.
type Set = [[u8; 3]; 4];

/// The render layer the Modern picture's sprites use; each piece's tilemaps have their own after it.
const SPRITES: usize = 1;

/// The pieces a frame is drawn in, top to bottom: the playfield from the map, then the runs.
const PIECES: usize = MAX_RUNS + 1;

/// The Modern picture's image, the camera clearing it to the backdrop, each piece's tilemaps
/// and the art they use.
#[derive(Resource)]
struct Modern {
    image: Handle<Image>,
    backdrop: Entity,
    pieces: [Layer; PIECES],
    atlases: HashMap<Key, Handle<Image>>,
    layout: Handle<TextureAtlasLayout>,
}

/// Four tilemaps, one per background palette, [`COLUMNS`] by [`ROWS`], their tilesets, and the
/// camera drawing them and the sprites over the piece's lines.
struct Layer {
    camera: Entity,
    chunks: [Entity; 4],
    tilesets: [Handle<Image>; 4],
    /// The banks and colours the tilesets were last built from.
    built: Option<([u8; 4], [Set; 4])>,
}

/// One sprite of the Modern picture, respawned every frame.
#[derive(Component)]
struct ModernSprite;

/// `n` as a texture or tilemap size.
fn size(n: usize) -> u32 {
    u32::try_from(n).expect("a screen's worth")
}

fn spawn(
    mut commands: Commands<'_, '_>,
    mut images: ResMut<'_, Assets<Image>>,
    mut layouts: ResMut<'_, Assets<TextureAtlasLayout>>,
) {
    let image = images.add(Image::new_target_texture(
        size(SCREEN_W),
        size(SCREEN_H),
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    ));
    let backdrop = commands
        .spawn((
            modern_camera(&image, -isize_of(PIECES) - 1, ClearColorConfig::Default),
            RenderLayers::none(),
        ))
        .id();
    let pieces =
        std::array::from_fn(|piece| spawn_layer(&mut commands, &mut images, &image, piece));
    let layout = layouts.add(TextureAtlasLayout::from_grid(
        UVec2::new(8, 16),
        16,
        8,
        None,
        None,
    ));
    commands.insert_resource(Modern {
        image,
        backdrop,
        pieces,
        atlases: HashMap::new(),
        layout,
    });
}

/// `n` as a camera order.
fn isize_of(n: usize) -> isize {
    isize::try_from(n).expect("a few pieces")
}

/// An inactive camera drawing into `image`, one world unit a pixel, y up from the screen's top.
fn modern_camera(
    image: &Handle<Image>,
    order: isize,
    clear_color: ClearColorConfig,
) -> impl Bundle {
    (
        Camera2d,
        Camera {
            order,
            is_active: false,
            clear_color,
            ..default()
        },
        RenderTarget::Image(image.clone().into()),
        Transform::from_xyz(128.0, -120.0, 0.0),
    )
}

/// Piece `piece`'s tilemaps, hidden, blank tilesets and its camera, which draws over the
/// earlier pieces' without clearing.
fn spawn_layer(
    commands: &mut Commands<'_, '_>,
    images: &mut Assets<Image>,
    image: &Handle<Image>,
    piece: usize,
) -> Layer {
    let layer = SPRITES + 1 + piece;
    let camera = commands
        .spawn((
            modern_camera(
                image,
                isize_of(piece) - isize_of(PIECES),
                ClearColorConfig::None,
            ),
            RenderLayers::from_layers(&[SPRITES, layer]),
        ))
        .id();
    let tilesets: [Handle<Image>; 4] =
        std::array::from_fn(|_| images.add(tileset(&[], [0; 4], [[0; 3]; 4])));
    let chunks = tilesets.clone().map(|tileset| {
        commands
            .spawn((
                TilemapChunk {
                    chunk_size: UVec2::new(size(COLUMNS), size(ROWS)),
                    tile_display_size: UVec2::splat(8),
                    tileset,
                    ..default()
                },
                TilemapChunkTileData(vec![None; COLUMNS * ROWS]),
                Transform::default(),
                Visibility::Hidden,
                RenderLayers::layer(layer),
            ))
            .id()
    });
    Layer {
        camera,
        chunks,
        tilesets,
        built: None,
    }
}

/// The 256 background tiles of `banks` as an 8x8 array texture, in `colours`, colour 0 clear.
fn tileset(chr: &[u8], banks: [u8; 4], colours: Set) -> Image {
    let mut data = vec![0; 8 * 8 * 4 * 256];
    if !chr.is_empty() {
        for tile in 0..256 {
            let bytes = chr::tile(chr, &banks, tile * 16);
            for row in 0..8 {
                for column in 0..8 {
                    let value = chr::pixel(bytes, column, row);
                    if value != 0 {
                        let [r, g, b] = colours[usize::from(value)];
                        let at = ((tile * 8 + row) * 8 + column) * 4;
                        data[at..at + 4].copy_from_slice(&[r, g, b, 0xFF]);
                    }
                }
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: 8,
            height: 8 * 256,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image
        .reinterpret_stacked_2d_as_array(256)
        .expect("256 stacked tiles");
    image
}

/// A palette's four colours as `rgb` shows them.
fn colours(palette: &[u8; 32], first: usize, greyscale: bool, rgb: &Rgb) -> Set {
    let grey = mask::colour_bits(greyscale);
    std::array::from_fn(|v| rgb[usize::from(palette[first + v] & grey)])
}

/// What the picture is drawn from.
#[derive(SystemParam)]
struct Inputs<'w> {
    play: Res<'w, Play>,
    settings: Res<'w, Settings>,
    colours: Res<'w, Colours>,
    screen: Res<'w, Screen>,
}

/// The entities the picture changes.
#[derive(SystemParam)]
struct Scene<'w, 's> {
    cameras: Cameras<'w, 's>,
    quads: Query<'w, 's, &'static mut Sprite, With<ScreenSprite>>,
    chunks: Chunks<'w, 's>,
    drawers: Query<'w, 's, (Entity, &'static Drawer)>,
    sprites: Query<'w, 's, Entity, With<ModernSprite>>,
}

/// Draws the frame on screen Modern when the setting asks for it and the frame allows it;
/// otherwise turns the Modern camera off and shows the PPU's picture.
fn draw(
    mut commands: Commands<'_, '_>,
    inputs: Inputs<'_>,
    mut modern: ResMut<'_, Modern>,
    mut images: ResMut<'_, Assets<Image>>,
    scene: Scene<'_, '_>,
) {
    let Inputs {
        play,
        settings,
        colours: colours_table,
        screen,
    } = inputs;
    let Scene {
        mut cameras,
        mut quads,
        mut chunks,
        drawers,
        sprites,
    } = scene;
    for sprite in &sprites {
        commands.entity(sprite).despawn();
    }
    let frame = play
        .shown
        .modern
        .as_ref()
        .filter(|_| settings.picture == Picture::Modern);
    let shown_image = if frame.is_some() {
        modern.image.clone()
    } else {
        screen_image(&screen)
    };
    for mut quad in &mut quads {
        if quad.image != shown_image {
            quad.image = shown_image.clone();
        }
    }
    let rgb = &colours_table.rgb;
    if let Ok((mut backdrop, _)) = cameras.get_mut(modern.backdrop) {
        backdrop.is_active = frame.is_some();
        if let Some(frame) = frame {
            let [red, green, blue] = colours(&play.ppu.palette, 0, frame.greyscale, rgb)[0];
            backdrop.clear_color = ClearColorConfig::Custom(Color::srgb_u8(red, green, blue));
        }
    }
    let mut out = Out {
        images: &mut images,
        chunks: &mut chunks,
        cameras: &mut cameras,
    };
    let Some(frame) = frame else {
        for layer in &mut modern.pieces {
            show_layer(layer, &mut out, &play, [[[0; 3]; 4]; 4], None);
        }
        return;
    };
    show_background(&mut modern, &mut out, &play, frame, rgb);
    let faithful = settings.ui == UiStyle::Faithful;
    spawn_sprites(
        &mut commands,
        &mut modern,
        &mut images,
        &drawers,
        &play,
        faithful,
        rgb,
    );
}

/// The tilemaps the background type uses.
type Chunks<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut TilemapChunkTileData,
        &'static mut Transform,
        &'static mut Visibility,
    ),
    Without<Camera>,
>;

/// The pieces' cameras and the backdrop's.
type Cameras<'w, 's> = Query<'w, 's, (&'static mut Camera, &'static mut Transform)>;

/// What drawing a piece changes.
struct Out<'a, 'w, 's> {
    images: &'a mut Assets<Image>,
    chunks: &'a mut Chunks<'w, 's>,
    cameras: &'a mut Cameras<'w, 's>,
}

/// Each piece's tilemaps: the playfield's from the frame's map tiles, each run's from the
/// nametables, in screen space.
fn show_background(
    modern: &mut Modern,
    out: &mut Out<'_, '_, '_>,
    play: &Play,
    frame: &ModernFrame,
    rgb: &Rgb,
) {
    let sets: [Set; 4] =
        std::array::from_fn(|set| colours(&play.ppu.palette, set * 4, frame.greyscale, rgb));
    let [left, top] = [frame.view.left, frame.view.top]
        .map(|at| f32::from(u8::try_from(at.rem_euclid(8)).expect("0-7")));
    let map = (frame.view.map_end > 0).then(|| Shown {
        banks: frame.view.banks,
        tiles: &frame.tiles,
        corner: Vec2::new(-left, top),
        lines: 0..frame.view.map_end,
    });
    let runs: Vec<_> = frame
        .view
        .runs
        .iter()
        .flatten()
        .map(|run| (run, run_tiles(play, run)))
        .collect();
    let pieces = std::iter::once(map)
        .chain(
            runs.iter()
                .map(|(run, tiles)| Some(screen_space(run, tiles))),
        )
        .chain(std::iter::repeat_with(|| None));
    for (layer, shown) in modern.pieces.iter_mut().zip(pieces) {
        show_layer(layer, out, play, sets, shown);
    }
}

/// A run's piece in screen space, its tiles from [`run_tiles`].
fn screen_space<'a>(run: &Run, tiles: &'a [Option<(u8, u8)>]) -> Shown<'a> {
    Shown {
        banks: run.banks(),
        tiles,
        corner: Vec2::new(
            -f32::from(run.top.fine_x),
            pixels(run.fine_y()) - pixels(run.line),
        ),
        lines: run.line..run.end,
    }
}

/// What one piece shows: its top-left tile's corner in camera coordinates (y up), and the
/// screen lines it is clipped to.
struct Shown<'a> {
    banks: [u8; 4],
    tiles: &'a [Option<(u8, u8)>],
    corner: Vec2,
    lines: Range<usize>,
}

/// The run's tiles on the layer's grid, from the tile row its first line is in down to its end.
fn run_tiles(play: &Play, run: &Run) -> Vec<Option<(u8, u8)>> {
    let rows = (run.end - run.line + run.fine_y()).div_ceil(8);
    (0..ROWS)
        .flat_map(|row| (0..COLUMNS).map(move |column| (column, row)))
        .map(|(column, row)| (row < rows).then(|| run_tile(&play.ppu, run, column, row)))
        .collect()
}

/// A piece's four tilemaps from `shown` and its camera on its lines, or both off.
fn show_layer(
    layer: &mut Layer,
    out: &mut Out<'_, '_, '_>,
    play: &Play,
    sets: [Set; 4],
    shown: Option<Shown<'_>>,
) {
    if let Ok((mut camera, mut transform)) = out.cameras.get_mut(layer.camera) {
        camera.is_active = shown.is_some();
        if let Some(shown) = &shown {
            let lines = shown.lines.clone();
            camera.viewport = Some(Viewport {
                physical_position: UVec2::new(0, size(lines.start)),
                physical_size: UVec2::new(size(SCREEN_W), size(lines.len())),
                ..default()
            });
            transform.translation.y = -pixels(lines.start + lines.end) / 2.0;
        }
    }
    let Some(shown) = shown else {
        for &chunk in &layer.chunks {
            if let Ok((_, _, mut visibility)) = out.chunks.get_mut(chunk) {
                *visibility = Visibility::Hidden;
            }
        }
        return;
    };
    if layer.built != Some((shown.banks, sets)) {
        for (set, handle) in layer.tilesets.iter().enumerate() {
            if let Some(mut image) = out.images.get_mut(handle) {
                *image = tileset(&play.chr, shown.banks, sets[set]);
            }
        }
        layer.built = Some((shown.banks, sets));
    }
    let centre = Vec3::new(
        shown.corner.x + pixels(COLUMNS * 4),
        shown.corner.y - pixels(ROWS * 4),
        2.0,
    );
    for (set, &chunk) in layer.chunks.iter().enumerate() {
        let Ok((mut data, mut transform, mut visibility)) = out.chunks.get_mut(chunk) else {
            continue;
        };
        let tiles = shown.tiles.iter().map(|tile| {
            tile.filter(|&(_, palette)| usize::from(palette) == set)
                .map(|(tile, _)| TileData::from_tileset_index(u16::from(tile)))
        });
        *data = TilemapChunkTileData::from_y_down_tiles(
            UVec2::new(size(COLUMNS), size(ROWS)),
            tiles.collect(),
        );
        transform.translation = centre;
        *visibility = Visibility::Visible;
    }
}

/// Every record's sprites in draw order, each a child of its drawer's mirror entity: behind
/// the tilemaps when its priority bit says, an earlier draw in front of a later one.
fn spawn_sprites(
    commands: &mut Commands<'_, '_>,
    modern: &mut Modern,
    images: &mut Assets<Image>,
    drawers: &Query<'_, '_, (Entity, &Drawer)>,
    play: &Play,
    faithful: bool,
    rgb: &Rgb,
) {
    let records = play
        .shown
        .records
        .iter()
        .filter(|record| !(faithful && record.mark.is_ui()));
    let mut used = HashSet::new();
    let mut order = 0u16;
    for record in records {
        let parent = drawers
            .iter()
            .find(|(_, drawer)| drawer.0 == record.mark)
            .map(|(entity, _)| entity);
        for entry in record.sprites(256).filter(|entry| entry.is_visible()) {
            let Some(key) = sprite_key(entry, &play.lines, &play.ppu.palette, rgb) else {
                continue;
            };
            used.insert(key);
            let atlas = modern
                .atlases
                .entry(key)
                .or_insert_with(|| images.add(tiles(&play.chr, &key)))
                .clone();
            let mut sprite = Sprite::from_atlas_image(
                atlas,
                TextureAtlas {
                    layout: modern.layout.clone(),
                    index: usize::from(entry.tile >> 1),
                },
            );
            sprite.flip_x = entry.attr & attr::FLIP_X != 0;
            sprite.flip_y = entry.attr & attr::FLIP_Y != 0;
            let depth = if entry.attr & attr::BEHIND != 0 {
                1.5
            } else {
                3.5
            };
            let mut spawned = commands.spawn((
                ModernSprite,
                sprite,
                Anchor::TOP_LEFT,
                Transform::from_xyz(
                    f32::from(entry.x),
                    -f32::from(entry.y) - 1.0,
                    depth - f32::from(order) / 1024.0,
                ),
                RenderLayers::layer(SPRITES),
            ));
            if let Some(parent) = parent {
                spawned.insert(ChildOf(parent));
            }
            order = order.saturating_add(1);
        }
    }
    modern.atlases.retain(|key, _| used.contains(key));
}

/// The Modern picture.
pub struct ModernPlugin;

impl Plugin for ModernPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn).add_systems(
            Update,
            draw.in_set(Step::Picture)
                .before(update_tilemap_chunk_indices),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;
    use std::path::PathBuf;

    use bevy::ecs::schedule::{LogLevel, ScheduleBuildSettings};
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    use bevy::window::ExitCondition;
    use bevy::winit::WinitPlugin;

    use super::*;

    /// Batman walking right and firing every other pair of frames.
    fn pad(frame: usize) -> u8 {
        if frame % 4 < 2 { 0x41 } else { 0x01 }
    }

    /// The chunk upload runs after `draw`, so a tile lands in the frame its scroll offset does.
    #[test]
    fn the_tiles_upload_after_draw() {
        let mut app = App::new();
        app.add_systems(Update, update_tilemap_chunk_indices)
            .add_plugins(ModernPlugin);
        let world = app.world_mut();
        let mut update = world
            .resource_mut::<Schedules>()
            .remove(Update)
            .expect("the Update schedule");
        update.set_build_settings(ScheduleBuildSettings {
            ambiguity_detection: LogLevel::Error,
            ..default()
        });
        update
            .initialize(world)
            .expect("Update builds, the two ordered");
        let order: Vec<TypeId> = update
            .systems()
            .expect("initialised")
            .map(|(_, system)| system.system_type())
            .collect();
        let at = |system: TypeId| order.iter().position(|&id| id == system);
        let draw = at(IntoSystem::into_system(draw).system_type()).expect("draw");
        let upload = IntoSystem::into_system(update_tilemap_chunk_indices).system_type();
        assert!(draw < at(upload).expect("the upload"));
    }

    /// Stage 1-1, the `$0E` boss bar, then the final boss's room with a part's death tiles; one
    /// test, since two headless renders at once fail.
    #[test]
    #[ignore = "renders with the GPU (here software Vulkan); ROTJ_SCREENSHOT names the PNGs' directory"]
    fn stage_1_and_a_boss_bar_render_modern() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let play = Play::start(crate::play::tests::prg(rom), rom.chr.clone(), 0);
        let (drawn, view) = renders_as_classic(play, 180, pad, "stage-1");
        assert!(drawn > 100, "{drawn} of 180 frames drawn Modern");
        assert_eq!(
            view.map(|view| (view.map_end, view.runs[0])),
            Some((240, None))
        );
        let mut play = Play::start(crate::play::tests::prg(rom), rom.chr.clone(), 5);
        crate::play::tests::enter_the_boss_room(&mut play, 0x0E);
        let (drawn, view) = renders_as_classic(play, 60, |_| 0, "boss-bar");
        assert_eq!(drawn, 60, "boss room frames drawn Modern");
        assert_eq!(
            view.map(|view| (view.map_end, view.runs.map(|run| run.map(|run| run.line)))),
            Some((
                192,
                [Some(192), Some(193), None, None, None, None, None, None]
            )),
            "the map down to the bar, its mixed line, the bar"
        );
        let play = Play::start(crate::play::tests::prg(rom), rom.chr.clone(), 9);
        let (drawn, view) = renders_as_classic(play, 61, pad, "seven-bands");
        assert_eq!(drawn, 61, "area $18's frames drawn Modern");
        assert_eq!(
            view.map(|view| (view.map_end, view.runs.iter().flatten().count())),
            Some((0, 6)),
            "six runs from the nametables"
        );
        let play = Play::start(crate::play::tests::prg(rom), rom.chr.clone(), 11);
        // From frame 40 the rotation puts two of an enemy's overlapping sprites out of draw order.
        let (drawn, view) = renders_as_classic(play, 31, pad, "fixed-top");
        assert_eq!(drawn, 31, "area $1C's frames drawn Modern");
        assert_eq!(
            view.map(|view| (view.map_end, view.runs.iter().flatten().count())),
            Some((0, 3)),
            "the playfield, the fixed art and the band below, from the lines after the loop"
        );
        let mut play = Play::start(crate::play::tests::prg(rom), rom.chr.clone(), 12);
        for arg in [0xC1, 0x61] {
            crate::play::tests::transition(&mut play, 1, arg);
        }
        crate::play::tests::enter_the_boss_room(&mut play, 0x22);
        let intact = play.ppu.nametables;
        let part = play
            .game_mut()
            .slots
            .main
            .iter_mut()
            .find(|slot| slot.kind == 0xB6)
            .expect("a part of the final boss");
        part.body.health |= 0x8000;
        let tables = crate::play::tests::prg(rom).families;
        let dest = tables.final_boss_part.death_dests[usize::from(part.body.subtype)];
        for _ in 0..10 {
            play.step(0);
        }
        let table = play
            .game()
            .scroll
            .mirroring
            .physical(usize::from(dest >> 10) & 3);
        let block = (0..4).flat_map(|row| (0..4).map(move |column| row * 32 + column));
        let changed = block
            .map(|cell| usize::from(dest & 0x3FF) + cell)
            .any(|at| play.ppu.nametables[table][at] != intact[table][at]);
        assert!(
            changed,
            "the death tiles at ${dest:04X} reach the nametables"
        );
        // An odd frame: the allocator's rotation then puts the portrait's overlapping sprites in draw order.
        let (drawn, view) = renders_as_classic(play, 61, |_| 0, "final-boss");
        assert!(drawn > 0, "final boss frames drawn Modern");
        assert_eq!(
            view.map(|view| view.map_end),
            Some(0),
            "the playfield from the nametables"
        );
    }

    /// Plays `frames` frames Modern and asserts its screenshot is Classic's.
    fn renders_as_classic(
        play: Play,
        frames: usize,
        pad: fn(usize) -> u8,
        name: &str,
    ) -> (usize, Option<View>) {
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    close_when_requested: false,
                    ..default()
                })
                .disable::<WinitPlugin>(),
        )
        .insert_resource(Settings {
            picture: Picture::Modern,
            ..Settings::default()
        })
        .init_resource::<Colours>()
        .add_plugins((crate::mirror::MirrorPlugin, ModernPlugin));
        let screen = Screen::new(&mut app.world_mut().resource_mut::<Assets<Image>>());
        app.insert_resource(screen).insert_resource(play);
        app.finish();
        app.cleanup();
        let mut drawn = 0;
        for frame in 0..frames {
            app.world_mut().resource_mut::<Play>().step(pad(frame));
            app.update();
            drawn += usize::from(app.world().resource::<Play>().shown.modern.is_some());
        }
        let path = std::env::var_os("ROTJ_SCREENSHOT")
            .map_or_else(std::env::temp_dir, PathBuf::from)
            .join(format!("rotj-modern-{name}.png"));
        let _ = std::fs::remove_file(&path);
        let image = app.world().resource::<Modern>().image.clone();
        app.world_mut()
            .spawn(Screenshot::image(image))
            .observe(save_to_disk(path.clone()));
        for _ in 0..20 {
            app.update();
        }
        assert!(path.exists(), "no screenshot at {}", path.display());
        let play = app.world().resource::<Play>();
        let view = play.shown.modern.as_ref().map(|frame| frame.view);
        let mut frame = rotj_core::render::Frame::new();
        play.compose(&mut frame, rotj_core::render::OAM_SPRITES, false);
        let mut classic = vec![0; SCREEN_W * SCREEN_H * 4];
        let rgb = &app.world().resource::<Colours>().rgb;
        crate::screen::rgba_into(frame.indices(), rgb, &[false; SCREEN_H], &mut classic);
        let classic = image::RgbaImage::from_raw(256, 240, classic).expect("256x240");
        classic
            .save(path.with_extension("classic.png"))
            .expect("the Classic picture saved");
        let modern = image::open(&path).expect("the screenshot").to_rgba8();
        let differing = (8..232)
            .flat_map(|y| (8..248).map(move |x| (x, y)))
            .filter(|&(x, y)| modern.get_pixel(x, y).0[..3] != classic.get_pixel(x, y).0[..3])
            .count();
        assert_eq!(differing, 0, "pixels differing from the Classic picture");
        (drawn, view)
    }

    #[test]
    fn tileset_draws_the_banks_pixels_in_its_colours() {
        let mut chr = vec![0u8; 2 * 1024];
        chr[1024] = 0b1000_0000;
        chr[1024 + 8] = 0b1000_0000; // bank 1, tile 0: value 3 at its first pixel
        chr[1] = 0b0100_0000; // bank 0, tile 64: value 1 one pixel in
        let set: Set = [[9; 3], [1; 3], [2; 3], [3; 3]];
        let image = tileset(&chr, [1, 0, 1, 0], set);
        let data = image.data.expect("pixels");
        let at = |tile: usize, row: usize, column: usize| {
            let at = ((tile * 8 + row) * 8 + column) * 4;
            &data[at..at + 4]
        };
        assert_eq!(at(0, 0, 0), [3, 3, 3, 0xFF], "value 3 from bank 1");
        assert_eq!(at(64, 1, 1), [1, 1, 1, 0xFF], "value 1 from bank 0");
        assert_eq!(at(0, 0, 1), [0, 0, 0, 0], "colour 0 clear");

        // Bank 3 runs past two banks of CHR, so it wraps onto bank 1's bytes.
        let wrapped = tileset(&chr, [3, 0, 1, 0], set);
        let data = wrapped.data.expect("pixels");
        assert_eq!(&data[..4], [3, 3, 3, 0xFF]);

        let mut palette = [0u8; 32];
        palette[0] = 0x2A;
        let rgb = rotj_core::palette::NES_PALETTE;
        assert_eq!(colours(&palette, 0, false, &rgb)[0], rgb[0x2A]);
        assert_eq!(colours(&palette, 0, true, &rgb)[0], rgb[0x20]);
    }
}
