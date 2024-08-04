use std::{
    future::ready,
    iter::Iterator,
    sync::{Arc, Mutex},
};

use cata::{Command, Container};
use clap::Parser;
use crossterm::event::{self, EventStream};
use eyre::{eyre, Result};
use futures::{future::try_join_all, FutureExt, StreamExt};
use itertools::Itertools;
use k8s_openapi::api::core::v1::Pod;
use kube::{
    api::{ListParams, ObjectList},
    runtime,
    runtime::{
        reflector::{self},
        watcher::{self, Config},
        WatchStreamExt,
    },
    Api, ResourceExt,
};
use ratatui::{
    backend::{self, CrosstermBackend},
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Rect},
    prelude::*,
    style::{palette::tailwind, Modifier, Style},
    terminal::TerminalOptions,
    text::Text,
    widgets::{
        self, Block, BorderType, Borders, Cell, Clear, Paragraph, Row, StatefulWidget, Table,
        TableState, Widget, WidgetRef,
    },
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

use crate::{
    events::{Event, Keypress},
    resources::{pod, pod::PodExt},
    widget::TableRow,
};

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

    let mut root = PodTable::new(kube::Client::try_default().await?);

    while let Some(ev) = rx.recv().await {
        match ev.clone() {
            Event::Render => {}
            Event::Keypress(key) => {
                if matches!(key, Keypress::EndOfText | Keypress::Escape) {
                    break;
                }

                root.dispatch(ev);
            }
            _ => {
                continue;
            }
        }

        term.draw(|frame| {
            let size = frame.size();

            frame.render_widget(&root, size);
        })?;
    }

    Ok(())
}

#[async_trait::async_trait]
impl Command for Dashboard {
    async fn run(&self) -> Result<()> {
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(std::io::stderr(), crossterm::terminal::EnterAlternateScreen)?;

        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel::<Event>();

        let mut background = JoinSet::new();

        background.spawn(events(self.ticks.into(), sender.clone()));
        background.spawn(ui(receiver, std::io::stdout()));

        // Exit when *anything* ends (on error or otherwise).
        while let Some(res) = background.join_next().await {
            res??;

            background.shutdown().await;
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

struct RowStyle {
    healthy: Style,
    unhealthy: Style,
    normal: Style,
}

impl Default for RowStyle {
    fn default() -> Self {
        Self {
            healthy: Style::default().fg(tailwind::GREEN.c300),
            unhealthy: Style::default().fg(tailwind::RED.c300),
            normal: Style::default().fg(tailwind::INDIGO.c300),
        }
    }
}

struct TableStyle {
    border: Style,
    header: Style,
    selected: Style,
    row: RowStyle,
}

impl Default for TableStyle {
    fn default() -> Self {
        Self {
            border: Style::default(),
            header: Style::default().bold(),
            selected: Style::default().add_modifier(Modifier::REVERSED),
            row: RowStyle::default(),
        }
    }
}

struct PodTable {
    pods: Store<Pod>,
    table: TableState,
}

impl PodTable {
    fn new(client: kube::Client) -> Self {
        Self {
            pods: Store::new(client),
            table: TableState::default().with_selected(0),
        }
    }
}

impl WidgetRef for PodTable {
    // TODO: implement a loading screen.
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let style = TableStyle::default();

        let border = Block::default()
            .title("Pods")
            .borders(Borders::ALL)
            .style(style.border);

        let state = self.pods.state();

        let rows = state
            .iter()
            .map(|pod| {
                let row = pod.row();

                match pod.status() {
                    pod::Phase::Pending | pod::Phase::Running => row.style(style.row.normal),
                    pod::Phase::Succeeded => row.style(style.row.healthy),
                    pod::Phase::Unknown(_) => row.style(style.row.unhealthy),
                }
            })
            .collect_vec();

        let table = Table::new(rows, Pod::constraints())
            .header(Pod::header().style(style.header))
            .block(border)
            .highlight_style(style.selected);
        StatefulWidget::render(&table, area, buf, &mut self.table.clone());
    }
}

impl Dispatch for PodTable {
    fn dispatch(&mut self, event: Event) {
        let Event::Keypress(key) = event else {
            return;
        };

        let current = self.table.selected().unwrap_or_default();

        let next = match key {
            Keypress::CursorUp => {
                if current == 0 {
                    0
                } else {
                    current - 1
                }
            }
            Keypress::CursorDown => {
                if current == self.pods.state().len() - 1 {
                    current
                } else {
                    current + 1
                }
            }
            _ => return,
        };

        self.table.select(Some(next));
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
    // TODO: need to have a way to filter stuff out (with some defaults) to keep
    // from memory going nuts.
    fn new(client: kube::Client) -> Self {
        let (reader, writer) = reflector::store();
        let stream = runtime::watcher(Api::<K>::all(client), Config::default())
            .default_backoff()
            .reflect(writer)
            .applied_objects()
            .boxed();

        let task = tokio::spawn(async move {
            stream.for_each(|_| ready(())).await;
        });

        Self { task, reader }
    }

    fn state(&self) -> Vec<Arc<K>> {
        self.reader.state()
    }

    // TODO: the naive implementation of this (loading is false on first element of
    // the stream), happens *fast*. It feels like there should be *something* that
    // comes back when the initial sync has fully completed but I can't find
    // anything in kube-rs yet that does that.
    fn loading(&self) -> bool {
        false
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
