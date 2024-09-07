use eyre::Result;
use ratatui::{
    layout::{Position, Rect},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::Widget;
use crate::events::{Broadcast, Event, Keypress};

#[derive(Default)]
pub struct Text {
    title: String,
    content: String,
    pos: u16,
}

impl Text {
    pub fn with_title(mut self, title: &str) -> Self {
        self.title = title.to_string();
        self
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn with_content(mut self, content: &str) -> Self {
        self.content = content.to_string();
        self.pos = self.content.len() as u16;
        self
    }

    pub fn content(&self) -> String {
        self.content.clone()
    }
}

impl Widget for Text {
    // TODO: implement ctrl + a, ctrl + e, ctrl + k, ctrl + u
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        match event.key() {
            Some(Keypress::Escape) => {
                return Ok(Broadcast::Exited);
            }
            Some(Keypress::Printable(x)) => {
                self.content.insert(self.pos as usize, *x);
                self.pos = self.pos.saturating_add(1);
            }
            Some(Keypress::Backspace) => 'outer: {
                if self.content.is_empty() || self.pos == 0 {
                    break 'outer;
                }

                self.content.remove(self.pos as usize - 1);
                self.pos = self.pos.saturating_sub(1);
            }
            Some(Keypress::CursorLeft) => {
                self.pos = self.pos.saturating_sub(1);
            }
            #[allow(clippy::cast_possible_truncation)]
            Some(Keypress::CursorRight) => {
                self.pos = self
                    .pos
                    .saturating_add(1)
                    .clamp(0, self.content.len() as u16);
            }
            _ => {
                return Ok(Broadcast::Ignored);
            }
        };

        Ok(Broadcast::Consumed)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let mut block = Block::default().borders(Borders::ALL);

        if !self.title.is_empty() {
            block = block.title(self.title.as_ref());
        }

        let cmd_pos = block.inner(area);

        let pg = Paragraph::new(self.content()).block(block);

        frame.render_widget(pg, area);

        frame.set_cursor_position(Position::new(cmd_pos.x + self.pos, cmd_pos.y));

        Ok(())
    }
}
