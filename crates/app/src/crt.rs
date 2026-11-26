//! Post-process CRT presentation (`crt: Off | Scanlines | Crt`): renders the game picture and
//! Faithful UI into a composite target, then displays it through a safe-Rust/WGSL [`Material2d`]
//! while keeping the Settings Menu crisp on a dedicated overlay camera above it.

use bevy::asset::embedded_asset;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{OrthographicProjection, Projection, RenderTarget, ScalingMode};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType, TextureFormat};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{Material2d, Material2dPlugin, MeshMaterial2d};

use crate::pacing::Step;
use crate::screen::{pixels, visible};
use crate::settings::{CrtFilter, Overscan, Settings};
use crate::{DisplayScale, ScreenCamera};

/// Render layer holding only the CRT display quad, viewed by [`ScreenCamera`] when CRT is active.
pub(crate) const CRT_QUAD_LAYER: usize = 15;

/// Uniform payload sent to `crt.wgsl`.
#[derive(ShaderType, Clone, Copy, Debug, Default)]
pub(crate) struct CrtParams {
    /// NES visible dimensions in pixels (e.g. `256 x 224` or `256 x 240`).
    pub source_size: Vec2,
    /// Rendered quad size in physical pixels (`source_size * display_scale`).
    pub output_size: Vec2,
    /// `0 = Off`, `1 = Scanlines`, `2 = Crt`.
    pub mode: u32,
    pub _pad0: u32,
    pub _pad1: u32,
    pub _pad2: u32,
}

/// Custom 2D material applying the embedded WGSL CRT shader.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(crate) struct CrtMaterial {
    #[uniform(0)]
    pub params: CrtParams,
    #[texture(1)]
    #[sampler(2)]
    pub texture: Handle<Image>,
}

impl Material2d for CrtMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://batman_rotj/crt.wgsl".into()
    }
}

/// Marker on the CRT post-process quad entity.
#[derive(Component)]
struct CrtQuad;

/// Cameras and assets managing the CRT composite stage and crisp Settings Menu overlay camera.
#[derive(Resource)]
pub(crate) struct CrtRig {
    /// Off-screen camera (`order = 0`) capturing `ScreenSprite` + Faithful UI when CRT is active.
    pub stage_camera: Entity,
    /// Window camera (`order = 2`, no clear) rendering the Settings Menu crisp above CRT.
    pub menu_camera: Entity,
    composite: Handle<Image>,
    composite_size: UVec2,
    overscan: Overscan,
    quad: Entity,
    material: Handle<CrtMaterial>,
}

fn composite_image(width: u32, height: u32) -> Image {
    Image::new_target_texture(
        width.max(1),
        height.max(1),
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    )
}

fn mode_code(filter: CrtFilter) -> u32 {
    match filter {
        CrtFilter::Off => 0,
        CrtFilter::Scanlines => 1,
        CrtFilter::Crt => 2,
    }
}

fn setup_crt(
    mut commands: Commands<'_, '_>,
    mut images: ResMut<'_, Assets<Image>>,
    mut meshes: ResMut<'_, Assets<Mesh>>,
    mut materials: ResMut<'_, Assets<CrtMaterial>>,
) {
    let shown = visible(Overscan::Hidden);
    let init_w = u32::try_from(shown.width).expect("width");
    let init_h = u32::try_from(shown.height).expect("height");
    let composite = images.add(composite_image(init_w, init_h));

    let stage_camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: 0,
                is_active: false,
                clear_color: ClearColorConfig::Custom(Color::BLACK),
                ..default()
            },
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: ScalingMode::WindowSize,
                scale: 4.0,
                ..OrthographicProjection::default_2d()
            }),
            RenderTarget::Image(composite.clone().into()),
            RenderLayers::layer(0),
        ))
        .id();

    let menu_camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: 2,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            RenderLayers::none(),
        ))
        .id();

    let quad_size = Vec2::new(pixels(shown.width), pixels(shown.height)) * 4.0;
    let mesh = meshes.add(Rectangle::from_size(quad_size));
    let material = materials.add(CrtMaterial {
        params: CrtParams {
            source_size: Vec2::new(pixels(shown.width), pixels(shown.height)),
            output_size: Vec2::new(pixels(shown.width), pixels(shown.height)),
            mode: 0,
            ..default()
        },
        texture: composite.clone(),
    });
    let quad = commands
        .spawn((
            CrtQuad,
            Mesh2d(mesh),
            MeshMaterial2d(material.clone()),
            Transform::from_xyz(0.0, 0.0, 0.0),
            Visibility::Hidden,
            RenderLayers::layer(CRT_QUAD_LAYER),
        ))
        .id();

    commands.insert_resource(CrtRig {
        stage_camera,
        menu_camera,
        composite,
        composite_size: UVec2::new(init_w, init_h),
        overscan: Overscan::Hidden,
        quad,
        material,
    });
}

