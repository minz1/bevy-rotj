//! The faithful UI: the game's own UI sprites (those its HUD, pause, title and password
//! drawers marked) shown as Bevy UI over the picture instead of by the PPU, so the HUD's
//! label holds still where the NES drew it every other frame.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use rotj_core::actor::oam::{Mark, attr};
use rotj_core::palette::Rgb;
use rotj_core::render::{Line, mask};

use crate::pacing::Step;
use crate::play::Play;
use crate::screen::{Colours, Visible, pixels, visible};
use crate::settings::{Settings, UiStyle};

/// What a tile atlas is drawn from: a pattern table's four 1 KiB CHR banks and a sprite
/// palette's colours, colour 0 transparent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    /// The CHR banks of the table.
    pub banks: [u8; 4],
    /// The palette's four colours.
    pub colours: [[u8; 3]; 4],
}

/// A UI sprite to show: its OAM bytes and its atlas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shown {
    /// OAM y: the sprite starts on the line after.
    pub y: u8,
    /// The 8x16 tile: bit 0 the pattern table, the rest the tile pair.
    pub tile: u8,
    /// Palette (bits 0-1) and flips (6, 7).
    pub attr: u8,
    /// Its left column.
    pub x: u8,
    /// The atlas holding its tile in its colours.
    pub key: Key,
}

/// The marked sprites the PPU would show, the label carried from the step before (when this
/// one drew none) first and the rest in OAM order, which the overlay draws back to front so
/// the lowest entry wins as on the PPU; each in its line's banks and colours, 8x16 mode.
#[must_use]
pub fn shown(
    oam: &[u8; 256],
    marks: &[Mark; 64],
    label: &[[u8; 4]],
    lines: &[Line],
    palette: &[u8; 32],
    rgb: &Rgb,
) -> Vec<Shown> {
    let carried = if marks.contains(&Mark::Label) {
        &[][..]
    } else {
        label
    };
    let marked = (0..64)
        .filter(|&entry| marks[entry].is_ui())
        .map(|entry| -> [u8; 4] { oam[entry * 4..entry * 4 + 4].try_into().expect("4 bytes") });
    carried
        .iter()
        .copied()
        .chain(marked)
        .filter_map(|entry| {
            let [y, tile, attr, x] = entry;
            Some(Shown {
                y,
                tile,
                attr,
                x,
                key: sprite_key(entry, lines, palette, rgb)?,
            })
        })
        .collect()
}

/// The atlas an OAM entry's 8x16 sprite is drawn from: its line's sprite banks and its
/// palette's colours, or `None` on a line with sprites off.
#[must_use]
pub fn sprite_key(entry: [u8; 4], lines: &[Line], palette: &[u8; 32], rgb: &Rgb) -> Option<Key> {
    let [y, tile, attr, _] = entry;
    let line = lines.get(usize::from(y) + 1)?;
    if line.mask & mask::SPRITES == 0 {
        return None;
    }
    let table = 4 * usize::from(tile & 1);
    let banks = line.sprite_chr[table..table + 4]
        .try_into()
        .expect("4 banks");
    let grey = if line.mask & mask::GREYSCALE != 0 {
        0x30
    } else {
        0x3F
    };
    let colours = std::array::from_fn(|v| {
        let entry = palette[0x10 + 4 * usize::from(attr & attr::PALETTE) + v] & grey;
        if v == 0 {
            [0; 3]
        } else {
            rgb[usize::from(entry)]
        }
    });
    Some(Key { banks, colours })
}

