//! The shared display image: every screen (title, gameplay) draws each frame into
//! a [`Frame`] through the port's NES-equivalent renderer and uploads its pixels, in the
//! chosen palette, to one Bevy image, shown by a screen quad cropped to the visible lines.

use std::path::Path;

use bevy::prelude::*;
use bevy::window::{PresentMode, PrimaryWindow};
use rotj_core::palette::{NES_PALETTE, RGB_PPU, Rgb, from_pal, ntsc};
use rotj_core::render::{Frame, SCREEN_H, SCREEN_W};

use crate::settings::{Overscan, PaletteChoice, Settings, SettingsDir};

/// The stage's rendered picture, plus the display image it is uploaded to.
#[derive(Resource)]
pub struct Screen {
    frame: Frame,
    image: Handle<Image>,
    dirty: bool,
}

impl Screen {
    /// A black 256x240 image plus an empty frame.
    #[must_use]
    pub fn new(images: &mut Assets<Image>) -> Self {
        Self {
            frame: Frame::new(),
            image: images.add(blank_image()),
            dirty: true,
        }
    }
}

/// The colour table the picture is shown in.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct Colours {
    /// RGB for each palette-RAM value.
    pub rgb: Rgb,
    /// `File` was chosen but `palette.pal` is missing or the wrong size.
    pub file_missing: bool,
}

impl Default for Colours {
    fn default() -> Self {
        Self {
            rgb: NES_PALETTE,
            file_missing: false,
        }
    }
}

/// The table for `choice`; `File` reads `<dir>/palette.pal` each time it is asked for.
#[must_use]
pub fn colours_for(choice: PaletteChoice, dir: Option<&Path>) -> Colours {
    let (rgb, file_missing) = match choice {
        PaletteChoice::CompositeDirect => (NES_PALETTE, false),
        PaletteChoice::RgbPpu => (RGB_PPU, false),
        PaletteChoice::Ntsc => (ntsc(), false),
        PaletteChoice::File => dir
            .and_then(|dir| std::fs::read(dir.join("palette.pal")).ok())
            .and_then(|bytes| from_pal(&bytes))
            .map_or((NES_PALETTE, true), |rgb| (rgb, false)),
    };
    Colours { rgb, file_missing }
}

/// Opaque RGBA for each palette-RAM value in `indices`, through `rgb`.
pub fn rgba_into(indices: &[u8], rgb: &Rgb, out: &mut [u8]) {
    for (pixel, &index) in out.as_chunks_mut::<4>().0.iter_mut().zip(indices) {
        let [r, g, b] = rgb[usize::from(index & 0x3F)];
        pixel.copy_from_slice(&[r, g, b, 0xFF]);
    }
}

/// The first visible line and how many show.
#[must_use]
pub const fn visible_lines(overscan: Overscan) -> (usize, usize) {
    match overscan {
        Overscan::Hidden => (8, 224),
        Overscan::Shown => (0, SCREEN_H),
    }
}

/// Marker for the screen quad.
#[derive(Component, Debug)]
pub struct ScreenSprite;

/// Spawns the 4x-scaled screen quad showing [`Screen`]'s image.
pub fn spawn_screen_quad(commands: &mut Commands<'_, '_>, image: &Handle<Image>) {
    commands.spawn((
        ScreenSprite,
        Sprite::from_image(image.clone()),
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));
}

/// The image the port's picture is uploaded to.
#[must_use]
pub fn screen_image(screen: &Screen) -> Handle<Image> {
    screen.image.clone()
}

/// The frame the port draws into.
pub fn screen_frame(screen: &mut Screen) -> &mut Frame {
    screen.dirty = true;
    &mut screen.frame
}

/// A black 256x240 RGBA image.
fn blank_image() -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    Image::new(
        Extent3d {
            width: u32::try_from(SCREEN_W).expect("tiny"),
            height: u32::try_from(SCREEN_H).expect("tiny"),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![0; SCREEN_W * SCREEN_H * 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// Copies the rendered frame into the displayed image, in [`Colours`], when either changed.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
pub fn upload_frame(
    mut screen: ResMut<'_, Screen>,
    colours: Res<'_, Colours>,
    mut images: ResMut<'_, Assets<Image>>,
) {
    if !screen.dirty && !colours.is_changed() {
        return;
    }
    if let Some(mut image) = images.get_mut(&screen.image)
        && let Some(data) = image.data.as_mut()
    {
        rgba_into(screen.frame.indices(), &colours.rgb, data);
    }
    screen.dirty = false;
}

/// Applies changed [`Settings`] to the picture: its colours, the visible lines and vsync.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
pub fn apply_settings(
    settings: Res<'_, Settings>,
    dir: Res<'_, SettingsDir>,
    mut colours: ResMut<'_, Colours>,
    mut quads: Query<'_, '_, &mut Sprite, With<ScreenSprite>>,
    mut windows: Query<'_, '_, &mut Window, With<PrimaryWindow>>,
) {
    if !settings.is_changed() {
        return;
    }
    colours.set_if_neq(colours_for(settings.palette, dir.0.as_deref()));
    let (first, count) = visible_lines(settings.overscan);
    for mut sprite in &mut quads {
        sprite.rect = Some(Rect::new(
            0.0,
            pixels(first),
            pixels(SCREEN_W),
            pixels(first + count),
        ));
        sprite.custom_size = Some(Vec2::new(pixels(SCREEN_W), pixels(count)) * 4.0);
    }
    let mode = if settings.vsync {
        PresentMode::AutoVsync
    } else {
        PresentMode::AutoNoVsync
    };
    for mut window in &mut windows {
        if window.present_mode != mode {
            window.present_mode = mode;
        }
    }
}

/// `count` pixels as a length.
pub fn pixels(count: usize) -> f32 {
    f32::from(u16::try_from(count).expect("a screen's worth"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composite_direct_upload_is_the_classic_frame() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let mut play = crate::play::Play::power_on(rom.prg.clone(), rom.chr.clone(), None);
        for _ in 0..400 {
            play.step(0);
        }
        let mut frame = Frame::new();
        play.compose(&mut frame, rotj_core::render::MAX_SPRITES_PER_LINE);
        let mut ours = vec![0; frame.rgba().len()];
        rgba_into(frame.indices(), &NES_PALETTE, &mut ours);
        assert!(ours == frame.rgba(), "the upload differs from Frame::rgba");
    }

    #[test]
    fn the_file_palette_is_reread_and_falls_back() {
        let dir = std::env::temp_dir().join("rotj-screen-palette");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let missing = Colours {
            rgb: NES_PALETTE,
            file_missing: true,
        };
        assert_eq!(colours_for(PaletteChoice::File, Some(&dir)), missing);
        std::fs::write(dir.join("palette.pal"), [0x11; 192]).expect("write");
        assert_eq!(
            colours_for(PaletteChoice::File, Some(&dir)),
            Colours {
                rgb: [[0x11; 3]; 64],
                file_missing: false
            }
        );
        std::fs::remove_file(dir.join("palette.pal")).expect("remove");
        assert_eq!(colours_for(PaletteChoice::File, Some(&dir)), missing);
        assert_eq!(colours_for(PaletteChoice::File, None), missing);
    }
}
