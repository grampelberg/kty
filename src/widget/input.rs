use bon::builder;
use eyre::Result;
use ratatui::{
    layout::{Position, Rect},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::{Renderable, StatefulWidget};
use crate::events::{Broadcast, Event, Keypress};

#[builder(on(String, into))]
pub struct Text<F>
where
    F: Filterable,
{
    #[builder(default)]
    title: String,

    #[builder(skip)]
    pos: u16,

    #[builder(skip)]
    _phantom: std::marker::PhantomData<F>,
}

impl<F> Renderable for Text<F> where F: Filterable {}

pub trait Filterable {
    fn filter(&mut self) -> &mut Option<String>;
}

impl<F> StatefulWidget for Text<F>
where
    F: Filterable,
{
    type State = F;

    fn dispatch(&mut self, event: &Event, state: &mut Self::State) -> Result<Broadcast> {
        let filter = state.filter();

        match event.key() {
            Some(Keypress::Escape) => {
                return Ok(Broadcast::Exited);
            }
            Some(Keypress::Printable(x)) => {
                filter
                    .get_or_insert_with(String::new)
                    .insert(self.pos as usize, *x);
                self.pos = self.pos.saturating_add(1);
            }
            Some(Keypress::Backspace) => 'outer: {
                if filter.is_none() || self.pos == 0 {
                    break 'outer;
                }

                for f in filter.iter_mut() {
                    f.remove(self.pos as usize - 1);
                }

                filter.take_if(|f| f.is_empty());

                self.pos = self.pos.saturating_sub(1);
            }
            Some(Keypress::CursorLeft) => {
                self.pos = self.pos.saturating_sub(1);
            }
            #[allow(clippy::cast_possible_truncation)]
            Some(Keypress::CursorRight) => {
                self.pos = self.pos.saturating_add(1).clamp(0, filter.len() as u16);
            }
            _ => {
                return Ok(Broadcast::Ignored);
            }
        };

        Ok(Broadcast::Ignored)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, state: &mut Self::State) -> Result<()> {
        let mut block = Block::default().borders(Borders::ALL);

        if !self.title.is_empty() {
            block = block.title(self.title.as_ref());
        }

        let cmd_pos = block.inner(area);

        let pg = Paragraph::new(state.filter().clone().unwrap_or_default()).block(block);

        frame.render_widget(pg, area);

        frame.set_cursor_position(Position::new(cmd_pos.x + self.pos, cmd_pos.y));

        Ok(())
    }
}

#[derive(Default)]
pub struct TextState(Option<String>);

impl TextState {
    pub fn new<T: Into<String>>(val: T) -> Self {
        Self(Some(val.into()))
    }
}

impl AsRef<Option<String>> for TextState {
    fn as_ref(&self) -> &Option<String> {
        &self.0
    }
}

impl AsMut<Option<String>> for TextState {
    fn as_mut(&mut self) -> &mut Option<String> {
        &mut self.0
    }
}

impl Filterable for TextState {
    fn filter(&mut self) -> &mut Option<String> {
        &mut self.0
    }
}

trait OptLen {
    fn len(&self) -> usize;
}

impl OptLen for Option<String> {
    fn len(&self) -> usize {
        self.as_ref().map_or(0, String::len)
    }
}
