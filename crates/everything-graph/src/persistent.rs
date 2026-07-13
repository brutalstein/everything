use crate::extractor::{extract_file, project_id};
use crate::schema::{
    CODE_GRAPH_SCHEMA_VERSION, CodeEntity, CodeGraphChangeImpactReport,
    CodeGraphChangeImpactRequest, CodeGraphEntityMetrics, CodeGraphImpactReport,
    CodeGraphIndexReport, CodeGraphPackageReference, CodeGraphPath, CodeGraphSearchResult,
    CodeLanguage, CodeRelationKind, GraphDirection, PersistentGraphStats,
};
use crate::store::CodeGraphStore;
use anyhow::{Context, Result};
use ignore::WalkBuilder;
use rayon::prelude::*;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct PersistentCodeGraph {
    store: CodeGraphStore,
}

impl PersistentCodeGraph {
    pub fn new(database_path: impl Into<PathBuf>) -> Self {
        Self {
            store: CodeGraphStore::new(database_path.into()),
        }
    }

    pub fn database_path(&self) -> &Path {
        self.store.path()
    }

    pub fn index_workspace(&self, workspace: &Path) -> Result<CodeGraphIndexReport> {
        let total_started = Instant::now();
        let workspace = workspace.canonicalize()?;
        let scan_started = Instant::now();
        let known_files = self.store.known_files()?;
        let candidates = discover_files(&workspace)?;
        let current_paths = candidates
            .iter()
            .map(|candidate| candidate.relative_path.clone())
            .collect::<BTreeSet<_>>();
        let deleted_files = known_files
            .keys()
            .filter(|path| !current_paths.contains(*path))
            .cloned()
            .collect::<Vec<_>>();
        let mut unchanged_files = 0usize;
        let changed_candidates = candidates
            .into_iter()
            .filter(|candidate| {
                let unchanged = known_files
                    .get(&candidate.relative_path)
                    .is_some_and(|known| {
                        known.size == candidate.size
                            && known.modified_millis == candidate.modified_millis
                            && known.content_hash == candidate.content_hash
                    });
                unchanged_files += usize::from(unchanged);
                !unchanged
            })
            .collect::<Vec<_>>();
        let scanned_files = current_paths.len();
        let scan_millis = scan_started.elapsed().as_millis();

        let parse_started = Instant::now();
        let project_id = project_id(&workspace);
        let extracted = changed_candidates
            .par_iter()
            .map(|candidate| {
                extract_file(
                    &project_id,
                    candidate.relative_path.clone(),
                    candidate.content.clone(),
                    candidate.content_hash.clone(),
                    candidate.language,
                    candidate.modified_millis,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let parse_millis = parse_started.elapsed().as_millis();
        let parsed_entities = extracted.iter().map(|file| file.entities.len()).sum();
        let parsed_relations = extracted.iter().map(|file| file.relations.len()).sum();
        let parse_errors = extracted.iter().map(|file| file.parse_errors).sum();

        let commit_started = Instant::now();
        self.store
            .apply_index(&workspace, &project_id, &extracted, &deleted_files)?;
        let commit_millis = commit_started.elapsed().as_millis();

        Ok(CodeGraphIndexReport {
            schema_version: CODE_GRAPH_SCHEMA_VERSION,
            graph_revision: self.store.graph_revision()?,
            workspace,
            database_path: self.store.path().to_path_buf(),
            scanned_files,
            changed_files: extracted.len(),
            unchanged_files,
            deleted_files: deleted_files.len(),
            parsed_entities,
            parsed_relations,
            parse_errors,
            scan_millis,
            parse_millis,
            commit_millis,
            total_millis: total_started.elapsed().as_millis(),
        })
    }

    pub fn stats(&self) -> Result<PersistentGraphStats> {
        self.store.stats()
    }

    pub fn search(&self, term: &str, limit: usize) -> Result<Vec<CodeGraphSearchResult>> {
        self.store.search(term, limit)
    }

    pub fn package_references(&self) -> Result<Vec<CodeGraphPackageReference>> {
        self.store.package_references()
    }

    pub fn representative_entities(&self, limit: usize) -> Result<Vec<CodeEntity>> {
        self.store.representative_entities(limit)
    }

    pub fn entity_metrics(&self, entity_id: &str) -> Result<CodeGraphEntityMetrics> {
        self.store.entity_metrics(entity_id)
    }

    pub fn analyze_change(
        &self,
        request: &CodeGraphChangeImpactRequest,
    ) -> Result<CodeGraphChangeImpactReport> {
        self.store.analyze_change(request)
    }

    pub fn impact(&self, term: &str, depth: usize, limit: usize) -> Result<CodeGraphImpactReport> {
        self.store.impact(term, depth, limit)
    }

    pub fn traverse(
        &self,
        term: &str,
        direction: GraphDirection,
        relation_kinds: &[CodeRelationKind],
        depth: usize,
        limit: usize,
    ) -> Result<CodeGraphImpactReport> {
        self.store
            .traverse(term, direction, relation_kinds, depth, limit)
    }

    pub fn path(&self, from: &str, to: &str, max_depth: usize) -> Result<Option<CodeGraphPath>> {
        self.store.find_path(from, to, max_depth)
    }

    pub fn path_with_options(
        &self,
        from: &str,
        to: &str,
        max_depth: usize,
        direction: GraphDirection,
        relation_kinds: &[CodeRelationKind],
    ) -> Result<Option<CodeGraphPath>> {
        self.store
            .find_path_filtered(from, to, max_depth, direction, relation_kinds)
    }
}

struct FileCandidate {
    relative_path: PathBuf,
    language: CodeLanguage,
    size: u64,
    modified_millis: u64,
    content: String,
    content_hash: String,
}

fn discover_files(workspace: &Path) -> Result<Vec<FileCandidate>> {
    let mut files = WalkBuilder::new(workspace)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
        .filter_map(|entry| {
            let language = CodeLanguage::from_path(entry.path())?;
            let relative_path = entry.path().strip_prefix(workspace).ok()?.to_path_buf();
            (!is_generated(&relative_path)).then(|| (entry.into_path(), relative_path, language))
        })
        .map(|(absolute_path, relative_path, language)| {
            let metadata = std::fs::metadata(&absolute_path)?;
            let modified_millis = metadata
                .modified()
                .unwrap_or(SystemTime::UNIX_EPOCH)
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let content = std::fs::read_to_string(&absolute_path)
                .with_context(|| format!("failed to read {}", absolute_path.display()))?;
            let content_hash = blake3::hash(content.as_bytes()).to_hex().to_string();
            Ok(FileCandidate {
                relative_path,
                language,
                size: metadata.len(),
                modified_millis,
                content,
                content_hash,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(files)
}

fn is_generated(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component.as_os_str().to_string_lossy().as_ref(),
            ".git"
                | ".everything"
                | "target"
                | "node_modules"
                | "dist"
                | "build"
                | "graphify-out"
                | ".venv"
                | "venv"
                | "__pycache__"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::PersistentCodeGraph;
    use crate::{CodeRelationKind, GraphDirection, RelationEvidenceKind};
    use rusqlite::Connection;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn indexes_multiple_languages_incrementally_and_preserves_history() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("everything-codegraph-{suffix}"));
        std::fs::create_dir_all(root.join("src")).expect("create workspace");
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n[dependencies]\nserde='1'\n",
        )
        .expect("write manifest");
        std::fs::write(
            root.join("src/lib.rs"),
            "pub trait Engine { fn run(&self); }\npub struct FastEngine;\nimpl Engine for FastEngine { fn run(&self) { helper(); } }\nfn helper() {}\n",
        )
        .expect("write rust");
        std::fs::write(
            root.join("worker.py"),
            "class Worker:\n    def execute(self):\n        helper()\n",
        )
        .expect("write python");
        std::fs::write(
            root.join("client.ts"),
            "export interface Client { run(): void }\nexport class LocalClient implements Client { run() {} }\n",
        )
        .expect("write typescript");

        let graph = PersistentCodeGraph::new(root.join(".everything/codegraph.sqlite3"));
        let first = graph.index_workspace(&root).expect("initial index");
        assert_eq!(first.graph_revision, 1);
        assert_eq!(first.changed_files, 4);
        assert!(first.parsed_entities >= 10);
        assert_eq!(first.parse_errors, 0);
        assert!(!graph.search("FastEngine", 5).expect("search").is_empty());
        assert!(!graph.search("Worker", 5).expect("search").is_empty());

        let second = graph.index_workspace(&root).expect("incremental index");
        assert_eq!(second.graph_revision, first.graph_revision);
        assert_eq!(second.changed_files, 0);
        assert_eq!(second.unchanged_files, 4);

        let rust_path = root.join("src/lib.rs");
        let original_modified = std::fs::metadata(&rust_path)
            .expect("rust metadata")
            .modified()
            .expect("modified time");
        let original = std::fs::read_to_string(&rust_path).expect("read rust");
        let replacement = original.replacen("helper", "change", 1);
        assert_eq!(original.len(), replacement.len());
        std::fs::write(&rust_path, replacement).expect("same-size rust change");
        let rust_file = std::fs::OpenOptions::new()
            .write(true)
            .open(&rust_path)
            .expect("open rust");
        rust_file
            .set_times(std::fs::FileTimes::new().set_modified(original_modified))
            .expect("restore modified time");
        let hash_detected = graph.index_workspace(&root).expect("hash-based index");
        assert_eq!(hash_detected.changed_files, 1);
        assert!(hash_detected.graph_revision > second.graph_revision);

        let traversal = graph
            .traverse(
                "run",
                GraphDirection::Outbound,
                &[CodeRelationKind::Calls],
                2,
                20,
            )
            .expect("directed traversal");
        assert_eq!(traversal.direction, GraphDirection::Outbound);
        assert!(
            traversal
                .relations
                .iter()
                .all(|relation| relation.kind == CodeRelationKind::Calls)
        );
        assert!(traversal.relations.iter().all(|relation| {
            matches!(
                relation.evidence_kind,
                RelationEvidenceKind::Observed | RelationEvidenceKind::Inferred
            )
        }));

        std::thread::sleep(std::time::Duration::from_millis(2));
        std::fs::write(
            root.join("worker.py"),
            "class Worker:\n    def execute(self):\n        changed()\n",
        )
        .expect("change python");
        let third = graph.index_workspace(&root).expect("changed index");
        assert_eq!(third.changed_files, 1);
        let database =
            Connection::open(root.join(".everything/codegraph.sqlite3")).expect("open graph");
        let dangling_relations = database
            .query_row(
                "SELECT COUNT(*) FROM relations r \
                 LEFT JOIN entities source ON source.id=r.source_id \
                 LEFT JOIN entities target ON target.id=r.target_id \
                 WHERE source.id IS NULL OR target.id IS NULL",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("count dangling relations");
        assert_eq!(
            dangling_relations, 0,
            "incremental re-index left dangling graph edges"
        );
        let stats = graph.stats().expect("stats");
        assert!(stats.historical_entities > 0);
        assert!(stats.historical_relations > 0);

        std::fs::remove_dir_all(root).expect("cleanup workspace");
    }
}
