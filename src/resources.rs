use color_eyre::Section;
use eyre::{eyre, Result};
use futures::StreamExt;
use itertools::Itertools;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::{
    api::{Api, PatchParams, PostParams, ResourceExt},
    CustomResourceExt,
};

use crate::identity;

pub(crate) fn all() -> Vec<CustomResourceDefinition> {
    vec![identity::User::crd(), identity::Key::crd()]
}

pub(crate) async fn create(
    client: &Api<CustomResourceDefinition>,
    update: bool,
) -> Result<Vec<CustomResourceDefinition>> {
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
