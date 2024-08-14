use std::sync::{Arc, Mutex};

use k8s_openapi::api::core::v1::Pod;
use kube::{
    api::{Api, ListParams, ObjectList},
    ResourceExt,
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Rect},
    text::Text,
    widgets::{self, Block, BorderType, Borders, Cell, Clear, Paragraph, Row, Widget, WidgetRef},
};
use tokio::task::JoinHandle;

use crate::ssh::Controller;

fn update_state(
    ctrl: Arc<Controller>,
    state: Arc<Mutex<Option<ObjectList<Pod>>>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let result = Api::<Pod>::all(ctrl.client().clone())
                .list(&ListParams::default())
                .await;

            match result {
                Err(e) => {
                    tracing::error!("error listing pods: {:?}", e);
                }
                Ok(pods) => {
                    let mut state = state.lock().unwrap();

                    *state = Some(pods);
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    })
}

pub struct Table {
    pods: Arc<Mutex<Option<ObjectList<Pod>>>>,

    task: JoinHandle<()>,
}

impl Table {
    pub fn new(ctrl: Arc<Controller>) -> Self {
        let state = Arc::new(Mutex::new(None));

        Self {
            task: update_state(ctrl, state.clone()),
            pods: state,
        }
    }
}

impl WidgetRef for Table {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);

        let state = self.pods.lock().unwrap();

        let Some(pods) = &*state else {
            Block::default()
                .title("Dashboard")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .render(area, buf);

            let pg = Paragraph::new("Loading...");

            let y = Layout::horizontal([pg.line_width() as u16]).flex(Flex::Center);
            let x =
                Layout::vertical([pg.line_count(pg.line_width() as u16) as u16]).flex(Flex::Center);
            let [area] = x.areas(area);
            let [area] = y.areas(area);

            Paragraph::new("Loading...").render(area, buf);

            return;
        };

        let header = ["Name", "Ready", "Status", "Restarts", "Age"]
            .into_iter()
            .map(Cell::from)
            .collect::<Row>()
            .height(1);

        let rows: Vec<Row> = pods
            .into_iter()
            .map(|pod| {
                Row::new(vec![
                    Cell::from(Text::raw(pod.name_any())),
                    Cell::from(Text::raw("foo")),
                    Cell::from(Text::raw("bar")),
                    Cell::from(Text::raw("baz")),
                    Cell::from(Text::raw("1234")),
                ])
            })
            .collect();

        widgets::Table::new(rows, [Constraint::Min(10)])
            .header(header)
            .render(area, buf);
    }
}

impl Drop for Table {
    fn drop(&mut self) {
        self.task.abort();
    }
}
