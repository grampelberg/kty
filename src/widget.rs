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

use eyre::Result;
use lazy_static::lazy_static;
use prometheus::{opts, register_int_counter_vec, IntCounterVec};
use prometheus_static_metric::make_static_metric;
use ratatui::{
    layout::{Constraint, Rect},
    widgets::Row,
    Frame,
};
use tokio::{io::AsyncWrite, sync::mpsc::UnboundedReceiver};

use crate::{
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

pub trait Widget: Send {
    fn _name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    fn dispatch(&mut self, _event: &Event) -> Result<Broadcast> {
        Ok(Broadcast::Ignored)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()>;
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
