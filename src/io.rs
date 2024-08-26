pub mod backend;

use std::{
    io::Write,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use eyre::{eyre, Result};
use futures::{future::BoxFuture, FutureExt};
use lazy_static::lazy_static;
use prometheus::{opts, register_int_counter_vec, IntCounterVec};
use prometheus_static_metric::make_static_metric;
use russh::{server::Handle, ChannelId, CryptoVec, Disconnect};
use tokio::io::AsyncWrite;
use tracing::error;

make_static_metric! {
    pub struct ChannelBytesSentVec: IntCounter {
        "type" => {
            blocking,
            non_blocking,
        },
    }
}

lazy_static! {
    static ref TOTAL_BYTES_VEC: IntCounterVec = register_int_counter_vec!(
        opts!("channel_bytes_sent_total", "Total number of bytes sent",),
        &["type"],
    )
    .unwrap();
    static ref TOTAL_BYTES: ChannelBytesSentVec = ChannelBytesSentVec::from(&TOTAL_BYTES_VEC);
}

#[derive(Clone)]
pub struct Channel {
    id: ChannelId,
    handle: Arc<Handle>,
}

impl Channel {
    pub fn new(id: ChannelId, handle: Handle) -> Self {
        Self {
            id,
            handle: Arc::new(handle),
        }
    }
}

#[async_trait::async_trait]
impl Writer for Channel {
    fn blocking_writer(&self) -> impl Write {
        SshWriter::new(self.id, self.handle.clone())
    }

    fn async_writer(&self) -> impl AsyncWrite + Send + Unpin + 'static {
        SshWriter::new(self.id, self.handle.clone())
    }

    async fn shutdown(&self, msg: String) -> Result<()> {
        self.handle
            .disconnect(Disconnect::ByApplication, msg, String::new())
            .await?;

        Ok(())
    }
}

pub struct SshWriter {
    id: ChannelId,
    handle: Arc<Handle>,
    buf: CryptoVec,

    active_send: Option<BoxFuture<'static, Result<(), CryptoVec>>>,
}

impl SshWriter {
    pub fn new(id: ChannelId, handle: Arc<Handle>) -> Self {
        Self {
            id,
            handle,
            buf: CryptoVec::new(),
            active_send: None,
        }
    }
}

impl std::io::Write for SshWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        TOTAL_BYTES.blocking.inc_by(buf.len() as u64);
        self.buf.extend(buf);

        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let buf = self.buf.clone();
        self.buf.clear();

        futures::executor::block_on(async move { self.handle.data(self.id, buf).await }).map_err(
            |e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    eyre!("error writing to channel: {:?}", e),
                )
            },
        )
    }
}

impl AsyncWrite for SshWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        #[allow(clippy::single_match_else)]
        let fut = match self.active_send {
            Some(ref mut fut) => fut,
            None => {
                let id = self.id;
                let handle = self.handle.clone();

                let buf = CryptoVec::from_slice(buf);
                let fut = async move {
                    TOTAL_BYTES.non_blocking.inc_by(buf.len() as u64);

                    handle.data(id, buf).await?;

                    Ok(())
                }
                .boxed();

                self.active_send = Some(fut);

                self.active_send.as_mut().unwrap()
            }
        };

        match fut.poll_unpin(cx) {
            Poll::Ready(result) => {
                self.active_send = None;

                match result {
                    Ok(()) => Poll::Ready(Ok(buf.len())),
                    Err(e) => {
                        error!("error writing to channel: {:?}", e);

                        Poll::Ready(Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            eyre!("error writing to channel: {:?}", e),
                        )))
                    }
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[async_trait::async_trait]
pub trait Writer: Send + Sync + 'static {
    fn async_writer(&self) -> impl AsyncWrite + Send + Unpin + 'static;
    fn blocking_writer(&self) -> impl Write + Send;

    async fn shutdown(&self, _msg: String) -> Result<()> {
        Ok(())
    }
}
