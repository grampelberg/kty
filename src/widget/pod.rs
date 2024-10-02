pub mod shell;

use std::sync::Arc;

use eyre::{eyre, Result};
use k8s_openapi::api::core::v1::Pod;
use kube::ResourceExt;
use ratatui::{layout::Rect, prelude::*};
use tokio::sync::oneshot;

use super::{
    loading::Loading,
    log::Log,
    propagate, table,
    tabs::{Tab, TabbedView},
    view::{Element, View},
    Placement, Widget, WIDGET_VIEWS,
};
use crate::{
    events::{Broadcast, Event, Keypress},
    resources::store::Store,
    widget::{pod::shell::Shell, yaml::Yaml},
};

pub struct List {
    view: View,
    is_ready: oneshot::Receiver<()>,
}

impl List {
    #[allow(clippy::blocks_in_conditions)]
    #[tracing::instrument(skip_all, fields(activity = "pod.list"))]
    pub fn new(client: kube::Client) -> Self {
        WIDGET_VIEWS.pod.list.inc();

        let (pods, is_ready) = Store::new(client.clone());
        let table = table::Filtered::builder()
            .table(table::Table::builder().items(pods.clone()).build())
            .constructor(Detail::from_store(client, pods))
            .build();

        let widgets = vec![
            table.boxed().into(),
            Element::builder()
                .widget(Loading.boxed())
                .ignore(true)
                .build(),
        ];

        Self {
            view: View::builder().widgets(widgets).build(),
            is_ready,
        }
    }

    pub fn tab(name: String, client: kube::Client, terminal: bool) -> Tab {
        Tab::builder()
            .name(name)
            .constructor(Box::new(move || {
                Element::builder()
                    .widget(Self::new(client.clone()).boxed())
                    .terminal(terminal)
                    .build()
            }))
            .build()
    }
}

impl Widget for List {
    fn dispatch(&mut self, event: &Event, buffer: &Buffer, area: Rect) -> Result<Broadcast> {
        propagate!(self.view.dispatch(event, buffer, area));

        if matches!(event.key(), Some(Keypress::Escape)) {
            return Ok(Broadcast::Exited);
        }

        Ok(Broadcast::Ignored)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        // TODO: add an error screen here if Err(TryRecvError::Closed)
        if let Ok(()) = self.is_ready.try_recv() {
            self.view.pop();
        }

        self.view.draw(frame, area)
    }

    fn placement(&self) -> Placement {
        Placement {
            horizontal: Constraint::Fill(0),
            vertical: Constraint::Fill(0),
        }
    }

    fn zindex(&self) -> u16 {
        self.view.zindex()
    }
}

struct Detail {
    view: TabbedView,
}

#[bon::bon]
impl Detail {
    #[builder]
    #[allow(clippy::needless_pass_by_value)]
    fn new(client: &kube::Client, pod: Arc<Pod>) -> Self {
        WIDGET_VIEWS.pod.detail.inc();

        let view = TabbedView::builder()
            .tabs(vec![
                Yaml::tab("Overview".to_string(), pod.clone()),
                Log::tab("Logs".to_string(), client.clone(), pod.clone()),
                Shell::tab("Shell".to_string(), client.clone(), pod.clone()),
            ])
            .title(vec![
                "pods".to_string(),
                pod.namespace().unwrap_or_default(),
                pod.name_any(),
            ])
            .build();

        Self { view }
    }

    pub fn from_store(client: kube::Client, pods: Arc<Store<Pod>>) -> table::DetailFn {
        Box::new(move |idx, filter| {
            let pod = pods
                .get(idx, filter)
                .ok_or_else(|| eyre!("pod not found"))?;

            Ok(Detail::builder().client(&client).pod(pod).build().boxed())
        })
    }
}

impl Widget for Detail {
    fn dispatch(&mut self, event: &Event, buffer: &Buffer, area: Rect) -> Result<Broadcast> {
        propagate!(self.view.dispatch(event, buffer, area));

        if matches!(event.key(), Some(Keypress::Escape)) {
            return Ok(Broadcast::Exited);
        }

        Ok(Broadcast::Ignored)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        self.view.draw(frame, area)
    }

    fn zindex(&self) -> u16 {
        1
    }
}
