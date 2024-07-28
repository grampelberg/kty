use color_eyre::Section;
use eyre::{eyre, OptionExt, Result};
use futures::{future, StreamExt, TryStreamExt};
use itertools::Itertools;
use json_value_merge::Merge;
use k8s_openapi::{
    apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition,
    apimachinery::pkg::apis::meta::v1::OwnerReference,
};
use kube::{
    api::{Api, ObjectMeta, PartialObjectMetaExt, PatchParams, PostParams, ResourceExt},
    core::NamespaceResourceScope,
    CustomResourceExt, Resource,
};
use regex::Regex;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tracing::info;

use crate::identity;

pub static MANAGER: &str = "kuberift.com";

pub(crate) fn all() -> Vec<CustomResourceDefinition> {
    vec![identity::User::crd(), identity::Key::crd()]
}

pub(crate) async fn create(
    client: &Api<CustomResourceDefinition>,
    update: bool,
) -> Result<Vec<CustomResourceDefinition>> {
    info!(update = update, "updating CRD definitions...");

    let results: Vec<_> = futures::stream::iter(all())
        .map(|resource| async move {
            if update {
                client
                    .patch(
                        &resource.name_any(),
                        &PatchParams::apply("kuberift").force(),
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

// TODO: ObjectMeta::default().into_request_partial::<ResourceType>(); is
// probably a better way to do this.
#[derive(Debug, PartialEq)]
pub struct GVK {
    pub api_version: String,
    pub kind: String,
}

pub trait GetGVK {
    fn gvk() -> GVK;
}

impl<K> GetGVK for K
where
    K: Resource,
    <K as Resource>::DynamicType: Default,
{
    fn gvk() -> GVK {
        GVK {
            api_version: K::api_version(&K::DynamicType::default()).into(),
            kind: K::kind(&K::DynamicType::default()).into(),
        }
    }
}

impl PartialEq<OwnerReference> for GVK {
    fn eq(&self, other: &OwnerReference) -> bool {
        self.api_version == other.api_version && self.kind == other.kind
    }
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

pub(crate) trait AddReferences: Resource {
    fn add_owner<K>(&mut self, owner: &K) -> Result<()>
    where
        K: Resource<DynamicType = ()>;

    fn add_controller<K>(&mut self, obj: &K) -> Result<()>
    where
        K: Resource<DynamicType = ()>;
}

impl<K: Resource> AddReferences for K {
    fn add_owner<O>(&mut self, owner: &O) -> Result<()>
    where
        O: Resource<DynamicType = ()>,
    {
        let owner_ref = owner
            .owner_ref(&())
            .ok_or_eyre("owner reference not found")?;

        self.meta_mut()
            .owner_references
            .get_or_insert_with(Vec::new)
            .push(owner_ref);

        Ok(())
    }

    fn add_controller<O>(&mut self, owner: &O) -> Result<()>
    where
        O: Resource<DynamicType = ()>,
    {
        let ctrl_ref = owner
            .controller_owner_ref(&())
            .ok_or_eyre("controller reference not found")?;

        self.meta_mut()
            .owner_references
            .get_or_insert_with(Vec::new)
            .push(ctrl_ref);

        Ok(())
    }
}

#[async_trait::async_trait]
pub(crate) trait GetOwners<Owned>
where
    Owned: Resource + Sync,
    <Owned as Resource>::DynamicType: Default,
{
    async fn get_owners<Owner>(self, obj: &Owned) -> Result<Vec<Owner>, kube::Error>
    where
        <Owner as Resource>::DynamicType: Default,
        Owner: Resource<Scope = NamespaceResourceScope>
            + Clone
            + DeserializeOwned
            + std::fmt::Debug
            + Send;
}

#[async_trait::async_trait]
impl<Owned> GetOwners<Owned> for Api<Owned>
where
    Owned: Resource + Sync,
    <Owned as Resource>::DynamicType: Default,
{
    async fn get_owners<Owner>(self, obj: &Owned) -> Result<Vec<Owner>, kube::Error>
    where
        <Owner as Resource>::DynamicType: Default,
        Owner: Resource<Scope = NamespaceResourceScope>
            + Clone
            + DeserializeOwned
            + std::fmt::Debug
            + Send,
    {
        let client: &Api<Owner> = &Api::default_namespaced(self.into());

        let owner_gvk = &Owner::gvk();

        futures::stream::iter(obj.owner_references())
            .filter(|reference| future::ready(&owner_gvk == reference))
            .then(move |reference| async { client.get(&reference.name) })
            .buffered(100)
            .boxed()
            .try_collect()
            .await
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
