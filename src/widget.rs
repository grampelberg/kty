pub mod apex;
pub mod debug;
pub mod error;
pub mod input;
pub mod loading;
pub mod log;
pub mod pod;
pub mod table;
pub mod tabs;
pub mod tunnel;
pub mod yaml;

use std::pin::Pin;

use bon::builder;
use eyre::{eyre, Result};
use itertools::Itertools;
use lazy_static::lazy_static;
use prometheus::{opts, register_int_counter_vec, IntCounterVec};
use prometheus_static_metric::make_static_metric;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    widgets::Row,
    Frame,
};
use tachyonfx::{Effect, EffectRenderer, Shader};
use tokio::{io::AsyncWrite, sync::mpsc::UnboundedReceiver};

use crate::{
    dashboard::RENDER_INTERVAL,
    events::{Broadcast, Event},
    widget::table::RowStyle,
};

make_static_metric! {
    pub struct WidgetVec: IntCounter {
        "resource" => {
            container,
            pod,
        },
        "type" => {
            cmd,
            detail,
            exec,
            list,
            log,
            yaml,
        },
    }
}

lazy_static! {
    pub static ref WIDGET_VIEWS_VEC: IntCounterVec = register_int_counter_vec!(
        opts!(
            "widget_views_total",
            "Number of times a widget has been viewed",
        ),
        &["resource", "type"],
    )
    .unwrap();
    pub static ref WIDGET_VIEWS: WidgetVec = WidgetVec::from(&WIDGET_VIEWS_VEC);
}

pub trait TableRow<'a> {
    fn constraints() -> Vec<Constraint>;

    fn row(&self, style: &RowStyle) -> Row;
    fn header() -> Option<Row<'a>> {
        None
    }
}

#[builder]
pub struct Placement {
    #[builder(default = Constraint::Length(0))]
    pub horizontal: Constraint,
    #[builder(default = Constraint::Length(0))]
    pub vertical: Constraint,
}

impl Default for Placement {
    fn default() -> Self {
        Self::builder().build()
    }
}

pub trait Widget {
    fn _name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    fn dispatch(&mut self, _event: &Event) -> Result<Broadcast> {
        Ok(Broadcast::Ignored)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()>;

    fn placement(&self) -> Placement {
        Placement::default()
    }

    fn zindex(&self) -> u16 {
        0
    }

    fn boxed(self) -> Box<dyn Widget>
    where
        Self: Sized + 'static,
    {
        Box::new(self)
    }
}

impl std::fmt::Debug for Box<dyn Widget> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(format!("Box<dyn Widget<{}>>", self._name()).as_str())
            .finish()
    }
}

#[async_trait::async_trait]
pub trait Raw: Send {
    fn _name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    async fn start(
        &mut self,
        stdin: &mut UnboundedReceiver<Event>,
        mut stdout: Pin<Box<dyn AsyncWrite + Send + Unpin>>,
    ) -> Result<()>;
}

impl std::fmt::Debug for Box<dyn Raw> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(format!("Box<dyn Raw<{}>>", self._name()).as_str())
            .finish()
    }
}

pub trait Container {
    fn effects(&mut self) -> &mut Vec<Effect>;
    fn widgets(&mut self) -> &mut Vec<Box<dyn Widget>>;
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast>;
}

impl<T> Widget for T
where
    T: Container,
{
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        propagate!(<T as Container>::dispatch(self, event));

        for (i, widget) in self.widgets().iter_mut().enumerate().rev() {
            propagate!(widget.dispatch(event), {
                if i == 0 {
                    return Ok(Broadcast::Exited);
                }
                self.widgets().remove(i);
                self.effects().reset();
            });
        }

        Ok(Broadcast::Ignored)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let chunks = self.widgets().iter_mut().chunk_by(|widget| widget.zindex());

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
            widget.draw(frame, *area)?;
        }

        for effect in self.effects().iter_mut().filter(|effect| effect.running()) {
            frame.render_effect(effect, area, RENDER_INTERVAL.into());
        }

        Ok(())
    }
}

trait ResetEffect {
    fn reset(&mut self);
}

impl ResetEffect for Vec<Effect> {
    fn reset(&mut self) {
        self.iter_mut().for_each(Shader::reset);
    }
}

/// Handle propagation of events from calls to `dispatch()`. This macro returns
/// immediately if the event is used (eg consumed). Pass an expression as the
/// second argument to handle (and consume) child components that exit.
#[macro_export]
macro_rules! propagate {
    ($fn:expr) => {
        let result = $fn?;
        match result {
            Broadcast::Ignored => {}
            _ => return Ok(result),
        }
    };
    ($fn:expr, $exit:expr) => {
        let result = $fn?;
        match result {
            Broadcast::Exited => {
                $exit;

                return Ok(Broadcast::Consumed);
            }
            Broadcast::Ignored => {}
            _ => return Ok(result),
        }
    };
}

pub use propagate;
