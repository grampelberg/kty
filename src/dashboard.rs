mod table;

use std::sync::Arc;

use eyre::Result;
use ratatui::{
    widgets::{Clear, WidgetRef},
    Terminal,
};
use tokio::sync::mpsc;

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
    fn render(&self, term: &mut Terminal<Backend>, root: &impl WidgetRef) -> Result<()> {
        term.draw(|frame| {
            let size = frame.size();

            frame.render_widget(Clear, size);
            frame.render_widget(root, size);
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

        let root = table::Table::new(self.controller.clone());

        while let Some(ev) = reader.recv().await {
            match ev {
                Event::Keypress(Keypress::EndOfText) | Event::Shutdown => break,
                Event::Resize(win) => {
                    let mut size = window_size.lock().unwrap();
                    *size = win;
                }
                Event::Render => self.render(&mut term, &root)?,
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
