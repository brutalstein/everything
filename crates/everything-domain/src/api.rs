use crate::{ArtifactDescriptor, PlanningDocument, RunStatus};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageReferenceSummary {
    pub label: String,
    pub outbound_references: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSummaryResponse {
    pub summary: String,
    pub package_references: Vec<PackageReferenceSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanResponse {
    pub run_id: String,
    pub document: PlanningDocument,
    pub journal_path: PathBuf,
    pub artifact: Option<ArtifactDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactContentResponse {
    pub descriptor: ArtifactDescriptor,
    pub encoding: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: String,
    pub objective: String,
    pub status: RunStatus,
    pub generated_by: String,
    pub event_count: usize,
    pub last_stage: Option<String>,
    pub journal_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkRecord {
    pub benchmark_id: String,
    pub benchmark_name: String,
    pub workspace_path: PathBuf,
    pub iterations: usize,
    pub mean_millis: f64,
    pub min_millis: u128,
    pub max_millis: u128,
    pub p50_millis: u128,
    pub p95_millis: u128,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub bytes_read: u64,
    pub created_at_epoch_millis: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeMetricsSnapshot {
    pub data_dir: PathBuf,
    pub runs_recorded: usize,
    pub benchmarks_recorded: usize,
    pub latest_run_id: Option<String>,
    pub latest_benchmark_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceEvent {
    pub sequence: u64,
    pub timestamp_epoch_millis: u128,
    pub event_kind: String,
    pub run_id: Option<String>,
    pub stage: Option<String>,
    pub detail: String,
}
