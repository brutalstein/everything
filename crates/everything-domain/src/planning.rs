use crate::{ContextPolicyDecision, ExecutionMode, ModelCapabilityProfile};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRequest {
    pub objective: String,
    pub mode: ExecutionMode,
    pub workspace_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningDocument {
    pub objective: String,
    pub content: String,
    #[serde(default)]
    pub graph_revision: u64,
    #[serde(default)]
    pub selected_files: Vec<PathBuf>,
    /// The backend that actually produced this document.
    pub generated_by: String,
    pub fallback_used: bool,
    pub fallback_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_capability_profile: Option<ModelCapabilityProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_policy: Option<ContextPolicyDecision>,
    #[serde(default)]
    pub estimated_context_tokens: u32,
}
