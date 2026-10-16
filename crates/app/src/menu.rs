//! The settings menu: Bevy UI over the frozen game, in the game's own font (the 8x16
//! letters of CHR banks `$CC-$CF`, which the sound test and the pause load).

use bevy::asset::RenderAssetUsages;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use rotj_core::palette::Rgb;

use crate::RomData;
use crate::input::{NesButtons, NesInput};
use crate::pacing::Step;
use crate::play::Play;
use crate::screen::{Colours, pixels, visible_lines};
use crate::settings::{Overscan, PaletteChoice, Settings, SettingsDir, save};

/// The menu's lines, in order.
const ROWS: [&str; 6] = [
    "VOLUME",
    "OVERSCAN",
    "PALETTE",
    "SPRITE LIMIT",
    "VSYNC",
    "BACK",
];
/// Where the values start, in glyphs.
const VALUE_COLUMN: usize = 14;
/// The widest line, in glyphs.
const COLUMNS: usize = VALUE_COLUMN + 9;
/// The box's padding and border, in NES pixels.
const PAD: f32 = 8.0;
const BORDER: f32 = 2.0;
/// The hint's line on the game's pause, below the area name it draws at `$BF`.
const HINT_ROW: usize = 200;
const HINT: &str = "SELECT  SETTINGS";
const _: () = assert!(
    HINT_ROW + 16 <= 8 + 224,
    "the hint shows with overscan hidden"
);
/// The atlas cell of a blank glyph, after the 64 pairs.
const BLANK: usize = 64;

/// Whether the menu is open, and its cursor.
#[derive(Resource, Default)]
pub struct Menu {
    /// The game is frozen under the menu.
    pub open: bool,
    row: usize,
    save_failed: bool,
}

/// The font's atlas.
#[derive(Resource)]
struct Font {
    image: Handle<Image>,
    layout: Handle<TextureAtlasLayout>,
}

/// The atlas cell (a top and bottom tile pair) of `c`, or `None` outside the game's font.
#[must_use]
pub fn glyph(c: char) -> Option<usize> {
    let cell = |column: usize, row: usize| 8 * row + column;
    match c {
        ' ' => Some(BLANK),
        'A'..='Z' => {
            let at = usize::from(u8::try_from(c).ok()? - b'A') + 6;
            Some(cell(at / 4, at % 4))
        }
        '0'..='9' => {
            let at = usize::from(u8::try_from(c).ok()? - b'0');
            Some(cell(at / 4, 4 + at % 4))
        }
        '?' => Some(cell(0, 2)),
        '-' => Some(cell(0, 3)),
        '.' => Some(cell(1, 1)),
        _ => None,
    }
}

