use std::{io::Read, os::fd::AsRawFd};

use cata::{Command, Container};
use clap::Parser;
use eyre::{eyre, Result};
use futures::{channel::mpsc::Sender, SinkExt, StreamExt};
use k8s_openapi::{api::core::v1::Pod, apimachinery::pkg::apis::meta::v1::Status};
use kube::api::{AttachParams, TerminalSize};
use mio::{unix::SourceFd, Events, Interest, Poll};
use tokio::{
    io::AsyncWriteExt,
    signal,
    sync::mpsc::{unbounded_channel, UnboundedSender},
    time::Duration,
};
use tokio_util::bytes::Bytes;

#[derive(Parser, Container)]
pub struct Shell {
    /// Namespace of the pod to start a shell in.
    ns: String,

    /// ID of the pod to start a shell in.
    pod: String,
}

async fn handle_terminal_size(mut channel: Sender<TerminalSize>) -> Result<()> {
    let (width, height) = crossterm::terminal::size()?;
    channel.send(TerminalSize { width, height }).await?;

    // create a stream to catch SIGWINCH signal
    let mut sig = signal::unix::signal(signal::unix::SignalKind::window_change())?;
    loop {
        if (sig.recv().await).is_none() {
            return Ok(());
        }

        let (width, height) = crossterm::terminal::size()?;
        channel.send(TerminalSize { width, height }).await?;
    }
}

#[allow(clippy::unused_async)]
async fn poll_stdin(tx: UnboundedSender<Bytes>) -> Result<()> {
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
impl Command for Shell {
    async fn run(&self) -> Result<()> {
        crossterm::terminal::enable_raw_mode()?;

        let client = kube::Client::try_default().await?;

        let pods = kube::Api::<Pod>::namespaced(client.clone(), &self.ns);

        let mut proc = pods
            .exec(
                &self.pod,
                vec!["/bin/bash"],
                &AttachParams {
                    stdin: true,
                    stdout: true,
                    stderr: false,
                    tty: true,
                    ..Default::default()
                },
            )
            .await?;

        let status = proc.take_status().ok_or(eyre!("status not available"))?;

        let (stdin_tx, mut stdin) = unbounded_channel::<Bytes>();

        // Note: this works accidentally, for whatever reason the mio loop is tight
        // enough to block rx.recv() from ever completing but allows ticks through fine.
        // The correct solution is to spawn mio in a blocking task. Note: this means
        // that the background thread can't be aborted and must instead be garbage
        // collected via dropping the receiver itself (which should happen when the
        // event loop exists anyways).
        tokio::spawn(poll_stdin(stdin_tx));

        let mut stdout = tokio::io::stdout();

        let mut output =
            tokio_util::io::ReaderStream::new(proc.stdout().ok_or(eyre!("stdout not available"))?);
        let mut input = proc.stdin().ok_or(eyre!("stdin not available"))?;

        let term_tx = proc
            .terminal_size()
            .ok_or(eyre!("terminal size not available"))?;
        let mut handle_terminal_size_handle = tokio::spawn(handle_terminal_size(term_tx));

        loop {
            tokio::select! {
                message = stdin.recv() => {
                    if let Some(message) = message {
                        input.write_all(&message).await?;
                    } else {
                        break;
                    }
                }
                message = output.next() => {
                    if let Some(message) = message {
                        stdout.write_all(&message?).await?;
                        stdout.flush().await?;

                    } else {
                        break;
                    }
                }
                result = &mut handle_terminal_size_handle => {
                    match result {
                        Ok(_) => println!("End of terminal size stream"),
                        Err(e) => println!("Error getting terminal size: {e:?}")
                    }
                }
            }
        }

        crossterm::terminal::disable_raw_mode().unwrap();

        proc.join().await?;

        let status = status.await;

        if let Some(Status {
            status: Some(result),
            ..
        }) = &status
        {
            if result != "Success" {
                return Err(eyre!("shell exited with status: {:?}", status));
            }
        }

        Ok(())
    }
}