/// A pattern table as 128 8x16 tiles, 16 to a row, in `key`'s colours, colour 0 clear.
#[must_use]
pub fn tiles(chr: &[u8], key: &Key) -> Image {
    const SIDE: usize = 128;
    let mut data = vec![0; SIDE * SIDE * 4];
    for cell in 0..128 {
        for half in 0..2 {
            let addr = (2 * cell + half) * 16;
            let bank = usize::from(key.banks[addr >> 10]) * 1024 % chr.len();
            let bytes = &chr[bank + (addr & 0x3FF)..][..16];
            for row in 0..8 {
                for column in 0..8 {
                    let value = (bytes[row] >> (7 - column)) & 1
                        | ((bytes[row + 8] >> (7 - column)) & 1) << 1;
                    if value == 0 {
                        continue;
                    }
                    let (px, py) = ((cell % 16) * 8 + column, (cell / 16) * 16 + half * 8 + row);
                    let [r, g, b] = key.colours[usize::from(value)];
                    data[(py * SIDE + px) * 4..][..4].copy_from_slice(&[r, g, b, 0xFF]);
                }
            }
        }
    }
    Image::new(
        Extent3d {
            width: 128,
            height: 128,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// Where a sprite's top left lies in the visible area, in NES pixels.
fn position(piece: &Shown, area: Visible) -> Vec2 {
    Vec2::new(
        f32::from(piece.x) - pixels(area.x),
        f32::from(piece.y) + 1.0 - pixels(area.y),
    )
}

/// The atlases the shown sprites use, and their shared layout.
#[derive(Resource, Default)]
struct Atlases {
    images: HashMap<Key, Handle<Image>>,
    layout: Option<Handle<TextureAtlasLayout>>,
}

impl Atlases {
    /// Drops the atlases no key in `keys` uses.
    fn keep(&mut self, keys: &[Key]) {
        self.images.retain(|key, _| keys.contains(key));
    }
}

/// The overlay's root, replaced whenever what it shows changes.
#[derive(Component)]
struct Overlay;

#[derive(SystemParam)]
struct Art<'w> {
    atlases: ResMut<'w, Atlases>,
    images: ResMut<'w, Assets<Image>>,
    layouts: ResMut<'w, Assets<TextureAtlasLayout>>,
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
fn show(
    mut commands: Commands<'_, '_>,
    play: Option<Res<'_, Play>>,
    settings: Res<'_, Settings>,
    colours: Res<'_, Colours>,
    mut art: Art<'_>,
    overlays: Query<'_, '_, Entity, With<Overlay>>,
    mut last: Local<'_, Vec<Shown>>,
) {
    let pieces = match &play {
        Some(play) if settings.ui == UiStyle::Faithful => shown(
            &play.oam,
            &play.marks,
            &play.label,
            &play.lines,
            &play.ppu.palette,
            &colours.rgb,
        ),
        _ => Vec::new(),
    };
    if pieces == *last && !settings.is_changed() {
        return;
    }
    last.clone_from(&pieces);
    for overlay in &overlays {
        commands.entity(overlay).despawn();
    }
    let keys: Vec<Key> = pieces.iter().map(|piece| piece.key).collect();
    art.atlases.keep(&keys);
    let Some(play) = play.filter(|_| !pieces.is_empty()) else {
        return;
    };
    let layout = art
        .atlases
        .layout
        .get_or_insert_with(|| {
            art.layouts.add(TextureAtlasLayout::from_grid(
                UVec2::new(8, 16),
                16,
                8,
                None,
                None,
            ))
        })
        .clone();
    for key in keys {
        if !art.atlases.images.contains_key(&key) {
            let image = art.images.add(tiles(&play.chr, &key));
            art.atlases.images.insert(key, image);
        }
    }
    let area = visible(settings.overscan);
    let sprites: Vec<_> = pieces
        .iter()
        .rev()
        .map(|piece| {
            let at = position(piece, area);
            let mut image = ImageNode::from_atlas_image(
                art.atlases.images[&piece.key].clone(),
                TextureAtlas {
                    layout: layout.clone(),
                    index: usize::from(piece.tile >> 1),
                },
            );
            image.flip_x = piece.attr & attr::FLIP_X != 0;
            image.flip_y = piece.attr & attr::FLIP_Y != 0;
            (
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(at.x),
                    top: Val::Px(at.y),
                    width: Val::Px(8.0),
                    height: Val::Px(16.0),
                    ..default()
                },
                image,
            )
        })
        .collect();
    commands.spawn((
        Overlay,
        GlobalZIndex(-1),
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        children![(
            Node {
                width: Val::Px(pixels(area.width)),
                height: Val::Px(pixels(area.height)),
                overflow: Overflow::clip(),
                ..default()
            },
            Children::spawn(SpawnIter(sprites.into_iter())),
        )],
    ));
}

/// The faithful UI.
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Atlases>()
            .add_systems(Update, show.in_set(Step::Picture));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Overscan;
    use rotj_core::render::ctrl;

    const SHOWING: Line = Line {
        v: 0,
        fine_x: 0,
        ctrl: ctrl::TALL_SPRITES,
        mask: mask::SHOWN,
        chr: [0; 8],
        sprite_chr: [0, 1, 2, 3, 4, 5, 6, 7],
        mirroring: 0,
    };

    fn hud_marks() -> [Mark; 64] {
        let mut marks = [Mark::Scene; 64];
        marks[1] = Mark::Hud;
        marks
    }

    fn oam_with(entry: usize, bytes: [u8; 4]) -> [u8; 256] {
        let mut oam = [0xF4; 256];
        oam[entry * 4..entry * 4 + 4].copy_from_slice(&bytes);
        oam
    }

    #[test]
    fn shown_takes_the_marked_sprites_in_their_lines_colours() {
        let mut lines = [SHOWING; 240];
        lines[20].mask = mask::SHOWN | mask::GREYSCALE;
        let mut palette = [0; 32];
        palette[0x10 + 4 * 3..0x10 + 4 * 3 + 4].copy_from_slice(&[0x0F, 0x16, 0x27, 0x30]);
        let rgb: Rgb = std::array::from_fn(|i| [u8::try_from(i).expect("64"); 3]);
        let oam = oam_with(1, [10, 0x11, 3, 24]);
        let pieces = shown(&oam, &hud_marks(), &[], &lines, &palette, &rgb);
        assert_eq!(pieces.len(), 1);
        assert_eq!(
            pieces[0].key.banks,
            [4, 5, 6, 7],
            "odd tiles from the $1000 table"
        );
        assert_eq!(pieces[0].key.colours[1], [0x16; 3]);
        assert_eq!(
            pieces[0].key.colours[0], [0; 3],
            "colour 0 is clear, whatever the backdrop"
        );
        let grey = shown(
            &oam_with(1, [19, 0x10, 3, 24]),
            &hud_marks(),
            &[],
            &lines,
            &palette,
            &rgb,
        );
        assert_eq!(grey[0].key.banks, [0, 1, 2, 3]);
        assert_eq!(
            grey[0].key.colours[1], [0x10; 3],
            "greyscale keeps the luma bits"
        );
    }

    #[test]
    fn shown_skips_lines_with_sprites_off_and_unmarked_or_parked_sprites() {
        let mut lines = [SHOWING; 240];
        lines[11].mask = mask::BACKGROUND | mask::LEFT_BACKGROUND | mask::LEFT_SPRITES;
        let rgb = [[0; 3]; 64];
        let none = |oam: [u8; 256], marks: [Mark; 64]| {
            shown(&oam, &marks, &[], &lines, &[0; 32], &rgb).is_empty()
        };
        assert!(
            none(oam_with(1, [10, 1, 0, 24]), hud_marks()),
            "sprites off"
        );
        assert!(
            none(oam_with(1, [20, 1, 0, 24]), [Mark::Scene; 64]),
            "unmarked"
        );
        assert!(none(oam_with(1, [0xF4, 1, 0, 24]), hud_marks()), "parked");
    }

    #[test]
    fn the_label_carries_over_only_when_this_step_drew_none() {
        let lines = [SHOWING; 240];
        let rgb = [[0; 3]; 64];
        let label = [[20, 1, 3, 24]];
        let oam = oam_with(1, [30, 0x11, 3, 32]);
        let carried = shown(&oam, &hud_marks(), &label, &lines, &[0; 32], &rgb);
        assert_eq!(carried.len(), 2);
        let mut marks = hud_marks();
        marks[2] = Mark::Label;
        let mut oam = oam;
        oam[8..12].copy_from_slice(&[40, 1, 3, 24]);
        let drawn = shown(&oam, &marks, &label, &lines, &[0; 32], &rgb);
        assert_eq!(drawn.len(), 2, "this step's label, not the last's");
        assert!(drawn.iter().all(|piece| piece.y != 20));
    }

    #[test]
    fn positions_follow_the_visible_area() {
        let piece = Shown {
            y: 3,
            tile: 1,
            attr: 0,
            x: 250,
            key: Key {
                banks: [0; 4],
                colours: [[0; 3]; 4],
            },
        };
        assert_eq!(
            position(&piece, visible(Overscan::Shown)),
            Vec2::new(250.0, 4.0)
        );
        assert_eq!(
            position(&piece, visible(Overscan::Hidden)),
            Vec2::new(250.0, -4.0)
        );
    }

    #[test]
    fn the_atlas_cache_keeps_only_this_frames_keys() {
        let key = |bank| Key {
            banks: [bank; 4],
            colours: [[0; 3]; 4],
        };
        let mut atlases = Atlases::default();
        for bank in 0..3 {
            atlases.images.insert(key(bank), Handle::default());
        }
        atlases.keep(&[key(1)]);
        assert_eq!(atlases.images.keys().copied().collect::<Vec<_>>(), [key(1)]);
    }

    #[test]
    fn tiles_draw_a_pair_from_its_banks_in_its_colours() {
        let mut chr = vec![0; 8 * 1024];
        let pair = 5;
        let top = 3 * 1024 + 2 * pair * 16;
        chr[top] = 0x80;
        chr[top + 8] = 0x80;
        chr[top + 16 + 1] = 0x40;
        let key = Key {
            banks: [7, 7, 7, 3],
            colours: [[9; 3], [1; 3], [2; 3], [3; 3]],
        };
        let image = tiles(
            &chr,
            &Key {
                banks: [3, 7, 7, 7],
                ..key
            },
        );
        let data = image.data.expect("pixels");
        let at = |x: usize, y: usize| &data[(y * 128 + x) * 4..][..4];
        let (x0, y0) = (pair % 16 * 8, pair / 16 * 16);
        assert_eq!(
            at(x0, y0),
            [3, 3, 3, 0xFF],
            "value 3 at the top tile's first pixel"
        );
        assert_eq!(
            at(x0 + 1, y0 + 9),
            [1, 1, 1, 0xFF],
            "the bottom tile, value 1"
        );
        assert_eq!(at(x0 + 1, y0), [0, 0, 0, 0], "colour 0 clear");
    }
}
