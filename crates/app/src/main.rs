//! Batman: Return of the Joker — Rust/Bevy port entry point.

mod audio;
mod input;
mod menu;
mod mirror;
mod modern;
mod pacing;
mod play;
mod screen;
mod settings;
mod tilesets;
mod ui;

use std::path::{Path, PathBuf};

use bevy::camera::{OrthographicProjection, Projection, ScalingMode};
use bevy::image::ImagePlugin;
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::settings::{Backends, RenderCreation, WgpuSettings};
use bevy::window::{PrimaryWindow, Window, WindowResolution};
use rotj_core::levels::{self, Catalog, Levels, Source};

/// The user's ROM: embedded at build time or, in a build without one, found at start-up.
#[derive(Resource)]
pub(crate) struct RomData(pub rotj_rom::Rom);

/// The levels the game plays: a level pack's, else the original game's.
#[derive(Resource)]
pub(crate) struct GameLevels(pub Levels);

const PRG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/prg.bin"));
const CHR: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/chr.bin"));

/// What the command line names; a file dropped on the exe arrives as an argument.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Launch {
    /// The ROM, read in a build without one embedded.
    pub rom: Option<PathBuf>,
    /// The level pack's folder.
    pub pack: Option<PathBuf>,
}

/// The arguments read once: `--pack <dir>`, a folder or a `pack.json` name the pack, the
/// first other argument the ROM.
pub(crate) fn launch(args: impl IntoIterator<Item = PathBuf>) -> Result<Launch, String> {
    let mut launch = Launch::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let named = arg.as_os_str() == "--pack";
        let arg = if named {
            args.next().ok_or("`--pack` needs a pack folder")?
        } else {
            arg
        };
        let pack = if arg
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("pack.json"))
        {
            match arg.parent() {
                Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
                _ => PathBuf::from("."),
            }
        } else if named || arg.is_dir() {
            arg
        } else {
            launch.rom.get_or_insert(arg);
            continue;
        };
        if launch.pack.replace(pack).is_some() {
            return Err("more than one level pack".into());
        }
    }
    Ok(launch)
}

/// The levels of the pack in `dir`, imported as a user's against the ROM.
pub(crate) fn pack_levels(rom: &rotj_rom::Rom, dir: &Path) -> Result<Levels, String> {
    let files = levels::read(dir)?;
    let catalog = Catalog::from_rom(&rom.prg, &rom.chr).ok_or("the ROM's levels do not import")?;
    levels::import(&files, &catalog, Source::User).map_err(|error| error.to_string())
}

