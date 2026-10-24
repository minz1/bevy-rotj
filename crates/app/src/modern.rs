//! The Modern picture (`picture: Modern`): in play, the level's tilemap and the game's sprites
//! as Bevy entities, drawn into an image the screen quad shows instead of the PPU's
//! (`docs/specs/2026-10-18-modern-picture-design.md`).

use std::collections::{HashMap, HashSet};

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{RenderTarget, ScalingMode};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::sprite::Anchor;
use bevy::sprite_render::{
    TileData, TilemapChunk, TilemapChunkTileData, update_tilemap_chunk_indices,
};
use rotj_core::actor::Game;
use rotj_core::actor::modern::{Strip, View, strip_tile, tile_at, view};
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
        let end = view.strip.map_or(i32::MAX, |strip| {
            view.top + i32::try_from(strip.line).expect("a line")
        });
        let tiles = (0..ROWS)
            .flat_map(|row| (0..COLUMNS).map(move |column| (column, row)))
            .map(|(column, row)| {
                let [x, y] =
                    [column, row].map(|at| i32::try_from(at * 8).expect("a screen's worth"));
                (view.nametable_playfield.is_none() && top + y < end)
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

/// The render layer the Modern picture's entities and camera use.
const LAYER: usize = 1;

/// The Modern picture's camera, image, tilemaps and the art they use.
#[derive(Resource)]
struct Modern {
    image: Handle<Image>,
    camera: Entity,
    playfield: Layer,
    strip: Layer,
    atlases: HashMap<Key, Handle<Image>>,
    layout: Handle<TextureAtlasLayout>,
}

/// Four tilemaps, one per background palette, [`COLUMNS`] by [`ROWS`], and their tilesets.
struct Layer {
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
    let camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: -1,
                is_active: false,
                ..default()
            },
            RenderTarget::Image(image.clone().into()),
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: ScalingMode::Fixed {
                    width: 256.0,
                    height: 240.0,
                },
                ..OrthographicProjection::default_2d()
            }),
            Transform::from_xyz(128.0, -120.0, 0.0),
            RenderLayers::layer(LAYER),
        ))
        .id();
    let playfield = spawn_layer(&mut commands, &mut images);
    let strip = spawn_layer(&mut commands, &mut images);
    let layout = layouts.add(TextureAtlasLayout::from_grid(
        UVec2::new(8, 16),
        16,
        8,
        None,
        None,
    ));
    commands.insert_resource(Modern {
        image,
        camera,
        playfield,
        strip,
        atlases: HashMap::new(),
        layout,
    });
}

/// A layer's tilemaps, hidden, and blank tilesets.
fn spawn_layer(commands: &mut Commands<'_, '_>, images: &mut Assets<Image>) -> Layer {
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
                RenderLayers::layer(LAYER),
            ))
            .id()
    });
    Layer {
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
    cameras: Query<'w, 's, &'static mut Camera>,
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
    let Ok(mut camera) = cameras.get_mut(modern.camera) else {
        return;
    };
    camera.is_active = frame.is_some();
    let Some(frame) = frame else {
        return;
    };
    let rgb = &colours_table.rgb;
    let [red, green, blue] = colours(&play.ppu.palette, 0, frame.greyscale, rgb)[0];
    camera.clear_color = ClearColorConfig::Custom(Color::srgb_u8(red, green, blue));
    show_background(&mut modern, &mut images, &mut chunks, &play, frame, rgb);
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
>;

/// The playfield's tilemaps from the frame and the strip's from the nametables, in screen space.
fn show_background(
    modern: &mut Modern,
    images: &mut Assets<Image>,
    chunks: &mut Chunks<'_, '_>,
    play: &Play,
    frame: &ModernFrame,
    rgb: &Rgb,
) {
    let sets: [Set; 4] =
        std::array::from_fn(|set| colours(&play.ppu.palette, set * 4, frame.greyscale, rgb));
    let [left, top] = [frame.view.left, frame.view.top]
        .map(|at| f32::from(u8::try_from(at.rem_euclid(8)).expect("0-7")));
    let bar = frame.view.strip.map_or(SCREEN_H, |strip| strip.line);
    let run = frame
        .view
        .nametable_playfield
        .map(|run| (run, strip_tiles(play, &run, bar)));
    let playfield = run.as_ref().map_or(
        Shown {
            banks: frame.view.banks,
            tiles: &frame.tiles,
            corner: Vec2::new(-left, top),
        },
        |(run, tiles)| screen_space(run, tiles),
    );
    show_layer(
        &mut modern.playfield,
        images,
        chunks,
        play,
        sets,
        Some(playfield),
    );
    let strip = frame
        .view
        .strip
        .map(|strip| (strip, strip_tiles(play, &strip, SCREEN_H)));
    let strip = strip
        .as_ref()
        .map(|(strip, tiles)| screen_space(strip, tiles));
    show_layer(&mut modern.strip, images, chunks, play, sets, strip);
}

/// A strip's layer in screen space, its tiles from [`strip_tiles`].
fn screen_space<'a>(strip: &Strip, tiles: &'a [Option<(u8, u8)>]) -> Shown<'a> {
    Shown {
        banks: strip.banks,
        tiles,
        corner: Vec2::new(
            -f32::from(strip.top.fine_x),
            -f32::from(u8::try_from(strip.line % 8).expect("0-7")),
        ),
    }
}

