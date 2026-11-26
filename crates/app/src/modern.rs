//! The Modern picture (`picture: Modern`): in play, the level's tilemap and the game's sprites
//! as Bevy entities, drawn into an image the screen quad shows instead of the PPU's, with
//! optional display-scale high-refresh interpolation (`frame_rate != Classic`).

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{Projection, RenderTarget, Viewport};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::sprite::Anchor;
use bevy::sprite_render::{
    TileData, TilemapChunk, TilemapChunkTileData, update_tilemap_chunk_indices,
};
use rotj_core::chr;
use rotj_core::palette::Rgb;
use rotj_core::render::{Line, Mirroring, SCREEN_H, SCREEN_W};
use rotj_core::sim::modern::{
    GRID_COLUMNS, GRID_ROWS, GUARD, MAX_RUNS, ModernFrame, Piece, Pieces, Run, TileGrid,
    run_tile_signed,
};
use rotj_core::sim::oam::{Mark, OamEntry, attr};
use rotj_core::sim::session::FrameSnapshot;

use crate::DisplayScale;
use crate::menu::Menu;
use crate::mirror::{Drawer, InterpolationHistory, TickSnapshot};
use crate::pacing::{Clock, Step};
use crate::play::Play;
use crate::screen::{Colours, Screen, ScreenSprite, pixels, screen_image, screen_quad_rect};
use crate::settings::{Picture, Settings, UiStyle};
use crate::ui::{Key, sprite_key_at_line};

/// A palette's four colours, as RGB.
type Set = [[u8; 3]; 4];

/// The render layer the Modern picture's sprites use; each piece's tilemaps have their own after it.
const SPRITES: usize = 1;

/// The pieces a frame is drawn in, top to bottom.
const PIECES: usize = MAX_RUNS;

/// Maximum per-tick displacement (in NES pixels) that interpolates; larger jumps snap cleanly
/// (teleports, respawns, scroll wraps, room transitions).
const SNAP_THRESHOLD: f32 = 16.0;

/// The Modern picture's image, the camera clearing it to the backdrop, each piece's tilemaps
/// and the art they use.
#[derive(Resource)]
struct Modern {
    image: Handle<Image>,
    /// Integer pixel multiple of `image` (`1` = `256x240`, `M` = `256*M x 240*M`).
    scale: u32,
    /// False on the frame a resized `image` handle is allocated so cameras wait one frame for
    /// `prepare_assets::<GpuImage>` before attaching depth buffers to the new target.
    ready: bool,
    backdrop: Entity,
    pieces: [Layer; PIECES],
    atlases: HashMap<Key, Handle<Image>>,
    layout: Handle<TextureAtlasLayout>,
}

/// Which renderer supplies the screen quad's image for the current frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ActivePicture {
    /// The PPU's software-composed image.
    #[default]
    Classic,
    /// Bevy tilemaps and sprites rendered to an off-screen target.
    Modern,
}

impl ActivePicture {
    /// Whether this frame renders Modern or falls back to Classic.
    #[must_use]
    pub(crate) const fn for_frame(settings: &Settings, play: &Play) -> ActivePicture {
        if matches!(settings.picture, Picture::Modern) && play.session().snapshot().modern.is_some()
        {
            ActivePicture::Modern
        } else {
            ActivePicture::Classic
        }
    }
}

/// The banks and palette colours a layer's tilesets were built from.
#[derive(Clone, Copy, PartialEq, Eq)]
struct BuiltTileset {
    banks: [u8; 4],
    sets: [Set; 4],
}

/// Four tilemaps, one per background palette, [`GRID_COLUMNS`] by [`GRID_ROWS`], their tilesets,
/// and the camera drawing them and the sprites over the piece's lines.
struct Layer {
    camera: Entity,
    chunks: [Entity; 4],
    tilesets: [Handle<Image>; 4],
    /// The banks and colours the tilesets were last built from.
    built: Option<BuiltTileset>,
}

/// One sprite of the Modern picture, respawned every frame.
#[derive(Component)]
struct ModernSprite;

/// `n` as a texture or tilemap size.
fn size(n: usize) -> u32 {
    u32::try_from(n).expect("a screen's worth")
}

fn target_image(scale: u32) -> Image {
    let s = scale.clamp(1, 8);
    Image::new_target_texture(
        size(SCREEN_W) * s,
        size(SCREEN_H) * s,
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    )
}

/// 1-pixel transparent gutter on every side of each 8x16 tile in Modern's sprite atlas so
/// subpixel-interpolated sprites at display scale never bleed into neighboring tiles.
const SPRITE_ATLAS_PAD: usize = 1;
const SPRITE_CELL_W: usize = 8 + 2 * SPRITE_ATLAS_PAD;
const SPRITE_CELL_H: usize = 16 + 2 * SPRITE_ATLAS_PAD;

/// A pattern table as a 16x8 grid of 8x16 tiles, each surrounded by a 1 px transparent gutter.
fn sprite_atlas_tiles(chr: &[u8], key: &Key) -> Image {
    let width = 16 * SPRITE_CELL_W;
    let height = 8 * SPRITE_CELL_H;
    let mut data = vec![0; width * height * 4];
    let stride = width * 4;
    let colours = std::array::from_fn(|v| {
        if v == 0 {
            [0, 0, 0, 0]
        } else {
            let [r, g, b] = key.colours[v];
            [r, g, b, 0xFF]
        }
    });
    for cell in 0..128 {
        for half in 0..2 {
            let addr = (2 * cell + half) * 16;
            let bytes = chr::tile(chr, &key.banks, addr);
            let px = (cell % 16) * SPRITE_CELL_W + SPRITE_ATLAS_PAD;
            let py = (cell / 16) * SPRITE_CELL_H + SPRITE_ATLAS_PAD + half * 8;
            chr::blit_rgba(bytes, &mut data[py * stride + px * 4..], stride, colours);
        }
    }
    crate::screen::rgba_image(width, height, data)
}

