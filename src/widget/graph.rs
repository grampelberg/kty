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
    widgets::{StatefulWidgetRef, Widget, WidgetRef},
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

#[allow(dead_code)]
#[derive(Default)]
pub struct State {
    selected: Option<NodeIndex>,
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
    fn node(&self, area: Rect, buffer: &mut Buffer, _: &mut State, node: &Placement) {
        let widget = &self.graph.raw_nodes()[node.idx.index()].weight;

        let mut subview = node.pos;
        subview.x += area.x;
        subview.y += area.y;

        if subview.x > area.width || subview.y > area.height {
            return;
        }

        widget.render(subview, buffer);
    }
}

impl StatefulWidgetRef for Directed<'_> {
    type State = State;

    fn render_ref(&self, area: Rect, buffer: &mut Buffer, state: &mut Self::State) {
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
