use bon::Builder;
use eyre::Result;
use itertools::Itertools;
use ratatui::{
    buffer::Buffer,
    layout::{Layout, Rect},
    Frame,
};

use super::{propagate, BoxWidget, Widget};
use crate::events::{Broadcast, Event};

#[derive(Builder)]
pub struct View {
    #[builder(default)]
    widgets: Vec<BoxWidget>,

    #[builder(default)]
    show_all: bool,
}

impl View {
    pub fn push(&mut self, widget: BoxWidget) {
        self.widgets.push(widget);
    }

    pub fn pop(&mut self) -> Option<BoxWidget> {
        self.widgets.pop()
    }

    fn layers<'a>(
        widgets: impl Iterator<Item = &'a mut BoxWidget>,
        area: Rect,
    ) -> Vec<Vec<(Rect, &'a mut BoxWidget)>> {
        let chunks = widgets.chunk_by(|widget| widget.zindex());

        chunks
            .into_iter()
            .sorted_by(|(a, _), (b, _)| a.cmp(b))
            .map(|(_, layer)| {
                let layer: Vec<_> = layer.collect();

                let areas =
                    Layout::vertical(layer.iter().map(|widget| widget.placement().vertical))
                        .split(area);

                areas.iter().copied().zip(layer).collect()
            })
            .collect()
    }
}

impl Widget for View {
    fn dispatch(&mut self, event: &Event, buffer: &Buffer, area: Rect) -> Result<Broadcast> {
        for (i, widget) in self.widgets.iter_mut().enumerate().rev() {
            propagate!(widget.dispatch(event, buffer, area), {
                if i == 0 {
                    return Ok(Broadcast::Exited);
                }
                self.widgets.remove(i);
            });
        }

        Ok(Broadcast::Ignored)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let show_all = self.show_all;

        let mut layers = View::layers(self.widgets.iter_mut(), area);

        let mut layers: Box<dyn Iterator<Item = _>> = Box::new(layers.iter_mut());

        if !show_all {
            layers = Box::new(layers.tail(1));
        }

        for layer in layers {
            for (area, widget) in layer {
                widget.draw(frame, *area)?;
            }
        }

        Ok(())
    }
}