fn spawn(
    mut commands: Commands<'_, '_>,
    mut images: ResMut<'_, Assets<Image>>,
    mut layouts: ResMut<'_, Assets<TextureAtlasLayout>>,
) {
    let image = images.add(target_image(1));
    let backdrop = commands
        .spawn((
            modern_camera(&image, -isize_of(PIECES) - 1, ClearColorConfig::Default),
            RenderLayers::none(),
        ))
        .id();
    let pieces =
        std::array::from_fn(|piece| spawn_layer(&mut commands, &mut images, &image, piece));
    let pad = size(SPRITE_ATLAS_PAD);
    let layout = layouts.add(TextureAtlasLayout::from_grid(
        UVec2::new(8, 16),
        16,
        8,
        Some(UVec2::splat(2 * pad)),
        Some(UVec2::splat(pad)),
    ));
    commands.insert_resource(Modern {
        image,
        scale: 1,
        ready: true,
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
                    chunk_size: UVec2::new(size(GRID_COLUMNS), size(GRID_ROWS)),
                    tile_display_size: UVec2::splat(8),
                    tileset,
                    ..default()
                },
                TilemapChunkTileData(vec![None; GRID_COLUMNS * GRID_ROWS]),
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
    let rgba_colours = std::array::from_fn(|v| {
        if v == 0 {
            [0, 0, 0, 0]
        } else {
            let [r, g, b] = colours[v];
            [r, g, b, 0xFF]
        }
    });
    if !chr.is_empty() {
        for tile in 0..256 {
            let bytes = chr::tile(chr, &banks, tile * 16);
            chr::blit_rgba(bytes, &mut data[tile * 8 * 8 * 4..], 8 * 4, rgba_colours);
        }
    }
    let mut image = crate::screen::rgba_image(8, 8 * 256, data);
    image
        .reinterpret_stacked_2d_as_array(256)
        .expect("256 stacked tiles");
    image
}

/// A palette's four colours as `rgb` shows them.
fn colours(palette: &[u8; 32], first: usize, greyscale: bool, rgb: &Rgb) -> Set {
    std::array::from_fn(|v| {
        rotj_core::palette::rgb(
            rgb,
            rotj_core::palette::colour(palette, first + v, greyscale),
        )
    })
}

/// Immutable per-frame interpolation context between the previous and current 60 Hz ticks.
#[derive(Clone, Copy)]
struct Interp<'a> {
    prev: Option<&'a FrameSnapshot>,
    curr: Option<&'a FrameSnapshot>,
    alpha: f32,
}

impl<'a> Interp<'a> {
    const NONE: Self = Self {
        prev: None,
        curr: None,
        alpha: 1.0,
    };

    fn from_history(history: Option<&'a InterpolationHistory>, alpha: f32) -> Self {
        let Some(history) = history else {
            return Self::NONE;
        };
        let (Some(prev), Some(curr)) = (&history.prev, &history.curr) else {
            return Self::NONE;
        };
        if !can_interpolate(prev, curr) || alpha >= 1.0 {
            return Self::NONE;
        }
        Self {
            prev: Some(&prev.snapshot),
            curr: Some(&curr.snapshot),
            alpha: alpha.clamp(0.0, 1.0),
        }
    }

    fn map_offset(self, piece_index: usize, curr: Piece) -> Vec2 {
        let (
            Some(prev),
            Piece::Map {
                line,
                end,
                left,
                top,
                ..
            },
        ) = (self.prev, curr)
        else {
            return Vec2::ZERO;
        };
        let Some(prev_modern) = prev.modern.as_ref() else {
            return Vec2::ZERO;
        };
        map_piece_offset(
            &prev_modern.view.pieces,
            piece_index,
            (line, end, left, top),
            self.alpha,
        )
    }

    fn run_offset(self, piece_index: usize, curr: Run) -> Vec2 {
        let Some(prev_modern) = self.prev.and_then(|p| p.modern.as_ref()) else {
            return Vec2::ZERO;
        };
        run_piece_offset(&prev_modern.view.pieces, piece_index, curr, self.alpha)
    }

    fn sprite_shift(self, mark: Mark) -> Vec2 {
        let (Some(prev), Some(curr)) = (self.prev, self.curr) else {
            return Vec2::ZERO;
        };
        drawer_screen_shift(prev, curr, mark, self.alpha)
    }
}

/// True when two consecutive tick snapshots belong to the same unpaused room and may interpolate.
fn can_interpolate(prev: &TickSnapshot, curr: &TickSnapshot) -> bool {
    prev.room == curr.room && !prev.paused && !curr.paused
}

