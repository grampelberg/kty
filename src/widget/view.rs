use bon::Builder;
use eyre::Result;
use itertools::Itertools;
use ratatui::{
    buffer::Buffer,
    layout::{Layout, Rect},
    Frame,
};
use tachyonfx::Effect;

use super::{propagate, BoxWidget, Placement, Widget};
use crate::{
    events::{Broadcast, Event},
    fx::Animated,
};

#[derive(Builder)]
pub struct Element {
    pub widget: BoxWidget,
    #[builder(default)]
    pub terminal: bool,

    // If this is set, the widget will not be used to calculate the zindex of the view. This allows
    // for things like debug and tunnel to float at the effective level of the view instead of
    // their own.
    #[builder(default)]
    pub ignore: bool,
    pub zindex: Option<u16>,
}

impl Element {
    pub fn animate(self, effect: Effect) -> Element {
        Self {
            widget: Animated::builder()
                .widget(self.widget)
                .effect(effect)
                .build()
                .boxed(),
            terminal: self.terminal,
            ignore: self.ignore,
            zindex: self.zindex,
        }
    }
}

impl Widget for Element {
    fn _name(&self) -> &'static str {
        self.widget._name()
    }

    fn dispatch(&mut self, event: &Event, buffer: &Buffer, area: Rect) -> Result<Broadcast> {
        self.widget.dispatch(event, buffer, area)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        self.widget.draw(frame, area)
    }

    fn placement(&self) -> Placement {
        self.widget.placement()
    }

    fn zindex(&self) -> u16 {
        self.zindex.unwrap_or(self.widget.zindex())
    }
}

impl From<BoxWidget> for Element {
    fn from(widget: BoxWidget) -> Self {
        Self {
            widget,
            terminal: false,
            ignore: false,
            zindex: None,
        }
    }
}

#[derive(Builder)]
pub struct View {
    #[builder(default)]
    widgets: Vec<Element>,

    #[builder(default)]
    show_all: bool,
}

impl View {
    pub fn push(&mut self, widget: Element) {
        self.widgets.push(widget);
    }

    pub fn pop(&mut self) -> Option<BoxWidget> {
        self.widgets.pop().map(|element| element.widget)
    }

    pub fn len(&self) -> usize {
        self.widgets.len()
    }

    fn layers(&mut self, area: Rect) -> Vec<Vec<(usize, Rect, &mut Element)>> {
        let show_all = self.show_all;

        // chunk_by only works with *consecutive* elements, so we need to first sort the
        // widgets.
        let chunks = self
            .widgets
            .iter_mut()
            .enumerate()
            .sorted_by(|(_, a), (_, b)| a.zindex().cmp(&b.zindex()))
            .chunk_by(|(_, widget)| widget.zindex());

        let layers = chunks.into_iter().map(|(_, layer)| {
            let layer: Vec<_> = layer.collect();

            let areas =
                Layout::vertical(layer.iter().map(|(_, widget)| widget.placement().vertical))
                    .split(area);

            areas
                .iter()
                .copied()
                .zip(layer)
                .map(|(area, (i, widget))| (i, area, widget))
                .collect()
        });

        if show_all {
            layers.collect()
        } else {
            layers.tail(1).collect()
        }
    }
}

impl Widget for View {
    #[tracing::instrument(ret(level = tracing::Level::TRACE), skip_all, fields(name = self._name()))]
    fn dispatch(&mut self, event: &Event, buffer: &Buffer, area: Rect) -> Result<Broadcast> {
        let mut layers = self.layers(area);
        layers.reverse();

        for mut layer in layers {
            layer.reverse();

            for (idx, area, el) in layer {
                tracing::trace!(name = el._name(), "dispatching event");
                propagate!(el.dispatch(event, buffer, area), {
                    if el.terminal {
                        return Ok(Broadcast::Exited);
                    }

                    self.widgets.remove(idx);
                });
            }
        }

        Ok(Broadcast::Ignored)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        for layer in self.layers(area) {
            for (_, area, widget) in layer {
                widget.draw(frame, area)?;
            }
        }

        Ok(())
    }

    fn zindex(&self) -> u16 {
        self.widgets
            .iter()
            .filter_map(|w| if w.ignore { None } else { Some(w.zindex()) })
            .max()
            .unwrap_or_default()
    }
}
