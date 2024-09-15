use bon::{builder, Builder};
use eyre::Result;
use ndarray::{s, ArrayViewMut2};
use ratatui::{buffer::Buffer, layout::Rect, Frame};
use tachyonfx::{CellFilter, CellIterator, Duration, Effect, EffectRenderer, EffectTimer, Shader};

use crate::{
    dashboard::RENDER_INTERVAL,
    events::{Broadcast, Event},
    widget::{BoxWidget, Placement, Widget},
};

#[derive(Builder)]
pub struct Animated {
    effect: Option<Effect>,
    widget: BoxWidget,
}

impl Widget for Animated {
    fn dispatch(&mut self, event: &Event, buffer: &Buffer, area: Rect) -> Result<Broadcast> {
        self.widget.dispatch(event, buffer, area)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        self.widget.draw(frame, area)?;

        if let Some(effect) = &mut self.effect {
            if !effect.running() {
                self.effect = None;

                return Ok(());
            }

            frame.render_effect(effect, area, RENDER_INTERVAL.into());
        }

        Ok(())
    }

    fn placement(&self) -> Placement {
        self.widget.placement()
    }

    fn zindex(&self) -> u16 {
        self.widget.zindex()
    }
}

#[derive(Clone, Default)]
pub enum Start {
    Left,
    #[default]
    Right,
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

#[builder]
pub fn horizontal_wipe<T: Into<EffectTimer>>(
    timer: T,
    buffer: Buffer,
    #[builder(default)] start: Start,
) -> Effect {
    Effect::new(
        Wipe::builder()
            .timer(timer.into())
            .previous(buffer)
            .start(start)
            .build(),
    )
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

        let window = ((width as f32 * alpha).round() as usize).clamp(0, width);

        if window == width {
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
            Start::Left => s![y..height, x + window..x + width],
            Start::Right => s![y..y + height, x..x + width - window],
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
