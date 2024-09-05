mod egress;
mod ingress;

use std::pin::Pin;

use chrono::Utc;
pub use egress::Egress;
use eyre::Result;
use futures::{future::join_all, Future};
pub use ingress::Ingress;
use lazy_static::lazy_static;
use prometheus::{
    histogram_opts, opts, register_histogram_vec, register_int_counter_vec, register_int_gauge_vec,
    HistogramVec, IntCounterVec, IntGaugeVec,
};
use prometheus_static_metric::make_static_metric;
use tokio::io::{AsyncRead, AsyncWrite};

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

struct StreamMetrics<'a> {
    resource: &'a str,
    direction: &'a str,
}

impl StreamMetrics<'_> {
    fn values(&self) -> [&str; 2] {
        [self.resource, self.direction]
    }
}

async fn stream(
    src: (impl AsyncRead + Send, impl AsyncWrite + Send),
    dst: (impl AsyncRead + Send, impl AsyncWrite + Send),
    meta: StreamMetrics<'_>,
) -> Result<()> {
    STREAM_TOTAL_VEC.with_label_values(&meta.values()).inc();
    STREAM_ACTIVE_VEC.with_label_values(&meta.values()).inc();

    let start = Utc::now();

    let src_read = src.0;
    let src_write = src.1;
    tokio::pin!(src_read);
    tokio::pin!(src_write);

    let dst_read = dst.0;
    let dst_write = dst.1;
    tokio::pin!(dst_read);
    tokio::pin!(dst_write);

    let mut bytes = join_all::<Vec<Pin<Box<dyn Future<Output = _> + Send>>>>(vec![
        Box::pin(tokio::io::copy(&mut src_read, &mut dst_write)),
        Box::pin(tokio::io::copy(&mut dst_read, &mut src_write)),
    ])
    .await;

    STREAM_ACTIVE_VEC.with_label_values(&meta.values()).dec();
    STREAM_DURATION.with_label_values(&meta.values()).observe(
        (Utc::now() - start)
            .to_std()
            .expect("duration in range")
            .as_secs_f64(),
    );

    let outgoing = bytes.pop().expect("outgoing bytes")?;
    let incoming = bytes.pop().expect("incoming bytes")?;

    STREAM_BYTES
        .with_label_values(&[meta.resource, meta.direction, "incoming"])
        .inc_by(incoming);
    STREAM_BYTES
        .with_label_values(&[meta.resource, meta.direction, "outgoing"])
        .inc_by(outgoing);

    Ok(())
}
