//! NES controller input.
//!
//! The port reads a button bitfield in the hardware's shift order, so gameplay
//! code written against the disassembly (`LDA $4016`-style logic) maps over
//! directly. Keyboard and gamepad are both merged into that one bitset.

use bevy::prelude::*;

/// The eight NES buttons as a bitset in the order the console shifts them out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NesButtons(pub u8);

impl NesButtons {
    /// `$4016` bit 0 / A.
    pub const A: u8 = 1 << 0;
    /// `$4016` bit 1 / B.
    pub const B: u8 = 1 << 1;
    /// Select.
    pub const SELECT: u8 = 1 << 2;
    /// Start.
    pub const START: u8 = 1 << 3;
    /// D-pad up.
    pub const UP: u8 = 1 << 4;
    /// D-pad down.
    pub const DOWN: u8 = 1 << 5;
    /// D-pad left.
    pub const LEFT: u8 = 1 << 6;
    /// D-pad right.
    pub const RIGHT: u8 = 1 << 7;

    /// Whether every button in the mask is held.
    #[must_use]
    #[cfg(test)]
    pub const fn contains(self, mask: u8) -> bool {
        self.0 & mask == mask
    }

    /// The byte the game's pad read (`$C1DB`) builds from them, which `Play::step`
    /// takes: the shift order reversed, Right in bit 0 and A in bit 7 (`$F6`).
    #[must_use]
    pub const fn game_byte(self) -> u8 {
        self.0.reverse_bits()
    }
}

/// Controller state for the current frame.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct NesInput {
    /// Buttons held right now.
    pub buttons: NesButtons,
    /// Buttons that went down since the previous frame.
    pub pressed: NesButtons,
    /// Buttons that were released since the previous frame.
    pub released: NesButtons,
    /// Held buttons the game does not see until they are released.
    hidden: u8,
}

impl NesInput {
    /// Move to the next frame's state, deriving the edge bits.
    pub fn update(&mut self, buttons: NesButtons) {
        self.pressed = NesButtons(buttons.0 & !self.buttons.0);
        self.released = NesButtons(!buttons.0 & self.buttons.0);
        self.buttons = buttons;
        self.hidden &= buttons.0;
    }

    /// Hides every button held now from the game until it is released.
    pub fn hide_held(&mut self) {
        self.hidden = self.buttons.0;
    }

    /// The pad the game reads: held buttons less the hidden ones, in its `$F6` order.
    #[must_use]
    pub const fn game_byte(self) -> u8 {
        NesButtons(self.buttons.0 & !self.hidden).game_byte()
    }
}

/// Keyboard bindings, NES order: A and B on the right hand, as on the pad.
const KEYMAP: [(u8, &[KeyCode]); 8] = [
    (NesButtons::A, &[KeyCode::KeyX, KeyCode::KeyK]),
    (NesButtons::B, &[KeyCode::KeyZ, KeyCode::KeyJ]),
    (
        NesButtons::SELECT,
        &[KeyCode::ShiftLeft, KeyCode::ShiftRight],
    ),
    (NesButtons::START, &[KeyCode::Enter]),
    (NesButtons::UP, &[KeyCode::ArrowUp, KeyCode::KeyW]),
    (NesButtons::DOWN, &[KeyCode::ArrowDown, KeyCode::KeyS]),
    (NesButtons::LEFT, &[KeyCode::ArrowLeft, KeyCode::KeyA]),
    (NesButtons::RIGHT, &[KeyCode::ArrowRight, KeyCode::KeyD]),
];

/// Gamepad bindings in the usual "SNES-style" layout.
const PADMAP: [(u8, &[GamepadButton]); 8] = [
    (NesButtons::A, &[GamepadButton::South]),
    (NesButtons::B, &[GamepadButton::West, GamepadButton::East]),
    (NesButtons::SELECT, &[GamepadButton::Select]),
    (NesButtons::START, &[GamepadButton::Start]),
    (NesButtons::UP, &[GamepadButton::DPadUp]),
    (NesButtons::DOWN, &[GamepadButton::DPadDown]),
    (NesButtons::LEFT, &[GamepadButton::DPadLeft]),
    (NesButtons::RIGHT, &[GamepadButton::DPadRight]),
];

