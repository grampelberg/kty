use eyre::Result;
use tachyonfx::{fx, Effect, EffectTimer, Interpolation};
use tracing::{metadata::LevelFilter, Level};

use super::{
    debug::Debug, error::Error, pod, tunnel::Tunnel, Container, Contents, EffectExt, Mode,
    Renderable, StatefulWidget, Widget,
};
use crate::events::{Broadcast, Event};

pub struct Apex {
    effects: Vec<Effect>,
    widgets: Vec<Mode<()>>,

    // The debug widget is a little special as it is an overlay that doesn't cover up anything
    // else. Ideally, this would be managed by z-index and visibility stuff. That's not implemented
    // yet, so it sits here as a special case.
    debug: Option<Debug>,
}

impl Apex {
    pub fn new(client: kube::Client) -> Self {
        let widgets = vec![pod::List::new(client).mode(), Tunnel::default().mode()];
        let mut debug = None;

        // TODO: This dependency on the crate is unfortunate, it should probably be
        // moved into something like `cata`. See `crate::cli::LEVEL` for an explanation
        // of why this is required instead of using `tracing::enabled!()`.
        if crate::cli::LEVEL.get().unwrap_or(&LevelFilter::ERROR) >= &Level::DEBUG {
            debug = Some(Debug::default());
        }

        Self {
            effects: vec![fx::coalesce(EffectTimer::from_ms(
                500,
                Interpolation::CubicOut,
            ))],
            widgets,
            debug,
        }
    }
}

impl Widget for Apex {
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        self.dispatch_children(event, &mut ())
    }

    fn draw(&mut self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) -> Result<()> {
        if let Some(widget) = &mut self.debug {
            widget.draw(frame, area)?;
        }

        StatefulWidget::draw(self, frame, area, &mut ())
    }
}

impl Container for Apex {
    type State = ();

    fn dispatch(&mut self, event: &Event, _state: &mut Self::State) -> Result<Broadcast> {
        if let Event::Tunnel(Err(err)) = event {
            self.widgets.push(Error::from(err.message()).mode());
            self.effects.reset();
        }

        Ok(Broadcast::Ignored)
    }

    fn contents(&mut self) -> Contents<()> {
        Contents {
            effects: &mut self.effects,
            widgets: &mut self.widgets,
        }
    }
}

impl Renderable for Apex {}
