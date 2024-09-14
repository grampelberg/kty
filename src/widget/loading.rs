use eyre::Result;
use ratatui::{
    layout::{Flex, Layout, Rect},
    widgets::Paragraph,
    Frame,
};

use super::Widget;

pub struct Loading;

impl Widget for Loading {
    #[allow(clippy::cast_possible_truncation)]
    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let pg = Paragraph::new("Loading...");

        let y = Layout::horizontal([pg.line_width() as u16]).flex(Flex::Center);
        let x = Layout::vertical([pg.line_count(pg.line_width() as u16) as u16]).flex(Flex::Center);
        let [area] = x.areas(area);
        let [area] = y.areas(area);

        frame.render_widget(pg, area);

        Ok(())
    }

    fn zindex(&self) -> u16 {
        1
    }
}
