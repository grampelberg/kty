use std::io::BufRead;

use ansi_to_tui::IntoText;
use eyre::{Report, Result};
use itertools::Itertools;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    prelude::*,
    style::Style,
    widgets::{Block, Borders, Clear},
    Frame,
};

use super::{nav::move_cursor, viewport::Viewport, Placement, Widget};
use crate::events::{Broadcast, Event, StringError};

#[derive(Default)]
pub struct Error {
    msg: String,

    position: Position,
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
    fn dispatch(&mut self, event: &Event, _: &Buffer, area: Rect) -> Result<Broadcast> {
        let Some(key) = event.key() else {
            return Ok(Broadcast::Ignored);
        };

        if let Some(m) = move_cursor(key, area) {
            self.position = m.saturating_adjust(self.position);

            Ok(Broadcast::Consumed)
        } else {
            Ok(Broadcast::Exited)
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red));

        let lines: Vec<_> = self
            .msg
            .as_bytes()
            .lines()
            .map_ok(|l| l.into_text())
            .flatten()
            .try_collect()?;

        let width = lines.iter().map(Text::width).max().unwrap_or(0) as u16 + 1;

        let [_, area, _] = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Max(width),
            Constraint::Fill(1),
        ])
        .areas(area);

        let height = lines.len() as u16 + 1;

        let [_, vert, _] = Layout::vertical([
            Constraint::Max(10),
            Constraint::Max(height),
            Constraint::Max(10),
        ])
        .areas(area);

        frame.render_widget(Clear, vert);

        Viewport::builder()
            .block(block)
            .buffer(&lines)
            .view(self.position.into())
            .build()
            .draw(frame, vert)
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
