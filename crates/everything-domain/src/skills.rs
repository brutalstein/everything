use crate::{ArtifactId, PermissionScope, RunId, VerificationReport};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum SkillWorkflowKind {
    RepositoryInvestigation,
    ArchitectureSummary,
    ScopedEdit,
    DebugFailingTest,
    TestRegression,
    DocumentationUpdate,
    InstallerDiagnostics,
    RefactorPlan,
    Prompt,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum SkillSourceKind {
    Builtin,
    Workspace,
    User,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    pub skill_id: String,
    pub name: String,
    pub version: String,
    pub runtime_api: String,
    pub description: String,
    #[serde(default)]
    pub permissions: Vec<PermissionScope>,
    pub input_schema: Value,
    pub output_schema: Value,
    pub entrypoint: String,
    pub workflow: SkillWorkflowKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillCompatibility {
    pub compatible: bool,
    pub runtime_api: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDescriptor {
    pub manifest: SkillManifest,
    pub enabled: bool,
    pub compatibility: SkillCompatibility,
    #[serde(default = "default_skill_source")]
    pub source: SkillSourceKind,
    #[serde(default)]
    pub source_path: Option<PathBuf>,
    #[serde(default)]
    pub content_hash: String,
    #[serde(default)]
    pub instructions_preview: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum SkillExecutionStatus {
    Completed,
    Failed,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillExecutionRequest {
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub approval_granted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillExecutionResponse {
    pub skill_id: String,
    pub skill_version: String,
    pub status: SkillExecutionStatus,
    #[serde(default)]
    pub run_id: Option<RunId>,
    #[serde(default)]
    pub artifact_ids: Vec<ArtifactId>,
    #[serde(default)]
    pub output: Value,
    #[serde(default)]
    pub verification_report: Option<VerificationReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInstallRequest {
    pub source_path: PathBuf,
}

fn default_skill_source() -> SkillSourceKind {
    SkillSourceKind::Builtin
}
