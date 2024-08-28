use std::{pin::Pin, sync::Arc, vec};

use chrono::{DateTime, Utc};
use color_eyre::{Section, SectionExt};
use derive_builder::Builder;
use eyre::{eyre, Result};
use futures::StreamExt;
use itertools::Itertools;
use k8s_openapi::api::core::v1::Pod;
use kube::{
    api::{Api, AttachParams},
    ResourceExt,
};
use lazy_static::lazy_static;
use prometheus::{histogram_opts, register_histogram, Histogram};
use ratatui::{layout::Rect, prelude::*};
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    sync::mpsc::UnboundedReceiver,
};
use tokio_util::io::ReaderStream;

use crate::{
    events::{Broadcast, Event, Keypress},
    resources::{
        container::{Container, ContainerExt},
        pod::{PodExt, StatusError, StatusExt},
    },
    widget::{
        input::Text,
        propagate,
        table::{DetailFn, Table},
        tabs::Tab,
        Raw, Widget, WIDGET_VIEWS,
    },
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
    pod: Arc<Pod>,
    table: Table,
}

impl Shell {
    pub fn new(client: kube::Client, pod: Arc<Pod>) -> Self {
        WIDGET_VIEWS.container.list.inc();

        let mut table = Table::default().constructor(Command::from_pod(client, pod.clone()));

        if pod.as_ref().containers(None).len() == 1 {
            let _unused = table.enter(0, None);
        }

        Self { pod, table }
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
        self.table.dispatch(event)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        if let Err(err) = self.table.draw(frame, area, &self.pod) {
            tracing::info!("failed to draw table: {}", err);
        }

        Ok(())
    }
}

enum CommandState {
    Input(Text),
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

    fn input(container: &Container) -> Text {
        Text::default()
            .with_title(container.name_any())
            .with_content(COMMAND)
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

    fn dispatch_input(&mut self, event: &Event) -> Result<Broadcast> {
        let cmd = {
            let CommandState::Input(ref mut txt) = self.state else {
                return Ok(Broadcast::Ignored);
            };

            propagate!(txt.dispatch(event));

            txt.content().to_string()
        };

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
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        propagate!(self.dispatch_input(event));

        match event {
            Event::Finished(result) => result.as_ref().map(|()| Broadcast::Exited).map_err(|err| {
                let Some(err) = err.downcast_ref::<StatusError>() else {
                    return eyre!(err.to_string());
                };

                let separated = err.message.splitn(8, ':');

                eyre!(
                    "{}",
                    separated.clone().last().unwrap_or("unknown error").trim()
                )
                .section(
                    separated
                        .with_position()
                        .map(|(i, line)| {
                            let l = line.trim().to_string();

                            match i {
                                itertools::Position::Middle => format!("├─ {l}"),
                                itertools::Position::Last => format!("└─ {l}"),
                                _ => l,
                            }
                        })
                        .join("\n")
                        .header("Raw:"),
                )
            }),
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
                    Err(StatusError::new(status))
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
