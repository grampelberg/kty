use eyre::Result;
use ratatui::{layout::Rect, Frame};
use tracing::{metadata::LevelFilter, Level};

use super::{debug::Debug, pod, Widget};
use crate::events::{Broadcast, Event};

pub struct Apex {
    pods: pod::List,
    debug: Option<Debug>,
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
        }
    }
}

// TODO: figure out how to manage auth issues and whether to show/hide UI
// elements.
impl Widget for Apex {
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        self.pods.dispatch(event)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        if let Some(debug) = self.debug.as_mut() {
            debug.draw(frame, area);
        }

        self.pods.draw(frame, area);
    }
}
