use ratatui::layout::Rect;

use crate::events::Keypress;

pub enum Movement {
    X(i16),
    Y(i16),
}

// Retrieve a (x, y) tuple of how far to move the cursor. Takes the key pressed
// and the area to move within. Returns i16, so use saturating_add_signed() to
// avoid overflow.
#[allow(clippy::cast_possible_wrap)]
pub fn move_cursor(key: &Keypress, area: Rect) -> Option<Movement> {
    match key {
        Keypress::CursorLeft | Keypress::Printable('h') => Some(Movement::X(-1)),
        Keypress::CursorRight | Keypress::Printable('l') => Some(Movement::X(1)),
        Keypress::CursorUp | Keypress::Printable('k') => Some(Movement::Y(-1)),
        Keypress::CursorDown | Keypress::Printable('j') => Some(Movement::Y(1)),
        Keypress::Printable('H') => Some(Movement::Y(-i16::MAX)),
        Keypress::Printable('L') => Some(Movement::Y(i16::MAX)),
        Keypress::Printable(' ' | 'f') | Keypress::Control('f') => {
            Some(Movement::Y(area.height as i16))
        }
        Keypress::Printable('b') | Keypress::Control('b') => {
            Some(Movement::Y(-(area.height as i16)))
        }
        Keypress::Printable('^') | Keypress::Control('a') => Some(Movement::X(-i16::MAX)),
        Keypress::Printable('$') | Keypress::Control('e') => Some(Movement::X(i16::MAX)),
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
