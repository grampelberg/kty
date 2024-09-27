use bon::Builder;
use eyre::Result;
use ratatui::{
    layout::{Margin, Offset, Rect},
    text::Text,
    widgets::{Block, Clear, Scrollbar, ScrollbarState},
    Frame,
};

use super::{
    nav::{BigPosition, Shrink},
    Widget,
};

#[derive(Builder)]
pub struct Viewport<'a> {
    block: Option<Block<'a>>,
    buffer: &'a Vec<Text<'a>>,
    #[builder(default)]
    view: BigPosition,
}

impl Viewport<'_> {
    fn content(&mut self, frame: &mut Frame, area: Rect) {
        let y: usize = self.view.y.shrink();

        let start = y.clamp(0, self.buffer.len().saturating_sub(area.height.into()));
        let end = y
            .saturating_add(area.height.into())
            .clamp(0, self.buffer.len());

        for (i, line) in self.buffer[start..end].iter().enumerate() {
            frame.render_widget(
                line,
                area.inner(Margin {
                    vertical: 0,
                    horizontal: 1,
                })
                .offset(Offset {
                    x: 0,
                    y: i.shrink(),
                }),
            );
        }
    }

    fn scroll(&self, frame: &mut Frame, area: Rect) {
        if self.buffer.len() <= area.height as usize {
            return;
        }

        let scrollbar = Scrollbar::new(ratatui::widgets::ScrollbarOrientation::VerticalRight)
            .track_symbol(Some("|"));

        let mut state = ScrollbarState::new(
            self.buffer
                .len()
                .saturating_sub(usize::from(area.height + 1)),
        )
        .position(self.view.y as usize);

        frame.render_stateful_widget(scrollbar, area, &mut state);
    }
}

impl<'a> Widget for Viewport<'a> {
    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        frame.render_widget(Clear, area);

        let area = if let Some(block) = self.block.as_ref() {
            let inner = block.inner(area);
            frame.render_widget(block, area);

            inner
        } else {
            area
        };

        self.content(frame, area);
        self.scroll(frame, area);

        Ok(())
    }
}
