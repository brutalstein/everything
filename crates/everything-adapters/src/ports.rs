use anyhow::Result;
use everything_domain::{DiscoveredModel, ModelCapabilityProfile, ModelHealth, WorkspaceSnapshot};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ModelPrompt {
    pub system_instruction: String,
    pub user_instruction: String,
    pub context: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ModelCompletion {
    pub content: String,
    /// The backend that actually produced the completion.
    pub model_name: String,
    pub is_fallback: bool,
    /// Populated whenever a primary failure caused fallback routing.
    pub fallback_reason: Option<String>,
    pub capability_profile: ModelCapabilityProfile,
}

#[derive(Debug, Clone)]
pub struct CommandRequest {
    pub program: String,
    pub args: Vec<String>,
    pub working_directory: std::path::PathBuf,
}

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub status_code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub trait FileSystemAdapter: Send + Sync {
    fn name(&self) -> &str;
    fn snapshot(&self, root: &Path) -> Result<WorkspaceSnapshot>;
}

pub trait ModelAdapter: Send + Sync {
    fn name(&self) -> &str;
    fn capability_profile(&self) -> ModelCapabilityProfile;
    fn discover_models(&self) -> Result<Vec<DiscoveredModel>>;
    fn complete(&self, prompt: ModelPrompt) -> Result<ModelCompletion>;
    fn health_check(&self) -> Result<ModelHealth>;
}

pub trait CommandExecutor: Send + Sync {
    fn name(&self) -> &str;
    fn execute(&self, request: CommandRequest) -> Result<CommandOutput>;
}
