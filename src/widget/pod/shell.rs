use std::{borrow::BorrowMut, str, sync::Arc};

use eyre::{eyre, Result};
use futures::StreamExt;
use k8s_openapi::api::core::v1::Pod;
use kube::{
    api::{
        Api, AttachParams, AttachedProcess, DeleteParams, PostParams, ResourceExt, TerminalSize,
    },
    runtime::wait::{await_condition, conditions::is_pod_running},
};
use ratatui::{
    layout::Rect,
    style::{palette::tailwind, Style},
    text::Line,
    widgets::Paragraph,
    Frame,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    runtime::{Handle, Runtime},
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
    task::{spawn_blocking, JoinHandle},
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

    stdin: Option<UnboundedSender<Bytes>>,
    stdout: Option<UnboundedReceiver<Bytes>>,
    buffer: Vec<String>,
    task: Option<JoinHandle<Result<()>>>,

    // TODO: there must be a better way to handle the error than this.
    error: Option<eyre::Report>,
}

impl Shell {
    pub fn new(client: kube::Client, pod: Arc<Pod>) -> Self {
        Self {
            client,
            pod,
            stdin: None,
            stdout: None,
            buffer: Vec::new(),
            task: None,
            error: None,
        }
    }

    pub fn tab(name: String, client: kube::Client, pod: Arc<Pod>) -> Tab {
        Tab::new(
            name,
            Box::new(move || Box::new(Self::new(client.clone(), pod.clone()))),
        )
    }

    pub fn start(&mut self) {
        let (stdout_tx, stdout_rx) = tokio::sync::mpsc::unbounded_channel();
        let (stdin_tx, stdin_rx) = tokio::sync::mpsc::unbounded_channel();

        let task = tokio::spawn(exec(
            self.client.clone(),
            self.pod.clone(),
            stdin_rx,
            stdout_tx,
        ));

        self.stdin = Some(stdin_tx);
        self.stdout = Some(stdout_rx);
        self.task = Some(task);
    }

    fn status(&mut self) {
        let Some(task) = self.task.borrow_mut() else {
            return;
        };

        let result = futures::executor::block_on(async move { task.await? });

        self.task = None;

        let Err(err) = result else {
            return;
        };

        self.error = Some(err);
    }
}

impl Widget for Shell {
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        let Event::Keypress(key) = event else {
            return Ok(Broadcast::Ignored);
        };

        if self.task.is_none() {
            self.start();
        }

        Ok(Broadcast::Ignored)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        if let Some(task) = &self.task {
            if task.is_finished() {
                self.status();
            }
        }

        // TODO: render a popup that can be dismissed.
        if let Some(err) = &self.error {
            frame.render_widget(
                Paragraph::new(format!("{err:?}")).style(Style::default()),
                area,
            );

            return;
        }

        if self.task.is_some() {
            while let Ok(line) = self.stdout.as_mut().unwrap().try_recv() {
                info!("line: {:?}", line);
                // self.buffer.push(str::from_utf8(&line).unwrap().to_string());
            }
        }

        // frame.render_widget(
        //     Paragraph::new(
        //         self.buffer
        //             .iter()
        //             .map(|x| Line::from(x.as_str()))
        //             .collect::<Vec<Line>>(),
        //     ),
        //     area,
        // );
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
