use ratatui::layout::{Position, Rect};

use crate::events::Keypress;

pub enum Movement {
    X(i32),
    Y(i32),
}

impl Movement {
    pub fn saturating_adjust(&self, position: Position) -> Position {
        match self {
            Movement::X(x) => Position {
                x: position.x.saturating_add_signed(x.shrink()),
                y: position.y,
            },
            Movement::Y(y) => Position {
                x: position.x,
                y: position.y.saturating_add_signed(y.shrink()),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BigPosition {
    pub x: u32,
    pub y: u32,
}

impl From<Position> for BigPosition {
    fn from(pos: Position) -> Self {
        Self {
            x: u32::from(pos.x),
            y: u32::from(pos.y),
        }
    }
}

pub trait Shrink<T> {
    fn shrink(self) -> T;
}

#[allow(clippy::cast_possible_truncation)]
impl Shrink<isize> for i32 {
    fn shrink(self) -> isize {
        match isize::try_from(self) {
            Ok(val) => val,
            Err(_) => {
                if self < isize::MIN as i32 {
                    isize::MIN
                } else {
                    isize::MAX
                }
            }
        }
    }
}

impl Shrink<i16> for i32 {
    fn shrink(self) -> i16 {
        match i16::try_from(self) {
            Ok(val) => val,
            Err(_) => {
                if self < i16::MIN.into() {
                    i16::MIN
                } else {
                    i16::MAX
                }
            }
        }
    }
}

#[allow(clippy::cast_possible_truncation)]
impl Shrink<u16> for u32 {
    fn shrink(self) -> u16 {
        self as u16
    }
}

#[allow(clippy::cast_possible_truncation)]
impl Shrink<u32> for usize {
    fn shrink(self) -> u32 {
        self as u32
    }
}

impl Shrink<i32> for usize {
    fn shrink(self) -> i32 {
        match i32::try_from(self) {
            Ok(val) => val,
            Err(_) => {
                if self == 0 {
                    0
                } else {
                    i32::MAX
                }
            }
        }
    }
}

#[allow(clippy::cast_possible_truncation)]
impl Shrink<usize> for u32 {
    fn shrink(self) -> usize {
        self as usize
    }
}

impl Shrink<(u16, u16)> for BigPosition {
    fn shrink(self) -> (u16, u16) {
        (self.x.shrink(), self.y.shrink())
    }
}

// Retrieve a (x, y) tuple of how far to move the cursor. Takes the key pressed
// and the area to move within. Returns i16, so use saturating_add_signed() to
// avoid overflow.
//
// WARNING: This works with i16, if you're setting something to u16:MAX, going
// to the first element won't work - it'll take 2 times to get there.
#[allow(clippy::cast_possible_wrap)]
pub fn move_cursor(key: &Keypress, area: Rect) -> Option<Movement> {
    match key {
        Keypress::CursorLeft | Keypress::Printable('h') => Some(Movement::X(-1)),
        Keypress::CursorRight | Keypress::Printable('l') => Some(Movement::X(1)),
        Keypress::CursorUp | Keypress::Printable('k') => Some(Movement::Y(-1)),
        Keypress::CursorDown | Keypress::Printable('j') => Some(Movement::Y(1)),
        Keypress::Printable('H') => Some(Movement::Y(-i32::MAX)),
        Keypress::Printable('L') => Some(Movement::Y(i32::MAX)),
        Keypress::Printable('b') | Keypress::Control('b') => {
            Some(Movement::Y(-i32::from(area.height)))
        }
        Keypress::Printable(' ' | 'f') | Keypress::Control('f') => {
            Some(Movement::Y(i32::from(area.height)))
        }
        Keypress::Printable('^') | Keypress::Control('a') => Some(Movement::X(-i32::MAX)),
        Keypress::Printable('$') | Keypress::Control('e') => Some(Movement::X(i32::MAX)),
        _ => None,
    }
}

/// Add to match key {} to handle exiting the widget.
#[macro_export]
macro_rules! exit_keys {
    () => {
        Keypress::Escape | Keypress::Control('c' | 'd')
    };
}

pub use exit_keys;