/// What one layer shows, its top-left tile's corner in camera coordinates (y up).
struct Shown<'a> {
    banks: [u8; 4],
    tiles: &'a [Option<(u8, u8)>],
    corner: Vec2,
}

/// The strip's tiles on the layer's grid, from the row its first line is in down to line `end`.
fn strip_tiles(play: &Play, strip: &Strip, end: usize) -> Vec<Option<(u8, u8)>> {
    let (first, rows) = (strip.line / 8, (end - strip.line).div_ceil(8));
    (0..ROWS)
        .flat_map(|row| (0..COLUMNS).map(move |column| (column, row)))
        .map(|(column, row)| {
            (first..first + rows)
                .contains(&row)
                .then(|| strip_tile(&play.ppu, strip, column, row - first))
        })
        .collect()
}

/// A layer's four tilemaps from `shown`, or hidden.
fn show_layer(
    layer: &mut Layer,
    images: &mut Assets<Image>,
    chunks: &mut Chunks<'_, '_>,
    play: &Play,
    sets: [Set; 4],
    shown: Option<Shown<'_>>,
) {
    let Some(shown) = shown else {
        for &chunk in &layer.chunks {
            if let Ok((_, _, mut visibility)) = chunks.get_mut(chunk) {
                *visibility = Visibility::Hidden;
            }
        }
        return;
    };
    if layer.built != Some((shown.banks, sets)) {
        for (set, handle) in layer.tilesets.iter().enumerate() {
            if let Some(mut image) = images.get_mut(handle) {
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
        let Ok((mut data, mut transform, mut visibility)) = chunks.get_mut(chunk) else {
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
                RenderLayers::layer(LAYER),
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
        let play = Play::start(rom.prg.clone(), rom.chr.clone(), 0);
        let (drawn, view) = renders_as_classic(play, 180, pad, "stage-1");
        assert!(drawn > 100, "{drawn} of 180 frames drawn Modern");
        assert_eq!(view.and_then(|view| view.strip), None);
        let mut play = Play::start(rom.prg.clone(), rom.chr.clone(), 5);
        crate::play::tests::enter_the_boss_room(&mut play, 0x0E);
        let (drawn, view) = renders_as_classic(play, 60, |_| 0, "boss-bar");
        assert_eq!(drawn, 60, "boss room frames drawn Modern");
        assert_eq!(
            view.and_then(|view| view.strip).map(|strip| strip.line),
            Some(192),
            "the bar's strip"
        );
        let mut play = Play::start(rom.prg.clone(), rom.chr.clone(), 12);
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
        let tables = rotj_core::prg::Prg::new(rom.prg.clone()).families;
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
        assert!(
            view.is_some_and(|view| view.nametable_playfield.is_some()),
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
