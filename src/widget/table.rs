use eyre::{eyre, Result};
use lazy_static::lazy_static;
use prometheus::{register_int_counter, IntCounter};
use ratatui::{
    layout::{Constraint, Rect},
    prelude::Stylize,
    style,
    style::{palette::tailwind, Modifier},
    widgets::{self, Block, Borders, TableState},
    Frame,
};
use tachyonfx::Effect;

use super::{
    input::{Filterable, Text},
    Container, Contents, Mode, Renderable, StatefulWidget, Widget,
};
use crate::events::{Broadcast, Event, Keypress};

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

pub trait Row {
    fn constraints() -> Vec<Constraint>;

    fn header<'a>() -> Option<widgets::Row<'a>> {
        None
    }

    fn row(&self, style: &RowStyle) -> widgets::Row;
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

    fn items(&self) -> Vec<Self::Item>;
}

pub struct Table<S> {
    // Configuration of how the table looks
    style: Style,
    title: Option<String>,
    highlight: bool,

    // Internal state
    view: TableState,

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
            view,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<S> StatefulWidget for Table<S>
where
    S: Items,
{
    type State = S;

    fn dispatch(&mut self, event: &Event, _state: &mut Self::State) -> Result<Broadcast> {
        let Some(key) = event.key() else {
            return Ok(Broadcast::Ignored);
        };

        match key {
            Keypress::CursorDown | Keypress::Printable('j') => self.view.select_next(),
            Keypress::CursorUp | Keypress::Printable('k') => self.view.select_previous(),
            _ => return Ok(Broadcast::Ignored),
        }

        Ok(Broadcast::Consumed)
    }

    fn draw<'a>(&mut self, frame: &mut Frame, area: Rect, state: &mut Self::State) -> Result<()> {
        let items = state.items();

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

impl<S> Renderable for Table<S> where S: Items {}

pub type DetailFn<I> = Box<dyn Fn(&I) -> Result<Box<dyn Widget>> + Send>;

pub struct CollectionView<S>
where
    S: Items + Filterable,
{
    constructor: DetailFn<S::Item>,

    effects: Vec<Effect>,
    widgets: Vec<Mode<S>>,

    _phantom: std::marker::PhantomData<S>,
}

#[bon::bon]
impl<S> CollectionView<S>
where
    S: Items + Filterable + 'static,
{
    #[builder]
    pub fn new(table: Table<S>, constructor: DetailFn<S::Item>) -> Self {
        let widgets = vec![table.mode()];

        Self {
            constructor,
            effects: Vec::new(),
            widgets,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<S> Container for CollectionView<S>
where
    S: Items + Filterable + 'static,
{
    type State = S;

    fn contents(&mut self) -> Contents<Self::State> {
        Contents {
            effects: &mut self.effects,
            widgets: &mut self.widgets,
        }
    }

    fn dispatch(&mut self, event: &Event, state: &mut Self::State) -> Result<Broadcast> {
        let Some(key) = event.key() else {
            return Ok(Broadcast::Ignored);
        };

        if let Keypress::Printable('/') = key {
            TABLE_FILTER.inc();
            self.widgets
                .push(Text::builder().title("Filter").build().mode());

            return Ok(Broadcast::Consumed);
        }

        match self.dispatch_children(event, state)? {
            Broadcast::Selected(idx) => {
                self.widgets.push(Mode::Stateless((self.constructor)(
                    state.items().get(idx).ok_or(eyre!("{idx} not found"))?,
                )?));

                Ok(Broadcast::Consumed)
            }
            x => Ok(x),
        }
    }
}

impl<S> Renderable for CollectionView<S> where S: Items + Filterable + 'static {}
