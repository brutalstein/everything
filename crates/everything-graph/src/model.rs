use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum GraphNodeKind {
    File,
    Package,
    Module,
    Type,
    Function,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum GraphEdgeKind {
    Contains,
    Defines,
    References,
    Calls,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub kind: GraphNodeKind,
    pub source_path: PathBuf,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub kind: GraphEdgeKind,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    #[serde(skip, default)]
    index: OnceLock<GraphIndex>,
}

impl WorkspaceGraph {
    pub fn new(nodes: Vec<GraphNode>, edges: Vec<GraphEdge>) -> Self {
        Self {
            nodes,
            edges,
            index: OnceLock::new(),
        }
    }

    pub(crate) fn index(&self) -> &GraphIndex {
        self.index
            .get_or_init(|| GraphIndex::build(&self.nodes, &self.edges))
    }

    pub fn nodes_of_kind(&self, kind: GraphNodeKind) -> impl Iterator<Item = &GraphNode> {
        self.nodes.iter().filter(move |node| node.kind == kind)
    }

    pub fn neighbors<'a>(
        &'a self,
        node_id: &'a str,
        kind: Option<GraphEdgeKind>,
    ) -> impl Iterator<Item = &'a GraphNode> + 'a {
        self.index()
            .outgoing
            .get(node_id)
            .into_iter()
            .flatten()
            .filter(move |(edge_kind, _)| kind.is_none_or(|expected| *edge_kind == expected))
            .map(|(_, target)| &self.nodes[*target])
    }

    pub fn summarize(&self) -> String {
        let mut counts = BTreeMap::new();
        for node in &self.nodes {
            *counts.entry(node.kind).or_insert(0usize) += 1;
        }

        let mut lines = vec![
            format!("Nodes: {}", self.nodes.len()),
            format!("Edges: {}", self.edges.len()),
        ];

        for (kind, count) in counts {
            lines.push(format!("- {kind:?}: {count}"));
        }

        lines.join("\n")
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct GraphIndex {
    pub(crate) node_positions: HashMap<String, usize>,
    pub(crate) normalized_nodes: Vec<NormalizedNode>,
    pub(crate) outgoing: HashMap<String, Vec<(GraphEdgeKind, usize)>>,
    pub(crate) impact_adjacency: Vec<Vec<usize>>,
}

#[derive(Debug, Clone)]
pub(crate) struct NormalizedNode {
    pub(crate) label: String,
    pub(crate) id: String,
    pub(crate) source: String,
}

impl GraphIndex {
    fn build(nodes: &[GraphNode], edges: &[GraphEdge]) -> Self {
        let node_positions = nodes
            .iter()
            .enumerate()
            .map(|(position, node)| (node.id.clone(), position))
            .collect::<HashMap<_, _>>();
        let normalized_nodes = nodes
            .iter()
            .map(|node| NormalizedNode {
                label: node.label.to_ascii_lowercase(),
                id: node.id.to_ascii_lowercase(),
                source: node.source_path.display().to_string().to_ascii_lowercase(),
            })
            .collect();
        let mut outgoing = HashMap::<String, Vec<(GraphEdgeKind, usize)>>::new();
        let mut impact_adjacency = vec![Vec::new(); nodes.len()];

        for edge in edges {
            let (Some(&from), Some(&to)) =
                (node_positions.get(&edge.from), node_positions.get(&edge.to))
            else {
                continue;
            };

            outgoing
                .entry(edge.from.clone())
                .or_default()
                .push((edge.kind, to));
            impact_adjacency[from].push(to);
            if matches!(edge.kind, GraphEdgeKind::References | GraphEdgeKind::Calls) {
                impact_adjacency[to].push(from);
            }
        }

        for targets in outgoing.values_mut() {
            targets.sort_unstable_by_key(|(kind, position)| (*kind, *position));
            targets.dedup();
        }
        for neighbors in &mut impact_adjacency {
            neighbors.sort_unstable();
            neighbors.dedup();
        }

        Self {
            node_positions,
            normalized_nodes,
            outgoing,
            impact_adjacency,
        }
    }
}
