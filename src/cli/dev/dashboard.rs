use std::{borrow::BorrowMut, iter::Iterator, ops::Deref};

use cata::{Command, Container};
use clap::Parser;
use crossterm::event::EventStream;
use eyre::{eyre, Result};
use futures::{FutureExt, StreamExt};
use ratatui::{backend::CrosstermBackend, prelude::Backend, widgets::Clear, Terminal};
use replace_with::replace_with_or_abort;
use tokio::{
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
    task::JoinSet,
    time::Duration,
};
use tracing::info;

use crate::{
    events::{Broadcast, Event, Keypress},
    widget::{
        pod::{self},
        Widget,
    },
};

#[derive(Parser, Container)]
pub struct Dashboard {
    #[arg(long, default_value = "100ms")]
    ticks: humantime::Duration,

    #[arg(long)]
    route: Vec<String>,
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

                info!("key: {:?}", key);

                let key: Keypress = key.try_into()?;
                sender.send(Event::Keypress(key.clone()))?;

                info!("key: {:?}", key);

                if matches!(key, Keypress::EndOfText) {
                    break;
                }
            }
        }
    }

    Ok(())
}

enum Mode {
    UI(Box<dyn Widget>),
    Raw(Box<dyn Widget>, Box<dyn Widget>),
}

impl Mode {
    fn raw(&mut self, widget: Box<dyn Widget>) {
        replace_with_or_abort(self, |self_| match self_ {
            Self::UI(previous) | Self::Raw(_, previous) => Self::Raw(widget, previous),
        });
    }
}

fn dispatch(mode: &mut Mode, term: &mut Terminal<impl Backend>, ev: &Event) -> Result<Broadcast> {
    let Mode::UI(widget) = mode else {
        return Err(eyre!("expected UI mode"));
    };

    match ev {
        Event::Render => {}
        Event::Keypress(key) => {
            if matches!(key, Keypress::EndOfText) {
                return Ok(Broadcast::Exited);
            }

            return widget.dispatch(ev);
        }
        _ => {
            return Ok(Broadcast::Ignored);
        }
    }

    term.draw(|frame| {
        let area = frame.size();

        widget.draw(frame, area);
    })?;

    Ok(Broadcast::Ignored)
}

async fn ui<W>(route: Vec<String>, mut rx: UnboundedReceiver<Event>, tx: W) -> Result<()>
where
    W: std::io::Write + Send + 'static,
{
    let mut term = Terminal::new(CrosstermBackend::new(tx))?;

    term.draw(|frame| {
        frame.render_widget(Clear, frame.size());
    })?;

    let mut root = pod::List::new(kube::Client::try_default().await?);
    root.dispatch(&Event::Goto(route.clone()))?;

    let mut state = Mode::UI(Box::new(root));

    while let Some(ev) = rx.recv().await {
        let result = match state {
            Mode::UI(_) => dispatch(&mut state, &mut term, &ev)?,
            Mode::Raw(_, _) => Broadcast::Ignored,
        };

        match result {
            Broadcast::Exited => {
                break;
            }
            Broadcast::Raw(widget) => {
                state.raw(widget);
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

        let mut background = JoinSet::new();

        background.spawn(events(self.ticks.into(), sender.clone()));
        background.spawn(ui(self.route.clone(), receiver, std::io::stdout()));

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
