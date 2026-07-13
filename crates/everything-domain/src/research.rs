use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResearchMode {
    #[default]
    General,
    Technical,
    News,
    Academic,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResearchFreshness {
    #[default]
    Any,
    Day,
    Week,
    Month,
    Year,
}

impl ResearchFreshness {
    pub fn days(self) -> Option<u32> {
        match self {
            Self::Any => None,
            Self::Day => Some(1),
            Self::Week => Some(7),
            Self::Month => Some(31),
            Self::Year => Some(366),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchRequest {
    pub query: String,
    #[serde(default)]
    pub mode: ResearchMode,
    #[serde(default)]
    pub freshness: ResearchFreshness,
    #[serde(default = "default_search_results")]
    pub max_results: usize,
    #[serde(default = "default_fetch_pages")]
    pub fetch_pages: usize,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default)]
    pub blocked_domains: Vec<String>,
    #[serde(default)]
    pub force_refresh: bool,
}

fn default_search_results() -> usize {
    12
}
fn default_fetch_pages() -> usize {
    6
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFetchRequest {
    pub url: String,
    #[serde(default)]
    pub force_refresh: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchSource {
    pub citation_id: String,
    pub title: String,
    pub url: String,
    pub canonical_url: String,
    pub domain: String,
    pub provider: String,
    pub rank: usize,
    pub score: f64,
    #[serde(default)]
    pub snippet: String,
    #[serde(default)]
    pub extracted_text: String,
    #[serde(default)]
    pub published_at: Option<String>,
    pub retrieved_at_epoch_millis: u128,
    pub content_hash: String,
    #[serde(default)]
    pub from_cache: bool,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFetchResponse {
    pub source: ResearchSource,
    pub status_code: u16,
    pub media_type: String,
    pub bytes_received: u64,
    pub truncated: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchProviderHealth {
    pub provider: String,
    pub available: bool,
    pub configured: bool,
    pub detail: String,
    #[serde(default)]
    pub latency_millis: Option<u128>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchStatus {
    pub enabled: bool,
    pub cache_path: String,
    pub providers: Vec<ResearchProviderHealth>,
    pub cached_queries: usize,
    pub cached_documents: usize,
    pub cache_bytes: u64,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchReport {
    pub report_id: String,
    pub query: String,
    pub normalized_query: String,
    pub mode: ResearchMode,
    pub freshness: ResearchFreshness,
    pub sources: Vec<ResearchSource>,
    pub provider_count: usize,
    pub cache_hits: usize,
    pub searched_at_epoch_millis: u128,
    pub search_millis: u128,
    pub fetch_millis: u128,
    #[serde(default)]
    pub warnings: Vec<String>,
}
