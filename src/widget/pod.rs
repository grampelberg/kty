use std::borrow::{Borrow, BorrowMut};

use k8s_openapi::api::core::v1::Pod;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    prelude::*,
    style::{palette::tailwind, Modifier, Style},
    widgets::{
        Block, Borders, Clear, Paragraph, Row, StatefulWidget, Table, TableState, WidgetRef,
    },
};

use crate::{
    events::{Event, Keypress},
    resources::{pod, pod::PodExt, store::Store},
    widget::{Dispatch, TableRow},
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

    fn scroll(&mut self, key: &Keypress) {
        let current = self.table.selected().unwrap_or_default();

        let next = match key {
            Keypress::CursorUp => current.saturating_sub(1),
            Keypress::CursorDown => current.saturating_add(1),
            _ => return,
        };

        let max = self.pods.state().len().saturating_sub(1);

        self.table.select(Some(next.clamp(0, max)));
    }
}

impl WidgetRef for PodTable {
    // TODO: implement a loading screen.
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let [table_area, cmd_area] =
            Layout::vertical([Constraint::Fill(0), Constraint::Length(3)]).areas(area);

        let style = TableStyle::default();

        let border = Block::default()
            .title("Pods")
            .borders(Borders::ALL)
            .style(style.border);

        let state = self.pods.state();

        let filter = self.cmd.as_ref().map(Command::content);

        let rows: Vec<Row> = state
            .iter()
            .filter(|pod| {
                if filter.is_none() || filter.unwrap().is_empty() {
                    return true;
                }

                pod.matches(filter.unwrap())
            })
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
        StatefulWidget::render(&table, area, buf, &mut self.table.clone());

        if self.cmd.is_none() {
            return;
        }

        // Command ends up being written *over the table (which writes to the whole
        // screen). The clear makes sure that table items don't show up weirdly behind a
        // transparent command buffer.
        Widget::render(Clear, cmd_area, buf);

        WidgetRef::render_ref(self.cmd.as_ref().unwrap(), cmd_area, buf);
    }
}

impl Dispatch for PodTable {
    fn dispatch(&mut self, event: &Event) {
        let Event::Keypress(key) = event else {
            return;
        };

        if let Some(ref mut cmd) = self.cmd {
            cmd.dispatch(event);
        }

        match key {
            Keypress::Escape => self.cmd = None,
            Keypress::CursorUp | Keypress::CursorDown => self.scroll(&key),
            Keypress::Printable(x) => {
                if x == "/" && self.cmd.is_none() {
                    self.cmd = Some(Command::new());
                }
            }
            _ => {}
        }
    }
}

struct Command {
    content: String,
}

impl Command {
    fn new() -> Self {
        Self {
            content: String::new(),
        }
    }

    fn content(&self) -> &str {
        self.content.as_str()
    }
}

impl WidgetRef for Command {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.content())
            .block(Block::default().title("Command").borders(Borders::ALL))
            .render(area, buf);
    }
}

impl Dispatch for Command {
    fn dispatch(&mut self, event: &Event) {
        match event {
            Event::Keypress(Keypress::Printable(x)) => {
                self.content += x;
            }
            Event::Keypress(Keypress::Backspace) => {
                self.content.pop();
            }
            _ => {}
        }
    }
}
