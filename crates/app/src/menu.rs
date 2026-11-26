//! The settings menu: categorized Bevy UI over the frozen game, in the game's own font
//! (the 8x16 letters of CHR banks `$CC-$CF`, which the sound test and the pause load).

use std::time::Duration;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use rotj_core::Buttons;
use rotj_core::chr;
use rotj_core::palette::Rgb;

use crate::RomData;
use crate::input::NesInput;
use crate::pacing::Step;
use crate::play::Play;
use crate::screen::{Colours, PaletteStatus, pixels, visible};
use crate::settings::{
    CrtFilter, FrameRate, Overscan, PaletteChoice, Picture, Settings, SettingsDir, SfxVoices,
    UiStyle, save,
};

/// Which screen of the settings menu is active.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Page {
    #[default]
    Root,
    Display,
    Graphics,
    Audio,
    LoadPack,
}

impl Page {
    #[cfg(test)]
    const ALL: [Self; 5] = [
        Self::Root,
        Self::Display,
        Self::Graphics,
        Self::Audio,
        Self::LoadPack,
    ];

    const fn title(self) -> &'static str {
        match self {
            Self::Root => "SETTINGS",
            Self::Display => "DISPLAY",
            Self::Graphics => "GRAPHICS",
            Self::Audio => "AUDIO",
            Self::LoadPack => "LOAD PACK",
        }
    }

    const fn rows(self) -> &'static [Row] {
        match self {
            Self::Root => &[
                Row::Open(Self::Display),
                Row::Open(Self::Graphics),
                Row::Open(Self::Audio),
                Row::Open(Self::LoadPack),
                Row::Back,
            ],
            Self::Display => &[
                Row::Palette,
                Row::Overscan,
                Row::Crt,
                Row::Vsync,
                Row::FrameRate,
                Row::Back,
            ],
            Self::Graphics => &[
                Row::Picture,
                Row::Ui,
                Row::SpriteLimit,
                Row::IntroBlend,
                Row::Back,
            ],
            Self::Audio => &[Row::Volume, Row::SfxVoices, Row::Back],
            Self::LoadPack => &[Row::OriginalPack, Row::Back],
        }
    }
}

/// One selectable line inside a menu [`Page`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    Open(Page),
    Palette,
    Overscan,
    Crt,
    Vsync,
    FrameRate,
    Picture,
    Ui,
    SpriteLimit,
    IntroBlend,
    Volume,
    SfxVoices,
    OriginalPack,
    Back,
}

