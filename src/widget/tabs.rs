use bon::Builder;
use eyre::Result;
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::Text,
    widgets::{Block, Borders},
    Frame,
};
use tachyonfx::{fx, EffectTimer, Interpolation};

use super::{error::Error, view::View, Placement, Widget};
use crate::{
    events::{Broadcast, Event},
    fx::{horizontal_wipe, Animated, Start},
    widget::nav::{move_cursor, Movement},
};

#[derive(Builder)]
pub struct Tab {
    name: String,
    constructor: Box<dyn Fn() -> Box<dyn Widget> + Send>,
}

impl Tab {
    pub fn widget(&self) -> Box<dyn Widget> {
        (self.constructor)()
    }
}

struct Bar {
    items: Vec<String>,
    style: Style,

    idx: usize,
}

impl Bar {
    fn new(items: &[Tab], style: Style) -> Self {
        Self {
            items: items.iter().map(|tab| tab.name.clone()).collect(),
            style,

            idx: 0,
        }
    }
}

impl Widget for Bar {
    fn dispatch(&mut self, event: &Event, _: &Buffer, area: Rect) -> Result<Broadcast> {
        let Some(key) = event.key() else {
            return Ok(Broadcast::Ignored);
        };

        if let Some(Movement::X(x)) = move_cursor(key, area) {
            self.idx = self
                .idx
                .wrapping_add_signed(x.into())
                .clamp(0, self.items.len().saturating_sub(1));

            return Ok(Broadcast::Selected(self.idx));
        }

        Ok(Broadcast::Ignored)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let layout =
            Layout::horizontal(std::iter::repeat(Constraint::Fill(1)).take(self.items.len()))
                .spacing(1)
                .split(area);

        for (i, (area, title)) in layout.iter().zip(self.items.iter()).enumerate() {
            let style = if i == self.idx {
                self.style
            } else {
                Style::default()
            };

            frame.render_widget(
                Text::from(title.as_str())
                    .style(style)
                    .alignment(Alignment::Center),
                *area,
            );
        }

        Ok(())
    }

    fn placement(&self) -> Placement {
        Placement {
            vertical: Constraint::Length(1),
            ..Default::default()
        }
    }
}

pub struct TabbedView {
    items: Vec<Tab>,

    current: usize,
    view: View,
}

#[bon::bon]
impl TabbedView {
    #[builder]
    pub fn new(
        tabs: Vec<Tab>,
        #[builder(default = Style::default().add_modifier(Modifier::REVERSED))] style: Style,
    ) -> Self {
        let mut widgets = vec![
            Bar::new(&tabs, style).boxed(),
            Divider::builder().build().boxed(),
        ];

        if !tabs.is_empty() {
            widgets.push(tabs[0].widget());
        }

        Self {
            view: View::builder().widgets(widgets).build(),
            current: 0,
            items: tabs,
        }
    }

    fn select(&mut self, idx: usize, buffer: &Buffer) {
        let start = if self.current < idx {
            Start::Left
        } else {
            Start::Right
        };

        self.current = idx;

        // TODO: this is *probably* a valid assumption, but it might need to be actually
        // checked.
        self.view.pop();
        self.view.push(
            Animated::builder()
                .widget(self.items[idx].widget())
                .effect(fx::parallel(&[
                    fx::coalesce(EffectTimer::from_ms(500, Interpolation::SineInOut)),
                    horizontal_wipe()
                        .buffer(buffer.clone())
                        .timer(EffectTimer::from_ms(500, Interpolation::SineInOut))
                        .start(start)
                        .call(),
                ]))
                .build()
                .boxed(),
        );
    }
}

impl Widget for TabbedView {
    fn dispatch(&mut self, event: &Event, buffer: &Buffer, area: Rect) -> Result<Broadcast> {
        match self.view.dispatch(event, buffer, area)? {
            Broadcast::Selected(idx) => {
                self.select(idx, buffer);

                Ok(Broadcast::Consumed)
            }
            broadcast => Ok(broadcast),
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        if let Err(err) = self.view.draw(frame, area) {
            self.view.push(Error::from(err).boxed());
        }

        Ok(())
    }
}

#[derive(Builder)]
struct Divider {
    #[builder(default = 0)]
    margin: u16,
}

impl Widget for Divider {
    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let [line, _] =
            Layout::vertical(vec![Constraint::Length(1), Constraint::Length(1)]).areas(area);

        frame.render_widget(Block::default().borders(Borders::BOTTOM), line);

        Ok(())
    }

    fn placement(&self) -> Placement {
        Placement {
            vertical: Constraint::Length(self.margin + 1),
            ..Default::default()
        }
    }
}
