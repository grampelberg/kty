use std::{cell::RefCell, rc::Rc};

use bon::Builder;
use eyre::Result;
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Flex, Layout, Rect},
    style::{palette::tailwind, Style, Stylize},
    text::Text,
    widgets::{Block, Borders, Clear, Row, Table},
    Frame,
};
use tachyonfx::{fx, EffectTimer, Interpolation};
use tracing::{metadata::LevelFilter, Level};

use super::{
    debug::Debug,
    error::Error,
    node, pod,
    tabs::TabbedView,
    tunnel::Tunnel,
    view::{Element, View},
    Placement, Widget,
};
use crate::{
    events::{Broadcast, Event, Keypress},
    fx::Animated,
};

pub struct Apex {
    view: View,
    top_idx: Rc<RefCell<u16>>,
}

impl Apex {
    pub fn new(client: kube::Client) -> Self {
        let top_idx = Rc::new(RefCell::new(0));

        let tabs = TabbedView::builder()
            .tabs(vec![
                pod::List::tab("Pods".to_string(), client.clone(), true),
                node::List::tab("Nodes".to_string(), client, true),
            ])
            .build();

        let mut widgets = vec![
            Element::builder()
                .widget(Banner::builder().idx(top_idx.clone()).build().boxed())
                .ignore(true)
                .build(),
            Element::builder()
                .widget(
                    Animated::builder()
                        .widget(tabs.boxed())
                        .effect(fx::coalesce(EffectTimer::from_ms(
                            500,
                            Interpolation::CubicOut,
                        )))
                        .build()
                        .boxed(),
                )
                .terminal(true)
                .build(),
            Element::builder()
                .widget(Tunnel::new(top_idx.clone()).boxed())
                .ignore(true)
                .build(),
        ];

        // TODO: This dependency on the crate is unfortunate, it should probably be
        // moved into something like `cata`. See `crate::cli::LEVEL` for an explanation
        // of why this is required instead of using `tracing::enabled!()`.
        if crate::cli::LEVEL.get().unwrap_or(&LevelFilter::ERROR) >= &Level::DEBUG {
            widgets.push(
                Element::builder()
                    .widget(Debug::default().boxed())
                    .ignore(true)
                    .build(),
            );
        }

        Self {
            view: View::builder().widgets(widgets).show_all(true).build(),
            top_idx,
        }
    }
}

impl Widget for Apex {
    #[tracing::instrument(ret(level = Level::TRACE), skip(self, buffer, area), fields(name = self._name()))]
    fn dispatch(&mut self, event: &Event, buffer: &Buffer, area: Rect) -> Result<Broadcast> {
        if let Event::Tunnel(Err(err)) = event {
            self.view.push(Error::from(err.message()).boxed().into());
        }

        Ok(match self.view.dispatch(event, buffer, area)? {
            Broadcast::Ignored => match event.key() {
                Some(Keypress::Printable('?')) => {
                    self.view.push(Help::builder().build().boxed().into());

                    Broadcast::Consumed
                }
                _ => Broadcast::Ignored,
            },
            x => x,
        })
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        *self.top_idx.borrow_mut() = self.view.zindex();

        self.view.draw(frame, area)
    }
}

#[derive(Builder)]
struct Banner {
    idx: Rc<RefCell<u16>>,

    #[builder(default = Style::default().fg(tailwind::GRAY.c200).bg(tailwind::SKY.c700))]
    style: Style,
}

impl Widget for Banner {
    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let inner = area;

        let [logo, help] = Layout::horizontal([Constraint::Fill(0), Constraint::Length(10)])
            .horizontal_margin(1)
            .flex(Flex::SpaceBetween)
            .areas(inner);

        frame.render_widget(
            Block::default().style(self.style).borders(Borders::TOP),
            area,
        );
        frame.render_widget(Text::from("kty >_"), logo);
        frame.render_widget(Text::from("<?> help").alignment(Alignment::Right), help);

        Ok(())
    }

    fn placement(&self) -> super::Placement {
        Placement {
            vertical: super::Constraint::Length(1),
            ..Placement::default()
        }
    }

    fn zindex(&self) -> u16 {
        *self.idx.borrow()
    }
}

#[derive(Builder)]
struct Help {
    #[builder(default = Style::default().bold().fg(tailwind::INDIGO.c300))]
    header_style: Style,
}

impl Widget for Help {
    #[tracing::instrument(ret(level = Level::TRACE), skip_all, fields(name = self._name()))]
    fn dispatch(&mut self, event: &Event, _: &Buffer, _: Rect) -> Result<Broadcast> {
        if event.key().is_some() {
            Ok(Broadcast::Exited)
        } else {
            Ok(Broadcast::Ignored)
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        frame.render_widget(Clear, area);

        let widths = [Constraint::Percentage(25), Constraint::Fill(0)];

        let rows = [
            Row::new(["<ctrl-c>", "Quit"]),
            Row::new(["<ctrl-d> | <esc>", "Close"]),
            Row::new(["<?>", "Help page"]),
            Row::new(["<enter>", "Select row or submit input"]),
            Row::new(["</>", "Filter rows or search content"]),
            Row::new(["<left> | <h>", "Switch tabs or scroll view left"]),
            Row::new(["<right> | <l>", "Switch tabs or scroll view right"]),
            Row::new(["<up> | <k>", "Navigate or scroll up one row"]),
            Row::new(["<down> | <j>", "Navigate or scroll down one row"]),
            Row::new(["<H>", "Navigate or scroll to the beginning"]),
            Row::new(["<L>", "Navigate or scroll to the end"]),
            Row::new(["<ctrl-b> | <b>", "Navigate or scroll up one page"]),
            Row::new(["< > | <f>", "Navigate or scroll down one page"]),
            Row::new(["<ctrl-a> | <^>", "Jump to the beginning of the line"]),
            Row::new(["<ctrl-e> | <$>", "Jump to the end of the line"]),
            Row::new(["<ctrl-k>", "Delete from the cursor to the end of the line"]),
        ];

        let table = Table::new(rows, widths)
            .block(Block::default().borders(Borders::ALL))
            .header(Row::new(vec!["Key", "Action"]).style(self.header_style));

        frame.render_widget(table, area);

        Ok(())
    }

    fn zindex(&self) -> u16 {
        5
    }
}
