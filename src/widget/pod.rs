pub mod shell;

use std::sync::Arc;

use eyre::{eyre, Result};
use k8s_openapi::api::core::v1::Pod;
use kube::ResourceExt;
use ratatui::{
    layout::Rect,
    prelude::*,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders},
};

use super::{
    loading::Loading, log::Log, propagate, table, tabs::TabbedView, Placement, Widget, WIDGET_VIEWS,
};
use crate::{
    events::{Broadcast, Event, Keypress},
    resources::store::Store,
    widget::{pod::shell::Shell, yaml::Yaml},
};

pub struct List {
    pods: Arc<Store<Pod>>,
    view: table::Filtered<Arc<Store<Pod>>>,
}

impl List {
    #[allow(clippy::blocks_in_conditions)]
    #[tracing::instrument(skip(client), fields(activity = "pod.list"))]
    pub fn new(client: kube::Client) -> Self {
        WIDGET_VIEWS.pod.list.inc();

        let pods = Arc::new(Store::new(client.clone()));
        let table = table::Table::builder()
            .title("Pods")
            .items(pods.clone())
            .build();

        Self {
            pods: pods.clone(),
            view: table::Filtered::builder()
                .table(table)
                .constructor(Detail::from_store(client, pods))
                .build(),
        }
    }
}

impl Widget for List {
    fn dispatch(&mut self, event: &Event, area: Rect) -> Result<Broadcast> {
        propagate!(self.view.dispatch(event, area));

        if matches!(event.key(), Some(Keypress::Escape)) {
            return Ok(Broadcast::Exited);
        }

        Ok(Broadcast::Ignored)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        if self.pods.loading() {
            frame.render_widget(&Loading, area);

            return Ok(());
        }

        self.view.draw(frame, area)
    }

    fn placement(&self) -> Placement {
        Placement {
            horizontal: Constraint::Fill(0),
            vertical: Constraint::Fill(0),
        }
    }
}

struct DetailStyle {
    breadcrumb: Style,
}

impl Default for DetailStyle {
    fn default() -> Self {
        Self {
            breadcrumb: Style::default().add_modifier(Modifier::BOLD),
        }
    }
}

struct Detail {
    pod: Arc<Pod>,

    view: TabbedView,
}

impl Detail {
    fn new(client: &kube::Client, pod: Arc<Pod>) -> Self {
        WIDGET_VIEWS.pod.detail.inc();

        let view = TabbedView::new(vec![
            Yaml::tab("Overview".to_string(), pod.clone()),
            Log::tab("Logs".to_string(), client.clone(), pod.clone()),
            Shell::tab("Shell".to_string(), client.clone(), pod.clone()).no_margin(),
        ])
        .unwrap();

        Self { pod, view }
    }

    pub fn from_store(client: kube::Client, pods: Arc<Store<Pod>>) -> table::DetailFn {
        Box::new(move |idx, filter| {
            let pod = pods
                .get(idx, filter)
                .ok_or_else(|| eyre!("pod not found"))?;

            Ok(Box::new(Detail::new(&client, pod.clone())))
        })
    }

    fn breadcrumb(&self) -> Vec<Span> {
        let style = DetailStyle::default();

        let mut crumb: Vec<Span> = Vec::new();

        if let Some(ns) = self.pod.namespace() {
            crumb.push(ns.into());
            crumb.push(Span::from(" → ").style(style.breadcrumb));
        }

        crumb.push(self.pod.name_any().into());

        crumb
    }
}

impl Widget for Detail {
    fn dispatch(&mut self, event: &Event, area: Rect) -> Result<Broadcast> {
        propagate!(self.view.dispatch(event, area));

        if matches!(event.key(), Some(Keypress::Escape)) {
            return Ok(Broadcast::Exited);
        }

        Ok(Broadcast::Ignored)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(Line::from(self.breadcrumb()));

        let inner = block.inner(area);

        frame.render_widget(block, area);

        self.view.draw(frame, inner)
    }

    fn zindex(&self) -> u16 {
        1
    }
}
