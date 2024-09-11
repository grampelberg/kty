use eyre::Result;
use tachyonfx::{fx, Effect, EffectTimer, Interpolation};
use tracing::{metadata::LevelFilter, Level};

use super::{debug::Debug, error::Error, pod, tunnel::Tunnel, Bundle, ResetEffect, Widget};
use crate::events::{Broadcast, Event};

pub struct Apex {
    effects: Vec<Effect>,
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

        Self {
            effects: vec![fx::coalesce(EffectTimer::from_ms(
                500,
                Interpolation::CubicOut,
            ))],
            widgets,
        }
    }
}

impl Bundle for Apex {
    fn show_all(&self) -> bool {
        true
    }

    fn effects(&mut self) -> &mut Vec<Effect> {
        &mut self.effects
    }

    fn widgets(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.widgets
    }
}

impl Widget for Apex {
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        if let Event::Tunnel(Err(err)) = event {
            self.widgets.push(Error::from(err.message()).boxed());
            self.effects.reset();
        }

        self.dispatch_children(event)
    }

    fn draw(&mut self, frame: &mut ratatui::Frame, area: ratatui::prelude::Rect) -> Result<()> {
        Bundle::draw(self, frame, area)
    }
}
