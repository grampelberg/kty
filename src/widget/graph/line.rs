use bon::Builder;
use itertools::Itertools;
use ratatui::{
    buffer::Buffer,
    layout,
    layout::{Position, Rect},
    style::Style,
    symbols,
    widgets::{RenderDirection, WidgetRef},
};

fn cell_symbol(buffer: &Buffer, pos: Position) -> &str {
    buffer
        .cell((pos.x, pos.y))
        .map_or(" ", |cell| cell.symbol())
}

#[derive(Builder)]
pub struct Line {
    #[builder(default = layout::Direction::Vertical)]
    direction: layout::Direction,
    from: Position,
    to: Position,
    #[builder(default)]
    style: Style,
}

impl Line {
    fn halfway(&self) -> u16 {
        self.to.y.saturating_sub(self.from.y) / 2
    }

    fn horizontal(&self, area: Rect, buffer: &mut Buffer) {
        let (direction, x_range) = if self.from.x < self.to.x {
            (RenderDirection::LeftToRight, self.from.x..=self.to.x)
        } else {
            (RenderDirection::RightToLeft, self.to.x..=self.from.x)
        };

        for (pos, x) in x_range.with_position() {
            let buf_pos = (area.x + x, area.y + self.halfway() + self.from.y);

            let current_symbol = cell_symbol(buffer, buf_pos.into());

            let mut symbol = match pos {
                itertools::Position::First => {
                    if current_symbol == symbols::line::HORIZONTAL {
                        symbols::line::HORIZONTAL_DOWN
                    } else if matches!(direction, RenderDirection::LeftToRight) {
                        symbols::line::BOTTOM_LEFT
                    } else {
                        symbols::line::TOP_LEFT
                    }
                }
                itertools::Position::Last => {
                    if current_symbol == symbols::line::HORIZONTAL {
                        symbols::line::HORIZONTAL_UP
                    } else if matches!(direction, RenderDirection::LeftToRight) {
                        symbols::line::TOP_RIGHT
                    } else {
                        symbols::line::BOTTOM_RIGHT
                    }
                }
                _ => symbols::line::HORIZONTAL,
            };

            // If this is the only thing we're drawing for the horizontal line, then it
            // needs to be vertical.
            if self.from.x == self.to.x {
                symbol = symbols::line::VERTICAL;
            }

            // If there's already a symbol here, we need to pick the correct joining symbol.
            symbol = match current_symbol {
                symbols::line::BOTTOM_RIGHT => symbols::line::HORIZONTAL_UP,
                symbols::line::TOP_RIGHT => symbols::line::HORIZONTAL_DOWN,
                symbols::line::HORIZONTAL_DOWN => {
                    if cell_symbol(buffer, (buf_pos.0, buf_pos.1 - 1).into()) == " " {
                        continue;
                    }

                    symbols::line::CROSS
                }
                symbols::line::HORIZONTAL_UP => {
                    if cell_symbol(buffer, (buf_pos.0, buf_pos.1 + 1).into()) == " " {
                        continue;
                    }

                    symbols::line::CROSS
                }
                _ => symbol,
            };

            let Some(cell) = buffer.cell_mut(buf_pos) else {
                continue;
            };

            cell.set_symbol(symbol).set_style(self.style);
        }
    }

    fn vertical(&self, area: Rect, buffer: &mut Buffer) {
        let halfway = self.halfway();

        for (pos, y) in (self.from.y..self.to.y).with_position() {
            // Leave drawing the middle line entirely to x.
            if y == self.from.y + halfway {
                continue;
            }

            let x = if (y.saturating_sub(self.from.y)) / halfway == 0 {
                self.from.x
            } else {
                self.to.x
            };

            let Some(cell) = buffer.cell_mut((x, area.y + y)) else {
                continue;
            };

            let symbol = match pos {
                itertools::Position::First => symbols::line::HORIZONTAL_DOWN,
                itertools::Position::Last => {
                    if matches!(cell.symbol(), symbols::line::HORIZONTAL | " ") {
                        "▽"
                    } else {
                        // If there isn't a border, skip drawing the last line.
                        continue;
                    }
                }
                _ => match cell.symbol() {
                    symbols::line::HORIZONTAL => {
                        if self.from.x == self.to.x {
                            symbols::line::VERTICAL
                        } else {
                            symbols::line::HORIZONTAL_DOWN
                        }
                    }
                    _ => symbols::line::VERTICAL,
                },
            };

            cell.set_symbol(symbol).set_style(self.style);
        }
    }
}

impl WidgetRef for Line {
    fn render_ref(&self, area: Rect, buffer: &mut Buffer) {
        if matches!(self.direction, layout::Direction::Horizontal) {
            return;
        }

        self.horizontal(area, buffer);
        self.vertical(area, buffer);
    }
}
