use crate::{ArtifactDescriptor, ExecutionMode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

pub const RUN_JOURNAL_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum ModelHealthStatus {
    Healthy,
    Degraded,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelHealth {
    pub status: ModelHealthStatus,
    /// True when at least one configured model path can serve requests.
    pub available: bool,
    /// The adapter currently expected to serve requests.
    pub adapter: String,
    pub detail: String,
    /// Whether the configured primary backend is healthy.
    pub primary_available: bool,
    /// Whether the configured fallback backend is healthy.
    pub fallback_available: bool,
    /// True when requests would currently be routed to the fallback backend.
    pub fallback_active: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum RunStatus {
    Queued,
    Started,
    Running,
    Paused,
    AwaitingApproval,
    Blocked,
    Cancelling,
    Cancelled,
    Recovering,
    Completed,
    Failed,
}

impl RunStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Cancelled | Self::Completed | Self::Failed)
    }

    pub fn is_recoverable(self) -> bool {
        matches!(
            self,
            Self::Queued
                | Self::Started
                | Self::Running
                | Self::Paused
                | Self::AwaitingApproval
                | Self::Blocked
                | Self::Recovering
        )
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum RecoveryDisposition {
    Recoverable,
    ManualReview,
    NotRecoverable,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum EventSeverity {
    Trace,
    Debug,
    #[default]
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum FailureClass {
    Transient,
    Configuration,
    Permission,
    Validation,
    Tool,
    Model,
    Internal,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEvent {
    #[serde(default)]
    pub sequence: u64,
    #[serde(default)]
    pub event_id: String,
    #[serde(default)]
    pub timestamp_epoch_millis: u128,
    /// Human-readable stage name kept for API compatibility.
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub stage_id: Option<String>,
    #[serde(default)]
    pub correlation_id: Option<String>,
    #[serde(default)]
    pub event_kind: String,
    #[serde(default)]
    pub severity: EventSeverity,
    #[serde(default)]
    pub summary: String,
    pub detail: String,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub provenance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunJournal {
    #[serde(default = "default_journal_schema_version")]
    pub schema_version: u32,
    pub run_id: String,
    pub objective: String,
    pub status: RunStatus,
    pub generated_by: String,
    #[serde(default)]
    pub mode: Option<ExecutionMode>,
    #[serde(default)]
    pub created_at_epoch_millis: u128,
    #[serde(default)]
    pub updated_at_epoch_millis: u128,
    #[serde(default)]
    pub events: Vec<RunEvent>,
    #[serde(default)]
    pub artifact_path: Option<PathBuf>,
    #[serde(default)]
    pub artifacts: Vec<ArtifactDescriptor>,
    #[serde(default)]
    pub failure_class: Option<FailureClass>,
    #[serde(default)]
    pub recoverable: bool,
    #[serde(default)]
    pub recovery_disposition: Option<RecoveryDisposition>,
}

fn default_journal_schema_version() -> u32 {
    1
}
