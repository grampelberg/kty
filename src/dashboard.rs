mod table;

use std::{io::Read, iter::Iterator, os::fd::AsRawFd, pin::Pin, sync::Arc};

use eyre::{eyre, Result};
use ratatui::{backend::Backend as BackendTrait, Terminal};
use replace_with::replace_with_or_abort;
use tokio::sync::{
    mpsc,
    mpsc::{UnboundedReceiver, UnboundedSender},
};
use tokio_util::bytes::Bytes;

use crate::{
    events::{Broadcast, Event, Input, Keypress},
    identity::user::User,
    io::{backend::Backend, Handler, Writer},
    ssh::Controller,
    widget::{apex::Apex, Raw, Widget},
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

enum Mode {
    UI(Box<dyn Widget>),
    Raw(Box<dyn Raw>, Box<dyn Widget>),
}

impl Mode {
    fn raw(&mut self, raw: Box<dyn Raw>) {
        replace_with_or_abort(self, |self_| match self_ {
            Self::UI(previous) | Self::Raw(_, previous) => Self::Raw(raw, previous),
        });
    }

    fn ui(&mut self) {
        replace_with_or_abort(self, |self_| match self_ {
            Self::Raw(_, previous) => Self::UI(previous),
            _ => self_,
        });
    }
}

// TODO: use this instead of `ui()` in the dashboard command.
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

        // kube::Client ends up being cloned by ~every widget, it'd be nice to Arc<> it
        // so that there's not a bunch of copying. Unfortunately, the Api interface
        // doesn't like Arc<>.
        let mut root = Apex::new(self.controller.client().clone());

        let mut state = Mode::UI(Box::new(root));

        while let Some(ev) = reader.recv().await {
            if let Event::Resize(area) = ev {
                let mut size = window_size.lock().unwrap();
                *size = area;
            }

            let result = match state {
                Mode::UI(_) => dispatch(&mut state, &mut term, &ev)?,
                Mode::Raw(ref mut raw_widget, ref mut current_widget) => {
                    let result = raw(&mut term, raw_widget, &mut reader).await;

                    current_widget.dispatch(&Event::Finished(result))?;

                    state.ui();

                    Broadcast::Ignored
                }
            };

            match result {
                Broadcast::Exited => {
                    break;
                }
                Broadcast::Raw(widget) => {
                    state.raw(widget);
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

fn dispatch(mode: &mut Mode, term: &mut Terminal<Backend>, ev: &Event) -> Result<Broadcast> {
    let Mode::UI(widget) = mode else {
        return Err(eyre!("expected UI mode"));
    };

    match ev {
        Event::Render => {}
        Event::Input(Input { key, .. }) => {
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

async fn raw(
    term: &mut Terminal<impl BackendTrait>,
    raw_widget: &mut Box<dyn Raw>,
    input: &mut UnboundedReceiver<Event>,
) -> Result<()> {
    term.clear()?;
    term.reset_cursor()?;

    // raw_widget
    //     .start(input, Pin::new(Box::new(tokio::io::stdout())))
    //     .await?;

    term.clear()?;

    Ok(())
}

trait ResetScreen {
    fn reset_cursor(&mut self) -> Result<()>;
}

impl<B> ResetScreen for Terminal<B>
where
    B: BackendTrait,
{
    fn reset_cursor(&mut self) -> Result<()> {
        self.draw(|frame| {
            frame.set_cursor(0, 0);
        })?;

        Ok(())
    }
}
