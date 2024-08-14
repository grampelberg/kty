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
    widgets::{Block, Borders, Paragraph},
};

use super::{
    loading::Loading,
    log::Log,
    table::{DetailFn, Table},
    tabs::TabbedView,
    Widget,
};
use crate::{
    events::{Broadcast, Event, Keypress},
    resources::store::Store,
    widget::{pod::shell::Shell, yaml::Yaml},
};

pub struct List {
    pods: Arc<Store<Pod>>,
    table: Table,

    route: Vec<String>,
}

impl List {
    pub fn new(client: kube::Client) -> Self {
        let pods = Arc::new(Store::new(client.clone()));

        Self {
            pods: pods.clone(),
            table: Table::new("Pods".to_string(), Detail::from_store(client, pods.clone())),

            route: Vec::new(),
        }
    }
}

impl Widget for List {
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        if let Event::Goto(route) = event {
            self.route.clone_from(route);

            return Ok(Broadcast::Consumed);
        }

        if matches!(self.table.dispatch(event)?, Broadcast::Consumed) {
            return Ok(Broadcast::Consumed);
        }

        match event {
            Event::Keypress(Keypress::Escape) => Ok(Broadcast::Exited),
            _ => Ok(Broadcast::Ignored),
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        if self.pods.loading() {
            frame.render_widget(&Loading, area);

            return;
        }

        if !self.route.is_empty() {
            let route = self.route.clone();

            if let Err(e) = self.table.dispatch(&Event::Goto(route)) {
                frame.render_widget(
                    Paragraph::new(e.to_string()).block(Block::default().borders(Borders::ALL)),
                    area,
                );

                return;
            }

            self.route.clear();
        }

        self.table.render(frame, area, &self.pods);
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
        let view = TabbedView::new(vec![
            Yaml::tab("Overview".to_string(), pod.clone()),
            Log::tab("Logs".to_string(), client.clone(), pod.clone()),
            Shell::tab("Shell".to_string(), client.clone(), pod.clone()),
        ])
        .unwrap();

        Self { pod, view }
    }

    pub fn from_store(client: kube::Client, pods: Arc<Store<Pod>>) -> DetailFn {
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
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        if matches!(self.view.dispatch(event)?, Broadcast::Consumed) {
            return Ok(Broadcast::Consumed);
        }

        let Event::Keypress(key) = event else {
            return Ok(Broadcast::Ignored);
        };

        if matches!(key, Keypress::Escape) {
            return Ok(Broadcast::Exited);
        }

        Ok(Broadcast::Ignored)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(Line::from(self.breadcrumb()));

        let inner = block.inner(area);

        frame.render_widget(block, area);

        self.view.draw(frame, inner);
    }
}