impl Row {
    /// The row's left-column label.
    const fn label(self) -> &'static str {
        match self {
            Self::Open(page) => page.title(),
            Self::Palette => "PALETTE",
            Self::Overscan => "OVERSCAN",
            Self::Crt => "CRT",
            Self::Vsync => "VSYNC",
            Self::FrameRate => "FRAME RATE",
            Self::Picture => "PICTURE",
            Self::Ui => "UI",
            Self::SpriteLimit => "SPRITE LIMIT",
            Self::IntroBlend => "INTRO BLEND",
            Self::Volume => "VOLUME",
            Self::SfxVoices => "SFX VOICES",
            Self::OriginalPack => "ORIGINAL",
            Self::Back => "BACK",
        }
    }

    /// The text the row shows for its setting.
    fn value(self, settings: &Settings, colours: &Colours) -> String {
        let on = |on: bool| if on { "ON" } else { "OFF" };
        match self {
            Self::Volume => settings.volume.to_string(),
            Self::Overscan => match settings.overscan {
                Overscan::Hidden => "HIDE",
                Overscan::Shown => "SHOW",
            }
            .into(),
            Self::Palette => match settings.palette {
                PaletteChoice::CompositeDirect => "COMPOSITE",
                PaletteChoice::RgbPpu => "RGB PPU",
                PaletteChoice::Ntsc => "NTSC",
                PaletteChoice::File if colours.status == PaletteStatus::FileMissing => "NO FILE",
                PaletteChoice::File => "FILE",
            }
            .into(),
            Self::Crt => match settings.crt {
                CrtFilter::Off => "OFF",
                CrtFilter::Scanlines => "SCANLINES",
                CrtFilter::Crt => "CRT",
            }
            .into(),
            Self::FrameRate => match settings.frame_rate {
                FrameRate::Classic => "60".into(),
                FrameRate::Fps(fps) => fps.to_string(),
                FrameRate::Display => "DISPLAY".into(),
            },
            Self::SpriteLimit => on(settings.sprite_limit).into(),
            Self::Vsync => on(settings.vsync).into(),
            Self::Ui => match settings.ui {
                UiStyle::Classic => "CLASSIC",
                UiStyle::Faithful => "FAITHFUL",
            }
            .into(),
            Self::Picture => match settings.picture {
                Picture::Classic => "CLASSIC",
                Picture::Modern => "MODERN",
            }
            .into(),
            Self::IntroBlend => on(settings.intro_blend).into(),
            Self::SfxVoices => match settings.sfx_voices {
                SfxVoices::Classic => "CLASSIC",
                SfxVoices::Unlimited => "UNLIMITED",
            }
            .into(),
            Self::Open(_) | Self::OriginalPack | Self::Back => String::new(),
        }
    }

    /// Changes the row's setting one step left (`-1`) or right (`1`).
    fn step(self, settings: &mut Settings, step: i8) {
        match self {
            Self::Volume => {
                settings.volume = if step < 0 {
                    settings.volume.saturating_sub(5)
                } else {
                    settings.volume.saturating_add(5).min(100)
                };
            }
            Self::Overscan => {
                settings.overscan = match settings.overscan {
                    Overscan::Hidden => Overscan::Shown,
                    Overscan::Shown => Overscan::Hidden,
                };
            }
            Self::Palette => {
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
            Self::Crt => {
                const ORDER: [CrtFilter; 3] =
                    [CrtFilter::Off, CrtFilter::Scanlines, CrtFilter::Crt];
                let at = ORDER.iter().position(|&c| c == settings.crt).unwrap_or(0);
                let next = if step < 0 {
                    at + ORDER.len() - 1
                } else {
                    at + 1
                };
                settings.crt = ORDER[next % ORDER.len()];
            }
            Self::FrameRate => settings.frame_rate = settings.frame_rate.step(step),
            Self::SpriteLimit => settings.sprite_limit = !settings.sprite_limit,
            Self::Vsync => settings.vsync = !settings.vsync,
            Self::Ui => {
                settings.ui = match settings.ui {
                    UiStyle::Classic => UiStyle::Faithful,
                    UiStyle::Faithful => UiStyle::Classic,
                };
            }
            Self::Picture => {
                settings.picture = match settings.picture {
                    Picture::Classic => Picture::Modern,
                    Picture::Modern => Picture::Classic,
                };
            }
            Self::IntroBlend => settings.intro_blend = !settings.intro_blend,
            Self::SfxVoices => {
                settings.sfx_voices = match settings.sfx_voices {
                    SfxVoices::Classic => SfxVoices::Unlimited,
                    SfxVoices::Unlimited => SfxVoices::Classic,
                };
            }
            Self::Open(_) | Self::OriginalPack | Self::Back => {}
        }
    }
}

/// The name of the chosen palette when it is a third party's, which its author asks a
/// selectable palette to show.
const fn credit(palette: PaletteChoice) -> &'static str {
    match palette {
        PaletteChoice::CompositeDirect => "COMPOSITE DIRECT FBX",
        PaletteChoice::RgbPpu | PaletteChoice::Ntsc | PaletteChoice::File => "",
    }
}

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
/// Footer notice shown when saving `settings.ron` failed.
const SAVE_FAILED: &str = "SAVE FAILED";
/// Footer notice on `DISPLAY` when high refresh is chosen with Classic picture.
const NEEDS_MODERN: &str = "NEEDS MODERN PICTURE";
/// Initial hold duration before directional repeat kicks in.
const REPEAT_DELAY: Duration = Duration::from_millis(330);
/// Interval between repeated steps while a direction stays held.
const REPEAT_INTERVAL: Duration = Duration::from_millis(65);
const _: () = assert!(
    HINT_ROW + 16 <= 8 + 224,
    "the hint shows with overscan hidden"
);
/// The atlas cell of a blank glyph, after the 64 pairs.
const BLANK: usize = 64;

