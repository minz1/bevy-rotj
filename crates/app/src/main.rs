//! Batman: Return of the Joker — Rust/Bevy port entry point.

mod audio;
mod input;
mod menu;
mod pacing;
mod play;
mod screen;
mod settings;
mod ui;

use bevy::camera::{OrthographicProjection, Projection, ScalingMode};
use bevy::image::ImagePlugin;
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::settings::{Backends, RenderCreation, WgpuSettings};
use bevy::window::{PrimaryWindow, Window, WindowResolution};

/// The user's ROM: embedded at build time or, in a build without one, found at start-up.
#[derive(Resource)]
pub(crate) struct RomData(pub rotj_rom::Rom);

const PRG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/prg.bin"));
const CHR: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/chr.bin"));

/// The embedded ROM, else the first argument (a file dropped on the exe), `ROTJ_ROM` or a
/// `.nes` beside the exe; without one, where it looked, and exit 2.
fn rom_or_exit() -> rotj_rom::Rom {
    if !PRG.is_empty() {
        return rotj_rom::Rom {
            prg: PRG.to_vec(),
            chr: CHR.to_vec(),
        };
    }
    let arg = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    rotj_rom::Rom::load(arg.as_deref()).unwrap_or_else(|error| {
        eprintln!(
            "batman-rotj: no usable ROM found.\n\
             Put Batman - Return of the Joker (USA).nes next to batman-rotj{}, pass it as the \
             first argument (drop it onto the exe), or set ROTJ_ROM.\n{error}",
            std::env::consts::EXE_SUFFIX
        );
        if cfg!(windows) {
            eprintln!("Press Enter to close.");
            let _ = std::io::stdin().read_line(&mut String::new());
        }
        std::process::exit(2);
    })
}

/// Which wgpu backends to use.
///
/// The default is everything wgpu offers, which picks Vulkan here. On some
/// software-rendering setups (WSL's llvmpipe, notably) the Vulkan path fails at
/// presentation — `winit event loop returned an error: Exit Failure: 1` after
/// broken-pipe errors — and the GL backend is the workaround, when a GL driver
/// is installed. Override with `ROTJ_RENDER_BACKEND=gl|vulkan|auto`.
fn render_plugin() -> RenderPlugin {
    let requested = std::env::var("ROTJ_RENDER_BACKEND").unwrap_or_default();
    let backends = match requested.to_ascii_lowercase().as_str() {
        "vulkan" => Backends::VULKAN,
        "gl" => Backends::GL,
        "auto" | "" => Backends::all(),
        other => {
            warn!("unknown ROTJ_RENDER_BACKEND {other:?}; using every backend");
            Backends::all()
        }
    };
    RenderPlugin {
        render_creation: RenderCreation::Automatic(Box::new(WgpuSettings {
            backends: Some(backends),
            ..default()
        })),
        ..default()
    }
}

/// Top-level game states, following the game: its boot and title, then gameplay.
#[derive(States, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
enum GameState {
    #[default]
    /// Power-on, the intro and the title, to the new game's first loop frame.
    Title,
    /// Stage gameplay.
    Playing,
}