/// Keyboard contribution to the bitset.
#[must_use]
pub fn keyboard_buttons(keys: &ButtonInput<KeyCode>) -> NesButtons {
    let mut bits = 0u8;
    for (bit, codes) in KEYMAP {
        if codes.iter().any(|key| keys.pressed(*key)) {
            bits |= bit;
        }
    }
    NesButtons(bits)
}

/// Gamepad contribution to the bitset, for one pad's `pressed` predicate.
#[must_use]
pub fn gamepad_buttons(pressed: impl Fn(GamepadButton) -> bool) -> NesButtons {
    let mut bits = 0u8;
    for (bit, buttons) in PADMAP {
        if buttons.iter().any(|button| pressed(*button)) {
            bits |= bit;
        }
    }
    NesButtons(bits)
}

/// Merge every connected gamepad and the keyboard into one NES bitset.
#[must_use]
pub fn read_buttons(keys: &ButtonInput<KeyCode>, gamepads: &Query<'_, '_, &Gamepad>) -> NesButtons {
    let mut bits = keyboard_buttons(keys).0;
    for pad in gamepads.iter() {
        bits |= gamepad_buttons(|button| pad.pressed(button)).0;
    }
    NesButtons(bits)
}

/// Input plugin: keeps [`NesInput`] current every frame.
pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NesInput>()
            .add_systems(PreUpdate, sample_input);
    }
}

/// Samples devices into [`NesInput`].
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
fn sample_input(
    keys: Res<'_, ButtonInput<KeyCode>>,
    gamepads: Query<'_, '_, &Gamepad>,
    mut input: ResMut<'_, NesInput>,
) {
    input.update(read_buttons(&keys, &gamepads));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_and_gamepad_bits_merge() {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::ArrowRight);
        keys.press(KeyCode::KeyX);
        let buttons = keyboard_buttons(&keys);
        assert!(buttons.contains(NesButtons::RIGHT));
        assert!(buttons.contains(NesButtons::A));
        assert!(!buttons.contains(NesButtons::B));

        let pad = gamepad_buttons(|b| b == GamepadButton::South);
        assert!(pad.contains(NesButtons::A));
        assert!(!pad.contains(NesButtons::B));
    }

    #[test]
    fn update_derives_edges() {
        let mut input = NesInput::default();
        input.update(NesButtons(NesButtons::A | NesButtons::START));
        assert!(input.pressed.contains(NesButtons::A));
        assert!(!input.pressed.contains(NesButtons::START | NesButtons::B));

        input.update(NesButtons(NesButtons::A));
        assert!(input.released.contains(NesButtons::START));
        assert!(!input.pressed.contains(NesButtons::A));
        assert!(input.buttons.contains(NesButtons::A));
    }

    #[test]
    fn hidden_buttons_stay_hidden_until_released() {
        let mut input = NesInput::default();
        input.update(NesButtons(NesButtons::A | NesButtons::RIGHT));
        input.hide_held();
        input.update(NesButtons(NesButtons::A | NesButtons::RIGHT));
        assert_eq!(input.game_byte(), 0);
        input.update(NesButtons(NesButtons::RIGHT));
        assert_eq!(input.game_byte(), 0);
        input.update(NesButtons(NesButtons::A | NesButtons::RIGHT));
        assert_eq!(input.game_byte(), 0x80);
    }

    #[test]
    fn the_game_byte_has_right_low_and_a_high() {
        assert_eq!(NesButtons(NesButtons::RIGHT).game_byte(), 0x01);
        assert_eq!(NesButtons(NesButtons::A).game_byte(), 0x80);
        assert_eq!(NesButtons(NesButtons::START).game_byte(), 0x10);
    }

    #[test]
    fn contains_needs_every_bit() {
        let buttons = NesButtons(NesButtons::A | NesButtons::B);
        assert!(buttons.contains(NesButtons::A));
        assert!(!buttons.contains(NesButtons::A | NesButtons::START));
    }
}
