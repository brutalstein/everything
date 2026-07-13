use crate::{GraphNode, WorkspaceGraph};
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::VecDeque;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphQueryResult {
    pub matched_nodes: Vec<GraphNode>,
}

#[cfg(test)]
mod tests {
    use super::WorkspaceGraph;
    use crate::{GraphEdge, GraphEdgeKind, GraphNode, GraphNodeKind};
    use std::path::PathBuf;

    fn graph() -> WorkspaceGraph {
        WorkspaceGraph::new(
            vec![
                GraphNode {
                    id: "type:Planner".to_owned(),
                    label: "Planner".to_owned(),
                    kind: GraphNodeKind::Type,
                    source_path: PathBuf::from("src/planner.rs"),
                },
                GraphNode {
                    id: "fn:create_plan".to_owned(),
                    label: "create_plan".to_owned(),
                    kind: GraphNodeKind::Function,
                    source_path: PathBuf::from("src/planner.rs"),
                },
            ],
            vec![GraphEdge {
                from: "type:Planner".to_owned(),
                to: "fn:create_plan".to_owned(),
                kind: GraphEdgeKind::Calls,
            }],
        )
    }

    #[test]
    fn indexed_search_and_impact_preserve_results() {
        let graph = graph();
        assert_eq!(graph.search("planner").matched_nodes[0].id, "type:Planner");

        let impact = graph.impact("Planner", 1).expect("impact query");
        assert_eq!(impact.affected_nodes.len(), 2);
        assert_eq!(impact.affected_nodes[1].id, "fn:create_plan");
    }

    #[test]
    fn index_is_rebuilt_after_deserialization() {
        let payload = serde_json::to_string(&graph()).expect("serialize graph");
        let restored: WorkspaceGraph = serde_json::from_str(&payload).expect("deserialize graph");

        assert_eq!(restored.search("create").matched_nodes.len(), 1);
        assert_eq!(restored.neighbors("type:Planner", None).count(), 1);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphImpactReport {
    pub root: GraphNode,
    pub depth: usize,
    pub affected_nodes: Vec<GraphNode>,
}

impl WorkspaceGraph {
    pub fn search(&self, term: &str) -> GraphQueryResult {
        let term = term.to_ascii_lowercase();
        let index = self.index();
        let mut scored = self
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(position, node)| {
                let normalized = &index.normalized_nodes[position];
                let score = match () {
                    _ if normalized.label == term => 0usize,
                    _ if normalized.id == term => 1,
                    _ if normalized.label.starts_with(&term) => 2,
                    _ if normalized.label.contains(&term) => 3,
                    _ if normalized.id.contains(&term) => 4,
                    _ if normalized.source.contains(&term) => 5,
                    _ => return None,
                };

                Some((score, node.clone()))
            })
            .collect::<Vec<_>>();

        scored.sort_by_key(|(score, node)| (*score, Reverse(node.id.len()), node.id.clone()));

        GraphQueryResult {
            matched_nodes: scored.into_iter().map(|(_, node)| node).collect(),
        }
    }

    pub fn impact(&self, term: &str, max_depth: usize) -> Result<GraphImpactReport> {
        let query = self.search(term);
        let root = query
            .matched_nodes
            .first()
            .cloned()
            .ok_or_else(|| anyhow!("no graph node matched '{term}'"))?;

        let index = self.index();
        let root_position = index.node_positions[&root.id];
        let mut visited = vec![false; self.nodes.len()];
        let mut queue = VecDeque::from([(root_position, 0usize)]);
        let mut affected = Vec::new();

        while let Some((current, depth)) = queue.pop_front() {
            if visited[current] {
                continue;
            }
            visited[current] = true;

            affected.push(self.nodes[current].clone());

            if depth >= max_depth {
                continue;
            }

            for &neighbor in &index.impact_adjacency[current] {
                queue.push_back((neighbor, depth + 1));
            }
        }

        Ok(GraphImpactReport {
            root,
            depth: max_depth,
            affected_nodes: affected,
        })
    }
}
