use std::time::Duration;

use eyre::{eyre, Report, Result};
use futures::TryStreamExt;
use ratatui::{backend::Backend as BackendTrait, widgets::Clear, Terminal};
use replace_with::replace_with_or_abort;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
    task::JoinHandle,
};
use tokio_util::io::ReaderStream;

use crate::{
    events::{Broadcast, Event, Input, Keypress, StringError},
    io::{backend::Backend, Writer},
    widget::{apex::Apex, Raw, Widget},
};

pub struct Dashboard {
    client: kube::Client,

    task: Option<JoinHandle<Result<()>>>,
    tx: Option<UnboundedSender<Event>>,

    tick: Duration,
}

impl Dashboard {
    pub fn new(client: kube::Client) -> Self {
        Self {
            client,
            task: None,
            tx: None,

            tick: Duration::from_millis(100),
        }
    }

    #[allow(dead_code)]
    pub fn with_fps(mut self, fps: u64) -> Self {
        self.tick = Duration::from_millis(1000 / fps);

        self
    }

    pub fn start<R>(&mut self, stdin: R, stdout: impl Writer) -> Result<UnboundedSender<Event>>
    where
        R: AsyncRead + Send + 'static,
    {
        if self.task.is_some() {
            return Err(eyre!("dashboard already started"));
        }

        let (tx, rx) = mpsc::unbounded_channel();
        self.tx = Some(tx.clone());

        let reader_tx = tx.clone();
        tokio::spawn(async move {
            let stream = ReaderStream::new(stdin);
            tokio::pin!(stream);

            loop {
                tokio::select! {
                    () = reader_tx.closed() => {
                        break;
                    }
                    Ok(Some(msg)) = stream.try_next() => {
                        reader_tx.send(msg.into())?;
                    }
                }
            }

            Ok::<(), Report>(())
        });

        self.task = Some(tokio::spawn(run(
            self.client.clone(),
            self.tick,
            rx,
            stdout,
        )));

        Ok(tx)
    }

    pub fn send(&self, ev: Event) -> Result<()> {
        let Some(tx) = &self.tx else {
            return Err(eyre!("channel not started"));
        };

        tx.send(ev)?;

        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        if self.tx.is_some() {
            self.send(Event::Shutdown)?;

            self.tx = None;
        }

        if let Some(task) = self.task.take() {
            task.await?
        } else {
            Ok(())
        }
    }
}

impl std::fmt::Debug for Dashboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dashboard").finish()
    }
}

#[derive(Debug)]
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

    #[allow(clippy::match_wildcard_for_single_variants)]
    fn ui(&mut self) {
        replace_with_or_abort(self, |self_| match self_ {
            Self::Raw(_, previous) => Self::UI(previous),
            _ => self_,
        });
    }
}

async fn run(
    client: kube::Client,
    tick: Duration,
    mut rx: UnboundedReceiver<Event>,

    stdout: impl Writer,
) -> Result<()> {
    let mut interval = tokio::time::interval(tick);
    // Because we pause the render loop while rendering a raw widget, the ticks can
    // really back up. While this wouldn't necessarily be a bad thing (just some
    // extra CPU), it causes `Handle.data()` to deadlock if called too quickly.
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let (backend, window_size) = Backend::with_size(stdout.blocking());
    let mut term = Terminal::new(backend)?;

    // kube::Client ends up being cloned by ~every widget, it'd be nice to Arc<> it
    // so that there's not a bunch of copying. Unfortunately, the Api interface
    // doesn't like Arc<>.
    let mut state = Mode::UI(Box::new(Apex::new(client)));

    loop {
        // It is important that this doesn't go *too* fast. Repeatedly writing to the
        // channel causes a deadlock for some reason that I've been unable to decipher.
        let ev = tokio::select! {
            ev = rx.recv() => {
                let Some(ev) = ev else {
                    break;
                };

                ev
            }
            _ = interval.tick() => {
                Event::Render
            }
        };

        if let Event::Resize(area) = ev {
            let mut size = window_size.lock().unwrap();
            *size = area;
        }

        let result = match state {
            Mode::UI(ref mut widget) => draw_ui(widget, &mut term, &ev)?,
            Mode::Raw(ref mut raw_widget, ref mut current_widget) => {
                let raw_result =
                    draw_raw(raw_widget, &mut term, &mut rx, stdout.non_blocking()).await;

                let result = current_widget.dispatch(&Event::Finished(
                    raw_result.map_err(|e| StringError(format!("{e:?}"))),
                ))?;

                state.ui();

                result
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

    term.draw(|frame| {
        frame.render_widget(Clear, frame.size());
        frame.set_cursor(0, 0);
    })?;

    stdout.shutdown("exiting...".to_string()).await?;

    Ok(())
}

fn draw_ui<W>(
    widget: &mut Box<dyn Widget>,
    term: &mut Terminal<Backend<W>>,
    ev: &Event,
) -> Result<Broadcast>
where
    W: std::io::Write + Send,
{
    let result = match ev {
        Event::Input(Input { key, .. }) => {
            if matches!(key, Keypress::EndOfText) {
                return Ok(Broadcast::Exited);
            }

            widget.dispatch(ev)
        }
        Event::Render => Ok(Broadcast::Ignored),
        _ => widget.dispatch(ev),
    };

    term.draw(|frame| {
        if let Err(err) = widget.draw(frame, frame.size()) {
            panic!("{err}");
        }
    })?;

    result
}

async fn draw_raw(
    raw_widget: &mut Box<dyn Raw>,
    term: &mut Terminal<impl BackendTrait>,
    input: &mut UnboundedReceiver<Event>,
    output: impl AsyncWrite + Unpin + Send + 'static,
) -> Result<()> {
    term.clear()?;
    term.reset_cursor()?;

    raw_widget.start(input, Box::pin(output)).await?;

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
