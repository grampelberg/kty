use std::iter::Iterator;

use cata::{Command, Container};
use clap::Parser;
use crossterm::event::EventStream;
use eyre::Result;
use futures::{FutureExt, StreamExt};
use ratatui::{backend::CrosstermBackend, prelude::*, widgets::Clear, Terminal};
use tokio::{
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
    task::JoinSet,
    time::Duration,
};

use crate::{
    events::{Event, Keypress},
    widget::{pod::PodTable, Dispatch},
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
