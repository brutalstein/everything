use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelQualityTier {
    Stub,
    Small,
    Medium,
    Large,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCapabilityProfile {
    pub provider: String,
    pub model_name: String,
    pub quality_tier: ModelQualityTier,
    pub context_window_tokens: u32,
    pub safe_context_tokens: u32,
    pub coding_suitability: f32,
    pub structured_output_reliability: f32,
    pub tool_calling_reliability: f32,
    pub estimated_tokens_per_second: Option<f32>,
    pub memory_estimate_mb: Option<u64>,
    #[serde(default)]
    pub recommended_task_classes: Vec<String>,
}

impl ModelCapabilityProfile {
    pub fn conservative_loopback(model_name: impl Into<String>) -> Self {
        Self {
            provider: "loopback".to_owned(),
            model_name: model_name.into(),
            quality_tier: ModelQualityTier::Stub,
            context_window_tokens: 8_192,
            safe_context_tokens: 4_096,
            coding_suitability: 0.25,
            structured_output_reliability: 0.95,
            tool_calling_reliability: 0.0,
            estimated_tokens_per_second: None,
            memory_estimate_mb: Some(128),
            recommended_task_classes: vec![
                "smoke_test".to_owned(),
                "deterministic_fallback".to_owned(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredModel {
    pub provider: String,
    pub model_name: String,
    pub installed: bool,
    pub configured: bool,
    pub profile: ModelCapabilityProfile,
}
