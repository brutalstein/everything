use crate::{ArtifactId, EvidenceId, InvocationId, RunId, VerificationStatus};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum VerificationCheckKind {
    InputSchema,
    Precondition,
    ToolResult,
    FileDiff,
    Build,
    Test,
    Lint,
    SuccessCriterion,
    EvidenceClaim,
    Recovery,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationCheck {
    pub check_id: String,
    pub kind: VerificationCheckKind,
    pub status: VerificationStatus,
    pub required: bool,
    pub summary: String,
    #[serde(default)]
    pub evidence_ids: Vec<EvidenceId>,
    #[serde(default)]
    pub artifact_ids: Vec<ArtifactId>,
    #[serde(default)]
    pub invocation_ids: Vec<InvocationId>,
    #[serde(default)]
    pub skip_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationClaim {
    pub claim_id: String,
    pub statement: String,
    pub status: VerificationStatus,
    #[serde(default)]
    pub evidence_ids: Vec<EvidenceId>,
    #[serde(default)]
    pub artifact_ids: Vec<ArtifactId>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationReport {
    pub report_id: String,
    pub run_id: RunId,
    pub status: VerificationStatus,
    pub generated_at_epoch_millis: u128,
    #[serde(default)]
    pub checks: Vec<VerificationCheck>,
    #[serde(default)]
    pub claims: Vec<VerificationClaim>,
    pub confidence: f32,
    #[serde(default)]
    pub unresolved_risks: Vec<String>,
}

impl VerificationReport {
    pub fn required_checks_passed(&self) -> bool {
        self.checks
            .iter()
            .all(|check| !check.required || matches!(check.status, VerificationStatus::Passed))
    }
}
