use std::sync::Arc;

use eyre::{eyre, Result};
use futures::channel::mpsc::UnboundedReceiver;
use k8s_openapi::api::core::v1::Pod;
use ratatui::{
    layout::Rect,
    prelude::*,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
};
use tokio::io::AsyncWrite;
use tokio_util::bytes::Bytes;
use tracing::info;

use crate::{
    events::{Broadcast, Event, Keypress},
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
        propagate!(self.table.dispatch(event), {});

        Ok(Broadcast::Ignored)
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
    txt: Text,
}

impl Command {
    pub fn new() -> Self {
        Self {
            txt: Text::default()
                .with_title("Command")
                .with_content("/bin/bash"),
        }
    }

    pub fn from_pod(client: kube::Client, pod: Arc<Pod>) -> DetailFn {
        Box::new(move |idx, filter| Ok(Box::new(Command::new())))
    }
}

impl Widget for Command {
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        propagate!(self.txt.dispatch(event), return Ok(Broadcast::Exited));

        match event {
            Event::Keypress(Keypress::Enter) => {
                info!("executing command: {:?}", self.txt.content());

                Ok(Broadcast::Raw(Box::new(Exec::new())))
            }
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

struct Exec {}

impl Exec {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait::async_trait]
impl Raw for Exec {
    async fn start(
        &mut self,
        stdin: UnboundedReceiver<Bytes>,
        stdout: Box<dyn AsyncWrite + Send>,
    ) -> Result<()> {
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
