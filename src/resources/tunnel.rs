mod egress;
mod ingress;

use std::hash::{Hash, Hasher};

use chrono::Utc;
use derive_builder::Builder;
pub use egress::EgressBuilder;
use eyre::{Report, Result};
pub use ingress::Ingress;
use lazy_static::lazy_static;
use prometheus::{
    histogram_opts, opts, register_histogram_vec, register_int_counter_vec, register_int_gauge_vec,
    HistogramVec, IntCounterVec, IntGaugeVec,
};
use prometheus_static_metric::make_static_metric;
use ratatui::{layout::Constraint, widgets::Row};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::widget::table;

make_static_metric! {
    pub struct ResourceVec: IntCounter {
        "resource" => {
            pod,
            service,
            node,
        },
        "direction" => {
            ingress,
            egress,
        }
    }
    pub struct ResourceGaugeVec: IntGauge {
        "resource" => {
            pod,
            service,
            node,
        },
        "direction" => {
            ingress,
            egress,
        }
    }
}

lazy_static! {
    static ref STREAM_DURATION: HistogramVec = register_histogram_vec!(
        histogram_opts!(
            "stream_duration_seconds",
            "Stream duration",
            vec!(0.1, 0.2, 0.3, 0.5, 0.8, 1.3, 2.1),
        ),
        &["resource", "direction"]
    )
    .unwrap();
    static ref STREAM_BYTES: IntCounterVec = register_int_counter_vec!(
        opts!(
            "stream_bytes_total",
            "Total number of bytes streamed by resource and direction"
        ),
        &["resource", "direction", "destination"]
    )
    .unwrap();
    static ref STREAM_TOTAL_VEC: IntCounterVec = register_int_counter_vec!(
        opts!(
            "stream_total",
            "Total number of streams by resource and direction"
        ),
        &["resource", "direction"]
    )
    .unwrap();
    static ref STREAM_TOTAL: ResourceVec = ResourceVec::from(&STREAM_TOTAL_VEC);
    static ref STREAM_ACTIVE_VEC: IntGaugeVec = register_int_gauge_vec!(
        opts!(
            "stream_active",
            "Number of active streams by resource and direction"
        ),
        &["resource", "direction"]
    )
    .unwrap();
    static ref STREAM_ACTIVE: ResourceGaugeVec = ResourceGaugeVec::from(&STREAM_ACTIVE_VEC);
}

#[derive(Clone, Debug, Builder)]
pub struct Tunnel {
    host: String,
    port: u16,
    kind: Kind,
    pub lifecycle: Lifecycle,
}

impl Tunnel {
    fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    pub fn into_active(mut self) -> Self {
        self.lifecycle = Lifecycle::Active;

        self
    }

    pub fn into_inactive(mut self) -> Self {
        self.lifecycle = Lifecycle::Inactive;

        self
    }

    pub fn into_listening(mut self) -> Self {
        self.lifecycle = Lifecycle::Listening;

        self
    }

    pub fn into_error(mut self) -> Self {
        self.lifecycle = Lifecycle::Error;

        self
    }
}

impl std::fmt::Display for Tunnel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}<{}>", self.kind, self.addr())
    }
}

impl Hash for Tunnel {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.addr().hash(state);
        self.kind.hash(state);
    }
}

impl PartialEq for Tunnel {
    fn eq(&self, other: &Self) -> bool {
        self.host == other.host && self.port == other.port && self.kind == other.kind
    }
}

impl Eq for Tunnel {}

impl PartialOrd for Tunnel {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Tunnel {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.addr().cmp(&other.addr())
    }
}

impl table::Row for Tunnel {
    fn constraints() -> Vec<Constraint> {
        vec![
            Constraint::Length(10),
            Constraint::Fill(0),
            Constraint::Length(15),
        ]
    }

    fn row(&self, style: &table::RowStyle) -> Row {
        Row::new(vec![
            self.kind.to_string().to_lowercase(),
            format!("{}:{}", self.host, self.port),
            self.lifecycle.to_string(),
        ])
        .style(match self.lifecycle {
            Lifecycle::Active => style.healthy,
            Lifecycle::Inactive | Lifecycle::Listening => style.normal,
            Lifecycle::Error => style.unhealthy,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Error {
    error: String,
    pub tunnel: Tunnel,
}

impl Error {
    pub fn new(error: &Report, tunnel: Tunnel) -> Self {
        Self {
            error: format!("{error:?}"),
            tunnel,
        }
    }

    pub fn message(&self) -> String {
        self.error.clone()
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.tunnel, self.error)
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, strum::Display)]
pub enum Kind {
    Ingress,
    Egress,
}

#[derive(Clone, Debug, strum::Display)]
pub enum Lifecycle {
    Active,
    Inactive,
    Listening,
    Error,
}

#[derive(Debug)]
struct StreamMetrics<'a> {
    resource: &'a str,
    direction: &'a str,
}

impl StreamMetrics<'_> {
    fn values(&self) -> [&str; 2] {
        [self.resource, self.direction]
    }
}

#[tracing::instrument(skip_all)]
async fn stream(
    mut src: impl AsyncRead + AsyncWrite + Unpin + Send,
    mut dst: impl AsyncRead + AsyncWrite + Unpin + Send,
    meta: StreamMetrics<'_>,
) -> Result<()> {
    STREAM_TOTAL_VEC.with_label_values(&meta.values()).inc();
    STREAM_ACTIVE_VEC.with_label_values(&meta.values()).inc();

    let start = Utc::now();

    let (incoming, outgoing) = tokio::io::copy_bidirectional(&mut src, &mut dst).await?;

    STREAM_ACTIVE_VEC.with_label_values(&meta.values()).dec();
    STREAM_DURATION.with_label_values(&meta.values()).observe(
        (Utc::now() - start)
            .to_std()
            .expect("duration in range")
            .as_secs_f64(),
    );

    STREAM_BYTES
        .with_label_values(&[meta.resource, meta.direction, "incoming"])
        .inc_by(incoming);
    STREAM_BYTES
        .with_label_values(&[meta.resource, meta.direction, "outgoing"])
        .inc_by(outgoing);

    Ok(())
}
