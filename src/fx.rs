pub mod wipe;

use bon::{builder, Builder};
use eyre::Result;
use ratatui::{buffer::Buffer, layout::Rect, Frame};
use tachyonfx::{Effect, EffectRenderer, EffectTimer, Shader};
pub use wipe::Start;

use crate::{
    dashboard::render_interval,
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

            frame.render_effect(effect, area, render_interval().into());
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

#[builder]
pub fn wipe<T: Into<EffectTimer>>(
    timer: T,
    buffer: Buffer,
    #[builder(default)] start: Start,
) -> Effect {
    Effect::new(
        wipe::Wipe::builder()
            .timer(timer.into())
            .previous(buffer)
            .start(start)
            .build(),
    )
}
