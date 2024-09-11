use std::{cell::RefCell, rc::Rc};

use eyre::{eyre, Result};
use ratatui::{
    layout::{Position, Rect},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::Widget;
use crate::events::{Broadcast, Event, Keypress};

pub type Content = Rc<RefCell<Option<String>>>;

pub trait ContentExt {
    fn from_string<S: Into<String>>(x: S) -> Content {
        Rc::new(RefCell::new(Some(x.into())))
    }
}

impl ContentExt for Content {}

pub struct Text {
    title: String,
    content: Content,
    pos: u16,
}

#[bon::bon]
impl Text {
    #[builder]
    pub fn new(#[builder(into)] title: String, #[builder(default)] content: Content) -> Self {
        #[allow(clippy::cast_possible_truncation)]
        let pos = content.borrow().as_ref().map_or(0, String::len) as u16;

        Self {
            title,
            content,
            pos,
        }
    }

    pub fn content(&self) -> Content {
        self.content.clone()
    }
}

impl Widget for Text {
    // TODO: implement ctrl + a, ctrl + e, ctrl + k, ctrl + u
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        let Some(key) = event.key() else {
            return Ok(Broadcast::Ignored);
        };

        match key {
            Keypress::Escape => return Ok(Broadcast::Exited),
            Keypress::Printable(x) => {
                self.content
                    .try_borrow_mut()?
                    .get_or_insert_with(String::new)
                    .insert(self.pos as usize, *x);
                self.pos = self.pos.saturating_add(1);
            }
            Keypress::Backspace => 'outer: {
                if self.pos == 0 {
                    break 'outer;
                }

                self.content
                    .try_borrow_mut()?
                    .as_mut()
                    .ok_or(eyre!("no content"))?
                    .remove(self.pos as usize - 1);
                self.pos = self.pos.saturating_sub(1);
            }
            Keypress::CursorLeft => {
                self.pos = self.pos.saturating_sub(1);
            }
            #[allow(clippy::cast_possible_truncation)]
            Keypress::CursorRight => {
                self.pos = self.pos.saturating_add(1).clamp(
                    0,
                    self.content.try_borrow()?.as_ref().map_or(0, String::len) as u16,
                );
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

        let pg = Paragraph::new(
            self.content
                .try_borrow()?
                .as_ref()
                .map_or(String::new(), String::clone),
        )
        .block(block);

        frame.render_widget(pg, area);

        frame.set_cursor_position(Position::new(cmd_pos.x + self.pos, cmd_pos.y));

        Ok(())
    }
}
