use eyre::Result;
use ratatui::{layout::Rect, Frame};
use tracing::{metadata::LevelFilter, Level};

use super::{debug::Debug, error::Error, pod, propagate, Widget};
use crate::events::{Broadcast, Event};

pub struct Apex {
    pods: pod::List,
    debug: Option<Debug>,
    error: Option<Error>,
}

impl Apex {
    pub fn new(client: kube::Client) -> Self {
        let mut debug = None;

        // TODO: This dependency on the crate is unfortunate, it should probably be
        // moved into something like `cata`. See `crate::cli::LEVEL` for an explanation
        // of why this is required instead of using `tracing::enabled!()`.
        if crate::cli::LEVEL.get().unwrap_or(&LevelFilter::ERROR) >= &Level::DEBUG {
            debug = Some(Debug::default());
        }

        Self {
            pods: pod::List::new(client),
            debug,
            error: None,
        }
    }
}

// TODO: figure out how to manage auth issues and whether to show/hide UI
// elements.
impl Widget for Apex {
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        if let Event::Error(err) = event {
            self.error = Some(Error::from(err.clone()));

            return Ok(Broadcast::Consumed);
        }

        if let Some(error) = self.error.as_mut() {
            propagate!(error.dispatch(event), self.error = None);
        }

        self.pods.dispatch(event)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        if let Some(debug) = self.debug.as_mut() {
            debug.draw(frame, area)?;
        }

        if let Some(error) = self.error.as_mut() {
            error.draw(frame, area)?;

            return Ok(());
        }

        self.pods.draw(frame, area)
    }
}