/// Directional buttons eligible for held-key auto-repeat.
const DIRECTIONS: Buttons = Buttons::from_bits(
    Buttons::UP.bits() | Buttons::DOWN.bits() | Buttons::LEFT.bits() | Buttons::RIGHT.bits(),
);

/// Held-direction auto-repeat timer so sweeping `VOLUME` or `FRAME RATE` feels responsive.
#[derive(Debug, Default, Clone, Copy)]
struct RepeatState {
    held: Buttons,
    remaining: Duration,
}

impl RepeatState {
    fn reset(&mut self) {
        self.held = Buttons::NONE;
        self.remaining = Duration::ZERO;
    }

    /// Returns directional pulses to act on this frame: immediate edge presses plus
    /// periodic repeats while held.
    fn poll(&mut self, held: Buttons, pressed: Buttons, dt: Duration) -> Buttons {
        let dir_held = held & DIRECTIONS;
        let dir_pressed = pressed & DIRECTIONS;
        if dir_held == Buttons::NONE {
            self.reset();
            return dir_pressed;
        }
        if dir_pressed != Buttons::NONE || dir_held != self.held {
            self.held = dir_held;
            self.remaining = REPEAT_DELAY;
            return dir_pressed;
        }
        if dt >= self.remaining {
            self.remaining = REPEAT_INTERVAL;
            return dir_held;
        }
        self.remaining = self.remaining.saturating_sub(dt);
        Buttons::NONE
    }
}

/// Whether the settings menu is closed or open on a specific page and cursor index.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum MenuState {
    #[default]
    Closed,
    Open {
        page: Page,
        cursor: usize,
        root_cursor: usize,
    },
}

/// Which overlay is currently shown on top of the game picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum OverlayView {
    #[default]
    None,
    PauseHint,
    Menu,
}

/// Whether the menu is open, and its cursor.
#[derive(Resource, Default)]
pub(crate) struct Menu {
    state: MenuState,
    save_failed: bool,
    repeat: RepeatState,
}

impl Menu {
    /// True while the settings menu is open and the game is frozen beneath it.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        matches!(self.state, MenuState::Open { .. })
    }
}

/// The font's atlas.
#[derive(Resource)]
struct Font {
    image: Handle<Image>,
    layout: Handle<TextureAtlasLayout>,
}

