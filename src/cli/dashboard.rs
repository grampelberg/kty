use std::{future::ready, iter::Iterator, sync::Arc};

use cata::{Command, Container};
use clap::Parser;
use crossterm::event::{self, EventStream};
use eyre::{eyre, Result};
use futures::{future::try_join_all, FutureExt, StreamExt};
use itertools::Itertools;
use k8s_openapi::api::core::v1::Pod;
use kube::{
    api::{ListParams, ObjectList},
    runtime::{
        reflector::{self},
        watcher,
        watcher::Config,
        WatchStreamExt,
    },
    Api,
};
use ratatui::{
    backend::{self, CrosstermBackend},
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Rect},
    terminal::TerminalOptions,
    text::Text,
    widgets::{self, Block, BorderType, Borders, Cell, Clear, Paragraph, Row, Widget, WidgetRef},
    Frame, Terminal, Viewport,
};
use serde::{de::DeserializeOwned, Deserialize};
use tokio::{
    pin,
    sync::{
        broadcast,
        mpsc::{self, UnboundedReceiver, UnboundedSender},
    },
    task::{JoinHandle, JoinSet},
    time::Duration,
};
use tracing::info;

use crate::events::{Event, Keypress};

#[derive(Parser, Container)]
pub struct Dashboard {
    #[arg(long, default_value = "100ms")]
    ticks: humantime::Duration,

    #[arg(long, default_value = "1s")]
    poll: humantime::Duration,
}

async fn events(tick: Duration, sender: UnboundedSender<Event>) -> Result<()> {
    let mut tick = tokio::time::interval(tick);
    let mut stream = EventStream::new();

    loop {
        let input = stream.next().fuse();

        tokio::select! {
            _ = tick.tick().fuse() => {
                sender.send(Event::Render)?;
            }
            Some(Ok(ev)) = input => {
                let crossterm::event::Event::Key(key) = ev else {
                    continue;
                };

                let key: Keypress = key.try_into()?;
                sender.send(Event::Keypress(key.clone()))?;

                if matches!(key, Keypress::EndOfText | Keypress::Escape) {
                    break;
                }
            }
        }
    }

    Ok(())
}

async fn ui<W>(mut rx: UnboundedReceiver<Event>, tx: W) -> Result<()>
where
    W: std::io::Write + Send + 'static,
{
    let mut term = Terminal::new(CrosstermBackend::new(tx))?;

    term.draw(|frame| {
        frame.render_widget(Clear, frame.size());
    })?;

    while let Some(ev) = rx.recv().await {
        match ev {
            Event::Render => {
                term.draw(|frame| {
                    let size = frame.size();

                    frame.render_widget(Clear, size);
                })?;
            }
            Event::Keypress(key) => {
                if matches!(key, Keypress::EndOfText | Keypress::Escape) {
                    break;
                }

                info!("keypress: {:?}", key);
            }
            _ => {}
        }
    }

    Ok(())
}

#[async_trait::async_trait]
impl Command for Dashboard {
    async fn run(&self) -> Result<()> {
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(std::io::stderr(), crossterm::terminal::EnterAlternateScreen)?;

        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel::<Event>();

        let mut js = JoinSet::new();

        js.spawn(events(self.ticks.into(), sender.clone()));
        js.spawn(ui(receiver, std::io::stdout()));

        // Exit when *anything* ends (on error or otherwise).
        while let Some(res) = js.join_next().await {
            res??;

            js.shutdown().await;
        }

        Ok(())
    }
}

impl Drop for Dashboard {
    fn drop(&mut self) {
        crossterm::terminal::disable_raw_mode().unwrap();
        crossterm::execute!(std::io::stderr(), crossterm::terminal::LeaveAlternateScreen).unwrap();
    }
}

struct PodTable {}

impl WidgetRef for PodTable {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        info!("rendering pod table");
    }
}

impl Dispatch for PodTable {
    fn dispatch(&mut self, event: Event) {
        info!("event: {:?}", event);
    }
}

struct Store<K>
where
    K: kube::Resource<DynamicType = ()>
        + Clone
        + std::fmt::Debug
        + Send
        + Sync
        + DeserializeOwned
        + 'static,
{
    task: JoinHandle<()>,
    reader: reflector::Store<K>,
}

impl<K> Store<K>
where
    K: kube::Resource<DynamicType = ()>
        + Clone
        + std::fmt::Debug
        + Send
        + Sync
        + DeserializeOwned
        + 'static,
{
    fn new(client: kube::Client) -> Self {
        println!("stuff: {}", std::any::type_name::<K>());

        // let client = kube::Client::try_default().await?;
        let (reader, writer) = reflector::store();
        let stream = watcher(Api::<K>::all(client), Config::default())
            .default_backoff()
            .reflect(writer)
            .applied_objects()
            .boxed();

        let task = tokio::spawn(async move { stream.for_each(|_| ready(())).await });

        Self { task, reader }
    }

    fn state(&self) -> Vec<Arc<K>> {
        self.reader.state()
    }
}

impl<K> Drop for Store<K>
where
    K: kube::Resource<DynamicType = ()>
        + Clone
        + std::fmt::Debug
        + Send
        + Sync
        + DeserializeOwned
        + 'static,
{
    fn drop(&mut self) {
        self.task.abort();
    }
}

trait Dispatch {
    fn dispatch(&mut self, event: Event);
}
