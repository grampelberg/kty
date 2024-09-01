use eyre::{eyre, Result};
use k8s_openapi::api::core::v1::Pod;
use kube::api::{Api, AttachParams};
use tokio::io::AsyncReadExt;

use super::{StatusError, StatusExt};
use crate::resources::container::{Container, ContainerExt};

pub struct Proc {
    container: Container,
}

impl Proc {
    pub fn new(container: Container) -> Self {
        Self { container }
    }

    pub async fn exec(&self, client: kube::Client, cmd: Vec<&str>) -> Result<(Vec<u8>, Vec<u8>)> {
        let mut proc = Api::<Pod>::namespaced(
            client,
            self.container
                .namespace()
                .expect("containers have namespaces")
                .as_str(),
        )
        .exec(
            self.container.pod_name().as_str(),
            cmd,
            &AttachParams {
                container: Some(self.container.name_any()),
                stdout: true,
                stderr: true,
                ..Default::default()
            },
        )
        .await?;

        let status = proc.take_status().ok_or(eyre!("status not available"))?;
        let mut stdout = proc.stdout().ok_or(eyre!("stdout not available"))?;
        let mut stderr = proc.stderr().ok_or(eyre!("stderr not available"))?;

        let mut out = Vec::new();
        let mut err = Vec::new();

        stdout.read_to_end(&mut out).await?;
        stderr.read_to_end(&mut err).await?;

        if let Some(status) = status.await {
            if !status.is_success() {
                return Err(eyre!(StatusError::new(status)));
            }
        }

        Ok((out, err))
    }
}