/// Returns `offset = interp - curr` (in NES pixels, y down) for a [`Piece::Map`]; `ZERO` on snap.
fn map_piece_offset(
    prev_pieces: &Pieces,
    piece_index: usize,
    (line, end, left, top): (usize, usize, i32, i32),
    alpha: f32,
) -> Vec2 {
    let Some(Piece::Map {
        line: prev_line,
        end: prev_end,
        left: prev_left,
        top: prev_top,
        ..
    }) = prev_pieces.get(piece_index).copied().flatten()
    else {
        return Vec2::ZERO;
    };
    if prev_line != line || prev_end != end {
        return Vec2::ZERO;
    }
    let delta_x = left - prev_left;
    let delta_y = top - prev_top;
    if delta_x.abs() > 16 || delta_y.abs() > 16 {
        return Vec2::ZERO;
    }
    let dx = f32::from(i16::try_from(delta_x).expect("within snap threshold"));
    let dy = f32::from(i16::try_from(delta_y).expect("within snap threshold"));
    let back = 1.0 - alpha;
    Vec2::new(-dx * back, -dy * back)
}

/// Signed shortest delta on a circular axis of length `period` (`period > 0`).
fn wrapped_delta(prev: i32, curr: i32, period: i32) -> i32 {
    let half = period / 2;
    (curr - prev + half).rem_euclid(period) - half
}

/// Returns `offset = interp - curr` (in NES pixels, y down) for a nametable [`Run`]; `ZERO` on snap.
fn run_piece_offset(prev_pieces: &Pieces, piece_index: usize, curr: Run, alpha: f32) -> Vec2 {
    let Some(Piece::Run(prev)) = prev_pieces.get(piece_index).copied().flatten() else {
        return Vec2::ZERO;
    };
    if prev.line != curr.line || prev.end != curr.end || prev.top.mirroring != curr.top.mirroring {
        return Vec2::ZERO;
    }
    let period_x = if curr.top.mirroring == Mirroring::SideBySide {
        512
    } else {
        256
    };
    let delta_x = wrapped_delta(
        i32::from(prev.scroll_x()).rem_euclid(period_x),
        i32::from(curr.scroll_x()).rem_euclid(period_x),
        period_x,
    );
    let run_y = |run: Run| {
        i32::try_from(((usize::from(run.top.v) >> 5) & 0x1F) * 8 + run.fine_y())
            .expect("nametable y")
    };
    let delta_y = wrapped_delta(run_y(prev), run_y(curr), 240);
    if delta_x.abs() > 16 || delta_y.abs() > 16 {
        return Vec2::ZERO;
    }
    let dx = f32::from(i16::try_from(delta_x).expect("within snap threshold"));
    let dy = f32::from(i16::try_from(delta_y).expect("within snap threshold"));
    let back = 1.0 - alpha;
    Vec2::new(-dx * back, -dy * back)
}

/// Net screen-space shift `(interp_world - curr_world) - (interp_cam - curr_cam)` in NES pixels
/// (y down) for a sprite drawer; returns `ZERO` for UI/Scene marks or on any snap condition
/// (slot reuse, empty slot, teleport, or large camera jump).
fn drawer_screen_shift(prev: &FrameSnapshot, curr: &FrameSnapshot, mark: Mark, alpha: f32) -> Vec2 {
    if !matches!(
        mark,
        Mark::Player | Mark::Actor(_) | Mark::Second(_) | Mark::Projectile(_)
    ) {
        return Vec2::ZERO;
    }
    let Some(p) = prev.drawers.iter().find(|d| d.drawer == mark) else {
        return Vec2::ZERO;
    };
    let Some(c) = curr.drawers.iter().find(|d| d.drawer == mark) else {
        return Vec2::ZERO;
    };
    if p.kind != c.kind || c.kind == Some(0) {
        return Vec2::ZERO;
    }
    let pos = |x: u16, y: u16| Vec2::new(f32::from(x), f32::from(y)) / 16.0;
    let d_world = pos(c.x, c.y) - pos(p.x, p.y);
    if d_world.x.abs() > SNAP_THRESHOLD || d_world.y.abs() > SNAP_THRESHOLD {
        return Vec2::ZERO;
    }
    let d_cam = pos(curr.camera.0, curr.camera.1) - pos(prev.camera.0, prev.camera.1);
    let d_cam = if d_cam.x.abs() > SNAP_THRESHOLD || d_cam.y.abs() > SNAP_THRESHOLD {
        Vec2::ZERO
    } else {
        d_cam
    };
    let back = 1.0 - alpha;
    (d_cam - d_world) * back
}

/// What the picture is drawn from.
#[derive(SystemParam)]
struct Inputs<'w> {
    play: Res<'w, Play>,
    settings: Res<'w, Settings>,
    colours: Res<'w, Colours>,
    screen: Res<'w, Screen>,
    clock: Option<Res<'w, Clock>>,
    menu: Option<Res<'w, Menu>>,
    history: Option<Res<'w, InterpolationHistory>>,
    display_scale: Option<Res<'w, DisplayScale>>,
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

