use std::sync::Arc;

use eyre::{eyre, Result};
use k8s_openapi::api::core::v1::Node;
use kube::ResourceExt;
use ratatui::{buffer::Buffer, layout::Rect, Frame};
use tokio::sync::oneshot;

use super::{
    loading::Loading,
    propagate, table,
    tabs::{Tab, TabbedView},
    view::{Element, View},
    yaml::Yaml,
    Widget, WIDGET_VIEWS,
};
use crate::{
    events::{Broadcast, Event, Keypress},
    resources::store::Store,
};

pub struct List {
    view: View,
    is_ready: oneshot::Receiver<()>,
}

#[bon::bon]
impl List {
    #[allow(clippy::blocks_in_conditions)]
    #[tracing::instrument(skip_all, fields(activity = "node.list"))]
    #[builder]
    pub fn new(client: kube::Client) -> Self {
        WIDGET_VIEWS.node.list.inc();

        let (nodes, is_ready) = Store::<Node>::new(client.clone());
        let table = table::Filtered::builder()
            .table(table::Table::builder().items(nodes.clone()).build())
            .constructor(Detail::from_store(client, nodes))
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
                    .widget(Self::builder().client(client.clone()).build().boxed())
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
        if let Ok(()) = self.is_ready.try_recv() {
            self.view.pop();
        }

        self.view.draw(frame, area)
    }

    fn zindex(&self) -> u16 {
        self.view.zindex()
    }
}

pub struct Detail {
    view: TabbedView,
}

#[bon::bon]
impl Detail {
    #[builder]
    #[allow(unused_variables, clippy::needless_pass_by_value)]
    pub fn new(client: kube::Client, node: Arc<Node>) -> Self {
        WIDGET_VIEWS.node.detail.inc();

        let view = TabbedView::builder()
            .tabs(vec![Yaml::tab("YAML".to_string(), node.clone())])
            .title(vec!["nodes".to_string(), node.name_any()])
            .build();

        Self { view }
    }

    pub fn from_store(client: kube::Client, store: Arc<Store<Node>>) -> table::DetailFn {
        Box::new(move |idx, filter| {
            let node = store
                .get(idx, filter)
                .ok_or_else(|| eyre!("node not found"))?;

            Ok(Detail::builder()
                .client(client.clone())
                .node(node)
                .build()
                .boxed())
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
