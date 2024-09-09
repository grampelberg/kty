use std::{pin::Pin, sync::Arc, vec};

use bon::Builder;
use chrono::{DateTime, Utc};
use eyre::{eyre, Result};
use futures::StreamExt;
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
    resources,
    resources::{pod, status::StatusExt, ContainerExt},
    widget::{
        container::Container,
        input::{Text, TextState},
        propagate,
        table::{self, CollectionView},
        tabs::Tab,
        Raw, Renderable, StatefulWidget, Widget, WIDGET_VIEWS,
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
    pod: pod::Store,
    // TODO: because this is nested under table, maybe it makes more sense to name it `Filtered`
    // since that's what it is.
    view: table::CollectionView<pod::Store>,
}

impl Shell {
    pub fn new(client: kube::Client, pod: Arc<Pod>) -> Self {
        WIDGET_VIEWS.container.list.inc();

        let view = CollectionView::builder()
            .table(table::Table::builder().build())
            .constructor(Command::from_pod(client, pod.clone()))
            .build();

        // TODO: need some way to select immediately.
        // if pod.as_ref().containers(None).len() == 1 {
        //     let _unused = table.enter(0, None);
        // }

        Self {
            pod: pod.into(),
            view,
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
        Container::dispatch(&mut self.view, event, &mut self.pod)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        if let Err(err) = StatefulWidget::draw(&mut self.view, frame, area, &mut self.pod) {
            tracing::error!("failed to draw table: {}", err);
        }

        Ok(())
    }
}

impl Renderable for Shell {}

enum CommandState {
    Input(Text<TextState>, TextState),
    Attached,
}

static COMMAND: &str = "/bin/bash";

struct Command {
    client: kube::Client,
    pod: Arc<Pod>,
    container: resources::Container,

    state: CommandState,
}

#[bon::bon]
impl Command {
    #[builder]
    pub fn new(client: kube::Client, pod: Arc<Pod>, container: resources::Container) -> Self {
        WIDGET_VIEWS.container.cmd.inc();

        let state = CommandState::Input(Command::input(&container), TextState::new(COMMAND));

        Self {
            client,
            pod,
            container,
            state,
        }
    }

    fn input(container: &resources::Container) -> Text<TextState> {
        Text::builder().title(container.name_any()).build()
    }

    pub fn from_pod(client: kube::Client, pod: Arc<Pod>) -> table::DetailFn<resources::Container> {
        Box::new(move |container| {
            Ok(Box::new(
                Command::builder()
                    .client(client.clone())
                    .pod(pod.clone())
                    .container(container.clone())
                    .build(),
            ))
        })
    }

    fn dispatch_input(&mut self, event: &Event) -> Result<Broadcast> {
        let cmd = {
            let CommandState::Input(widget, state) = &mut self.state else {
                return Ok(Broadcast::Ignored);
            };

            propagate!(widget.dispatch(event, state));

            state.as_ref().as_ref().ok_or(eyre!("no command"))?.clone()
        };

        match event.key() {
            Some(Keypress::Enter) => {
                self.state = CommandState::Attached;

                Ok(Broadcast::Raw(Box::new(
                    Exec::builder()
                        .start(Utc::now())
                        .client(self.client.clone())
                        .pod(self.pod.clone())
                        .container(self.container.clone())
                        .cmd(cmd)
                        .build(),
                )))
            }
            Some(Keypress::Escape) => Ok(Broadcast::Exited),
            _ => Ok(Broadcast::Ignored),
        }
    }

    fn draw_input(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let CommandState::Input(widget, state) = &mut self.state else {
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

        widget.draw(frame, area, state)
    }
}

impl Widget for Command {
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        propagate!(self.dispatch_input(event));

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
            CommandState::Input { .. } => self.draw_input(frame, area)?,
            CommandState::Attached => {}
        }

        Ok(())
    }
}
impl Renderable for Command {}

#[derive(Builder)]
struct Exec {
    start: DateTime<Utc>,
    client: kube::Client,
    pod: Arc<Pod>,
    container: resources::Container,
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
