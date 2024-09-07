use ansi_to_tui::IntoText;
use eyre::{Report, Result};
use ratatui::{
    layout::Rect,
    prelude::*,
    style::Style,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::{Placement, Widget};
use crate::events::{Broadcast, Event, Keypress, StringError};

#[derive(Default)]
pub struct Error {
    msg: String,

    position: (u16, u16),
}

impl From<Report> for Error {
    fn from(err: Report) -> Self {
        let Some(err) = err.downcast_ref::<StringError>() else {
            return format!("{err:?}").into();
        };

        err.to_string().into()
    }
}

impl From<String> for Error {
    fn from(msg: String) -> Self {
        Self {
            msg: format!("Error:{msg}"),
            ..Default::default()
        }
    }
}

impl Widget for Error {
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        match event.key() {
            Some(Keypress::CursorLeft) => {
                self.position.1 = self.position.1.saturating_sub(1);
            }
            Some(Keypress::CursorRight) => {
                self.position.1 = self.position.1.saturating_add(1);
            }
            Some(Keypress::CursorUp) => {
                self.position.0 = self.position.0.saturating_sub(1);
            }
            Some(Keypress::CursorDown) => {
                self.position.0 = self.position.0.saturating_add(1);
            }
            _ => return Ok(Broadcast::Exited),
        }

        Ok(Broadcast::Consumed)
    }

    #[allow(clippy::cast_possible_truncation)]
    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red));

        let pg = Paragraph::new(self.msg.as_bytes().into_text()?)
            .block(block)
            .scroll(self.position);

        let width = pg.line_width() as u16 + 1;

        let [_, area, _] = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Max(width),
            Constraint::Fill(1),
        ])
        .areas(area);

        let height = pg.line_count(area.width) as u16 + 1;

        let [_, vert, _] = Layout::vertical([
            Constraint::Max(10),
            Constraint::Max(height),
            Constraint::Max(10),
        ])
        .areas(area);

        frame.render_widget(pg, vert);

        Ok(())
    }

    fn placement(&self) -> Placement {
        Placement {
            horizontal: Constraint::Fill(1),
            vertical: Constraint::Percentage(100),
        }
    }

    fn zindex(&self) -> u16 {
        1
    }
}
