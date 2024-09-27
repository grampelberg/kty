use bon::Builder;
use eyre::Result;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders},
    Frame,
};
use tachyonfx::{fx, EffectTimer, Interpolation};

use super::{
    error::Error,
    view::{Element, View},
    Placement, Widget,
};
use crate::{
    events::{Broadcast, Event},
    fx::{horizontal_wipe, Start},
    widget::nav::{move_cursor, Movement, Shrink},
};

#[derive(Builder)]
pub struct Tab {
    name: String,
    constructor: Box<dyn Fn() -> Element + Send>,
}

impl Tab {
    pub fn widget(&self) -> Element {
        (self.constructor)()
    }
}

struct Bar {
    items: Vec<String>,
    title: Vec<String>,
    style: Style,

    idx: usize,
}

#[bon::bon]
impl Bar {
    #[builder]
    fn new(items: &[Tab], style: Style, title: Vec<String>) -> Self {
        Self {
            items: items.iter().map(|tab| tab.name.clone()).collect(),
            title,
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
                .wrapping_add_signed(x.shrink())
                .clamp(0, self.items.len().saturating_sub(1));

            return Ok(Broadcast::Selected(self.idx));
        }

        Ok(Broadcast::Ignored)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let border = Block::default()
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .title(Line::from(
                itertools::Itertools::intersperse(
                    self.title.iter().map(|s| Span::from(s.as_str())),
                    Span::from(" → ").style(Style::default().add_modifier(Modifier::BOLD)),
                )
                .collect::<Vec<_>>(),
            ));

        let layout =
            Layout::horizontal(std::iter::repeat(Constraint::Fill(1)).take(self.items.len()))
                .spacing(1)
                .split(border.inner(area));

        for (i, (area, txt)) in layout.iter().zip(self.items.iter()).enumerate() {
            let style = if i == self.idx {
                self.style
            } else {
                Style::default()
            };

            frame.render_widget(Text::from(txt.as_str()).style(style).centered(), *area);
        }

        frame.render_widget(border, area);

        Ok(())
    }

    fn placement(&self) -> Placement {
        Placement {
            vertical: Constraint::Length(2),
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
        #[builder(default = Vec::new())] title: Vec<String>,
    ) -> Self {
        let mut widgets = vec![Bar::builder()
            .items(&tabs)
            .style(style)
            .title(title)
            .build()
            .boxed()
            .into()];

        if !tabs.is_empty() {
            widgets.push(tabs[0].widget());
        }

        Self {
            items: tabs,

            current: 0,
            view: View::builder().widgets(widgets).build(),
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
            self.items[idx].widget().animate(fx::parallel(&[
                fx::coalesce(EffectTimer::from_ms(500, Interpolation::SineInOut)),
                horizontal_wipe()
                    .buffer(buffer.clone())
                    .timer(EffectTimer::from_ms(500, Interpolation::SineInOut))
                    .start(start)
                    .call(),
            ])),
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
            self.view.push(Error::from(err).boxed().into());
        }

        Ok(())
    }

    fn zindex(&self) -> u16 {
        self.view.zindex()
    }
}
