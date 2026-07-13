use crate::ports::FileSystemAdapter;
use anyhow::Result;
use everything_domain::{ProjectFile, WorkspaceSnapshot, WorkspaceSnapshotStats};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Default)]
pub struct LocalFileSystemAdapter {
    cache_dir: Option<PathBuf>,
}

impl LocalFileSystemAdapter {
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        Self {
            cache_dir: Some(cache_dir.into()),
        }
    }
}

impl FileSystemAdapter for LocalFileSystemAdapter {
    fn name(&self) -> &str {
        "local-filesystem"
    }

    fn snapshot(&self, root: &Path) -> Result<WorkspaceSnapshot> {
        let root = root.canonicalize()?;
        let mut files = Vec::new();
        let mut stats = WorkspaceSnapshotStats::default();
        let mut manifest = self.load_manifest()?;
        let mut next_manifest = SnapshotCacheManifest::default();
        let mut manifest_changed = false;

        for entry in WalkDir::new(&root)
            .into_iter()
            .filter_entry(|entry| !is_ignored(entry.path()))
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }

            let extension = entry
                .path()
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            if !matches!(extension, "rs" | "toml" | "md") {
                continue;
            }

            let absolute_path = entry.into_path();
            let relative_path = absolute_path.strip_prefix(&root)?.to_path_buf();
            let relative_key = relative_path.to_string_lossy().replace('\\', "/");
            let metadata = std::fs::metadata(&absolute_path)?;
            let modified_unix_ms = metadata
                .modified()?
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis() as u64;
            let len = metadata.len();

            stats.scanned_files += 1;

            let content = if let Some(cached) = manifest.files.remove(&relative_key) {
                if cached.modified_unix_ms == modified_unix_ms && cached.len == len {
                    stats.cache_hits += 1;
                    cached.content
                } else {
                    stats.cache_misses += 1;
                    stats.bytes_read += len;
                    manifest_changed = true;
                    std::fs::read_to_string(&absolute_path)?
                }
            } else {
                stats.cache_misses += 1;
                stats.bytes_read += len;
                manifest_changed = true;
                std::fs::read_to_string(&absolute_path)?
            };

            next_manifest.files.insert(
                relative_key,
                SnapshotCacheEntry {
                    modified_unix_ms,
                    len,
                    content: content.clone(),
                },
            );

            files.push(ProjectFile {
                relative_path,
                absolute_path,
                content,
            });
        }

        files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

        if !manifest.files.is_empty() {
            manifest_changed = true;
        }

        if manifest_changed {
            self.store_manifest(&next_manifest)?;
        }

        Ok(WorkspaceSnapshot {
            root_path: root,
            files,
            stats,
        })
    }
}

impl LocalFileSystemAdapter {
    fn load_manifest(&self) -> Result<SnapshotCacheManifest> {
        let Some(path) = self.manifest_path() else {
            return Ok(SnapshotCacheManifest::default());
        };

        if !path.exists() {
            return Ok(SnapshotCacheManifest::default());
        }

        let raw = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw).unwrap_or_default())
    }

    fn store_manifest(&self, manifest: &SnapshotCacheManifest) -> Result<()> {
        let Some(path) = self.manifest_path() else {
            return Ok(());
        };

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let payload = serde_json::to_string_pretty(manifest)?;
        std::fs::write(path, payload)?;
        Ok(())
    }

    fn manifest_path(&self) -> Option<PathBuf> {
        self.cache_dir
            .as_ref()
            .map(|base| base.join("workspace-snapshot.json"))
    }
}

fn is_ignored(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component.as_os_str().to_string_lossy().as_ref(),
            ".git"
                | ".everything"
                | "target"
                | "node_modules"
                | "dist"
                | "build"
                | "out"
                | "graphify-out"
                | ".venv"
                | "venv"
                | "__pycache__"
        )
    })
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SnapshotCacheManifest {
    files: BTreeMap<String, SnapshotCacheEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SnapshotCacheEntry {
    modified_unix_ms: u64,
    len: u64,
    content: String,
}

#[cfg(test)]
mod tests {
    use super::is_ignored;
    use std::path::Path;

    #[test]
    fn ignores_generated_and_dependency_directories() {
        assert!(is_ignored(Path::new("node_modules/react/index.js")));
        assert!(is_ignored(Path::new(
            ".everything/cache/workspace-snapshot.json"
        )));
        assert!(is_ignored(Path::new("target/debug/everythingd.exe")));
        assert!(is_ignored(Path::new("graphify-out/graph.json")));
        assert!(!is_ignored(Path::new(
            "crates/everything-runtime/src/lib.rs"
        )));
    }
}
