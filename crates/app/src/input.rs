//! NES controller input.
//!
//! Keyboard and gamepad devices map directly into the game's [`Buttons`] bitset
//! (`$F6` order: Right in bit 0, A in bit 7).

use bevy::prelude::*;
use rotj_core::actor::button::Buttons;

/// Controller state for the current frame.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct NesInput {
    /// Buttons held right now.
    pub buttons: Buttons,
    /// Buttons that went down since the previous frame.
    pub pressed: Buttons,
    /// Buttons that were released since the previous frame.
    pub released: Buttons,
    /// Held buttons the game does not see until they are released.
    hidden: Buttons,
}

impl NesInput {
    /// Move to the next frame's state, deriving the edge bits.
    pub fn update(&mut self, buttons: Buttons) {
        self.pressed = buttons & !self.buttons;
        self.released = !buttons & self.buttons;
        self.buttons = buttons;
        self.hidden &= buttons;
    }

    /// Hides every button held now from the game until it is released.
    pub fn hide_held(&mut self) {
        self.hidden = self.buttons;
    }

    /// The pad the game reads: held buttons less the hidden ones.
    #[must_use]
    pub const fn game_buttons(self) -> Buttons {
        Buttons::from_bits(self.buttons.bits() & !self.hidden.bits())
    }
}

/// Keyboard bindings, NES order: A and B on the right hand, as on the pad.
const KEYMAP: [(Buttons, &[KeyCode]); 8] = [
    (Buttons::A, &[KeyCode::KeyX, KeyCode::KeyK]),
    (Buttons::B, &[KeyCode::KeyZ, KeyCode::KeyJ]),
    (Buttons::SELECT, &[KeyCode::ShiftLeft, KeyCode::ShiftRight]),
    (Buttons::START, &[KeyCode::Enter]),
    (Buttons::UP, &[KeyCode::ArrowUp, KeyCode::KeyW]),
    (Buttons::DOWN, &[KeyCode::ArrowDown, KeyCode::KeyS]),
    (Buttons::LEFT, &[KeyCode::ArrowLeft, KeyCode::KeyA]),
    (Buttons::RIGHT, &[KeyCode::ArrowRight, KeyCode::KeyD]),
];

/// Gamepad bindings in the usual "SNES-style" layout.
const PADMAP: [(Buttons, &[GamepadButton]); 8] = [
    (Buttons::A, &[GamepadButton::South]),
    (Buttons::B, &[GamepadButton::West, GamepadButton::East]),
    (Buttons::SELECT, &[GamepadButton::Select]),
    (Buttons::START, &[GamepadButton::Start]),
    (Buttons::UP, &[GamepadButton::DPadUp]),
    (Buttons::DOWN, &[GamepadButton::DPadDown]),
    (Buttons::LEFT, &[GamepadButton::DPadLeft]),
    (Buttons::RIGHT, &[GamepadButton::DPadRight]),
];

/// Keyboard contribution to the bitset.
#[must_use]
pub fn keyboard_buttons(keys: &ButtonInput<KeyCode>) -> Buttons {
    let mut buttons = Buttons::NONE;
    for (button, codes) in KEYMAP {
        if codes.iter().any(|key| keys.pressed(*key)) {
            buttons |= button;
        }
    }
    buttons
}

/// Gamepad contribution to the bitset, for one pad's `pressed` predicate.
#[must_use]
pub fn gamepad_buttons(pressed: impl Fn(GamepadButton) -> bool) -> Buttons {
    let mut buttons = Buttons::NONE;
    for (button, pads) in PADMAP {
        if pads.iter().any(|pad| pressed(*pad)) {
            buttons |= button;
        }
    }
    buttons
}

/// Merge every connected gamepad and the keyboard into one NES bitset.
#[must_use]
pub fn read_buttons(keys: &ButtonInput<KeyCode>, gamepads: &Query<'_, '_, &Gamepad>) -> Buttons {
    let mut buttons = keyboard_buttons(keys);
    for pad in gamepads.iter() {
        buttons |= gamepad_buttons(|button| pad.pressed(button));
    }
    buttons
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
        assert!(buttons.contains(Buttons::RIGHT));
        assert!(buttons.contains(Buttons::A));
        assert!(!buttons.contains(Buttons::B));

        let pad = gamepad_buttons(|b| b == GamepadButton::South);
        assert!(pad.contains(Buttons::A));
        assert!(!pad.contains(Buttons::B));
    }

    #[test]
    fn update_derives_edges() {
        let mut input = NesInput::default();
        input.update(Buttons::A | Buttons::START);
        assert!(input.pressed.contains(Buttons::A));
        assert!(!input.pressed.contains(Buttons::START | Buttons::B));

        input.update(Buttons::A);
        assert!(input.released.contains(Buttons::START));
        assert!(!input.pressed.contains(Buttons::A));
        assert!(input.buttons.contains(Buttons::A));
    }

    #[test]
    fn hidden_buttons_stay_hidden_until_released() {
        let mut input = NesInput::default();
        input.update(Buttons::A | Buttons::RIGHT);
        input.hide_held();
        input.update(Buttons::A | Buttons::RIGHT);
        assert_eq!(input.game_buttons(), Buttons::NONE);
        input.update(Buttons::RIGHT);
        assert_eq!(input.game_buttons(), Buttons::NONE);
        input.update(Buttons::A | Buttons::RIGHT);
        assert_eq!(input.game_buttons(), Buttons::A);
    }

    #[test]
    fn every_key_and_pad_maps_to_its_shift_reversed_game_byte() {
        let old_keys = [
            (KeyCode::KeyX, 0u8),
            (KeyCode::KeyK, 0),
            (KeyCode::KeyZ, 1),
            (KeyCode::KeyJ, 1),
            (KeyCode::ShiftLeft, 2),
            (KeyCode::ShiftRight, 2),
            (KeyCode::Enter, 3),
            (KeyCode::ArrowUp, 4),
            (KeyCode::KeyW, 4),
            (KeyCode::ArrowDown, 5),
            (KeyCode::KeyS, 5),
            (KeyCode::ArrowLeft, 6),
            (KeyCode::KeyA, 6),
            (KeyCode::ArrowRight, 7),
            (KeyCode::KeyD, 7),
        ];
        for (key, shift_bit) in old_keys {
            let mut keys = ButtonInput::<KeyCode>::default();
            keys.press(key);
            assert_eq!(
                keyboard_buttons(&keys).bits(),
                (1u8 << shift_bit).reverse_bits(),
                "{key:?}"
            );
        }
        let old_pads = [
            (GamepadButton::South, 0u8),
            (GamepadButton::West, 1),
            (GamepadButton::East, 1),
            (GamepadButton::Select, 2),
            (GamepadButton::Start, 3),
            (GamepadButton::DPadUp, 4),
            (GamepadButton::DPadDown, 5),
            (GamepadButton::DPadLeft, 6),
            (GamepadButton::DPadRight, 7),
        ];
        for (pad, shift_bit) in old_pads {
            assert_eq!(
                gamepad_buttons(|b| b == pad).bits(),
                (1u8 << shift_bit).reverse_bits(),
                "{pad:?}"
            );
        }
    }

    #[test]
    fn contains_needs_every_bit() {
        let buttons = Buttons::A | Buttons::B;
        assert!(buttons.contains(Buttons::A));
        assert!(!buttons.contains(Buttons::A | Buttons::START));
    }
}
