use eyre::Result;
use ratatui::{buffer::Buffer, layout::Rect, Frame};
use tachyonfx::{fx, EffectTimer, Interpolation};
use tracing::{metadata::LevelFilter, Level};

use super::{debug::Debug, error::Error, pod, tunnel::Tunnel, view::View, Widget};
use crate::{
    events::{Broadcast, Event},
    fx::Animated,
};

pub struct Apex {
    view: View,
}

impl Apex {
    pub fn new(client: kube::Client) -> Self {
        let mut widgets = vec![
            Animated::builder()
                .widget(pod::List::new(client).boxed())
                .effect(fx::coalesce(EffectTimer::from_ms(
                    500,
                    Interpolation::CubicOut,
                )))
                .build()
                .boxed(),
            Tunnel::default().boxed(),
        ];

        // TODO: This dependency on the crate is unfortunate, it should probably be
        // moved into something like `cata`. See `crate::cli::LEVEL` for an explanation
        // of why this is required instead of using `tracing::enabled!()`.
        if crate::cli::LEVEL.get().unwrap_or(&LevelFilter::ERROR) >= &Level::DEBUG {
            widgets.push(Debug::default().boxed());
        }

        Self {
            view: View::builder().widgets(widgets).show_all(true).build(),
        }
    }
}

impl Widget for Apex {
    fn dispatch(&mut self, event: &Event, buffer: &Buffer, area: Rect) -> Result<Broadcast> {
        if let Event::Tunnel(Err(err)) = event {
            self.view.push(Error::from(err.message()).boxed());
        }

        self.view.dispatch(event, buffer, area)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        self.view.draw(frame, area)
    }
}
