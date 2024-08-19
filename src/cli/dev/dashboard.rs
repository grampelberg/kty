use std::{io::Read, iter::Iterator, os::fd::AsRawFd};

use cata::{Command, Container};
use clap::Parser;
use eyre::Result;
use mio::{unix::SourceFd, Events, Interest, Poll};
use ratatui::{backend::WindowSize, layout::Size};
use tokio::{
    sync::mpsc::{unbounded_channel, UnboundedSender},
    time::Duration,
};
use tokio_util::bytes::Bytes;

use crate::{dashboard::Dashboard as UIDashboard, events::Event, io::Writer};

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

#[async_trait::async_trait]
impl Command for Dashboard {
    async fn run(&self) -> Result<()> {
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(std::io::stdout(), crossterm::terminal::EnterAlternateScreen)?;

        let (tx, mut rx) = unbounded_channel::<Bytes>();
        let (stop_tx, mut stop_rx) = unbounded_channel::<()>();

        // While blocking tasks cannot be aborted, this *should* exit when this function
        // drops rx. Spawning the mio polling via spawn results in rx.recv() being
        // blocked without a yield happening.
        tokio::task::spawn_blocking(move || poll_stdin(&tx).unwrap());

        let mut dashboard = UIDashboard::new(kube::Client::try_default().await?);

        dashboard.start(LocalWriter { stop: stop_tx })?;

        // TODO: listen to resize events and publish them to the dashboard.
        let (cx, cy) = crossterm::terminal::size()?;
        dashboard.send(Event::Resize(WindowSize {
            columns_rows: Size {
                width: cx,
                height: cy,
            },
            pixels: Size {
                width: 0,
                height: 0,
            },
        }))?;

        loop {
            tokio::select! {
                _ = stop_rx.recv() => {
                    break;
                }
                msg = rx.recv() => {
                    let Some(msg) = msg else {
                        break;
                    };

                    dashboard.send(msg.into())?;
                }
            }
        }

        dashboard.stop().await?;

        Ok(())
    }
}

impl Drop for Dashboard {
    fn drop(&mut self) {
        crossterm::terminal::disable_raw_mode().unwrap();
        crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen).unwrap();
    }
}

pub struct LocalWriter {
    stop: UnboundedSender<()>,
}

#[async_trait::async_trait]
impl Writer for LocalWriter {
    fn async_writer(&self) -> impl tokio::io::AsyncWrite + Send + Unpin + 'static {
        tokio::io::stdout()
    }

    fn blocking_writer(&self) -> impl std::io::Write + Send {
        std::io::stdout()
    }

    async fn shutdown(&self, _msg: String) -> Result<()> {
        self.stop.send(())?;

        Ok(())
    }
}
