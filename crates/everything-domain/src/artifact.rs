use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum ArtifactKind {
    ContextPack,
    Plan,
    Patch,
    Diff,
    Log,
    TestReport,
    VerificationReport,
    ImpactReport,
    ResearchReport,
    GitHubReport,
    WorkflowReport,
    MemoryExport,
    GeneratedDocument,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactDescriptor {
    pub artifact_id: String,
    pub run_id: String,
    pub kind: ArtifactKind,
    pub content_hash: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub object_path: PathBuf,
    pub created_at_epoch_millis: u128,
    pub origin: String,
}
