pub mod age;
pub mod container;
pub mod file;
pub mod install;
pub mod node;
pub mod pod;
pub mod refs;
pub mod status;
pub mod store;
pub mod tunnel;

use color_eyre::Section;
use eyre::{eyre, Report, Result};
pub use file::File;
use futures::{Stream, StreamExt};
use itertools::Itertools;
use json_value_merge::Merge;
use k8s_openapi::{
    api::core::v1::ObjectReference,
    apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition,
    apimachinery::pkg::apis::meta::v1::OwnerReference,
};
use kube::{
    api,
    api::{
        Api, DynamicObject, GroupVersionKind, ObjectMeta, PartialObjectMetaExt, PatchParams,
        PostParams, ResourceExt,
    },
    core::discovery::Scope,
    discovery::pinned_kind,
    CustomResourceExt, Resource,
};
use petgraph::Graph;
use regex::Regex;
use serde::Serialize;
pub use tunnel::Tunnel;

use crate::identity;

pub static MANAGER: &str = "kkty.dev";

pub(crate) fn all() -> Vec<CustomResourceDefinition> {
    vec![identity::key::Key::crd()]
}

pub(crate) async fn create(
    client: &Api<CustomResourceDefinition>,
    update: bool,
) -> Result<Vec<CustomResourceDefinition>> {
    tracing::info!(update = update, "updating CRD definitions...");

    let results: Vec<_> = futures::stream::iter(all())
        .map(|resource| async move {
            tracing::info!("creating/updating CRD: {}", resource.name_any());

            if update {
                client
                    .patch(
                        &resource.name_any(),
                        &PatchParams::apply("kty").force(),
                        &kube::api::Patch::Apply(&resource),
                    )
                    .await
            } else {
                client.create(&PostParams::default(), &resource).await
            }
        })
        .buffered(100)
        .collect()
        .await;

    let (success, failure): (Vec<CustomResourceDefinition>, Vec<_>) =
        results.into_iter().partition_result();

    if !failure.is_empty() {
        return Err(failure
            .into_iter()
            .fold(eyre!("unable to create resources"), |acc, err| {
                acc.error(err)
            }));
    }

    Ok(success)
}

pub(crate) trait KubeID {
    fn kube_id(&self) -> Result<String>;
}

impl KubeID for String {
    fn kube_id(&self) -> Result<String> {
        Ok(Regex::new(r"[^A-Za-z\d]")?
            .replace_all(self, "-")
            .to_lowercase())
    }
}

pub(crate) trait ApplyPatch<K>
where
    K: Resource,
{
    fn patch(patch: &serde_json::Value) -> Result<serde_json::Value>;
}

impl<K> ApplyPatch<K> for K
where
    K: Resource<DynamicType = ()>,
{
    fn patch(right: &serde_json::Value) -> Result<serde_json::Value> {
        let mut left = serde_json::to_value(ObjectMeta::default().into_request_partial::<K>())?;
        left.merge(right);

        Ok(left)
    }
}

pub trait Yaml<K>
where
    K: Resource + Serialize,
{
    fn to_yaml(&self) -> Result<String>;
}

impl<K> Yaml<K> for K
where
    K: Resource + Serialize,
{
    fn to_yaml(&self) -> Result<String> {
        serde_yaml::to_string(&self).map_err(Into::into)
    }
}

pub trait Filter {
    fn matches(&self, filter: &str) -> bool;
}

pub trait Compare {
    fn cmp(&self, right: &Self) -> std::cmp::Ordering;
}

pub trait GetGv {
    fn gv(&self) -> (String, String);
}

impl GetGv for String {
    fn gv(&self) -> (String, String) {
        let version: Vec<_> = self.splitn(2, '/').collect();

        if version.len() == 1 {
            (String::new(), version[0].to_string())
        } else {
            (version[0].to_string(), version[1].to_string())
        }
    }
}

pub trait GetGvk {
    fn gvk(&self) -> Result<GroupVersionKind>;
}

impl GetGvk for DynamicObject {
    fn gvk(&self) -> Result<GroupVersionKind> {
        let Some(types) = self.types.as_ref() else {
            return Err(eyre!("no types found"));
        };

        let (group, version) = types.api_version.gv();

        Ok(GroupVersionKind {
            group,
            version,
            kind: types.kind.clone(),
        })
    }
}

impl GetGvk for OwnerReference {
    fn gvk(&self) -> Result<GroupVersionKind> {
        let (group, version) = self.api_version.gv();

        Ok(GroupVersionKind {
            group,
            version,
            kind: self.kind.clone(),
        })
    }
}

pub trait ApiResource {
    fn api_resource(&self) -> api::ApiResource;
}

impl ApiResource for DynamicObject {
    fn api_resource(&self) -> api::ApiResource {
        api::ApiResource::from_gvk(&self.gvk().unwrap())
    }
}

async fn dynamic_client(
    client: kube::Client,
    namespace: &str,
    gvk: &GroupVersionKind,
) -> Result<Api<DynamicObject>> {
    let (ar, caps) = pinned_kind(&client, gvk).await?;

    if matches!(caps.scope, Scope::Namespaced) {
        Ok(Api::namespaced_with(client, namespace, &ar))
    } else {
        Ok(Api::all_with(client, &ar))
    }
}

pub trait DynamicClient {
    async fn dynamic(&self, client: kube::Client) -> Result<Api<DynamicObject>>;
}

impl DynamicClient for DynamicObject {
    async fn dynamic(&self, client: kube::Client) -> Result<Api<DynamicObject>> {
        dynamic_client(
            client,
            self.namespace().unwrap_or_default().as_str(),
            &self.gvk()?,
        )
        .await
    }
}

#[async_trait::async_trait]
pub(crate) trait GetOwners {
    fn get_owners(&self, client: kube::Client) -> impl Stream<Item = Result<DynamicObject>>;
}

#[async_trait::async_trait]
impl GetOwners for ObjectMeta {
    fn get_owners(&self, client: kube::Client) -> impl Stream<Item = Result<DynamicObject>> {
        futures::stream::iter(self.owner_references.clone().unwrap_or_default())
            .map(move |reference| {
                let client = client.clone();
                let namespace = self.namespace.clone().unwrap_or_default();

                async move {
                    let resource = dynamic_client(client, namespace.as_str(), &reference.gvk()?)
                        .await?
                        .get(reference.name.as_str())
                        .await?;

                    Ok::<DynamicObject, Report>(resource)
                }
            })
            .buffered(100)
    }
}

pub(crate) trait NamedReference {
    fn named_ref<N, NS>(name: N, namespace: Option<NS>) -> ObjectReference
    where
        N: Into<String>,
        NS: Into<String>;
}

impl<K> NamedReference for K
where
    K: Resource,
    <K as Resource>::DynamicType: Default,
{
    fn named_ref<N, NS>(name: N, namespace: Option<NS>) -> ObjectReference
    where
        N: Into<String>,
        NS: Into<String>,
    {
        let namespace = namespace.map(Into::into);

        ObjectReference {
            api_version: Some(K::api_version(&K::DynamicType::default()).to_string()),
            field_path: None,
            kind: Some(K::kind(&K::DynamicType::default()).to_string()),
            name: Some(name.into()),
            namespace,
            resource_version: None,
            uid: None,
        }
    }
}

#[async_trait::async_trait]
pub(crate) trait ResourceGraph {
    async fn graph(&self, client: &kube::Client) -> Result<Graph<ObjectReference, ()>>;
}
