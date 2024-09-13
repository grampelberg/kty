use bon::Builder;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    prelude::Widget,
    text::Line,
    widgets::{Paragraph, StatefulWidgetRef},
};

#[derive(Builder)]
pub struct Viewport<'a> {
    buffer: &'a Vec<String>,
}

impl<'a> StatefulWidgetRef for Viewport<'a> {
    type State = Position;

    #[allow(clippy::cast_possible_truncation)]
    fn render_ref(&self, area: Rect, buf: &mut Buffer, state: &mut Position) {
        state.y = state
            .y
            .clamp(0, (self.buffer.len() as u16).saturating_sub(area.height));
        let start = state.y as usize;
        let end = state
            .y
            .saturating_add(area.height)
            .clamp(0, self.buffer.len() as u16) as usize;

        let txt: Vec<Line> = self.buffer[start..end]
            .iter()
            .map(|l| Line::from(l.as_str()))
            .collect();

        Paragraph::new(txt).render(area, buf);
    }
}
