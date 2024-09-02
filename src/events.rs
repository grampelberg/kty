use std::str;

use eyre::Result;
use ratatui::backend::WindowSize;
use tokio_util::bytes::Bytes;

use crate::widget::Raw;

#[derive(Debug)]
pub enum Broadcast {
    Consumed,
    Ignored,
    Exited,
    Raw(Box<dyn Raw>),
}

#[derive(Debug)]
pub enum Event {
    Input(Input),
    Resize(WindowSize),
    Goto(Vec<String>),
    Error(String),
    Shutdown,
    Render,
    Finished(Result<()>),
}

impl Event {
    pub fn key(&self) -> Option<&Keypress> {
        match self {
            Event::Input(Input { key, .. }) => Some(key),
            _ => None,
        }
    }
}

impl From<&[u8]> for Event {
    fn from(data: &[u8]) -> Event {
        Bytes::copy_from_slice(data).into()
    }
}

impl From<Bytes> for Event {
    fn from(data: Bytes) -> Event {
        Event::Input(Input {
            key: data.as_ref().into(),
            raw: data,
        })
    }
}

#[derive(Debug)]
pub struct Input {
    pub key: Keypress,
    raw: Bytes,
}

impl<'a> From<&'a Input> for &'a [u8] {
    fn from(input: &'a Input) -> &'a [u8] {
        input.raw.as_ref()
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

    Unknown(Bytes),
}

fn parse_escape(data: &[u8]) -> Keypress {
    if data.len() == 1 {
        return Keypress::Escape;
    }

    if data[1] != b'[' {
        return Keypress::Unknown(Bytes::copy_from_slice(data));
    }

    match data[2..] {
        [b'A'] => Keypress::CursorUp,
        [b'B'] => Keypress::CursorDown,
        [b'C'] => Keypress::CursorRight,
        [b'D'] => Keypress::CursorLeft,
        [b'H'] => Keypress::CursorHome,
        _ => Keypress::Unknown(Bytes::copy_from_slice(data)),
    }
}

impl From<&[u8]> for Keypress {
    fn from(data: &[u8]) -> Keypress {
        match data[0] {
            b'\x00' => Keypress::Null,
            b'\x01' => Keypress::StartOfHeader,
            b'\x02' => Keypress::Control('b'),
            // b'\x02' => Keypress::StartOfText,
            b'\x03' => Keypress::EndOfText,
            b'\x04' => Keypress::EndOfTransmission,
            b'\x05' => Keypress::Enquiry,
            b'\x06' => Keypress::Control('f'),
            // b'\x06' => Keypress::Acknowledge,
            b'\x07' => Keypress::Bell,
            b'\x08' => Keypress::Backspace,
            b'\x09' => Keypress::HorizontalTab,
            b'\x0A' | b'\x0D' => Keypress::Enter,
            b'\x0B' => Keypress::VerticalTab,
            b'\x0C' => Keypress::Formfeed,
            b'\x0E' => Keypress::ShiftOut,
            b'\x0F' => Keypress::ShiftIn,
            b'\x10' => Keypress::DLE,
            b'\x11' => Keypress::XON,
            b'\x12' => Keypress::DC2,
            b'\x13' => Keypress::XOFF,
            b'\x14' => Keypress::DC4,
            b'\x15' => Keypress::NAK,
            b'\x16' => Keypress::SYN,
            b'\x17' => Keypress::ETB,
            b'\x18' => Keypress::Cancel,
            b'\x19' => Keypress::EM,
            b'\x1A' => Keypress::Substitute,
            b'\x1b' => parse_escape(data),
            b'\x1C' => Keypress::FS,
            b'\x1D' => Keypress::GS,
            b'\x1E' => Keypress::RS,
            b'\x1F' => Keypress::US,
            b'\x7f' => Keypress::Delete,
            _ => Keypress::Printable(str::from_utf8(data).unwrap().chars().next().unwrap()),
        }
    }
}
