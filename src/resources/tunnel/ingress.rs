use std::time::Duration;

use eyre::{eyre, Result};
use k8s_openapi::api::{
    authorization::v1::{ResourceAttributes, SelfSubjectAccessReview, SelfSubjectAccessReviewSpec},
    core::v1::{Node, Pod, Service},
};
use kube::{api::PostParams, core::ErrorResponse, Api, Resource};
use russh::server::{self};
use tokio::net::TcpStream;

use super::{stream, StreamMetrics};

static CONNECT_TIMEOUT: Duration = Duration::from_secs(1);

pub struct Ingress {
    host: Host,
    port: u16,
}

impl std::fmt::Display for Ingress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.host, self.port)
    }
}

impl Ingress {
    pub fn new(host: &str, port: u16) -> Result<Self> {
        Ok(Self {
            host: Host::new(host)?,
            port,
        })
    }

    pub fn host(&self) -> String {
        self.host.to_string()
    }

    pub async fn run(
        &self,
        client: kube::Client,
        channel: russh::Channel<server::Msg>,
    ) -> Result<()> {
        tracing::debug!(
            resource = self.host.resource(),
            direction = "ingress",
            activity = "tunnel::ingress",
            "connection",
        );

        let addr = self.host.addr(client.clone()).await?;

        let remote = tokio::time::timeout(
            CONNECT_TIMEOUT,
            TcpStream::connect((addr.as_str(), self.port)),
        )
        .await
        .map_err(|_| {
            eyre!(
                "connect to {addr}:{} timed out after {}s",
                self.port,
                CONNECT_TIMEOUT.as_secs_f32()
            )
        })?
        .map_err(|e| eyre!(e).wrap_err(format!("connect to {addr}:{} failed", self.port)))?;

        tracing::debug!(ingress = self.to_string(), "connected to cluster resource");

        stream(
            channel.into_stream(),
            remote,
            StreamMetrics {
                resource: self.host.resource(),
                direction: "ingress",
            },
        )
        .await?;

        tracing::debug!(
            ingress = self.to_string(),
            "connection lost cluster resource"
        );

        Ok(())
    }
}

struct Host {
    resource: String,
    segments: Vec<String>,
}

impl std::fmt::Display for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.segments.join("/"))
    }
}

impl Host {
    fn new(host: &str) -> Result<Self> {
        let segments: Vec<String> = host
            .split('/')
            .map(std::string::ToString::to_string)
            .collect();

        Ok(Self {
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

    async fn addr(&self, client: kube::Client) -> Result<String> {
        match self.resource() {
            "pods" => Pod::get_host(client, &self.segments[1..]).await,
            "services" => Service::get_host(client, &self.segments[1..]).await,
            "nodes" => Node::get_host(client, &self.segments[1..]).await,
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
