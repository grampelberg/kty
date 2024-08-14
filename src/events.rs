use std::str;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use eyre::{eyre, Result};
use ratatui::backend::WindowSize;

pub enum Broadcast {
    Consumed,
    Ignored,
    Exited,
}

#[derive(Debug, Clone)]
pub enum Event {
    Keypress(Keypress),
    Resize(WindowSize),
    Goto(Vec<String>),
    Shutdown,
    Render,
}

impl TryInto<Event> for &[u8] {
    type Error = eyre::Report;

    fn try_into(self) -> Result<Event> {
        Ok(Event::Keypress(self.try_into()?))
    }
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone)]
pub enum Keypress {
    Null,
    Control(char),

    // Most of these should map to ctrl+char but some of them are mapped to their own keys and I'm
    // not confident that I can distinguish between those use cases.
    StartOfHeader,
    StartOfText,
    EndOfText,
    EndOfTransmission,
    Enquiry,
    Acknowledge,
    Bell,
    Backspace,
    HorizontalTab,
    Enter, // Linefeed
    VerticalTab,
    Formfeed,
    // CarriageReturn, // Enter
    ShiftOut,
    ShiftIn,
    DLE,
    XON,
    DC2,
    XOFF,
    DC4,
    NAK,
    SYN,
    ETB,
    Cancel,
    EM,
    Substitute,
    Escape,
    FS,
    GS,
    RS,
    US,
    Delete,
    Printable(char),

    // Escape Sequences
    CursorUp,
    CursorDown,
    CursorRight,
    CursorLeft,
    CursorHome,
}

fn parse_escape(data: &[u8]) -> Result<Keypress> {
    if data.len() == 1 {
        return Ok(Keypress::Escape);
    }

    if data[1] != b'[' {
        return Err(eyre!("Unknown escape sequence"));
    }

    match data[2..] {
        [b'A'] => Ok(Keypress::CursorUp),
        [b'B'] => Ok(Keypress::CursorDown),
        [b'C'] => Ok(Keypress::CursorRight),
        [b'D'] => Ok(Keypress::CursorLeft),
        [b'H'] => Ok(Keypress::CursorHome),
        _ => Err(eyre!("Unknown escape sequence")),
    }
}

impl TryInto<Keypress> for &[u8] {
    type Error = eyre::Report;

    fn try_into(self) -> Result<Keypress> {
        if self.is_empty() {
            return Err(eyre!("Empty keypress"));
        }

        match self[0] {
            b'\x00' => Ok(Keypress::Null),
            b'\x01' => Ok(Keypress::StartOfHeader),
            b'\x02' => Ok(Keypress::Control('b')),
            // b'\x02' => Ok(Keypress::StartOfText),
            b'\x03' => Ok(Keypress::EndOfText),
            b'\x04' => Ok(Keypress::EndOfTransmission),
            b'\x05' => Ok(Keypress::Enquiry),
            b'\x06' => Ok(Keypress::Control('f')),
            // b'\x06' => Ok(Keypress::Acknowledge),
            b'\x07' => Ok(Keypress::Bell),
            b'\x08' => Ok(Keypress::Backspace),
            b'\x09' => Ok(Keypress::HorizontalTab),
            b'\x0A' | b'\x0D' => Ok(Keypress::Enter),
            b'\x0B' => Ok(Keypress::VerticalTab),
            b'\x0C' => Ok(Keypress::Formfeed),
            b'\x0E' => Ok(Keypress::ShiftOut),
            b'\x0F' => Ok(Keypress::ShiftIn),
            b'\x10' => Ok(Keypress::DLE),
            b'\x11' => Ok(Keypress::XON),
            b'\x12' => Ok(Keypress::DC2),
            b'\x13' => Ok(Keypress::XOFF),
            b'\x14' => Ok(Keypress::DC4),
            b'\x15' => Ok(Keypress::NAK),
            b'\x16' => Ok(Keypress::SYN),
            b'\x17' => Ok(Keypress::ETB),
            b'\x18' => Ok(Keypress::Cancel),
            b'\x19' => Ok(Keypress::EM),
            b'\x1A' => Ok(Keypress::Substitute),
            b'\x1b' => parse_escape(self),
            b'\x1C' => Ok(Keypress::FS),
            b'\x1D' => Ok(Keypress::GS),
            b'\x1E' => Ok(Keypress::RS),
            b'\x1F' => Ok(Keypress::US),
            b'\x7f' => Ok(Keypress::Delete),
            _ => Ok(Keypress::Printable(
                str::from_utf8(self).unwrap().chars().next().unwrap(),
            )),
        }
    }
}

impl TryInto<Keypress> for KeyEvent {
    type Error = eyre::Report;

    fn try_into(self) -> Result<Keypress> {
        match self {
            KeyEvent {
                code: KeyCode::Null,
                modifiers: _,
                kind: _,
                state: _,
            } => Ok(Keypress::Null),
            KeyEvent {
                code: KeyCode::Backspace,
                modifiers: _,
                kind: _,
                state: _,
            } => Ok(Keypress::Backspace),
            KeyEvent {
                code: KeyCode::Enter,
                modifiers: _,
                kind: _,
                state: _,
            } => Ok(Keypress::Enter),
            KeyEvent {
                code: KeyCode::Left,
                modifiers: _,
                kind: _,
                state: _,
            } => Ok(Keypress::CursorLeft),
            KeyEvent {
                code: KeyCode::Right,
                modifiers: _,
                kind: _,
                state: _,
            } => Ok(Keypress::CursorRight),
            KeyEvent {
                code: KeyCode::Up,
                modifiers: _,
                kind: _,
                state: _,
            } => Ok(Keypress::CursorUp),
            KeyEvent {
                code: KeyCode::Down,
                modifiers: _,
                kind: _,
                state: _,
            } => Ok(Keypress::CursorDown),
            KeyEvent {
                code: KeyCode::Home,
                modifiers: _,
                kind: _,
                state: _,
            } => Ok(Keypress::CursorHome),
            KeyEvent {
                code: KeyCode::Delete,
                modifiers: _,
                kind: _,
                state: _,
            } => Ok(Keypress::Delete),
            KeyEvent {
                code: KeyCode::Esc,
                modifiers: _,
                kind: _,
                state: _,
            } => Ok(Keypress::Escape),
            KeyEvent {
                code: KeyCode::Char('b'),
                modifiers: KeyModifiers::CONTROL,
                kind: _,
                state: _,
            } => Ok(Keypress::Control('b')),
            KeyEvent {
                code: KeyCode::Char('f'),
                modifiers: KeyModifiers::CONTROL,
                kind: _,
                state: _,
            } => Ok(Keypress::Control('f')),
            KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
                kind: _,
                state: _,
            } => Ok(Keypress::EndOfText),
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers: _,
                kind: _,
                state: _,
            } => Ok(Keypress::Printable(c)),
            _ => Err(eyre!("Unknown keypress")),
        }
    }
}
