use ratatui::{
    buffer::Buffer,
    layout::{Flex, Layout, Rect},
    widgets::{Paragraph, Widget as _, WidgetRef},
};

pub struct Loading;

impl WidgetRef for Loading {
    #[allow(clippy::cast_possible_truncation)]
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let pg = Paragraph::new("Loading...");

        let y = Layout::horizontal([pg.line_width() as u16]).flex(Flex::Center);
        let x = Layout::vertical([pg.line_count(pg.line_width() as u16) as u16]).flex(Flex::Center);
        let [area] = x.areas(area);
        let [area] = y.areas(area);

        pg.render(area, buf);
    }
}
