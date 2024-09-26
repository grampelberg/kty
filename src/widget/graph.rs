mod line;
mod node;
mod placement;

use std::collections::BTreeMap;

use bon::Builder;
use line::Line;
pub use node::Node;
use petgraph::graph::{Graph, NodeIndex};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    widgets::{StatefulWidgetRef, WidgetRef},
};

static PADDING: Rect = Rect {
    x: 0,
    y: 0,
    width: 2,
    height: 3,
};

type NodeTree = BTreeMap<NodeIndex, Placement>;

#[derive(Debug, Builder)]
struct Placement {
    idx: NodeIndex,
    rank: u16,
    #[builder(default)]
    pos: Rect,
    #[builder(default)]
    edges: Vec<Edge>,
}

#[derive(Debug)]
struct Edge {
    from: Position,
    to: Position,
}

#[derive(Default)]
pub struct State {
    selected: Option<NodeIndex>,
}

impl State {
    pub fn select(&mut self, idx: NodeIndex) {
        self.selected = Some(idx);
    }

    pub fn select_signed(&mut self, idx: isize) {
        let selected = self
            .selected
            .map_or(0, |n| n.index().saturating_add_signed(idx));

        self.selected = Some(NodeIndex::new(selected));
    }

    pub fn selected(&self) -> Option<NodeIndex> {
        self.selected
    }

    pub fn next(&mut self) {
        self.select_signed(1);
    }

    pub fn prev(&mut self) {
        self.select_signed(-1);
    }
}

pub struct Directed<'a> {
    graph: Graph<node::Node<'a>, u16>,
    nodes: NodeTree,
}

#[bon::bon]
impl<'a> Directed<'a> {
    #[builder]
    pub fn new(graph: Graph<node::Node<'a>, u16>) -> Self {
        let mut nodes = placement::rank(&graph);
        placement::node(&graph, PADDING, &mut nodes);
        placement::edge(&graph, &mut nodes);

        Self { graph, nodes }
    }
}

impl Directed<'_> {
    fn node(&self, area: Rect, buffer: &mut Buffer, state: &mut State, node: &Placement) {
        let widget = &self.graph.raw_nodes()[node.idx.index()].weight;

        let mut subview = node.pos;
        subview.x += area.x;
        subview.y += area.y;

        if subview.x > area.width || subview.y > area.height {
            return;
        }

        let mut selected = state.selected() == Some(node.idx);

        widget.render_ref(subview, buffer, &mut selected);
    }
}

impl StatefulWidgetRef for Directed<'_> {
    type State = State;

    fn render_ref(&self, area: Rect, buffer: &mut Buffer, state: &mut Self::State) {
        if let Some(selected) = state.selected() {
            let max = self.graph.node_count();
            if selected.index() >= max {
                state.select(NodeIndex::new(max - 1));
            }
        }

        self.nodes.iter().for_each(|(_, node)| {
            self.node(area, buffer, state, node);
        });

        // The edges need to be drawn after nodes so that they can draw the connectors
        // correctly.
        self.nodes.iter().for_each(|(_, node)| {
            draw_edges(area, buffer, state, node);
        });
    }
}

fn draw_edges(area: Rect, buffer: &mut Buffer, _: &mut State, node: &Placement) {
    for edge in &node.edges {
        Line::builder()
            .from(edge.from)
            .to(edge.to)
            .build()
            .render_ref(area, buffer);
    }
}
