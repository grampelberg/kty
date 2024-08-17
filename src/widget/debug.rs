use std::time::Instant;

use eyre::Result;
use ratatui::{layout::Rect, text::Text, Frame};
use ringbuffer::{AllocRingBuffer, RingBuffer};

use super::Widget;
use crate::events::{Broadcast, Event, Keypress};

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
    fn dispatch(&mut self, _: &Event) -> Result<Broadcast> {
        Ok(Broadcast::Ignored)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
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
    }
}
