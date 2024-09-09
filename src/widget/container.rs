use eyre::{eyre, Result};
use itertools::Itertools;
use ratatui::{
    layout::{Layout, Rect},
    Frame,
};
use tachyonfx::{Effect, EffectRenderer};

use super::{propagate, EffectExt, Placement, Renderable, StatefulWidget, Widget};
use crate::{
    dashboard::RENDER_INTERVAL,
    events::{Broadcast, Event},
};

#[derive(Debug)]
pub enum Mode<S> {
    Stateless(Box<dyn Widget>),
    Stateful(Box<dyn StatefulWidget<State = S>>),
}

impl<S> Mode<S> {
    pub fn dispatch(&mut self, event: &Event, state: &mut S) -> Result<Broadcast> {
        match self {
            Mode::Stateless(widget) => widget.dispatch(event),
            Mode::Stateful(widget) => widget.dispatch(event, state),
        }
    }

    pub fn draw(&mut self, frame: &mut Frame, area: Rect, state: &mut S) -> Result<()> {
        match self {
            Mode::Stateless(widget) => widget.draw(frame, area),
            Mode::Stateful(widget) => widget.draw(frame, area, state),
        }
    }

    pub fn placement(&self) -> Placement {
        match self {
            Mode::Stateless(widget) => widget.placement(),
            Mode::Stateful(widget) => widget.placement(),
        }
    }

    pub fn zindex(&self) -> u16 {
        match self {
            Mode::Stateless(widget) => widget.zindex(),
            Mode::Stateful(widget) => widget.zindex(),
        }
    }
}

pub struct Contents<'a, S> {
    pub effects: &'a mut Vec<Effect>,
    pub widgets: &'a mut Vec<Mode<S>>,
}

pub trait Container {
    type State;

    fn dispatch(&mut self, event: &Event, state: &mut Self::State) -> Result<Broadcast> {
        self.dispatch_children(event, state)
    }

    fn dispatch_children(&mut self, event: &Event, state: &mut Self::State) -> Result<Broadcast> {
        let Contents { effects, widgets } = self.contents();

        for (i, widget) in widgets.iter_mut().enumerate().rev() {
            propagate!(widget.dispatch(event, state), {
                if i == 0 {
                    return Ok(Broadcast::Exited);
                }
                widgets.remove(i);

                effects.reset();
            });
        }

        Ok(Broadcast::Ignored)
    }

    // This is a workaround for the borrow checker. If each component of Contents
    // was a separate function call, it would count as a separate borrow of `&mut
    // self`. Because this takes separate borrows for each of the potential struct
    // members, it gets that all into a *single* borrow of `&mut self`.
    fn contents(&mut self) -> Contents<Self::State>;
}

impl<C, S> StatefulWidget for C
where
    C: Container<State = S> + Renderable,
{
    type State = S;

    fn dispatch(&mut self, event: &Event, state: &mut Self::State) -> Result<Broadcast> {
        propagate!(<C as Container>::dispatch(self, event, state));

        Ok(Broadcast::Ignored)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, state: &mut Self::State) -> Result<()> {
        let Contents { effects, widgets } = self.contents();

        let chunks = widgets.iter_mut().chunk_by(|widget| widget.zindex());

        let Some((_, layer)) = chunks
            .into_iter()
            .sorted_by(|(a, _), (b, _)| a.cmp(b))
            .last()
        else {
            return Err(eyre!("no widgets to draw"));
        };

        let layer: Vec<_> = layer.collect();

        let areas =
            Layout::vertical(layer.iter().map(|widget| widget.placement().vertical)).split(area);

        for (widget, area) in layer.into_iter().zip(areas.iter()) {
            widget.draw(frame, *area, state)?;
        }

        for effect in effects.running() {
            frame.render_effect(effect, area, RENDER_INTERVAL.into());
        }

        Ok(())
    }
}
