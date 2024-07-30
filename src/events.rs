use std::str;

use eyre::{eyre, Result};
use tracing::info;

#[derive(Debug)]
enum Keypress {
    Null,
    StartOfHeader,
    StartOfText,
    EndOfText,
    EndOfTransmission,
    Enquiry,
    Acknowledge,
    Bell,
    Backspace,
    HorizontalTab,
    Linefeed,
    VerticalTab,
    Formfeed,
    CarriageReturn,
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
    Printable(String),

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
            b'\x02' => Ok(Keypress::StartOfText),
            b'\x03' => Ok(Keypress::EndOfText),
            b'\x04' => Ok(Keypress::EndOfTransmission),
            b'\x05' => Ok(Keypress::Enquiry),
            b'\x06' => Ok(Keypress::Acknowledge),
            b'\x07' => Ok(Keypress::Bell),
            b'\x08' => Ok(Keypress::Backspace),
            b'\x09' => Ok(Keypress::HorizontalTab),
            b'\x0A' => Ok(Keypress::Linefeed),
            b'\x0B' => Ok(Keypress::VerticalTab),
            b'\x0C' => Ok(Keypress::Formfeed),
            b'\x0D' => Ok(Keypress::CarriageReturn),
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
                str::from_utf8(self).unwrap().to_string(),
            )),
        }
    }
}

#[derive(Default)]
struct EventStream {}

impl EventStream {
    fn parse(data: &[u8]) -> Result<Option<Keypress>> {
        info!("data: {:?}", data.escape_ascii().to_string());

        if data.is_empty() {
            return Ok(None);
        }

        Ok(Some(data.try_into()?))
    }
}
