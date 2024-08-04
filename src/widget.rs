pub mod pod;

use ratatui::{layout::Constraint, widgets::Row};

use crate::events::Event;

pub trait TableRow<'a> {
    fn constraints() -> Vec<Constraint>;

    fn row(&self) -> Row;
    fn header() -> Row<'a>;
}

pub trait Dispatch {
    fn dispatch(&mut self, event: Event);
}