/// The font as an 8-wide grid of 8x16 glyphs, the last cell blank: pixel value 0
/// transparent, 1 `$0F`, 2 and 3 `$30`, as the sound test's palette `0F 0F 30 30` draws it.
#[must_use]
pub fn font_atlas(chr: &[u8], rgb: &Rgb) -> Image {
    const WIDTH: usize = 64;
    const HEIGHT: usize = 9 * 16;
    let colour = |value: u8| match value {
        0 => [0, 0, 0, 0],
        1 => with_alpha(rgb[0x0F]),
        _ => with_alpha(rgb[0x30]),
    };
    let font = &chr[0xCC * 1024..0xD0 * 1024];
    let mut data = vec![0; WIDTH * HEIGHT * 4];
    for pair in 0..64 {
        for (half, tile) in [2 * pair, 2 * pair + 1].into_iter().enumerate() {
            let bytes = &font[tile * 16..tile * 16 + 16];
            for y in 0..8 {
                for x in 0..8 {
                    let value = (bytes[y] >> (7 - x)) & 1 | ((bytes[y + 8] >> (7 - x)) & 1) << 1;
                    let (px, py) = ((pair % 8) * 8 + x, (pair / 8) * 16 + half * 8 + y);
                    let at = (py * WIDTH + px) * 4;
                    data[at..at + 4].copy_from_slice(&colour(value));
                }
            }
        }
    }
    Image::new(
        Extent3d {
            width: u32::try_from(WIDTH).expect("tiny"),
            height: u32::try_from(HEIGHT).expect("tiny"),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

fn with_alpha([r, g, b]: [u8; 3]) -> [u8; 4] {
    [r, g, b, 0xFF]
}

/// The text `row` shows for its setting.
fn value(settings: &Settings, colours: &Colours, row: usize) -> String {
    let on = |on: bool| if on { "ON" } else { "OFF" };
    match row {
        0 => settings.volume.to_string(),
        1 => match settings.overscan {
            Overscan::Hidden => "HIDE",
            Overscan::Shown => "SHOW",
        }
        .into(),
        2 => match settings.palette {
            PaletteChoice::CompositeDirect => "COMPOSITE",
            PaletteChoice::RgbPpu => "RGB PPU",
            PaletteChoice::Ntsc => "NTSC",
            PaletteChoice::File if colours.file_missing => "NO FILE",
            PaletteChoice::File => "FILE",
        }
        .into(),
        3 => on(settings.sprite_limit).into(),
        4 => on(settings.vsync).into(),
        _ => String::new(),
    }
}

/// Changes `row`'s setting one step left (`-1`) or right (`1`): volume by 5 within 0-100,
/// the others cycling.
fn change(settings: &mut Settings, row: usize, step: i8) {
    match row {
        0 => {
            settings.volume = if step < 0 {
                settings.volume.saturating_sub(5)
            } else {
                settings.volume.saturating_add(5).min(100)
            };
        }
        1 => {
            settings.overscan = match settings.overscan {
                Overscan::Hidden => Overscan::Shown,
                Overscan::Shown => Overscan::Hidden,
            };
        }
        2 => {
            const ORDER: [PaletteChoice; 4] = [
                PaletteChoice::CompositeDirect,
                PaletteChoice::RgbPpu,
                PaletteChoice::Ntsc,
                PaletteChoice::File,
            ];
            let at = ORDER
                .iter()
                .position(|&p| p == settings.palette)
                .unwrap_or(0);
            let next = if step < 0 {
                at + ORDER.len() - 1
            } else {
                at + 1
            };
            settings.palette = ORDER[next % ORDER.len()];
        }
        3 => settings.sprite_limit = !settings.sprite_limit,
        4 => settings.vsync = !settings.vsync,
        _ => {}
    }
}

/// Opens and drives the menu: Esc, F1, a pad's Guide or right stick anywhere, Select on
/// the game's pause; the same, B, or A or Start on BACK close it.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
pub fn steer(
    keys: Res<'_, ButtonInput<KeyCode>>,
    gamepads: Query<'_, '_, &Gamepad>,
    mut input: ResMut<'_, NesInput>,
    play: Option<Res<'_, Play>>,
    mut menu: ResMut<'_, Menu>,
    mut settings: ResMut<'_, Settings>,
    dir: Res<'_, SettingsDir>,
) {
    let toggle = keys.any_just_pressed([KeyCode::Escape, KeyCode::F1])
        || gamepads.iter().any(|pad| {
            pad.just_pressed(GamepadButton::Mode) || pad.just_pressed(GamepadButton::RightThumb)
        });
    let pressed = |button: u8| input.pressed.0 & button != 0;
    if !menu.open {
        let paused = play.is_some_and(|play| play.paused());
        if toggle || paused && pressed(NesButtons::SELECT) {
            menu.open = true;
            menu.row = 0;
        }
        return;
    }
    let back = menu.row == ROWS.len() - 1;
    if toggle || pressed(NesButtons::B) || back && pressed(NesButtons::A | NesButtons::START) {
        menu.open = false;
        input.hide_held();
        if let Some(dir) = &dir.0
            && let Err(error) = save(dir, &settings)
            && !menu.save_failed
        {
            warn!("settings not saved to {}: {error}", dir.display());
            menu.save_failed = true;
        }
        return;
    }
    if pressed(NesButtons::UP) {
        menu.row = (menu.row + ROWS.len() - 1) % ROWS.len();
    }
    if pressed(NesButtons::DOWN) {
        menu.row = (menu.row + 1) % ROWS.len();
    }
    if pressed(NesButtons::LEFT) {
        change(&mut settings, menu.row, -1);
    }
    if pressed(NesButtons::RIGHT) {
        change(&mut settings, menu.row, 1);
    }
}

/// The menu's and the hint's root, replaced whenever what they show changes.
#[derive(Component)]
struct Overlay;

#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
fn load_font(
    mut commands: Commands<'_, '_>,
    rom: Res<'_, RomData>,
    colours: Res<'_, Colours>,
    font: Option<ResMut<'_, Font>>,
    mut images: ResMut<'_, Assets<Image>>,
    mut layouts: ResMut<'_, Assets<TextureAtlasLayout>>,
) {
    let image = font_atlas(&rom.0.chr, &colours.rgb);
    if let Some(font) = font {
        images
            .insert(&font.image, image)
            .expect("the font's handle");
    } else {
        let layout = TextureAtlasLayout::from_grid(UVec2::new(8, 16), 8, 9, None, None);
        commands.insert_resource(Font {
            image: images.add(image),
            layout: layouts.add(layout),
        });
    }
}

/// What the overlay shows.
#[derive(SystemParam)]
struct Shown<'w> {
    menu: Res<'w, Menu>,
    settings: Res<'w, Settings>,
    colours: Res<'w, Colours>,
    play: Option<Res<'w, Play>>,
}

impl Shown<'_> {
    fn colour(&self, index: usize) -> Color {
        let [r, g, b] = self.colours.rgb[index];
        Color::srgb_u8(r, g, b)
    }
}

/// One line of `text` in the game's font, tinted.
fn line(font: &Font, text: &str, tint: Color) -> impl Bundle {
    let glyphs: Vec<_> = text
        .chars()
        .map(|c| {
            (
                Node {
                    width: Val::Px(8.0),
                    height: Val::Px(16.0),
                    ..default()
                },
                ImageNode::from_atlas_image(
                    font.image.clone(),
                    TextureAtlas {
                        layout: font.layout.clone(),
                        index: glyph(c).unwrap_or(BLANK),
                    },
                )
                .with_color(tint),
            )
        })
        .collect();
    (
        Node {
            flex_direction: FlexDirection::Row,
            height: Val::Px(16.0),
            ..default()
        },
        Children::spawn(SpawnIter(glyphs.into_iter())),
    )
}

/// The menu's box: the title, then each row with its value, the cursor's row white.
fn menu_box(picture: &mut ChildSpawnerCommands<'_>, font: &Font, shown: &Shown<'_>) {
    picture
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(PAD)),
                border: UiRect::all(Val::Px(BORDER)),
                ..default()
            },
            BackgroundColor(shown.colour(0x0F)),
            BorderColor::all(shown.colour(0x30)),
        ))
        .with_children(|menu_box| {
            menu_box.spawn(line(
                font,
                &format!("{:^COLUMNS$}", "SETTINGS"),
                Color::WHITE,
            ));
            menu_box.spawn(line(font, "", Color::WHITE));
            for (row, label) in ROWS.iter().enumerate() {
                let value = value(&shown.settings, &shown.colours, row);
                let tint = if row == shown.menu.row {
                    Color::WHITE
                } else {
                    shown.colour(0x10)
                };
                menu_box.spawn(line(font, &format!("{label:<VALUE_COLUMN$}{value}"), tint));
            }
        });
}

