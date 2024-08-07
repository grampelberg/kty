pub mod pod;
pub mod yaml;

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

#[macro_export]
macro_rules! propagate {
    ($dispatch:expr, $event:expr) => {
        if let Some(obj) = $dispatch.as_mut() {
            match obj.dispatch($event)? {
                Broadcast::Consumed => return Ok(Broadcast::Consumed),
                Broadcast::Exited => {
                    $dispatch = None;

                    return Ok(Broadcast::Consumed);
                }
                Broadcast::Ignored => {}
            }
        }
    };
}

pub use propagate;
