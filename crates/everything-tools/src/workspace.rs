use anyhow::{Context, Result, anyhow};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct WorkspaceGuard {
    root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMatch {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub preview: String,
}

impl WorkspaceGuard {
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        let root = root
            .as_ref()
            .canonicalize()
            .with_context(|| format!("canonicalize workspace {}", root.as_ref().display()))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn resolve_existing(&self, relative: &Path) -> Result<PathBuf> {
        validate_relative(relative)?;
        let candidate = self
            .root
            .join(relative)
            .canonicalize()
            .with_context(|| format!("resolve workspace path {}", relative.display()))?;
        self.ensure_within(&candidate)?;
        self.ensure_not_internal(&candidate)?;
        Ok(candidate)
    }

    pub fn resolve_existing_for_write(&self, relative: &Path) -> Result<PathBuf> {
        let candidate = self.resolve_existing(relative)?;
        let workspace_relative = candidate.strip_prefix(&self.root).unwrap_or(&candidate);
        anyhow::ensure!(
            workspace_relative != Path::new("everything.toml"),
            "runtime policy file 'everything.toml' cannot be modified through workspace tools"
        );
        Ok(candidate)
    }

    pub fn resolve_for_write(&self, relative: &Path) -> Result<PathBuf> {
        validate_relative(relative)?;
        let candidate = self.root.join(relative);
        let parent = candidate
            .parent()
            .ok_or_else(|| anyhow!("write target has no parent"))?
            .canonicalize()
            .with_context(|| format!("resolve parent for {}", relative.display()))?;
        self.ensure_within(&parent)?;
        self.ensure_not_internal(&parent)?;
        anyhow::ensure!(
            relative != Path::new("everything.toml"),
            "runtime policy file 'everything.toml' cannot be modified through workspace tools"
        );
        Ok(candidate)
    }

    pub fn read_text(&self, relative: &Path, max_bytes: usize) -> Result<String> {
        let path = self.resolve_existing(relative)?;
        let metadata = std::fs::metadata(&path)?;
        anyhow::ensure!(
            metadata.is_file(),
            "path is not a file: {}",
            relative.display()
        );
        read_bounded_text(&path, max_bytes)
            .with_context(|| format!("read workspace file {}", relative.display()))
    }

    pub fn list_directory(&self, relative: &Path, max_entries: usize) -> Result<Vec<PathBuf>> {
        let path = self.resolve_existing(relative)?;
        anyhow::ensure!(
            path.is_dir(),
            "path is not a directory: {}",
            relative.display()
        );
        let mut entries = std::fs::read_dir(path)?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                let relative = path.strip_prefix(&self.root).ok()?;
                if is_internal(relative) {
                    return None;
                }
                Some(relative.to_path_buf())
            })
            .collect::<Vec<_>>();
        entries.sort();
        entries.truncate(max_entries.max(1));
        Ok(entries)
    }

    pub fn search_exact(
        &self,
        query: &str,
        relative: &Path,
        max_matches: usize,
        max_file_bytes: usize,
    ) -> Result<Vec<SearchMatch>> {
        anyhow::ensure!(!query.is_empty(), "search query must not be empty");
        let base = self.resolve_existing(relative)?;
        anyhow::ensure!(base.is_dir(), "search path is not a directory");
        let mut matches = Vec::new();
        for entry in WalkBuilder::new(base)
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .build()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
        {
            let path = entry.path();
            let relative = path.strip_prefix(&self.root).unwrap_or(path);
            if is_internal(relative) {
                continue;
            }
            let content = match read_bounded_text(path, max_file_bytes) {
                Ok(content) => content,
                Err(_) => continue,
            };
            for (line_index, line) in content.lines().enumerate() {
                let mut offset = 0usize;
                while let Some(found) = line[offset..].find(query) {
                    let column = offset + found + 1;
                    matches.push(SearchMatch {
                        path: path.strip_prefix(&self.root).unwrap_or(path).to_path_buf(),
                        line: line_index + 1,
                        column,
                        preview: line.chars().take(240).collect(),
                    });
                    if matches.len() >= max_matches.max(1) {
                        return Ok(matches);
                    }
                    offset += found + query.len();
                }
            }
        }
        Ok(matches)
    }

    fn ensure_within(&self, candidate: &Path) -> Result<()> {
        anyhow::ensure!(
            candidate.starts_with(&self.root),
            "workspace path escapes root: {}",
            candidate.display()
        );
        Ok(())
    }

    fn ensure_not_internal(&self, candidate: &Path) -> Result<()> {
        let relative = candidate.strip_prefix(&self.root).unwrap_or(candidate);
        anyhow::ensure!(
            !is_internal(relative),
            "workspace path targets protected runtime metadata: {}",
            relative.display()
        );
        Ok(())
    }
}

fn read_bounded_text(path: &Path, max_bytes: usize) -> Result<String> {
    let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
    std::fs::File::open(path)?
        .take(max_bytes.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() <= max_bytes,
        "file exceeds read limit of {max_bytes} bytes"
    );
    String::from_utf8(bytes).map_err(|_| anyhow!("file is not valid UTF-8"))
}

fn is_internal(path: &Path) -> bool {
    matches!(
        path.components().next(),
        Some(Component::Normal(value))
            if value == std::ffi::OsStr::new(".git")
                || value == std::ffi::OsStr::new(".everything")
    )
}

fn validate_relative(path: &Path) -> Result<()> {
    anyhow::ensure!(!path.as_os_str().is_empty(), "path must not be empty");
    anyhow::ensure!(!path.is_absolute(), "absolute paths are not allowed");
    anyhow::ensure!(
        !path.components().any(|component| matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )),
        "path traversal is not allowed"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::WorkspaceGuard;
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn root(label: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("everything-workspace-{label}-{stamp}"));
        fs::create_dir_all(&root).expect("workspace");
        root
    }

    #[test]
    fn bounded_reads_reject_growth_beyond_limit() {
        let root = root("bounded");
        fs::write(root.join("large.txt"), b"123456789").expect("write");
        let guard = WorkspaceGuard::new(&root).expect("guard");
        assert!(guard.read_text(Path::new("large.txt"), 8).is_err());
        assert_eq!(
            guard.read_text(Path::new("large.txt"), 9).expect("read"),
            "123456789"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn runtime_metadata_is_never_exposed() {
        let root = root("metadata");
        fs::create_dir_all(root.join(".everything")).expect("metadata");
        fs::write(root.join(".everything/state"), b"secret").expect("state");
        let guard = WorkspaceGuard::new(&root).expect("guard");
        assert!(
            guard
                .read_text(Path::new(".everything/state"), 100)
                .is_err()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_cannot_escape_the_workspace() {
        use std::os::unix::fs::symlink;

        let workspace_root = root("symlink");
        let outside = root("outside");
        fs::write(outside.join("secret.txt"), b"secret").expect("outside file");
        symlink(
            outside.join("secret.txt"),
            workspace_root.join("escape.txt"),
        )
        .expect("symlink");
        let guard = WorkspaceGuard::new(&workspace_root).expect("guard");
        assert!(guard.read_text(Path::new("escape.txt"), 100).is_err());
        let _ = fs::remove_dir_all(workspace_root);
        let _ = fs::remove_dir_all(outside);
    }
}
