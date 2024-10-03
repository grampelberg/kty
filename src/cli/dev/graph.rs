use cata::{Command, Container};
use clap::Parser;
use eyre::Result;
use futures::stream::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::{ObjectReference, Pod};
use kube::{api::ListParams, Api};
use petgraph::graph::Graph;
use ratatui::{
    layout::Constraint,
    style::{palette::tailwind, Style},
    text::Text,
    widgets::{block::Title, Borders, Paragraph},
    Frame,
};
use tokio::io::AsyncReadExt;

use crate::{
    events::{Event, Keypress},
    exit_keys,
    resources::ResourceGraph,
    widget::graph,
};

#[derive(Parser, Container)]
pub struct Cmd {}

#[async_trait::async_trait]
impl Command for Cmd {
    async fn run(&self) -> Result<()> {
        let mut term = ratatui::init();

        let client = kube::Client::try_default().await?;

        let pods = Api::<Pod>::all(client.clone())
            .list(&ListParams::default())
            .await?;

        let graphs = futures::stream::iter(pods.items)
            .then(|pod| {
                let client = client.clone();
                async move { pod.graph(&client).await }
            })
            .try_collect::<Vec<_>>()
            .await?;

        let mut interval = tokio::time::interval(tokio::time::Duration::from_micros(100));
        let mut stdin = tokio::io::stdin();
        let mut buf = Vec::new();
        let mut i: usize = 0;
        let mut state = graph::State::default();

        loop {
            tokio::select! {
                _ = stdin.read_buf(&mut buf) => {
                    let ev = Event::from(buf.as_slice());
                    buf.clear();

                    let Some(key) = ev.key() else {
                        continue;
                    };

                    tracing::info!("key: {:?}", key);

                    match key {
                        exit_keys!() => break,
                        Keypress::CursorLeft => i = i.saturating_sub(1),
                        Keypress::CursorRight => i = i.saturating_add(1),
                        Keypress::CursorDown => state.next(),
                        Keypress::CursorUp => state.prev(),
                        _ => {},
                    }
                }
                _ = interval.tick() => {
                    let g = &graphs.get(i % graphs.len()).unwrap();

                    term.draw(|frame| draw(frame, i, g, &mut state))?;
                }
            }
        }

        Ok(())
    }
}

fn draw(frame: &mut Frame, i: usize, graph: &Graph<ObjectReference, ()>, state: &mut graph::State) {
    frame.render_widget(Paragraph::new(format!("{i}")), frame.area());

    let ng = graph.map(
        |_, o| {
            graph::Node::builder()
                .text(Text::from(o.name.clone().unwrap_or("unknown".to_string())))
                .borders(Borders::ALL)
                .titles(vec![Title::default().content(
                    o.kind
                        .clone()
                        .unwrap_or("unknown".to_string().to_lowercase()),
                )])
                .maybe_constraint(if o.kind == Some("Pod".to_string()) {
                    Some(Constraint::Fill(0))
                } else {
                    None
                })
                .selected_style(Style::default().fg(tailwind::INDIGO.c300))
                .build()
        },
        |_, ()| 0,
    );

    let widget = graph::Directed::builder().graph(ng).build();

    frame.render_stateful_widget_ref(widget, frame.area(), state);
}

impl Drop for Cmd {
    fn drop(&mut self) {
        ratatui::restore();
    }
}
