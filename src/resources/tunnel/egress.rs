use std::collections::BTreeMap;

use eyre::{eyre, Result};
use k8s_openapi::{
    api::{
        core::v1::{Pod, Service, ServicePort, ServiceSpec},
        discovery::v1::{Endpoint, EndpointConditions, EndpointPort, EndpointSlice},
    },
    apimachinery::pkg::util::intstr::IntOrString,
};
use kube::{
    api::{ObjectMeta, Patch, PatchParams},
    Api, Resource, ResourceExt,
};
use russh::server;
use tokio::{net::TcpListener, task::JoinSet};

use super::{stream, StreamMetrics};
use crate::{
    identity::Identity,
    resources::{pod::PodExt, MANAGER},
};

static HOST_LABEL: &str = "egress.kuberift.com/host";
static IDENTITY_LABEL: &str = "egress.kuberift.com/identity";

pub struct Egress {
    metadata: ObjectMeta,
    port: u16,
    tasks: JoinSet<Result<()>>,
    current_pod: Pod,
}

impl Egress {
    pub fn new(identity: &Identity, current_pod: Pod, service: &str, port: u16) -> Result<Self> {
        let (ns, name) = service
            .split_once('/')
            .ok_or_else(|| eyre!("format is <namespace>/<name>"))?;

        Ok(Self {
            metadata: ObjectMeta {
                name: Some(name.into()),
                namespace: Some(ns.into()),
                annotations: Some(BTreeMap::from([
                    (HOST_LABEL.to_string(), current_pod.name_any()),
                    (IDENTITY_LABEL.to_string(), identity.name.clone()),
                ])),
                ..Default::default()
            },
            port,
            tasks: JoinSet::new(),
            current_pod,
        })
    }

    fn namespace(&self) -> Option<String> {
        self.metadata.namespace.clone()
    }

    fn name_any(&self) -> String {
        self.metadata.name.clone().expect("name is required")
    }

    fn path(&self) -> String {
        format!(
            "{}/{}",
            self.namespace().unwrap_or_default(),
            self.name_any()
        )
    }

    // The assumption here is that the current hostname is the same as the pod name
    // and that this is running inside a k8s cluster. This allows us to setup a
    // selector that is only this pod because we add a label to the pod on startup.
    #[allow(clippy::cast_lossless)]
    async fn service(&self, client: kube::Client, local_port: u16) -> Result<Service> {
        Api::<Service>::namespaced(
            client,
            self.namespace().expect("namespace is required").as_str(),
        )
        .patch(
            &self.name_any(),
            &PatchParams::apply(MANAGER).force(),
            &Patch::Apply(&Service {
                metadata: self.metadata.clone(),
                spec: Some(ServiceSpec {
                    ports: Some(vec![ServicePort {
                        port: self.port as i32,
                        target_port: Some(IntOrString::Int(local_port as i32)),
                        ..Default::default()
                    }]),
                    selector: None,
                    type_: Some("ClusterIP".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
        )
        .await
        .map_err(|e| match e {
            kube::Error::Api(e) => {
                eyre!(e.message).wrap_err(format!("failed to update {}", self.path()))
            }
            e => e.into(),
        })
    }

    async fn endpoint(&self, client: kube::Client, local_port: u16) -> Result<EndpointSlice> {
        let addr = self
            .current_pod
            .ip()
            .expect("current pod has an IP address");
        let address_type = if addr.is_ipv4() { "IPv4" } else { "IPv6" };

        // Owner references cannot be cross-namespace. Because the server will run in
        // namespace X and the services can be in namespace Y, this results in the
        // EndpointSlice being immediately deleted. It would be nice to have some kind
        // of garbage collection tied to the pod itself - but that might need to be a
        // startup process.
        let mut metadata = self.metadata.clone();
        metadata.labels.get_or_insert(BTreeMap::new()).extend([
            (
                "endpointslice.kubernetes.io/managed-by".to_string(),
                "egress.kuberift.com".to_string(),
            ),
            ("kubernetes.io/service-name".to_string(), self.name_any()),
        ]);

        #[allow(clippy::cast_lossless)]
        let endpoint = EndpointSlice {
            metadata,
            address_type: address_type.to_string(),
            endpoints: vec![Endpoint {
                addresses: vec![addr.to_string()],
                target_ref: Some(self.current_pod.object_ref(&())),
                conditions: Some(EndpointConditions {
                    ready: Some(true),
                    serving: Some(true),
                    terminating: Some(false),
                }),
                ..Default::default()
            }],
            ports: Some(vec![EndpointPort {
                port: Some(local_port as i32),
                ..Default::default()
            }]),
        };

        Api::<EndpointSlice>::namespaced(
            client,
            self.namespace().expect("namespace is required").as_str(),
        )
        .patch(
            &self.name_any(),
            &PatchParams::apply(MANAGER).force(),
            &Patch::Apply(&endpoint),
        )
        .await
        .map_err(|e| match e {
            kube::Error::Api(e) => {
                eyre!(e.message).wrap_err(format!("failed to update endpoint for {}", self.path()))
            }
            e => e.into(),
        })
    }

    pub async fn run(&mut self, client: kube::Client, handle: server::Handle) -> Result<()> {
        tracing::debug!(
            resource = "service",
            direction = "egress",
            activity = "tunnel::egress",
            "connection",
        );

        let listener = TcpListener::bind("0.0.0.0:0").await?;
        let local_port = listener.local_addr()?.port();

        self.service(client.clone(), local_port).await?;
        self.endpoint(client.clone(), local_port).await?;

        loop {
            let (socket, addr) = listener.accept().await?;
            let handle = handle.clone();
            let mut channel = handle
                .channel_open_forwarded_tcpip(
                    self.path(),
                    u32::from(self.port),
                    addr.ip().to_string(),
                    u32::from(addr.port()),
                )
                .await?;
            let id = channel.id();

            self.tasks.spawn(async move {
                let (src_read, src_write) = socket.into_split();
                let dst_write = channel.make_writer();
                let dst_read = channel.make_reader();

                let result = stream(
                    (src_read, src_write),
                    (dst_read, dst_write),
                    StreamMetrics {
                        resource: "service",
                        direction: "egress",
                    },
                )
                .await;

                handle
                    .close(id)
                    .await
                    .map_err(|()| eyre!("failed to close channel {id}"))?;

                result
            });
        }
    }
}

impl Drop for Egress {
    fn drop(&mut self) {
        self.tasks.abort_all();
    }
}
