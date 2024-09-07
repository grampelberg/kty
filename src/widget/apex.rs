use eyre::Result;
use tracing::{metadata::LevelFilter, Level};

use super::{debug::Debug, error::Error, pod, tunnel::Tunnel, Container, Widget};
use crate::events::{Broadcast, Event};

pub struct Apex {
    widgets: Vec<Box<dyn Widget>>,
}

impl Apex {
    pub fn new(client: kube::Client) -> Self {
        let mut widgets = vec![pod::List::new(client).boxed(), Tunnel::default().boxed()];

        // TODO: This dependency on the crate is unfortunate, it should probably be
        // moved into something like `cata`. See `crate::cli::LEVEL` for an explanation
        // of why this is required instead of using `tracing::enabled!()`.
        if crate::cli::LEVEL.get().unwrap_or(&LevelFilter::ERROR) >= &Level::DEBUG {
            widgets.push(Debug::default().boxed());
        }

        Self { widgets }
    }
}

impl Container for Apex {
    fn widgets(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.widgets
    }

    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        if let Event::Tunnel(Err(err)) = event {
            self.widgets.push(Error::from(err.message()).boxed());

            return Ok(Broadcast::Consumed);
        }

        Ok(Broadcast::Ignored)
    }
}
