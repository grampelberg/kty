use std::borrow::BorrowMut;

use bon::builder;
use eyre::Result;
use lazy_static::lazy_static;
use prometheus::{register_int_counter, IntCounter};
use ratatui::{
    layout::Rect,
    prelude::*,
    style::{palette::tailwind, Modifier},
    widgets::{self, Block, Borders, Clear, TableState},
    Frame,
};

use super::{input::Text, propagate, TableRow, Widget};
use crate::{
    events::{Broadcast, Event, Keypress},
    widget::error::Error,
};

lazy_static! {
    static ref TABLE_FILTER: IntCounter = register_int_counter!(
        "table_filter_total",
        "Number of times a table has been filtered"
    )
    .unwrap();
}

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
    fn items(&self, filter: Option<String>) -> Vec<impl TableRow<'a>>;
}

enum State {
    List(TableState),
    Filtered(TableState, Text),
    Detail(Box<dyn Widget>),
}

impl State {
    fn list(&mut self) {
        *self = State::default();
    }

    fn filter(&mut self) {
        TABLE_FILTER.inc();

        let state = match self {
            State::List(state) => state.to_owned(),
            _ => TableState::default().with_selected(0),
        };

        *self = State::Filtered(state, Text::default().with_title("Filter"));
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

pub type DetailFn = Box<dyn Fn(usize, Option<String>) -> Result<Box<dyn Widget>> + Send>;

#[builder(on(String, into))]
pub struct Table {
    #[builder(default)]
    style: Style,
    title: Option<String>,
    #[builder(default)]
    no_highlight: bool,

    #[builder(default)]
    state: State,
    constructor: Option<DetailFn>,
}

impl Table {
    pub fn enter(&mut self, idx: usize, filter: Option<String>) -> Result<Broadcast> {
        let Some(constructor) = self.constructor.as_ref() else {
            return Ok(Broadcast::Ignored);
        };

        let detail = { (constructor)(idx, filter)? };

        self.state.detail(detail);

        Ok(Broadcast::Consumed)
    }

    #[allow(clippy::unnecessary_wraps)]
    fn render_list<'a, C, K>(&mut self, frame: &mut Frame, area: Rect, content: &C) -> Result<()>
    where
        C: Content<'a, K>,
        K: TableRow<'a>,
    {
        let (state, filter) = match self.state {
            State::Filtered(ref mut state, ref filter) => (state, Some(filter)),
            State::List(ref mut state) => (state, None),
            State::Detail(_) => return Ok(()),
        };

        let items = content.items(filter.map(Text::content));
        let max = items.len().saturating_sub(1);

        if state.selected().unwrap_or_default() > max {
            state.select(Some(max));
        }

        let rows = items
            .iter()
            .map(|item| item.row(&self.style.row))
            .collect::<Vec<_>>();

        let mut table = widgets::Table::new(rows, K::constraints());

        if !self.no_highlight {
            table = table.highlight_style(self.style.selected);
        }

        if let Some(header) = K::header() {
            table = table.header(header).style(self.style.header);
        };

        let title = if let Some(title) = self.title.as_ref() {
            title.as_str()
        } else {
            ""
        };

        if self.title.is_some() {
            let border = Block::default()
                .title(title) // TODO: need a breadcrumb
                .borders(Borders::ALL)
                .style(self.style.border);

            table = table.block(border);
        };

        frame.render_stateful_widget(table, area, state);

        Ok(())
    }

    fn render_filter(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let State::Filtered(_, filter) = self.state.borrow_mut() else {
            return Ok(());
        };

        let [_, filter_area] =
            Layout::vertical([Constraint::Fill(0), Constraint::Length(3)]).areas(area);

        // Command ends up being written *over the table (which writes to
        // the whole screen). The clear makes sure that table
        // items don't show up weirdly behind a transparent command
        // buffer. frame.
        frame.render_widget(Clear, filter_area);

        filter.draw(frame, filter_area)
    }

    fn render_detail(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let State::Detail(ref mut widget) = self.state.borrow_mut() else {
            return Ok(());
        };

        widget.draw(frame, area)
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

        let Some(key) = event.key() else {
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
                let idx = state.selected().unwrap_or_default();
                let filter = filter.map(|f| f.content().to_string());

                self.enter(idx, filter)?;

                return Ok(Broadcast::Consumed);
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

    #[allow(clippy::unnecessary_wraps)]
    fn dispatch_detail(&mut self, event: &Event) -> Result<Broadcast> {
        let State::Detail(ref mut widget) = self.state.borrow_mut() else {
            return Ok(Broadcast::Ignored);
        };

        match widget.dispatch(event) {
            Ok(result) => Ok(result),
            Err(err) => {
                self.state.detail(Box::new(Error::from(err)));

                Ok(Broadcast::Consumed)
            }
        }
    }

    fn handle_route(&mut self, route: &[String]) -> Result<()> {
        let (first, rest) = route.split_first().unwrap();

        self.enter(0, Some(first.to_string()))?;

        self.dispatch_detail(&Event::Goto(rest.to_vec()))?;

        Ok(())
    }

    pub fn draw<'a, C, K>(&mut self, frame: &mut Frame, area: Rect, content: &C) -> Result<()>
    where
        C: Content<'a, K>,
        K: TableRow<'a>,
    {
        self.render_list(frame, area, content)?;
        self.render_filter(frame, area)?;
        self.render_detail(frame, area)?;

        Ok(())
    }
}
