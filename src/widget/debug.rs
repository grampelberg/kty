use std::{
    iter::once,
    time::{Duration, Instant},
};

use eyre::Result;
use itertools::Itertools;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    widgets::Paragraph,
    Frame,
};
use ringbuffer::{AllocRingBuffer, RingBuffer};

use super::{Placement, Widget};

static RANGE: usize = 30;

pub struct Fps {
    last: Instant,
    period: AllocRingBuffer<Duration>,
}

impl Default for Fps {
    fn default() -> Self {
        let mut period = AllocRingBuffer::new(RANGE);
        period.fill_default();

        Self {
            last: Instant::now(),
            period,
        }
    }
}

impl Widget for Fps {
    #[allow(clippy::cast_possible_wrap)]
    #[allow(clippy::cast_possible_truncation)]
    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let elapsed = self.last.elapsed();
        self.period.push(elapsed);
        self.last = Instant::now();

        frame.render_widget(
            Paragraph::new(format!(
                "FPS: {}\nLast: {}ms",
                self.period.rate(),
                elapsed.as_millis()
            )),
            area,
        );

        Ok(())
    }

    fn placement(&self) -> Placement {
        Placement {
            vertical: Constraint::Length(2),
            ..Default::default()
        }
    }
}

pub struct Debug {
    widgets: Vec<Box<dyn Widget>>,
}

impl Default for Debug {
    fn default() -> Self {
        Self {
            widgets: vec![Box::new(Fps::default())],
        }
    }
}

impl Widget for Debug {
    #[allow(clippy::cast_possible_truncation)]
    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let [_, area, _] = Layout::horizontal([
            Constraint::Fill(0),
            Constraint::Length(10),
            Constraint::Length(3),
        ])
        .areas(area);

        let vertical = Layout::vertical(
            once(Constraint::Fill(0))
                .chain(self.widgets.iter().map(|w| w.placement().vertical))
                .chain(once(Constraint::Length(5)))
                .collect::<Vec<_>>(),
        )
        .split(area);

        let components_areas = vertical.iter().dropping(1).dropping_back(1);

        for (widget, area) in self.widgets.iter_mut().zip(components_areas) {
            widget.draw(frame, *area)?;
        }

        Ok(())
    }

    fn zindex(&self) -> u16 {
        10
    }
}

trait BufferRate {
    fn rate(&self) -> f64;
}

impl BufferRate for AllocRingBuffer<Duration> {
    #[allow(clippy::cast_precision_loss)]
    fn rate(&self) -> f64 {
        RANGE as f64 / self.iter().sum::<Duration>().as_secs_f64()
    }
}
