use crate::{
    EventSeverity, FailureClass, ModelCapabilityProfile, PermissionScope, RunEvent, ToolEffect,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::{Display, Formatter};

macro_rules! identifier {
    ($name:ident) => {
        #[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }
    };
}

identifier!(RunId);
identifier!(StageId);
identifier!(EventId);
identifier!(InvocationId);
identifier!(ArtifactId);
identifier!(CheckpointId);
identifier!(EvidenceId);
identifier!(CriterionId);
identifier!(ErrorCode);

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum RunStage {
    Preparing,
    Retrieving,
    Planning,
    AwaitingApproval,
    Executing,
    Verifying,
    Recovering,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEventEnvelope {
    pub schema_version: u32,
    pub run_id: RunId,
    pub event: RunEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub checkpoint_id: CheckpointId,
    pub run_id: RunId,
    pub stage_id: Option<StageId>,
    pub event_sequence: u64,
    pub created_at_epoch_millis: u128,
    pub safe_to_resume: bool,
    pub summary: String,
    #[serde(default)]
    pub artifact_ids: Vec<ArtifactId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceReference {
    pub evidence_id: EvidenceId,
    pub run_id: RunId,
    pub kind: String,
    pub artifact_id: Option<ArtifactId>,
    pub summary: String,
    pub provenance: String,
    pub created_at_epoch_millis: u128,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum InvocationStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvocationRecord {
    pub invocation_id: InvocationId,
    pub run_id: RunId,
    pub stage_id: Option<StageId>,
    pub tool_name: String,
    #[serde(default)]
    pub tool_version: String,
    #[serde(default)]
    pub required_permissions: Vec<PermissionScope>,
    #[serde(default)]
    pub effect: ToolEffect,
    pub status: InvocationStatus,
    pub started_at_epoch_millis: u128,
    pub finished_at_epoch_millis: Option<u128>,
    #[serde(default)]
    pub arguments: Value,
    #[serde(default)]
    pub output: Value,
    #[serde(default)]
    pub output_truncated: bool,
    pub timeout_millis: Option<u64>,
    pub replay_key: Option<String>,
    pub result_summary: Option<String>,
    pub failure_class: Option<FailureClass>,
    pub error_code: Option<ErrorCode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInvocationRecord {
    pub invocation_id: InvocationId,
    pub run_id: RunId,
    pub stage_id: Option<StageId>,
    pub provider: String,
    pub model: String,
    pub status: InvocationStatus,
    pub started_at_epoch_millis: u128,
    pub finished_at_epoch_millis: Option<u128>,
    pub prompt_hash: Option<String>,
    pub response_artifact_id: Option<ArtifactId>,
    pub fallback_used: bool,
    pub primary_error: Option<String>,
    #[serde(default)]
    pub capability_profile: Option<ModelCapabilityProfile>,
    #[serde(default)]
    pub prompt_estimated_tokens: u32,
    #[serde(default)]
    pub output_bytes: u64,
    #[serde(default)]
    pub duration_millis: u128,
    pub failure_class: Option<FailureClass>,
    pub error_code: Option<ErrorCode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub backoff_millis: u64,
    #[serde(default)]
    pub retryable_failure_classes: Vec<FailureClass>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 1,
            backoff_millis: 0,
            retryable_failure_classes: vec![FailureClass::Transient],
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionBudget {
    pub max_wall_clock_millis: Option<u64>,
    pub max_model_calls: Option<u32>,
    pub max_tool_invocations: Option<u32>,
    pub max_output_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuccessCriterion {
    pub criterion_id: CriterionId,
    pub description: String,
    pub required: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum VerificationStatus {
    Passed,
    Failed,
    Inconclusive,
    Blocked,
    Skipped,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub criterion_id: CriterionId,
    pub status: VerificationStatus,
    pub severity: EventSeverity,
    pub summary: String,
    #[serde(default)]
    pub evidence_ids: Vec<EvidenceId>,
}

#[cfg(test)]
mod tests {
    use super::{Checkpoint, CheckpointId, RunId};

    #[test]
    fn identifier_contracts_serialize_as_strings() {
        let checkpoint = Checkpoint {
            checkpoint_id: CheckpointId::from("checkpoint-1"),
            run_id: RunId::from("run-1"),
            stage_id: None,
            event_sequence: 3,
            created_at_epoch_millis: 10,
            safe_to_resume: true,
            summary: "safe boundary".to_owned(),
            artifact_ids: Vec::new(),
        };

        let payload = serde_json::to_value(checkpoint).expect("serialize checkpoint");
        assert_eq!(payload["checkpoint_id"], "checkpoint-1");
        assert_eq!(payload["run_id"], "run-1");
    }
}
