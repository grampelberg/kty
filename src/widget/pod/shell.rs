use std::{pin::Pin, sync::Arc, vec};

use derive_builder::Builder;
use eyre::{eyre, Result};
use futures::StreamExt;
use k8s_openapi::api::core::v1::Pod;
use kube::{
    api::{Api, AttachParams},
    ResourceExt,
};
use ratatui::{
    layout::Rect,
    prelude::*,
    style::{palette::tailwind, Modifier, Style},
    widgets,
    widgets::{Block, Borders, Row},
};
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    sync::mpsc::UnboundedReceiver,
};
use tokio_util::{bytes::Bytes, io::ReaderStream};

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
        Raw, Widget,
    },
};

pub struct Shell {
    pod: Arc<Pod>,
    table: Table,
}

impl Shell {
    pub fn new(client: kube::Client, pod: Arc<Pod>) -> Self {
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

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        self.table.render(frame, area, &self.pod);
    }
}

enum CommandState {
    Input(Text),
    Attached,
    Error(String),
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

        match event {
            Event::Keypress(Keypress::Enter) => {
                self.state = CommandState::Attached;

                Ok(Broadcast::Raw(Box::new(
                    ExecBuilder::default()
                        .client(self.client.clone())
                        .pod(self.pod.clone())
                        .container(self.container.clone())
                        .cmd(cmd)
                        .build()?,
                )))
            }
            Event::Keypress(Keypress::Escape) => Ok(Broadcast::Exited),
            _ => Ok(Broadcast::Ignored),
        }
    }

    #[allow(clippy::unnecessary_wraps)]
    fn dispatch_error(&mut self, event: &Event) -> Result<Broadcast> {
        if !matches!(self.state, CommandState::Error(_)) {
            return Ok(Broadcast::Ignored);
        }

        match event {
            // TODO: should handle scrolling inside the error message.
            Event::Keypress(_) => {
                self.state = CommandState::Input(Command::input(&self.container));

                Ok(Broadcast::Consumed)
            }
            _ => Ok(Broadcast::Ignored),
        }
    }

    fn draw_input(&mut self, frame: &mut Frame, area: Rect) {
        let CommandState::Input(ref mut txt) = self.state else {
            return;
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

        txt.draw(frame, area);
    }

    // TODO: this should be a separate widget of its own.
    #[allow(clippy::cast_possible_truncation)]
    fn draw_error(&mut self, frame: &mut Frame, area: Rect) {
        let CommandState::Error(ref err) = self.state else {
            return;
        };

        let block = Block::default()
            .title("Error")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red))
            .title_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD));

        // TODO: get this into a variable so that it can be styled.
        let rows: Vec<Row> = err
            .split('\n')
            .enumerate()
            .map(|(i, line)| {
                Row::new(vec![
                    Span::from(format!("{i}: "))
                        .style(style::Style::default().fg(tailwind::RED.c300)),
                    Span::from(line),
                ])
            })
            .collect();

        let height = rows.len() as u16 + 2;

        let content =
            widgets::Table::new(rows, vec![Constraint::Max(3), Constraint::Fill(0)]).block(block);

        let [_, area, _] = Layout::horizontal([
            Constraint::Max(10),
            Constraint::Fill(0),
            Constraint::Max(10),
        ])
        .areas(area);

        let [_, mut vert, _] = Layout::vertical([
            Constraint::Max(10),
            Constraint::Fill(0),
            Constraint::Max(10),
        ])
        .areas(area);

        if vert.height > height {
            vert.height = height;
        }

        frame.render_widget(content, vert);
    }
}

impl Widget for Command {
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        propagate!(self.dispatch_input(event));
        propagate!(self.dispatch_error(event));

        match event {
            Event::Finished(result) => {
                let Err(err) = result else {
                    return Ok(Broadcast::Exited);
                };

                self.state = CommandState::Error(err.to_string());

                Ok(Broadcast::Consumed)
            }
            _ => Ok(Broadcast::Ignored),
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        match self.state {
            CommandState::Input(_) => self.draw_input(frame, area),
            CommandState::Attached => {}
            CommandState::Error(_) => self.draw_error(frame, area),
        }
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

// match err.downcast::<StatusError>() {
//     Ok(status) => {
//         info!(?status, "error executing command");
//     }
//     Err(err) => return Err(err),
// }

// write!(f, "{lines}")

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
