use k8s_openapi::api::core::v1::Pod;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    prelude::*,
    style::{palette::tailwind, Modifier, Style},
    widgets::{Block, Borders, Row, ScrollbarState, StatefulWidget, Table, TableState, WidgetRef},
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
    scroll: ScrollbarState,
    table: TableState,
    selected: Option<String>,
}

impl PodTable {
    pub fn new(client: kube::Client) -> Self {
        Self {
            pods: Store::new(client),
            scroll: ScrollbarState::default().content_length(1),
            table: TableState::default().with_selected(0),
            selected: None,
        }
    }
}

impl WidgetRef for PodTable {
    // TODO: implement a loading screen.
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let style = TableStyle::default();

        let border = Block::default()
            .title("Pods")
            .borders(Borders::ALL)
            .style(style.border);

        let state = self.pods.state();

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
        StatefulWidget::render(&table, area, buf, &mut self.table.clone());
    }
}

impl Dispatch for PodTable {
    fn dispatch(&mut self, event: Event) {
        let Event::Keypress(key) = event else {
            return;
        };

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
