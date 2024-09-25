use std::sync::Arc;

use color_eyre::{Section, SectionExt};
use eyre::{eyre, Report, Result};
use futures::{
    future::{try_join_all, BoxFuture},
    io::AsyncBufRead,
    stream, AsyncBufReadExt, FutureExt, TryStreamExt,
};
use k8s_openapi::api::core::v1::Pod;
use kube::{api::LogParams, Api, ResourceExt};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{palette::tailwind, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use tokio::{
    sync::{mpsc, mpsc::UnboundedSender},
    task::JoinHandle,
};

use super::{
    nav::{move_cursor, Movement},
    tabs::Tab,
    viewport::Viewport,
    Widget, WIDGET_VIEWS,
};
use crate::{
    events::{Broadcast, Event},
    resources::{
        container::{Container, ContainerExt},
        pod::PodExt,
    },
};

pub struct Log {
    task: Option<JoinHandle<Result<()>>>,

    rx: mpsc::UnboundedReceiver<String>,
    buffer: Vec<String>,

    position: Position,
}

// TODO:
// - Make this work with with anything that has pods (e.g. deployments,
//   stateful).
// - Allow for searching within the logs. Feels like it should be ala fzf and
//   jump to the text + highlight it.
// - Only fetch the most recent X lines, on scroll-back, fetch more.
// - Convert into something more general, this is fundamentally the same thing
//   as the yaml widget - but without the syntax highlighting. There should
//   probably be an "editor" widget that takes something to populate the lines.
impl Log {
    #[allow(clippy::blocks_in_conditions)]
    #[tracing::instrument(skip(client, pod), fields(activity = "pod.logs"))]
    pub fn new(client: kube::Client, pod: Arc<Pod>) -> Self {
        WIDGET_VIEWS.pod.log.inc();

        let (tx, rx) = mpsc::unbounded_channel();

        // TODO: this should be a function call.
        let task = tokio::spawn(log_stream(
            client,
            pod,
            tx,
            LogParams {
                follow: true,
                pretty: true,
                previous: true,
                ..Default::default()
            },
            true,
        ));

        Self {
            task: Some(task),
            rx,
            buffer: Vec::new(),

            position: Position::default(),
        }
    }

    // TODO: This should be a macro. Ideally, it'd be a trait with a default impl
    // but I don't think it is possible to do generically.
    pub fn tab(name: String, client: kube::Client, pod: Arc<Pod>) -> Tab {
        Tab::builder()
            .name(name)
            .constructor(Box::new(move || {
                Log::new(client.clone(), pod.clone()).boxed().into()
            }))
            .build()
    }

    fn update(&mut self) -> u16 {
        let mut i = 0;

        while let Ok(line) = self.rx.try_recv() {
            self.buffer.push(line);
            i += 1;
        }

        i
    }
}

impl Widget for Log {
    fn dispatch(&mut self, event: &Event, _: &Buffer, area: Rect) -> Result<Broadcast> {
        let Some(key) = event.key() else {
            return Ok(Broadcast::Ignored);
        };

        if let Some(Movement::Y(y)) = move_cursor(key, area) {
            self.position.y = self.position.y.saturating_add_signed(y);

            return Ok(Broadcast::Consumed);
        }

        Ok(Broadcast::Ignored)
    }

    #[allow(clippy::cast_possible_truncation)]
    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let lines = self.update();

        if self
            .position
            .y
            .saturating_add(lines)
            .saturating_add(area.height)
            >= self.buffer.len() as u16
        {
            self.position.y = u16::MAX;
        }

        if self.task.as_ref().map_or(false, JoinHandle::is_finished) {
            let task = self.task.take().expect("task is finished");

            match futures::executor::block_on(async move { task.await? }) {
                Ok(()) => return Err(eyre!("Log task finished unexpectedly")),
                Err(err) => {
                    let Some(kube::Error::Api(resp)) = err.downcast_ref::<kube::Error>() else {
                        return Err(err);
                    };

                    return Err(
                        eyre!("{}", resp.message).section(format!("{resp:#?}").header("Raw:"))
                    );
                }
            }
        }

        let block = Block::default().borders(Borders::ALL);

        let inner = block.inner(area);

        let result = Viewport::builder()
            .buffer(&self.buffer)
            .view(self.position)
            .build()
            .draw(frame, inner);

        if self.task.is_none() {
            frame.render_widget(
                Paragraph::new("Log stream ended, come back to restart")
                    .style(Style::default().fg(tailwind::RED.c300))
                    .centered(),
                inner,
            );
        }

        frame.render_widget(block, area);

        result
    }
}

impl Drop for Log {
    fn drop(&mut self) {
        if let Some(task) = self.task.as_ref() {
            task.abort();
        }
    }
}

#[tracing::instrument(skip(client, pod, tx, params))]
fn log_stream<'a>(
    client: kube::Client,
    pod: Arc<Pod>,
    tx: UnboundedSender<String>,
    params: LogParams,
    retry: bool,
) -> BoxFuture<'a, Result<()>> {
    async move {
        let pod_client = Api::<Pod>::namespaced(client.clone(), &pod.namespace().unwrap());

        let containers = try_join_all(pod.containers(None).iter().map(|c| {
            let mut params = params.clone();
            params.container = Some(c.name_any());

            container_stream(&pod_client, c, params)
        }))
        .await?;

        let mut all_logs = stream::select_all(containers.into_iter().map(AsyncBufReadExt::lines));

        while let Some(line) = all_logs.try_next().await? {
            tx.send(line)?;
        }

        tracing::debug!(pod = pod.name_any(), "stream ended");

        // The api server is a little finicky about streaming previous logs. It is
        // possible that the request succeeds, but also that the stream finishes
        // immediately. If this happens, retry without the previous flag. Because we're
        // not clearing out the buffer on the widget side of things, the logs will be
        // duplicated. Before the restart, we send a simple message to tell the user
        // what happened.
        if retry {
            tx.send("Stream terminated, retrying without previous logs".to_string())?;

            let mut new_params = params.clone();
            new_params.previous = false;

            return log_stream(client, pod, tx, new_params, false).await;
        }

        Ok(())
    }
    .boxed()
}

fn container_stream<'a>(
    client: &'a Api<Pod>,
    container: &'a Container,
    params: LogParams,
) -> BoxFuture<'a, Result<impl AsyncBufRead>> {
    async move {
        match client.log_stream(&container.pod_name(), &params).await {
            Ok(stream) => Ok(stream),
            Err(err) => {
                let kube::Error::Api(resp) = &err else {
                    return Err(Report::new(err));
                };

                if resp.message.contains("previous terminated") {
                    let mut new_params = params.clone();

                    new_params.previous = false;

                    return container_stream(client, container, new_params).await;
                }

                Err(eyre!(err))
            }
        }
    }
    .boxed()
}
