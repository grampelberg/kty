use std::{cell::RefCell, rc::Rc};

use eyre::Result;
use lazy_static::lazy_static;
use prometheus::{register_int_counter, IntCounter};
use ratatui::{
    layout::{Constraint, Rect},
    style,
    style::{palette::tailwind, Modifier, Stylize},
    widgets::{self, Block, Borders, TableState},
    Frame,
};
use tachyonfx::Effect;

use super::{input::Text, BoxWidget, Bundle, Widget};
use crate::events::{Broadcast, Event, Keypress};

lazy_static! {
    static ref TABLE_FILTER: IntCounter = register_int_counter!(
        "table_filter_total",
        "Number of times a table has been filtered"
    )
    .unwrap();
}

pub trait Row {
    fn constraints() -> Vec<Constraint>;

    fn header<'a>() -> Option<widgets::Row<'a>> {
        None
    }

    fn row(&self, style: &RowStyle) -> widgets::Row;
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
            selected: style::Style::default()
                .add_modifier(Modifier::REVERSED)
                .bg(tailwind::GRAY.c700),
            row: RowStyle::default(),
        }
    }
}

pub trait Items
where
    Self::Item: Row,
{
    type Item;

    fn items(&self, filter: Option<String>) -> Vec<Self::Item>;
}

pub struct Table<S>
where
    S: Items,
{
    // Configuration of how the table looks
    style: Style,
    title: Option<String>,
    highlight: bool,

    // Internal state
    items: S,
    view: TableState,
    filter: Rc<RefCell<Option<String>>>,

    _phantom: std::marker::PhantomData<S>,
}

#[bon::bon]
impl<S> Table<S>
where
    S: Items,
{
    #[builder(on(String, into))]
    pub fn new(
        #[builder(default)] style: Style,
        title: Option<String>,
        #[builder(default = true)] highlight: bool,
        #[builder(default = true)] selected: bool,
        items: S,
        #[builder(default)] filter: Rc<RefCell<Option<String>>>,
    ) -> Self {
        let view = if selected {
            TableState::default().with_selected(0)
        } else {
            TableState::default()
        };

        Self {
            style,
            title,
            highlight,
            items,
            view,
            filter,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn filter(&self) -> Rc<RefCell<Option<String>>> {
        self.filter.clone()
    }
}

impl<S> Widget for Table<S>
where
    S: Items,
{
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        let Some(key) = event.key() else {
            return Ok(Broadcast::Ignored);
        };

        match key {
            Keypress::CursorDown | Keypress::Printable('j') => self.view.select_next(),
            Keypress::CursorUp | Keypress::Printable('k') => self.view.select_previous(),
            Keypress::Enter => {
                return Ok(Broadcast::Selected(
                    self.view.selected().unwrap_or_default(),
                ))
            }
            _ => return Ok(Broadcast::Ignored),
        }

        Ok(Broadcast::Consumed)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let items = self.items.items(self.filter.borrow().clone());

        let rows = items
            .iter()
            .map(|item| item.row(&self.style.row))
            .collect::<Vec<_>>();

        let mut table = widgets::Table::new(rows, S::Item::constraints());
        let mut border = Block::default()
            .borders(Borders::ALL)
            .style(self.style.border);

        if self.highlight {
            table = table.highlight_style(self.style.selected);
        }

        if let Some(header) = S::Item::header() {
            table = table.header(header).style(self.style.header);
        };

        if let Some(title) = self.title.as_ref() {
            border = border.title(title.as_str());
        };

        table = table.block(border);

        frame.render_stateful_widget(table, area, &mut self.view);

        Ok(())
    }
}

pub type DetailFn = Box<dyn Fn(usize, Option<String>) -> Result<Box<dyn Widget>> + Send>;

pub struct Filtered<S>
where
    S: Items,
{
    constructor: DetailFn,
    filter: Rc<RefCell<Option<String>>>,

    effects: Vec<Effect>,
    widgets: Vec<BoxWidget>,

    _phantom: std::marker::PhantomData<S>,
}

#[bon::bon]
impl<S> Filtered<S>
where
    S: Items + 'static,
{
    #[builder]
    pub fn new(table: Table<S>, constructor: DetailFn) -> Self {
        Self {
            constructor,
            filter: table.filter(),
            effects: vec![],
            widgets: vec![table.boxed()],
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn select(&mut self, idx: usize) -> Result<()> {
        self.widgets
            .push((self.constructor)(idx, self.filter.borrow().clone())?);

        Ok(())
    }
}

impl<S> Bundle for Filtered<S>
where
    S: Items + 'static,
{
    fn effects(&mut self) -> &mut Vec<Effect> {
        &mut self.effects
    }

    fn widgets(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.widgets
    }
}

impl<S> Widget for Filtered<S>
where
    S: Items + 'static,
{
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        if let Some(Keypress::Printable('/')) = event.key() {
            TABLE_FILTER.inc();

            self.widgets
                .push(Text::builder().title("Filter").build().boxed());

            return Ok(Broadcast::Consumed);
        }

        match self.dispatch_children(event)? {
            Broadcast::Selected(idx) => {
                self.select(idx)?;

                Ok(Broadcast::Consumed)
            }
            x => Ok(x),
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        Bundle::draw(self, frame, area)
    }
}
