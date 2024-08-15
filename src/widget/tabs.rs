use eyre::{eyre, Result};
use ratatui::{
    layout::Rect,
    prelude::*,
    style::{Modifier, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::{propagate, Widget};
use crate::events::{Broadcast, Event, Keypress};

pub struct Tab {
    name: String,
    widget: Box<dyn Fn() -> Box<dyn Widget> + Send>,
    margin: u16,
}

impl Tab {
    pub fn new(name: String, widget: Box<dyn Fn() -> Box<dyn Widget> + Send>) -> Self {
        Self {
            name,
            widget,
            margin: 2,
        }
    }

    pub fn no_margin(mut self) -> Self {
        self.margin = 0;
        self
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

    pub fn scroll(&mut self, idx: usize) {
        self.idx = idx.clamp(0, self.items.len().saturating_sub(1));
        self.current = self.items[self.idx].widget();
    }
}

impl Widget for TabbedView {
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        if let Event::Goto(route) = event {
            if !route.is_empty() {
                self.scroll(route[0].parse::<usize>()?);
            }

            return Ok(Broadcast::Consumed);
        }

        propagate!(self.current.dispatch(event), {});

        let Event::Keypress(key) = event else {
            return Ok(Broadcast::Ignored);
        };

        match key {
            Keypress::CursorLeft => self.scroll(self.idx.saturating_sub(1)),
            Keypress::CursorRight => self.scroll(self.idx.saturating_add(1)),
            _ => return Ok(Broadcast::Ignored),
        }

        Ok(Broadcast::Consumed)
    }

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
            .horizontal_margin(self.items[self.idx].margin)
            .areas(body_area);

        self.current.draw(frame, nested);
    }
}
