use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModelBackend {
    Loopback,
    Ollama,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum ModelFallbackPolicy {
    Disabled,
    #[default]
    Reported,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSettings {
    pub backend: ModelBackend,
    pub model_name: String,
    pub binary: String,
    pub keep_alive: String,
    pub hide_thinking: bool,
    #[serde(default)]
    pub fallback_policy: ModelFallbackPolicy,
    #[serde(default = "default_model_timeout_millis")]
    pub timeout_millis: u64,
    #[serde(default = "default_model_output_bytes")]
    pub max_output_bytes: u64,
    #[serde(default)]
    pub context_window_tokens: Option<u32>,
    #[serde(default)]
    pub safe_context_tokens: Option<u32>,
}

impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            backend: ModelBackend::Ollama,
            model_name: "qwen2.5-coder:7b".to_owned(),
            binary: "ollama".to_owned(),
            keep_alive: "10m".to_owned(),
            hide_thinking: true,
            fallback_policy: ModelFallbackPolicy::Reported,
            timeout_millis: default_model_timeout_millis(),
            max_output_bytes: default_model_output_bytes(),
            context_window_tokens: None,
            safe_context_tokens: None,
        }
    }
}

fn default_model_timeout_millis() -> u64 {
    180_000
}

fn default_model_output_bytes() -> u64 {
    4_000_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSettings {
    pub max_impact_depth: usize,
}

impl Default for GraphSettings {
    fn default() -> Self {
        Self {
            max_impact_depth: 3,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum ToolTrustMode {
    #[default]
    Guided,
    TrustedWorkspace,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSettings {
    #[serde(default)]
    pub trust_mode: ToolTrustMode,
    #[serde(default = "default_os_sandbox_enabled")]
    pub os_sandbox_enabled: bool,
    #[serde(default = "default_allowed_programs")]
    pub allowed_programs: Vec<String>,
    #[serde(default = "default_tool_timeout_millis")]
    pub default_timeout_millis: u64,
    #[serde(default = "default_tool_output_bytes")]
    pub max_output_bytes: u64,
}

impl Default for ToolSettings {
    fn default() -> Self {
        Self {
            trust_mode: ToolTrustMode::default(),
            os_sandbox_enabled: default_os_sandbox_enabled(),
            allowed_programs: default_allowed_programs(),
            default_timeout_millis: default_tool_timeout_millis(),
            max_output_bytes: default_tool_output_bytes(),
        }
    }
}

fn default_os_sandbox_enabled() -> bool {
    true
}

fn default_allowed_programs() -> Vec<String> {
    [
        "cargo", "rustc", "python", "python3", "pytest", "ruff", "node", "npm", "npx", "pnpm",
        "yarn", "git", "make", "cmake", "ninja", "meson", "go", "gofmt", "gcc", "g++", "clang",
        "clang++", "cc", "c++", "java", "javac", "mvn", "gradle", "dotnet", "msbuild", "swift",
        "swiftc",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn default_tool_timeout_millis() -> u64 {
    120_000
}

fn default_tool_output_bytes() -> u64 {
    1_000_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomySettings {
    #[serde(default = "default_scheduler_enabled")]
    pub scheduler_enabled: bool,
    #[serde(default = "default_scheduler_poll_millis")]
    pub scheduler_poll_millis: u64,
    #[serde(default = "default_scheduler_max_concurrency")]
    pub max_concurrency: usize,
    #[serde(default = "default_oauth_callback_port")]
    pub oauth_callback_port: u16,
}

impl Default for AutonomySettings {
    fn default() -> Self {
        Self {
            scheduler_enabled: default_scheduler_enabled(),
            scheduler_poll_millis: default_scheduler_poll_millis(),
            max_concurrency: default_scheduler_max_concurrency(),
            oauth_callback_port: default_oauth_callback_port(),
        }
    }
}

fn default_scheduler_enabled() -> bool {
    true
}
fn default_scheduler_poll_millis() -> u64 {
    1_000
}
fn default_scheduler_max_concurrency() -> usize {
    2
}
fn default_oauth_callback_port() -> u16 {
    43_821
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorSettings {
    #[serde(default = "default_connector_http_timeout_millis")]
    pub http_timeout_millis: u64,
    #[serde(default = "default_connector_output_bytes")]
    pub max_response_bytes: u64,
    #[serde(default)]
    pub allow_custom_connectors: bool,
}

impl Default for ConnectorSettings {
    fn default() -> Self {
        Self {
            http_timeout_millis: default_connector_http_timeout_millis(),
            max_response_bytes: default_connector_output_bytes(),
            allow_custom_connectors: false,
        }
    }
}

fn default_connector_http_timeout_millis() -> u64 {
    30_000
}
fn default_connector_output_bytes() -> u64 {
    4_000_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchSettings {
    #[serde(default = "default_research_enabled")]
    pub enabled: bool,
    #[serde(default = "default_auto_research")]
    pub auto_research: bool,
    #[serde(default = "default_searxng_url")]
    pub searxng_url: Option<String>,
    #[serde(default = "default_keyless_search_fallback")]
    pub keyless_search_fallback: bool,
    #[serde(default = "default_research_timeout_millis")]
    pub timeout_millis: u64,
    #[serde(default = "default_research_response_bytes")]
    pub max_response_bytes: u64,
    #[serde(default = "default_research_document_bytes")]
    pub max_document_bytes: u64,
    #[serde(default = "default_research_cache_ttl_millis")]
    pub cache_ttl_millis: u64,
    #[serde(default = "default_research_user_agent")]
    pub user_agent: String,
}

impl Default for ResearchSettings {
    fn default() -> Self {
        Self {
            enabled: default_research_enabled(),
            auto_research: default_auto_research(),
            searxng_url: default_searxng_url(),
            keyless_search_fallback: default_keyless_search_fallback(),
            timeout_millis: default_research_timeout_millis(),
            max_response_bytes: default_research_response_bytes(),
            max_document_bytes: default_research_document_bytes(),
            cache_ttl_millis: default_research_cache_ttl_millis(),
            user_agent: default_research_user_agent(),
        }
    }
}

fn default_research_enabled() -> bool {
    true
}
fn default_auto_research() -> bool {
    true
}
fn default_searxng_url() -> Option<String> {
    Some("http://127.0.0.1:8888".to_owned())
}
fn default_keyless_search_fallback() -> bool {
    true
}
fn default_research_timeout_millis() -> u64 {
    20_000
}
fn default_research_response_bytes() -> u64 {
    2_000_000
}
fn default_research_document_bytes() -> u64 {
    1_000_000
}
fn default_research_cache_ttl_millis() -> u64 {
    6 * 60 * 60 * 1000
}
fn default_research_user_agent() -> String {
    "EverythingResearch/0.3 (+local-first)".to_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSettings {
    pub workspace_path: PathBuf,
    pub data_dir: PathBuf,
    pub model: ModelSettings,
    pub graph: GraphSettings,
    #[serde(default)]
    pub tools: ToolSettings,
    #[serde(default)]
    pub autonomy: AutonomySettings,
    #[serde(default)]
    pub connectors: ConnectorSettings,
    #[serde(default)]
    pub research: ResearchSettings,
}

impl RuntimeSettings {
    pub fn for_workspace(workspace_path: PathBuf) -> Self {
        Self {
            data_dir: workspace_path.join(".everything"),
            workspace_path,
            model: ModelSettings::default(),
            graph: GraphSettings::default(),
            tools: ToolSettings::default(),
            autonomy: AutonomySettings::default(),
            connectors: ConnectorSettings::default(),
            research: ResearchSettings::default(),
        }
    }
}
