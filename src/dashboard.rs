use eyre::{eyre, Result};
use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    widgets::{Block, Borders, Clear, Paragraph},
    Terminal,
};
use russh::{server, ChannelStream};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite},
    task::{self, JoinHandle},
};
use tokio_util::{io::SyncIoBridge, sync::CancellationToken};
use tracing::info;

use crate::identity::user::User;

// #[derive(Debug)]
// pub struct Dashboard<R, W>
// where
//     R: AsyncRead,
//     W: std::io::Write + Clone + Send + 'static,
// {
//     reader: R,
//     writer: W,

//     cancel: CancellationToken,
//     task: Option<JoinHandle<()>>,
// }
pub struct Dashboard {
    user: User,
    stream: ChannelStream<server::Msg>,

    task: Option<JoinHandle<()>>,
}

// impl<R, W> Dashboard<R, W>
// where
//     R: AsyncRead,
//     W: std::io::Write + Clone + Send + 'static,
impl Dashboard {
    pub fn new(user: User, stream: &ChannelStream<server::Msg>) -> Self {
        Self {
            user,
            stream,
            task: None,
        }
    }

    pub fn start(&mut self, width: u16, height: u16) -> Result<()> {
        if self.task.is_some() {
            return Err(eyre!("Dashboard is already started"));
        }

        let stream = &self.stream;

        self.task = Some(tokio::spawn(async move {
            loop {
                stream.read(&mut [0; 1024]);

                info!("Dashboard tick");
            }
        }));

        // let mut term = Terminal::new(CrosstermBackend::new(writer))?;
        // let writer = SyncIoBridge::new(channel.make_writer());

        // self.task = Some(tokio::task::spawn_blocking(move || {
        //     let writer = SyncIoBridge::new(&mut self.stream);
        //     let mut term = Terminal::new(CrosstermBackend::new(writer)).unwrap();

        //     term.resize(Rect {
        //         x: 0,
        //         y: 0,
        //         width,
        //         height,
        //     })
        //     .unwrap();

        //     loop {
        //         std::thread::sleep(std::time::Duration::from_secs(1));

        //         info!("Dashboard tick");

        //         // term.draw(|f| {
        //         //     let size = f.size();
        //         //     f.render_widget(Clear, size);
        //         //     f.render_widget(Paragraph::new("Hello World"), size);
        //         // })
        //         // .unwrap();
        //     }
        // }));

        Ok(())
    }

    pub fn stop(&self) {
        // TODO: wait before forcing the abort
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

// impl<R, W> Drop for Dashboard<R, W>
// where
//     R: AsyncRead,
//     W: std::io::Write + Clone + Send + 'static,
impl Drop for Dashboard {
    #[tracing::instrument(skip(self))]
    fn drop(&mut self) {
        self.stop();
    }
}

impl std::fmt::Debug for Dashboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dashboard").finish()
    }
}
