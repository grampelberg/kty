use std::{borrow::Cow, collections::BTreeMap};

use base64::prelude::*;
use eyre::{eyre, Report, Result};
use itertools::Itertools;
use json_patch::{patch, PatchOperation};
use kube::api::{DynamicObject, ResourceExt};
use pkcs8::EncodePrivateKey;
use russh_keys::key::KeyPair;
use rust_embed::Embed;
use serde_json::{from_value, json, to_value};

#[derive(Embed)]
#[folder = "resources"]
pub struct RawDefinitions;

trait SafeName {
    fn safe_name(&self) -> String;
}

impl SafeName for Cow<'_, str> {
    fn safe_name(&self) -> String {
        self.replace(|c: char| !c.is_ascii_alphanumeric(), "-")
            .to_lowercase()
    }
}

pub fn list() -> Result<serde_json::Value> {
    let resource_iter: Vec<_> = RawDefinitions::iter()
        .map(|name| {
            serde_yaml::from_slice::<DynamicObject>(
                &RawDefinitions::get(&name).expect("slice").data,
            )
            .map_err(Report::new)
        })
        .map(|resource| to_value(resource?).map_err(Report::new))
        .try_collect()?;

    let mut resources = RawDefinitions::iter().zip(resource_iter).fold(
        BTreeMap::new(),
        |mut acc, (name, resource)| {
            acc.insert(name.safe_name(), resource);

            acc
        },
    );

    for resource in crate::resources::all()
        .iter()
        .map(|resource| to_value(resource).and_then(from_value::<DynamicObject>))
        .try_collect::<DynamicObject, Vec<_>, serde_json::Error>()?
    {
        resources.insert(resource.name_any(), to_value(resource)?);
    }

    to_value(resources).map_err(Report::new)
}

pub fn add_patches(
    namespace: &str,
    mut resources: serde_json::Value,
) -> Result<Vec<DynamicObject>> {
    let mut patches: Vec<_> = RawDefinitions::iter()
        .map(|name| {
            from_value::<PatchOperation>(json!({
                "op": "add",
                "path": format!("/{}/metadata/namespace", name.safe_name()),
                "value": namespace,
            }))
        })
        .try_collect()?;

    patches.push(from_value(json!({
        "op": "replace",
        "path": "/binding-yaml/subjects/0/namespace",
        "value": namespace,
    }))?);

    let KeyPair::Ed25519(key) = KeyPair::generate_ed25519().expect("key was generated") else {
        return Err(eyre!("key was wrong type"));
    };

    patches.push(from_value(json!({
        "op": "add",
        "path": "/key-yaml/data/id_ed25519",
        "value": BASE64_STANDARD.encode(key.to_pkcs8_pem(ssh_key::LineEnding::default())?),
    }))?);

    patch(&mut resources, &patches)?;

    Ok(from_value::<BTreeMap<String, DynamicObject>>(resources)?
        .into_values()
        .collect())
}
