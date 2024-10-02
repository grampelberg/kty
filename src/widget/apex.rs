use std::{cell::RefCell, rc::Rc};

use eyre::Result;
use ratatui::{buffer::Buffer, layout::Rect, Frame};
use tachyonfx::{fx, EffectTimer, Interpolation};
use tracing::{metadata::LevelFilter, Level};

use super::{
    debug::Debug,
    error::Error,
    node, pod,
    tabs::TabbedView,
    tunnel::Tunnel,
    view::{Element, View},
    Widget,
};
use crate::{
    events::{Broadcast, Event},
    fx::Animated,
};

pub struct Apex {
    view: View,
    tunnel_idx: Rc<RefCell<u16>>,
}

impl Apex {
    pub fn new(client: kube::Client) -> Self {
        let tunnel_idx = Rc::new(RefCell::new(0));

        let tabs = TabbedView::builder()
            .tabs(vec![
                pod::List::tab("Pods".to_string(), client.clone(), true),
                node::List::tab("Nodes".to_string(), client, true),
            ])
            .build();

        let mut widgets = vec![
            Element::builder()
                .widget(
                    Animated::builder()
                        .widget(tabs.boxed())
                        .effect(fx::coalesce(EffectTimer::from_ms(
                            500,
                            Interpolation::CubicOut,
                        )))
                        .build()
                        .boxed(),
                )
                .terminal(true)
                .build(),
            Element::builder()
                .widget(Tunnel::new(tunnel_idx.clone()).boxed())
                .ignore(true)
                .build(),
        ];

        // TODO: This dependency on the crate is unfortunate, it should probably be
        // moved into something like `cata`. See `crate::cli::LEVEL` for an explanation
        // of why this is required instead of using `tracing::enabled!()`.
        if crate::cli::LEVEL.get().unwrap_or(&LevelFilter::ERROR) >= &Level::DEBUG {
            widgets.push(
                Element::builder()
                    .widget(Debug::default().boxed())
                    .ignore(true)
                    .build(),
            );
        }

        Self {
            view: View::builder().widgets(widgets).show_all(true).build(),
            tunnel_idx,
        }
    }
}

impl Widget for Apex {
    #[tracing::instrument(ret(level = Level::TRACE), skip(self, buffer, area), fields(name = self._name()))]
    fn dispatch(&mut self, event: &Event, buffer: &Buffer, area: Rect) -> Result<Broadcast> {
        if let Event::Tunnel(Err(err)) = event {
            self.view.push(Error::from(err.message()).boxed().into());
        }

        self.view.dispatch(event, buffer, area)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        *self.tunnel_idx.borrow_mut() = self.view.zindex();

        self.view.draw(frame, area)
    }
}