/// Prints `message` and exits 2; on Windows, where a dropped file's console closes at once,
/// after Enter.
fn exit_with(message: &str) -> ! {
    eprintln!("batman-rotj: {message}");
    if cfg!(windows) {
        eprintln!("Press Enter to close.");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
    std::process::exit(2);
}

/// The embedded ROM, else `arg`, `ROTJ_ROM` or a `.nes` beside the exe; without one, where
/// it looked, and exit 2.
fn rom_or_exit(arg: Option<&Path>) -> rotj_rom::Rom {
    if !PRG.is_empty() {
        return rotj_rom::Rom {
            prg: PRG.to_vec(),
            chr: CHR.to_vec(),
        };
    }
    rotj_rom::Rom::load(arg).unwrap_or_else(|error| {
        exit_with(&format!(
            "no usable ROM found.\n\
             Put Batman - Return of the Joker (USA).nes next to batman-rotj{}, pass it as the \
             first argument (drop it onto the exe), or set ROTJ_ROM.\n{error}",
            std::env::consts::EXE_SUFFIX
        ))
    })
}

/// The pack's levels, else the original game's; a pack that does not import exits 2,
/// naming its problem.
fn levels_or_exit(rom: &rotj_rom::Rom, pack: Option<&Path>) -> Levels {
    let Some(dir) = pack else {
        return Levels::original(&rom.prg, &rom.chr).expect("the ROM's levels import");
    };
    pack_levels(rom, dir).unwrap_or_else(|error| exit_with(&format!("{}: {error}", dir.display())))
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
    let launch = launch(std::env::args_os().skip(1).map(PathBuf::from))
        .unwrap_or_else(|error| exit_with(&error));
    let rom = rom_or_exit(launch.rom.as_deref());
    let levels = levels_or_exit(&rom, launch.pack.as_deref());
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
    .insert_resource(GameLevels(levels))
    .insert_resource(saved)
    .init_resource::<screen::Colours>()
    .insert_resource(settings::SettingsDir(dir))
    .init_state::<GameState>()
    .add_systems(Startup, (spawn_camera, write_tilesets))
    .add_systems(Update, fit_camera)
    .add_plugins((
        pacing::PacingPlugin,
        menu::MenuPlugin,
        ui::UiPlugin,
        audio::AudioPlugin,
        input::InputPlugin,
        play::PlayPlugin,
        mirror::MirrorPlugin,
        modern::ModernPlugin,
    ));

    // The display image is made before the app runs, so every system can rely on it.
    let screen = {
        let mut images = app.world_mut().resource_mut::<Assets<Image>>();
        screen::Screen::new(&mut images)
    };
    app.insert_resource(screen).run();
}

/// The camera that shows the screen quad in the window, the only one [`fit_camera`] fits.
#[derive(Component)]
struct ScreenCamera;

/// Writes the tilesets beside the settings file for level packs; a failure is logged and
/// the game goes on.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
fn write_tilesets(rom: Res<'_, RomData>, dir: Res<'_, settings::SettingsDir>) {
    let Some(dir) = &dir.0 else {
        return;
    };
    if let Err(error) = tilesets::write(dir, &rom.0.prg, &rom.0.chr) {
        warn!("{}: {error}", dir.join("tilesets").display());
    }
}

/// `Camera2d` over the 4x-scaled screen quad (1024x960 world units), sized to the
/// window by [`fit_camera`].
fn spawn_camera(mut commands: Commands<'_, '_>) {
    commands.spawn((
        ScreenCamera,
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
    mut cameras: Query<'_, '_, (&mut Projection, &mut Transform), With<ScreenCamera>>,
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
    use crate::play::tests::{on_title, step_until};
    use rotj_core::levels::model::{Map, Object, PackFile, Region, to_text};
    use rotj_core::levels::{ImportError, PackFiles};
    use std::collections::BTreeSet;

    /// A fresh folder under the temp dir, named for `name` and this process.
    fn temp_folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rotj-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temp folder");
        dir
    }

    #[test]
    fn the_arguments_name_a_rom_and_a_pack_once() {
        let dir = temp_folder("launch");
        let dir_text = dir.to_str().expect("a UTF-8 temp dir");
        let pack_json = dir.join("pack.json");
        let pack_json = pack_json.to_str().expect("UTF-8");
        let pack = |rom: Option<&str>, pack: &Path| Launch {
            rom: rom.map(PathBuf::from),
            pack: Some(pack.to_path_buf()),
        };
        let cases: [(&[&str], Result<Launch, String>); 12] = [
            (&["--pack", dir_text], Ok(pack(None, &dir))),
            (&["--pack", pack_json], Ok(pack(None, &dir))),
            (
                &["rotj.nes", "--pack", dir_text],
                Ok(pack(Some("rotj.nes"), &dir)),
            ),
            (&[dir_text], Ok(pack(None, &dir))),
            (&[pack_json], Ok(pack(None, &dir))),
            (&["PACK.JSON"], Ok(pack(None, Path::new(".")))),
            (&["rotj.nes", dir_text], Ok(pack(Some("rotj.nes"), &dir))),
            (&[pack_json, "rotj.nes"], Ok(pack(Some("rotj.nes"), &dir))),
            (
                &["rotj.nes"],
                Ok(Launch {
                    rom: Some("rotj.nes".into()),
                    pack: None,
                }),
            ),
            (&[], Ok(Launch::default())),
            (
                &[dir_text, pack_json],
                Err("more than one level pack".into()),
            ),
            (&["--pack"], Err("`--pack` needs a pack folder".into())),
        ];
        for (args, expected) in cases {
            assert_eq!(launch(args.iter().map(PathBuf::from)), expected, "{args:?}");
        }
        std::fs::remove_dir_all(&dir).expect("cleans up");
    }

    fn objects<'a>(map: &'a mut Map, name: &str) -> &'a mut Vec<Object> {
        let layer = map.layers.iter_mut().find(|layer| layer.name == name);
        layer
            .and_then(|layer| layer.objects.as_mut())
            .expect("an object layer")
    }

    /// Paints entry `to`'s 16x16 cells with entry `from`'s, both page-table entries (0-31).
    fn copy_region(map: &mut Map, from: u32, to: u32) {
        let vertical = map
            .properties
            .iter()
            .any(|property| property.name == "scroll" && property.value == "vertical");
        let width = usize::try_from(map.width).expect("a width");
        let cell = |entry: u32, cell: usize| {
            let (along, band) = (entry as usize & 15, entry as usize >> 4);
            let (x, y) = if vertical {
                (band, along)
            } else {
                (along, band)
            };
            (y * 16 + cell / 16) * width + x * 16 + cell % 16
        };
        let tiles = map.layers[0].data.as_mut().expect("terrain");
        for index in 0..256 {
            tiles[cell(to, index)] = tiles[cell(from, index)];
        }
    }

    /// The exported original made a user pack, with each object it drops and why.
    fn user_pack(rom: &rotj_rom::Rom, catalog: &Catalog) -> (PackFiles, Vec<(String, String)>) {
        let mut files = levels::export(&rom.prg, &rom.chr).expect("the ROM exports");
        let edit_map = |files: &mut PackFiles, path: &str, edit: &mut dyn FnMut(&mut Map)| {
            let mut map: Map = serde_json::from_str(&files[path]).expect("a room");
            edit(&mut map);
            files.insert(path.into(), to_text(&map));
        };
        let rooms: Vec<String> = files
            .keys()
            .filter(|path| path.starts_with("rooms/"))
            .cloned()
            .collect();
        for path in &rooms {
            edit_map(&mut files, path, &mut |map| {
                objects(map, "spawns").retain(|object| object.kind != "no_handler");
            });
        }
        let mut dropped = Vec::new();
        let mut remapped = BTreeSet::new();
        loop {
            let (file, reason) = match levels::import(&files, catalog, Source::User) {
                Ok(_) => return (files, dropped),
                Err(ImportError::Invalid { file, reason }) => (file, reason),
                Err(error) => panic!("{error}"),
            };
            let words: Vec<&str> = reason.split(' ').collect();
            if let [
                "layer",
                "`terrain`:",
                "entry",
                entry,
                "is",
                "page",
                page,
                ..,
            ] = words[..]
            {
                let entry: u32 = entry.parse().expect("an entry");
                let page: usize = page.trim_end_matches(',').parse().expect("a page");
                let mut pack: PackFile =
                    serde_json::from_str(&files["pack.json"]).expect("pack.json");
                let stage = pack
                    .stages
                    .iter_mut()
                    .find(|stage| stage.rooms.contains(&file))
                    .expect("the room's stage");
                let on = |page: &Vec<Region>, entry| {
                    page.iter()
                        .position(|region| region.room == file && region.entry == entry)
                };
                let at = on(&stage.pages[page], entry).expect("the region");
                let region = stage.pages[page].remove(at);
                let (written, from) = (0..page)
                    .find_map(|written| {
                        let held = stage.pages[written]
                            .iter()
                            .find(|region| region.room == file && region.entry < 32)?;
                        Some((written, held.entry))
                    })
                    .expect("an entry of the room on a written page");
                stage.pages[written].push(region);
                remapped.extend(stage.rooms.iter().cloned());
                edit_map(&mut files, &file, &mut |map| copy_region(map, from, entry));
                files.insert("pack.json".into(), to_text(&pack));
            } else if let [
                "layer",
                layer @ ("`spawns`" | "`markers`"),
                "object",
                id,
                ..,
            ] = words[..]
            {
                let id: u32 = id.trim_end_matches(':').parse().expect("an object id");
                if words[4] == "schedule" {
                    assert!(remapped.contains(&file), "{file}: {reason}");
                }
                let layer = layer.trim_matches('`');
                edit_map(&mut files, &file, &mut |map| {
                    objects(map, layer).retain(|object| object.id != id);
                });
                let stem = file.trim_start_matches("rooms/").trim_end_matches(".tmj");
                let marker = if layer == "markers" { "marker " } else { "" };
                dropped.push((format!("{stem} {marker}{id}"), words[4..].join(" ")));
            } else {
                panic!("{file}: {reason}");
            }
        }
    }

    /// The exported original, made a user pack, plays through `--pack` and a dropped
    /// `pack.json`: from each checkpoint's start, and stage 1-1 for a while.
    #[test]
    fn a_pack_plays_from_its_folder() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let catalog = Catalog::from_rom(&rom.prg, &rom.chr).expect("the ROM's catalog");
        let (files, dropped) = user_pack(rom, &catalog);
        let refused: Vec<&str> = dropped
            .iter()
            .filter(|(_, why)| !why.starts_with("schedule cell"))
            .map(|(object, _)| object.as_str())
            .collect();
        assert_eq!(
            refused,
            [
                "3-1-4 3", "3-1-4 11", "3-2-2 12", "3-3-1 3", "3-3-1 11", "5-1-4 1", "5-1-4 28",
                "6-3-1 3", "6-3-1 15"
            ],
            "the trailing exits"
        );
        assert!(
            dropped.iter().any(|(object, _)| object == "4-3-1 9"),
            "4-3-1's trailing exit collides on its remapped page"
        );
        let expected = levels::import(&files, &catalog, Source::User).expect("the user pack");
        assert_ne!(
            expected,
            Levels::original(&rom.prg, &rom.chr).expect("the original levels")
        );
        let dir = temp_folder("pack");
        levels::save(&dir, &files).expect("saves the pack");
        for args in [
            vec![PathBuf::from("--pack"), dir.clone()],
            vec![dir.join("pack.json")],
        ] {
            let launch = launch(args).expect("the arguments");
            let pack = launch.pack.expect("a pack");
            assert_eq!(pack_levels(rom, &pack).expect("the pack imports"), expected);
        }
        std::fs::remove_dir_all(&dir).expect("cleans up");
        let prg = || rotj_core::prg::Prg::new(rom.prg.clone(), expected.clone());
        let checkpoints = (0..)
            .take_while(|&level| rotj_core::actor::stage::level_area(&expected, level).is_some());
        for level in checkpoints {
            let forced = u8::try_from(level).ok().filter(|&level| level > 0);
            let mut play = play::Play::power_on(prg(), rom.chr.clone(), forced);
            assert!(step_until(&mut play, 400, 0, on_title), "{level}: no title");
            play.step(0x10);
            assert!(
                step_until(&mut play, 400, 0, |play| !play.booting()),
                "{level}"
            );
            assert_eq!(
                Some(play.game().world.area),
                rotj_core::actor::stage::level_area(&expected, level)
            );
        }
        let mut play = play::Play::start(prg(), rom.chr.clone(), 0);
        for _ in 0..300 {
            play.step(0x01);
        }
        assert_eq!(play.game().world.area, 0);
    }

    const FULL: Vec2 = Vec2::new(256.0, 240.0);
    const TV: Vec2 = Vec2::new(256.0, 224.0);

    #[test]
    fn fitting_the_window_leaves_the_modern_camera_alone() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.insert_resource(settings::Settings::default());
        world.insert_resource(UiScale(1.0));
        world.spawn((Window::default(), PrimaryWindow));
        let screen = world
            .spawn((
                ScreenCamera,
                Camera2d,
                Projection::Orthographic(OrthographicProjection::default_2d()),
            ))
            .id();
        let modern = world
            .spawn((
                Camera2d,
                Projection::Orthographic(OrthographicProjection::default_2d()),
                Transform::from_xyz(128.0, -120.0, 0.0),
            ))
            .id();
        world.run_system_once(fit_camera).expect("fit_camera runs");
        let scale = |entity| match world.get::<Projection>(entity) {
            Some(Projection::Orthographic(orthographic)) => orthographic.scale,
            _ => panic!("an orthographic camera"),
        };
        assert_ne!(scale(screen), 1.0, "the screen camera is fitted");
        assert_eq!(scale(modern), 1.0);
        assert_eq!(
            world.get::<Transform>(modern).map(|t| t.translation),
            Some(Vec3::new(128.0, -120.0, 0.0))
        );
    }

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
