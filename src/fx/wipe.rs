use bon::{builder, Builder};
use ndarray::{s, ArrayViewMut2};
use ratatui::{buffer::Buffer, layout::Rect};
use tachyonfx::{CellFilter, CellIterator, Duration, EffectTimer, Shader};

#[derive(Clone, Default)]
pub enum Start {
    Left,
    #[default]
    Right,
    #[allow(dead_code)]
    Top,
    #[allow(dead_code)]
    Bottom,
}

/// Wipes content in from the start edge, uses `previous` as the source and then
/// the current buffer as the destination.
#[derive(Builder, Clone)]
pub struct Wipe {
    #[builder(into)]
    timer: EffectTimer,
    #[builder(default)]
    start: Start,
    previous: Buffer,
    #[builder(default)]
    done: bool,
}

// This assumes that the area from the original buffer to the new one doesn't
// change. If it is unable to create a slice correctly, it'll just give up.
impl Shader for Wipe {
    fn name(&self) -> &'static str {
        "slider"
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn process(&mut self, duration: Duration, buf: &mut Buffer, area: Rect) -> Option<Duration> {
        let (overflow, alpha) = self
            .timer_mut()
            .map_or((None, 1.0), |t| (t.process(duration), t.alpha()));

        let x = area.x as usize;
        let y = area.y as usize;
        let width = area.width as usize;
        let height = area.height as usize;
        let buffer_width = self.previous.area.width as usize;
        let buffer_height = self.previous.area.height as usize;

        let axis = match self.start {
            Start::Left | Start::Right => width,
            Start::Top | Start::Bottom => height,
        };

        let window = ((axis as f32 * alpha).round() as usize).clamp(0, axis);

        if window == axis {
            self.done = true;
        }

        let Ok(previous) =
            ArrayViewMut2::from_shape((buffer_height, buffer_width), &mut self.previous.content)
        else {
            tracing::debug!(area = ?area, "unable to create view from previous buffer");

            self.done = true;
            return overflow;
        };

        let Ok(mut next) =
            ArrayViewMut2::from_shape((buffer_height, buffer_width), &mut buf.content)
        else {
            tracing::debug!(area = ?area, "unable to create view from next buffer");

            self.done = true;
            return overflow;
        };

        let slice = match self.start {
            Start::Left => s![y..y + height, x + window..x + width],
            Start::Right => s![y..y + height, x..x + width - window],
            Start::Top => s![y + window..y + height, x..x + width],
            Start::Bottom => s![y..y + height - window, x..x + width],
        };

        let previous_section = previous.slice(slice);

        next.slice_mut(slice).assign(&previous_section);

        overflow
    }

    fn execute(&mut self, _: f32, _: Rect, _: CellIterator<'_>) {}

    fn done(&self) -> bool {
        self.done
    }

    fn clone_box(&self) -> Box<dyn Shader> {
        Box::new(self.clone())
    }

    fn area(&self) -> Option<Rect> {
        None
    }

    fn set_area(&mut self, _area: Rect) {}

    fn set_cell_selection(&mut self, _: CellFilter) {}

    fn timer_mut(&mut self) -> Option<&mut EffectTimer> {
        Some(&mut self.timer)
    }

    fn timer(&self) -> Option<EffectTimer> {
        Some(self.timer)
    }
}