/// The atlas cell (a top and bottom tile pair) of `c`, or `None` outside the game's font.
#[must_use]
pub(crate) fn glyph(c: char) -> Option<usize> {
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
pub(crate) fn font_atlas(chr: &[u8], rgb: &Rgb) -> Image {
    const WIDTH: usize = 64;
    const HEIGHT: usize = 9 * 16;
    let colours = [
        [0, 0, 0, 0],
        with_alpha(rgb[usize::from(rotj_core::palette::BLACK)]),
        with_alpha(rgb[usize::from(rotj_core::palette::WHITE)]),
        with_alpha(rgb[usize::from(rotj_core::palette::WHITE)]),
    ];
    let font = &chr[0xCC * 1024..0xD0 * 1024];
    let mut data = vec![0; WIDTH * HEIGHT * 4];
    let stride = WIDTH * 4;
    for pair in 0..64 {
        for (half, tile) in [2 * pair, 2 * pair + 1].into_iter().enumerate() {
            let bytes = &font[tile * 16..tile * 16 + 16];
            let (px, py) = ((pair % 8) * 8, (pair / 8) * 16 + half * 8);
            chr::blit_rgba(bytes, &mut data[py * stride + px * 4..], stride, colours);
        }
    }
    crate::screen::rgba_image(WIDTH, HEIGHT, data)
}

fn with_alpha([r, g, b]: [u8; 3]) -> [u8; 4] {
    [r, g, b, 0xFF]
}

fn close_and_save(
    menu: &mut Menu,
    input: &mut NesInput,
    settings: &Settings,
    dir: Option<&std::path::Path>,
) {
    menu.state = MenuState::Closed;
    menu.repeat.reset();
    input.hide_held();
    if let Some(dir) = dir
        && let Err(error) = save(dir, settings)
        && !menu.save_failed
    {
        warn!("settings not saved to {}: {error}", dir.display());
        menu.save_failed = true;
    }
}

/// Inputs read while steering the settings menu.
#[derive(SystemParam)]
pub(crate) struct SteerInput<'w, 's> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    gamepads: Query<'w, 's, &'static Gamepad>,
    time: Res<'w, Time>,
    nes: ResMut<'w, NesInput>,
}

