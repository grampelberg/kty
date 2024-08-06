use std::{
    borrow::{Borrow, BorrowMut},
    sync::Arc,
};

use eyre::Result;
use k8s_openapi::api::core::v1::Pod;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    prelude::*,
    style::{palette::tailwind, Modifier, Style},
    widgets,
    widgets::{
        Block, Borders, Clear, Paragraph, Row, StatefulWidget, Table, TableState, WidgetRef,
    },
};
use tracing::info;

use crate::{
    events::{Broadcast, Event, Keypress},
    resources::{
        pod::{self, PodExt},
        store::Store,
    },
    widget::{Dispatch, Screen, TableRow},
};

struct RowStyle {
    healthy: Style,
    unhealthy: Style,
    normal: Style,
}

impl Default for RowStyle {
    fn default() -> Self {
        Self {
            healthy: Style::default().fg(tailwind::GREEN.c300),
            unhealthy: Style::default().fg(tailwind::RED.c300),
            normal: Style::default().fg(tailwind::INDIGO.c300),
        }
    }
}

struct TableStyle {
    border: Style,
    header: Style,
    selected: Style,
    row: RowStyle,
}

impl Default for TableStyle {
    fn default() -> Self {
        Self {
            border: Style::default(),
            header: Style::default().bold(),
            selected: Style::default().add_modifier(Modifier::REVERSED),
            row: RowStyle::default(),
        }
    }
}

// - Handle items being removed/added
// - Render scrollbar only if there's something that needs to be scrolled.
pub struct PodTable {
    pods: Store<Pod>,
    table: TableState,
    cmd: Option<Command>,
}

impl PodTable {
    pub fn new(client: kube::Client) -> Self {
        Self {
            pods: Store::new(client),
            table: TableState::default().with_selected(0),

            cmd: None,
        }
    }

    fn items(&self) -> Vec<Arc<Pod>> {
        let filter = self.cmd.as_ref().map(Command::content);

        if filter.is_none() {
            return self.pods.state();
        }

        self.pods
            .state()
            .into_iter()
            .filter(|pod| {
                let filter = filter.unwrap();

                if filter.is_empty() {
                    return true;
                }

                pod.matches(filter)
            })
            .collect()
    }

    fn scroll(&mut self, key: &Keypress) {
        let current = self.table.selected().unwrap_or_default();

        let next = match key {
            Keypress::CursorUp => current.saturating_sub(1),
            Keypress::CursorDown => current.saturating_add(1),
            _ => return,
        };

        let max = self.items().len().saturating_sub(1);

        self.table.select(Some(next.clamp(0, max)));
    }
}

impl Dispatch for PodTable {
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        let Event::Keypress(key) = event else {
            return Ok(Broadcast::Ignored);
        };

        if let Some(ref mut cmd) = self.cmd {
            if matches!(cmd.dispatch(event)?, Broadcast::Consumed) {
                return Ok(Broadcast::Consumed);
            }
        }

        match key {
            Keypress::Escape => self.cmd = None,
            Keypress::CursorUp | Keypress::CursorDown => self.scroll(key),
            Keypress::Printable(x) => {
                if x == "/" && self.cmd.is_none() {
                    self.cmd = Some(Command::new());
                }
            }
            _ => {
                return Ok(Broadcast::Ignored);
            }
        };

        Ok(Broadcast::Consumed)
    }
}

impl Screen for PodTable {
    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let [_, cmd_area] =
            Layout::vertical([Constraint::Fill(0), Constraint::Length(3)]).areas(area);

        let style = TableStyle::default();

        let border = Block::default()
            .title("Pods")
            .borders(Borders::ALL)
            .style(style.border);

        let state = self.items();

        if self.table.selected().unwrap_or_default() > state.len() {
            self.table.select(Some(state.len() - 1));
        }

        let rows: Vec<Row> = state
            .iter()
            .map(|pod| {
                let row = pod.row();

                match pod.status() {
                    pod::Phase::Pending | pod::Phase::Running => row.style(style.row.normal),
                    pod::Phase::Succeeded => row.style(style.row.healthy),
                    pod::Phase::Unknown(_) => row.style(style.row.unhealthy),
                }
            })
            .collect();

        let table = Table::new(rows, Pod::constraints())
            .header(Pod::header().style(style.header))
            .block(border)
            .highlight_style(style.selected);
        frame.render_stateful_widget(&table, area, &mut self.table);

        if self.cmd.is_none() {
            return;
        }

        // Command ends up being written *over the table (which writes to the whole
        // screen). The clear makes sure that table items don't show up weirdly behind a
        // transparent command buffer.
        frame.render_widget(Clear, cmd_area);

        self.cmd.as_mut().unwrap().draw(frame, cmd_area);
    }
}

struct Command {
    content: String,
    pos: u16,
}

impl Command {
    fn new() -> Self {
        Self {
            content: String::new(),
            pos: 0,
        }
    }

    fn content(&self) -> &str {
        self.content.as_str()
    }
}

impl Dispatch for Command {
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        match event {
            Event::Keypress(Keypress::Printable(x)) => {
                self.content.insert_str(self.pos as usize, x);
                self.pos = self.pos.saturating_add(1);
            }
            Event::Keypress(Keypress::Backspace) => 'outer: {
                if self.content.is_empty() || self.pos == 0 {
                    break 'outer;
                }

                self.content.remove(self.pos as usize - 1);
                self.pos = self.pos.saturating_sub(1);
            }
            Event::Keypress(Keypress::CursorLeft) => {
                self.pos = self.pos.saturating_sub(1);
            }
            #[allow(clippy::cast_possible_truncation)]
            Event::Keypress(Keypress::CursorRight) => {
                self.pos = self
                    .pos
                    .saturating_add(1)
                    .clamp(0, self.content.len() as u16);
            }
            _ => {
                return Ok(Broadcast::Ignored);
            }
        };

        Ok(Broadcast::Consumed)
    }
}

impl Screen for Command {
    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let block = Block::default().title("Command").borders(Borders::ALL);

        let cmd_pos = block.inner(area);

        let pg = Paragraph::new(self.content()).block(block);

        frame.render_widget(pg, area);

        frame.set_cursor(cmd_pos.x + self.pos, cmd_pos.y);
    }
}
