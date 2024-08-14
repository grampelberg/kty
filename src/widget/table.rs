use std::borrow::BorrowMut;

use eyre::Result;
use ratatui::{
    layout::Rect,
    prelude::*,
    style::{palette::tailwind, Modifier},
    widgets::{self, Block, Borders, Clear, TableState},
    Frame,
};

use super::{filter::Filter, propagate, TableRow, Widget};
use crate::events::{Broadcast, Event, Keypress};

pub struct RowStyle {
    pub healthy: style::Style,
    pub unhealthy: style::Style,
    pub normal: style::Style,
}

impl Default for RowStyle {
    fn default() -> Self {
        Self {
            healthy: style::Style::default().fg(tailwind::GREEN.c300),
            unhealthy: style::Style::default().fg(tailwind::RED.c300),
            normal: style::Style::default().fg(tailwind::INDIGO.c300),
        }
    }
}

pub struct Style {
    pub border: style::Style,
    pub header: style::Style,
    pub selected: style::Style,
    pub row: RowStyle,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            border: style::Style::default(),
            header: style::Style::default().bold(),
            selected: style::Style::default().add_modifier(Modifier::REVERSED),
            row: RowStyle::default(),
        }
    }
}

pub trait Content<'a, K>
where
    K: TableRow<'a>,
{
    fn items(&self, filter: Option<&str>) -> Vec<impl TableRow<'a>>;
}

enum State {
    List(TableState),
    Filtered(TableState, Filter),
    Detail(Box<dyn Widget>),
}

impl State {
    fn list(&mut self) {
        *self = State::default();
    }

    fn filter(&mut self) {
        let state = match self {
            State::List(state) => state.to_owned(),
            _ => TableState::default().with_selected(0),
        };

        *self = State::Filtered(state, Filter::default());
    }

    fn detail(&mut self, widget: Box<dyn Widget>) {
        *self = State::Detail(widget);
    }
}

impl Default for State {
    fn default() -> Self {
        Self::List(TableState::default().with_selected(0))
    }
}

pub type DetailFn = Box<dyn Fn(usize, Option<&str>) -> Result<Box<dyn Widget>> + Send>;

pub struct Table {
    style: Style,
    title: String,

    state: State,
    constructor: DetailFn,
}

impl Table {
    pub fn new(title: String, constructor: DetailFn) -> Self {
        Self {
            style: Style::default(),
            title,

            state: State::default(),
            constructor,
        }
    }

    pub fn title(mut self, title: &str) -> Self {
        self.title = title.to_string();
        self
    }

    fn render_list<'a, C, K>(&mut self, frame: &mut Frame, area: Rect, content: &C)
    where
        C: Content<'a, K>,
        K: TableRow<'a>,
    {
        let (state, filter) = match self.state {
            State::Filtered(ref mut state, ref filter) => (state, Some(filter)),
            State::List(ref mut state) => (state, None),
            State::Detail(_) => return,
        };

        let border = Block::default()
            .title(self.title.as_str()) // TODO: need a breadcrumb
            .borders(Borders::ALL)
            .style(self.style.border);

        let items = content.items(filter.map(Filter::content));
        let max = items.len().saturating_sub(1);

        if state.selected().unwrap_or_default() > max {
            state.select(Some(max));
        }

        let rows = items
            .iter()
            .map(|item| item.row(&self.style.row))
            .collect::<Vec<_>>();

        let table = widgets::Table::new(rows, K::constraints())
            .header(K::header().style(self.style.header))
            .block(border)
            .highlight_style(self.style.selected);

        frame.render_stateful_widget(table, area, state);
    }

    fn render_filter(&mut self, frame: &mut Frame, area: Rect) {
        let State::Filtered(_, filter) = self.state.borrow_mut() else {
            return;
        };

        let [_, filter_area] =
            Layout::vertical([Constraint::Fill(0), Constraint::Length(3)]).areas(area);

        // Command ends up being written *over the table (which writes to
        // the whole screen). The clear makes sure that table
        // items don't show up weirdly behind a transparent command
        // buffer. frame.
        frame.render_widget(Clear, filter_area);

        filter.draw(frame, filter_area);
    }

    fn render_detail(&mut self, frame: &mut Frame, area: Rect) {
        let State::Detail(ref mut widget) = self.state.borrow_mut() else {
            return;
        };

        widget.draw(frame, area);
    }

    pub fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        if let Event::Goto(route) = event {
            self.handle_route(route)?;

            return Ok(Broadcast::Consumed);
        }

        propagate!(self.dispatch_list(event), {});
        propagate!(self.dispatch_filter(event), self.state.list());
        propagate!(self.dispatch_detail(event), self.state.list());

        Ok(Broadcast::Ignored)
    }

    //     #[allow(clippy::unnecessary_wraps)]
    fn dispatch_list(&mut self, event: &Event) -> Result<Broadcast> {
        let (state, filter) = match self.state {
            State::Filtered(ref mut state, ref filter) => (state, Some(filter)),
            State::List(ref mut state) => (state, None),
            State::Detail(_) => return Ok(Broadcast::Ignored),
        };

        let Event::Keypress(key) = event else {
            return Ok(Broadcast::Ignored);
        };

        match key {
            Keypress::CursorDown => {
                state.select(Some(state.selected().unwrap_or_default().saturating_add(1)));
            }
            Keypress::CursorUp => {
                state.select(Some(state.selected().unwrap_or_default().saturating_sub(1)));
            }
            // TODO: this should be handled by a router
            Keypress::Printable('/') => self.state.filter(),
            Keypress::Enter => {
                let detail = {
                    (self.constructor)(
                        state.selected().unwrap_or_default(),
                        filter.map(Filter::content),
                    )?
                };

                self.state.detail(detail);
            }
            _ => return Ok(Broadcast::Ignored),
        }

        Ok(Broadcast::Consumed)
    }

    fn dispatch_filter(&mut self, event: &Event) -> Result<Broadcast> {
        let State::Filtered(_, ref mut filter) = self.state.borrow_mut() else {
            return Ok(Broadcast::Ignored);
        };

        filter.dispatch(event)
    }

    fn dispatch_detail(&mut self, event: &Event) -> Result<Broadcast> {
        let State::Detail(ref mut widget) = self.state.borrow_mut() else {
            return Ok(Broadcast::Ignored);
        };

        widget.dispatch(event)
    }

    fn handle_route(&mut self, route: &[String]) -> Result<()> {
        let (first, rest) = route.split_first().unwrap();

        let mut detail = { (self.constructor)(0, Some(first.as_str()))? };

        if !rest.is_empty() {
            detail.dispatch(&Event::Goto(rest.to_vec()))?;
        }

        self.state.detail(detail);

        Ok(())
    }

    pub fn render<'a, C, K>(&mut self, frame: &mut Frame, area: Rect, content: &C)
    where
        C: Content<'a, K>,
        K: TableRow<'a>,
    {
        self.render_list(frame, area, content);
        self.render_filter(frame, area);
        self.render_detail(frame, area);
    }
}
