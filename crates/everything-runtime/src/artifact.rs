use anyhow::{Context, Result};
use everything_domain::{ArtifactDescriptor, ArtifactKind};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct ArtifactStore {
    root: PathBuf,
}

impl ArtifactStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(root.join("objects"))?;
        Ok(Self { root })
    }

    pub fn persist(
        &self,
        run_id: &str,
        kind: ArtifactKind,
        media_type: impl Into<String>,
        origin: impl Into<String>,
        content: &[u8],
    ) -> Result<ArtifactDescriptor> {
        let content_hash = blake3::hash(content).to_hex().to_string();
        let prefix = &content_hash[..2];
        let object_directory = self.root.join("objects").join(prefix);
        fs::create_dir_all(&object_directory)?;
        let object_path = object_directory.join(&content_hash);

        if !object_path.exists() {
            let temporary_path = object_directory.join(format!(
                ".{content_hash}.{}.{}.tmp",
                std::process::id(),
                now_epoch_millis()
            ));
            fs::write(&temporary_path, content).with_context(|| {
                format!("write temporary artifact {}", temporary_path.display())
            })?;
            match fs::rename(&temporary_path, &object_path) {
                Ok(()) => {}
                Err(error) if object_path.exists() => {
                    let _ = fs::remove_file(&temporary_path);
                    let _ = error;
                }
                Err(error) => return Err(error.into()),
            }
        }

        Ok(ArtifactDescriptor {
            artifact_id: format!("artifact-{run_id}-{}", &content_hash[..16]),
            run_id: run_id.to_owned(),
            kind,
            content_hash,
            media_type: media_type.into(),
            size_bytes: u64::try_from(content.len()).unwrap_or(u64::MAX),
            object_path,
            created_at_epoch_millis: now_epoch_millis(),
            origin: origin.into(),
        })
    }

    pub fn read(&self, descriptor: &ArtifactDescriptor) -> Result<Vec<u8>> {
        ensure_within(&self.root, &descriptor.object_path)?;
        let content = fs::read(&descriptor.object_path)?;
        let actual_hash = blake3::hash(&content).to_hex().to_string();
        anyhow::ensure!(
            actual_hash == descriptor.content_hash,
            "artifact '{}' failed content hash validation",
            descriptor.artifact_id
        );
        Ok(content)
    }
}

fn ensure_within(root: &Path, candidate: &Path) -> Result<()> {
    let root = root.canonicalize()?;
    let candidate = candidate.canonicalize()?;
    anyhow::ensure!(
        candidate.starts_with(&root),
        "artifact path escapes store root: {}",
        candidate.display()
    );
    Ok(())
}

fn now_epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::ArtifactStore;
    use everything_domain::ArtifactKind;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn persists_and_validates_content_addressed_artifacts() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("everything-artifacts-{suffix}"));
        let store = ArtifactStore::new(&root).expect("artifact store");
        let descriptor = store
            .persist(
                "run-1",
                ArtifactKind::Plan,
                "text/markdown",
                "test",
                b"# Plan",
            )
            .expect("persist artifact");

        assert_eq!(store.read(&descriptor).expect("read artifact"), b"# Plan");
        assert_eq!(descriptor.size_bytes, 6);
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
