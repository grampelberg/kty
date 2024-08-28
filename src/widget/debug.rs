use std::time::Instant;

use eyre::Result;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::Text,
    Frame,
};
use ringbuffer::{AllocRingBuffer, RingBuffer};

use super::Widget;

pub struct Fps {
    start: Instant,
    frames: i32,

    period: AllocRingBuffer<i32>,
}

impl Default for Fps {
    fn default() -> Self {
        let mut period = AllocRingBuffer::new(5);
        period.fill_default();

        Self {
            start: Instant::now(),
            frames: 0,

            period,
        }
    }
}

impl Widget for Fps {
    #[allow(clippy::cast_possible_wrap)]
    #[allow(clippy::cast_possible_truncation)]
    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        self.frames += 1;

        let now = Instant::now();
        let elapsed = (now - self.start).as_secs_f64();
        if elapsed >= 1.0 {
            self.start = now;
            self.period.push(self.frames);
            self.frames = 0;
        }

        let fps = self.period.iter().sum::<i32>() / self.period.len() as i32;

        frame.render_widget(Text::from(format!("FPS: {fps}")), area);

        Ok(())
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
        let [_, area, _] = Layout::vertical([
            Constraint::Fill(0),
            Constraint::Length(self.widgets.len() as u16),
            Constraint::Length(3),
        ])
        .areas(area);

        let component_areas =
            Layout::vertical(vec![Constraint::Length(1); self.widgets.len()]).split(area);

        for (i, widget) in self.widgets.iter_mut().enumerate() {
            widget.draw(frame, component_areas[i])?;
        }

        Ok(())
    }
}
