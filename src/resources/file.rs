use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use eyre::{eyre, Result};
use k8s_openapi::api::core::v1::{Namespace, Pod};
use kube::{api::ListParams, Api, ResourceExt};
use russh_sftp::protocol::{self, FileAttributes, FileMode};

use super::{
    container::{Container, ContainerExt, ContainerFiles},
    pod::PodExt,
};

trait FileExt {
    fn to_file(&self) -> protocol::File;
}

impl FileExt for PathBuf {
    fn to_file(&self) -> protocol::File {
        protocol::File {
            filename: self
                .as_path()
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            longname: self.to_string_lossy().into(),
            attrs: FileAttributes {
                permissions: Some(FileMode::DIR.bits()),
                ..Default::default()
            },
        }
    }
}

impl FileExt for Namespace {
    fn to_file(&self) -> protocol::File {
        ["/", self.name_any().as_str()]
            .iter()
            .collect::<PathBuf>()
            .to_file()
    }
}

impl FileExt for Pod {
    fn to_file(&self) -> protocol::File {
        [
            "/",
            self.namespace().expect("pods have namespaces").as_str(),
            self.name_any().as_str(),
        ]
        .iter()
        .collect::<PathBuf>()
        .to_file()
    }
}

impl FileExt for Container {
    fn to_file(&self) -> protocol::File {
        [
            "/",
            self.namespace()
                .expect("containers have namespaces")
                .as_str(),
            self.pod_name().as_str(),
            self.name_any().as_str(),
        ]
        .iter()
        .collect::<PathBuf>()
        .to_file()
    }
}

#[derive(Debug)]
pub struct File<'a> {
    pub namespace: Option<Cow<'a, str>>,
    pub pod: Option<Cow<'a, str>>,
    pub container: Option<Cow<'a, str>>,
    pub path: Option<PathBuf>,
}

impl<'a> File<'a> {
    pub fn new(path: &'a Path) -> Self {
        let segments: Vec<Cow<str>> = path.iter().map(|s| s.to_string_lossy()).collect();

        let namespace = segments.get(1).cloned();
        let pod = segments.get(2).cloned();
        let container = segments.get(3).cloned();
        let path = segments
            .iter()
            .skip(4)
            .fold(PathBuf::from("/"), |mut path, segment| {
                path.push(segment.to_string());
                path
            });

        Self {
            namespace,
            pod,
            container,
            path: if segments.len() > 4 { Some(path) } else { None },
        }
    }

    pub async fn list(&self, client: kube::Client) -> Result<Vec<protocol::File>> {
        match self {
            File {
                namespace: None, ..
            } => Ok(Api::<Namespace>::all(client)
                .list(&ListParams::default())
                .await?
                .iter()
                .map(Namespace::to_file)
                .collect()),
            File {
                namespace: Some(ns),
                pod: None,
                ..
            } => Ok(Api::<Pod>::namespaced(client, ns)
                .list(&ListParams::default())
                .await?
                .iter()
                .map(Pod::to_file)
                .collect()),
            File {
                namespace: Some(ns),
                pod: Some(pod),
                container: None,
                ..
            } => Ok(Api::<Pod>::namespaced(client, ns)
                .get(pod)
                .await?
                .containers(None)
                .iter()
                .map(Container::to_file)
                .collect()),
            File {
                namespace: Some(ns),
                pod: Some(pod),
                container: Some(container),
                path,
            } => {
                let containers = Api::<Pod>::namespaced(client.clone(), ns)
                    .get(pod)
                    .await?
                    .containers(Some(container.to_string()));

                containers
                    .first()
                    .ok_or(eyre!(
                        "container {container} not found in pod {pod} from namespace {ns}",
                    ))?
                    .list(
                        client,
                        path.as_ref().map_or(Path::new("/"), PathBuf::as_path),
                    )
                    .await
            }
        }
    }

    pub async fn stat(&self, client: kube::Client) -> Result<FileAttributes> {
        match self {
            File {
                namespace: None, ..
            } => Ok(FileAttributes::dir()),
            File {
                namespace: Some(ns),
                pod: None,
                ..
            } => Ok(Api::<Namespace>::all(client)
                .get(ns)
                .await
                .map(|_| FileAttributes::dir())?),
            File {
                namespace: Some(ns),
                pod: Some(pod),
                container: None,
                ..
            } => Ok(Api::<Pod>::namespaced(client, ns)
                .get(pod)
                .await
                .map(|_| FileAttributes::dir())?),
            File {
                container: Some(_),
                path: None,
                ..
            } => Ok(Container::from_path(client, self)
                .await
                .map(|_| FileAttributes::dir())?),
            File {
                path: Some(path), ..
            } => {
                Container::from_path(client.clone(), self)
                    .await?
                    .stat(client, path)
                    .await
            }
        }
    }

    pub async fn read(&self, client: kube::Client) -> Result<Vec<u8>> {
        match self {
            File {
                path: Some(path), ..
            } => {
                Container::from_path(client.clone(), self)
                    .await?
                    .read(client, path)
                    .await
            }
            _ => Err(eyre!("invalid path: {:?}", self)),
        }
    }
}

impl std::fmt::Display for File<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "namespace: {:?} pod: {:?} container: {:?} path: {:?}",
            self.namespace, self.pod, self.container, self.path
        )
    }
}

trait FileDir {
    fn dir() -> FileAttributes;
}

impl FileDir for FileAttributes {
    fn dir() -> FileAttributes {
        FileAttributes {
            permissions: Some(FileMode::DIR.bits()),
            ..Default::default()
        }
    }
}
