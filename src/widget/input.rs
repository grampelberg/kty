use std::{cell::RefCell, rc::Rc};

use eyre::{eyre, Result};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::{
    nav::{exit_keys, move_cursor, Movement, Shrink},
    Widget,
};
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
    fn dispatch(&mut self, event: &Event, _: &Buffer, area: Rect) -> Result<Broadcast> {
        let Some(key) = event.key() else {
            return Ok(Broadcast::Ignored);
        };

        match key {
            exit_keys!() => {
                self.content.try_borrow_mut()?.take();

                return Ok(Broadcast::Exited);
            }
            Keypress::Printable(x) => {
                self.content
                    .try_borrow_mut()?
                    .get_or_insert_with(String::new)
                    .insert(self.pos as usize, *x);
                self.pos = self.pos.saturating_add(1);

                return Ok(Broadcast::Consumed);
            }
            Keypress::Backspace | Keypress::Delete => 'outer: {
                if self.pos == 0 {
                    break 'outer;
                }

                self.content
                    .try_borrow_mut()?
                    .as_mut()
                    .ok_or(eyre!("no content"))?
                    .remove(self.pos as usize - 1);
                self.pos = self.pos.saturating_sub(1);

                return Ok(Broadcast::Consumed);
            }
            Keypress::Control('k') => {
                let mut opt = self.content.try_borrow_mut()?;

                let content = opt.get_or_insert_with(String::new);

                *content = String::new();

                self.pos = 0;
            }
            _ => {}
        };

        #[allow(clippy::cast_possible_truncation)]
        if let Some(Movement::X(x)) = move_cursor(key, area) {
            self.pos = self.pos.saturating_add_signed(x.shrink());

            return Ok(Broadcast::Consumed);
        }

        Ok(Broadcast::Ignored)
    }

    #[allow(clippy::cast_possible_truncation)]
    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let mut block = Block::default().borders(Borders::ALL);

        if !self.title.is_empty() {
            block = block.title(self.title.as_ref());
        }

        let cmd_pos = block.inner(area);
        let content = self
            .content
            .try_borrow()?
            .as_ref()
            .map_or(String::new(), String::clone);

        self.pos = self.pos.clamp(0, content.len() as u16);

        let pg = Paragraph::new(content).block(block);

        frame.render_widget(pg, area);

        frame.set_cursor_position(Position::new(cmd_pos.x + self.pos, cmd_pos.y));

        Ok(())
    }

    fn placement(&self) -> super::Placement {
        super::Placement {
            vertical: super::Constraint::Length(3),
            ..Default::default()
        }
    }
}
