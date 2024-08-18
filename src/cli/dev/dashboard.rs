use std::{io::Read, iter::Iterator, os::fd::AsRawFd, pin::Pin};

use cata::{Command, Container};
use clap::Parser;
use eyre::{eyre, Result};
use mio::{unix::SourceFd, Events, Interest, Poll};
use ratatui::{backend::CrosstermBackend, prelude::Backend, Terminal};
use replace_with::replace_with_or_abort;
use tokio::{
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    task::JoinSet,
    time::Duration,
};
use tokio_util::bytes::Bytes;

use crate::{
    events::{Broadcast, Event, Input, Keypress},
    widget::{apex::Apex, Raw, Widget},
};

#[derive(Parser, Container)]
pub struct Dashboard {
    #[arg(long, default_value = "100ms")]
    ticks: humantime::Duration,

    #[arg(long)]
    route: Vec<String>,
}

fn poll_stdin(tx: &UnboundedSender<Bytes>) -> Result<()> {
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(1024);

    let fd = std::io::stdin().as_raw_fd();
    let mut fd = SourceFd(&fd);

    poll.registry()
        .register(&mut fd, mio::Token(0), Interest::READABLE)?;

    loop {
        if tx.is_closed() {
            break;
        }

        poll.poll(&mut events, Some(Duration::from_millis(100)))?;

        for event in &events {
            if event.token() == mio::Token(0) {
                let mut buf = [0; 1024];
                let n = std::io::stdin().read(&mut buf)?;

                tx.send(Bytes::copy_from_slice(&buf[..n]))?;
            }
        }
    }

    Ok(())
}

async fn events(tick: Duration, sender: UnboundedSender<Event>) -> Result<()> {
    let mut tick = tokio::time::interval(tick);

    let (tx, mut rx) = unbounded_channel::<Bytes>();

    // While blocking tasks cannot be aborted, this *should* exit when this function
    // drops rx. Spawning the mio polling via spawn results in rx.recv() being
    // blocked without a yield happening.
    tokio::task::spawn_blocking(move || poll_stdin(&tx).unwrap());

    loop {
        tokio::select! {
            message = rx.recv() => {
                let Some(message) = message else {
                    break;
                };

                sender.send(message.try_into()?)?;
            }
            _ = tick.tick() => {
                sender.send(Event::Render)?;
            }
        }
    }

    drop(rx);

    Ok(())
}

enum Mode {
    UI(Box<dyn Widget>),
    Raw(Box<dyn Raw>, Box<dyn Widget>),
}

impl Mode {
    fn raw(&mut self, raw: Box<dyn Raw>) {
        replace_with_or_abort(self, |self_| match self_ {
            Self::UI(previous) | Self::Raw(_, previous) => Self::Raw(raw, previous),
        });
    }

    fn ui(&mut self) {
        replace_with_or_abort(self, |self_| match self_ {
            Self::Raw(_, previous) => Self::UI(previous),
            _ => self_,
        });
    }
}

fn dispatch(mode: &mut Mode, term: &mut Terminal<impl Backend>, ev: &Event) -> Result<Broadcast> {
    let Mode::UI(widget) = mode else {
        return Err(eyre!("expected UI mode"));
    };

    match ev {
        Event::Render => {}
        Event::Input(Input { key, .. }) => {
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
        widget.draw(frame, frame.size());
    })?;

    Ok(Broadcast::Ignored)
}

async fn raw(
    term: &mut Terminal<impl Backend>,
    raw_widget: &mut Box<dyn Raw>,
    input: &mut UnboundedReceiver<Event>,
) -> Result<()> {
    term.clear()?;
    term.reset_cursor()?;

    raw_widget
        .start(input, Pin::new(Box::new(tokio::io::stdout())))
        .await?;

    term.clear()?;

    Ok(())
}

async fn ui<W>(route: Vec<String>, mut rx: UnboundedReceiver<Event>, tx: W) -> Result<()>
where
    W: std::io::Write + Send + 'static,
{
    let mut term = Terminal::new(CrosstermBackend::new(tx))?;

    term.clear()?;

    let mut root = Apex::new(kube::Client::try_default().await?);

    root.dispatch(&Event::Goto(route.clone()))?;

    let mut state = Mode::UI(Box::new(root));

    while let Some(ev) = rx.recv().await {
        let result = match state {
            Mode::UI(_) => dispatch(&mut state, &mut term, &ev)?,
            Mode::Raw(ref mut raw_widget, ref mut current_widget) => {
                let result = raw(&mut term, raw_widget, &mut rx).await;

                current_widget.dispatch(&Event::Finished(result))?;

                state.ui();

                Broadcast::Ignored
            }
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

trait ClearScreen {
    fn reset_cursor(&mut self) -> Result<()>;
}

impl<B> ClearScreen for Terminal<B>
where
    B: Backend,
{
    fn reset_cursor(&mut self) -> Result<()> {
        self.draw(|frame| {
            frame.set_cursor(0, 0);
        })?;

        Ok(())
    }
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
