use crate::{GraphEdge, GraphEdgeKind, GraphNode, GraphNodeKind, WorkspaceGraph};
use everything_domain::{ProjectFile, WorkspaceSnapshot};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub struct WorkspaceGraphBuilder {
    package_name_regex: Regex,
    path_dependency_regex: Regex,
    module_regex: Regex,
    type_regex: Regex,
    function_regex: Regex,
    invocation_regex: Regex,
    identifier_regex: Regex,
}

impl Default for WorkspaceGraphBuilder {
    fn default() -> Self {
        Self {
            package_name_regex: Regex::new(r#"(?m)^\s*name\s*=\s*"([^"]+)""#)
                .expect("valid package regex"),
            path_dependency_regex: Regex::new(
                r#"(?m)^\s*([A-Za-z0-9_-]+)\s*=\s*\{[^}]*path\s*=\s*"([^"]+)""#,
            )
            .expect("valid dependency regex"),
            module_regex: Regex::new(r#"(?m)^\s*(?:pub\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)"#)
                .expect("valid module regex"),
            type_regex: Regex::new(r#"\b(struct|enum|trait)\s+([A-Za-z_][A-Za-z0-9_]*)"#)
                .expect("valid type regex"),
            function_regex: Regex::new(r#"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\("#)
                .expect("valid function regex"),
            invocation_regex: Regex::new(r#"\b([A-Za-z_][A-Za-z0-9_]*)\s*\("#)
                .expect("valid invocation regex"),
            identifier_regex: Regex::new(r#"\b([A-Za-z_][A-Za-z0-9_]*)\b"#)
                .expect("valid identifier regex"),
        }
    }
}

impl WorkspaceGraphBuilder {
    pub fn build(&self, snapshot: &WorkspaceSnapshot) -> WorkspaceGraph {
        let mut nodes = BTreeMap::<String, GraphNode>::new();
        let mut edges = BTreeSet::<GraphEdge>::new();
        let mut package_dirs = BTreeMap::<String, String>::new();
        let mut functions_by_name = BTreeMap::<String, Vec<String>>::new();
        let mut types_by_name = BTreeMap::<String, Vec<String>>::new();
        let mut function_bodies = Vec::<FunctionBody>::new();

        for file in &snapshot.files {
            let file_id = format!("file:{}", file.relative_path.display());
            nodes.insert(
                file_id.clone(),
                GraphNode {
                    id: file_id,
                    label: file.relative_path.display().to_string(),
                    kind: GraphNodeKind::File,
                    source_path: file.relative_path.clone(),
                },
            );

            if file
                .relative_path
                .file_name()
                .is_some_and(|name| name == "Cargo.toml")
                && let Some(package_name) = self.package_name(file)
            {
                let package_id = format!("package:{package_name}");
                nodes.insert(
                    package_id.clone(),
                    GraphNode {
                        id: package_id.clone(),
                        label: package_name.clone(),
                        kind: GraphNodeKind::Package,
                        source_path: file.relative_path.clone(),
                    },
                );

                package_dirs.insert(
                    normalize_dir(file.relative_path.parent()),
                    package_id.clone(),
                );
                edges.insert(GraphEdge {
                    from: package_id,
                    to: format!("file:{}", file.relative_path.display()),
                    kind: GraphEdgeKind::Contains,
                });
            }
        }

        for file in &snapshot.files {
            self.link_file_to_package(file, &package_dirs, &mut edges);

            if file
                .relative_path
                .file_name()
                .is_some_and(|name| name == "Cargo.toml")
            {
                self.add_package_reference_edges(file, snapshot, &mut nodes, &mut edges);
                continue;
            }

            if file.relative_path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }

            self.add_module_nodes(file, &mut nodes, &mut edges);
            self.add_type_and_function_nodes(
                file,
                &mut nodes,
                &mut edges,
                &mut functions_by_name,
                &mut types_by_name,
                &mut function_bodies,
            );
        }

        for function_body in function_bodies {
            for capture in self.invocation_regex.captures_iter(&function_body.body) {
                let function_name = capture
                    .get(1)
                    .map(|value| value.as_str())
                    .unwrap_or_default();
                if matches!(
                    function_name,
                    "if" | "for" | "while" | "loop" | "match" | "Some" | "Ok" | "Err"
                ) {
                    continue;
                }

                if let Some(targets) = functions_by_name.get(function_name)
                    && let Some(target) = targets
                        .iter()
                        .find(|candidate| *candidate != &function_body.source_id)
                {
                    edges.insert(GraphEdge {
                        from: function_body.source_id.clone(),
                        to: target.clone(),
                        kind: GraphEdgeKind::Calls,
                    });
                }
            }

            for capture in self.identifier_regex.captures_iter(&function_body.body) {
                let type_name = capture
                    .get(1)
                    .map(|value| value.as_str())
                    .unwrap_or_default();
                let Some(candidates) = types_by_name.get(type_name) else {
                    continue;
                };
                let local_prefix = format!("type:{}:", function_body.source_path.display());
                let target = candidates
                    .iter()
                    .find(|candidate| candidate.starts_with(&local_prefix))
                    .or_else(|| (candidates.len() == 1).then(|| &candidates[0]));

                if let Some(target) = target {
                    edges.insert(GraphEdge {
                        from: function_body.source_id.clone(),
                        to: target.clone(),
                        kind: GraphEdgeKind::References,
                    });
                }
            }
        }

        WorkspaceGraph::new(nodes.into_values().collect(), edges.into_iter().collect())
    }

    fn package_name(&self, file: &ProjectFile) -> Option<String> {
        self.package_name_regex
            .captures(&file.content)
            .and_then(|captures| captures.get(1))
            .map(|value| value.as_str().to_owned())
    }

    fn add_package_reference_edges(
        &self,
        file: &ProjectFile,
        snapshot: &WorkspaceSnapshot,
        nodes: &mut BTreeMap<String, GraphNode>,
        edges: &mut BTreeSet<GraphEdge>,
    ) {
        let Some(package_name) = self.package_name(file) else {
            return;
        };

        let from_id = format!("package:{package_name}");
        for capture in self.path_dependency_regex.captures_iter(&file.content) {
            let dependency_name = capture
                .get(1)
                .map(|value| value.as_str())
                .unwrap_or_default();
            let relative_dependency_path = capture
                .get(2)
                .map(|value| value.as_str())
                .unwrap_or_default();
            let cargo_dir = file
                .absolute_path
                .parent()
                .unwrap_or(snapshot.root_path.as_path());
            let manifest_path = cargo_dir.join(relative_dependency_path).join("Cargo.toml");

            let package_label = snapshot
                .files
                .iter()
                .find(|candidate| candidate.absolute_path == manifest_path)
                .and_then(|candidate| self.package_name(candidate))
                .unwrap_or_else(|| dependency_name.to_owned());

            let to_id = format!("package:{package_label}");
            nodes.entry(to_id.clone()).or_insert_with(|| GraphNode {
                id: to_id.clone(),
                label: package_label,
                kind: GraphNodeKind::Package,
                source_path: PathBuf::from(relative_dependency_path),
            });

            edges.insert(GraphEdge {
                from: from_id.clone(),
                to: to_id,
                kind: GraphEdgeKind::References,
            });
        }
    }

    fn link_file_to_package(
        &self,
        file: &ProjectFile,
        package_dirs: &BTreeMap<String, String>,
        edges: &mut BTreeSet<GraphEdge>,
    ) {
        let mut current = Some(normalize_dir(file.relative_path.parent()));
        while let Some(path) = current.clone() {
            if let Some(package_id) = package_dirs.get(&path) {
                edges.insert(GraphEdge {
                    from: package_id.clone(),
                    to: format!("file:{}", file.relative_path.display()),
                    kind: GraphEdgeKind::Contains,
                });
                break;
            }

            current = parent_dir(&path);
        }
    }

    fn add_module_nodes(
        &self,
        file: &ProjectFile,
        nodes: &mut BTreeMap<String, GraphNode>,
        edges: &mut BTreeSet<GraphEdge>,
    ) {
        for capture in self.module_regex.captures_iter(&file.content) {
            let module_name = capture
                .get(1)
                .map(|value| value.as_str())
                .unwrap_or_default();
            let module_id = format!("module:{}:{module_name}", file.relative_path.display());
            nodes.insert(
                module_id.clone(),
                GraphNode {
                    id: module_id.clone(),
                    label: module_name.to_owned(),
                    kind: GraphNodeKind::Module,
                    source_path: file.relative_path.clone(),
                },
            );

            edges.insert(GraphEdge {
                from: format!("file:{}", file.relative_path.display()),
                to: module_id,
                kind: GraphEdgeKind::Defines,
            });
        }
    }

    fn add_type_and_function_nodes(
        &self,
        file: &ProjectFile,
        nodes: &mut BTreeMap<String, GraphNode>,
        edges: &mut BTreeSet<GraphEdge>,
        functions_by_name: &mut BTreeMap<String, Vec<String>>,
        types_by_name: &mut BTreeMap<String, Vec<String>>,
        function_bodies: &mut Vec<FunctionBody>,
    ) {
        for capture in self.type_regex.captures_iter(&file.content) {
            let type_name = capture
                .get(2)
                .map(|value| value.as_str())
                .unwrap_or_default();
            let type_id = format!("type:{}:{type_name}", file.relative_path.display());
            nodes.insert(
                type_id.clone(),
                GraphNode {
                    id: type_id.clone(),
                    label: type_name.to_owned(),
                    kind: GraphNodeKind::Type,
                    source_path: file.relative_path.clone(),
                },
            );

            edges.insert(GraphEdge {
                from: format!("file:{}", file.relative_path.display()),
                to: type_id.clone(),
                kind: GraphEdgeKind::Defines,
            });
            types_by_name
                .entry(type_name.to_owned())
                .or_default()
                .push(type_id);
        }

        for capture in self.function_regex.captures_iter(&file.content) {
            let Some(name_match) = capture.get(1) else {
                continue;
            };

            let function_name = name_match.as_str();
            let function_id = format!(
                "fn:{}:{}:{}",
                file.relative_path.display(),
                function_name,
                name_match.start()
            );

            nodes.insert(
                function_id.clone(),
                GraphNode {
                    id: function_id.clone(),
                    label: function_name.to_owned(),
                    kind: GraphNodeKind::Function,
                    source_path: file.relative_path.clone(),
                },
            );

            edges.insert(GraphEdge {
                from: format!("file:{}", file.relative_path.display()),
                to: function_id.clone(),
                kind: GraphEdgeKind::Defines,
            });

            functions_by_name
                .entry(function_name.to_owned())
                .or_default()
                .push(function_id.clone());

            function_bodies.push(FunctionBody {
                source_id: function_id,
                source_path: file.relative_path.clone(),
                body: extract_block(&file.content, name_match.start()),
            });
        }
    }
}

fn normalize_dir(path: Option<&Path>) -> String {
    path.unwrap_or_else(|| Path::new(""))
        .to_string_lossy()
        .replace('\\', "/")
}

fn parent_dir(path: &str) -> Option<String> {
    let candidate = Path::new(path)
        .parent()?
        .to_string_lossy()
        .replace('\\', "/");
    if candidate == path {
        None
    } else {
        Some(candidate)
    }
}

fn extract_block(content: &str, start_index: usize) -> String {
    let Some(opening_offset) = content[start_index..].find('{') else {
        return String::new();
    };

    let opening_index = start_index + opening_offset;
    let mut depth = 0usize;
    for (relative_index, character) in content[opening_index..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let end_index = opening_index + relative_index + character.len_utf8();
                    return content[opening_index..end_index].to_owned();
                }
            }
            _ => {}
        }
    }

    content[opening_index..].to_owned()
}

struct FunctionBody {
    source_id: String,
    source_path: PathBuf,
    body: String,
}

#[cfg(test)]
mod tests {
    use super::WorkspaceGraphBuilder;
    use crate::GraphEdgeKind;
    use everything_domain::{ProjectFile, WorkspaceSnapshot, WorkspaceSnapshotStats};
    use std::path::PathBuf;

    #[test]
    fn builds_package_reference_and_call_edges() {
        let snapshot = WorkspaceSnapshot {
            root_path: PathBuf::from("/workspace"),
            files: vec![
                ProjectFile {
                    relative_path: PathBuf::from("crates/app/Cargo.toml"),
                    absolute_path: PathBuf::from("/workspace/crates/app/Cargo.toml"),
                    content: r#"
                        [package]
                        name = "app"

                        [dependencies]
                        core = { path = "../core" }
                    "#
                    .into(),
                },
                ProjectFile {
                    relative_path: PathBuf::from("crates/core/Cargo.toml"),
                    absolute_path: PathBuf::from("/workspace/crates/core/Cargo.toml"),
                    content: r#"
                        [package]
                        name = "core"
                    "#
                    .into(),
                },
                ProjectFile {
                    relative_path: PathBuf::from("crates/app/src/lib.rs"),
                    absolute_path: PathBuf::from("/workspace/crates/app/src/lib.rs"),
                    content: r#"
                        pub fn entry() {
                            let planner = Planner {};
                            consume(planner);
                            helper();
                        }

                        struct Planner {}

                        fn consume(_planner: Planner) {}
                        fn helper() {}
                    "#
                    .into(),
                },
            ],
            stats: WorkspaceSnapshotStats::default(),
        };

        let graph = WorkspaceGraphBuilder::default().build(&snapshot);
        assert!(graph.nodes.iter().any(|node| node.id == "package:app"));
        assert!(graph.nodes.iter().any(|node| node.id == "package:core"));
        assert!(graph.edges.iter().any(|edge| {
            edge.from == "package:app"
                && edge.to == "package:core"
                && edge.kind == GraphEdgeKind::References
        }));
        assert!(
            graph
                .edges
                .iter()
                .any(|edge| edge.kind == GraphEdgeKind::Calls)
        );
        assert!(graph.edges.iter().any(|edge| {
            edge.kind == GraphEdgeKind::References
                && edge.to == "type:crates/app/src/lib.rs:Planner"
        }));
    }
}
