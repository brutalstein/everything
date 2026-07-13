use crate::{ArtifactId, EvidenceId, RunId};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MemoryId(pub String);

impl MemoryId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for MemoryId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum MemoryScope {
    Session,
    Workspace,
    Task,
    Artifact,
    Graph,
    Preference,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub memory_id: MemoryId,
    pub scope: MemoryScope,
    pub title: String,
    pub content: String,
    pub source: String,
    #[serde(default)]
    pub workspace_key: Option<String>,
    #[serde(default)]
    pub run_id: Option<RunId>,
    #[serde(default)]
    pub artifact_id: Option<ArtifactId>,
    #[serde(default)]
    pub valid_from_epoch_millis: Option<u128>,
    #[serde(default)]
    pub valid_until_epoch_millis: Option<u128>,
    pub version: u32,
    pub confidence: f32,
    #[serde(default)]
    pub evidence_ids: Vec<EvidenceId>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub superseded_by: Option<MemoryId>,
    pub editable: bool,
    pub forgettable: bool,
    pub created_at_epoch_millis: u128,
    pub updated_at_epoch_millis: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryUpsertRequest {
    #[serde(default)]
    pub memory_id: Option<MemoryId>,
    pub scope: MemoryScope,
    pub title: String,
    pub content: String,
    pub source: String,
    #[serde(default)]
    pub workspace_key: Option<String>,
    #[serde(default)]
    pub run_id: Option<RunId>,
    #[serde(default)]
    pub artifact_id: Option<ArtifactId>,
    #[serde(default)]
    pub valid_from_epoch_millis: Option<u128>,
    #[serde(default)]
    pub valid_until_epoch_millis: Option<u128>,
    #[serde(default = "default_memory_version")]
    pub version: u32,
    #[serde(default = "default_confidence")]
    pub confidence: f32,
    #[serde(default)]
    pub evidence_ids: Vec<EvidenceId>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_true")]
    pub editable: bool,
    #[serde(default = "default_true")]
    pub forgettable: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryQuery {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub scope: Option<MemoryScope>,
    #[serde(default)]
    pub workspace_key: Option<String>,
    #[serde(default = "default_memory_limit")]
    pub limit: usize,
    #[serde(default)]
    pub include_superseded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySearchResult {
    pub entry: MemoryEntry,
    pub score: f64,
}

fn default_memory_version() -> u32 {
    1
}

fn default_confidence() -> f32 {
    0.8
}

fn default_true() -> bool {
    true
}

fn default_memory_limit() -> usize {
    20
}
