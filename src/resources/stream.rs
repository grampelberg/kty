use std::{pin::Pin, time::Duration};

use chrono::Utc;
use eyre::{eyre, Result};
use futures::{future::join_all, Future};
use k8s_openapi::api::{
    authorization::v1::{ResourceAttributes, SelfSubjectAccessReview, SelfSubjectAccessReviewSpec},
    core::v1::{Node, Pod, Service},
};
use kube::{api::PostParams, core::ErrorResponse, Api, Resource};
use lazy_static::lazy_static;
use prometheus::{
    histogram_opts, opts, register_histogram_vec, register_int_counter_vec, register_int_gauge_vec,
    HistogramVec, IntCounterVec, IntGaugeVec,
};
use prometheus_static_metric::make_static_metric;
use russh::server::{self};
use tokio::net::TcpStream;

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

static CONNECT_TIMEOUT: Duration = Duration::from_secs(1);

struct Host {
    client: kube::Client,
    resource: String,
    segments: Vec<String>,
}

impl Host {
    fn new(client: kube::Client, host: &str) -> Result<Self> {
        let segments: Vec<String> = host
            .split('/')
            .map(std::string::ToString::to_string)
            .collect();

        Ok(Self {
            client,
            resource: match segments
                .first()
                .map(std::string::String::as_str)
                .ok_or_else(|| eyre!("resource not provided"))?
            {
                "pods" | "pod" | "po" => "pods".to_string(),
                "services" | "service" | "svc" => "services".to_string(),
                "nodes" | "node" | "no" => "nodes".to_string(),
                _ => return Err(eyre!("resource not supported")),
            },
            segments,
        })
    }

    fn resource(&self) -> &str {
        self.resource.as_str()
    }

    async fn addr(&self) -> Result<String> {
        match self.resource() {
            "pods" => Pod::get_host(self.client.clone(), &self.segments[1..]).await,
            "services" => Service::get_host(self.client.clone(), &self.segments[1..]).await,
            "nodes" => Node::get_host(self.client.clone(), &self.segments[1..]).await,
            x => Err(eyre!("resource {x} not supported")),
        }
    }
}