/// Replaces the overlay when what it shows changes: the menu, or the hint on the game's
/// pause, laid over the visible lines in NES pixels.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
fn draw(
    mut commands: Commands<'_, '_>,
    shown: Shown<'_>,
    font: Option<Res<'_, Font>>,
    overlays: Query<'_, '_, Entity, With<Overlay>>,
    mut hinted: Local<'_, bool>,
) {
    let Some(font) = font else {
        return;
    };
    let hint = !shown.menu.open && shown.play.as_ref().is_some_and(|play| play.paused());
    let changed = shown.menu.is_changed()
        || shown.settings.is_changed()
        || shown.colours.is_changed()
        || font.is_changed();
    if !changed && hint == *hinted {
        return;
    }
    *hinted = hint;
    for overlay in &overlays {
        commands.entity(overlay).despawn();
    }
    if !shown.menu.open && !hint {
        return;
    }
    let (first, count) = visible_lines(shown.settings.overscan);
    commands
        .spawn((
            Overlay,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|root| {
            root.spawn(Node {
                width: Val::Px(256.0),
                height: Val::Px(pixels(count)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|picture| {
                if hint {
                    picture.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            top: Val::Px(pixels(HINT_ROW - first)),
                            ..default()
                        },
                        children![line(&font, HINT, Color::WHITE)],
                    ));
                } else {
                    menu_box(picture, &font, &shown);
                }
            });
        });
}

