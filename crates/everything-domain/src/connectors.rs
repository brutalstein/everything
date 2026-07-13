use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum ConnectorProvider {
    Gmail,
    Spotify,
    Instagram,
    TikTok,
    GitHub,
    Custom,
}

impl ConnectorProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gmail => "gmail",
            Self::Spotify => "spotify",
            Self::Instagram => "instagram",
            Self::TikTok => "tiktok",
            Self::GitHub => "github",
            Self::Custom => "custom",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "gmail" | "google-mail" => Some(Self::Gmail),
            "spotify" => Some(Self::Spotify),
            "instagram" | "meta-instagram" => Some(Self::Instagram),
            "tiktok" | "tik-tok" => Some(Self::TikTok),
            "github" | "gh" => Some(Self::GitHub),
            "custom" | "http" => Some(Self::Custom),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum ConnectorStatus {
    NotConfigured,
    ReadyToConnect,
    Authorizing,
    Connected,
    Degraded,
    Error,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum ConnectorRisk {
    ReadOnly,
    ReversibleWrite,
    ExternalPublish,
    AccountMutation,
}

impl ConnectorRisk {
    pub fn requires_explicit_approval(self) -> bool {
        matches!(
            self,
            Self::ExternalPublish | Self::AccountMutation | Self::ReversibleWrite
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorActionDescriptor {
    pub action_id: String,
    pub title: String,
    pub description: String,
    pub risk: ConnectorRisk,
    #[serde(default)]
    pub required_scopes: Vec<String>,
    #[serde(default)]
    pub input_schema: Value,
    #[serde(default)]
    pub supports_dry_run: bool,
    #[serde(default)]
    pub idempotent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorDescriptor {
    pub provider: ConnectorProvider,
    pub display_name: String,
    pub description: String,
    pub status: ConnectorStatus,
    pub configured: bool,
    pub connected: bool,
    #[serde(default)]
    pub account_label: Option<String>,
    #[serde(default)]
    pub granted_scopes: Vec<String>,
    #[serde(default)]
    pub token_expires_at_epoch_millis: Option<u128>,
    #[serde(default)]
    pub actions: Vec<ConnectorActionDescriptor>,
    #[serde(default)]
    pub limitations: Vec<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorConfigureRequest {
    pub provider: ConnectorProvider,
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub redirect_uri: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthStartRequest {
    pub provider: ConnectorProvider,
    #[serde(default)]
    pub force_consent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthStartResponse {
    pub provider: ConnectorProvider,
    pub authorization_url: String,
    pub redirect_uri: String,
    pub state: String,
    pub expires_at_epoch_millis: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthCallbackRequest {
    pub provider: ConnectorProvider,
    pub state: String,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub error_description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorActionRequest {
    pub provider: ConnectorProvider,
    pub action_id: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub approval_granted: bool,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorActionResponse {
    pub provider: ConnectorProvider,
    pub action_id: String,
    pub status: String,
    pub risk: ConnectorRisk,
    pub executed: bool,
    #[serde(default)]
    pub output: Value,
    #[serde(default)]
    pub external_reference: Option<String>,
    #[serde(default)]
    pub retry_after_millis: Option<u64>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorAuditRecord {
    pub audit_id: String,
    pub provider: ConnectorProvider,
    pub action_id: String,
    pub risk: ConnectorRisk,
    pub started_at_epoch_millis: u128,
    pub finished_at_epoch_millis: u128,
    pub status: String,
    pub approval_granted: bool,
    pub input_hash: String,
    #[serde(default)]
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub error_code: Option<String>,
}
