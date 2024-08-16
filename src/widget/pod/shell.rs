use std::{pin::Pin, sync::Arc};

use derive_builder::Builder;
use eyre::{eyre, Result};
use futures::{future::BoxFuture, StreamExt};
use k8s_openapi::{api::core::v1::Pod, apimachinery::pkg::apis::meta::v1::Status};
use kube::{
    api::{Api, AttachParams, TerminalSize},
    ResourceExt,
};
use ratatui::{
    layout::Rect,
    prelude::*,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
};
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    sync::mpsc::UnboundedReceiver,
};
use tokio_util::{bytes::Bytes, io::ReaderStream};
use tracing::info;

use crate::{
    events::{Broadcast, Event, Keypress},
    resources::{
        container::{Container, ContainerExt},
        pod::PodExt,
    },
    widget::{
        input::Text,
        propagate,
        table::{Content, DetailFn, Table},
        tabs::Tab,
        Raw, Widget,
    },
};

pub struct Shell {
    pod: Arc<Pod>,
    table: Table,
}

impl Shell {
    pub fn new(client: kube::Client, pod: Arc<Pod>) -> Self {
        Self {
            pod: pod.clone(),
            table: Table::default().constructor(Command::from_pod(client, pod)),
        }
    }

    pub fn tab(name: String, client: kube::Client, pod: Arc<Pod>) -> Tab {
        Tab::new(
            name,
            Box::new(move || Box::new(Self::new(client.clone(), pod.clone()))),
        )
    }
}

impl Widget for Shell {
    // TODO: handle raw and close the detail view.
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        match self.table.dispatch(event)? {
            Broadcast::Raw(raw) => {
                self.table.exit();

                Ok(Broadcast::Raw(raw))
            }
            Broadcast::Exited => Ok(Broadcast::Exited),
            _ => Ok(Broadcast::Ignored),
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        self.table.render(frame, area, &self.pod);
    }
}

enum CommandState {
    Input(Text),
    Attached(Exec),
}

struct Command {
    client: kube::Client,
    pod: Arc<Pod>,
    container: Container,

    txt: Text,
}

impl Command {
    pub fn new(client: kube::Client, pod: Arc<Pod>, container: Container) -> Self {
        let title = container.name_any().to_string();

        Self {
            client,
            pod,
            container,

            txt: Text::default()
                .with_title(title.as_str())
                .with_content("/bin/bash"),
        }
    }

    pub fn from_pod(client: kube::Client, pod: Arc<Pod>) -> DetailFn {
        Box::new(move |idx, filter| {
            let containers = pod.containers(filter);

            Ok(Box::new(Command::new(
                client.clone(),
                pod.clone(),
                containers.get(idx).unwrap().clone(),
            )))
        })
    }
}

impl Widget for Command {
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        propagate!(self.txt.dispatch(event), return Ok(Broadcast::Exited));

        match event {
            Event::Keypress(Keypress::Enter) => Ok(Broadcast::Raw(Box::new(
                ExecBuilder::default()
                    .client(self.client.clone())
                    .pod(self.pod.clone())
                    .container(self.container.clone())
                    .cmd(self.txt.content().to_string())
                    .build()?,
            ))),
            _ => Ok(Broadcast::Ignored),
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let [_, area, _] = Layout::vertical([
            Constraint::Fill(0),
            Constraint::Length(3),
            Constraint::Fill(0),
        ])
        .areas(area);

        self.txt.draw(frame, area);
    }
}

#[derive(Builder)]
struct Exec {
    client: kube::Client,
    pod: Arc<Pod>,
    container: Container,
    cmd: String,
}

#[async_trait::async_trait]
impl Raw for Exec {
    async fn start(
        &mut self,
        stdin: &mut UnboundedReceiver<Bytes>,
        mut stdout: Pin<Box<dyn AsyncWrite + Send + Unpin>>,
    ) -> Result<()> {
        let mut proc = Api::<Pod>::namespaced(self.client.clone(), &self.pod.namespace().unwrap())
            .exec(
                &self.pod.name_any(),
                vec![self.cmd.as_str()],
                &AttachParams {
                    container: Some(self.container.name_any().to_string()),
                    stdin: true,
                    stdout: true,
                    stderr: false,
                    tty: true,
                    ..Default::default()
                },
            )
            .await?;

        let status = proc.take_status().ok_or(eyre!("status not available"))?;

        let mut output = ReaderStream::new(proc.stdout().ok_or(eyre!("stdout not available"))?);
        let mut input = proc.stdin().ok_or(eyre!("stdin not available"))?;

        // TODO: handle resize events.

        loop {
            tokio::select! {
                msg = stdin.recv() => {
                    let Some(msg) = msg else {
                        break;
                    };

                    input.write_all(&msg).await?;
                    input.flush().await?;

                    if matches!(msg.try_into()?, Event::Keypress(Keypress::Control('b'))) {
                        break;
                    }
                }
                msg = output.next() => {
                    let Some(msg) = msg else {
                        break;
                    };

                    stdout.write_all(&msg?).await?;
                    stdout.flush().await?;

                }
            }
        }

        if let Some(Status {
            status: Some(result),
            ..
        }) = status.await
        {
            if result != "Success" {
                return Err(eyre!("command failed: {}", result));
            }
        }

        proc.join().await?;

        Ok(())
    }
}

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

// async fn exec(
//     client: kube::Client,
//     pod: Arc<Pod>,
//     mut input: UnboundedReceiver<Bytes>,
//     output: UnboundedSender<Bytes>,
// ) -> Result<()> {
//     let mut proc = Api::<Pod>::namespaced(client, &pod.namespace().unwrap())
//         .exec(
//             &pod.name_any(),
//             vec!["/bin/bash"],
//             &AttachParams {
//                 stdin: true,
//                 stdout: true,
//                 stderr: false,
//                 tty: true,
//                 ..Default::default()
//             },
//         )
//         .await?;

//     let mut stdin = proc.stdin().ok_or(eyre!("stdin not available"))?;
//     // let mut stderr = proc.stderr().ok_or(eyre!("stderr not available"))?;
//     let mut stdout =
//         tokio_util::io::ReaderStream::new(proc.stdout().ok_or(eyre!("stdout
// not available"))?);

//     loop {
//         tokio::select! {
//             message = input.recv() => {
//                 if let Some(message) = message {
//                     stdin.write_all(&message).await?;
//                 } else {
//                     break;
//                 }
//             }
//             message = stdout.next() => {
//                 info!("message: {:?}", message);

//                 if let Some(Ok(message)) = message {
//                     output.send(message).unwrap();
//                 } else {
//                     break;
//                 }
//             }
//         }
//     }

//     proc.join().await?;

//     Ok(())
// }

// pub struct Shell {
//     client: kube::Client,
//     pod: Arc<Pod>,
// }

// impl Shell {
//     pub fn new(client: kube::Client, pod: Arc<Pod>) -> Self {
//         Self { client, pod }
//     }

//     pub fn tab(name: String, client: kube::Client, pod: Arc<Pod>) -> Tab {
//         Tab::new(
//             name,
//             Box::new(move || Box::new(Self::new(client.clone(),
// pod.clone()))),         )
//     }
// }

// impl Widget for Shell {
//     fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
//         Ok(Broadcast::Ignored)
//     }

//     fn draw(&mut self, frame: &mut Frame, area: Rect) {}
// }
