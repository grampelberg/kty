use eyre::{eyre, Result};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    prelude::*,
    style::{palette::tailwind, Modifier, Style},
    text::Line,
    widgets::{
        self, block::Title, Block, Borders, Clear, Paragraph, Row, StatefulWidget,
        StatefulWidgetRef, Table, TableState, Widget as _, WidgetRef,
    },
    Frame,
};

use super::{Dispatch, Screen, Widget};
use crate::events::{Broadcast, Event, Keypress};

pub struct Tab {
    name: String,
    widget: Box<dyn Fn() -> Box<dyn Widget> + Send>,
}

impl Tab {
    pub fn new(name: String, widget: Box<dyn Fn() -> Box<dyn Widget> + Send>) -> Self {
        Self { name, widget }
    }

    pub fn widget(&self) -> Box<dyn Widget> {
        (self.widget)()
    }
}

pub struct TabbedView {
    items: Vec<Tab>,

    selected_style: Style,

    idx: usize,
    current: Box<dyn Widget>,
}

impl TabbedView {
    pub fn new(tabs: Vec<Tab>) -> Result<Self> {
        if tabs.is_empty() {
            return Err(eyre!("Tabs must not be empty"));
        }

        Ok(Self {
            current: tabs[0].widget(),
            idx: 0,
            items: tabs,
            selected_style: Style::default().add_modifier(Modifier::REVERSED),
        })
    }

    pub fn scroll(&mut self, key: &Keypress) {
        self.idx = match key {
            Keypress::CursorLeft => self.idx.saturating_sub(1),
            Keypress::CursorRight => self.idx.saturating_add(1),
            _ => return,
        }
        .clamp(0, self.items.len().saturating_sub(1));

        self.current = self.items[self.idx].widget();
    }
}

impl Widget for TabbedView {}

impl Dispatch for TabbedView {
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        if matches!(self.current.dispatch(event)?, Broadcast::Consumed) {
            return Ok(Broadcast::Consumed);
        }

        let Event::Keypress(key) = event else {
            return Ok(Broadcast::Ignored);
        };

        match key {
            Keypress::CursorLeft | Keypress::CursorRight => self.scroll(key),
            _ => return Ok(Broadcast::Ignored),
        }

        Ok(Broadcast::Consumed)
    }
}

impl Screen for TabbedView {
    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let [tab_area, border, body_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .areas(area);

        let layout =
            Layout::horizontal(std::iter::repeat(Constraint::Fill(1)).take(self.items.len()))
                .spacing(1)
                .split(tab_area);

        for (i, (tab, area)) in self.items.iter().zip(layout.iter()).enumerate() {
            let style = if i == self.idx {
                self.selected_style
            } else {
                Style::default()
            };

            frame.render_widget(
                Paragraph::new(tab.name.clone())
                    .style(style)
                    .alignment(Alignment::Center),
                *area,
            );
        }

        frame.render_widget(Block::default().borders(Borders::TOP), border);

        let [nested] = Layout::default()
            .constraints([Constraint::Min(0)])
            .horizontal_margin(2)
            .areas(body_area);

        self.current.draw(frame, nested);
    }
}