/// Allocates a fresh off-screen target at `desired_scale`, repoints every Modern camera to it,
/// and marks `modern.ready = false` for one frame so `prepare_assets::<GpuImage>` runs before
/// any camera attaches a depth buffer to the new texture.
fn resize_target(
    modern: &mut Modern,
    images: &mut Assets<Image>,
    cameras: &mut Cameras<'_, '_>,
    desired_scale: u32,
) {
    let new_image = images.add(target_image(desired_scale));
    if let Ok((_, _, _, mut target)) = cameras.get_mut(modern.backdrop) {
        *target = RenderTarget::Image(new_image.clone().into());
    }
    for layer in &modern.pieces {
        if let Ok((_, _, _, mut target)) = cameras.get_mut(layer.camera) {
            *target = RenderTarget::Image(new_image.clone().into());
        }
    }
    modern.image = new_image;
    modern.scale = desired_scale;
    modern.ready = false;
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
        clock,
        menu,
        history,
        display_scale,
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
    let active = ActivePicture::for_frame(&settings, &play);
    let frame = (active == ActivePicture::Modern)
        .then(|| play.session().snapshot().modern.as_ref())
        .flatten();
    let menu_open = menu.as_deref().is_some_and(Menu::is_open);
    let interpolating =
        frame.is_some() && !settings.frame_rate.is_classic() && !menu_open && !play.paused();
    let desired_scale = if settings.picture == Picture::Modern && !settings.frame_rate.is_classic()
    {
        display_scale.map_or(1, |s| s.0.clamp(1, 8))
    } else {
        1
    };
    if desired_scale != modern.scale {
        resize_target(&mut modern, &mut images, &mut cameras, desired_scale);
    }
    let ready = std::mem::replace(&mut modern.ready, true);
    let active_frame = ready.then_some(frame).flatten();
    if ready {
        let (shown_image, active_scale) = match active {
            ActivePicture::Modern => (modern.image.clone(), modern.scale),
            ActivePicture::Classic => (screen_image(&screen), 1),
        };
        let expected_rect = Some(screen_quad_rect(settings.overscan, active_scale));
        for mut quad in &mut quads {
            if quad.image != shown_image {
                quad.image = shown_image.clone();
            }
            if quad.rect != expected_rect {
                quad.rect = expected_rect;
            }
        }
    }
    let rgb = &colours_table.rgb;
    if let Ok((mut backdrop, _, _, _)) = cameras.get_mut(modern.backdrop) {
        backdrop.is_active = active_frame.is_some();
        if let Some(frame) = active_frame {
            let [red, green, blue] =
                colours(&play.session().ppu().palette, 0, frame.greyscale, rgb)[0];
            backdrop.clear_color = ClearColorConfig::Custom(Color::srgb_u8(red, green, blue));
        }
    }
    let mut out = Out {
        images: &mut images,
        chunks: &mut chunks,
        cameras: &mut cameras,
        scale: modern.scale,
    };
    let Some(frame) = active_frame else {
        for layer in &mut modern.pieces {
            show_layer(layer, &mut out, &play, [[[0; 3]; 4]; 4], None);
        }
        return;
    };
    let alpha = if interpolating {
        clock.map_or(1.0, |c| c.alpha())
    } else {
        1.0
    };
    let interp = Interp::from_history(history.as_deref(), alpha);
    show_background(&mut modern, &mut out, &play, frame, rgb, interp);
    let faithful = settings.ui == UiStyle::Faithful;
    spawn_sprites(
        &mut commands,
        &mut modern,
        &mut images,
        &drawers,
        &play,
        SpriteFrame {
            faithful,
            rgb,
            interp,
        },
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
type Cameras<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Camera,
        &'static mut Transform,
        &'static mut Projection,
        &'static mut RenderTarget,
    ),
>;

/// What drawing a piece changes.
struct Out<'a, 'w, 's> {
    images: &'a mut Assets<Image>,
    chunks: &'a mut Chunks<'w, 's>,
    cameras: &'a mut Cameras<'w, 's>,
    scale: u32,
}

/// Each piece's tilemaps: map playfields from the frame's map tiles, nametable runs from the PPU,
/// in screen space.
fn show_background(
    modern: &mut Modern,
    out: &mut Out<'_, '_, '_>,
    play: &Play,
    frame: &ModernFrame,
    rgb: &Rgb,
    interp: Interp<'_>,
) {
    let sets: [Set; 4] = std::array::from_fn(|set| {
        colours(&play.session().ppu().palette, set * 4, frame.greyscale, rgb)
    });
    let run_grids: [Option<TileGrid>; MAX_RUNS] =
        std::array::from_fn(|index| match frame.view.pieces[index] {
            Some(Piece::Run(run)) => Some(run_tiles(play, &run)),
            _ => None,
        });
    let guard_px = pixels(GUARD * 8);
    for (index, layer) in modern.pieces.iter_mut().enumerate() {
        let shown = match frame.view.pieces[index] {
            Some(
                piece @ Piece::Map {
                    line,
                    end,
                    left,
                    top,
                    banks,
                },
            ) => {
                let fine_x = f32::from(u8::try_from(left.rem_euclid(8)).expect("0-7"));
                let fine_y = f32::from(u8::try_from(top.rem_euclid(8)).expect("0-7"));
                let offset = interp.map_offset(index, piece);
                frame.tiles_for(index).map(|tiles| Shown {
                    banks,
                    tiles,
                    corner: Vec2::new(
                        -fine_x - guard_px - offset.x,
                        fine_y + guard_px - pixels(line) + offset.y,
                    ),
                    lines: line..end,
                })
            }
            Some(Piece::Run(run)) => {
                let offset = interp.run_offset(index, run);
                run_grids[index]
                    .as_deref()
                    .map(|tiles| screen_space(&run, tiles, offset))
            }
            None => None,
        };
        show_layer(layer, out, play, sets, shown);
    }
}

