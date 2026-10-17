//! The pad's buttons in the order the game keeps them (`$F5`, `$F6`: its read at `$C1DB`
//! shifts them in from Right up to A).

/// The D-pad's right.
pub const RIGHT: u8 = 0x01;
/// The D-pad's left.
pub const LEFT: u8 = 0x02;
/// The D-pad's down.
pub const DOWN: u8 = 0x04;
/// The D-pad's up.
pub const UP: u8 = 0x08;
/// Start.
pub const START: u8 = 0x10;
/// Select.
pub const SELECT: u8 = 0x20;
/// B: fire.
pub const B: u8 = 0x40;
/// A: jump.
pub const A: u8 = 0x80;
/// Left or right.
pub const SIDEWAYS: u8 = LEFT | RIGHT;
/// Up or down.
pub const UP_OR_DOWN: u8 = UP | DOWN;
/// A, B or Start: what confirms a choice or skips a screen.
pub const ACCEPT: u8 = A | B | START;
/// A, B, Select or Start.
pub const FACE: u8 = A | B | SELECT | START;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_buttons_are_the_games_order() {
        assert_eq!(
            [RIGHT, LEFT, DOWN, UP, START, SELECT, B, A],
            [1, 2, 4, 8, 0x10, 0x20, 0x40, 0x80]
        );
        assert_eq!(
            [SIDEWAYS, UP_OR_DOWN, ACCEPT, FACE],
            [0x03, 0x0C, 0xD0, 0xF0]
        );
    }
}
