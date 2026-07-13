use crate::{ExecutionMode, ModelCapabilityProfile};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifierStrength {
    Basic,
    Standard,
    Strict,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSegmentKind {
    TrustedPolicy,
    Objective,
    StageState,
    GraphSelection,
    SourceEvidence,
    PriorDecision,
    ToolResult,
    WebEvidence,
    GitHubEvidence,
    ImpactEvidence,
    Blocker,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSegment {
    pub segment_id: String,
    pub kind: ContextSegmentKind,
    pub provenance: String,
    pub trusted: bool,
    pub priority: u8,
    pub estimated_tokens: u32,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPolicyDecision {
    pub mode: ExecutionMode,
    pub model_name: String,
    pub safe_context_tokens: u32,
    pub prompt_token_budget: u32,
    pub retrieval_depth: usize,
    pub symbol_limit: usize,
    pub excerpt_byte_budget: usize,
    pub verifier_strength: VerifierStrength,
    pub max_model_calls: u32,
    pub max_tool_invocations: u32,
    pub escalation_threshold: f32,
    #[serde(default)]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalSelection {
    pub entity_id: String,
    pub qualified_name: String,
    pub kind: String,
    pub language: String,
    pub file_path: PathBuf,
    pub start_line: usize,
    pub end_line: usize,
    pub score: f64,
    pub matched_terms: Vec<String>,
    pub reason: String,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalContextPack {
    pub graph_schema_version: u32,
    pub model_profile: ModelCapabilityProfile,
    pub policy: ContextPolicyDecision,
    pub graph_revision: u64,
    pub objective: String,
    pub mode: ExecutionMode,
    pub query_terms: Vec<String>,
    pub selections: Vec<RetrievalSelection>,
    pub related_files: Vec<PathBuf>,
    pub total_excerpt_bytes: usize,
    pub total_estimated_tokens: u32,
    #[serde(default)]
    pub segments: Vec<ContextSegment>,
    #[serde(default)]
    pub research: Option<crate::ResearchReport>,
}
