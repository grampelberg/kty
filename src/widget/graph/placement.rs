use std::collections::BTreeMap;

use itertools::Itertools;
use petgraph::{
    graph::{Graph, NodeIndex},
    Direction,
};
use ratatui::{
    layout::{Constraint, Flex, Layout, Position, Rect},
    widgets::Borders,
};

use super::{node, Edge, NodeTree, Placement};

trait NodeSize {
    fn constraint(&self, idx: NodeIndex) -> Constraint;
    fn height(&self, idx: NodeIndex) -> u16;
    fn width(&self, idx: NodeIndex) -> u16;
}

impl NodeSize for Graph<node::Node<'_>, u16> {
    fn constraint(&self, idx: NodeIndex) -> Constraint {
        self.raw_nodes()[idx.index()].weight.constraint()
    }

    fn height(&self, idx: NodeIndex) -> u16 {
        self.raw_nodes()[idx.index()].weight.height()
    }

    fn width(&self, idx: NodeIndex) -> u16 {
        self.raw_nodes()[idx.index()].weight.width()
    }
}

fn dfs<T>(graph: &Graph<T, u16>, nodes: &NodeTree, node: NodeIndex) -> u16 {
    if let Some(Placement { rank, .. }) = nodes.get(&node) {
        return *rank;
    }

    graph
        .neighbors_directed(node, Direction::Incoming)
        .map(|neighbor| dfs(graph, nodes, neighbor) + 1)
        .max()
        .unwrap_or(0)
}

fn compress<T>(graph: &Graph<T, u16>, nodes: &NodeTree, node: NodeIndex, rank: u16) -> u16 {
    graph
        .neighbors_directed(node, Direction::Outgoing)
        .map(|n| {
            let next = nodes.get(&n).map_or(0, |n| n.rank);
            if next + 1 == rank {
                compress(graph, nodes, n, next)
            } else {
                next - 1
            }
        })
        .min()
        .unwrap_or(rank)
}

pub fn rank<T>(graph: &Graph<T, u16>) -> NodeTree {
    let mut nodes: NodeTree = BTreeMap::new();

    for node in graph.node_indices() {
        let rank = dfs(graph, &nodes, node);

        nodes.insert(node, Placement::builder().idx(node).rank(rank).build());
    }

    for node in graph.node_indices() {
        let rank = compress(graph, &nodes, node, nodes[&node].rank);

        if let Some(n) = nodes.get_mut(&node) {
            n.rank = rank;
        }
    }

    nodes
}

pub fn node(graph: &Graph<node::Node<'_>, u16>, padding: Rect, nodes: &mut NodeTree) -> Rect {
    let mut ranks = nodes
        .values_mut()
        .sorted_by(|a, b| a.rank.cmp(&b.rank))
        .chunk_by(|n| n.rank)
        .into_iter()
        .map(|(_, nodes)| nodes.collect_vec())
        .collect_vec();

    #[allow(clippy::cast_possible_truncation)]
    let width = ranks
        .iter()
        .map(|r| {
            r.iter().fold(0, |acc, n| graph.width(n.idx) + acc)
                // Need to account for the left and right margin in addition to the min padding between nodes.
                + padding.width * (r.len() as u16 + 1)
        })
        .max()
        .unwrap_or(0);

    let mut offset_y = 0;
    for rank in &mut ranks {
        let rank_height = rank.iter().map(|n| graph.height(n.idx)).max().unwrap_or(0);

        let horizontal = Layout::horizontal(rank.iter().map(|n| graph.constraint(n.idx)))
            .flex(Flex::SpaceBetween)
            .spacing(padding.width)
            .split(Rect::new(0, offset_y, width, rank_height));

        for (node, area) in rank.iter_mut().zip(horizontal.iter()) {
            let [area] = Layout::vertical([Constraint::Length(graph.height(node.idx))])
                .flex(Flex::Center)
                .areas(*area);

            node.pos = area;
        }

        offset_y += rank_height + padding.height;
    }

    Rect::new(0, 0, width, offset_y)
}

pub fn edge(graph: &Graph<node::Node<'_>, u16>, nodes: &mut NodeTree) {
    for edge in graph.raw_edges() {
        let Some(dst) = nodes.get(&edge.target()) else {
            continue;
        };

        let (dst_x, mut dst_y) = (dst.pos.x + dst.pos.width / 2, dst.pos.y);

        if graph.raw_nodes()[edge.target().index()]
            .weight
            .borders()
            .contains(Borders::TOP)
        {
            dst_y = dst_y.saturating_add(1);
        }

        let Some(source) = nodes.get_mut(&edge.source()) else {
            continue;
        };

        let (src_x, mut src_y) = (
            source.pos.x + source.pos.width / 2,
            source.pos.y + source.pos.height,
        );

        if graph.raw_nodes()[edge.source().index()]
            .weight
            .borders()
            .contains(Borders::TOP)
        {
            src_y = src_y.saturating_sub(1);
        }

        source.edges.push(Edge {
            from: Position { x: src_x, y: src_y },
            to: Position { x: dst_x, y: dst_y },
        });
    }
}
