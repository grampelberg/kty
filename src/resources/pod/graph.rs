use eyre::{eyre, Result};
use k8s_openapi::api::{
    core::v1::{
        ConfigMap, Namespace, Node, ObjectReference, PersistentVolume, PersistentVolumeClaim,
        PersistentVolumeClaimSpec, Pod, PodSpec, Secret, Service, ServiceAccount, Volume,
    },
    discovery::v1::EndpointSlice,
    rbac::v1::{ClusterRole, ClusterRoleBinding, Role, RoleBinding, RoleRef},
};
use kube::{api::ListParams, Api, Resource, ResourceExt};
use petgraph::{graph::NodeIndex, Graph};

use crate::resources::{refs::References, NamedReference, ResourceGraph};

fn for_role_ref(refs: &mut References, ns: &str, from: NodeIndex, role_ref: &RoleRef) {
    if role_ref.kind == "Role" {
        refs.edge_to(from, Role::named_ref(role_ref.name.as_str(), Some(ns)));
    } else {
        refs.edge_to(
            from,
            ClusterRole::named_ref(role_ref.name.as_str(), None::<String>),
        );
    }
}

async fn auth(pod: &Pod, client: &kube::Client, refs: &mut References) -> Result<()> {
    let ns = pod.namespace().ok_or_else(|| eyre!("no namespace"))?;

    let Some(PodSpec {
        service_account: Some(sa),
        ..
    }) = &pod.spec
    else {
        return Ok(());
    };

    let sa_index = refs.to(ServiceAccount::named_ref(sa, pod.namespace()));

    // RoleBinding nodes
    let rbs = Api::<RoleBinding>::namespaced(client.clone(), ns.as_str())
        .list(&ListParams::default())
        .await?
        .into_iter()
        .filter(|rb| {
            rb.subjects.as_ref().map_or(false, |s| {
                s.iter()
                    .any(|s| s.kind == "ServiceAccount" && s.name.as_str() == sa.as_str())
            })
        });

    for rb in rbs {
        let i = refs.edge_to(sa_index, rb.object_ref(&()));

        for_role_ref(refs, ns.as_str(), i, &rb.role_ref);
    }

    let crbs = Api::<ClusterRoleBinding>::all(client.clone())
        .list(&ListParams::default())
        .await?
        .into_iter()
        .filter(|rb| {
            rb.subjects.as_ref().map_or(false, |s| {
                s.iter()
                    .any(|s| s.kind == "ServiceAccount" && s.name.as_str() == sa.as_str())
            })
        });

    for crb in crbs {
        let i = refs.edge_to(sa_index, crb.object_ref(&()));

        for_role_ref(refs, ns.as_str(), i, &crb.role_ref);
    }

    Ok(())
}

async fn network(pod: &Pod, client: &kube::Client, refs: &mut References) -> Result<()> {
    let ns = pod.namespace().ok_or_else(|| eyre!("no namespace"))?;
    let self_ref = pod.object_ref(&());

    // TODO: getting *all* the endpointslices for every pod seems excessive (and
    // potentially bad for the API server).
    let eps = Api::<EndpointSlice>::namespaced(client.clone(), ns.as_str())
        .list(&ListParams::default())
        .await?
        .into_iter()
        .filter(|ep| {
            let self_ref = self_ref.clone();

            ep.endpoints.iter().any(move |e| {
                e.target_ref
                    .as_ref()
                    .map_or(false, |t| t.uid == self_ref.uid)
            })
        });

    for ep in eps {
        let idx = refs.to(ep.object_ref(&()));

        // This is using the label instead of the owner reference. The owner reference
        // does not appear to be required (as it isn't used with the `EndpointSlice`
        // created by egress right now). The `OwnerReference` feels like it is a better
        // option though.
        if let Some(svc_name) = ep
            .metadata
            .labels
            .as_ref()
            .and_then(|l| l.get("kubernetes.io/service-name"))
        {
            refs.edge_to(idx, Service::named_ref(svc_name, pod.namespace()));
        }
    }

    Ok(())
}

// TODO: make this pull in the owners (and volumes) themselves.
async fn volumes(pod: &Pod, client: &kube::Client, refs: &mut References) -> Result<()> {
    let ns = pod.namespace().ok_or_else(|| eyre!("no namespace"))?;

    let Some(PodSpec {
        volumes: Some(volumes),
        ..
    }) = &pod.spec
    else {
        return Ok(());
    };

    for vol in volumes {
        match vol {
            Volume {
                config_map: Some(cm),
                ..
            } => {
                refs.to(ConfigMap::named_ref(cm.name.as_str(), pod.namespace()));
            }
            Volume {
                secret: Some(sec), ..
            } => {
                refs.to(Secret::named_ref(
                    sec.secret_name.clone().unwrap_or_default(),
                    pod.namespace(),
                ));
            }
            Volume {
                persistent_volume_claim: Some(pvc),
                ..
            } => {
                let pvc = Api::<PersistentVolumeClaim>::namespaced(client.clone(), ns.as_str())
                    .get(pvc.claim_name.as_str())
                    .await?;

                let idx = refs.to(pvc.object_ref(&()));

                if let Some(PersistentVolumeClaimSpec {
                    volume_name: Some(name),
                    ..
                }) = &pvc.spec
                {
                    refs.edge_to(
                        idx,
                        PersistentVolume::named_ref(name.as_str(), pod.namespace()),
                    );
                }
            }
            _ => continue,
        };
    }

    Ok(())
}

#[async_trait::async_trait]
impl ResourceGraph for Pod {
    async fn graph(&self, client: &kube::Client) -> Result<Graph<ObjectReference, ()>> {
        let mut refs = References::new(client.clone(), &self.object_ref(&()));

        refs.add_owners(&self.metadata).await?;

        let ns = self.namespace().ok_or_else(|| eyre!("no namespace"))?;

        refs.from(Namespace::named_ref(ns.as_str(), None::<String>));

        if let Some(PodSpec {
            node_name: Some(node),
            ..
        }) = &self.spec
        {
            refs.from(Node::named_ref(node.as_str(), None::<String>));
        }

        auth(self, client, &mut refs).await?;
        network(self, client, &mut refs).await?;
        volumes(self, client, &mut refs).await?;

        Ok(refs.graph())
    }
}
