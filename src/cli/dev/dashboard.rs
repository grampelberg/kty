use std::{
    io::Read, iter::Iterator, os::fd::AsRawFd, pin::Pin, sync::atomic::Ordering, task::Context,
};

use cata::{Command, Container};
use clap::Parser;
use eyre::Result;
use mio::{unix::SourceFd, Events, Interest, Poll};
use ratatui::{backend::WindowSize, layout::Size};
use tokio::{
    io::{AsyncRead, ReadBuf},
    sync::mpsc::{unbounded_channel, UnboundedSender},
    time::Duration,
};

use crate::{dashboard, events::Event, io::Writer};

static STDIN_TOKEN: mio::Token = mio::Token(0);

#[derive(Parser, Container)]
pub struct Dashboard {
    #[arg(long, default_value = "10")]
    fps: u16,

    #[arg(long)]
    route: Vec<String>,
}

struct Stdin {
    poll: Poll,
}

// `tokio::io::stdin` runs in an actual background thread. This results in the
// program not exiting naturally, instead requiring the user to hit enter.
// Instead, use mio directly to not not require a thread at all.
//
// Note: the alternative way to do this is by having `Drop` get called on the
// reader. That doesn't happen currently because we spin up an async task to
// read and convert to events before sending to the raw dashboard. This allows
// for the actual `run()` function to block when it switches to raw mode.
impl Stdin {
    fn new() -> Result<Self> {
        let poll = Poll::new()?;

        let fd = std::io::stdin().as_raw_fd();
        let mut fd = SourceFd(&fd);

        poll.registry()
            .register(&mut fd, STDIN_TOKEN, Interest::READABLE)?;

        Ok(Self { poll })
    }
}

impl AsyncRead for Stdin {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let mut events = Events::with_capacity(1024);

        self.get_mut()
            .poll
            .poll(&mut events, Some(Duration::from_millis(1)))?;

        for event in &events {
            if event.token() != STDIN_TOKEN {
                continue;
            }

            let n = std::io::stdin().read(buf.initialize_unfilled())?;
            buf.advance(n);
        }

        if !buf.filled().is_empty() {
            return std::task::Poll::Ready(Ok(()));
        }

        let waker = cx.waker().clone();

        // Check for input every 100ms.
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;

            waker.wake();
        });

        std::task::Poll::Pending
    }
}

#[async_trait::async_trait]
impl Command for Dashboard {
    async fn run(&self) -> Result<()> {
        dashboard::FPS.store(self.fps, Ordering::Relaxed);

        ratatui::init();

        let (stop_tx, mut stop_rx) = unbounded_channel::<()>();

        let dashboard = dashboard::Dashboard::builder()
            .client(kube::Client::try_default().await?)
            .build()
            .start(Stdin::new()?, LocalWriter { stop: stop_tx })?;

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

        stop_rx.recv().await;

        Ok(())
    }
}

impl Drop for Dashboard {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

#[derive(Clone)]
pub struct LocalWriter {
    stop: UnboundedSender<()>,
}

#[async_trait::async_trait]
impl Writer for LocalWriter {
    fn blocking(&self) -> impl std::io::Write + Send {
        std::io::stdout()
    }

    fn non_blocking(&self) -> impl tokio::io::AsyncWrite + Send + Unpin + 'static {
        tokio::io::stdout()
    }

    async fn shutdown(&self, _msg: String) -> Result<()> {
        self.stop.send(())?;

        Ok(())
    }
}
