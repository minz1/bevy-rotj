//! The pad's buttons in the order the game keeps them (`$F5`, `$F6`: its read at `$C1DB`
//! shifts them in from Right up to A).

use std::ops::{BitAnd, BitAndAssign, BitOr, Not};

/// A set of the pad's buttons, as the game's `$F5`/`$F6` byte holds them.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Buttons(u8);

impl Buttons {
    /// No button.
    pub const NONE: Buttons = Buttons(0);
    /// The D-pad's right.
    pub const RIGHT: Buttons = Buttons(0x01);
    /// The D-pad's left.
    pub const LEFT: Buttons = Buttons(0x02);
    /// The D-pad's down.
    pub const DOWN: Buttons = Buttons(0x04);
    /// The D-pad's up.
    pub const UP: Buttons = Buttons(0x08);
    /// Start.
    pub const START: Buttons = Buttons(0x10);
    /// Select.
    pub const SELECT: Buttons = Buttons(0x20);
    /// B: fire.
    pub const B: Buttons = Buttons(0x40);
    /// A: jump.
    pub const A: Buttons = Buttons(0x80);
    /// Left or right.
    pub const SIDEWAYS: Buttons = Buttons(0x03);
    /// Up or down.
    pub const UP_OR_DOWN: Buttons = Buttons(0x0C);
    /// Any direction.
    pub const DPAD: Buttons = Buttons(0x0F);
    /// A, B or Start: what confirms a choice or skips a screen.
    pub const ACCEPT: Buttons = Buttons(0xD0);
    /// A, B, Select or Start.
    pub const FACE: Buttons = Buttons(0xF0);

    /// The byte the game keeps.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// The buttons a game byte holds.
    #[must_use]
    pub const fn from_bits(bits: u8) -> Buttons {
        Buttons(bits)
    }

    /// Every button of `other` is held.
    #[must_use]
    pub const fn contains(self, other: Buttons) -> bool {
        self.0 & other.0 == other.0
    }

    /// Some button of `other` is held.
    #[must_use]
    pub const fn intersects(self, other: Buttons) -> bool {
        self.0 & other.0 != 0
    }
}

impl BitAnd for Buttons {
    type Output = Buttons;
    fn bitand(self, other: Buttons) -> Buttons {
        Buttons(self.0 & other.0)
    }
}

impl BitOr for Buttons {
    type Output = Buttons;
    fn bitor(self, other: Buttons) -> Buttons {
        Buttons(self.0 | other.0)
    }
}

impl Not for Buttons {
    type Output = Buttons;
    fn not(self) -> Buttons {
        Buttons(!self.0)
    }
}

impl BitAndAssign for Buttons {
    fn bitand_assign(&mut self, other: Buttons) {
        self.0 &= other.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_are_the_game_bits() {
        assert_eq!(
            [
                Buttons::RIGHT,
                Buttons::LEFT,
                Buttons::DOWN,
                Buttons::UP,
                Buttons::START,
                Buttons::SELECT,
                Buttons::B,
                Buttons::A
            ]
            .map(Buttons::bits),
            [1, 2, 4, 8, 0x10, 0x20, 0x40, 0x80]
        );
        assert_eq!(
            [
                Buttons::SIDEWAYS,
                Buttons::UP_OR_DOWN,
                Buttons::DPAD,
                Buttons::ACCEPT,
                Buttons::FACE
            ]
            .map(Buttons::bits),
            [0x03, 0x0C, 0x0F, 0xD0, 0xF0]
        );
        for byte in 0..=u8::MAX {
            let pad = Buttons::from_bits(byte);
            assert_eq!(
                !pad.intersects(Buttons::SIDEWAYS),
                byte.trailing_zeros() >= 2
            );
            assert_eq!(!pad.intersects(Buttons::DPAD), byte.trailing_zeros() >= 4);
            assert_eq!(!pad.intersects(Buttons::SIDEWAYS), byte.is_multiple_of(4));
        }
    }
}
