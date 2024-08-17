mod table;

use std::sync::Arc;

use eyre::Result;
use ratatui::{
    widgets::{Clear, WidgetRef},
    Terminal,
};
use tokio::sync::mpsc;

use crate::{
    events::{Broadcast, Event, Keypress},
    identity::user::User,
    io::{backend::Backend, Handler, Writer},
    ssh::Controller,
    widget::{pod, Widget},
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

        let mut root = pod::List::new(self.controller.client().clone());

        while let Some(ev) = reader.recv().await {
            if let Event::Resize(area) = ev {
                let mut size = window_size.lock().unwrap();
                *size = area;
            }

            match dispatch(&mut root, &mut term, &ev)? {
                Broadcast::Exited => {
                    break;
                }
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

fn dispatch(widget: &mut pod::List, term: &mut Terminal<Backend>, ev: &Event) -> Result<Broadcast> {
    match ev {
        Event::Render => {}
        Event::Keypress(key) => {
            if matches!(key, Keypress::EndOfText) {
                return Ok(Broadcast::Exited);
            }

            return widget.dispatch(ev);
        }
        _ => return Ok(Broadcast::Ignored),
    }

    term.draw(|frame| {
        widget.draw(frame, frame.size());
    })?;

    Ok(Broadcast::Ignored)
}