fn main() {
    let rom = rom_or_exit();
    let dir = settings::exe_dir();
    let saved = dir.as_deref().map(settings::load).unwrap_or_default();
    let mut app = App::new();
    app.add_plugins(
        // Nearest-neighbor for everything — pixel-art game, global default.
        DefaultPlugins
            .set(ImagePlugin::default_nearest())
            .set(render_plugin())
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Batman: Return of the Joker — Rust port".into(),
                    // No explicit resolution: a fixed 1024x960 window fails to
                    // present on some Wayland/WSLg setups (the event loop dies
                    // with "Exit Failure: 1" after broken-pipe errors). The
                    // camera letterboxes the picture at a whole multiple
                    // instead (`fit_camera`), so any window size works.
                    resolution: WindowResolution::default().with_scale_factor_override(1.0),
                    // One queued frame, not Bevy's two: a frame less of input lag.
                    desired_maximum_frame_latency: std::num::NonZeroU32::new(1),
                    ..default()
                }),
                ..default()
            }),
    )
    .insert_resource(RomData(rom))
    .insert_resource(saved)
    .init_resource::<screen::Colours>()
    .insert_resource(settings::SettingsDir(dir))
    .init_state::<GameState>()
    .add_systems(Startup, spawn_camera)
    .add_systems(Update, fit_camera)
    .add_plugins((
        pacing::PacingPlugin,
        menu::MenuPlugin,
        ui::UiPlugin,
        audio::AudioPlugin,
        input::InputPlugin,
        play::PlayPlugin,
    ));

    // The display image is made before the app runs, so every system can rely on it.
    let screen = {
        let mut images = app.world_mut().resource_mut::<Assets<Image>>();
        screen::Screen::new(&mut images)
    };
    app.insert_resource(screen).run();
}

/// `Camera2d` over the 4x-scaled screen quad (1024x960 world units), sized to the
/// window by [`fit_camera`].
fn spawn_camera(mut commands: Commands<'_, '_>) {
    commands.spawn((
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::WindowSize,
            ..OrthographicProjection::default_2d()
        }),
    ));
}

/// The projection's world units per pixel that show a `picture` (columns, lines) at the
/// largest whole multiple fitting a `width` x `height` window (at least 1), and the camera
/// offset that puts the picture's edges on whole pixels when a side is odd.
fn integer_view(width: f32, height: f32, picture: Vec2) -> (f32, Vec2) {
    let multiple = (width / picture.x)
        .floor()
        .min((height / picture.y).floor())
        .max(1.0);
    let scale = 4.0 / multiple;
    let half_if_odd = |side: f32| if side % 2.0 >= 1.0 { 0.5 } else { 0.0 };
    (
        scale,
        Vec2::new(half_if_odd(width), half_if_odd(height)) * scale,
    )
}

/// Letterboxes the visible lines at a whole multiple ([`integer_view`]): every NES pixel
/// the same size, so nothing shimmers as the camera scrolls.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
fn fit_camera(
    windows: Query<'_, '_, Ref<'_, Window>, With<PrimaryWindow>>,
    settings: Res<'_, settings::Settings>,
    mut cameras: Query<'_, '_, (&mut Projection, &mut Transform), With<Camera2d>>,
    mut ui_scale: ResMut<'_, UiScale>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    if !window.is_changed() && !settings.is_changed() {
        return;
    }
    let shown = screen::visible(settings.overscan);
    let picture = Vec2::new(screen::pixels(shown.width), screen::pixels(shown.height));
    let (scale, offset) = integer_view(window.width(), window.height(), picture);
    ui_scale.0 = 4.0 / scale;
    for (mut projection, mut transform) in &mut cameras {
        if let Projection::Orthographic(orthographic) = &mut *projection {
            orthographic.scale = scale;
        }
        transform.translation = offset.extend(transform.translation.z);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: Vec2 = Vec2::new(256.0, 240.0);
    const TV: Vec2 = Vec2::new(256.0, 224.0);

    #[test]
    fn the_picture_scales_by_whole_multiples_on_whole_pixels() {
        assert_eq!(
            integer_view(1659.0, 1400.0, FULL),
            (0.8, Vec2::new(0.4, 0.0))
        );
        assert_eq!(integer_view(1280.0, 1200.0, FULL), (0.8, Vec2::ZERO));
        assert_eq!(
            integer_view(1280.0, 1199.0, FULL),
            (1.0, Vec2::new(0.0, 0.5))
        );
        assert_eq!(integer_view(100.0, 100.0, FULL), (4.0, Vec2::ZERO));
        assert_eq!(integer_view(1280.0, 1150.0, TV), (0.8, Vec2::ZERO));
        assert_eq!(integer_view(1280.0, 1150.0, FULL), (1.0, Vec2::ZERO));
    }
}