/// The settings menu.
pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Menu>()
            .add_systems(Startup, load_font)
            .add_systems(
                Update,
                (
                    steer.before(Step::Game),
                    (load_font.run_if(resource_changed::<Colours>), draw)
                        .chain()
                        .in_set(Step::Picture),
                ),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sound_test_text_maps_to_its_tiles() {
        let tiles: Vec<usize> = "SOUND TEST"
            .chars()
            .map(|c| glyph(c).map_or(0, |cell| if cell == BLANK { 0 } else { 2 * cell }))
            .collect();
        assert_eq!(
            tiles,
            [0x0C, 0x0A, 0x2C, 0x38, 0x14, 0x00, 0x1C, 0x24, 0x0C, 0x1C]
        );
    }

    #[test]
    fn every_glyph_the_menu_draws_has_pixels() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let atlas = font_atlas(&rom.chr, &rotj_core::palette::NES_PALETTE);
        let data = atlas.data.expect("pixels");
        let lit = |cell: usize| {
            (0..16).any(|y| {
                (0..8).any(|x| {
                    let at = (((cell / 8) * 16 + y) * 64 + (cell % 8) * 8 + x) * 4;
                    data[at + 3] != 0
                })
            })
        };
        for c in ('A'..='Z').chain('0'..='9').chain(['?', '-', '.']) {
            assert!(lit(glyph(c).expect("in the font")), "{c:?} is blank");
        }
        assert!(!lit(BLANK));
    }

    #[test]
    fn every_menu_character_has_a_glyph() {
        let settings = Settings::default();
        let mut texts: Vec<String> = ROWS.iter().map(ToString::to_string).collect();
        texts.extend(["SETTINGS".into(), HINT.into()]);
        for palette in [
            PaletteChoice::CompositeDirect,
            PaletteChoice::RgbPpu,
            PaletteChoice::Ntsc,
            PaletteChoice::File,
        ] {
            for file_missing in [false, true] {
                let settings = Settings {
                    palette,
                    sprite_limit: false,
                    ..settings.clone()
                };
                let colours = Colours {
                    file_missing,
                    ..Colours::default()
                };
                texts.extend((0..ROWS.len()).map(|row| value(&settings, &colours, row)));
            }
        }
        for text in &texts {
            assert!(text.chars().all(|c| glyph(c).is_some()), "{text:?}");
        }
    }

    #[test]
    fn the_box_fits_the_smallest_view() {
        let width = pixels(COLUMNS * 8) + 2.0 * (PAD + BORDER);
        let height = pixels((ROWS.len() + 2) * 16) + 2.0 * (PAD + BORDER);
        assert!(width <= 256.0 && height <= 224.0, "{width}x{height}");
        let longest = ROWS
            .iter()
            .map(|label| VALUE_COLUMN.max(label.len()) + "COMPOSITE".len())
            .max();
        assert!(longest <= Some(COLUMNS));
    }

    /// The menu over the game at stage 1-1's start, keys pressed by hand, settings saved
    /// to a temporary directory of `name`; `None` without the ROM.
    fn app(name: &str) -> Option<(App, std::path::PathBuf)> {
        let rom = rotj_rom::for_test()?;
        let dir = std::env::temp_dir().join(format!("rotj-menu-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .init_state::<crate::GameState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_plugins(crate::input::InputPlugin)
            .init_resource::<Menu>()
            .init_resource::<Colours>()
            .insert_resource(Settings::default())
            .insert_resource(SettingsDir(Some(dir.clone())))
            .insert_resource(Play::start(rom.prg.clone(), rom.chr.clone(), 0))
            .configure_sets(Update, (Step::Game, Step::Music, Step::Picture).chain())
            .add_systems(
                Update,
                (
                    steer.before(Step::Game),
                    crate::play::tick
                        .in_set(Step::Game)
                        .run_if(crate::play::menu_closed),
                ),
            );
        app.update();
        Some((app, dir))
    }

    fn keys(app: &mut App) -> Mut<'_, ButtonInput<KeyCode>> {
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>()
    }

    /// One frame with `key` going down, held after.
    fn press(app: &mut App, key: KeyCode) {
        keys(app).press(key);
        app.update();
        keys(app).clear();
    }

    /// One frame with `key` going up.
    fn release(app: &mut App, key: KeyCode) {
        keys(app).release(key);
        app.update();
        keys(app).clear();
    }

    fn tap(app: &mut App, key: KeyCode) {
        press(app, key);
        release(app, key);
    }

    fn open(app: &App) -> bool {
        app.world().resource::<Menu>().open
    }

    fn frame(app: &App) -> u8 {
        app.world().resource::<Play>().game().world.frame
    }

    #[test]
    fn esc_opens_and_closes_once_per_press() {
        let Some((mut app, _)) = app("esc") else {
            return;
        };
        press(&mut app, KeyCode::Escape);
        for _ in 0..4 {
            app.update();
        }
        assert!(open(&app), "held, it stays open");
        release(&mut app, KeyCode::Escape);
        tap(&mut app, KeyCode::Escape);
        assert!(!open(&app));
        tap(&mut app, KeyCode::F1);
        assert!(open(&app));
    }

    #[test]
    fn the_game_does_not_step_while_open() {
        let Some((mut app, _)) = app("frozen") else {
            return;
        };
        tap(&mut app, KeyCode::Escape);
        let at_open = frame(&app);
        for _ in 0..30 {
            app.update();
        }
        assert_eq!(frame(&app), at_open);
        tap(&mut app, KeyCode::Escape);
        app.update();
        assert_ne!(frame(&app), at_open);
    }

    #[test]
    fn right_changes_the_setting_and_closing_saves() {
        let Some((mut app, dir)) = app("save") else {
            return;
        };
        tap(&mut app, KeyCode::Escape);
        tap(&mut app, KeyCode::ArrowDown);
        tap(&mut app, KeyCode::ArrowRight);
        assert_eq!(app.world().resource::<Settings>().overscan, Overscan::Shown);
        assert!(!dir.join("settings.ron").exists(), "saved on close only");
        tap(&mut app, KeyCode::Escape);
        assert_eq!(crate::settings::load(&dir).overscan, Overscan::Shown);
    }

    #[test]
    fn select_opens_it_only_in_the_games_pause() {
        let Some((mut app, _)) = app("select") else {
            return;
        };
        for _ in 0..30 {
            app.update();
        }
        tap(&mut app, KeyCode::ShiftLeft);
        assert!(!open(&app), "Select in play is the game's");
        tap(&mut app, KeyCode::Enter);
        for _ in 0..5 {
            app.update();
        }
        assert!(app.world().resource::<Play>().paused());
        tap(&mut app, KeyCode::ShiftLeft);
        assert!(open(&app));
    }

    #[test]
    fn the_button_that_closed_it_is_hidden() {
        let Some((mut app, _)) = app("hidden") else {
            return;
        };
        tap(&mut app, KeyCode::Escape);
        press(&mut app, KeyCode::KeyZ);
        assert!(!open(&app), "B closes it");
        app.update();
        assert_eq!(app.world().resource::<NesInput>().game_byte(), 0);
        release(&mut app, KeyCode::KeyZ);
        press(&mut app, KeyCode::KeyZ);
        assert_eq!(app.world().resource::<NesInput>().game_byte(), 0x40);
    }

    #[test]
    fn volume_steps_by_five_and_stops_at_the_ends() {
        let mut settings = Settings {
            volume: 7,
            ..Settings::default()
        };
        change(&mut settings, 0, 1);
        assert_eq!(settings.volume, 12);
        settings.volume = 98;
        change(&mut settings, 0, 1);
        assert_eq!(settings.volume, 100);
        settings.volume = 3;
        change(&mut settings, 0, -1);
        assert_eq!(settings.volume, 0);
    }
}
