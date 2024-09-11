use std::collections::BTreeMap;

use derive_builder::Builder;
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

use super::{stream, StreamMetrics, Tunnel};
use crate::{
    broadcast::Broadcast,
    events::Event,
    identity::Identity,
    resources::{pod::PodExt, tunnel, MANAGER},
};

static HOST_LABEL: &str = "egress.kty.dev/host";
static IDENTITY_LABEL: &str = "egress.kty.dev/identity";

#[derive(Builder)]
#[builder(pattern = "owned")]
pub struct Egress {
    #[builder(default)]
    metadata: ObjectMeta,
    user: String,
    port: u16,
    #[builder(default)]
    tasks: JoinSet<Result<()>>,
    #[builder(setter(custom))]
    server: Pod,
    broadcast: Broadcast,
    meta: Tunnel,
}

impl std::fmt::Display for Egress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Host[{}:{}] User[{}]", self.path(), self.port, self.user)
    }
}

impl EgressBuilder {
    pub fn host(mut self, service: &str) -> Result<Self> {
        let (ns, name) = service
            .split_once('/')
            .ok_or_else(|| eyre!("format is <namespace>/<name>"))?;

        let meta = self.metadata.get_or_insert(ObjectMeta::default());
        meta.name = Some(name.into());
        meta.namespace = Some(ns.into());

        Ok(self)
    }

    pub fn annotation(mut self, key: String, value: String) -> Self {
        self.metadata
            .get_or_insert(ObjectMeta::default())
            .annotations
            .get_or_insert_with(BTreeMap::new)
            .insert(key, value);

        self
    }

    pub fn identity(self, identity: &Identity) -> Self {
        self.user(identity.name.clone())
            .annotation(IDENTITY_LABEL.to_string(), identity.name.clone())
    }

    pub fn server(self, pod: Pod) -> Self {
        let mut this = self.annotation(HOST_LABEL.to_string(), pod.name_any());

        this.server = Some(pod);

        this
    }
}

impl Egress {
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
        let addr = self.server.ip().expect("current pod has an IP address");
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
                "egress.kty.dev".to_string(),
            ),
            ("kubernetes.io/service-name".to_string(), self.name_any()),
        ]);

        #[allow(clippy::cast_lossless)]
        let endpoint = EndpointSlice {
            metadata,
            address_type: address_type.to_string(),
            endpoints: vec![Endpoint {
                addresses: vec![addr.to_string()],
                target_ref: Some(self.server.object_ref(&())),
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
            let channel = match handle
                .channel_open_forwarded_tcpip(
                    self.path(),
                    u32::from(self.port),
                    addr.ip().to_string(),
                    u32::from(addr.port()),
                )
                .await
            {
                Ok(channel) => channel,
                Err(e) => {
                    let e = if let russh::Error::ChannelOpenFailure(err) = e {
                        eyre!("are you listening on the configured local port?")
                            .wrap_err(format!("failed to open channel to localhost: {err:?}"))
                            .wrap_err("reverse tunnel failed")
                    } else {
                        e.into()
                    };

                    self.broadcast
                        .all(Event::Tunnel(Err(tunnel::Error::new(
                            &e,
                            self.meta.clone(),
                        ))))
                        .await?;

                    continue;
                }
            };

            let id = channel.id();
            let connection_string = self.to_string();

            self.broadcast
                .all(Event::Tunnel(Ok(self.meta.clone().into_active())))
                .await?;

            if let Some(result) = self.tasks.try_join_next() {
                // The error from this should have already been broadcast.
                let _unused = result?;
            }

            let num_tasks = self.tasks.len();
            let broadcast = self.broadcast.clone();
            let meta = self.meta.clone();

            self.tasks.spawn(async move {
                tracing::debug!(egress = connection_string, "outgoing connection opened");

                let result = stream(
                    channel.into_stream(),
                    socket,
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

                tracing::debug!(egress = connection_string, "outgoing connection closed");

                if let Err(e) = &result {
                    broadcast
                        .all(Event::Tunnel(Err(tunnel::Error::new(
                            e,
                            meta.clone().into_error(),
                        ))))
                        .await?;
                } else if num_tasks == 0 {
                    broadcast
                        .all(Event::Tunnel(Ok(meta.into_listening())))
                        .await?;
                }

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