async fn access(client: kube::Client, attrs: ResourceAttributes) -> Result<bool> {
    let access = Api::<SelfSubjectAccessReview>::all(client)
        .create(
            &PostParams::default(),
            &SelfSubjectAccessReview {
                spec: SelfSubjectAccessReviewSpec {
                    resource_attributes: Some(attrs),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .await?;

    Ok(access.status.map_or(false, |status| status.allowed))
}

pub async fn direct(
    mut channel: russh::Channel<server::Msg>,
    client: kube::Client,
    host: String,
    port: u16,
) -> Result<()> {
    let start = Utc::now();

    let lookup = Host::new(client.clone(), host.as_str())?;
    STREAM_TOTAL_VEC
        .with_label_values(&[lookup.resource(), "ingress"])
        .inc();
    tracing::debug!(
        resource = lookup.resource(),
        direction = "ingress",
        activity = "forward::tcpip",
        "connection",
    );

    let addr = lookup.addr().await?;

    let mut stream =
        tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((addr.as_str(), port)))
            .await
            .map_err(|_| {
                eyre!(
                    "connect to {addr}:{port} timed out after {}s",
                    CONNECT_TIMEOUT.as_secs_f32()
                )
            })?
            .map_err(|e| eyre!(e).wrap_err(format!("connect to {addr}:{port} failed")))?;

    tracing::debug!("connected to {}:{}", host, port);

    STREAM_ACTIVE_VEC
        .with_label_values(&[lookup.resource(), "ingress"])
        .inc();

    let (mut dest_read, mut dest_write) = stream.split();
    let mut src_write = channel.make_writer();
    let mut src_read = channel.make_reader();

    let mut bytes = join_all::<Vec<Pin<Box<dyn Future<Output = _> + Send>>>>(vec![
        Box::pin(tokio::io::copy(&mut src_read, &mut dest_write)),
        Box::pin(tokio::io::copy(&mut dest_read, &mut src_write)),
    ])
    .await;

    STREAM_ACTIVE_VEC
        .with_label_values(&[lookup.resource(), "ingress"])
        .dec();
    STREAM_DURATION
        .with_label_values(&[lookup.resource(), "ingress"])
        .observe(
            (Utc::now() - start)
                .to_std()
                .expect("duration in range")
                .as_secs_f64(),
        );

    let outgoing = bytes.pop().expect("outgoing bytes")?;
    let incoming = bytes.pop().expect("incoming bytes")?;

    STREAM_BYTES
        .with_label_values(&[lookup.resource(), "ingress", "incoming"])
        .inc_by(incoming);
    STREAM_BYTES
        .with_label_values(&[lookup.resource(), "ingress", "outgoing"])
        .inc_by(outgoing);

    tracing::debug!("connection lost for {}:{}", host, port);

    Ok(())
}

trait Proxy {
    async fn get_host(client: kube::Client, segments: &[String]) -> Result<String>;
}

impl Proxy for Pod {
    async fn get_host(client: kube::Client, segments: &[String]) -> Result<String> {
        let typ = Self::plural(&());
        let format = format!("format is {typ}/<namespace>/<name>");

        let Some(namespace) = segments.first() else {
            return Err(eyre!(format).wrap_err("namespace not provided"));
        };

        let Some(name) = segments.get(1) else {
            return Err(eyre!(format).wrap_err("name not provided"));
        };

        let path = segments.join("/");

        if !access(
            client.clone(),
            ResourceAttributes {
                resource: Some(format!("{typ}/proxy")),
                verb: Some("create".to_string()),
                namespace: Some((*namespace).to_string()),
                name: Some((*name).to_string()),
                ..Default::default()
            },
        )
        .await?
        {
            return Err(eyre!("grant `create` for `{typ}/proxy`")
                .wrap_err(format!("proxy for {path} is forbidden.")));
        }

        match Api::<Pod>::namespaced(client, namespace)
            .get_opt(name)
            .await
        {
            Ok(Some(pod)) => pod
                .status
                .ok_or(eyre!("{path} not running"))?
                .pod_ip
                .ok_or(eyre!("{path} ip not available")),
            Ok(None) => Err(eyre!("{path} not found")),
            Err(kube::Error::Api(ErrorResponse { code: 403, .. })) => {
                Err(eyre!("grant `get` for `{typ}` to proxy")
                    .wrap_err(format!("get forbidden for {path}")))
            }
            Err(kube::Error::Api(e)) => {
                Err(eyre!(e.message).wrap_err(format!("failed getting {typ}")))
            }
            Err(e) => Err(eyre!(e).wrap_err(format!("failed getting {typ}"))),
        }
    }
}

impl Proxy for Service {
    async fn get_host(client: kube::Client, segments: &[String]) -> Result<String> {
        let typ = Self::plural(&());
        let format = format!("format is {typ}/<namespace>/<name>");

        let Some(namespace) = segments.first() else {
            return Err(eyre!(format).wrap_err("namespace not provided"));
        };

        let Some(name) = segments.get(1) else {
            return Err(eyre!(format).wrap_err("name not provided"));
        };

        let path = segments.join("/");

        if !access(
            client.clone(),
            ResourceAttributes {
                resource: Some(format!("{typ}/proxy")),
                verb: Some("create".to_string()),
                namespace: Some((*namespace).to_string()),
                name: Some((*name).to_string()),
                ..Default::default()
            },
        )
        .await?
        {
            return Err(eyre!("grant `create` for `{typ}/proxy`")
                .wrap_err(format!("proxy for {path} is forbidden.")));
        }

        match Api::<Service>::namespaced(client, namespace)
            .get_opt(name)
            .await
        {
            Ok(Some(_)) => Ok([name, namespace, "svc"].join(".")),
            Ok(None) => Err(eyre!("{path} not found")),
            Err(kube::Error::Api(ErrorResponse { code: 403, .. })) => {
                Err(eyre!("grant `get` for `{typ}` to proxy")
                    .wrap_err(format!("get forbidden for {path}")))
            }
            Err(kube::Error::Api(e)) => {
                Err(eyre!(e.message).wrap_err(format!("failed getting {typ}")))
            }
            Err(e) => Err(eyre!(e).wrap_err(format!("failed getting {typ}"))),
        }
    }
}

impl Proxy for Node {
    async fn get_host(client: kube::Client, segments: &[String]) -> Result<String> {
        let typ = Self::plural(&());
        let format = format!("format is {typ}/<name>");

        let Some(name) = segments.first() else {
            return Err(eyre!(format).wrap_err("name not provided"));
        };

        let path = segments.join("/");

        if !access(
            client.clone(),
            ResourceAttributes {
                resource: Some(format!("{typ}/proxy")),
                verb: Some("create".to_string()),
                name: Some((*name).to_string()),
                ..Default::default()
            },
        )
        .await?
        {
            return Err(eyre!("grant `create` for `{typ}/proxy`")
                .wrap_err(format!("proxy for {path} is forbidden.")));
        }

        match Api::<Node>::all(client).get_opt(name).await {
            Ok(Some(node)) => Ok(node
                .status
                .ok_or(eyre!("{path} missing status"))?
                .addresses
                .ok_or(eyre!("{path} missing addresses"))?
                .iter()
                .find(|a| a.type_ == "InternalIP")
                .ok_or(eyre!("{path} missing internal ip"))?
                .address
                .clone()),
            Ok(None) => Err(eyre!("{path} not found")),
            Err(kube::Error::Api(ErrorResponse { code: 403, .. })) => {
                Err(eyre!("grant `get` for `{typ}` to proxy")
                    .wrap_err(format!("get forbidden for {path}")))
            }
            Err(kube::Error::Api(e)) => {
                Err(eyre!(e.message).wrap_err(format!("failed getting {typ}")))
            }
            Err(e) => Err(eyre!(e).wrap_err(format!("failed getting {typ}"))),
        }
    }
}
