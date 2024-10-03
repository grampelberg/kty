use std::{cell::RefCell, rc::Rc};

use eyre::Result;
use lazy_static::lazy_static;
use prometheus::{register_int_counter, IntCounter};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style,
    style::{palette::tailwind, Modifier, Stylize},
    widgets::{self, Block, Borders, TableState},
    Frame,
};
use tachyonfx::{fx, EffectTimer, Interpolation};
use tracing::Level;

use super::{
    error::Error,
    input::Text,
    nav::{move_cursor, Movement, Shrink},
    view::{Element, View},
    BoxWidget, Widget,
};
use crate::{
    events::{Broadcast, Event, Keypress},
    fx::Animated,
};

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
    border: Borders,

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
        #[builder(default = Borders::ALL)] border: Borders,
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
            border,
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
    #[tracing::instrument(ret(level = tracing::Level::TRACE), skip_all, fields(name = self._name()))]
    fn dispatch(&mut self, event: &Event, _: &Buffer, area: Rect) -> Result<Broadcast> {
        let Some(key) = event.key() else {
            return Ok(Broadcast::Ignored);
        };

        if let Some(Movement::Y(y)) = move_cursor(key, area) {
            self.view.select(Some(
                self.view
                    .selected()
                    .unwrap_or_default()
                    .saturating_add_signed(y.shrink()),
            ));

            return Ok(Broadcast::Consumed);
        }

        if matches!(key, Keypress::Enter) {
            return Ok(Broadcast::Selected(
                self.view.selected().unwrap_or_default(),
            ));
        }

        Ok(Broadcast::Ignored)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let items = self.items.items(self.filter.borrow().clone());

        let rows = items
            .iter()
            .map(|item| item.row(&self.style.row))
            .collect::<Vec<_>>();

        let mut table = widgets::Table::new(rows, S::Item::constraints());
        let mut border = Block::default()
            .borders(self.border)
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

        if self.border != Borders::NONE {
            table = table.block(border);
        }

        frame.render_stateful_widget(table, area, &mut self.view);

        Ok(())
    }
}

pub type DetailFn = Box<dyn Fn(usize, Option<String>) -> Result<BoxWidget>>;

pub struct Filtered {
    constructor: DetailFn,
    filter: Rc<RefCell<Option<String>>>,
    view: View,
}

#[bon::bon]
impl Filtered {
    #[builder]
    pub fn new<S>(table: Table<S>, constructor: DetailFn) -> Self
    where
        S: Items + 'static,
    {
        Self {
            constructor,
            filter: table.filter(),
            view: View::builder()
                .widgets(vec![Element::builder()
                    .widget(table.boxed())
                    .terminal(true)
                    .build()])
                .build(),
        }
    }

    pub fn select(&mut self, idx: usize) -> Result<()> {
        self.select_with(idx)
    }

    fn select_with(&mut self, idx: usize) -> Result<()> {
        let widget = (self.constructor)(idx, self.filter.borrow().clone())?;

        self.view.push(
            Animated::builder()
                .effect(fx::coalesce(EffectTimer::from_ms(
                    500,
                    Interpolation::SineInOut,
                )))
                .widget(widget)
                .build()
                .boxed()
                .into(),
        );

        Ok(())
    }
}

impl Widget for Filtered {
    #[tracing::instrument(ret(level = Level::TRACE), skip_all, fields(name = self._name()))]
    fn dispatch(&mut self, event: &Event, buffer: &Buffer, area: Rect) -> Result<Broadcast> {
        match self.view.dispatch(event, buffer, area) {
            Ok(Broadcast::Selected(idx)) => {
                self.select_with(idx)?;

                Ok(Broadcast::Consumed)
            }
            Ok(Broadcast::Ignored) => {
                let Some(Keypress::Printable('/')) = event.key() else {
                    return Ok(Broadcast::Ignored);
                };

                // If there's more than one widget in the view, either there's already a filter
                // or a detail is being drawn. In either case, don't create a new filter. The
                // fact that this exists is unfortunate and suggests that the abstractions here
                // are wrong. If this continues, especially in this component, it is likely time
                // to figure out a better abstraction. Note, I've tried a couple and none have
                // worked very well.
                if self.view.len() > 1 {
                    return Ok(Broadcast::Ignored);
                }

                TABLE_FILTER.inc();

                // This is explicitly *not* animated currently. The ideal animation would be to
                // wipe up from the bottom. That doesn't really work because the filter box is
                // empty by default. It needs more visual content to have the wipe make sense.
                self.view.push(
                    Text::builder()
                        .title("Filter")
                        .content(self.filter.clone())
                        .border_style(style::Style::default().fg(tailwind::BLUE.c500))
                        .build()
                        .boxed()
                        .into(),
                );

                Ok(Broadcast::Consumed)
            }
            Ok(x) => Ok(x),
            Err(e) => {
                self.view.push(Error::from(e).boxed().into());

                Ok(Broadcast::Consumed)
            }
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        self.view.draw(frame, area)
    }

    fn zindex(&self) -> u16 {
        self.view.zindex()
    }
}