/// Opens and drives the menu: Esc, F1, a pad's Guide or right stick anywhere, Select on
/// the game's pause; B steps back one level (closing from the root), Esc/F1 closes from any page.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
pub(crate) fn steer(
    mut inputs: SteerInput<'_, '_>,
    play: Res<'_, Play>,
    mut menu: ResMut<'_, Menu>,
    mut settings: ResMut<'_, Settings>,
    dir: Res<'_, SettingsDir>,
) {
    let toggle = inputs.keys.any_just_pressed([KeyCode::Escape, KeyCode::F1])
        || inputs.gamepads.iter().any(|pad| {
            pad.just_pressed(GamepadButton::Mode) || pad.just_pressed(GamepadButton::RightThumb)
        });
    let pressed = inputs.nes.pressed;
    let MenuState::Open {
        page,
        cursor,
        root_cursor,
    } = menu.state
    else {
        if toggle || play.paused() && pressed.contains(Buttons::SELECT) {
            menu.state = MenuState::Open {
                page: Page::Root,
                cursor: 0,
                root_cursor: 0,
            };
            menu.repeat.reset();
        }
        return;
    };
    if toggle {
        close_and_save(&mut menu, &mut inputs.nes, &settings, dir.0.as_deref());
        return;
    }
    let rows = page.rows();
    let row = rows[cursor.min(rows.len() - 1)];
    let confirm = pressed.intersects(Buttons::A | Buttons::START);
    if pressed.contains(Buttons::B) || (confirm && row == Row::Back) {
        if page == Page::Root {
            close_and_save(&mut menu, &mut inputs.nes, &settings, dir.0.as_deref());
        } else {
            menu.state = MenuState::Open {
                page: Page::Root,
                cursor: root_cursor,
                root_cursor,
            };
            menu.repeat.reset();
        }
        return;
    }
    if confirm && let Row::Open(target) = row {
        menu.state = MenuState::Open {
            page: target,
            cursor: 0,
            root_cursor: cursor,
        };
        menu.repeat.reset();
        return;
    }

    let dirs = menu
        .repeat
        .poll(inputs.nes.buttons, pressed, inputs.time.delta());
    let mut next_cursor = cursor;
    if dirs.contains(Buttons::UP) {
        next_cursor = (next_cursor + rows.len() - 1) % rows.len();
    }
    if dirs.contains(Buttons::DOWN) {
        next_cursor = (next_cursor + 1) % rows.len();
    }
    if next_cursor != cursor {
        menu.state = MenuState::Open {
            page,
            cursor: next_cursor,
            root_cursor: if page == Page::Root {
                next_cursor
            } else {
                root_cursor
            },
        };
    }
    let active_row = rows[next_cursor];
    if dirs.contains(Buttons::LEFT) {
        active_row.step(&mut settings, -1);
    }
    if dirs.contains(Buttons::RIGHT) {
        active_row.step(&mut settings, 1);
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
    play: Res<'w, Play>,
}

impl Shown<'_> {
    fn colour(&self, index: u8) -> Color {
        let [r, g, b] = rotj_core::palette::rgb(&self.colours.rgb, index);
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

/// Footer lines shown below `page`'s rows.
fn footer_lines(page: Page, settings: &Settings, save_failed: bool) -> Vec<&'static str> {
    let mut lines = Vec::new();
    if page == Page::Display {
        let palette_credit = credit(settings.palette);
        if !palette_credit.is_empty() {
            lines.push(palette_credit);
        }
        if !settings.frame_rate.is_classic() && settings.picture == Picture::Classic {
            lines.push(NEEDS_MODERN);
        }
    }
    if save_failed {
        lines.push(SAVE_FAILED);
    }
    lines
}

/// The menu's box: the page title, its rows (the cursor's row white), and any footer notices.
fn menu_box(
    picture: &mut ChildSpawnerCommands<'_>,
    font: &Font,
    shown: &Shown<'_>,
    page: Page,
    selected: usize,
) {
    let grey = shown.colour(rotj_core::palette::GREY);
    picture
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(PAD)),
                border: UiRect::all(Val::Px(BORDER)),
                ..default()
            },
            BackgroundColor(shown.colour(rotj_core::palette::BLACK)),
            BorderColor::all(shown.colour(rotj_core::palette::WHITE)),
        ))
        .with_children(|menu_box| {
            menu_box.spawn(line(
                font,
                &format!("{:^COLUMNS$}", page.title()),
                Color::WHITE,
            ));
            menu_box.spawn(line(font, "", Color::WHITE));
            for (index, &row) in page.rows().iter().enumerate() {
                let value = row.value(&shown.settings, &shown.colours);
                let tint = if index == selected {
                    Color::WHITE
                } else {
                    grey
                };
                menu_box.spawn(line(
                    font,
                    &format!("{:<VALUE_COLUMN$}{value}", row.label()),
                    tint,
                ));
            }
            let footers = footer_lines(page, &shown.settings, shown.menu.save_failed);
            if !footers.is_empty() {
                menu_box.spawn(line(font, "", Color::WHITE));
                for notice in footers {
                    menu_box.spawn(line(font, &format!("{notice:^COLUMNS$}"), grey));
                }
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
    crt: Option<Res<'_, crate::crt::CrtRig>>,
    overlays: Query<'_, '_, Entity, With<Overlay>>,
    mut last_view: Local<'_, OverlayView>,
) {
    let Some(font) = font else {
        return;
    };
    let view = match shown.menu.state {
        MenuState::Open { .. } => OverlayView::Menu,
        MenuState::Closed if shown.play.paused() => OverlayView::PauseHint,
        MenuState::Closed => OverlayView::None,
    };
    let changed = shown.menu.is_changed()
        || shown.settings.is_changed()
        || shown.colours.is_changed()
        || font.is_changed();
    if !changed && view == *last_view {
        return;
    }
    *last_view = view;
    for overlay in &overlays {
        commands.entity(overlay).despawn();
    }
    if view == OverlayView::None {
        return;
    }
    let area = visible(shown.settings.overscan);
    let mut root_entity = commands.spawn((
        Overlay,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
    ));
    if let Some(crt) = crt {
        root_entity.insert(UiTargetCamera(crt.menu_camera));
    }
    root_entity.with_children(|root| {
        root.spawn(Node {
            width: Val::Px(pixels(area.width)),
            height: Val::Px(pixels(area.height)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|picture| match shown.menu.state {
            MenuState::Closed => {
                picture.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(pixels(HINT_ROW - area.y)),
                        ..default()
                    },
                    children![line(&font, HINT, Color::WHITE)],
                ));
            }
            MenuState::Open { page, cursor, .. } => {
                menu_box(picture, &font, &shown, page, cursor);
            }
        });
    });
}

/// The settings menu.
pub(crate) struct MenuPlugin;

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

    const PALETTES: [PaletteChoice; 4] = [
        PaletteChoice::CompositeDirect,
        PaletteChoice::RgbPpu,
        PaletteChoice::Ntsc,
        PaletteChoice::File,
    ];

    const CRTS: [CrtFilter; 3] = [CrtFilter::Off, CrtFilter::Scanlines, CrtFilter::Crt];

    const RATES: [FrameRate; 3] = [FrameRate::Classic, FrameRate::Fps(170), FrameRate::Display];

    #[test]
    fn every_menu_character_has_a_glyph() {
        let mut texts: Vec<String> = Page::ALL
            .into_iter()
            .flat_map(|page| {
                std::iter::once(page.title().to_string())
                    .chain(page.rows().iter().map(|row| row.label().to_string()))
            })
            .collect();
        texts.extend([HINT.into(), SAVE_FAILED.into(), NEEDS_MODERN.into()]);
        texts.extend(PALETTES.map(|palette| credit(palette).into()));
        for palette in PALETTES {
            for status in [PaletteStatus::Loaded, PaletteStatus::FileMissing] {
                for crt in CRTS {
                    for frame_rate in RATES {
                        let settings = Settings {
                            palette,
                            crt,
                            frame_rate,
                            sprite_limit: false,
                            ui: UiStyle::Faithful,
                            picture: Picture::Modern,
                            sfx_voices: SfxVoices::Unlimited,
                            intro_blend: true,
                            ..Settings::default()
                        };
                        let colours = Colours {
                            status,
                            ..Colours::default()
                        };
                        for page in Page::ALL {
                            texts.extend(
                                page.rows().iter().map(|row| row.value(&settings, &colours)),
                            );
                        }
                    }
                }
            }
        }
        for text in &texts {
            assert!(text.chars().all(|c| glyph(c).is_some()), "{text:?}");
        }
    }

    #[test]
    fn each_setting_row_steps_alone() {
        let colours = Colours::default();
        let base = Settings {
            volume: 95,
            ..Settings::default()
        };
        let setting_rows: Vec<Row> = Page::ALL
            .into_iter()
            .flat_map(|page| page.rows().iter().copied())
            .filter(|row| !matches!(row, Row::Open(_) | Row::OriginalPack | Row::Back))
            .collect();
        for &row in &setting_rows {
            let mut stepped = base.clone();
            row.step(&mut stepped, 1);
            let changed: Vec<&str> = setting_rows
                .iter()
                .filter(|&&other| other.value(&base, &colours) != other.value(&stepped, &colours))
                .map(|r| r.label())
                .collect();
            assert_eq!(changed, [row.label()], "{row:?}");
        }
    }

    #[test]
    fn every_page_box_fits_the_smallest_view() {
        let width = pixels(COLUMNS * 8) + 2.0 * (PAD + BORDER);
        assert!(width <= 256.0, "width {width}");
        let worst_settings = Settings {
            palette: PaletteChoice::CompositeDirect,
            frame_rate: FrameRate::Display,
            picture: Picture::Classic,
            ..Settings::default()
        };
        for page in Page::ALL {
            let footers = footer_lines(page, &worst_settings, true);
            let footer_rows = if footers.is_empty() {
                0
            } else {
                1 + footers.len()
            };
            let total_rows = 2 + page.rows().len() + footer_rows;
            let height = pixels(total_rows * 16) + 2.0 * (PAD + BORDER);
            assert!(height <= 224.0, "{page:?}: {width}x{height}");
            for &row in page.rows() {
                assert!(row.label().len() <= VALUE_COLUMN, "{row:?}");
            }
        }
        assert!(PALETTES.iter().all(|&p| credit(p).len() <= COLUMNS));
        assert!(NEEDS_MODERN.len() <= COLUMNS);
    }

    #[test]
    fn only_composite_direct_is_credited_and_shown_on_display() {
        let credited: Vec<_> = PALETTES
            .into_iter()
            .filter(|&p| !credit(p).is_empty())
            .collect();
        assert_eq!(credited, [PaletteChoice::CompositeDirect]);
        let defaults = Settings::default();
        assert_eq!(
            footer_lines(Page::Display, &defaults, false),
            ["COMPOSITE DIRECT FBX"]
        );
        let high_classic = Settings {
            frame_rate: FrameRate::Display,
            picture: Picture::Classic,
            ..defaults
        };
        assert_eq!(
            footer_lines(Page::Display, &high_classic, false),
            ["COMPOSITE DIRECT FBX", NEEDS_MODERN]
        );
    }

    #[test]
    fn repeat_fires_on_edge_then_waits_delay_and_repeats() {
        let mut repeat = RepeatState::default();
        assert_eq!(
            repeat.poll(Buttons::RIGHT, Buttons::RIGHT, Duration::from_millis(16)),
            Buttons::RIGHT
        );
        assert_eq!(
            repeat.poll(Buttons::RIGHT, Buttons::NONE, Duration::from_millis(100)),
            Buttons::NONE
        );
        assert_eq!(
            repeat.poll(Buttons::RIGHT, Buttons::NONE, REPEAT_DELAY),
            Buttons::RIGHT
        );
        assert_eq!(
            repeat.poll(Buttons::RIGHT, Buttons::NONE, REPEAT_INTERVAL),
            Buttons::RIGHT
        );
        assert_eq!(
            repeat.poll(Buttons::NONE, Buttons::NONE, Duration::from_millis(16)),
            Buttons::NONE
        );
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
            .insert_resource(Play::start(
                crate::play::tests::prg(rom),
                rom.chr.clone(),
                0,
            ))
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
        app.world().resource::<Menu>().is_open()
    }

    fn frame(app: &App) -> u8 {
        app.world().resource::<Play>().session().frame()
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
    fn submenu_changes_setting_and_closing_saves() {
        let Some((mut app, dir)) = app("save") else {
            return;
        };
        tap(&mut app, KeyCode::Escape); // Root -> DISPLAY
        tap(&mut app, KeyCode::KeyX); // A enters DISPLAY (row 0 = PALETTE)
        tap(&mut app, KeyCode::ArrowDown); // row 1 = OVERSCAN
        tap(&mut app, KeyCode::ArrowRight);
        assert_eq!(app.world().resource::<Settings>().overscan, Overscan::Shown);
        assert!(!dir.join("settings.ron").exists(), "saved on close only");
        tap(&mut app, KeyCode::KeyZ); // B returns to Root
        assert!(open(&app), "B from submenu returns to Root");
        tap(&mut app, KeyCode::KeyZ); // B from Root closes & saves
        assert!(!open(&app));
        assert_eq!(
            crate::settings::load_with_warnings(&dir).0.overscan,
            Overscan::Shown
        );
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
        assert_eq!(
            app.world().resource::<NesInput>().game_buttons(),
            Buttons::NONE
        );
        release(&mut app, KeyCode::KeyZ);
        press(&mut app, KeyCode::KeyZ);
        assert_eq!(
            app.world().resource::<NesInput>().game_buttons(),
            Buttons::B
        );
    }

    #[test]
    fn volume_steps_by_five_and_stops_at_the_ends() {
        let mut settings = Settings {
            volume: 7,
            ..Settings::default()
        };
        Row::Volume.step(&mut settings, 1);
        assert_eq!(settings.volume, 12);
        settings.volume = 98;
        Row::Volume.step(&mut settings, 1);
        assert_eq!(settings.volume, 100);
        settings.volume = 3;
        Row::Volume.step(&mut settings, -1);
        assert_eq!(settings.volume, 0);
    }
}
