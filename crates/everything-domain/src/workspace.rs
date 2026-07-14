use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFile {
    pub relative_path: PathBuf,
    pub absolute_path: PathBuf,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub root_path: PathBuf,
    pub files: Vec<ProjectFile>,
    pub stats: WorkspaceSnapshotStats,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceSnapshotStats {
    pub scanned_files: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub bytes_read: u64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum ModuleKind {
    Core,
    Adapter,
    EntryPoint,
    Tests,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleDescriptor {
    pub name: String,
    pub kind: ModuleKind,
    pub responsibility: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDescriptor {
    pub root_path: PathBuf,
    pub file_count: usize,
    pub graph_node_count: usize,
    pub graph_edge_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterSummary {
    pub filesystem: String,
    pub model: String,
    pub command: String,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DoctorCheckStatus {
    #[default]
    Healthy,
    Degraded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeDoctorCheck {
    pub check_id: String,
    pub label: String,
    pub status: DoctorCheckStatus,
    pub detail: String,
    #[serde(default)]
    pub remediation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeDoctorReport {
    pub project: ProjectDescriptor,
    pub adapters: AdapterSummary,
    pub model_health: super::ModelHealth,
    pub snapshot_stats: WorkspaceSnapshotStats,
    pub bootstrap_metrics: BootstrapMetrics,
    #[serde(default)]
    pub overall_status: DoctorCheckStatus,
    #[serde(default)]
    pub checks: Vec<RuntimeDoctorCheck>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BootstrapMetrics {
    pub snapshot_millis: u128,
    pub graph_millis: u128,
    pub catalog_millis: u128,
}
