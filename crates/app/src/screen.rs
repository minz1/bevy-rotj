//! The shared display image: every screen (title, gameplay) draws each frame into
//! a [`Frame`] through the port's NES-equivalent renderer and uploads its RGBA
//! pixels to one Bevy image, shown by a screen quad.

use bevy::prelude::*;
use rotj_core::render::{Frame, SCREEN_H, SCREEN_W};

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

/// Marker for the screen quad.
#[derive(Component, Debug)]
pub struct ScreenSprite;

/// Spawns the 4x-scaled screen quad showing [`Screen`]'s image.
pub fn spawn_screen_quad(commands: &mut Commands<'_, '_>, image: &Handle<Image>) {
    commands.spawn((
        ScreenSprite,
        Sprite {
            image: image.clone(),
            custom_size: Some(Vec2::new(
                f32::from(u16::try_from(SCREEN_W).expect("tiny")) * 4.0,
                f32::from(u16::try_from(SCREEN_H).expect("tiny")) * 4.0,
            )),
            ..default()
        },
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

/// Copies the rendered frame into the displayed image when it changed.
pub fn upload_frame(mut screen: ResMut<'_, Screen>, mut images: ResMut<'_, Assets<Image>>) {
    if !screen.dirty {
        return;
    }
    if let Some(mut image) = images.get_mut(&screen.image)
        && let Some(data) = image.data.as_mut()
    {
        data.copy_from_slice(screen.frame.rgba());
    }
    screen.dirty = false;
}