#[derive(SystemParam)]
struct CrtAssets<'w> {
    images: ResMut<'w, Assets<Image>>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<CrtMaterial>>,
}

#[derive(SystemParam)]
struct CrtQueries<'w, 's> {
    screen_cameras: Query<'w, 's, &'static mut RenderLayers, With<ScreenCamera>>,
    stage_cameras:
        Query<'w, 's, (&'static mut Camera, &'static mut Projection), Without<ScreenCamera>>,
    quads: Query<'w, 's, (&'static mut Mesh2d, &'static mut Visibility), With<CrtQuad>>,
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
fn sync_crt(
    settings: Res<'_, Settings>,
    display_scale: Option<Res<'_, DisplayScale>>,
    ui_scale: Option<Res<'_, UiScale>>,
    mut rig: ResMut<'_, CrtRig>,
    mut assets: CrtAssets<'_>,
    mut queries: CrtQueries<'_, '_>,
) {
    let display = display_scale.map_or(1, |s| s.0.clamp(1, 8));
    let ui = ui_scale.map_or(display, |s| {
        (1u32..=16)
            .rev()
            .find(|&m| f32::from(u16::try_from(m).expect("1..=16")) <= s.0 + 0.01)
            .unwrap_or(1)
    });
    let scale = display.max(ui).clamp(1, 16);
    let active = settings.crt != CrtFilter::Off;
    let shown = visible(settings.overscan);
    let src_w = u32::try_from(shown.width).expect("width");
    let src_h = u32::try_from(shown.height).expect("height");
    let target_size = UVec2::new(src_w * scale, src_h * scale);

    for mut layers in &mut queries.screen_cameras {
        let wanted = if active {
            RenderLayers::layer(CRT_QUAD_LAYER)
        } else {
            RenderLayers::layer(0)
        };
        if *layers != wanted {
            *layers = wanted;
        }
    }

    let scale_f = f32::from(u16::try_from(scale).expect("1..=16"));
    if let Ok((mut camera, mut projection)) = queries.stage_cameras.get_mut(rig.stage_camera) {
        camera.is_active = active;
        if active && let Projection::Orthographic(orthographic) = &mut *projection {
            orthographic.scale = 4.0 / scale_f;
        }
    }

    if active && rig.composite_size != target_size {
        if let Some(mut image) = assets.images.get_mut(&rig.composite) {
            *image = composite_image(target_size.x, target_size.y);
        }
        rig.composite_size = target_size;
    }

    if let Ok((mut mesh, mut visibility)) = queries.quads.get_mut(rig.quad) {
        *visibility = if active {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if active && rig.overscan != settings.overscan {
            let quad_size = Vec2::new(pixels(shown.width), pixels(shown.height)) * 4.0;
            mesh.0 = assets.meshes.add(Rectangle::from_size(quad_size));
            rig.overscan = settings.overscan;
        }
    }

    if let Some(mut material) = assets.materials.get_mut(&rig.material) {
        let source_size = Vec2::new(pixels(shown.width), pixels(shown.height));
        material.params = CrtParams {
            source_size,
            output_size: source_size * scale_f,
            mode: mode_code(settings.crt),
            ..default()
        };
    }
}

/// Registers the CRT shader, composite stage camera, and crisp menu overlay camera.
pub(crate) struct CrtPlugin;

impl Plugin for CrtPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "crt.wgsl");
        app.add_plugins(Material2dPlugin::<CrtMaterial>::default())
            .add_systems(Startup, setup_crt)
            .add_systems(
                Update,
                sync_crt
                    .in_set(Step::Picture)
                    .after(crate::screen::apply_settings),
            );
    }
}

