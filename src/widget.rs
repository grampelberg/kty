pub mod filter;
pub mod loading;
pub mod log;
pub mod pod;
pub mod table;
pub mod tabs;
pub mod yaml;

use eyre::Result;
use ratatui::{
    layout::{Constraint, Rect},
    widgets::Row,
    Frame,
};

use crate::{
    events::{Broadcast, Event},
    widget::table::RowStyle,
};

pub trait TableRow<'a> {
    fn constraints() -> Vec<Constraint>;

    fn row(&self, style: &RowStyle) -> Row;
    fn header() -> Row<'a>;
}

pub trait Widget: Send {
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast>;

    fn draw(&mut self, frame: &mut Frame, area: Rect);
}

#[macro_export]
macro_rules! propagate {
    ($fn:expr, $exit:expr) => {
        match $fn? {
            Broadcast::Consumed => return Ok(Broadcast::Consumed),
            Broadcast::Exited => {
                $exit;

                return Ok(Broadcast::Consumed);
            }
            Broadcast::Ignored => {}
        }
    };
}

pub use propagate;