/// A run's piece in screen space, its tiles from [`run_tiles`].
fn screen_space<'a>(run: &Run, tiles: &'a [Option<(u8, u8)>], offset: Vec2) -> Shown<'a> {
    let guard_px = pixels(GUARD * 8);
    Shown {
        banks: run.banks(),
        tiles,
        corner: Vec2::new(
            -f32::from(run.top.fine_x) - guard_px - offset.x,
            pixels(run.fine_y()) + guard_px - pixels(run.line) + offset.y,
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

/// The run's tiles on the guard-padded layer grid, from one tile above/left of its top row.
fn run_tiles(play: &Play, run: &Run) -> TileGrid {
    let rows = (run.end - run.line + run.fine_y()).div_ceil(8) + 2 * GUARD;
    let guard = i32::try_from(GUARD).expect("small guard");
    (0..GRID_ROWS)
        .flat_map(|row| (0..GRID_COLUMNS).map(move |column| (column, row)))
        .map(|(column, row)| {
            (row < rows).then(|| {
                let col = i32::try_from(column).expect("column") - guard;
                let r = i32::try_from(row).expect("row") - guard;
                run_tile_signed(play.session().ppu(), run, col, r)
            })
        })
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
    if let Ok((mut camera, mut transform, mut projection, _)) = out.cameras.get_mut(layer.camera) {
        camera.is_active = shown.is_some();
        if let Some(shown) = &shown {
            let s = out.scale.clamp(1, 8);
            let lines = shown.lines.clone();
            camera.viewport = Some(Viewport {
                physical_position: UVec2::new(0, size(lines.start) * s),
                physical_size: UVec2::new(size(SCREEN_W) * s, size(lines.len()) * s),
                ..default()
            });
            transform.translation.y = -pixels(lines.start + lines.end) / 2.0;
            if let Projection::Orthographic(orthographic) = &mut *projection {
                orthographic.scale = 1.0 / f32::from(u16::try_from(s).expect("1..=8"));
            }
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
    let wanted = BuiltTileset {
        banks: shown.banks,
        sets,
    };
    if layer.built != Some(wanted) {
        for (set, handle) in layer.tilesets.iter().enumerate() {
            if let Some(mut image) = out.images.get_mut(handle) {
                *image = tileset(&play.chr, shown.banks, sets[set]);
            }
        }
        layer.built = Some(wanted);
    }
    let centre = Vec3::new(
        shown.corner.x + pixels(GRID_COLUMNS * 4),
        shown.corner.y - pixels(GRID_ROWS * 4),
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
            UVec2::new(size(GRID_COLUMNS), size(GRID_ROWS)),
            tiles.collect(),
        );
        transform.translation = centre;
        *visibility = Visibility::Visible;
    }
}

/// Contiguous vertical row spans `[row_start..row_end)` of an 8x16 OAM sprite sharing one
/// `(sprite_chr, palette)` [`Key`]. Steady sprites produce one span; bank-split sprites (such as
/// Batman straddling Stage 1's boss floor) produce two clipped slices.
fn sprite_slices(
    entry: OamEntry,
    lines: &[Line],
    palette: &[u8; 32],
    rgb: &Rgb,
) -> Vec<(usize, usize, Key)> {
    let top_y = usize::from(entry.y) + 1;
    let mut slices: Vec<(usize, usize, Key)> = Vec::new();
    for row in 0..16 {
        let Some(line) = lines.get(top_y + row) else {
            break;
        };
        let Some(key) = sprite_key_at_line(entry, line, palette, rgb) else {
            continue;
        };
        if let Some(last) = slices.last_mut()
            && last.1 == row
            && last.2 == key
        {
            last.1 = row + 1;
        } else {
            slices.push((row, row + 1, key));
        }
    }
    slices
}

/// Builds a Bevy [`Sprite`] for rows `row_start..row_end` of `entry`'s 8x16 tile in `atlas`.
fn slice_sprite(
    atlas: Handle<Image>,
    layout: &Handle<TextureAtlasLayout>,
    entry: OamEntry,
    row_start: usize,
    row_end: usize,
) -> Sprite {
    let flip_x = entry.attr & attr::FLIP_X != 0;
    let flip_y = entry.attr & attr::FLIP_Y != 0;
    let mut sprite = if row_start == 0 && row_end == 16 {
        Sprite::from_atlas_image(
            atlas,
            TextureAtlas {
                layout: layout.clone(),
                index: usize::from(entry.tall_index()),
            },
        )
    } else {
        let index = usize::from(entry.tall_index());
        let tx = pixels((index % 16) * SPRITE_CELL_W + SPRITE_ATLAS_PAD);
        let ty = pixels((index / 16) * SPRITE_CELL_H + SPRITE_ATLAS_PAD);
        let (src_top, src_bottom) = if flip_y {
            (pixels(16 - row_end), pixels(16 - row_start))
        } else {
            (pixels(row_start), pixels(row_end))
        };
        let mut s = Sprite::from_image(atlas);
        s.rect = Some(Rect::new(tx, ty + src_top, tx + 8.0, ty + src_bottom));
        s
    };
    sprite.flip_x = flip_x;
    sprite.flip_y = flip_y;
    sprite
}

/// Per-frame sprite rendering options passed to [`spawn_sprites`].
#[derive(Clone, Copy)]
struct SpriteFrame<'a> {
    faithful: bool,
    rgb: &'a Rgb,
    interp: Interp<'a>,
}

/// Every record's sprites in draw order, each a child of its drawer's mirror entity: behind
/// the tilemaps when its priority bit says, an earlier draw in front of a later one.
fn spawn_sprites(
    commands: &mut Commands<'_, '_>,
    modern: &mut Modern,
    images: &mut Assets<Image>,
    drawers: &Query<'_, '_, (Entity, &Drawer)>,
    play: &Play,
    frame: SpriteFrame<'_>,
) {
    let SpriteFrame {
        faithful,
        rgb,
        interp,
    } = frame;
    let session = play.session();
    let records = session
        .snapshot()
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
        let shift = interp.sprite_shift(record.mark);
        for entry in record.sprites(256).filter(|entry| entry.is_visible()) {
            let slices = sprite_slices(entry, session.lines(), &session.ppu().palette, rgb);
            if slices.is_empty() {
                continue;
            }
            let depth = if entry.attr & attr::BEHIND != 0 {
                1.5
            } else {
                3.5
            };
            let z = depth - f32::from(order) / 1024.0;
            for (row_start, row_end, key) in slices {
                used.insert(key);
                let atlas = modern
                    .atlases
                    .entry(key)
                    .or_insert_with(|| images.add(sprite_atlas_tiles(&play.chr, &key)))
                    .clone();
                let sprite = slice_sprite(atlas, &modern.layout, entry, row_start, row_end);
                let mut spawned = commands.spawn((
                    ModernSprite,
                    sprite,
                    Anchor::TOP_LEFT,
                    Transform::from_xyz(
                        f32::from(entry.x) + shift.x,
                        -f32::from(entry.y) - 1.0 - pixels(row_start) - shift.y,
                        z,
                    ),
                    RenderLayers::layer(SPRITES),
                ));
                if let Some(parent) = parent {
                    spawned.insert(ChildOf(parent));
                }
            }
            order = order.saturating_add(1);
        }
    }
    modern.atlases.retain(|key, _| used.contains(key));
}

/// The Modern picture.
pub(crate) struct ModernPlugin;

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

    use rotj_core::Buttons;
    use rotj_core::sim::Transition;
    use rotj_core::sim::modern::View;
    use rotj_core::sim::session::DrawerState;

    /// Batman walking right and firing every other pair of frames.
    fn pad(frame: usize) -> Buttons {
        if frame % 4 < 2 {
            Buttons::B | Buttons::RIGHT
        } else {
            Buttons::RIGHT
        }
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

    /// Stage 1-1, the `$0E` boss bar, Stage 1 boss floor, band rooms, and final boss; one
    /// test, since two headless renders at once fail.
    #[test]
    #[ignore = "renders with the GPU (here software Vulkan); ROTJ_SCREENSHOT names the PNGs' directory"]
    fn stage_1_and_a_boss_bar_render_modern() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let piece_summary = |view: Option<View>| {
            view.map(|v| {
                v.pieces
                    .iter()
                    .flatten()
                    .map(|p| (p.is_map(), p.line(), p.end()))
                    .collect::<Vec<_>>()
            })
        };
        let play = Play::start(crate::play::tests::prg(rom), rom.chr.clone(), 0);
        let (drawn, view) = renders_as_classic(play, 180, pad, "stage-1");
        assert!(drawn > 100, "{drawn} of 180 frames drawn Modern");
        assert_eq!(piece_summary(view), Some(vec![(true, 0, 240)]));
        let mut play = Play::start(crate::play::tests::prg(rom), rom.chr.clone(), 5);
        crate::play::tests::enter_the_boss_room(&mut play, 0x0E);
        let (drawn, view) = renders_as_classic(play, 60, |_| Buttons::NONE, "boss-bar");
        assert_eq!(drawn, 60, "boss room frames drawn Modern");
        assert_eq!(
            piece_summary(view),
            Some(vec![(true, 0, 192), (false, 192, 193), (false, 193, 240)]),
            "the map down to the bar, its mixed line, the bar"
        );
        let mut play = Play::start(crate::play::tests::prg(rom), rom.chr.clone(), 1);
        crate::play::tests::enter_the_boss_room(&mut play, 0x04);
        let (drawn, view) = renders_as_classic(play, 60, |_| Buttons::NONE, "boss-floor");
        assert_eq!(
            drawn, 60,
            "Stage 1 boss floor frames (bank-split sprites) all draw Modern"
        );
        assert!(
            view.is_some_and(|v| v.has_map()),
            "boss floor arena draws from the map: {view:?}"
        );
        let play = Play::start(crate::play::tests::prg(rom), rom.chr.clone(), 9);
        let (drawn, view) = renders_as_classic(play, 61, pad, "seven-bands");
        assert_eq!(drawn, 61, "area $18's frames drawn Modern");
        assert_eq!(
            view.map(|v| (v.has_map(), v.pieces.iter().flatten().count())),
            Some((false, 6)),
            "six parallax runs from the nametables"
        );
        let play = Play::start(crate::play::tests::prg(rom), rom.chr.clone(), 11);
        // From frame 40 the rotation puts two of an enemy's overlapping sprites out of draw order.
        let (drawn, view) = renders_as_classic(play, 31, pad, "fixed-top");
        assert_eq!(drawn, 31, "area $1C's frames drawn Modern");
        assert_eq!(
            view.map(|v| (
                v.pieces.iter().flatten().filter(|p| p.is_map()).count(),
                v.pieces.iter().flatten().count()
            )),
            Some((2, 3)),
            "top and bottom map playfields around the fixed middle art"
        );
        let mut play = Play::start(crate::play::tests::prg(rom), rom.chr.clone(), 12);
        for arg in [0xC1, 0x61] {
            crate::play::tests::transition(&mut play, Transition::RoomChange, arg);
        }
        crate::play::tests::enter_the_boss_room(&mut play, 0x22);
        let intact = play.session().ppu().nametables;
        let (dest, mirroring) = play
            .destroy_final_boss_part()
            .expect("a part of the final boss");
        for _ in 0..10 {
            play.step(Buttons::NONE);
        }
        let table = mirroring.physical(usize::from(dest >> 10) & 3);
        let block = (0..4).flat_map(|row| (0..4).map(move |column| row * 32 + column));
        let changed = block
            .map(|cell| usize::from(dest & 0x3FF) + cell)
            .any(|at| play.session().ppu().nametables[table][at] != intact[table][at]);
        assert!(
            changed,
            "the death tiles at ${dest:04X} reach the nametables"
        );
        // An odd frame: the allocator's rotation then puts the portrait's overlapping sprites in draw order.
        let (drawn, view) = renders_as_classic(play, 61, |_| Buttons::NONE, "final-boss");
        assert!(drawn > 0, "final boss frames drawn Modern");
        assert_eq!(
            view.map(|v| v.has_map()),
            Some(false),
            "the playfield from the nametables"
        );
    }

    /// Plays `frames` frames Modern and asserts its screenshot is Classic's.
    fn renders_as_classic(
        play: Play,
        frames: usize,
        pad: fn(usize) -> Buttons,
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
            drawn += usize::from(
                app.world()
                    .resource::<Play>()
                    .session()
                    .snapshot()
                    .modern
                    .is_some(),
            );
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
        let view = play
            .session()
            .snapshot()
            .modern
            .as_ref()
            .map(|frame| frame.view);
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

    #[test]
    fn bank_split_sprite_slices_respect_flip_y() {
        let mut lines = vec![
            Line {
                mask: rotj_core::render::mask::SPRITES,
                sprite_chr: [1, 2, 3, 4, 5, 6, 7, 8],
                ..Line::default()
            };
            SCREEN_H
        ];
        for line in &mut lines[192..] {
            line.sprite_chr[0] = 0x39;
        }
        let entry = OamEntry {
            y: 185,
            tile: 2,
            attr: attr::FLIP_Y,
            x: 40,
        };
        let palette = [0x0F; 32];
        let slices = sprite_slices(entry, &lines, &palette, &rotj_core::palette::NES_PALETTE);
        assert_eq!(slices.len(), 2);
        assert_eq!((slices[0].0, slices[0].1, slices[0].2.banks[0]), (0, 6, 1));
        assert_eq!(
            (slices[1].0, slices[1].1, slices[1].2.banks[0]),
            (6, 16, 0x39)
        );

        let dummy_atlas = Handle::<Image>::default();
        let dummy_layout = Handle::<TextureAtlasLayout>::default();
        let top_slice = slice_sprite(dummy_atlas.clone(), &dummy_layout, entry, 0, 6);
        let bottom_slice = slice_sprite(dummy_atlas, &dummy_layout, entry, 6, 16);
        // Cell 1 in the padded 10x18 atlas starts at (11, 1).
        assert_eq!(top_slice.rect, Some(Rect::new(11.0, 11.0, 19.0, 17.0)));
        assert_eq!(bottom_slice.rect, Some(Rect::new(11.0, 1.0, 19.0, 11.0)));
        assert!(top_slice.flip_y && bottom_slice.flip_y);
    }

    #[test]
    fn sprite_atlas_pads_every_tile_with_transparent_gutter() {
        let chr = vec![0xFF; 8 * 1024];
        let key = Key {
            banks: [0, 1, 2, 3],
            colours: [[0; 3], [10; 3], [20; 3], [30; 3]],
        };
        let atlas = sprite_atlas_tiles(&chr, &key);
        let width = 16 * SPRITE_CELL_W;
        let data = atlas.data.expect("pixels");
        let alpha = |x: usize, y: usize| data[(y * width + x) * 4 + 3];
        for cell in 0..128 {
            let x0 = (cell % 16) * SPRITE_CELL_W;
            let y0 = (cell / 16) * SPRITE_CELL_H;
            // Inner tile pixels are opaque; 1 px border on all 4 sides is transparent.
            assert_eq!(alpha(x0 + 1, y0 + 1), 0xFF);
            assert_eq!(alpha(x0 + 8, y0 + 16), 0xFF);
            for dx in 0..SPRITE_CELL_W {
                assert_eq!(alpha(x0 + dx, y0), 0, "top gutter cell {cell}");
                assert_eq!(
                    alpha(x0 + dx, y0 + SPRITE_CELL_H - 1),
                    0,
                    "bottom gutter cell {cell}"
                );
            }
            for dy in 0..SPRITE_CELL_H {
                assert_eq!(alpha(x0, y0 + dy), 0, "left gutter cell {cell}");
                assert_eq!(
                    alpha(x0 + SPRITE_CELL_W - 1, y0 + dy),
                    0,
                    "right gutter cell {cell}"
                );
            }
        }
    }

    #[test]
    fn interpolation_midpoints_and_snap_conditions() {
        // 1. Drawer moving +4 px right while camera stays still.
        let snap = |cam_x: u16, actor_kind: u8, actor_x: u16| FrameSnapshot {
            camera: (cam_x * 16, 0),
            drawers: vec![
                DrawerState {
                    drawer: Mark::Player,
                    kind: None,
                    x: 100 * 16,
                    y: 120 * 16,
                },
                DrawerState {
                    drawer: Mark::Actor(0),
                    kind: Some(actor_kind),
                    x: actor_x * 16,
                    y: 80 * 16,
                },
            ],
            ..FrameSnapshot::default()
        };
        let prev = snap(0, 0x12, 50);
        let curr = snap(0, 0x12, 54);
        assert_eq!(
            drawer_screen_shift(&prev, &curr, Mark::Actor(0), 0.0),
            Vec2::new(-4.0, 0.0)
        );
        assert_eq!(
            drawer_screen_shift(&prev, &curr, Mark::Actor(0), 0.5),
            Vec2::new(-2.0, 0.0)
        );
        assert_eq!(
            drawer_screen_shift(&prev, &curr, Mark::Actor(0), 1.0),
            Vec2::ZERO
        );

        // 2. Stationary player while camera scrolls +4 px right stays locked to floor.
        let scroll_curr = snap(4, 0x12, 50);
        assert_eq!(
            drawer_screen_shift(&prev, &scroll_curr, Mark::Player, 0.5),
            Vec2::new(2.0, 0.0)
        );

        // 3. Slot reuse (different kind) snaps to ZERO.
        let reused = snap(0, 0x34, 54);
        assert_eq!(
            drawer_screen_shift(&prev, &reused, Mark::Actor(0), 0.5),
            Vec2::ZERO
        );

        // 4. Teleport (> 16 px) snaps to ZERO.
        let teleported = snap(0, 0x12, 90);
        assert_eq!(
            drawer_screen_shift(&prev, &teleported, Mark::Actor(0), 0.5),
            Vec2::ZERO
        );

        // 5. UI marks never interpolate.
        assert_eq!(
            drawer_screen_shift(&prev, &curr, Mark::Hud, 0.5),
            Vec2::ZERO
        );

        // 6. Room change or pause disables interpolation cleanly.
        let mut hist = InterpolationHistory::default();
        hist.push(TickSnapshot {
            room: (0, 0),
            paused: false,
            snapshot: prev.clone(),
        });
        hist.push(TickSnapshot {
            room: (0, 2),
            paused: false,
            snapshot: curr.clone(),
        });
        assert_eq!(
            Interp::from_history(Some(&hist), 0.5).sprite_shift(Mark::Actor(0)),
            Vec2::ZERO
        );

        // 7. Map piece interpolation and snap.
        let mut pieces: Pieces = [None; MAX_RUNS];
        pieces[0] = Some(Piece::Map {
            line: 0,
            end: 240,
            left: 100,
            top: 0,
            banks: [0; 4],
        });
        assert_eq!(
            map_piece_offset(&pieces, 0, (0, 240, 104, 0), 0.25),
            Vec2::new(-3.0, 0.0)
        );
        assert_eq!(
            map_piece_offset(&pieces, 0, (0, 240, 150, 0), 0.25),
            Vec2::ZERO,
            "camera jump > 16 px snaps"
        );

        // 8. Parallax run horizontal wrap (510 -> 2 mod 512 = +4 px).
        let run_at = |sx: u16| Run {
            line: 0,
            end: 64,
            top: Line {
                v: ((sx >> 8) << 10) | ((sx >> 3) & 0x1F),
                fine_x: u8::try_from(sx & 7).expect("0..7"),
                mirroring: Mirroring::SideBySide,
                ..Line::default()
            },
        };
        let mut run_pieces: Pieces = [None; MAX_RUNS];
        run_pieces[0] = Some(Piece::Run(run_at(510)));
        assert_eq!(
            run_piece_offset(&run_pieces, 0, run_at(2), 0.5),
            Vec2::new(-2.0, 0.0)
        );
    }

    #[test]
    fn resizing_modern_target_allocates_fresh_handle_and_waits_one_frame() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::sprite_render::TilemapChunkPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<Mesh>()
        .init_asset::<bevy::sprite_render::TilemapChunkMaterial>()
        .init_asset::<TextureAtlasLayout>()
        .insert_resource(Settings {
            picture: Picture::Modern,
            frame_rate: crate::settings::FrameRate::Fps(170),
            ..Settings::default()
        })
        .init_resource::<Colours>()
        .insert_resource(DisplayScale(1))
        .add_plugins((crate::mirror::MirrorPlugin, ModernPlugin));
        let screen = Screen::new(&mut app.world_mut().resource_mut::<Assets<Image>>());
        app.insert_resource(screen).insert_resource(Play::start(
            crate::play::tests::prg(rom),
            rom.chr.clone(),
            0,
        ));
        app.update();
        let initial_id = app.world().resource::<Modern>().image.id();
        let backdrop = app.world().resource::<Modern>().backdrop;
        assert!(app.world().get::<Camera>(backdrop).expect("cam").is_active);

        // Resize to 3x (768x720): must allocate a brand-new AssetId and deactivate cameras for 1 frame.
        app.world_mut().resource_mut::<DisplayScale>().0 = 3;
        app.update();
        let resized_id = app.world().resource::<Modern>().image.id();
        assert_ne!(resized_id, initial_id, "must allocate fresh image handle");
        assert!(
            !app.world().get::<Camera>(backdrop).expect("cam").is_active,
            "backdrop waits one frame before rendering to resized target"
        );
        let target = app.world().get::<RenderTarget>(backdrop).expect("target");
        assert!(
            matches!(target, RenderTarget::Image(img) if img.handle.id() == resized_id),
            "camera target points to new image handle"
        );

        // Next frame: GpuImage is prepared, backdrop activates cleanly on the 768x720 target.
        app.update();
        assert!(app.world().get::<Camera>(backdrop).expect("cam").is_active);
    }
}
