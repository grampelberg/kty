use std::path::Path;

use eyre::{eyre, Result};
use k8s_openapi::api::core::v1::Pod;
use kube::Api;
use russh_sftp::protocol;
use umask::Mode;

use super::Container;
use crate::resources::{
    pod::{PodExt, Proc},
    File,
};

pub trait ContainerFiles {
    async fn from_path(client: kube::Client, path: &File) -> Result<Container>;

    async fn get_files(
        &self,
        client: kube::Client,
        path: &Path,
        contents: bool,
    ) -> Result<Vec<protocol::File>>;

    async fn read(&self, client: kube::Client, path: &Path) -> Result<Vec<u8>>;
    async fn list(&self, client: kube::Client, path: &Path) -> Result<Vec<protocol::File>>;
    async fn stat(&self, client: kube::Client, path: &Path) -> Result<protocol::FileAttributes>;
}

impl ContainerFiles for Container {
    async fn from_path(client: kube::Client, path: &File<'_>) -> Result<Container> {
        let File {
            namespace: Some(ns),
            pod: Some(pod),
            container: Some(container),
            ..
        } = path
        else {
            return Err(eyre!("invalid path: {:?}", path));
        };

        let containers = Api::<Pod>::namespaced(client.clone(), ns)
            .get(pod)
            .await?
            .containers(Some(container.to_string()));

        containers
            .first()
            .ok_or(eyre!(
                "container {container} not found in pod {pod} from namespace {ns}",
            ))
            .cloned()
    }

    async fn get_files(
        &self,
        client: kube::Client,
        path: &Path,
        contents: bool,
    ) -> Result<Vec<protocol::File>> {
        let full_path = path.to_string_lossy();

        // It might be a better idea to use `stat` here instead of `ls`, there's a lot
        // more control over the output. The downside is that it only stats a single
        // thing. To get a directory, something like `*` ends up being required which'll
        // need a shell (or find).
        let mut cmd = vec!["ls", "-l", "--time-style=+%s"];

        if !contents {
            cmd.push("-d");
        }

        cmd.push(full_path.as_ref());

        let (out, _) = Proc::new(self.clone()).exec(client.clone(), cmd).await?;

        let files = std::str::from_utf8(&out)?;

        if contents {
            Ok(files
                .lines()
                .skip(1)
                .map(|l| l.to_file(path))
                .collect::<Vec<_>>())
        } else {
            Ok(files.lines().map(|l| l.to_file(path)).collect::<Vec<_>>())
        }
    }

    #[tracing::instrument(skip(self, client))]
    async fn read(&self, client: kube::Client, path: &Path) -> Result<Vec<u8>> {
        let full_path = path.to_string_lossy();
        let cmd = vec!["cat", full_path.as_ref()];

        let (out, _) = Proc::new(self.clone()).exec(client.clone(), cmd).await?;

        Ok(out)
    }

    #[tracing::instrument(skip(self, client))]
    async fn list(&self, client: kube::Client, path: &Path) -> Result<Vec<protocol::File>> {
        self.get_files(client, path, true).await
    }

    #[tracing::instrument(skip(self, client))]
    async fn stat(&self, client: kube::Client, path: &Path) -> Result<protocol::FileAttributes> {
        let files = self.get_files(client, path, false).await?;

        files
            .first()
            .map_or(Err(eyre!("no files found")), |file| Ok(file.attrs.clone()))
    }
}

trait ParseFile {
    fn to_file(&self, path: &Path) -> protocol::File;
}

impl ParseFile for &str {
    fn to_file(&self, path: &Path) -> protocol::File {
        self.split_ascii_whitespace().enumerate().fold(
            protocol::File {
                filename: String::new(),
                longname: String::new(),
                attrs: protocol::FileAttributes::default(),
            },
            |mut file, (i, s)| {
                match i {
                    0 => {
                        file.attrs.permissions =
                            Some(Mode::parse(&s[1..s.len()]).expect("valid mode").into());

                        let mode = match s.chars().next().unwrap_or_default() {
                            'b' => protocol::FileMode::BLK,
                            'c' => protocol::FileMode::CHR,
                            'd' => protocol::FileMode::DIR,
                            'l' => protocol::FileMode::LNK,
                            's' => protocol::FileMode::SOCK,
                            _ => protocol::FileMode::REG,
                        };

                        file.attrs.set_type(mode);
                    }
                    2 => file.attrs.user = Some(s.to_string()),
                    3 => file.attrs.group = Some(s.to_string()),
                    4 => file.attrs.size = Some(s.parse().unwrap()),
                    5 => {
                        file.attrs.mtime = Some(s.parse().unwrap());
                    }
                    // This is used for both `stat` and `list`. When used via `stat`, `ls` returns
                    // an absolute path for the file (or directory). When used via `list`, `ls`
                    // returns the relative path based on the directory that was listed.
                    6 => {
                        let out_path = Path::new(s);

                        if out_path.is_absolute() {
                            file.filename =
                                out_path.file_name().unwrap().to_string_lossy().to_string();
                            file.longname = out_path.to_string_lossy().to_string();
                        } else {
                            file.filename = s.to_string();
                            file.longname = path.join(s).to_string_lossy().to_string();
                        }
                    }
                    _ => {}
                }

                file
            },
        )
    }
}
