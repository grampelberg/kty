pub mod pod;

use eyre::Result;
use ratatui::{
    layout::{Constraint, Rect},
    widgets::Row,
    Frame,
};

use crate::events::{Broadcast, Event};

pub trait TableRow<'a> {
    fn constraints() -> Vec<Constraint>;

    fn row(&self) -> Row;
    fn header() -> Row<'a>;
}

pub trait Dispatch {
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast>;
}

pub trait Screen {
    fn draw(&mut self, frame: &mut Frame, area: Rect);
}
