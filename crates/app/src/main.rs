//! Batman: Return of the Joker — Rust/Bevy port entry point.

mod audio;
mod input;
mod play;
mod screen;

use bevy::camera::{OrthographicProjection, Projection, ScalingMode};
use bevy::image::ImagePlugin;
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::settings::{Backends, RenderCreation, WgpuSettings};
use bevy::time::Fixed;
use bevy::window::{Window, WindowResolution};

/// The user's ROM, embedded and checked at build time.
#[derive(Resource)]
pub(crate) struct RomData(pub rotj_rom::Rom);

const PRG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/prg.bin"));
const CHR: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/chr.bin"));

/// The ROM the build embedded; without one, the message and exit 2.
fn rom_or_exit() -> rotj_rom::Rom {
    if PRG.is_empty() {
        eprintln!(
            "batman-rotj was built without a ROM: set ROTJ_ROM or put \
             Batman - Return of the Joker (USA).nes in raw/ and rebuild."
        );
        if cfg!(windows) {
            eprintln!("Press Enter to close.");
            let _ = std::io::stdin().read_line(&mut String::new());
        }
        std::process::exit(2);
    }
    rotj_rom::Rom {
        prg: PRG.to_vec(),
        chr: CHR.to_vec(),
    }
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
                    // camera below letterboxes the picture instead, so any
                    // window size works and stays integer-scalable.
                    resolution: WindowResolution::default().with_scale_factor_override(1.0),
                    // One queued frame, not Bevy's two: a frame less of input lag.
                    desired_maximum_frame_latency: std::num::NonZeroU32::new(1),
                    ..default()
                }),
                ..default()
            }),
    )
    // NES logic ticks at 60 Hz — bevy's FixedUpdate default is 64.
    .insert_resource(Time::<Fixed>::from_hz(60.0))
    .insert_resource(RomData(rom))
    .init_state::<GameState>()
    .add_systems(Startup, spawn_camera)
    .add_plugins((audio::AudioPlugin, input::InputPlugin, play::PlayPlugin));

    // The display image is made before the app runs, so every system can rely on it.
    let screen = {
        let mut images = app.world_mut().resource_mut::<Assets<Image>>();
        screen::Screen::new(&mut images)
    };
    app.insert_resource(screen).run();
}

/// `Camera2d` covering the world; sprites use world units = NES pixels.
///
/// The projection shows at least the 4x-scaled screen (1024x960 world units),
/// so the whole picture is visible and centred in whatever window size the
/// compositor gives us, with letterboxing.
fn spawn_camera(mut commands: Commands<'_, '_>) {
    commands.spawn((
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::AutoMin {
                min_width: 1024.0,
                min_height: 960.0,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));
}
