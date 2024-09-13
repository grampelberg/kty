use std::{io::Read, os::fd::AsRawFd};

use cata::{Command, Container};
use clap::Parser;
use eyre::Result;
use mio::{unix::SourceFd, Events, Interest, Poll};
use tokio::{
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    time::Duration,
};
use tokio_util::bytes::Bytes;

use crate::events::{Event, Keypress};

/// Throwaway meant to test why tokio blocks on stdin.
#[derive(Parser, Container)]
pub struct Stdin {}

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

                tracing::info!("read: {:?}", &buf[..n]);
                tx.send(Bytes::copy_from_slice(&buf[..n]))?;
            }
        }
    }

    Ok(())
}

async fn event_loop(mut rx: UnboundedReceiver<Bytes>) {
    let mut tick = tokio::time::interval(Duration::from_millis(100));

    loop {
        tokio::select! {
            msg = rx.recv() => {
                let Some(msg) = msg else {
                    break;
                };

                let ev: Event = msg.into();
                tracing::info!("ev: {:?}", ev);


                if matches!(ev.key(), Some(Keypress::Control('c'))) {
                    break;
                }
            }
            _ = tick.tick() => {}
        }
    }
}

#[async_trait::async_trait]
impl Command for Stdin {
    async fn run(&self) -> Result<()> {
        crossterm::terminal::enable_raw_mode()?;

        let (tx, rx) = unbounded_channel::<Bytes>();

        tokio::task::spawn_blocking(move || poll_stdin(&tx));

        tokio::spawn(event_loop(rx)).await?;

        crossterm::terminal::disable_raw_mode().unwrap();

        Ok(())
    }
}
