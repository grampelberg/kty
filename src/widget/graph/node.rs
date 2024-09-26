use bon::Builder;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::Style,
    text::Text,
    widgets::{
        block::{self, Title},
        Block, Borders, StatefulWidgetRef, WidgetRef,
    },
};

#[derive(Builder)]
pub struct Node<'a> {
    text: Text<'a>,
    #[builder(default = Borders::NONE)]
    borders: Borders,
    #[builder(default)]
    style: Style,
    #[builder(default)]
    selected_style: Style,
    titles: Vec<Title<'a>>,
    constraint: Option<Constraint>,
}

impl Node<'_> {
    pub fn constraint(&self) -> Constraint {
        self.constraint.unwrap_or(Constraint::Length(self.width()))
    }

    pub fn borders(&self) -> Borders {
        self.borders
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn height(&self) -> u16 {
        let mut y = 0;

        if self.borders.contains(Borders::TOP) {
            y += 1;
        }

        if self.borders.contains(Borders::BOTTOM) {
            y += 1;
        }

        self.text.height() as u16 + y
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn width(&self) -> u16 {
        let mut x = 0;

        if self.borders.contains(Borders::LEFT) {
            x += 1;
        }

        if self.borders.contains(Borders::RIGHT) {
            x += 1;
        }

        let width = self
            .titles
            .iter()
            .fold([0, 0], |mut total, title| {
                match title.position {
                    Some(block::Position::Top) | None => {
                        total[0] += title.content.width() as u16;
                    }
                    Some(block::Position::Bottom) => {
                        total[1] += title.content.width() as u16;
                    }
                }

                total
            })
            .into_iter()
            .max()
            .unwrap_or(0);

        width.max(self.text.width() as u16) + x
    }
}

impl StatefulWidgetRef for Node<'_> {
    type State = bool;

    fn render_ref(&self, area: Rect, buffer: &mut Buffer, selected: &mut Self::State) {
        let style = if *selected {
            self.selected_style
        } else {
            self.style
        };

        let area = if matches!(self.borders, Borders::NONE) {
            area
        } else {
            let block = self
                .titles
                .iter()
                .fold(Block::new().borders(self.borders), |block, title| {
                    block.title(title.clone())
                })
                .style(style);

            block.render_ref(area, buffer);

            block.inner(area)
        };

        self.text.clone().style(style).render_ref(area, buffer);
    }
}
