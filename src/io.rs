pub mod backend;

use std::borrow::Borrow;

use derivative::Derivative;
use eyre::{eyre, Result};
use russh::{server::Handle, ChannelId, CryptoVec, Disconnect};
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_util::bytes::Bytes;
use tracing::error;

use crate::events::Event;

pub struct Writer {
    id: ChannelId,
    handle: Handle,
    buf: CryptoVec,
}

impl Writer {
    pub fn new(id: ChannelId, handle: Handle) -> Self {
        Self {
            id,
            handle,
            buf: CryptoVec::new(),
        }
    }
}

impl std::io::Write for Writer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buf.extend(buf);

        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let buf = self.buf.clone();
        self.buf.clear();

        futures::executor::block_on(async move { self.handle.borrow().data(self.id, buf).await })
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    eyre!("error writing to channel: {:?}", e),
                )
            })
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct Channel {
    id: ChannelId,
    #[derivative(Debug = "ignore")]
    handle: Handle,
    task: Option<JoinHandle<Result<()>>>,
    tx: Option<mpsc::UnboundedSender<Event>>,
}

impl Channel {
    pub fn new(id: ChannelId, handle: Handle) -> Self {
        Self {
            id,
            handle,
            task: None,
            tx: None,
        }
    }

    pub fn writer(&self) -> Writer {
        Writer::new(self.id, self.handle.clone())
    }

    pub fn start<H>(&mut self, handler: H) -> Result<()>
    where
        H: Handler + Send + 'static,
    {
        if self.task.is_some() {
            return Err(eyre!("channel is already started"));
        }

        let (tx, rx) = mpsc::unbounded_channel::<Event>();
        self.tx = Some(tx.clone());

        let handle = self.handle.clone();
        let writer = self.writer();

        // TODO: rendering ticks should be part of the dashboard itself.
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(tokio::time::Duration::from_millis(100));

            loop {
                tokio::select! {
                    _ = tx.closed() => break,
                    _ = tick.tick() => tx.send(Event::Render).unwrap_or_else(|e| error!("error sending render: {:?}", e)),
                }
            }
        });

        self.task = Some(tokio::spawn(async move {
            let reason = match handler.start(rx, writer).await {
                Ok(()) => "closed".into(),
                Err(e) => {
                    error!("handler exited with error: {:?}", e);

                    "unexpected error".into()
                }
            };

            handle
                .disconnect(Disconnect::ByApplication, reason, String::new())
                .await?;

            Ok(())
        }));

        Ok(())
    }

    pub fn send(&self, ev: Event) -> Result<()> {
        if let Some(tx) = self.tx.as_ref() {
            tx.send(ev)?;
        } else {
            return Err(eyre!("channel is not started"));
        }

        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        self.send(Event::Shutdown)?;

        self.tx = None;

        // TODO: have some kind of timeout.
        if let Some(task) = self.task.take() {
            return task.await?;
        }

        Ok(())
    }
}

#[async_trait::async_trait]
pub trait Handler {
    async fn start(&self, rx: mpsc::UnboundedReceiver<Event>, tx: Writer) -> Result<()>;
}
