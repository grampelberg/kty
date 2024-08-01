use std::sync::Arc;

use eyre::{eyre, Result};
use ratatui::{
    backend::{self, CrosstermBackend},
    layout::Rect,
    terminal::TerminalOptions,
    widgets::{Block, Borders, Clear, Paragraph},
    Terminal, Viewport,
};
use tokio::sync::mpsc;
use tracing::{debug, info, trace};

use crate::{
    events::{Event, Keypress},
    identity::user::User,
    io::{backend::Backend, Handler, Writer},
    ssh::Controller,
};

pub struct Dashboard {
    controller: Arc<Controller>,
    user: User,
}

fn reset_terminal(term: &mut Terminal<Backend>) -> Result<()> {
    term.show_cursor()?;

    Ok(())
}

impl Dashboard {
    pub fn new(controller: Arc<Controller>, user: User) -> Self {
        Self { controller, user }
    }
}

impl Dashboard {
    fn render(&self, term: &mut Terminal<Backend>) -> Result<()> {
        term.draw(|f| {
            let size = f.size();

            let block = Block::default()
                .title("Dashboard")
                .borders(Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Rounded);

            f.render_widget(Clear, size);
            f.render_widget(block, size);
        })?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl Handler for Dashboard {
    #[tracing::instrument(skip(self, reader, writer))]
    async fn start(
        &self,
        mut reader: mpsc::UnboundedReceiver<Event>,
        writer: Writer,
    ) -> Result<()> {
        let (backend, window_size) = Backend::with_size(writer);

        let mut term = Terminal::new(backend)?;

        while let Some(ev) = reader.recv().await {
            match ev {
                Event::Keypress(Keypress::EndOfText) | Event::Shutdown => break,
                Event::Resize(win) => {
                    let mut size = window_size.lock().unwrap();
                    *size = win;
                }
                Event::Render => self.render(&mut term)?,
                _ => {}
            }
        }

        reset_terminal(&mut term)?;

        Ok(())
    }
}

impl std::fmt::Debug for Dashboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dashboard").finish()
    }
}
