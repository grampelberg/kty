use ansi_to_tui::IntoText;
use bon::Builder;
use eyre::Result;
use itertools::Itertools;
use ratatui::{
    layout::{Position, Rect},
    text::Text,
    widgets::Paragraph,
    Frame,
};

use super::Widget;

#[derive(Builder)]
pub struct Viewport<'a> {
    buffer: &'a Vec<String>,
    #[builder(default)]
    view: Position,
}

impl<'a> Widget for Viewport<'a> {
    #[allow(clippy::cast_possible_truncation)]
    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        self.view.y = self
            .view
            .y
            .clamp(0, (self.buffer.len() as u16).saturating_sub(area.height));
        let start = self.view.y as usize;
        let end = self
            .view
            .y
            .saturating_add(area.height)
            .clamp(0, self.buffer.len() as u16) as usize;

        let txt = self.buffer[start..end]
            .iter()
            .map(|l| l.as_str().into_text())
            .fold_ok(Text::default(), |txt, l| txt + l)?;

        frame.render_widget(Paragraph::new(txt), area);

        Ok(())
    }
}
