use anyhow::{Context, Result, anyhow};
use everything_domain::{
    ResearchFreshness, ResearchMode, ResearchProviderHealth, ResearchReport, ResearchSettings,
    ResearchSource, ResearchStatus, WebFetchRequest, WebFetchResponse, WebSearchRequest,
};
use regex::Regex;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::net::{IpAddr, Ipv6Addr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const RESEARCH_SCHEMA_VERSION: u32 = 1;
const MAX_QUERY_BYTES: usize = 4_096;
const MAX_REDIRECTS: usize = 3;
const MAX_EXTRACTED_TEXT_BYTES: usize = 384 * 1024;
const ROBOTS_TTL_MILLIS: u64 = 24 * 60 * 60 * 1000;
const MAX_FETCH_WORKERS: usize = 6;
const MAX_CACHED_SEARCHES: usize = 1_000;
const MAX_CACHED_DOCUMENTS: usize = 5_000;
static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct ResearchRuntime {
    store: ResearchStore,
    client: SafeHttpClient,
    settings: ResearchSettings,
}

impl ResearchRuntime {
    pub fn new(database_path: impl Into<PathBuf>, settings: ResearchSettings) -> Result<Self> {
        Ok(Self {
            store: ResearchStore::new(database_path.into())?,
            client: SafeHttpClient::new(
                settings.timeout_millis,
                settings.max_response_bytes,
                settings.user_agent.clone(),
            ),
            settings,
        })
    }

    pub fn search(&self, request: &WebSearchRequest) -> Result<ResearchReport> {
        anyhow::ensure!(
            self.settings.enabled,
            "web research is disabled by runtime policy"
        );
        let query = normalize_query(&request.query);
        anyhow::ensure!(!query.is_empty(), "search query must not be empty");
        anyhow::ensure!(
            query.len() <= MAX_QUERY_BYTES,
            "search query exceeds {MAX_QUERY_BYTES} bytes"
        );
        anyhow::ensure!(request.max_results <= 100, "max_results cannot exceed 100");
        anyhow::ensure!(request.fetch_pages <= 24, "fetch_pages cannot exceed 24");
        validate_domains(&request.allowed_domains)?;
        validate_domains(&request.blocked_domains)?;

        let cache_key = search_cache_key(&query, request);
        if !request.force_refresh {
            if let Some(mut cached) = self.store.get_search(&cache_key, now_millis())? {
                for source in &mut cached.sources {
                    source.from_cache = true;
                }
                cached.cache_hits = cached.sources.len();
                return Ok(cached);
            }
        }

        let search_started = Instant::now();
        let mut warnings = Vec::new();
        let mut candidates = Vec::<ProviderResult>::new();
        let requested = request.max_results.clamp(1, 100);
        let provider_limit = requested.saturating_mul(2).clamp(8, 100);

        // Provider fan-out is concurrent so a slow public endpoint cannot serialize the
        // complete research pipeline. Ranking below remains deterministic.
        let (sender, receiver) = mpsc::channel();
        std::thread::scope(|scope| {
            if let Some(base_url) = self.settings.searxng_url.as_deref() {
                let searxng_sender = sender.clone();
                let runtime = self.clone();
                let base_url = base_url.to_owned();
                let searxng_query = query.clone();
                scope.spawn(move || {
                    let result =
                        runtime.search_searxng(&base_url, &searxng_query, request, provider_limit);
                    let _ = searxng_sender.send(("SearXNG", result));
                });
            }
            if self.settings.keyless_search_fallback {
                let bing_sender = sender.clone();
                let runtime = self.clone();
                let bing_query = query.clone();
                scope.spawn(move || {
                    let result = runtime.search_bing_rss(&bing_query, request, provider_limit);
                    let _ = bing_sender.send(("Bing RSS", result));
                });
                if matches!(
                    request.mode,
                    ResearchMode::General | ResearchMode::Technical
                ) {
                    let wikipedia_sender = sender.clone();
                    let runtime = self.clone();
                    let wikipedia_query = query.clone();
                    scope.spawn(move || {
                        let result =
                            runtime.search_wikipedia(&wikipedia_query, provider_limit.min(20));
                        let _ = wikipedia_sender.send(("Wikipedia", result));
                    });
                }
                if request.mode == ResearchMode::Academic {
                    let openalex_sender = sender.clone();
                    let runtime = self.clone();
                    let openalex_query = query.clone();
                    scope.spawn(move || {
                        let result =
                            runtime.search_openalex(&openalex_query, provider_limit.min(50));
                        let _ = openalex_sender.send(("OpenAlex", result));
                    });
                }
            }
            drop(sender);
            for (provider, result) in receiver {
                match result {
                    Ok(results) => candidates.extend(results),
                    Err(error) => warnings.push(format!("{provider} unavailable: {error}")),
                }
            }
        });
        warnings.sort();
        let search_millis = search_started.elapsed().as_millis();

        let allowed = request
            .allowed_domains
            .iter()
            .map(|domain| normalize_domain(domain))
            .collect::<BTreeSet<_>>();
        let blocked = request
            .blocked_domains
            .iter()
            .map(|domain| normalize_domain(domain))
            .collect::<BTreeSet<_>>();
        let query_terms = tokenize(&query);
        let mut deduplicated = BTreeMap::<String, ProviderResult>::new();
        for mut result in candidates {
            let Ok(parsed) = ParsedUrl::parse(&result.url) else {
                continue;
            };
            let domain = normalize_domain(&parsed.host);
            if !allowed.is_empty() && !domain_matches_any(&domain, &allowed) {
                continue;
            }
            if domain_matches_any(&domain, &blocked) {
                continue;
            }
            result.canonical_url = canonicalize_url(&result.url);
            let quality = source_quality_score(&domain, request.mode);
            result.score += lexical_score(
                &query_terms,
                &format!("{} {}", result.title, result.snippet),
            );
            result.score += quality;
            result.metadata.insert(
                "source_quality".to_owned(),
                source_quality_label(quality).to_owned(),
            );
            let key = result.canonical_url.clone();
            deduplicated
                .entry(key)
                .and_modify(|existing| {
                    if result.score > existing.score {
                        *existing = result.clone();
                    } else if existing.snippet.len() < result.snippet.len() {
                        existing.snippet = result.snippet.clone();
                    }
                })
                .or_insert(result);
        }
        let mut ranked = deduplicated.into_values().collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.provider_rank.cmp(&right.provider_rank))
                .then_with(|| left.canonical_url.cmp(&right.canonical_url))
        });
        // Prevent one host from monopolising a small-model context window while still
        // allowing explicit single-domain research to return every requested result.
        if allowed.len() != 1 {
            let mut domain_counts = BTreeMap::<String, usize>::new();
            ranked.retain(|result| {
                let domain = ParsedUrl::parse(&result.url)
                    .map(|url| normalize_domain(&url.host))
                    .unwrap_or_default();
                let count = domain_counts.entry(domain).or_default();
                if *count >= 3 {
                    false
                } else {
                    *count += 1;
                    true
                }
            });
        }
        ranked.truncate(requested);

        let fetch_started = Instant::now();
        let fetch_count = request.fetch_pages.min(ranked.len());
        let mut fetched = BTreeMap::<usize, WebFetchResponse>::new();
        if fetch_count > 0 {
            // Bound native threads/curl processes. Local models often run on memory-constrained
            // machines; six concurrent fetches saturate typical broadband without stealing the
            // CPU/RAM budget from inference.
            for batch_start in (0..fetch_count).step_by(MAX_FETCH_WORKERS) {
                let batch_end = (batch_start + MAX_FETCH_WORKERS).min(fetch_count);
                let (sender, receiver) = mpsc::channel();
                std::thread::scope(|scope| {
                    for (index, result) in
                        ranked[batch_start..batch_end].iter().cloned().enumerate()
                    {
                        let sender = sender.clone();
                        let runtime = self.clone();
                        let absolute_index = batch_start + index;
                        scope.spawn(move || {
                            let response = runtime.fetch(&WebFetchRequest {
                                url: result.url,
                                force_refresh: request.force_refresh,
                            });
                            let _ = sender.send((absolute_index, response));
                        });
                    }
                    drop(sender);
                    for (index, response) in receiver {
                        match response {
                            Ok(response) => {
                                fetched.insert(index, response);
                            }
                            Err(error) => warnings.push(format!("page fetch failed: {error}")),
                        }
                    }
                });
            }
        }
        let fetch_millis = fetch_started.elapsed().as_millis();

        let searched_at = now_millis();
        let mut cache_hits = 0usize;
        let sources = ranked
            .into_iter()
            .enumerate()
            .map(|(index, result)| {
                if let Some(response) = fetched.remove(&index) {
                    cache_hits += usize::from(response.source.from_cache);
                    let mut source = response.source;
                    source.provider = result.provider;
                    source.rank = index + 1;
                    source.score = result.score;
                    if source.title.trim().is_empty() {
                        source.title = result.title;
                    }
                    if source.snippet.trim().is_empty() {
                        source.snippet = result.snippet;
                    }
                    source.published_at = result.published_at.or(source.published_at);
                    source.metadata.extend(result.metadata);
                    source
                } else {
                    let domain = ParsedUrl::parse(&result.url)
                        .map(|url| normalize_domain(&url.host))
                        .unwrap_or_default();
                    ResearchSource {
                        citation_id: String::new(),
                        title: result.title,
                        url: result.url,
                        canonical_url: result.canonical_url,
                        domain,
                        provider: result.provider,
                        rank: index + 1,
                        score: result.score,
                        snippet: result.snippet.clone(),
                        extracted_text: String::new(),
                        published_at: result.published_at,
                        retrieved_at_epoch_millis: searched_at,
                        content_hash: blake3::hash(result.snippet.as_bytes()).to_hex().to_string(),
                        from_cache: false,
                        metadata: result.metadata,
                    }
                }
            })
            .enumerate()
            .map(|(index, mut source)| {
                source.citation_id = format!("W{}", index + 1);
                source
            })
            .collect::<Vec<_>>();
        let provider_count = sources
            .iter()
            .map(|source| source.provider.as_str())
            .collect::<BTreeSet<_>>()
            .len();
        let report_id = format!(
            "research_{}",
            &blake3::hash(format!("{query}:{searched_at}").as_bytes()).to_hex()[..24]
        );
        let report = ResearchReport {
            report_id,
            query: request.query.clone(),
            normalized_query: query,
            mode: request.mode,
            freshness: request.freshness,
            sources,
            provider_count,
            cache_hits,
            searched_at_epoch_millis: searched_at,
            search_millis,
            fetch_millis,
            warnings,
        };
        self.store.put_search(
            &cache_key,
            &report,
            searched_at.saturating_add(u128::from(self.settings.cache_ttl_millis)),
        )?;
        Ok(report)
    }

    pub fn fetch(&self, request: &WebFetchRequest) -> Result<WebFetchResponse> {
        anyhow::ensure!(
            self.settings.enabled,
            "web research is disabled by runtime policy"
        );
        let parsed = ParsedUrl::parse(&request.url)?;
        if !request.force_refresh {
            if let Some(mut response) = self
                .store
                .get_document(&canonicalize_url(&request.url), now_millis())?
            {
                response.source.from_cache = true;
                return Ok(response);
            }
        }
        if !parsed.is_loopback() && !self.robots_allowed(&parsed)? {
            return Err(anyhow!(
                "robots.txt disallows research fetch for {}",
                parsed.host
            ));
        }
        let response = self.client.get(&request.url, &[])?;
        anyhow::ensure!(
            (200..300).contains(&response.status_code),
            "HTTP {} while fetching {}",
            response.status_code,
            request.url
        );
        let media_type = response
            .headers
            .get("content-type")
            .cloned()
            .unwrap_or_else(|| "application/octet-stream".to_owned());
        anyhow::ensure!(
            is_text_media_type(&media_type),
            "unsupported research content type '{media_type}'"
        );
        let body = String::from_utf8_lossy(&response.body);
        let extracted = extract_document(&body, &media_type);
        let title = extracted
            .title
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| parsed.host.clone());
        let canonical_url = extracted
            .canonical_url
            .filter(|url| ParsedUrl::parse(url).is_ok())
            .unwrap_or_else(|| canonicalize_url(&response.final_url));
        let content_hash = blake3::hash(extracted.text.as_bytes()).to_hex().to_string();
        let retrieved_at = now_millis();
        let source = ResearchSource {
            citation_id: "W1".to_owned(),
            title,
            url: response.final_url.clone(),
            canonical_url: canonical_url.clone(),
            domain: ParsedUrl::parse(&canonical_url)
                .map(|url| normalize_domain(&url.host))
                .unwrap_or_else(|_| parsed.host.clone()),
            provider: "direct_fetch".to_owned(),
            rank: 1,
            score: 1.0,
            snippet: first_sentence(&extracted.text, 480),
            extracted_text: truncate_utf8(
                &extracted.text,
                usize::try_from(self.settings.max_document_bytes)
                    .unwrap_or(MAX_EXTRACTED_TEXT_BYTES)
                    .clamp(16 * 1024, 4 * 1024 * 1024),
            ),
            published_at: extracted.published_at,
            retrieved_at_epoch_millis: retrieved_at,
            content_hash,
            from_cache: false,
            metadata: BTreeMap::from([
                (
                    "etag".to_owned(),
                    response.headers.get("etag").cloned().unwrap_or_default(),
                ),
                (
                    "last_modified".to_owned(),
                    response
                        .headers
                        .get("last-modified")
                        .cloned()
                        .unwrap_or_default(),
                ),
            ]),
        };
        let result = WebFetchResponse {
            source,
            status_code: response.status_code,
            media_type,
            bytes_received: response.body.len() as u64,
            truncated: response.truncated,
            warnings: Vec::new(),
        };
        self.store.put_document(
            &canonical_url,
            &result,
            retrieved_at.saturating_add(u128::from(self.settings.cache_ttl_millis)),
        )?;
        Ok(result)
    }

    pub fn status(&self) -> Result<ResearchStatus> {
        let curl_available = command_exists("curl");
        let mut providers = Vec::new();
        providers.push(ResearchProviderHealth {
            provider: "searxng".to_owned(),
            available: curl_available && self.settings.searxng_url.is_some(),
            configured: self.settings.searxng_url.is_some(),
            detail: self
                .settings
                .searxng_url
                .clone()
                .unwrap_or_else(|| "not configured".to_owned()),
            latency_millis: None,
        });
        for provider in ["bing_rss", "wikipedia", "openalex"] {
            providers.push(ResearchProviderHealth {
                provider: provider.to_owned(),
                available: curl_available && self.settings.keyless_search_fallback,
                configured: self.settings.keyless_search_fallback,
                detail: "keyless HTTPS fallback".to_owned(),
                latency_millis: None,
            });
        }
        let (cached_queries, cached_documents) = self.store.counts()?;
        Ok(ResearchStatus {
            enabled: self.settings.enabled,
            cache_path: self.store.path.display().to_string(),
            providers,
            cached_queries,
            cached_documents,
            cache_bytes: std::fs::metadata(&self.store.path)
                .map(|meta| meta.len())
                .unwrap_or(0),
            warnings: if curl_available {
                Vec::new()
            } else {
                vec!["curl is required for the hardened HTTPS transport".to_owned()]
            },
        })
    }

    pub fn purge_expired(&self) -> Result<usize> {
        self.store.purge_expired(now_millis())
    }

    fn search_searxng(
        &self,
        base_url: &str,
        query: &str,
        request: &WebSearchRequest,
        limit: usize,
    ) -> Result<Vec<ProviderResult>> {
        let base = base_url.trim_end_matches('/');
        let categories = match request.mode {
            ResearchMode::News => "news",
            ResearchMode::Academic => "science",
            ResearchMode::Technical => "it",
            ResearchMode::General => "general",
        };
        let mut url = format!(
            "{base}/search?q={}&format=json&language=all&safesearch=1&categories={categories}",
            percent_encode(query)
        );
        if let Some(days) = request.freshness.days() {
            let range = if days <= 1 {
                "day"
            } else if days <= 31 {
                "month"
            } else {
                "year"
            };
            url.push_str("&time_range=");
            url.push_str(range);
        }
        let response = self.client.get(&url, &[])?;
        anyhow::ensure!(
            (200..300).contains(&response.status_code),
            "SearXNG returned HTTP {}",
            response.status_code
        );
        let value: Value = serde_json::from_slice(&response.body).context("parse SearXNG JSON")?;
        let results = value
            .get("results")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("SearXNG JSON did not contain results"))?;
        Ok(results
            .iter()
            .take(limit)
            .enumerate()
            .filter_map(|(index, item)| {
                let url = item.get("url")?.as_str()?.to_owned();
                let title = item
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or(&url)
                    .to_owned();
                let snippet = item
                    .get("content")
                    .or_else(|| item.get("snippet"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                Some(ProviderResult {
                    title,
                    url: url.clone(),
                    canonical_url: canonicalize_url(&url),
                    snippet,
                    published_at: item
                        .get("publishedDate")
                        .or_else(|| item.get("published_date"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    provider: "searxng".to_owned(),
                    provider_rank: index + 1,
                    score: 1.2 / (index as f64 + 1.0).sqrt(),
                    metadata: BTreeMap::new(),
                })
            })
            .collect())
    }

    fn search_bing_rss(
        &self,
        query: &str,
        request: &WebSearchRequest,
        limit: usize,
    ) -> Result<Vec<ProviderResult>> {
        let freshness = match request.freshness {
            ResearchFreshness::Day => " freshness:Day",
            ResearchFreshness::Week => " freshness:Week",
            ResearchFreshness::Month => " freshness:Month",
            _ => "",
        };
        let url = format!(
            "https://www.bing.com/search?format=rss&q={}",
            percent_encode(&format!("{query}{freshness}"))
        );
        let response = self.client.get(&url, &[])?;
        anyhow::ensure!(
            (200..300).contains(&response.status_code),
            "Bing RSS returned HTTP {}",
            response.status_code
        );
        let xml = String::from_utf8_lossy(&response.body);
        let item_regex = Regex::new(r"(?is)<item>(.*?)</item>")?;
        Ok(item_regex
            .captures_iter(&xml)
            .take(limit)
            .enumerate()
            .filter_map(|(index, capture)| {
                let item = capture.get(1)?.as_str();
                let title = xml_tag(item, "title").unwrap_or_default();
                let url = xml_tag(item, "link")?;
                let snippet = xml_tag(item, "description").unwrap_or_default();
                Some(ProviderResult {
                    title: decode_entities(&strip_tags(&title)),
                    url: decode_entities(&url),
                    canonical_url: canonicalize_url(&url),
                    snippet: decode_entities(&strip_tags(&snippet)),
                    published_at: xml_tag(item, "pubDate"),
                    provider: "bing_rss".to_owned(),
                    provider_rank: index + 1,
                    score: 1.0 / (index as f64 + 1.0).sqrt(),
                    metadata: BTreeMap::new(),
                })
            })
            .collect())
    }

    fn search_wikipedia(&self, query: &str, limit: usize) -> Result<Vec<ProviderResult>> {
        let url = format!(
            "https://en.wikipedia.org/w/api.php?action=opensearch&search={}&limit={limit}&namespace=0&format=json",
            percent_encode(query)
        );
        let response = self.client.get(&url, &[])?;
        anyhow::ensure!(
            (200..300).contains(&response.status_code),
            "Wikipedia returned HTTP {}",
            response.status_code
        );
        let value: Value = serde_json::from_slice(&response.body)?;
        let array = value
            .as_array()
            .ok_or_else(|| anyhow!("invalid Wikipedia response"))?;
        let titles = array
            .get(1)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let descriptions = array
            .get(2)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let urls = array
            .get(3)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(urls
            .iter()
            .take(limit)
            .enumerate()
            .filter_map(|(index, url)| {
                let url = url.as_str()?.to_owned();
                Some(ProviderResult {
                    title: titles
                        .get(index)
                        .and_then(Value::as_str)
                        .unwrap_or(&url)
                        .to_owned(),
                    url: url.clone(),
                    canonical_url: canonicalize_url(&url),
                    snippet: descriptions
                        .get(index)
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    published_at: None,
                    provider: "wikipedia".to_owned(),
                    provider_rank: index + 1,
                    score: 0.72 / (index as f64 + 1.0).sqrt(),
                    metadata: BTreeMap::new(),
                })
            })
            .collect())
    }

    fn search_openalex(&self, query: &str, limit: usize) -> Result<Vec<ProviderResult>> {
        let url = format!(
            "https://api.openalex.org/works?search={}&per-page={limit}&select=id,doi,display_name,publication_year,primary_location",
            percent_encode(query)
        );
        let response = self.client.get(&url, &[])?;
        anyhow::ensure!(
            (200..300).contains(&response.status_code),
            "OpenAlex returned HTTP {}",
            response.status_code
        );
        let value: Value = serde_json::from_slice(&response.body)?;
        let results = value
            .get("results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(results
            .iter()
            .take(limit)
            .enumerate()
            .filter_map(|(index, item)| {
                let title = item.get("display_name")?.as_str()?.to_owned();
                let url = item
                    .get("doi")
                    .and_then(Value::as_str)
                    .or_else(|| item.get("id").and_then(Value::as_str))?
                    .to_owned();
                Some(ProviderResult {
                    title,
                    url: url.clone(),
                    canonical_url: canonicalize_url(&url),
                    snippet: item
                        .get("publication_year")
                        .and_then(Value::as_i64)
                        .map(|year| format!("Publication year: {year}"))
                        .unwrap_or_default(),
                    published_at: item
                        .get("publication_year")
                        .and_then(Value::as_i64)
                        .map(|year| year.to_string()),
                    provider: "openalex".to_owned(),
                    provider_rank: index + 1,
                    score: 0.95 / (index as f64 + 1.0).sqrt(),
                    metadata: BTreeMap::new(),
                })
            })
            .collect())
    }

    fn robots_allowed(&self, parsed: &ParsedUrl) -> Result<bool> {
        let key = format!("{}://{}:{}", parsed.scheme, parsed.host, parsed.port);
        if let Some(rules) = self.store.get_robots(&key, now_millis())? {
            return Ok(robots_path_allowed(&rules, &parsed.path_query));
        }
        let robots_url = format!(
            "{}://{}:{}/robots.txt",
            parsed.scheme,
            host_for_url(&parsed.host),
            parsed.port
        );
        let rules = match self.client.get(&robots_url, &[]) {
            Ok(response) if (200..300).contains(&response.status_code) => {
                truncate_utf8(&String::from_utf8_lossy(&response.body), 256 * 1024)
            }
            _ => String::new(),
        };
        self.store.put_robots(
            &key,
            &rules,
            now_millis().saturating_add(u128::from(ROBOTS_TTL_MILLIS)),
        )?;
        Ok(robots_path_allowed(&rules, &parsed.path_query))
    }
}

#[derive(Debug, Clone)]
struct ProviderResult {
    title: String,
    url: String,
    canonical_url: String,
    snippet: String,
    published_at: Option<String>,
    provider: String,
    provider_rank: usize,
    score: f64,
    metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
struct ResearchStore {
    path: PathBuf,
}

impl ResearchStore {
    fn new(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let store = Self { path };
        let connection = store.connect()?;
        connection.execute_batch(RESEARCH_SCHEMA)?;
        let existing = connection
            .query_row(
                "SELECT value FROM metadata WHERE key='schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .and_then(|value| value.parse::<u32>().ok());
        if let Some(version) = existing {
            anyhow::ensure!(
                version <= RESEARCH_SCHEMA_VERSION,
                "research cache schema {version} is newer than runtime schema {RESEARCH_SCHEMA_VERSION}"
            );
        }
        connection.execute(
            "INSERT OR REPLACE INTO metadata(key, value) VALUES('schema_version', ?1)",
            [RESEARCH_SCHEMA_VERSION.to_string()],
        )?;
        Ok(store)
    }

    fn connect(&self) -> Result<Connection> {
        let connection = Connection::open(&self.path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.busy_timeout(Duration::from_secs(5))?;
        Ok(connection)
    }

    fn get_search(&self, key: &str, now: u128) -> Result<Option<ResearchReport>> {
        let connection = self.connect()?;
        let payload = connection
            .query_row(
                "SELECT payload_json FROM search_cache WHERE cache_key=?1 AND expires_at>?2",
                params![key, now as i64],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        payload
            .map(|payload| serde_json::from_str(&payload).context("parse cached research report"))
            .transpose()
    }

    fn put_search(&self, key: &str, report: &ResearchReport, expires_at: u128) -> Result<()> {
        let connection = self.connect()?;
        connection.execute(
            "INSERT INTO search_cache(cache_key, query, payload_json, created_at, expires_at) VALUES(?1, ?2, ?3, ?4, ?5) ON CONFLICT(cache_key) DO UPDATE SET query=excluded.query, payload_json=excluded.payload_json, created_at=excluded.created_at, expires_at=excluded.expires_at",
            params![key, report.normalized_query, serde_json::to_string(report)?, report.searched_at_epoch_millis as i64, expires_at as i64],
        )?;
        connection.execute(
            "DELETE FROM search_cache WHERE cache_key IN (SELECT cache_key FROM search_cache ORDER BY created_at DESC LIMIT -1 OFFSET ?1)",
            [MAX_CACHED_SEARCHES as i64],
        )?;
        Ok(())
    }

    fn get_document(&self, canonical_url: &str, now: u128) -> Result<Option<WebFetchResponse>> {
        let connection = self.connect()?;
        let payload = connection
            .query_row(
                "SELECT payload_json FROM documents WHERE canonical_url=?1 AND expires_at>?2",
                params![canonical_url, now as i64],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        payload
            .map(|payload| serde_json::from_str(&payload).context("parse cached web document"))
            .transpose()
    }

    fn put_document(
        &self,
        canonical_url: &str,
        response: &WebFetchResponse,
        expires_at: u128,
    ) -> Result<()> {
        let connection = self.connect()?;
        connection.execute(
            "INSERT INTO documents(canonical_url, url_hash, content_hash, payload_json, retrieved_at, expires_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(canonical_url) DO UPDATE SET url_hash=excluded.url_hash, content_hash=excluded.content_hash, payload_json=excluded.payload_json, retrieved_at=excluded.retrieved_at, expires_at=excluded.expires_at",
            params![canonical_url, blake3::hash(canonical_url.as_bytes()).to_hex().to_string(), response.source.content_hash, serde_json::to_string(response)?, response.source.retrieved_at_epoch_millis as i64, expires_at as i64],
        )?;
        connection.execute(
            "DELETE FROM document_fts WHERE canonical_url=?1",
            [canonical_url],
        )?;
        connection.execute(
            "INSERT INTO document_fts(canonical_url, title, body) VALUES(?1, ?2, ?3)",
            params![
                canonical_url,
                response.source.title,
                response.source.extracted_text
            ],
        )?;
        let excess_urls = {
            let mut statement = connection.prepare(
                "SELECT canonical_url FROM documents ORDER BY retrieved_at DESC LIMIT -1 OFFSET ?1",
            )?;
            statement
                .query_map([MAX_CACHED_DOCUMENTS as i64], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for url in excess_urls {
            connection.execute("DELETE FROM document_fts WHERE canonical_url=?1", [&url])?;
            connection.execute("DELETE FROM documents WHERE canonical_url=?1", [&url])?;
        }
        Ok(())
    }

    fn get_robots(&self, host_key: &str, now: u128) -> Result<Option<String>> {
        let connection = self.connect()?;
        Ok(connection
            .query_row(
                "SELECT rules FROM robots_cache WHERE host_key=?1 AND expires_at>?2",
                params![host_key, now as i64],
                |row| row.get(0),
            )
            .optional()?)
    }

    fn put_robots(&self, host_key: &str, rules: &str, expires_at: u128) -> Result<()> {
        let connection = self.connect()?;
        connection.execute(
            "INSERT INTO robots_cache(host_key, rules, expires_at) VALUES(?1, ?2, ?3) ON CONFLICT(host_key) DO UPDATE SET rules=excluded.rules, expires_at=excluded.expires_at",
            params![host_key, rules, expires_at as i64],
        )?;
        Ok(())
    }

    fn counts(&self) -> Result<(usize, usize)> {
        let connection = self.connect()?;
        let queries = connection.query_row("SELECT COUNT(*) FROM search_cache", [], |row| {
            row.get::<_, i64>(0)
        })? as usize;
        let documents = connection.query_row("SELECT COUNT(*) FROM documents", [], |row| {
            row.get::<_, i64>(0)
        })? as usize;
        Ok((queries, documents))
    }

    fn purge_expired(&self, now: u128) -> Result<usize> {
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        let mut removed = 0usize;
        removed += transaction.execute(
            "DELETE FROM search_cache WHERE expires_at<=?1",
            [now as i64],
        )?;
        let expired_urls = {
            let mut statement =
                transaction.prepare("SELECT canonical_url FROM documents WHERE expires_at<=?1")?;
            statement
                .query_map([now as i64], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for url in &expired_urls {
            transaction.execute("DELETE FROM document_fts WHERE canonical_url=?1", [url])?;
        }
        removed +=
            transaction.execute("DELETE FROM documents WHERE expires_at<=?1", [now as i64])?;
        removed += transaction.execute(
            "DELETE FROM robots_cache WHERE expires_at<=?1",
            [now as i64],
        )?;
        transaction.commit()?;
        Ok(removed)
    }
}

#[derive(Debug, Clone)]
struct SafeHttpClient {
    timeout_millis: u64,
    max_response_bytes: u64,
    user_agent: String,
}

#[derive(Debug)]
struct HttpResponse {
    status_code: u16,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
    final_url: String,
    truncated: bool,
}

impl SafeHttpClient {
    fn new(timeout_millis: u64, max_response_bytes: u64, user_agent: String) -> Self {
        Self {
            timeout_millis: timeout_millis.clamp(1_000, 120_000),
            max_response_bytes: max_response_bytes.clamp(16 * 1024, 16 * 1024 * 1024),
            user_agent,
        }
    }

    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<HttpResponse> {
        let mut current = url.to_owned();
        for _ in 0..=MAX_REDIRECTS {
            let response = self.get_once(&current, headers)?;
            if matches!(response.status_code, 301 | 302 | 303 | 307 | 308) {
                let location = response
                    .headers
                    .get("location")
                    .ok_or_else(|| anyhow!("redirect response did not include Location"))?;
                current = resolve_redirect(&current, location)?;
                continue;
            }
            return Ok(HttpResponse {
                final_url: current,
                ..response
            });
        }
        Err(anyhow!("too many HTTP redirects"))
    }

    fn get_once(&self, url: &str, headers: &[(String, String)]) -> Result<HttpResponse> {
        let parsed = ParsedUrl::parse(url)?;
        let pinned_ip = parsed.resolve_public_ip()?;
        let request_sequence = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let suffix = &blake3::hash(
            format!(
                "{url}:{}:{}:{request_sequence}",
                std::process::id(),
                now_millis()
            )
            .as_bytes(),
        )
        .to_hex()[..16];
        let temp_dir = std::env::temp_dir().join("everything-research");
        std::fs::create_dir_all(&temp_dir)?;
        let header_path = temp_dir.join(format!("headers-{suffix}.txt"));
        let body_path = temp_dir.join(format!("body-{suffix}.bin"));
        let mut command = Command::new("curl");
        command
            .arg("--silent")
            .arg("--show-error")
            .arg("--request")
            .arg("GET")
            .arg("--connect-timeout")
            .arg((self.timeout_millis / 1_000).clamp(1, 30).to_string())
            .arg("--max-time")
            .arg(self.timeout_millis.div_ceil(1_000).clamp(1, 120).to_string())
            .arg("--max-filesize")
            .arg(self.max_response_bytes.to_string())
            .arg("--proto")
            .arg(if parsed.is_loopback() { "=http,https" } else { "=https" })
            .arg("--proto-redir")
            .arg("=https")
            .arg("--dump-header")
            .arg(&header_path)
            .arg("--output")
            .arg(&body_path)
            .arg("--write-out")
            .arg("%{http_code}")
            .arg("--header")
            .arg("Accept: text/html,application/xhtml+xml,application/json,application/xml,text/plain;q=0.9,*/*;q=0.1")
            .arg("--header")
            .arg("Accept-Encoding: gzip, br, zstd")
            .arg("--compressed")
            .arg("--user-agent")
            .arg(&self.user_agent)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(ip) = pinned_ip {
            command.arg("--resolve").arg(format!(
                "{}:{}:{}",
                parsed.host,
                parsed.port,
                resolve_ip_for_curl(ip)
            ));
        }
        for (name, value) in headers {
            validate_header(name, value)?;
            command.arg("--header").arg(format!("{name}: {value}"));
        }
        command.arg(url);
        let output = command
            .output()
            .with_context(|| "execute curl research transport")?;
        let status_text = String::from_utf8_lossy(&output.stdout);
        let status_code = status_text.trim().parse::<u16>().unwrap_or_default();
        let header_text = std::fs::read_to_string(&header_path).unwrap_or_default();
        let (body, truncated) = read_limited_file(&body_path, self.max_response_bytes as usize);
        let _ = std::fs::remove_file(&header_path);
        let _ = std::fs::remove_file(&body_path);
        if !output.status.success() && status_code == 0 {
            return Err(anyhow!(
                "curl transport failed: {}",
                truncate_utf8(&String::from_utf8_lossy(&output.stderr), 2_048)
            ));
        }
        Ok(HttpResponse {
            status_code,
            headers: parse_headers(&header_text),
            body,
            final_url: url.to_owned(),
            truncated,
        })
    }
}

#[derive(Debug, Clone)]
struct ParsedUrl {
    scheme: String,
    host: String,
    port: u16,
    path_query: String,
}

impl ParsedUrl {
    fn parse(value: &str) -> Result<Self> {
        anyhow::ensure!(value.len() <= 8_192, "URL is too long");
        anyhow::ensure!(
            !value.chars().any(char::is_control),
            "URL contains control characters"
        );
        anyhow::ensure!(
            !value.contains('#'),
            "URL fragments are not accepted by the research fetcher"
        );
        let (scheme, rest) = value
            .split_once("://")
            .ok_or_else(|| anyhow!("URL must include a scheme"))?;
        let scheme = scheme.to_ascii_lowercase();
        anyhow::ensure!(
            matches!(scheme.as_str(), "https" | "http"),
            "unsupported URL scheme"
        );
        let authority_end = rest
            .find('/')
            .or_else(|| rest.find('?'))
            .unwrap_or(rest.len());
        let authority = &rest[..authority_end];
        anyhow::ensure!(!authority.contains('@'), "URL userinfo is not allowed");
        anyhow::ensure!(!authority.is_empty(), "URL host is empty");
        let path_query = if authority_end < rest.len() {
            let suffix = &rest[authority_end..];
            if suffix.starts_with('?') {
                format!("/{suffix}")
            } else {
                suffix.to_owned()
            }
        } else {
            "/".to_owned()
        };
        let (host, port) = if authority.starts_with('[') {
            let closing = authority
                .find(']')
                .ok_or_else(|| anyhow!("invalid IPv6 URL host"))?;
            let host = authority[1..closing].to_owned();
            let port = authority[closing + 1..]
                .strip_prefix(':')
                .map(str::parse::<u16>)
                .transpose()?
                .unwrap_or(if scheme == "https" { 443 } else { 80 });
            (host, port)
        } else if let Some((host, port)) = authority.rsplit_once(':') {
            if port.chars().all(|character| character.is_ascii_digit()) {
                (host.to_owned(), port.parse::<u16>()?)
            } else {
                (
                    authority.to_owned(),
                    if scheme == "https" { 443 } else { 80 },
                )
            }
        } else {
            (
                authority.to_owned(),
                if scheme == "https" { 443 } else { 80 },
            )
        };
        anyhow::ensure!(
            !host.is_empty() && host.is_ascii(),
            "URL host must be non-empty ASCII"
        );
        let parsed = Self {
            scheme,
            host: host.to_ascii_lowercase(),
            port,
            path_query,
        };
        if parsed.scheme == "http" {
            anyhow::ensure!(
                parsed.is_loopback(),
                "plain HTTP is only permitted for loopback services"
            );
        }
        Ok(parsed)
    }

    fn is_loopback(&self) -> bool {
        self.host == "localhost"
            || self.host == "127.0.0.1"
            || self.host == "::1"
            || self.host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
    }

    fn resolve_public_ip(&self) -> Result<Option<IpAddr>> {
        if self.is_loopback() {
            anyhow::ensure!(
                self.scheme == "http",
                "loopback hosts are only permitted for explicit plain-HTTP local services"
            );
            return Ok(None);
        }
        let mut addresses = (self.host.as_str(), self.port)
            .to_socket_addrs()
            .with_context(|| format!("resolve host {}", self.host))?
            .map(|address| address.ip())
            .collect::<Vec<_>>();
        addresses.sort_by_key(|ip| match ip {
            IpAddr::V4(_) => 0,
            IpAddr::V6(_) => 1,
        });
        addresses.dedup();
        let public = addresses.into_iter().find(|ip| is_public_ip(*ip));
        public
            .map(Some)
            .ok_or_else(|| anyhow!("host {} did not resolve to a public IP address", self.host))
    }
}

#[derive(Debug)]
struct ExtractedDocument {
    title: Option<String>,
    canonical_url: Option<String>,
    published_at: Option<String>,
    text: String,
}

fn extract_document(body: &str, media_type: &str) -> ExtractedDocument {
    if media_type.to_ascii_lowercase().contains("html") {
        let title = Regex::new(r"(?is)<title[^>]*>(.*?)</title>")
            .ok()
            .and_then(|regex| regex.captures(body))
            .and_then(|capture| capture.get(1))
            .map(|value| {
                decode_entities(&strip_tags(value.as_str()))
                    .trim()
                    .to_owned()
            });
        let canonical_url = Regex::new(
            r#"(?is)<link[^>]+rel=["'][^"']*canonical[^"']*["'][^>]+href=["']([^"']+)["']"#,
        )
        .ok()
        .and_then(|regex| regex.captures(body))
        .and_then(|capture| capture.get(1))
        .map(|value| value.as_str().to_owned())
        .or_else(|| {
            Regex::new(
                r#"(?is)<link[^>]+href=["']([^"']+)["'][^>]+rel=["'][^"']*canonical[^"']*["']"#,
            )
            .ok()
            .and_then(|regex| regex.captures(body))
            .and_then(|capture| capture.get(1))
            .map(|value| value.as_str().to_owned())
        });
        let published_at = ["article:published_time", "datePublished", "date"]
            .iter()
            .find_map(|name| meta_content(body, name));
        let mut cleaned = body.to_owned();
        for tag in [
            "script", "style", "noscript", "svg", "nav", "footer", "header", "form",
        ] {
            if let Ok(regex) = Regex::new(&format!(r"(?is)<{tag}[^>]*>.*?</{tag}>")) {
                cleaned = regex.replace_all(&cleaned, " ").into_owned();
            }
        }
        let text = collapse_whitespace(&decode_entities(&strip_tags(&cleaned)));
        ExtractedDocument {
            title,
            canonical_url,
            published_at,
            text,
        }
    } else if media_type.to_ascii_lowercase().contains("json") {
        let text = serde_json::from_str::<Value>(body)
            .and_then(|value| serde_json::to_string_pretty(&value))
            .unwrap_or_else(|_| body.to_owned());
        ExtractedDocument {
            title: None,
            canonical_url: None,
            published_at: None,
            text: truncate_utf8(&text, MAX_EXTRACTED_TEXT_BYTES),
        }
    } else {
        ExtractedDocument {
            title: None,
            canonical_url: None,
            published_at: None,
            text: collapse_whitespace(&decode_entities(&strip_tags(body))),
        }
    }
}

fn meta_content(body: &str, name: &str) -> Option<String> {
    let escaped = regex::escape(name);
    let patterns = [
        format!(
            r#"(?is)<meta[^>]+(?:property|name)=["']{escaped}["'][^>]+content=["']([^"']+)["']"#
        ),
        format!(
            r#"(?is)<meta[^>]+content=["']([^"']+)["'][^>]+(?:property|name)=["']{escaped}["']"#
        ),
    ];
    patterns.into_iter().find_map(|pattern| {
        Regex::new(&pattern)
            .ok()
            .and_then(|regex| regex.captures(body))
            .and_then(|capture| capture.get(1))
            .map(|value| decode_entities(value.as_str()))
    })
}

fn robots_path_allowed(rules: &str, path: &str) -> bool {
    if rules.trim().is_empty() {
        return true;
    }
    let mut applies = false;
    let mut allowed = true;
    for raw_line in rules.lines() {
        let line = raw_line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        match name.trim().to_ascii_lowercase().as_str() {
            "user-agent" => {
                let agent = value.trim().to_ascii_lowercase();
                applies = agent == "*" || agent.contains("everythingresearch");
            }
            "disallow" if applies => {
                let rule = value.trim();
                if !rule.is_empty() && path.starts_with(rule) {
                    allowed = false;
                }
            }
            "allow" if applies => {
                let rule = value.trim();
                if !rule.is_empty() && path.starts_with(rule) {
                    allowed = true;
                }
            }
            _ => {}
        }
    }
    allowed
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.octets()[0] == 0
                || ip.octets()[0] >= 224)
        }
        IpAddr::V6(ip) => {
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || is_unique_local_v6(ip)
                || is_link_local_v6(ip)
                || is_documentation_v6(ip))
        }
    }
}

fn is_unique_local_v6(ip: Ipv6Addr) -> bool {
    ip.segments()[0] & 0xfe00 == 0xfc00
}
fn is_link_local_v6(ip: Ipv6Addr) -> bool {
    ip.segments()[0] & 0xffc0 == 0xfe80
}
fn is_documentation_v6(ip: Ipv6Addr) -> bool {
    ip.segments()[0] == 0x2001 && ip.segments()[1] == 0x0db8
}

fn resolve_ip_for_curl(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(ip) => ip.to_string(),
        IpAddr::V6(ip) => format!("[{ip}]"),
    }
}

fn validate_header(name: &str, value: &str) -> Result<()> {
    anyhow::ensure!(
        !name.is_empty()
            && name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-'),
        "invalid HTTP header name"
    );
    anyhow::ensure!(
        !value
            .chars()
            .any(|character| matches!(character, '\r' | '\n' | '\0')),
        "invalid HTTP header value"
    );
    Ok(())
}

fn parse_headers(raw: &str) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    for line in raw.lines() {
        if line.starts_with("HTTP/") {
            headers.clear();
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    headers
}

fn read_limited_file(path: &Path, limit: usize) -> (Vec<u8>, bool) {
    let Ok(mut file) = std::fs::File::open(path) else {
        return (Vec::new(), false);
    };
    let mut body = Vec::with_capacity(limit.min(64 * 1024));
    let mut buffer = [0u8; 8_192];
    let mut truncated = false;
    loop {
        let count = match file.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        let remaining = limit.saturating_sub(body.len());
        body.extend_from_slice(&buffer[..count.min(remaining)]);
        if count > remaining {
            truncated = true;
            break;
        }
    }
    (body, truncated)
}

fn resolve_redirect(base: &str, location: &str) -> Result<String> {
    if location.starts_with("https://") || location.starts_with("http://") {
        ParsedUrl::parse(location)?;
        return Ok(location.to_owned());
    }
    let base = ParsedUrl::parse(base)?;
    anyhow::ensure!(
        location.starts_with('/'),
        "relative redirect must be origin-absolute"
    );
    let redirected = format!(
        "{}://{}:{}{}",
        base.scheme,
        host_for_url(&base.host),
        base.port,
        location
    );
    ParsedUrl::parse(&redirected)?;
    Ok(redirected)
}

fn host_for_url(host: &str) -> String {
    if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    }
}

fn normalize_query(query: &str) -> String {
    query.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn search_cache_key(query: &str, request: &WebSearchRequest) -> String {
    blake3::hash(
        serde_json::to_string(&(
            query,
            request.mode,
            request.freshness,
            request.max_results,
            request.fetch_pages,
            &request.allowed_domains,
            &request.blocked_domains,
        ))
        .unwrap_or_default()
        .as_bytes(),
    )
    .to_hex()
    .to_string()
}

fn tokenize(value: &str) -> BTreeSet<String> {
    value
        .split(|character: char| {
            !character.is_alphanumeric() && character != '_' && character != '-'
        })
        .map(str::to_ascii_lowercase)
        .filter(|token| token.len() >= 2)
        .collect()
}

fn lexical_score(query_terms: &BTreeSet<String>, value: &str) -> f64 {
    if query_terms.is_empty() {
        return 0.0;
    }
    let value_terms = tokenize(value);
    let matches = query_terms.intersection(&value_terms).count();
    matches as f64 / query_terms.len() as f64
}

fn source_quality_score(domain: &str, mode: ResearchMode) -> f64 {
    let official_prefix = domain.starts_with("docs.")
        || domain.starts_with("developer.")
        || domain.starts_with("api.");
    let primary_domain = official_prefix
        || domain.ends_with(".gov")
        || domain.ends_with(".edu")
        || matches!(
            domain,
            "github.com"
                | "docs.github.com"
                | "rust-lang.org"
                | "doc.rust-lang.org"
                | "python.org"
                | "docs.python.org"
                | "nodejs.org"
                | "developer.mozilla.org"
                | "ietf.org"
                | "rfc-editor.org"
                | "w3.org"
                | "openalex.org"
                | "api.openalex.org"
        );
    let technical_registry = matches!(domain, "crates.io" | "docs.rs" | "npmjs.com" | "pypi.org");
    if primary_domain {
        0.35
    } else if technical_registry && matches!(mode, ResearchMode::Technical | ResearchMode::Academic)
    {
        0.24
    } else {
        0.0
    }
}

fn source_quality_label(score: f64) -> &'static str {
    if score >= 0.34 {
        "primary"
    } else if score >= 0.20 {
        "registry"
    } else {
        "discovered"
    }
}

fn validate_domains(domains: &[String]) -> Result<()> {
    for domain in domains {
        let normalized = normalize_domain(domain);
        anyhow::ensure!(
            !normalized.is_empty() && normalized.is_ascii(),
            "invalid domain filter"
        );
        anyhow::ensure!(
            normalized.chars().all(
                |character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-')
            ),
            "invalid domain filter '{domain}'"
        );
    }
    Ok(())
}

fn normalize_domain(domain: &str) -> String {
    domain
        .trim()
        .trim_start_matches("www.")
        .trim_start_matches('.')
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

fn domain_matches_any(domain: &str, filters: &BTreeSet<String>) -> bool {
    filters
        .iter()
        .any(|filter| domain == filter || domain.ends_with(&format!(".{filter}")))
}

fn canonicalize_url(url: &str) -> String {
    let mut value = url.trim().to_owned();
    if let Some(fragment) = value.find('#') {
        value.truncate(fragment);
    }
    if let Some(query) = value.find('?') {
        let mut prefix = value[..query].to_owned();
        while prefix.ends_with('/') && prefix.matches('/').count() > 2 {
            prefix.pop();
        }
        let params = value[query + 1..]
            .split('&')
            .filter(|parameter| {
                let key = parameter
                    .split('=')
                    .next()
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                !key.starts_with("utm_")
                    && !matches!(
                        key.as_str(),
                        "gclid" | "fbclid" | "mc_cid" | "mc_eid" | "ref" | "source"
                    )
            })
            .collect::<Vec<_>>();
        value = if params.is_empty() {
            prefix
        } else {
            format!("{prefix}?{}", params.join("&"))
        };
    }
    while value.ends_with('/') && value.matches('/').count() > 2 {
        value.pop();
    }
    value
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(*byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn strip_tags(value: &str) -> String {
    Regex::new(r"(?is)<[^>]+>")
        .map(|regex| regex.replace_all(value, " ").into_owned())
        .unwrap_or_else(|_| value.to_owned())
}

fn xml_tag(value: &str, tag: &str) -> Option<String> {
    Regex::new(&format!(r"(?is)<{tag}[^>]*>(.*?)</{tag}>"))
        .ok()?
        .captures(value)?
        .get(1)
        .map(|capture| capture.as_str().trim().to_owned())
}

fn decode_entities(value: &str) -> String {
    value
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn first_sentence(value: &str, limit: usize) -> String {
    let candidate = value.split(['.', '!', '?']).next().unwrap_or(value).trim();
    truncate_utf8(candidate, limit)
}

fn truncate_utf8(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    value[..end].to_owned()
}

fn is_text_media_type(media_type: &str) -> bool {
    let lower = media_type.to_ascii_lowercase();
    lower.starts_with("text/")
        || lower.contains("json")
        || lower.contains("xml")
        || lower.contains("javascript")
        || lower.contains("xhtml")
}

fn command_exists(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

const RESEARCH_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS search_cache (
    cache_key TEXT PRIMARY KEY,
    query TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS documents (
    canonical_url TEXT PRIMARY KEY,
    url_hash TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    retrieved_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE VIRTUAL TABLE IF NOT EXISTS document_fts USING fts5(
    canonical_url UNINDEXED,
    title,
    body,
    tokenize='unicode61'
);
CREATE TABLE IF NOT EXISTS robots_cache (
    host_key TEXT PRIMARY KEY,
    rules TEXT NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_search_cache_expiry ON search_cache(expires_at);
CREATE INDEX IF NOT EXISTS idx_documents_expiry ON documents(expires_at);
CREATE INDEX IF NOT EXISTS idx_documents_hash ON documents(content_hash);
CREATE INDEX IF NOT EXISTS idx_robots_expiry ON robots_cache(expires_at);
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_network_targets_are_rejected() {
        let url = ParsedUrl::parse("https://127.0.0.1/private").expect("parse");
        assert!(url.resolve_public_ip().is_err());
        assert!(ParsedUrl::parse("http://example.com/plain").is_err());
        assert!(ParsedUrl::parse("http://127.0.0.1:8888/search").is_ok());
    }

    #[test]
    fn canonical_urls_drop_tracking_parameters() {
        assert_eq!(
            canonicalize_url("https://example.com/docs/?utm_source=x&a=1#part"),
            "https://example.com/docs?a=1"
        );
    }

    #[test]
    fn robots_rules_are_respected() {
        let rules = "User-agent: *\nDisallow: /private\nAllow: /private/public\n";
        assert!(!robots_path_allowed(rules, "/private/a"));
        assert!(robots_path_allowed(rules, "/private/public/a"));
    }

    #[test]
    fn primary_sources_receive_a_quality_boost() {
        assert!(source_quality_score("docs.github.com", ResearchMode::Technical) > 0.3);
        assert!(source_quality_score("docs.rs", ResearchMode::Technical) > 0.2);
        assert_eq!(
            source_quality_score("example.com", ResearchMode::General),
            0.0
        );
    }

    #[test]
    fn source_quality_labels_are_stable() {
        assert_eq!(source_quality_label(0.35), "primary");
        assert_eq!(source_quality_label(0.24), "registry");
        assert_eq!(source_quality_label(0.0), "discovered");
    }

    #[test]
    fn html_extraction_removes_script_content() {
        let document = extract_document(
            "<html><head><title>Demo</title><script>ignore()</script></head><body><main>Hello world</main></body></html>",
            "text/html",
        );
        assert_eq!(document.title.as_deref(), Some("Demo"));
        assert!(document.text.contains("Hello world"));
        assert!(!document.text.contains("ignore"));
    }
}
