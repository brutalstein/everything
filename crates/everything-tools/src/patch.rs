use crate::workspace::WorkspaceGuard;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct PatchTransaction {
    guard: WorkspaceGuard,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchPreview {
    pub relative_path: PathBuf,
    pub before_hash: String,
    pub after_hash: String,
    pub diff: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchReceipt {
    pub relative_path: PathBuf,
    pub before_hash: String,
    pub after_hash: String,
    pub diff: String,
    #[serde(skip)]
    original_content: String,
}

impl PatchTransaction {
    pub fn new(guard: WorkspaceGuard) -> Self {
        Self { guard }
    }

    pub fn preview(
        &self,
        relative_path: &Path,
        expected_content_hash: &str,
        replacement_content: &str,
    ) -> Result<PatchPreview> {
        let target = self.guard.resolve_existing_for_write(relative_path)?;
        anyhow::ensure!(target.is_file(), "patch target is not a file");
        let original_content = std::fs::read_to_string(&target)?;
        let before_hash = content_hash(original_content.as_bytes());
        anyhow::ensure!(
            before_hash == expected_content_hash,
            "base content hash mismatch: expected {expected_content_hash}, actual {before_hash}"
        );
        let after_hash = content_hash(replacement_content.as_bytes());
        let diff = simple_diff(relative_path, &original_content, replacement_content);
        Ok(PatchPreview {
            relative_path: relative_path.to_path_buf(),
            before_hash,
            after_hash,
            diff,
        })
    }

    pub fn apply(
        &self,
        relative_path: &Path,
        expected_content_hash: &str,
        replacement_content: &str,
    ) -> Result<PatchReceipt> {
        let preview = self.preview(relative_path, expected_content_hash, replacement_content)?;
        let target = self.guard.resolve_existing_for_write(relative_path)?;
        let original_content = std::fs::read_to_string(&target)?;
        atomic_write(&target, replacement_content.as_bytes())?;
        let persisted_hash = content_hash(&std::fs::read(&target)?);
        if persisted_hash != preview.after_hash {
            let _ = atomic_write(&target, original_content.as_bytes());
            anyhow::bail!("patched file failed post-write hash validation");
        }
        Ok(PatchReceipt {
            relative_path: relative_path.to_path_buf(),
            before_hash: preview.before_hash,
            after_hash: preview.after_hash,
            diff: preview.diff,
            original_content,
        })
    }

    pub fn rollback(&self, receipt: &PatchReceipt) -> Result<()> {
        let target = self.guard.resolve_existing(&receipt.relative_path)?;
        let current_hash = content_hash(&std::fs::read(&target)?);
        anyhow::ensure!(
            current_hash == receipt.after_hash,
            "rollback refused because the patched file changed after apply"
        );
        atomic_write(&target, receipt.original_content.as_bytes())?;
        let restored_hash = content_hash(&std::fs::read(&target)?);
        anyhow::ensure!(
            restored_hash == receipt.before_hash,
            "rollback hash validation failed"
        );
        Ok(())
    }
}

pub fn content_hash(content: &[u8]) -> String {
    blake3::hash(content).to_hex().to_string()
}

fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path.parent().context("write target has no parent")?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("file");
    let temporary = parent.join(format!(
        ".{file_name}.everything-{}.{}.tmp",
        std::process::id(),
        now_millis()
    ));
    std::fs::write(&temporary, content)
        .with_context(|| format!("write temporary patch {}", temporary.display()))?;
    let backup = parent.join(format!(
        ".{file_name}.everything-{}.{}.bak",
        std::process::id(),
        now_millis()
    ));
    std::fs::rename(path, &backup)
        .with_context(|| format!("stage original file {}", path.display()))?;
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::rename(&backup, path);
        let _ = std::fs::remove_file(&temporary);
        return Err(error.into());
    }
    if let Err(error) = std::fs::remove_file(&backup) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::rename(&backup, path);
        return Err(error.into());
    }
    Ok(())
}

fn simple_diff(path: &Path, before: &str, after: &str) -> String {
    if before == after {
        return format!("--- a/{0}\n+++ b/{0}\n", path.display());
    }
    let before_lines = before.lines().collect::<Vec<_>>();
    let after_lines = after.lines().collect::<Vec<_>>();
    let mut prefix = 0usize;
    while prefix < before_lines.len()
        && prefix < after_lines.len()
        && before_lines[prefix] == after_lines[prefix]
    {
        prefix += 1;
    }
    let mut before_suffix = before_lines.len();
    let mut after_suffix = after_lines.len();
    while before_suffix > prefix
        && after_suffix > prefix
        && before_lines[before_suffix - 1] == after_lines[after_suffix - 1]
    {
        before_suffix -= 1;
        after_suffix -= 1;
    }
    let mut diff = format!(
        "--- a/{0}\n+++ b/{0}\n@@ -{1},{2} +{1},{3} @@\n",
        path.display(),
        prefix + 1,
        before_suffix.saturating_sub(prefix),
        after_suffix.saturating_sub(prefix)
    );
    for line in &before_lines[prefix..before_suffix] {
        diff.push('-');
        diff.push_str(line);
        diff.push('\n');
    }
    for line in &after_lines[prefix..after_suffix] {
        diff.push('+');
        diff.push_str(line);
        diff.push('\n');
    }
    diff
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
