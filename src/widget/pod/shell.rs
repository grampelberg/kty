use std::sync::Arc;

use eyre::{eyre, Result};
use futures::StreamExt;
use k8s_openapi::api::core::v1::Pod;
use kube::api::{Api, AttachParams, ResourceExt};
use ratatui::{layout::Rect, Frame};
use tokio::{
    io::AsyncWriteExt,
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
};
use tokio_util::bytes::Bytes;
use tracing::info;

use super::Widget;
use crate::{
    events::{Broadcast, Event},
    widget::tabs::Tab,
};

// fn handles(
//     proc: &AttachedProcess,
// ) -> (
//     impl AsyncWriteExt + Unpin,
//     impl AsyncReadExt + Unpin,
//     impl AsyncReadExt + Unpin,
// ) {
//     let stdin = proc.stdin().ok_or(eyre!("stdin not available"))?;
//     let stdout = proc.stdout().ok_or(eyre!("stdout not available"))?;
//     let stderr = proc.stderr().ok_or(eyre!("stderr not available"))?;

//     (stdin, stdout, stderr)
// }

async fn exec(
    client: kube::Client,
    pod: Arc<Pod>,
    mut input: UnboundedReceiver<Bytes>,
    output: UnboundedSender<Bytes>,
) -> Result<()> {
    let mut proc = Api::<Pod>::namespaced(client, &pod.namespace().unwrap())
        .exec(
            &pod.name_any(),
            vec!["/bin/bash"],
            &AttachParams {
                stdin: true,
                stdout: true,
                stderr: false,
                tty: true,
                ..Default::default()
            },
        )
        .await?;

    let mut stdin = proc.stdin().ok_or(eyre!("stdin not available"))?;
    // let mut stderr = proc.stderr().ok_or(eyre!("stderr not available"))?;
    let mut stdout =
        tokio_util::io::ReaderStream::new(proc.stdout().ok_or(eyre!("stdout not available"))?);

    loop {
        tokio::select! {
            message = input.recv() => {
                if let Some(message) = message {
                    stdin.write_all(&message).await?;
                } else {
                    break;
                }
            }
            message = stdout.next() => {
                info!("message: {:?}", message);

                if let Some(Ok(message)) = message {
                    output.send(message).unwrap();
                } else {
                    break;
                }
            }
        }
    }

    proc.join().await?;

    Ok(())
}

pub struct Shell {
    client: kube::Client,
    pod: Arc<Pod>,
}

impl Shell {
    pub fn new(client: kube::Client, pod: Arc<Pod>) -> Self {
        Self { client, pod }
    }

    pub fn tab(name: String, client: kube::Client, pod: Arc<Pod>) -> Tab {
        Tab::new(
            name,
            Box::new(move || Box::new(Self::new(client.clone(), pod.clone()))),
        )
    }
}

impl Widget for Shell {
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        Ok(Broadcast::Ignored)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) {}
}
