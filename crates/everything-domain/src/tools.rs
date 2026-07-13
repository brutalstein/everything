use crate::{ArtifactDescriptor, ExecutionMode, InvocationId, RunId, ToolInvocationRecord};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionScope {
    WorkspaceRead,
    WorkspaceWrite,
    ProcessExecute,
    NetworkLocal,
    NetworkExternal,
    GitRead,
    GitWrite,
    SystemInstall,
}

impl PermissionScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WorkspaceRead => "workspace.read",
            Self::WorkspaceWrite => "workspace.write",
            Self::ProcessExecute => "process.execute",
            Self::NetworkLocal => "network.local",
            Self::NetworkExternal => "network.external",
            Self::GitRead => "git.read",
            Self::GitWrite => "git.write",
            Self::SystemInstall => "system.install",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDecision {
    Allow,
    Deny,
    #[default]
    RequireApproval,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolEffect {
    #[default]
    ReadOnly,
    WorkspaceMutation,
    Process,
    GitMutation,
    Network,
    SystemMutation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub tool_id: String,
    pub version: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Value,
    pub required_permissions: Vec<PermissionScope>,
    pub default_timeout_millis: u64,
    pub max_output_bytes: u64,
    pub supports_cancellation: bool,
    pub verifier_hook: Option<String>,
    pub effect: ToolEffect,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvocationRequest {
    pub run_id: RunId,
    pub tool_id: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub approval_granted: bool,
    pub timeout_millis: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvocationResponse {
    pub invocation: ToolInvocationRecord,
    #[serde(default)]
    pub output: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationCommand {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub label: String,
    pub timeout_millis: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchExecutionRequest {
    pub objective: String,
    #[serde(default)]
    pub mode: ExecutionMode,
    pub relative_path: PathBuf,
    pub expected_content_hash: String,
    pub replacement_content: String,
    #[serde(default)]
    pub verification_commands: Vec<VerificationCommand>,
    #[serde(default)]
    pub approval_granted: bool,
    #[serde(default)]
    pub allow_repeat_failure: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchExecutionResponse {
    pub run_id: RunId,
    pub status: String,
    pub patch_invocation_id: InvocationId,
    #[serde(default)]
    pub verification_invocation_ids: Vec<InvocationId>,
    #[serde(default)]
    pub artifacts: Vec<ArtifactDescriptor>,
    pub rolled_back: bool,
    pub summary: String,
    #[serde(default)]
    pub verification_report: Option<crate::VerificationReport>,
    #[serde(default)]
    pub verification_artifact_id: Option<crate::ArtifactId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditProposalRequest {
    pub objective: String,
    #[serde(default)]
    pub mode: ExecutionMode,
    #[serde(default)]
    pub skill_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditProposalResponse {
    pub run_id: RunId,
    pub status: String,
    pub summary: String,
    pub patch: PatchExecutionRequest,
    pub diff: String,
    pub artifact: ArtifactDescriptor,
    pub generated_by: String,
    #[serde(default)]
    pub skill_id: Option<String>,
    #[serde(default)]
    pub fallback_used: bool,
    #[serde(default)]
    pub fallback_reason: Option<String>,
    #[serde(default)]
    pub impact_analysis: Value,
    #[serde(default)]
    pub impact_artifact: Option<ArtifactDescriptor>,
}
