pub mod age;
pub mod container;
pub mod file;
pub mod pod;
pub mod status;
pub mod store;
pub mod tunnel;

use color_eyre::Section;
pub use container::{Container, ContainerExt};
use eyre::{eyre, Result};
pub use file::File;
use futures::StreamExt;
use itertools::Itertools;
use json_value_merge::Merge;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::{
    api::{Api, ObjectMeta, PartialObjectMetaExt, PatchParams, PostParams, ResourceExt},
    CustomResourceExt, Resource,
};
use regex::Regex;
use serde::Serialize;
pub use tunnel::Tunnel;

use crate::identity;

pub static MANAGER: &str = "kuberift.com";

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
