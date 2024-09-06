use eyre::Result;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    Frame,
};
use tracing::{metadata::LevelFilter, Level};

use super::{debug::Debug, error::Error, pod, propagate, tunnel::Tunnel, Widget};
use crate::events::{Broadcast, Event};

pub struct Apex {
    pods: pod::List,
    debug: Option<Debug>,
    error: Option<Error>,
    tunnel: Tunnel,
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
            tunnel: Tunnel::default(),
        }
    }

    #[allow(clippy::unnecessary_wraps)]
    fn dispatch_tunnel(&mut self, event: &Event) -> Result<Broadcast> {
        let Event::Tunnel(result) = event else {
            return Ok(Broadcast::Ignored);
        };

        propagate!(self.tunnel.dispatch(event));

        if let Err(err) = result {
            self.error = Some(Error::from(err.message()));

            return Ok(Broadcast::Consumed);
        }

        Ok(Broadcast::Ignored)
    }

    #[allow(clippy::unnecessary_wraps)]
    fn dispatch_error(&mut self, event: &Event) -> Result<Broadcast> {
        let Some(error) = self.error.as_mut() else {
            return Ok(Broadcast::Ignored);
        };

        propagate!(error.dispatch(event), self.error = None);

        Ok(Broadcast::Ignored)
    }
}

// TODO: figure out how to manage auth issues and whether to show/hide UI
// elements.
impl Widget for Apex {
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        propagate!(self.dispatch_tunnel(event));
        propagate!(self.dispatch_error(event));

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

        let [main, footer] = Layout::vertical([
            Constraint::Fill(0),
            Constraint::Length(self.tunnel.height()),
        ])
        .areas(area);

        self.pods.draw(frame, main)?;
        self.tunnel.draw(frame, footer)
    }
}