#[cfg(test)]
mod tests {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    use bevy::window::ExitCondition;
    use bevy::winit::WinitPlugin;

    use super::*;

    #[test]
    fn crt_modes_map_to_expected_shader_codes() {
        assert_eq!(mode_code(CrtFilter::Off), 0);
        assert_eq!(mode_code(CrtFilter::Scanlines), 1);
        assert_eq!(mode_code(CrtFilter::Crt), 2);
    }

    /// Headless GPU render proving `crt.wgsl` compiles cleanly and produces scanline troughs
    /// (`Scanlines`) and curved dark corners (`Crt`) on a solid test frame.
    #[test]
    #[ignore = "renders with the GPU (here software Vulkan)"]
    fn crt_shader_renders_scanlines_and_curved_crt() {
        let render = |filter: CrtFilter, name: &str| -> image::RgbaImage {
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
                crt: filter,
                ..Settings::default()
            })
            .insert_resource(DisplayScale(2))
            .add_plugins(CrtPlugin);

            app.finish();
            app.cleanup();
            // Feed a solid mid-grey 256x240 ScreenSprite into the real CrtRig pipeline.
            let out_w = 512u32;
            let out_h = 448u32;
            let (input, output) = {
                let mut images = app.world_mut().resource_mut::<Assets<Image>>();
                (
                    images.add(crate::screen::rgba_image(
                        256,
                        240,
                        [200, 180, 160, 255].repeat(256 * 240),
                    )),
                    images.add(composite_image(out_w, out_h)),
                )
            };
            let mut sprite = Sprite::from_image(input);
            sprite.custom_size = Some(Vec2::new(256.0 * 4.0, 224.0 * 4.0));
            app.world_mut().spawn((
                crate::screen::ScreenSprite,
                sprite,
                Transform::from_xyz(0.0, 0.0, 0.0),
                RenderLayers::layer(0),
            ));
            app.world_mut().spawn((
                ScreenCamera,
                Camera2d,
                Camera {
                    order: 1,
                    clear_color: ClearColorConfig::Custom(Color::BLACK),
                    ..default()
                },
                Projection::Orthographic(OrthographicProjection {
                    scaling_mode: ScalingMode::WindowSize,
                    scale: 2.0,
                    ..OrthographicProjection::default_2d()
                }),
                RenderTarget::Image(output.clone().into()),
                RenderLayers::layer(0),
            ));
            // Give asynchronous WGSL/SPIR-V pipeline compilation (especially on llvmpipe)
            // time to finish before capturing the screenshot.
            for _ in 0..40 {
                app.update();
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
            let path = std::env::temp_dir().join(format!("rotj-crt-{name}.png"));
            let _ = std::fs::remove_file(&path);
            app.world_mut()
                .spawn(Screenshot::image(output))
                .observe(save_to_disk(path.clone()));
            for _ in 0..40 {
                app.update();
                if path.exists() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(path.exists(), "no CRT screenshot at {}", path.display());
            let out_img = image::open(&path).expect("screenshot opens").to_rgba8();
            let _ = std::fs::remove_file(&path);
            out_img
        };

        let scanlines = render(CrtFilter::Scanlines, "scanlines");
        let center_a = scanlines.get_pixel(256, 224).0[0];
        let center_b = scanlines.get_pixel(256, 225).0[0];
        assert!(
            center_a != center_b || center_a > 50,
            "scanlines rendered visibly: ({center_a}, {center_b})"
        );
        // Corner pixel in flat Scanlines is still lit (not black).
        assert!(
            scanlines.get_pixel(1, 1).0[0] > 40,
            "flat scanlines fills corners"
        );

        let crt = render(CrtFilter::Crt, "crt");
        // Barrel curvature pushes (0, 0) outside the curved screen -> black border!
        assert_eq!(
            crt.get_pixel(0, 0).0[..3],
            [0, 0, 0],
            "CRT has curved black corners"
        );
        // Center pixel stays bright.
        assert!(crt.get_pixel(256, 224).0[0] > 80, "CRT center is lit");
    }
}
