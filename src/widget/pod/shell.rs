use std::{pin::Pin, sync::Arc, vec};

use chrono::{DateTime, Utc};
use derive_builder::Builder;
use eyre::{eyre, Result};
use futures::StreamExt;
use k8s_openapi::api::core::v1::Pod;
use kube::{
    api::{Api, AttachParams},
    ResourceExt,
};
use lazy_static::lazy_static;
use prometheus::{histogram_opts, register_histogram, Histogram};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    Frame,
};
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    sync::mpsc::UnboundedReceiver,
};
use tokio_util::io::ReaderStream;

use crate::{
    events::{Broadcast, Event, Keypress},
    resources::{
        container::{Container, ContainerExt},
        pod::PodExt,
        status::StatusExt,
    },
    widget::{input, input::ContentExt, propagate, table, tabs::Tab, Raw, Widget, WIDGET_VIEWS},
};

lazy_static! {
    static ref EXEC_DURATION: Histogram = register_histogram!(histogram_opts!(
        "container_exec_duration_minutes",
        "The time spent exec'd into a container in a pod",
        vec!(0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0),
    ))
    .unwrap();
}

pub struct Shell {
    view: table::Filtered,
}

#[bon::bon]
impl Shell {
    #[builder]
    pub fn new(client: kube::Client, pod: Arc<Pod>) -> Self {
        WIDGET_VIEWS.container.list.inc();

        let len = pod.as_ref().containers(None).len();

        let mut view = table::Filtered::builder()
            .table(
                table::Table::builder()
                    .items(pod.clone())
                    .border(false)
                    .build(),
            )
            .constructor(Command::from_pod(client, pod))
            .build();

        if len == 1 {
            view.select(0).expect("can select");
        }

        Self { view }
    }

    pub fn tab(name: String, client: kube::Client, pod: Arc<Pod>) -> Tab {
        Tab::new(
            name,
            Box::new(move || {
                Box::new(
                    Self::builder()
                        .client(client.clone())
                        .pod(pod.clone())
                        .build(),
                )
            }),
        )
    }
}

impl Widget for Shell {
    fn dispatch(&mut self, event: &Event, buffer: &Buffer, area: Rect) -> Result<Broadcast> {
        self.view.dispatch(event, buffer, area)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        self.view.draw(frame, area)
    }
}

enum CommandState {
    Input(input::Text),
    Attached,
}

static COMMAND: &str = "/bin/bash";

struct Command {
    client: kube::Client,
    pod: Arc<Pod>,
    container: Container,

    state: CommandState,
}

impl Command {
    pub fn new(client: kube::Client, pod: Arc<Pod>, container: Container) -> Self {
        WIDGET_VIEWS.container.cmd.inc();

        let state = CommandState::Input(Command::input(&container));

        Self {
            client,
            pod,
            container,
            state,
        }
    }

    fn input(container: &Container) -> input::Text {
        input::Text::builder()
            .title(container.name_any())
            .content(input::Content::from_string(COMMAND))
            .build()
    }

    pub fn from_pod(client: kube::Client, pod: Arc<Pod>) -> table::DetailFn {
        Box::new(move |idx, filter| {
            let containers = pod.containers(filter);

            Ok(Command::new(
                client.clone(),
                pod.clone(),
                containers.get(idx).unwrap().clone(),
            )
            .boxed())
        })
    }

    fn dispatch_input(&mut self, event: &Event, buffer: &Buffer, area: Rect) -> Result<Broadcast> {
        let CommandState::Input(ref mut txt) = self.state else {
            return Ok(Broadcast::Ignored);
        };

        propagate!(txt.dispatch(event, buffer, area));

        let cmd = txt
            .content()
            .borrow()
            .as_ref()
            .map_or(String::new(), String::clone);

        match event.key() {
            Some(Keypress::Enter) => {
                self.state = CommandState::Attached;

                Ok(Broadcast::Raw(Box::new(
                    ExecBuilder::default()
                        .start(Utc::now())
                        .client(self.client.clone())
                        .pod(self.pod.clone())
                        .container(self.container.clone())
                        .cmd(cmd)
                        .build()?,
                )))
            }
            Some(Keypress::Escape) => Ok(Broadcast::Exited),
            _ => Ok(Broadcast::Ignored),
        }
    }

    fn draw_input(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let CommandState::Input(ref mut txt) = self.state else {
            return Ok(());
        };

        let [_, area, _] = Layout::vertical([
            Constraint::Fill(0),
            Constraint::Length(3),
            Constraint::Fill(0),
        ])
        .areas(area);

        let [_, area, _] = Layout::horizontal([
            Constraint::Max(10),
            Constraint::Fill(0),
            Constraint::Max(10),
        ])
        .areas(area);

        txt.draw(frame, area)
    }
}

impl Widget for Command {
    fn dispatch(&mut self, event: &Event, buffer: &Buffer, area: Rect) -> Result<Broadcast> {
        propagate!(self.dispatch_input(event, buffer, area));

        match event {
            Event::Finished(result) => Ok(result
                .as_ref()
                .map(|()| Broadcast::Exited)
                .map_err(std::clone::Clone::clone)?),
            _ => Ok(Broadcast::Ignored),
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        match self.state {
            CommandState::Input(_) => self.draw_input(frame, area)?,
            CommandState::Attached => {}
        }

        Ok(())
    }

    fn zindex(&self) -> u16 {
        1
    }
}

#[derive(Builder)]
struct Exec {
    start: DateTime<Utc>,
    client: kube::Client,
    pod: Arc<Pod>,
    container: Container,
    cmd: String,
}

#[async_trait::async_trait]
impl Raw for Exec {
    #[allow(clippy::blocks_in_conditions)]
    #[tracing::instrument(skip(self, stdin, stdout), fields(activity = "pod.exec"))]
    async fn start(
        &mut self,
        stdin: &mut UnboundedReceiver<Event>,
        mut stdout: Pin<Box<dyn AsyncWrite + Send + Unpin>>,
    ) -> Result<()> {
        WIDGET_VIEWS.container.exec.inc();

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

                    let Event::Input(incoming) = &msg else {
                        continue;
                    };

                    input.write_all(incoming.into()).await?;
                    input.flush().await?;

                    if matches!(msg.key(), Some(Keypress::Control('b'))) {
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

        status
            .await
            .map(|status| {
                if status.is_success() {
                    Ok(())
                } else {
                    Err(status.into_report())
                }
            })
            .ok_or(eyre!("status not available"))??;

        proc.join().await?;

        Ok(())
    }
}

impl Drop for Exec {
    fn drop(&mut self) {
        EXEC_DURATION.observe(
            (Utc::now() - self.start)
                .to_std()
                .expect("duration in range")
                .as_secs_f64()
                / 60.0,
        );
    }
}
